//! The rows of the table between refreshes: which endpoints are new, which changed their state
//! and which are gone. Closed endpoints stay in the table (marked) for a while, like in TCPView,
//! so a short connection is still visible after it ended.
//!
//! Times are milliseconds since the Unix epoch, passed in by the caller – the logic is pure and
//! tested without waiting.

use crate::net::{ConnKey, Connection};
use crate::process::ProcessInfo;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

/// Current time in ms since the Unix epoch.
pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// How a row is highlighted.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mark {
    None,
    /// Appeared in the last refresh(es).
    New,
    /// TCP state changed (e.g. Established → TimeWait).
    Changed,
    /// No longer reported by the system; removed when the mark expires.
    Closed,
}

#[derive(Clone, Debug)]
pub struct Row {
    pub conn: Connection,
    pub process: Arc<ProcessInfo>,
    /// When MM PortLens first saw the endpoint (not when the system created it).
    pub first_seen: u64,
    pub mark: Mark,
    /// When the mark ends (a closed row is removed then).
    pub mark_until: u64,
    /// Host names from the reverse DNS cache (#5); filled in the view, `None` in the tracker.
    pub local_host: Option<Arc<str>>,
    pub remote_host: Option<Arc<str>>,
    /// Bytes per second sent / received since the previous snapshot (#8).
    pub rate_out: Option<f64>,
    pub rate_in: Option<f64>,
    /// Time of the snapshot that last updated the row (base of the rates).
    pub updated: u64,
}

/// Bytes per second between two counter readings `dt_ms` apart; `None` without both readings or
/// time, 0 when the counter went back (the collection was switched on again).
pub fn rate(old: Option<u64>, new: Option<u64>, dt_ms: u64) -> Option<f64> {
    match (old, new) {
        (Some(a), Some(b)) if dt_ms > 0 => Some(b.saturating_sub(a) as f64 * 1000.0 / dt_ms as f64),
        _ => None,
    }
}

/// What one refresh changed – the base for the status bar and later for recording.
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub struct Changes {
    pub new: usize,
    pub closed: usize,
    pub changed: usize,
}

#[derive(Default)]
pub struct Tracker {
    rows: HashMap<ConnKey, Row>,
    started: bool,
}

impl Tracker {
    /// Merges a new snapshot. `hold_ms` is how long new / changed / closed rows stay marked.
    pub fn update(
        &mut self,
        snapshot: Vec<(Connection, Arc<ProcessInfo>)>,
        now: u64,
        hold_ms: u64,
    ) -> Changes {
        let mut ch = Changes::default();
        // The very first snapshot is the starting point, not a flood of "new" rows.
        let first = !self.started;
        self.started = true;
        let until = now + hold_ms;
        let mut seen = HashSet::with_capacity(snapshot.len());
        for (conn, process) in snapshot {
            let key = conn.key();
            seen.insert(key);
            match self.rows.get_mut(&key) {
                Some(row) => {
                    if row.mark == Mark::Closed {
                        // Came back (same addresses and PID) – treat as a new endpoint.
                        row.mark = Mark::New;
                        row.mark_until = until;
                        row.first_seen = now;
                        ch.new += 1;
                    } else if row.conn.state != conn.state {
                        row.mark = Mark::Changed;
                        row.mark_until = until;
                        ch.changed += 1;
                    }
                    let dt = now.saturating_sub(row.updated);
                    let (old, new) = (row.conn.stats, conn.stats);
                    row.rate_out = rate(old.map(|s| s.bytes_out), new.map(|s| s.bytes_out), dt);
                    row.rate_in = rate(old.map(|s| s.bytes_in), new.map(|s| s.bytes_in), dt);
                    row.updated = now;
                    row.conn = conn;
                    row.process = process;
                }
                None => {
                    let mark = if first { Mark::None } else { Mark::New };
                    if !first {
                        ch.new += 1;
                    }
                    self.rows.insert(
                        key,
                        Row {
                            conn,
                            process,
                            first_seen: now,
                            mark,
                            mark_until: until,
                            local_host: None,
                            remote_host: None,
                            rate_out: None,
                            rate_in: None,
                            updated: now,
                        },
                    );
                }
            }
        }
        for (key, row) in self.rows.iter_mut() {
            if !seen.contains(key) && row.mark != Mark::Closed {
                row.mark = Mark::Closed;
                row.mark_until = until;
                ch.closed += 1;
            }
        }
        self.expire(now);
        ch
    }

    /// Ends expired marks and drops closed rows whose time is up. Returns true when something
    /// changed (the table must be redrawn).
    pub fn expire(&mut self, now: u64) -> bool {
        let before = self.rows.len();
        self.rows
            .retain(|_, r| !(r.mark == Mark::Closed && r.mark_until <= now));
        let mut changed = before != self.rows.len();
        for r in self.rows.values_mut() {
            if matches!(r.mark, Mark::New | Mark::Changed) && r.mark_until <= now {
                r.mark = Mark::None;
                changed = true;
            }
        }
        changed
    }

    /// The earliest time a mark ends – when the UI has to repaint next.
    pub fn next_expiry(&self) -> Option<u64> {
        self.rows
            .values()
            .filter(|r| r.mark != Mark::None)
            .map(|r| r.mark_until)
            .min()
    }

    pub fn rows(&self) -> impl Iterator<Item = &Row> {
        self.rows.values()
    }

    pub fn contains(&self, key: &ConnKey) -> bool {
        self.rows.contains_key(key)
    }

    pub fn len(&self) -> usize {
        self.rows.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::net::{Protocol, TcpState};

    fn conn(port: u16, state: TcpState) -> (Connection, Arc<ProcessInfo>) {
        (
            Connection {
                proto: Protocol::Tcp,
                local: format!("10.0.0.1:{port}").parse().unwrap(),
                remote: Some("10.0.0.2:443".parse().unwrap()),
                state: Some(state),
                pid: 100,
                created: None,
                module: None,
                stats: None,
            },
            Arc::new(ProcessInfo::default()),
        )
    }

    fn mark_of(t: &Tracker, port: u16) -> Option<Mark> {
        t.rows()
            .find(|r| r.conn.local.port() == port)
            .map(|r| r.mark)
    }

    #[test]
    fn first_snapshot_is_not_new() {
        let mut t = Tracker::default();
        let ch = t.update(vec![conn(1, TcpState::Established)], 1000, 2000);
        assert_eq!(ch, Changes::default());
        assert_eq!(mark_of(&t, 1), Some(Mark::None));
    }

    #[test]
    fn new_changed_closed_and_expiry() {
        let mut t = Tracker::default();
        t.update(
            vec![
                conn(1, TcpState::Established),
                conn(2, TcpState::Established),
            ],
            0,
            2000,
        );
        // 1 changes state, 2 disappears, 3 appears.
        let ch = t.update(
            vec![conn(1, TcpState::TimeWait), conn(3, TcpState::SynSent)],
            1000,
            2000,
        );
        assert_eq!(
            ch,
            Changes {
                new: 1,
                closed: 1,
                changed: 1
            }
        );
        assert_eq!(mark_of(&t, 1), Some(Mark::Changed));
        assert_eq!(mark_of(&t, 2), Some(Mark::Closed));
        assert_eq!(mark_of(&t, 3), Some(Mark::New));
        assert_eq!(t.next_expiry(), Some(3000));

        // Still closed, not counted again.
        let ch = t.update(
            vec![conn(1, TcpState::TimeWait), conn(3, TcpState::SynSent)],
            2000,
            2000,
        );
        assert_eq!(ch, Changes::default());
        assert_eq!(t.len(), 3);

        // Marks expire, the closed row goes away.
        assert!(t.expire(3000));
        assert_eq!(t.len(), 2);
        assert_eq!(mark_of(&t, 1), Some(Mark::None));
        assert_eq!(mark_of(&t, 3), Some(Mark::None));
        assert!(!t.expire(4000));
        assert_eq!(t.next_expiry(), None);
    }

    #[test]
    fn rates_between_snapshots() {
        use crate::net::estats::TcpStats;
        let with = |out: u64, inn: u64| {
            let (mut c, p) = conn(1, TcpState::Established);
            c.stats = Some(TcpStats {
                bytes_out: out,
                bytes_in: inn,
                ..Default::default()
            });
            (c, p)
        };
        let mut t = Tracker::default();
        t.update(vec![with(1000, 0)], 0, 2000);
        let r = t.rows().next().unwrap();
        assert_eq!((r.rate_out, r.rate_in), (None, None));
        // 2 s later: 4000 B more sent, 1 MB received.
        t.update(vec![with(5000, 1_000_000)], 2000, 2000);
        let r = t.rows().next().unwrap();
        assert_eq!(r.rate_out, Some(2000.0));
        assert_eq!(r.rate_in, Some(500_000.0));
        // Collection switched on again: the counter starts over, no negative rate.
        t.update(vec![with(10, 1_000_000)], 3000, 2000);
        assert_eq!(t.rows().next().unwrap().rate_out, Some(0.0));
        // No statistics any more.
        t.update(vec![conn(1, TcpState::Established)], 4000, 2000);
        assert_eq!(t.rows().next().unwrap().rate_in, None);
        assert_eq!(rate(Some(1), Some(2), 0), None);
    }

    #[test]
    fn reappearing_endpoint_is_new_again() {
        let mut t = Tracker::default();
        t.update(vec![conn(1, TcpState::Established)], 0, 5000);
        t.update(vec![], 1000, 5000);
        assert_eq!(mark_of(&t, 1), Some(Mark::Closed));
        let ch = t.update(vec![conn(1, TcpState::Established)], 2000, 5000);
        assert_eq!(ch.new, 1);
        assert_eq!(mark_of(&t, 1), Some(Mark::New));
    }
}
