//! Statistics (#19): the history of the connection counts for the chart and the top lists of the
//! statistics window (`app::stats`). Pure functions over the rows, unit-tested.
//!
//! The history is a ring buffer of one [`Sample`] per refresh of the live table, kept for
//! [`HISTORY_MS`]. It is fed from the tracker on every snapshot also while the window is closed,
//! so the chart has data when it opens – one counting pass over the rows per refresh, nothing
//! next to building the table. The top lists are computed only while the window is open.
//!
//! Rows marked closed (still highlighted for a moment after they disappeared) are not counted.

use std::collections::{HashMap, VecDeque};

use crate::columns::ColumnId;
use crate::model::{Mark, Row};
use crate::net::{TcpState, ip_text};

/// How long the chart looks back (ms).
pub const HISTORY_MS: u64 = 10 * 60 * 1000;
/// Upper limit of the samples (10 minutes at the smallest interval of 100 ms are 6000).
const MAX_SAMPLES: usize = 8000;
/// Entries of a top list.
pub const TOP_N: usize = 10;

/// The counts of one refresh.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Sample {
    /// Time of the snapshot (ms since the epoch).
    pub at: u64,
    pub established: u32,
    pub listening: u32,
    pub time_wait: u32,
    /// All endpoints (TCP and UDP).
    pub total: u32,
}

/// Counts the rows of one snapshot.
pub fn sample<'a>(rows: impl IntoIterator<Item = &'a Row>, at: u64) -> Sample {
    let mut s = Sample {
        at,
        ..Default::default()
    };
    for r in rows.into_iter().filter(|r| r.mark != Mark::Closed) {
        s.total += 1;
        match r.conn.state {
            Some(TcpState::Established) => s.established += 1,
            Some(TcpState::Listen) => s.listening += 1,
            Some(TcpState::TimeWait) => s.time_wait += 1,
            _ => {}
        }
    }
    s
}

/// The samples of the last [`HISTORY_MS`].
#[derive(Default)]
pub struct History {
    samples: VecDeque<Sample>,
}

impl History {
    /// Adds a sample and drops the ones older than [`HISTORY_MS`] before it.
    pub fn push(&mut self, s: Sample) {
        self.samples.push_back(s);
        while let Some(first) = self.samples.front() {
            if first.at + HISTORY_MS < s.at || self.samples.len() > MAX_SAMPLES {
                self.samples.pop_front();
            } else {
                break;
            }
        }
    }

    pub fn samples(&self) -> &VecDeque<Sample> {
        &self.samples
    }
}

/// What a top list counts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TopKind {
    /// Processes by the number of their endpoints.
    Processes,
    /// Processes by their traffic (bytes per second sent + received, #8).
    Traffic,
    /// Remote addresses by the number of connections.
    RemoteHosts,
    /// Remote ports by the number of connections.
    RemotePorts,
}

impl TopKind {
    /// The column a click on an entry filters.
    pub fn column(self) -> ColumnId {
        match self {
            TopKind::Processes | TopKind::Traffic => ColumnId::Process,
            TopKind::RemoteHosts => ColumnId::RemoteAddress,
            TopKind::RemotePorts => ColumnId::RemotePort,
        }
    }
}

/// One entry of a top list.
#[derive(Clone, Debug, PartialEq)]
pub struct TopItem {
    /// The value: process name, remote address or remote port.
    pub key: String,
    /// Endpoints with this value.
    pub count: usize,
    /// Bytes per second sent + received; `None` when no endpoint has a rate.
    pub rate: Option<f64>,
}

impl TopItem {
    /// The column filter that shows exactly the endpoints of this entry (`=value`).
    pub fn filter(&self) -> String {
        format!("={}", self.key)
    }
}

/// The value a row is counted under, `None` when it does not count (no remote side, port 0).
fn key_of(r: &Row, kind: TopKind) -> Option<String> {
    match kind {
        TopKind::Processes | TopKind::Traffic => Some(r.process.name.clone()),
        TopKind::RemoteHosts => r
            .conn
            .remote
            .filter(|a| !a.ip().is_unspecified())
            .map(|a| ip_text(a.ip())),
        TopKind::RemotePorts => r
            .conn
            .remote
            .filter(|a| a.port() != 0 && !a.ip().is_unspecified())
            .map(|a| a.port().to_string()),
    }
}

/// The first `n` entries of a top list: by count (traffic: by rate, only entries with traffic),
/// ties by the value.
pub fn top<'a>(rows: impl IntoIterator<Item = &'a Row>, kind: TopKind, n: usize) -> Vec<TopItem> {
    let mut map: HashMap<String, TopItem> = HashMap::new();
    for r in rows.into_iter().filter(|r| r.mark != Mark::Closed) {
        let Some(key) = key_of(r, kind) else {
            continue;
        };
        let rate = match (r.rate_in, r.rate_out) {
            (None, None) => None,
            (i, o) => Some(i.unwrap_or(0.0) + o.unwrap_or(0.0)),
        };
        let e = map.entry(key.clone()).or_insert(TopItem {
            key,
            count: 0,
            rate: None,
        });
        e.count += 1;
        if let Some(v) = rate {
            e.rate = Some(e.rate.unwrap_or(0.0) + v);
        }
    }
    let mut list: Vec<TopItem> = map.into_values().collect();
    if kind == TopKind::Traffic {
        list.retain(|e| e.rate.is_some_and(|r| r > 0.0));
        list.sort_by(|a, b| {
            b.rate
                .unwrap_or(0.0)
                .total_cmp(&a.rate.unwrap_or(0.0))
                .then_with(|| a.key.to_lowercase().cmp(&b.key.to_lowercase()))
        });
    } else if kind == TopKind::RemotePorts {
        list.sort_by(|a, b| {
            b.count.cmp(&a.count).then_with(|| {
                let port = |e: &TopItem| e.key.parse::<u16>().unwrap_or(0);
                port(a).cmp(&port(b))
            })
        });
    } else {
        list.sort_by(|a, b| {
            b.count
                .cmp(&a.count)
                .then_with(|| a.key.to_lowercase().cmp(&b.key.to_lowercase()))
        });
    }
    list.truncate(n);
    list
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::colfilter::ColumnFilter;
    use crate::net::{Connection, Protocol};
    use crate::process::ProcessInfo;
    use std::sync::Arc;

    fn row(name: &str, remote: Option<&str>, state: Option<TcpState>) -> Row {
        Row {
            conn: Connection {
                proto: if state.is_some() {
                    Protocol::Tcp
                } else {
                    Protocol::Udp
                },
                local: "10.0.0.5:50000".parse().unwrap(),
                remote: remote.map(|r| r.parse().unwrap()),
                state,
                pid: 1,
                created: None,
                module: None,
                stats: None,
            },
            process: Arc::new(ProcessInfo {
                name: name.into(),
                ..Default::default()
            }),
            first_seen: 0,
            mark: Mark::None,
            mark_until: 0,
            local_host: None,
            remote_host: None,
            rate_out: None,
            rate_in: None,
            updated: 0,
            etw: false,
            as_of: None,
        }
    }

    fn rows() -> Vec<Row> {
        use TcpState::*;
        let mut closed = row("gone.exe", Some("1.2.3.4:443"), Some(Established));
        closed.mark = Mark::Closed;
        let mut fast = row("chrome.exe", Some("140.82.112.3:443"), Some(Established));
        fast.rate_in = Some(1000.0);
        fast.rate_out = Some(24.0);
        vec![
            fast,
            row("chrome.exe", Some("140.82.112.3:443"), Some(TimeWait)),
            row("chrome.exe", Some("8.8.8.8:80"), Some(Established)),
            row("svchost.exe", Some("0.0.0.0:0"), Some(Listen)),
            row("svchost.exe", None, None),
            row("Code.exe", Some("8.8.8.8:443"), Some(Established)),
            closed,
        ]
    }

    #[test]
    fn sample_counts_states_without_closed_rows() {
        let s = sample(&rows(), 7);
        assert_eq!(
            s,
            Sample {
                at: 7,
                established: 3,
                listening: 1,
                time_wait: 1,
                total: 6,
            }
        );
    }

    #[test]
    fn history_keeps_ten_minutes() {
        let mut h = History::default();
        for i in 0..=20 {
            h.push(Sample {
                at: i * 60_000,
                ..Default::default()
            });
        }
        // 10:00 … 20:00 – the sample exactly HISTORY_MS old stays.
        assert_eq!(h.samples().len(), 11);
        assert_eq!(h.samples().front().unwrap().at, 10 * 60_000);
        assert_eq!(h.samples().back().unwrap().at, 20 * 60_000);
        let mut many = History::default();
        for i in 0..(MAX_SAMPLES as u64 + 50) {
            many.push(Sample {
                at: i,
                ..Default::default()
            });
        }
        assert_eq!(many.samples().len(), MAX_SAMPLES);
    }

    #[test]
    fn top_lists() {
        let rows = rows();
        let p = top(&rows, TopKind::Processes, TOP_N);
        let keys: Vec<(&str, usize)> = p.iter().map(|e| (e.key.as_str(), e.count)).collect();
        assert_eq!(
            keys,
            vec![("chrome.exe", 3), ("svchost.exe", 2), ("Code.exe", 1)]
        );
        assert_eq!(p[0].rate, Some(1024.0));
        assert_eq!(p[1].rate, None);
        // Traffic: only processes with a rate.
        let t = top(&rows, TopKind::Traffic, TOP_N);
        assert_eq!(t.len(), 1);
        assert_eq!(t[0].key, "chrome.exe");
        // Hosts and ports: no listener 0.0.0.0, no UDP without remote, no closed row.
        let h = top(&rows, TopKind::RemoteHosts, TOP_N);
        let keys: Vec<(&str, usize)> = h.iter().map(|e| (e.key.as_str(), e.count)).collect();
        assert_eq!(keys, vec![("140.82.112.3", 2), ("8.8.8.8", 2)]);
        let ports = top(&rows, TopKind::RemotePorts, TOP_N);
        let keys: Vec<(&str, usize)> = ports.iter().map(|e| (e.key.as_str(), e.count)).collect();
        assert_eq!(keys, vec![("443", 3), ("80", 1)]);
        // Cut to n.
        assert_eq!(top(&rows, TopKind::Processes, 1).len(), 1);
    }

    #[test]
    fn filters_of_entries_match_exactly_their_rows() {
        let rows = rows();
        for kind in [
            TopKind::Processes,
            TopKind::RemoteHosts,
            TopKind::RemotePorts,
        ] {
            let col = kind.column();
            for e in top(&rows, kind, TOP_N) {
                let f = ColumnFilter::parse(&e.filter(), col.filter_kind())
                    .unwrap()
                    .unwrap();
                let matched = rows
                    .iter()
                    .filter(|r| r.mark != Mark::Closed)
                    .filter(|r| f.matches(&crate::columns::cell_text(r, col)))
                    .count();
                assert_eq!(matched, e.count, "{kind:?} {}", e.key);
            }
        }
        // `=80` must not match port 8080, `=chrome.exe` not "chrome.exe2".
        let f = ColumnFilter::parse("=80", ColumnId::RemotePort.filter_kind())
            .unwrap()
            .unwrap();
        assert!(f.matches("80") && !f.matches("8080"));
    }
}
