//! The rows of the table between refreshes: which endpoints are new, which changed their state
//! and which are gone. Closed endpoints stay in the table (marked) for a while, like in TCPView,
//! so a short connection is still visible after it ended.
//!
//! Times are milliseconds since the Unix epoch, passed in by the caller – the logic is pure and
//! tested without waiting.

use crate::net::etw::Seen;
use crate::net::{ConnKey, Connection, Protocol, TcpState};
use crate::process::ProcessInfo;
use crate::recording::{Event, EventKind};
use std::collections::{HashMap, HashSet};
use std::net::IpAddr;
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
    /// Seen only by ETW, never by a snapshot: a connection shorter than a refresh (#4).
    pub etw: bool,
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
    /// Snapshots merged so far (ETW rows only from the second one on, #4).
    snapshots: u64,
}

/// An endpoint as ETW and the tables can both identify it (#4): TCP by protocol and both
/// addresses without the scope and the process (the kernel reports some events with PID 0),
/// UDP by the local port only (a socket bound to `0.0.0.0` / `[::]` sends from a concrete
/// address, a dual-stack socket also over IPv4).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
enum Loose {
    Tcp(IpAddr, u16, IpAddr, u16),
    Udp(u16),
}

fn loose(
    proto: Protocol,
    local: std::net::SocketAddr,
    remote: Option<std::net::SocketAddr>,
) -> Loose {
    match (proto.is_tcp(), remote) {
        (true, Some(r)) => Loose::Tcp(
            local.ip().to_canonical(),
            local.port(),
            r.ip().to_canonical(),
            r.port(),
        ),
        _ => Loose::Udp(local.port()),
    }
}

/// What the tracker knows about the rows of one [`Loose`] endpoint.
#[derive(Clone, Default, Debug)]
struct Known {
    /// Local address and PID of the live (not closed) rows; UDP: `None` = bound to any address.
    live: Vec<(Option<IpAddr>, u32)>,
    /// When the latest of its closed rows was closed.
    closed_at: Option<u64>,
}

impl Known {
    /// A live row stands for what ETW saw of `s`. A TCP connection the local side closed first
    /// stays in the table in TimeWait with PID 0: when ETW saw it opened by a known process,
    /// only a live row with a PID counts, so the ETW row shows who made the connection.
    fn covers(&self, s: &Seen) -> bool {
        let ip = s.key.local.ip().to_canonical();
        let udp = !s.key.proto.is_tcp();
        let mut live = self
            .live
            .iter()
            .filter(|(a, _)| !udp || a.is_none_or(|a| a == ip));
        if !udp && s.opened.is_some() && s.key.pid != 0 {
            live.any(|&(_, pid)| pid != 0)
        } else {
            live.next().is_some()
        }
    }
}

impl Tracker {
    /// Merges a new snapshot. `hold_ms` is how long new / changed / closed rows stay marked.
    #[cfg(test)]
    pub fn update(
        &mut self,
        snapshot: Vec<(Connection, Arc<ProcessInfo>)>,
        now: u64,
        hold_ms: u64,
    ) -> Changes {
        self.update_with_events(snapshot, now, hold_ms, None)
    }

    /// [`Tracker::update`] that also appends what changed to `events` (a running recording,
    /// #3): one event per counted change, in the same order. The first snapshot produces none.
    pub fn update_with_events(
        &mut self,
        snapshot: Vec<(Connection, Arc<ProcessInfo>)>,
        now: u64,
        hold_ms: u64,
        mut events: Option<&mut Vec<Event>>,
    ) -> Changes {
        let mut emit = |kind: EventKind, row: &Row, before: Option<crate::net::TcpState>| {
            if let Some(ev) = events.as_deref_mut() {
                let mut conn = row.conn.clone();
                conn.stats = None;
                if kind == EventKind::Closed {
                    conn.state = None;
                }
                ev.push(Event {
                    time: now,
                    kind,
                    conn,
                    state_before: before,
                    process: row.process.clone(),
                    etw: false,
                });
            }
        };
        let mut ch = Changes::default();
        // The very first snapshot is the starting point, not a flood of "new" rows.
        let first = !self.started;
        self.started = true;
        self.snapshots += 1;
        let until = now + hold_ms;
        let mut seen = HashSet::with_capacity(snapshot.len());
        for (conn, process) in snapshot {
            let key = conn.key();
            seen.insert(key);
            match self.rows.get_mut(&key) {
                Some(row) => {
                    let kind = if row.mark == Mark::Closed {
                        Some(EventKind::New)
                    } else if row.conn.state != conn.state {
                        Some(EventKind::Changed)
                    } else {
                        None
                    };
                    let before = row.conn.state;
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
                    row.etw = false;
                    match kind {
                        Some(EventKind::Changed) => emit(EventKind::Changed, row, before),
                        Some(k) => emit(k, row, None),
                        None => {}
                    }
                }
                None => {
                    let mark = if first { Mark::None } else { Mark::New };
                    if !first {
                        ch.new += 1;
                    }
                    let row = self.rows.entry(key).insert_entry(Row {
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
                        etw: false,
                    });
                    if !first {
                        emit(EventKind::New, row.get(), None);
                    }
                }
            }
        }
        for (key, row) in self.rows.iter_mut() {
            if !seen.contains(key) && row.mark != Mark::Closed {
                row.mark = Mark::Closed;
                row.mark_until = until;
                ch.closed += 1;
                emit(EventKind::Closed, row, row.conn.state);
            }
        }
        self.expire(now);
        ch
    }

    /// Adds the connections ETW saw but the snapshot just merged did not (#4) – opened and
    /// closed between two refreshes. They become closed rows marked [`Row::etw`], and with
    /// `events` a new + closed event at the times ETW saw them. An endpoint the tables know is
    /// skipped: a live row, or a closed one unless ETW saw it opened again after it was closed
    /// (TCP: a connect / accept; UDP: any datagram). Call it right after
    /// [`Tracker::update_with_events`] with the same snapshot time; returns the rows added.
    pub fn merge_etw(
        &mut self,
        seen: Vec<(Seen, Arc<ProcessInfo>)>,
        now: u64,
        hold_ms: u64,
        mut events: Option<&mut Vec<Event>>,
    ) -> usize {
        // The first snapshot is the starting point; ETW saw nothing before it.
        if self.snapshots < 2 || seen.is_empty() {
            return 0;
        }
        let mut known: HashMap<Loose, Known> = HashMap::new();
        for r in self.rows.values() {
            let k = known
                .entry(loose(r.conn.proto, r.conn.local, r.conn.remote))
                .or_default();
            if r.mark == Mark::Closed {
                let at = r.mark_until.saturating_sub(hold_ms);
                k.closed_at = Some(k.closed_at.map_or(at, |t| t.max(at)));
            } else {
                let ip = r.conn.local.ip().to_canonical();
                k.live
                    .push(((!ip.is_unspecified()).then_some(ip), r.conn.pid));
            }
        }
        let mut added = 0;
        for (s, process) in seen {
            let udp = !s.key.proto.is_tcp();
            let lk = loose(s.key.proto, s.key.local, s.key.remote);
            let k = known.get(&lk);
            let new = match k {
                None => true,
                Some(k) if k.covers(&s) => false,
                Some(k) => {
                    // Closed: only a later opening is a new connection.
                    let opened = if udp { Some(s.first) } else { s.opened };
                    match (k.closed_at, opened) {
                        (Some(closed), Some(t)) => t > closed,
                        (None, _) => true,
                        _ => false,
                    }
                }
            };
            if !new {
                continue;
            }
            let state = (!udp).then_some(TcpState::Established);
            let opened = s.opened.unwrap_or(s.first);
            let conn = Connection {
                proto: s.key.proto,
                local: s.key.local,
                remote: s.key.remote,
                state,
                pid: s.key.pid,
                created: s.opened,
                module: None,
                stats: Some(crate::net::estats::TcpStats {
                    bytes_out: s.bytes_out,
                    bytes_in: s.bytes_in,
                    segs_out: s.sends,
                    segs_in: s.recvs,
                }),
            };
            if let Some(ev) = events.as_deref_mut() {
                let mut c = conn.clone();
                c.stats = None;
                ev.push(Event {
                    time: opened,
                    kind: EventKind::New,
                    conn: c.clone(),
                    state_before: None,
                    process: process.clone(),
                    etw: true,
                });
                c.state = None;
                ev.push(Event {
                    time: s.closed.unwrap_or(now).max(opened),
                    kind: EventKind::Closed,
                    conn: c,
                    state_before: state,
                    process: process.clone(),
                    etw: true,
                });
            }
            self.rows.insert(
                conn.key(),
                Row {
                    conn,
                    process,
                    first_seen: opened,
                    mark: Mark::Closed,
                    mark_until: now + hold_ms,
                    local_host: None,
                    remote_host: None,
                    rate_out: None,
                    rate_in: None,
                    updated: s.last,
                    etw: true,
                },
            );
            // Further events of the same endpoint in this batch (another PID) are not new.
            let k = known.entry(lk).or_default();
            k.closed_at = Some(k.closed_at.map_or(now, |t| t.max(now)));
            added += 1;
        }
        added
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

    /// The starting state of a recording (#3): a `Present` event per endpoint that is not closed.
    pub fn present_events(&self, now: u64) -> Vec<Event> {
        self.rows
            .values()
            .filter(|r| r.mark != Mark::Closed)
            .map(|r| {
                let mut conn = r.conn.clone();
                conn.stats = None;
                Event {
                    time: now,
                    kind: EventKind::Present,
                    conn,
                    state_before: None,
                    process: r.process.clone(),
                    etw: false,
                }
            })
            .collect()
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
    fn events_follow_the_changes() {
        let mut t = Tracker::default();
        let mut ev = Vec::new();
        // The first snapshot is the starting point: no events.
        let ch = t.update_with_events(
            vec![
                conn(1, TcpState::Established),
                conn(2, TcpState::Established),
            ],
            0,
            2000,
            Some(&mut ev),
        );
        assert_eq!(ch, Changes::default());
        assert!(ev.is_empty());
        let present = t.present_events(500);
        assert_eq!(present.len(), 2);
        assert!(
            present
                .iter()
                .all(|e| e.kind == EventKind::Present && e.time == 500)
        );

        t.update_with_events(
            vec![conn(1, TcpState::TimeWait), conn(3, TcpState::SynSent)],
            1000,
            2000,
            Some(&mut ev),
        );
        let find = |k: EventKind| ev.iter().find(|e| e.kind == k).unwrap();
        assert_eq!(ev.len(), 3);
        let changed = find(EventKind::Changed);
        assert_eq!(changed.conn.local.port(), 1);
        assert_eq!(changed.state_before, Some(TcpState::Established));
        assert_eq!(changed.conn.state, Some(TcpState::TimeWait));
        let new = find(EventKind::New);
        assert_eq!(new.conn.local.port(), 3);
        assert_eq!(
            (new.state_before, new.conn.state),
            (None, Some(TcpState::SynSent))
        );
        let closed = find(EventKind::Closed);
        assert_eq!(closed.conn.local.port(), 2);
        assert_eq!(closed.state_before, Some(TcpState::Established));
        assert_eq!(closed.conn.state, None);
        assert_eq!(closed.shown_state(), Some(TcpState::Established));
        assert!(ev.iter().all(|e| e.time == 1000));

        // Nothing changes: no events; the closed row is not closed again. A closed row that
        // comes back is new; closed rows are not part of the starting state.
        ev.clear();
        t.update_with_events(
            vec![conn(1, TcpState::TimeWait), conn(3, TcpState::SynSent)],
            1500,
            2000,
            Some(&mut ev),
        );
        assert!(ev.is_empty());
        assert_eq!(t.present_events(1500).len(), 2);
        t.update_with_events(
            vec![
                conn(1, TcpState::TimeWait),
                conn(2, TcpState::Established),
                conn(3, TcpState::SynSent),
            ],
            1800,
            2000,
            Some(&mut ev),
        );
        assert_eq!(ev.len(), 1);
        assert_eq!((ev[0].kind, ev[0].conn.local.port()), (EventKind::New, 2));
    }

    fn seen(proto: Protocol, local: &str, remote: &str, pid: u32) -> Seen {
        Seen {
            key: ConnKey {
                proto,
                local: local.parse().unwrap(),
                remote: Some(remote.parse().unwrap()),
                pid,
            },
            first: 1200,
            last: 1300,
            opened: proto.is_tcp().then_some(1200),
            closed: proto.is_tcp().then_some(1300),
            bytes_out: 100,
            bytes_in: 2000,
            sends: 1,
            recvs: 2,
        }
    }

    fn p() -> Arc<ProcessInfo> {
        Arc::new(ProcessInfo::default())
    }

    #[test]
    fn etw_adds_only_unknown_connections() {
        let udp = |port: u16, local: &str| {
            (
                Connection {
                    proto: Protocol::Udp,
                    local: format!("{local}:{port}").parse().unwrap(),
                    remote: None,
                    state: None,
                    pid: 200,
                    created: None,
                    module: None,
                    stats: None,
                },
                p(),
            )
        };
        let time_wait = || {
            let (mut c, cp) = conn(8, TcpState::TimeWait);
            c.remote = Some("10.0.0.2:80".parse().unwrap());
            c.pid = 0;
            (c, cp)
        };
        let mut t = Tracker::default();
        // Nothing before the second snapshot.
        t.update(vec![conn(1, TcpState::Established)], 0, 3000);
        assert_eq!(
            t.merge_etw(
                vec![(seen(Protocol::Tcp, "10.0.0.1:9", "10.0.0.2:80", 5), p())],
                0,
                3000,
                None
            ),
            0
        );
        let mut ev = Vec::new();
        let ch = t.update_with_events(
            vec![
                conn(1, TcpState::Established),
                udp(53, "0.0.0.0"),
                time_wait(),
            ],
            2000,
            3000,
            Some(&mut ev),
        );
        assert_eq!(ch.new, 2);
        ev.clear();
        let batch = vec![
            // Known: the live TCP row (ETW reports PID 0) and the UDP socket on any address.
            (seen(Protocol::Tcp, "10.0.0.1:1", "10.0.0.2:443", 0), p()),
            (
                seen(Protocol::Udp, "192.168.1.5:53", "8.8.8.8:53", 200),
                p(),
            ),
            // Unknown: a short TCP connection and a short UDP socket.
            (seen(Protocol::Tcp, "10.0.0.1:7", "10.0.0.2:80", 300), p()),
            (
                seen(Protocol::Udp6, "[fe80::1]:5353", "[ff02::fb]:5353", 300),
                p(),
            ),
            // The same TCP connection again with another PID: not a second row.
            (seen(Protocol::Tcp, "10.0.0.1:7", "10.0.0.2:80", 0), p()),
            // Its TimeWait row (PID 0) does not tell the process: a row from ETW.
            (seen(Protocol::Tcp, "10.0.0.1:8", "10.0.0.2:80", 400), p()),
        ];
        assert_eq!(t.merge_etw(batch, 2000, 3000, Some(&mut ev)), 3);
        assert!(t.rows().any(|r| r.etw && r.conn.pid == 400));
        let row = t.rows().find(|r| r.conn.local.port() == 7).unwrap();
        assert!(row.etw);
        assert_eq!(row.mark, Mark::Closed);
        assert_eq!((row.first_seen, row.conn.created), (1200, Some(1200)));
        assert_eq!(row.conn.state, Some(TcpState::Established));
        assert_eq!(row.conn.stats.unwrap().bytes_in, 2000);
        assert!(t.rows().any(|r| r.etw && r.conn.proto == Protocol::Udp6));
        assert_eq!(ev.len(), 6);
        assert!(ev.iter().all(|e| e.etw && e.conn.stats.is_none()));
        let tcp: Vec<&Event> = ev.iter().filter(|e| e.conn.local.port() == 7).collect();
        assert_eq!((tcp[0].kind, tcp[0].time), (EventKind::New, 1200));
        assert_eq!((tcp[1].kind, tcp[1].time), (EventKind::Closed, 1300));
        assert_eq!(tcp[1].state_before, Some(TcpState::Established));
        // The UDP socket closed by the tables has no disconnect: closed at the snapshot.
        let u: Vec<&Event> = ev.iter().filter(|e| e.conn.local.port() == 5353).collect();
        assert_eq!(
            (u[1].kind, u[1].time, u[1].state_before),
            (EventKind::Closed, 2000, None)
        );

        // Port 1 closes (seen closed by the tables at 3000); its late disconnect event is not
        // a new connection, a reconnect after the close is.
        t.update(vec![udp(53, "0.0.0.0")], 3000, 3000);
        let mut late = seen(Protocol::Tcp, "10.0.0.1:1", "10.0.0.2:443", 100);
        late.opened = None;
        assert_eq!(t.merge_etw(vec![(late.clone(), p())], 3000, 3000, None), 0);
        late.opened = Some(3100);
        t.update(vec![udp(53, "0.0.0.0")], 4000, 3000);
        assert_eq!(t.merge_etw(vec![(late, p())], 4000, 3000, None), 1);
        // A snapshot that sees an ETW row makes it a normal row again.
        let (mut c, cp) = conn(7, TcpState::Established);
        c.remote = Some("10.0.0.2:80".parse().unwrap());
        c.pid = 300;
        t.update(vec![(c, cp), udp(53, "0.0.0.0")], 5000, 3000);
        let row = t.rows().find(|r| r.conn.local.port() == 7).unwrap();
        assert!(!row.etw);
        assert_eq!(row.mark, Mark::New);
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
