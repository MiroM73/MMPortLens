//! Recording of connection events (#3): what the `Tracker` saw change between two refreshes,
//! written into a SQLite database (a `.plrec` file) and read back for the replay.
//!
//! * [`Event`] – one change (new, closed, state changed) or one row of the starting state.
//! * [`db`] – all SQL: [`db::RecordingWriter`] and [`db::RecordingReader`], unit-tested on an
//!   in-memory database. The UI never touches SQL.
//! * [`threads`] – the writer thread (owns the database connection while recording, one batch
//!   of events per refresh over a channel) and the replay worker (answers the queries of the
//!   open recording), so the UI thread never waits for the disk.

pub mod db;
pub mod threads;

use crate::net::{Connection, TcpState};
use crate::process::ProcessInfo;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// File extension of a recording.
pub const EXTENSION: &str = "plrec";

/// What happened to an endpoint. The numbers are stored in the database (`events.kind`).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum EventKind {
    /// The endpoint existed when the recording started (the starting state of the replay).
    Present = 0,
    New = 1,
    Closed = 2,
    /// The TCP state changed (e.g. Established → TimeWait).
    Changed = 3,
}

impl EventKind {
    pub const ALL: [EventKind; 4] = [
        EventKind::Present,
        EventKind::New,
        EventKind::Closed,
        EventKind::Changed,
    ];

    pub fn code(self) -> i64 {
        self as i64
    }

    pub fn from_code(v: i64) -> Option<Self> {
        Self::ALL.into_iter().find(|k| k.code() == v)
    }

    /// The `t!` key of its name.
    pub fn key(self) -> &'static str {
        match self {
            EventKind::Present => "event.present",
            EventKind::New => "event.new",
            EventKind::Closed => "event.closed",
            EventKind::Changed => "event.changed",
        }
    }
}

/// Which events a recording keeps (the starting state is always written).
#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Debug)]
#[serde(default)]
pub struct EventKinds {
    pub new: bool,
    pub closed: bool,
    pub changed: bool,
}

impl Default for EventKinds {
    fn default() -> Self {
        Self {
            new: true,
            closed: true,
            changed: true,
        }
    }
}

impl EventKinds {
    pub fn wants(self, k: EventKind) -> bool {
        match k {
            EventKind::Present => true,
            EventKind::New => self.new,
            EventKind::Closed => self.closed,
            EventKind::Changed => self.changed,
        }
    }

    /// Stored in the `meta` table, e.g. `new,closed,changed`.
    pub fn text(self) -> String {
        let mut v = Vec::new();
        for (on, name) in [
            (self.new, "new"),
            (self.closed, "closed"),
            (self.changed, "changed"),
        ] {
            if on {
                v.push(name);
            }
        }
        v.join(",")
    }
}

/// One event of a recording.
#[derive(Clone, Debug, PartialEq)]
pub struct Event {
    /// ms since the Unix epoch (the time of the refresh that saw it).
    pub time: u64,
    pub kind: EventKind,
    /// The endpoint; `conn.state` is the state after the event (`None` for a closed endpoint
    /// and for UDP).
    pub conn: Connection,
    /// The state before the event (changed and closed TCP endpoints).
    pub state_before: Option<TcpState>,
    pub process: Arc<ProcessInfo>,
    /// A connection seen only by ETW (#4); `events.source = 1` in the file.
    pub etw: bool,
}

impl Event {
    /// The state to show for the endpoint after the event: a closed endpoint keeps its last one.
    pub fn shown_state(&self) -> Option<TcpState> {
        self.conn.state.or(self.state_before)
    }
}

/// Recording durations offered in the start window (minutes; `None` = until stopped).
pub const DURATIONS: [Option<u64>; 5] = [None, Some(1), Some(5), Some(15), Some(60)];

/// The choices of the start window, remembered in the settings.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, Debug)]
#[serde(default)]
pub struct Settings {
    /// One of [`DURATIONS`] (minutes, `None` = until stopped) …
    pub minutes: Option<u64>,
    /// … or the custom duration typed into `custom` (`10m`, `1h30m`, `90s`).
    pub use_custom: bool,
    pub custom: String,
    pub kinds: EventKinds,
    /// Only endpoints matching the filters of the table at the start.
    pub only_filtered: bool,
    /// Folder of the last recording (the file dialogs start there).
    pub folder: Option<std::path::PathBuf>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            minutes: None,
            use_custom: false,
            custom: "30m".into(),
            kinds: EventKinds::default(),
            only_filtered: false,
            folder: None,
        }
    }
}

impl Settings {
    /// The chosen duration in ms (`None` = until stopped); `Err` = the custom text is invalid.
    pub fn duration_ms(&self) -> Result<Option<u64>, ()> {
        if self.use_custom {
            parse_duration(&self.custom).map(Some).ok_or(())
        } else {
            Ok(self.minutes.map(|m| m * 60_000))
        }
    }
}

/// Parses a duration like `10m`, `90s`, `1h`, `1h30m` or a bare number of minutes into ms
/// (the custom duration of the start window, `--duration` of the command line, #16).
pub fn parse_duration(text: &str) -> Option<u64> {
    let t = text.trim().to_ascii_lowercase();
    if t.is_empty() {
        return None;
    }
    if let Ok(min) = t.parse::<u64>() {
        return (min > 0).then(|| min * 60_000);
    }
    let mut total = 0u64;
    let mut num = String::new();
    for c in t.chars() {
        match c {
            '0'..='9' => num.push(c),
            ' ' => {}
            'h' | 'm' | 's' => {
                let n: u64 = num.parse().ok()?;
                num.clear();
                total += n * match c {
                    'h' => 3_600_000,
                    'm' => 60_000,
                    _ => 1000,
                };
            }
            _ => return None,
        }
    }
    (num.is_empty() && total > 0).then_some(total)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kinds_and_codes() {
        for k in EventKind::ALL {
            assert_eq!(EventKind::from_code(k.code()), Some(k));
        }
        assert_eq!(EventKind::from_code(9), None);
        let only_new = EventKinds {
            new: true,
            closed: false,
            changed: false,
        };
        assert!(only_new.wants(EventKind::Present) && only_new.wants(EventKind::New));
        assert!(!only_new.wants(EventKind::Closed));
        assert_eq!(only_new.text(), "new");
        assert_eq!(EventKinds::default().text(), "new,closed,changed");
    }

    #[test]
    fn durations() {
        assert_eq!(parse_duration("10"), Some(600_000));
        assert_eq!(parse_duration("10m"), Some(600_000));
        assert_eq!(parse_duration(" 90s "), Some(90_000));
        assert_eq!(parse_duration("1h30m"), Some(5_400_000));
        assert_eq!(parse_duration("1H 5M"), Some(3_900_000));
        assert_eq!(parse_duration(""), None);
        assert_eq!(parse_duration("0"), None);
        assert_eq!(parse_duration("5x"), None);
        assert_eq!(parse_duration("1h5"), None);
        let mut s = Settings::default();
        assert_eq!(s.duration_ms(), Ok(None));
        s.minutes = Some(5);
        assert_eq!(s.duration_ms(), Ok(Some(300_000)));
        s.use_custom = true;
        assert_eq!(s.duration_ms(), Ok(Some(1_800_000)));
        s.custom = "soon".into();
        assert_eq!(s.duration_ms(), Err(()));
    }
}
