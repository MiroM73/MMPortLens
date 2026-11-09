//! Watch rules and alerts (#18): "tell me when something specific happens".
//!
//! A rule names a trigger (new connection, new listener, any new endpoint, state change,
//! closed endpoint) and conditions in the column filter syntax (#1) – one filter per column,
//! all must match, e.g. Process `chrome.exe` and Remote address `!10.0.0.0/8`. The rules are
//! checked against the events of the tracker (`model::Tracker::update_with_events`, the same
//! events a recording writes, #3), so a rule fires **once per event** – once per new connection,
//! not on every refresh – and also for the short connections ETW saw (#4).
//!
//! What a hit does is chosen per rule ([`Actions`]): a desktop notification, a sound, bringing
//! the window to the front, a line in the alert log file, starting a recording. The hits are
//! listed in the Alerts window (`app/watch.rs`). Everything here is pure and tested.

use crate::colfilter::{ColumnFilter, FilterError};
use crate::columns::{ColumnId, cell_text};
use crate::model::Row;
use crate::net::ConnKey;
use crate::recording::{Event, EventKind};
use crate::t;
use serde::{Deserialize, Serialize};

/// What happened to an endpoint.
#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Debug, Default)]
#[serde(rename_all = "snake_case")]
pub enum Trigger {
    /// A new endpoint with a remote side (a TCP connection; UDP rows from ETW, #4).
    #[default]
    NewConnection,
    /// A new endpoint without a remote side: a TCP listener or a UDP socket.
    NewListener,
    /// Any new endpoint.
    NewEndpoint,
    /// The TCP state changed (after the filtering of #32).
    StateChanged,
    /// The endpoint is gone.
    Closed,
}

impl Trigger {
    pub const ALL: [Trigger; 5] = [
        Trigger::NewConnection,
        Trigger::NewListener,
        Trigger::NewEndpoint,
        Trigger::StateChanged,
        Trigger::Closed,
    ];

    pub fn label(self) -> String {
        match self {
            Trigger::NewConnection => t!("watch.trigger.new_connection"),
            Trigger::NewListener => t!("watch.trigger.new_listener"),
            Trigger::NewEndpoint => t!("watch.trigger.new_endpoint"),
            Trigger::StateChanged => t!("watch.trigger.changed"),
            Trigger::Closed => t!("watch.trigger.closed"),
        }
    }

    /// Whether `event` is what this trigger waits for.
    pub fn fires_on(self, event: &Event) -> bool {
        match self {
            Trigger::NewConnection => event.kind == EventKind::New && event.conn.remote.is_some(),
            Trigger::NewListener => event.kind == EventKind::New && event.conn.remote.is_none(),
            Trigger::NewEndpoint => event.kind == EventKind::New,
            Trigger::StateChanged => event.kind == EventKind::Changed,
            Trigger::Closed => event.kind == EventKind::Closed,
        }
    }
}

/// One condition: the cell of `column` matches `pattern` (column filter syntax).
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, Debug)]
pub struct Condition {
    pub column: ColumnId,
    pub pattern: String,
}

/// What a hit of the rule does.
#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Debug)]
#[serde(default)]
pub struct Actions {
    /// A desktop notification (Windows: a notification of the tray icon).
    pub notify: bool,
    pub sound: bool,
    /// Bring the main window to the front (also out of the taskbar).
    pub focus: bool,
    /// A line in the alert log file.
    pub log: bool,
    /// Start a recording (#3) unless one is running.
    pub record: bool,
}

impl Default for Actions {
    fn default() -> Self {
        Self {
            notify: true,
            sound: false,
            focus: false,
            log: true,
            record: false,
        }
    }
}

#[derive(Clone, PartialEq, Serialize, Deserialize, Debug)]
#[serde(default)]
pub struct WatchRule {
    pub enabled: bool,
    pub name: String,
    pub trigger: Trigger,
    /// All must match; none = every endpoint of the trigger.
    pub conditions: Vec<Condition>,
    pub actions: Actions,
}

impl Default for WatchRule {
    fn default() -> Self {
        Self {
            enabled: true,
            name: t!("watch.new_name"),
            trigger: Trigger::NewConnection,
            conditions: Vec::new(),
            actions: Actions::default(),
        }
    }
}

/// The settings of the watch rules (`Config::watch`).
#[derive(Clone, PartialEq, Serialize, Deserialize, Debug)]
#[serde(default)]
pub struct WatchSettings {
    pub rules: Vec<WatchRule>,
    /// The alert log file; `None` = `alerts.log` next to the settings file.
    pub log_file: Option<std::path::PathBuf>,
    /// Length of a recording started by a rule, in minutes.
    pub record_minutes: u64,
}

impl Default for WatchSettings {
    fn default() -> Self {
        Self {
            rules: default_rules(),
            log_file: None,
            record_minutes: 5,
        }
    }
}

/// Example rules, all switched off (names in the current UI language).
pub fn default_rules() -> Vec<WatchRule> {
    let cond = |column, pattern: &str| Condition {
        column,
        pattern: pattern.into(),
    };
    vec![
        WatchRule {
            enabled: false,
            name: t!("watch.default.port_4444"),
            trigger: Trigger::NewConnection,
            conditions: vec![cond(ColumnId::RemotePort, "4444")],
            actions: Actions {
                sound: true,
                ..Default::default()
            },
        },
        WatchRule {
            enabled: false,
            name: t!("watch.default.listener"),
            trigger: Trigger::NewListener,
            conditions: vec![cond(ColumnId::Protocol, "=TCP|=TCPv6")],
            actions: Actions::default(),
        },
        WatchRule {
            enabled: false,
            name: t!("watch.default.chrome_outside"),
            trigger: Trigger::NewConnection,
            conditions: vec![
                cond(ColumnId::Process, "=chrome.exe"),
                cond(ColumnId::RemoteAddress, "!10.0.0.0/8&!loopback"),
            ],
            actions: Actions::default(),
        },
    ]
}

/// A rule ready to be checked: `None` = off (or no valid conditions), else the filters;
/// a condition that does not parse is kept as an error and the rule fires never.
pub enum CompiledRule {
    Off,
    On(Vec<(ColumnId, ColumnFilter)>),
    Invalid(usize, FilterError),
}

/// The rules compiled, in the order of the settings.
#[derive(Default)]
pub struct Compiled(pub Vec<CompiledRule>);

impl Compiled {
    pub fn new(rules: &[WatchRule]) -> Self {
        Compiled(rules.iter().map(compile).collect())
    }

    /// Whether any rule can fire (the tracker events are collected only then).
    pub fn any_on(&self) -> bool {
        self.0.iter().any(|r| matches!(r, CompiledRule::On(_)))
    }

    /// The error of the rule `index`: (condition index, error).
    pub fn error(&self, index: usize) -> Option<(usize, &FilterError)> {
        match self.0.get(index) {
            Some(CompiledRule::Invalid(i, e)) => Some((*i, e)),
            _ => None,
        }
    }
}

fn compile(rule: &WatchRule) -> CompiledRule {
    if !rule.enabled {
        return CompiledRule::Off;
    }
    let mut filters = Vec::new();
    for (i, c) in rule.conditions.iter().enumerate() {
        match ColumnFilter::parse(&c.pattern, c.column.filter_kind()) {
            Ok(Some(f)) => filters.push((c.column, f)),
            Ok(None) => {}
            Err(e) => return CompiledRule::Invalid(i, e),
        }
    }
    CompiledRule::On(filters)
}

/// One rule hit: when, which rule, what happened and the endpoint as a row (for the list, the
/// jump to the table and the log).
#[derive(Clone, Debug)]
pub struct Hit {
    pub time: u64,
    pub rule: String,
    pub actions: Actions,
    pub kind: EventKind,
    pub row: Row,
}

impl Hit {
    pub fn key(&self) -> ConnKey {
        self.row.conn.key()
    }

    /// One line for the log, the notification and the copy: `chrome.exe (1234) TCP
    /// 10.0.0.5:51000 → 1.2.3.4:443 Established`.
    pub fn summary(&self) -> String {
        let r = &self.row;
        let mut s = format!(
            "{} ({}) {} {}",
            r.process.name,
            r.conn.pid,
            r.conn.proto.label(),
            r.conn.local
        );
        if let Some(remote) = r.conn.remote {
            s.push_str(" → ");
            s.push_str(&remote.to_string());
        }
        let state = cell_text(r, ColumnId::State);
        if !state.is_empty() {
            s.push(' ');
            s.push_str(&state);
        }
        s
    }

    /// The line of the alert log file: local time, rule, event, summary (tab-separated).
    pub fn log_line(&self) -> String {
        format!(
            "{}\t{}\t{}\t{}",
            crate::timefmt::date_time(self.time),
            self.rule,
            event_label(self.kind),
            self.summary()
        )
    }
}

/// The name of an event kind for the alert list and the log.
pub fn event_label(kind: EventKind) -> String {
    match kind {
        EventKind::Present | EventKind::New => t!("watch.event.new"),
        EventKind::Changed => t!("watch.event.changed"),
        EventKind::Closed => t!("watch.event.closed"),
    }
}

/// The hits of `events` (one refresh): every event against every rule that is on, in event
/// order; an event can hit several rules.
pub fn check(rules: &[WatchRule], compiled: &Compiled, events: &[Event]) -> Vec<Hit> {
    let mut hits = Vec::new();
    for e in events {
        let mut row: Option<Row> = None;
        for (rule, c) in rules.iter().zip(&compiled.0) {
            let CompiledRule::On(filters) = c else {
                continue;
            };
            if !rule.trigger.fires_on(e) {
                continue;
            }
            let r = row.get_or_insert_with(|| crate::rowfilter::event_row(e));
            if filters
                .iter()
                .all(|(col, f)| f.matches(&cell_text(r, *col)))
            {
                hits.push(Hit {
                    time: e.time,
                    rule: rule.name.clone(),
                    actions: rule.actions,
                    kind: e.kind,
                    row: r.clone(),
                });
            }
        }
    }
    hits
}

/// The actions of a batch of hits, each at most once: the notification names the first hit and
/// how many more there are, so a burst of connections is one notification, not hundreds.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct Batch {
    /// (title, text) of the notification.
    pub notify: Option<(String, String)>,
    pub sound: bool,
    pub focus: bool,
    pub record: bool,
    /// Lines for the log file.
    pub log: Vec<String>,
}

pub fn batch(hits: &[Hit]) -> Batch {
    let mut b = Batch::default();
    let notified: Vec<&Hit> = hits.iter().filter(|h| h.actions.notify).collect();
    if let Some(first) = notified.first() {
        let mut text = first.summary();
        if notified.len() > 1 {
            text.push('\n');
            text.push_str(&t!("watch.notify.more", count = notified.len() - 1));
        }
        b.notify = Some((
            t!(
                "watch.notify.title",
                rule = &first.rule,
                event = event_label(first.kind)
            ),
            text,
        ));
    }
    b.sound = hits.iter().any(|h| h.actions.sound);
    b.focus = hits.iter().any(|h| h.actions.focus);
    b.record = hits.iter().any(|h| h.actions.record);
    b.log = hits
        .iter()
        .filter(|h| h.actions.log)
        .map(Hit::log_line)
        .collect();
    b
}

/// The alert log file of the settings: the chosen file or `alerts.log` next to the settings.
pub fn log_path(settings: &WatchSettings) -> Option<std::path::PathBuf> {
    settings
        .log_file
        .clone()
        .or_else(|| crate::config::Config::path().map(|p| p.with_file_name("alerts.log")))
}

/// Appends `lines` to the log file (created with its folder when missing).
pub fn append_log(path: &std::path::Path, lines: &[String]) -> std::io::Result<()> {
    use std::io::Write;
    if lines.is_empty() {
        return Ok(());
    }
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    let mut text = lines.join("\n");
    text.push('\n');
    f.write_all(text.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Tracker;
    use crate::net::{Connection, Protocol, TcpState};
    use crate::process::ProcessInfo;
    use std::sync::Arc;

    fn conn(
        local: &str,
        remote: Option<&str>,
        state: Option<TcpState>,
        name: &str,
    ) -> (Connection, Arc<ProcessInfo>) {
        (
            Connection {
                proto: if state.is_some() {
                    Protocol::Tcp
                } else {
                    Protocol::Udp
                },
                local: local.parse().unwrap(),
                remote: remote.map(|r| r.parse().unwrap()),
                state,
                pid: 4242,
                created: None,
                module: None,
                stats: None,
            },
            Arc::new(ProcessInfo {
                name: name.into(),
                ..Default::default()
            }),
        )
    }

    fn rule(trigger: Trigger, conditions: &[(ColumnId, &str)]) -> WatchRule {
        WatchRule {
            name: "test".into(),
            trigger,
            conditions: conditions
                .iter()
                .map(|(c, p)| Condition {
                    column: *c,
                    pattern: (*p).into(),
                })
                .collect(),
            ..Default::default()
        }
    }

    /// The acceptance of the issue: a rule on a local port fires once per new connection, not
    /// on every refresh – the hits come from the tracker events.
    #[test]
    fn a_rule_fires_once_per_new_connection() {
        let rules = vec![rule(
            Trigger::NewConnection,
            &[(ColumnId::LocalPort, "=5555")],
        )];
        let compiled = Compiled::new(&rules);
        let mut t = Tracker::default();
        let est = Some(TcpState::Established);
        let listener = conn("127.0.0.1:5555", None, Some(TcpState::Listen), "srv.exe");
        let a = conn("127.0.0.1:5555", Some("127.0.0.1:50001"), est, "srv.exe");
        let b = conn("127.0.0.1:5555", Some("127.0.0.1:50002"), est, "srv.exe");
        let other = conn("127.0.0.1:6666", Some("127.0.0.1:50003"), est, "srv.exe");
        let mut hits = Vec::new();
        let snapshots = [
            vec![listener.clone()],
            vec![listener.clone(), a.clone()],
            vec![listener.clone(), a.clone()],
            vec![listener.clone(), a.clone(), b.clone(), other.clone()],
            vec![listener.clone(), a.clone(), b.clone(), other.clone()],
        ];
        for (i, snap) in snapshots.into_iter().enumerate() {
            let mut events = Vec::new();
            t.update_with_events(snap, i as u64 * 1000, 500, Some(&mut events));
            hits.extend(check(&rules, &compiled, &events));
        }
        let remotes: Vec<u16> = hits
            .iter()
            .map(|h| h.row.conn.remote.unwrap().port())
            .collect();
        assert_eq!(remotes, vec![50001, 50002]);
        assert_eq!(
            hits[0].summary(),
            "srv.exe (4242) TCP 127.0.0.1:5555 → 127.0.0.1:50001 Established"
        );
    }

    #[test]
    fn triggers_and_conditions() {
        let est = Some(TcpState::Established);
        let mut t = Tracker::default();
        t.update_with_events(vec![], 0, 500, None);
        let mut events = Vec::new();
        t.update_with_events(
            vec![
                conn("0.0.0.0:8080", None, Some(TcpState::Listen), "web.exe"),
                conn("0.0.0.0:53", None, None, "dns.exe"),
                conn("10.0.0.5:50000", Some("10.1.2.3:443"), est, "chrome.exe"),
                conn("10.0.0.5:50001", Some("8.8.8.8:443"), est, "chrome.exe"),
            ],
            1000,
            500,
            Some(&mut events),
        );
        let count = |r: WatchRule| {
            let rules = vec![r];
            check(&rules, &Compiled::new(&rules), &events).len()
        };
        assert_eq!(count(rule(Trigger::NewConnection, &[])), 2);
        assert_eq!(count(rule(Trigger::NewListener, &[])), 2);
        assert_eq!(
            count(rule(
                Trigger::NewListener,
                &[(ColumnId::Protocol, "=TCP|=TCPv6")]
            )),
            1
        );
        assert_eq!(count(rule(Trigger::NewEndpoint, &[])), 4);
        assert_eq!(count(rule(Trigger::Closed, &[])), 0);
        // chrome.exe outside 10.0.0.0/8: only the 8.8.8.8 connection.
        let outside = rule(
            Trigger::NewConnection,
            &[
                (ColumnId::Process, "=chrome.exe"),
                (ColumnId::RemoteAddress, "!10.0.0.0/8"),
            ],
        );
        assert_eq!(count(outside), 1);
        // Off, or an invalid condition: never.
        let mut off = rule(Trigger::NewEndpoint, &[]);
        off.enabled = false;
        assert_eq!(count(off), 0);
        let bad = rule(Trigger::NewEndpoint, &[(ColumnId::Process, "/(/")]);
        let c = Compiled::new(std::slice::from_ref(&bad));
        assert!(c.error(0).is_some());
        assert!(!c.any_on());
        assert_eq!(count(bad), 0);

        // Closed and changed events.
        let mut events = Vec::new();
        t.update_with_events(
            vec![
                conn("0.0.0.0:8080", None, Some(TcpState::Listen), "web.exe"),
                conn("0.0.0.0:53", None, None, "dns.exe"),
                conn(
                    "10.0.0.5:50000",
                    Some("10.1.2.3:443"),
                    Some(TcpState::TimeWait),
                    "chrome.exe",
                ),
            ],
            2000,
            500,
            Some(&mut events),
        );
        let rules = vec![rule(Trigger::Closed, &[]), rule(Trigger::StateChanged, &[])];
        let hits = check(&rules, &Compiled::new(&rules), &events);
        assert_eq!(hits.len(), 2);
        assert!(
            hits.iter()
                .any(|h| h.kind == EventKind::Closed && h.row.conn.local.port() == 50001)
        );
        assert!(hits.iter().any(|h| h.kind == EventKind::Changed));
    }

    #[test]
    fn a_burst_is_one_notification() {
        let est = Some(TcpState::Established);
        let mut t = Tracker::default();
        t.update_with_events(vec![], 0, 500, None);
        let mut events = Vec::new();
        let snap = (0..5)
            .map(|i| {
                conn(
                    &format!("10.0.0.5:{}", 50000 + i),
                    Some("8.8.8.8:443"),
                    est,
                    "a.exe",
                )
            })
            .collect();
        t.update_with_events(snap, 1000, 500, Some(&mut events));
        let mut r = rule(Trigger::NewConnection, &[]);
        r.actions.sound = true;
        let rules = vec![r];
        let hits = check(&rules, &Compiled::new(&rules), &events);
        let b = batch(&hits);
        assert_eq!(hits.len(), 5);
        let (_, text) = b.notify.unwrap();
        assert_eq!(text.lines().count(), 2, "{text}");
        assert!(b.sound && !b.focus && !b.record);
        assert_eq!(b.log.len(), 5);
        assert!(b.log[0].contains("\ttest\t"));
        assert_eq!(batch(&[]), Batch::default());
    }

    #[test]
    fn examples_are_valid_and_off() {
        let rules = default_rules();
        let c = Compiled::new(&rules);
        assert!(rules.iter().all(|r| !r.enabled));
        for (i, mut r) in rules.into_iter().enumerate() {
            r.enabled = true;
            let c1 = Compiled::new(std::slice::from_ref(&r));
            assert!(c1.any_on(), "example {i} does not compile");
        }
        assert!(!c.any_on());
    }

    #[test]
    fn log_file_gets_lines_appended() {
        let dir = std::env::temp_dir().join(format!("mmportlens-watch-{}", std::process::id()));
        let path = dir.join("sub").join("alerts.log");
        append_log(&path, &["a".into()]).unwrap();
        append_log(&path, &["b".into(), "c".into()]).unwrap();
        append_log(&path, &[]).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "a\nb\nc\n");
        let _ = std::fs::remove_dir_all(dir);
    }
}
