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
