//! Live rates of one process for the graph of the properties window (#30): TCP segments and
//! bytes per second summed over its connections (ESTATS counters of #8) and I/O operations per
//! second (`IoCounters`), kept for the last [`WINDOW_MS`].
//!
//! The counters are cumulative, the graph wants rates: [`Sampler::push`] takes the counters of
//! one poll and stores the difference to the previous poll divided by the elapsed time. TCP
//! differences are taken **per connection** and only when positive, so a closed connection (its
//! counters disappear from the sum) or a connection whose collection has just been switched on
//! (its counters appear with a value collected before) does not make a negative or a huge spike.

use crate::net::ConnKey;
use crate::net::estats::TcpStats;
use crate::process::IoCounters;
use std::collections::{HashMap, VecDeque};

/// How much history the graph keeps (ms).
pub const WINDOW_MS: u64 = 120_000;
/// Upper bound of the kept samples (a very short poll interval).
const MAX_SAMPLES: usize = 2_000;

/// A connection between two polls: the key of the table plus its creation time (a reused
/// address pair is another connection).
pub type ConnId = (ConnKey, Option<u64>);

/// TCP traffic per second.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct TcpRates {
    pub segs_in: f64,
    pub segs_out: f64,
    pub bytes_in: f64,
    pub bytes_out: f64,
}

/// I/O operations per second.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct IoRates {
    pub read: f64,
    pub write: f64,
    pub other: f64,
}

/// The rates between two polls, stamped with the time of the later one (ms since the epoch).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sample {
    pub at: u64,
    pub tcp: TcpRates,
    /// `None` when the counters of the process could not be read (now or the poll before).
    pub io: Option<IoRates>,
}

#[derive(Default)]
pub struct Sampler {
    last_at: Option<u64>,
    conns: HashMap<ConnId, TcpStats>,
    io: Option<IoCounters>,
    samples: VecDeque<Sample>,
}

impl Sampler {
    /// Adds the counters of one poll taken at `at`: the TCP counters of the connections of the
    /// process (those without collected counters are left out) and its I/O counters. The first
    /// poll only sets the baseline.
    pub fn push(
        &mut self,
        at: u64,
        conns: impl IntoIterator<Item = (ConnId, TcpStats)>,
        io: Option<IoCounters>,
    ) {
        let conns: HashMap<ConnId, TcpStats> = conns.into_iter().collect();
        if let Some(last) = self.last_at
            && at > last
        {
            let secs = (at - last) as f64 / 1000.0;
            let mut d = TcpStats::default();
            for (id, now) in &conns {
                // A connection seen for the first time is only the baseline.
                if let Some(before) = self.conns.get(id) {
                    d.segs_in += now.segs_in.saturating_sub(before.segs_in);
                    d.segs_out += now.segs_out.saturating_sub(before.segs_out);
                    d.bytes_in += now.bytes_in.saturating_sub(before.bytes_in);
                    d.bytes_out += now.bytes_out.saturating_sub(before.bytes_out);
                }
            }
            let tcp = TcpRates {
                segs_in: d.segs_in as f64 / secs,
                segs_out: d.segs_out as f64 / secs,
                bytes_in: d.bytes_in as f64 / secs,
                bytes_out: d.bytes_out as f64 / secs,
            };
            let io = match (self.io, io) {
                (Some(b), Some(n)) => Some(IoRates {
                    read: n.read_ops.saturating_sub(b.read_ops) as f64 / secs,
                    write: n.write_ops.saturating_sub(b.write_ops) as f64 / secs,
                    other: n.other_ops.saturating_sub(b.other_ops) as f64 / secs,
                }),
                _ => None,
            };
            self.samples.push_back(Sample { at, tcp, io });
            while self.samples.len() > MAX_SAMPLES
                || self.samples.front().is_some_and(|s| s.at + WINDOW_MS < at)
            {
                self.samples.pop_front();
            }
        } else if self.last_at.is_some_and(|last| at <= last) {
            // The same (or an older) snapshot: nothing to add.
            return;
        }
        self.last_at = Some(at);
        self.conns = conns;
        self.io = io;
    }

    pub fn samples(&self) -> &VecDeque<Sample> {
        &self.samples
    }

    pub fn latest(&self) -> Option<&Sample> {
        self.samples.back()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::net::Protocol;

    fn id(port: u16) -> ConnId {
        let key = ConnKey {
            proto: Protocol::Tcp,
            local: format!("127.0.0.1:{port}").parse().unwrap(),
            remote: Some("10.0.0.1:443".parse().unwrap()),
            pid: 7,
        };
        (key, Some(1))
    }

    fn stats(segs_in: u64, segs_out: u64, bytes_in: u64, bytes_out: u64) -> TcpStats {
        TcpStats {
            bytes_out,
            bytes_in,
            segs_out,
            segs_in,
        }
    }

    fn io(read: u64, write: u64, other: u64) -> Option<IoCounters> {
        Some(IoCounters {
            read_ops: read,
            write_ops: write,
            other_ops: other,
            ..Default::default()
        })
    }

    #[test]
    fn first_poll_is_the_baseline() {
        let mut s = Sampler::default();
        s.push(1000, [(id(1), stats(5, 5, 500, 500))], io(10, 10, 10));
        assert!(s.samples().is_empty());
    }

    #[test]
    fn rates_per_second_summed_over_connections() {
        let mut s = Sampler::default();
        s.push(
            1000,
            [(id(1), stats(10, 4, 1000, 400)), (id(2), stats(0, 0, 0, 0))],
            io(100, 50, 20),
        );
        // Two seconds later.
        s.push(
            3000,
            [
                (id(1), stats(30, 8, 3000, 800)),
                (id(2), stats(10, 2, 1000, 200)),
            ],
            io(140, 60, 20),
        );
        let last = s.latest().unwrap();
        assert_eq!(last.at, 3000);
        assert_eq!(
            last.tcp,
            TcpRates {
                segs_in: 15.0,
                segs_out: 3.0,
                bytes_in: 1500.0,
                bytes_out: 300.0,
            }
        );
        assert_eq!(
            last.io,
            Some(IoRates {
                read: 20.0,
                write: 5.0,
                other: 0.0,
            })
        );
    }

    #[test]
    fn closed_and_new_connections_make_no_spikes() {
        let mut s = Sampler::default();
        s.push(1000, [(id(1), stats(1000, 1000, 9000, 9000))], None);
        // Connection 1 closed, connection 2 appears with counters collected before.
        s.push(2000, [(id(2), stats(500, 500, 5000, 5000))], None);
        assert_eq!(s.latest().unwrap().tcp, TcpRates::default());
        s.push(3000, [(id(2), stats(501, 500, 5100, 5000))], None);
        assert_eq!(s.latest().unwrap().tcp.segs_in, 1.0);
        assert_eq!(s.latest().unwrap().tcp.bytes_in, 100.0);
    }

    #[test]
    fn counter_reset_is_not_negative() {
        let mut s = Sampler::default();
        s.push(1000, [(id(1), stats(50, 50, 50, 50))], io(50, 50, 50));
        s.push(2000, [(id(1), stats(10, 10, 10, 10))], io(10, 10, 10));
        let last = s.latest().unwrap();
        assert_eq!(last.tcp, TcpRates::default());
        assert_eq!(last.io, Some(IoRates::default()));
    }

    #[test]
    fn io_needs_two_readable_polls() {
        let mut s = Sampler::default();
        s.push(1000, [], None);
        s.push(2000, [], io(5, 5, 5));
        assert_eq!(s.latest().unwrap().io, None);
        s.push(3000, [], io(6, 5, 5));
        assert_eq!(s.latest().unwrap().io.unwrap().read, 1.0);
    }

    #[test]
    fn same_snapshot_twice_adds_nothing() {
        let mut s = Sampler::default();
        s.push(1000, [], io(1, 1, 1));
        s.push(2000, [], io(2, 2, 2));
        s.push(2000, [], io(9, 9, 9));
        assert_eq!(s.samples().len(), 1);
        s.push(3000, [], io(3, 2, 2));
        assert_eq!(s.latest().unwrap().io.unwrap().read, 1.0);
    }

    #[test]
    fn history_is_limited_to_the_window() {
        let mut s = Sampler::default();
        for i in 0..=200u64 {
            s.push(i * 1000, [], None);
        }
        let first = s.samples().front().unwrap().at;
        assert_eq!(first, 200_000 - WINDOW_MS);
        assert_eq!(s.samples().len(), 121);
    }
}
