//! The recording database (#3) – every SQL statement of MM PortLens is in this file.
//!
//! A `.plrec` file is a SQLite database:
//!
//! * `meta(key, value)` – computer, start / end, refresh interval, app version, recorded event
//!   kinds, the filter used; the schema version is `PRAGMA user_version`.
//! * `processes(id, pid, name, path, create_time)` – every process once; events refer to it.
//! * `events(id, time_ms, kind, proto, local_addr, local_port, remote_addr, remote_port,
//!   state_before, state_after, created, process_id, conn_id)` with an index on `time_ms`;
//!   `conn_id` numbers the endpoints (same protocol, addresses and process) and the index
//!   `events_conn(conn_id, time_ms, kind)` lets the replay find the last event of every
//!   endpoint without sorting (32 ms instead of 136 ms at the end of a busy hour). Kinds,
//!   protocols and states are small integers; the addresses are text, so the file stays readable
//!   in DB Browser for SQLite (the view `events_readable` spells everything out).
//!
//! The writer uses a WAL journal and one transaction per refresh (a crash loses at most the
//! last refresh); [`RecordingWriter::finish`] checkpoints and switches back to a rollback
//! journal, so a finished recording is one file.

use super::{Event, EventKind, EventKinds};
use crate::model::{Mark, Row};
use crate::net::{Connection, Protocol, TcpState};
use crate::process::ProcessInfo;
use rusqlite::{Connection as Db, OpenFlags, OptionalExtension, params};
use std::collections::HashMap;
use std::fmt;
use std::net::{IpAddr, SocketAddr};
use std::path::Path;
use std::sync::Arc;

/// Version of the schema in `PRAGMA user_version`; a newer file is refused.
pub const SCHEMA_VERSION: i64 = 1;
/// `meta.format` of every recording (tells a recording from another SQLite file).
const FORMAT: &str = "mmportlens-recording";

const SCHEMA: &str = "
CREATE TABLE meta(key TEXT PRIMARY KEY, value TEXT NOT NULL);
CREATE TABLE processes(
    id INTEGER PRIMARY KEY,
    pid INTEGER NOT NULL,
    name TEXT NOT NULL,
    path TEXT NOT NULL,
    create_time INTEGER
);
CREATE TABLE events(
    id INTEGER PRIMARY KEY,
    time_ms INTEGER NOT NULL,
    kind INTEGER NOT NULL,
    proto INTEGER NOT NULL,
    local_addr TEXT NOT NULL,
    local_port INTEGER NOT NULL,
    remote_addr TEXT,
    remote_port INTEGER,
    state_before INTEGER,
    state_after INTEGER,
    created INTEGER,
    process_id INTEGER NOT NULL REFERENCES processes(id),
    conn_id INTEGER NOT NULL
);
CREATE INDEX events_time ON events(time_ms);
CREATE INDEX events_conn ON events(conn_id, time_ms, kind);
CREATE VIEW events_readable AS
SELECT e.id,
       datetime(e.time_ms / 1000, 'unixepoch', 'localtime') AS time,
       e.time_ms % 1000 AS ms,
       CASE e.kind WHEN 0 THEN 'present' WHEN 1 THEN 'new' WHEN 2 THEN 'closed'
                   WHEN 3 THEN 'changed' END AS event,
       CASE e.proto WHEN 1 THEN 'TCP' WHEN 2 THEN 'TCPv6' WHEN 3 THEN 'UDP'
                    WHEN 4 THEN 'UDPv6' END AS protocol,
       e.local_addr, e.local_port, e.remote_addr, e.remote_port,
       e.state_before, e.state_after,
       p.pid, p.name AS process, p.path
FROM events e JOIN processes p ON p.id = e.process_id;
";

/// An error of a recording file.
#[derive(Debug)]
pub enum RecError {
    Sql(rusqlite::Error),
    Io(std::io::Error),
    /// The file is a SQLite database, but not a recording.
    NotRecording,
    /// Written by a newer MM PortLens (schema version).
    Newer(i64),
}

impl fmt::Display for RecError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RecError::Sql(e) => write!(f, "{e}"),
            RecError::Io(e) => write!(f, "{e}"),
            RecError::NotRecording => f.write_str("not an MM PortLens recording"),
            RecError::Newer(v) => write!(
                f,
                "recording format {v} is newer than this version understands ({SCHEMA_VERSION})"
            ),
        }
    }
}

impl From<rusqlite::Error> for RecError {
    fn from(e: rusqlite::Error) -> Self {
        RecError::Sql(e)
    }
}

pub type Result<T> = std::result::Result<T, RecError>;

fn proto_code(p: Protocol) -> i64 {
    match p {
        Protocol::Tcp => 1,
        Protocol::Tcp6 => 2,
        Protocol::Udp => 3,
        Protocol::Udp6 => 4,
    }
}

fn proto_from(v: i64) -> Protocol {
    match v {
        2 => Protocol::Tcp6,
        3 => Protocol::Udp,
        4 => Protocol::Udp6,
        _ => Protocol::Tcp,
    }
}

fn state_code(s: Option<TcpState>) -> Option<i64> {
    s.map(|s| s.to_mib() as i64)
}

fn state_from(v: Option<i64>) -> Option<TcpState> {
    v.map(|v| TcpState::from_mib(v as u32))
}

fn ms(v: u64) -> i64 {
    v.min(i64::MAX as u64) as i64
}

/// What identifies a process in the `processes` table.
#[derive(Clone, PartialEq, Eq, Hash)]
struct ProcKey {
    pid: u32,
    started: Option<u64>,
    name: String,
    path: String,
}

/// Writes one recording. Lives in the writer thread ([`super::threads::Recorder`]).
pub struct RecordingWriter {
    db: Db,
    processes: HashMap<ProcKey, i64>,
    /// `conn_id` of every endpoint written so far (endpoint key + process id).
    conns: HashMap<(crate::net::ConnKey, i64), i64>,
    events: u64,
}

impl RecordingWriter {
    /// Creates the recording file; an existing file (and its journal) is replaced.
    pub fn create(path: &Path) -> Result<Self> {
        for ext in ["-wal", "-shm", "-journal"] {
            let mut p = path.as_os_str().to_owned();
            p.push(ext);
            let _ = std::fs::remove_file(p);
        }
        match std::fs::remove_file(path) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(RecError::Io(e)),
            _ => {}
        }
        Self::init(Db::open(path)?)
    }

    /// A recording in memory (tests).
    #[cfg(test)]
    pub fn in_memory() -> Result<Self> {
        Self::init(Db::open_in_memory()?)
    }

    fn init(db: Db) -> Result<Self> {
        // WAL: a refresh is one small append; NORMAL sync is safe with WAL (a power cut may
        // lose the last transactions, never corrupt the file).
        let _: String = db.query_row("PRAGMA journal_mode = WAL", [], |r| r.get(0))?;
        db.pragma_update(None, "synchronous", "NORMAL")?;
        db.execute_batch(SCHEMA)?;
        db.pragma_update(None, "user_version", SCHEMA_VERSION)?;
        let mut w = Self {
            db,
            processes: HashMap::new(),
            conns: HashMap::new(),
            events: 0,
        };
        w.set_meta("format", FORMAT)?;
        Ok(w)
    }

    pub fn set_meta(&mut self, key: &str, value: &str) -> Result<()> {
        self.db.execute(
            "INSERT INTO meta(key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )?;
        Ok(())
    }

    /// Writes the events of one refresh in one transaction.
    pub fn write(&mut self, events: &[Event]) -> Result<()> {
        if events.is_empty() {
            return Ok(());
        }
        let tx = self.db.transaction()?;
        {
            let mut add_proc = tx.prepare_cached(
                "INSERT INTO processes(pid, name, path, create_time) VALUES (?1, ?2, ?3, ?4)",
            )?;
            let mut add = tx.prepare_cached(
                "INSERT INTO events(time_ms, kind, proto, local_addr, local_port, remote_addr,
                     remote_port, state_before, state_after, created, process_id, conn_id)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
            )?;
            for e in events {
                let p = &e.process;
                let key = ProcKey {
                    pid: e.conn.pid,
                    started: p.started,
                    name: p.name.clone(),
                    path: p.path.clone(),
                };
                let pid = match self.processes.get(&key) {
                    Some(id) => *id,
                    None => {
                        add_proc.execute(params![
                            key.pid,
                            key.name,
                            key.path,
                            key.started.map(ms)
                        ])?;
                        let id = tx.last_insert_rowid();
                        self.processes.insert(key, id);
                        id
                    }
                };
                let c = &e.conn;
                let next = self.conns.len() as i64 + 1;
                let conn_id = *self.conns.entry((c.key(), pid)).or_insert(next);
                add.execute(params![
                    ms(e.time),
                    e.kind.code(),
                    proto_code(c.proto),
                    c.local.ip().to_string(),
                    c.local.port(),
                    c.remote.map(|r| r.ip().to_string()),
                    c.remote.map(|r| r.port()),
                    state_code(e.state_before),
                    state_code(c.state),
                    c.created.map(ms),
                    pid,
                    conn_id,
                ])?;
            }
        }
        tx.commit()?;
        self.events += events.len() as u64;
        Ok(())
    }

    /// Events written so far.
    pub fn events(&self) -> u64 {
        self.events
    }

    /// Ends the recording: the end time, then everything moved from the WAL into the file and
    /// the journal switched back, so the recording is a single file.
    pub fn finish(mut self, end: u64) -> Result<u64> {
        self.set_meta("end_ms", &end.to_string())?;
        let _: (i64, i64, i64) = self
            .db
            .query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?))
            })?;
        let _: String = self
            .db
            .query_row("PRAGMA journal_mode = DELETE", [], |r| r.get(0))?;
        Ok(self.events)
    }

    #[cfg(test)]
    fn into_reader(self) -> RecordingReader {
        RecordingReader {
            db: self.db,
            processes: HashMap::new(),
        }
    }
}

/// What the replay shows about a recording.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Info {
    /// The `meta` table (without `format`).
    pub meta: Vec<(String, String)>,
    pub events: u64,
    /// Time of the first / last event (the range of the time slider).
    pub first: Option<u64>,
    pub last: Option<u64>,
}

impl Info {
    pub fn meta(&self, key: &str) -> Option<&str> {
        self.meta
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    }
}

/// Which events the event list shows.
#[derive(Clone, Debug, PartialEq)]
pub struct EventQuery {
    /// Contained in the process name, an address, a port or the PID (case-insensitive).
    pub text: String,
    pub present: bool,
    pub kinds: EventKinds,
    pub newest_first: bool,
}

impl Default for EventQuery {
    fn default() -> Self {
        Self {
            text: String::new(),
            present: true,
            kinds: EventKinds::default(),
            newest_first: false,
        }
    }
}

impl EventQuery {
    /// The `WHERE` clause and its text parameter (`LIKE` pattern, `%` / `_` escaped).
    fn clause(&self) -> (String, String) {
        let kinds: Vec<String> = EventKind::ALL
            .into_iter()
            .filter(|&k| {
                if k == EventKind::Present {
                    self.present
                } else {
                    self.kinds.wants(k)
                }
            })
            .map(|k| k.code().to_string())
            .collect();
        let mut sql = format!("e.kind IN ({})", kinds.join(","));
        let text = self.text.trim();
        let mut pattern = String::new();
        if !text.is_empty() {
            for c in text.chars() {
                if matches!(c, '%' | '_' | '\\') {
                    pattern.push('\\');
                }
                pattern.push(c);
            }
            pattern = format!("%{pattern}%");
            sql.push_str(
                " AND (p.name LIKE ?1 ESCAPE '\\' OR p.path LIKE ?1 ESCAPE '\\'
                   OR e.local_addr LIKE ?1 ESCAPE '\\' OR e.remote_addr LIKE ?1 ESCAPE '\\'
                   OR CAST(e.local_port AS TEXT) LIKE ?1 ESCAPE '\\'
                   OR CAST(e.remote_port AS TEXT) LIKE ?1 ESCAPE '\\'
                   OR CAST(p.pid AS TEXT) LIKE ?1 ESCAPE '\\')",
            );
        } else {
            sql.push_str(" AND ?1 = ''");
        }
        (sql, pattern)
    }
}

/// Columns read for an event (`e` = events, `p` = processes).
const EVENT_COLS: &str = "e.time_ms, e.kind, e.proto, e.local_addr, e.local_port, e.remote_addr,
    e.remote_port, e.state_before, e.state_after, e.created, e.process_id,
    p.pid, p.name, p.path, p.create_time";

/// One row of [`EVENT_COLS`] before the process is shared.
struct RawEvent {
    time: u64,
    kind: EventKind,
    conn: Connection,
    state_before: Option<TcpState>,
    process_id: i64,
    process: ProcessInfo,
}

fn raw_event(r: &rusqlite::Row<'_>) -> rusqlite::Result<RawEvent> {
    let ip = |s: Option<String>| s.and_then(|s| s.parse::<IpAddr>().ok());
    let local_ip = ip(r.get(3)?).unwrap_or(IpAddr::from([0, 0, 0, 0]));
    let local = SocketAddr::new(local_ip, r.get::<_, i64>(4)? as u16);
    let remote = match (ip(r.get(5)?), r.get::<_, Option<i64>>(6)?) {
        (Some(ip), Some(port)) => Some(SocketAddr::new(ip, port as u16)),
        _ => None,
    };
    let pid = r.get::<_, i64>(11)? as u32;
    Ok(RawEvent {
        time: r.get::<_, i64>(0)?.max(0) as u64,
        kind: EventKind::from_code(r.get(1)?).unwrap_or(EventKind::Present),
        conn: Connection {
            proto: proto_from(r.get(2)?),
            local,
            remote,
            state: state_from(r.get(8)?),
            pid,
            created: r.get::<_, Option<i64>>(9)?.map(|v| v.max(0) as u64),
            module: None,
            stats: None,
        },
        state_before: state_from(r.get(7)?),
        process_id: r.get(10)?,
        process: ProcessInfo {
            name: r.get(12)?,
            path: r.get(13)?,
            started: r.get::<_, Option<i64>>(14)?.map(|v| v.max(0) as u64),
            ..Default::default()
        },
    })
}

/// Reads a recording (the replay worker owns it).
pub struct RecordingReader {
    db: Db,
    /// The processes read so far, shared by all their rows.
    processes: HashMap<i64, Arc<ProcessInfo>>,
}

impl RecordingReader {
    /// Opens a recording read-only and checks that it is one this version understands.
    pub fn open(path: &Path) -> Result<Self> {
        let db = Db::open_with_flags(
            path,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        let r = Self {
            db,
            processes: HashMap::new(),
        };
        r.check()?;
        Ok(r)
    }

    fn check(&self) -> Result<()> {
        let version: i64 = self.db.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        if version > SCHEMA_VERSION {
            return Err(RecError::Newer(version));
        }
        let format: Option<String> = self
            .db
            .query_row("SELECT value FROM meta WHERE key = 'format'", [], |r| {
                r.get(0)
            })
            .optional()
            .unwrap_or(None);
        if version < 1 || format.as_deref() != Some(FORMAT) {
            return Err(RecError::NotRecording);
        }
        Ok(())
    }

    pub fn info(&self) -> Result<Info> {
        let mut st = self
            .db
            .prepare("SELECT key, value FROM meta WHERE key != 'format' ORDER BY key")?;
        let meta = st
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<rusqlite::Result<Vec<(String, String)>>>()?;
        let (events, first, last): (i64, Option<i64>, Option<i64>) = self.db.query_row(
            "SELECT count(*), min(time_ms), max(time_ms) FROM events",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )?;
        Ok(Info {
            meta,
            events: events as u64,
            first: first.map(|v| v.max(0) as u64),
            last: last.map(|v| v.max(0) as u64),
        })
    }

    fn share(&mut self, raw: RawEvent) -> Event {
        let process = self
            .processes
            .entry(raw.process_id)
            .or_insert_with(|| Arc::new(raw.process))
            .clone();
        Event {
            time: raw.time,
            kind: raw.kind,
            conn: raw.conn,
            state_before: raw.state_before,
            process,
        }
    }

    /// The endpoints at the time `at`: for every endpoint its last event up to `at`; closed ones
    /// only while closed less than `hold_ms` ago (shown as closed, like in the live table).
    /// Rows whose last event is younger than `hold_ms` are marked new / changed / closed.
    pub fn state_at(&mut self, at: u64, hold_ms: u64) -> Result<Vec<Row>> {
        let sql = format!(
            "WITH last AS (
                 SELECT max(id) AS id,
                        max(CASE WHEN kind IN (0, 1) THEN time_ms END) AS seen
                 FROM events WHERE time_ms <= ?1
                 GROUP BY conn_id)
             SELECT {EVENT_COLS}, last.seen
             FROM last JOIN events e ON e.id = last.id JOIN processes p ON p.id = e.process_id
             WHERE e.kind != 2 OR e.time_ms > ?2"
        );
        let recent = ms(at.saturating_sub(hold_ms));
        let raws: Vec<(RawEvent, Option<i64>)> = {
            let mut st = self.db.prepare_cached(&sql)?;
            st.query_map(params![ms(at), recent], |r| {
                Ok((raw_event(r)?, r.get::<_, Option<i64>>(15)?))
            })?
            .collect::<rusqlite::Result<_>>()?
        };
        let mut rows = Vec::with_capacity(raws.len());
        for (raw, seen) in raws {
            let e = self.share(raw);
            let young = e.time as i64 > recent;
            let mark = match e.kind {
                EventKind::Closed => Mark::Closed,
                EventKind::New if young => Mark::New,
                EventKind::Changed if young => Mark::Changed,
                _ => Mark::None,
            };
            let state = e.shown_state();
            let mut conn = e.conn;
            conn.state = state;
            rows.push(Row {
                conn,
                process: e.process,
                first_seen: seen.map_or(e.time, |v| v.max(0) as u64),
                mark,
                mark_until: e.time + hold_ms,
                local_host: None,
                remote_host: None,
                rate_out: None,
                rate_in: None,
                updated: e.time,
            });
        }
        Ok(rows)
    }

    /// The time of the last event before `at` and of the first one after it (the step buttons).
    pub fn neighbours(&self, at: u64) -> Result<(Option<u64>, Option<u64>)> {
        let (prev, next): (Option<i64>, Option<i64>) = self.db.query_row(
            "SELECT (SELECT max(time_ms) FROM events WHERE time_ms < ?1),
                    (SELECT min(time_ms) FROM events WHERE time_ms > ?1)",
            params![ms(at)],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        Ok((prev.map(|v| v as u64), next.map(|v| v as u64)))
    }

    /// Number of events matching `q`.
    pub fn count(&self, q: &EventQuery) -> Result<u64> {
        let (clause, text) = q.clause();
        let sql = format!(
            "SELECT count(*) FROM events e JOIN processes p ON p.id = e.process_id WHERE {clause}"
        );
        let n: i64 = self.db.query_row(&sql, params![text], |r| r.get(0))?;
        Ok(n as u64)
    }

    /// `limit` events matching `q` from the position `offset`, in time order.
    pub fn events(&mut self, q: &EventQuery, offset: u64, limit: u64) -> Result<Vec<Event>> {
        let (clause, text) = q.clause();
        let order = if q.newest_first { "DESC" } else { "ASC" };
        let sql = format!(
            "SELECT {EVENT_COLS} FROM events e JOIN processes p ON p.id = e.process_id
             WHERE {clause} ORDER BY e.time_ms {order}, e.id {order} LIMIT ?2 OFFSET ?3"
        );
        let raws: Vec<RawEvent> = {
            let mut st = self.db.prepare_cached(&sql)?;
            st.query_map(params![text, ms(limit), ms(offset)], raw_event)?
                .collect::<rusqlite::Result<_>>()?
        };
        Ok(raws.into_iter().map(|r| self.share(r)).collect())
    }
}

#[cfg(test)]
#[path = "db_tests.rs"]
mod tests;
