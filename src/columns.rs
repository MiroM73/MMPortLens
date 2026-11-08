//! The table columns: their texts, sort order, and the user's layout (which columns are shown and
//! in which order), stored in the settings.

use crate::model::Row;
use crate::net::{TcpState, ip_text};
use crate::t;
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ColumnId {
    Process,
    Pid,
    Protocol,
    State,
    LocalAddress,
    LocalPort,
    RemoteAddress,
    RemotePort,
    FirstSeen,
    ProcessPath,
    /// Host name of the remote address (#5).
    RemoteHost,
    /// Host name of the local address (#5).
    LocalHost,
    /// Name of the well-known port (#5).
    Service,
    /// Windows services running in the process (#7).
    WinServices,
    /// Owner of the process (#7).
    User,
    CommandLine,
    /// Company / description from the version resource (#7).
    Company,
    Description,
    /// When the system created the endpoint (#14).
    Created,
    /// How long the endpoint exists (#14).
    Duration,
    /// Owner module: the executable or the service in svchost (#14).
    Module,
    /// Traffic counters of TCP connections (#8).
    BytesSent,
    BytesReceived,
    RateSent,
    RateReceived,
    PacketsSent,
    PacketsReceived,
}

impl ColumnId {
    /// All columns in the default order (the order of TCPView, then the additional ones).
    pub const ALL: [ColumnId; 27] = [
        ColumnId::Process,
        ColumnId::Pid,
        ColumnId::Protocol,
        ColumnId::State,
        ColumnId::LocalAddress,
        ColumnId::LocalPort,
        ColumnId::LocalHost,
        ColumnId::RemoteAddress,
        ColumnId::RemotePort,
        ColumnId::RemoteHost,
        ColumnId::Service,
        ColumnId::Created,
        ColumnId::Duration,
        ColumnId::FirstSeen,
        ColumnId::Module,
        ColumnId::BytesSent,
        ColumnId::BytesReceived,
        ColumnId::RateSent,
        ColumnId::RateReceived,
        ColumnId::PacketsSent,
        ColumnId::PacketsReceived,
        ColumnId::ProcessPath,
        ColumnId::WinServices,
        ColumnId::User,
        ColumnId::CommandLine,
        ColumnId::Company,
        ColumnId::Description,
    ];

    /// Shown by default (the path is long and the local host name is mostly the own computer,
    /// so they start hidden).
    fn default_visible(self) -> bool {
        !matches!(
            self,
            ColumnId::ProcessPath
                | ColumnId::LocalHost
                | ColumnId::WinServices
                | ColumnId::User
                | ColumnId::CommandLine
                | ColumnId::Company
                | ColumnId::Description
                | ColumnId::FirstSeen
                | ColumnId::Duration
                | ColumnId::Module
        ) && !self.traffic()
    }

    /// The traffic columns of #8: while one is visible, the counters are collected.
    pub fn traffic(self) -> bool {
        matches!(
            self,
            ColumnId::BytesSent
                | ColumnId::BytesReceived
                | ColumnId::RateSent
                | ColumnId::RateReceived
                | ColumnId::PacketsSent
                | ColumnId::PacketsReceived
        )
    }

    fn key(self) -> &'static str {
        match self {
            ColumnId::Process => "column.process",
            ColumnId::Pid => "column.pid",
            ColumnId::Protocol => "column.protocol",
            ColumnId::State => "column.state",
            ColumnId::LocalAddress => "column.local_address",
            ColumnId::LocalPort => "column.local_port",
            ColumnId::RemoteAddress => "column.remote_address",
            ColumnId::RemotePort => "column.remote_port",
            ColumnId::FirstSeen => "column.first_seen",
            ColumnId::ProcessPath => "column.process_path",
            ColumnId::RemoteHost => "column.remote_host",
            ColumnId::LocalHost => "column.local_host",
            ColumnId::Service => "column.service",
            ColumnId::WinServices => "column.win_services",
            ColumnId::User => "column.user",
            ColumnId::CommandLine => "column.command_line",
            ColumnId::Company => "column.company",
            ColumnId::Description => "column.description",
            ColumnId::Created => "column.created",
            ColumnId::Duration => "column.duration",
            ColumnId::Module => "column.module",
            ColumnId::BytesSent => "column.bytes_sent",
            ColumnId::BytesReceived => "column.bytes_received",
            ColumnId::RateSent => "column.rate_sent",
            ColumnId::RateReceived => "column.rate_received",
            ColumnId::PacketsSent => "column.packets_sent",
            ColumnId::PacketsReceived => "column.packets_received",
        }
    }

    pub fn title(self) -> String {
        t!(self.key())
    }

    /// Initial width in points.
    pub fn width(self) -> f32 {
        match self {
            ColumnId::Process => 170.0,
            ColumnId::Pid | ColumnId::LocalPort | ColumnId::RemotePort => 70.0,
            ColumnId::Protocol => 70.0,
            ColumnId::State => 100.0,
            ColumnId::LocalAddress | ColumnId::RemoteAddress => 200.0,
            ColumnId::FirstSeen => 90.0,
            ColumnId::ProcessPath => 360.0,
            ColumnId::RemoteHost | ColumnId::LocalHost => 220.0,
            ColumnId::Service => 90.0,
            ColumnId::WinServices | ColumnId::Company => 180.0,
            ColumnId::User => 160.0,
            ColumnId::CommandLine => 400.0,
            ColumnId::Description => 220.0,
            ColumnId::Created => 110.0,
            ColumnId::Duration => 100.0,
            ColumnId::Module => 150.0,
            ColumnId::BytesSent
            | ColumnId::BytesReceived
            | ColumnId::RateSent
            | ColumnId::RateReceived => 100.0,
            ColumnId::PacketsSent | ColumnId::PacketsReceived => 90.0,
        }
    }

    /// Which alternatives the column filter understands (#1).
    pub fn filter_kind(self) -> crate::colfilter::FilterKind {
        use crate::colfilter::FilterKind;
        match self {
            ColumnId::Pid
            | ColumnId::LocalPort
            | ColumnId::RemotePort
            | ColumnId::PacketsSent
            | ColumnId::PacketsReceived => FilterKind::Number,
            ColumnId::LocalAddress | ColumnId::RemoteAddress => FilterKind::Address,
            _ => FilterKind::Text,
        }
    }

    /// Numbers are right-aligned.
    pub fn numeric(self) -> bool {
        matches!(
            self,
            ColumnId::Pid
                | ColumnId::LocalPort
                | ColumnId::RemotePort
                | ColumnId::Duration
                | ColumnId::BytesSent
                | ColumnId::BytesReceived
                | ColumnId::RateSent
                | ColumnId::RateReceived
                | ColumnId::PacketsSent
                | ColumnId::PacketsReceived
        )
    }
}

/// State column text: the netstat name, or a translated "Invalid (n)" for a value the OS should
/// never report (issue #21) instead of a bare `? (n)`.
pub fn state_text(s: TcpState) -> String {
    match s {
        TcpState::Unknown(v) => t!("state.invalid", value = v),
        s => s.label(),
    }
}

/// Text of a cell as shown in the table (also what the text filter searches).
pub fn cell_text(row: &Row, col: ColumnId) -> String {
    let c = &row.conn;
    match col {
        ColumnId::Process => row.process.name.clone(),
        ColumnId::Pid => c.pid.to_string(),
        ColumnId::Protocol => c.proto.label().into(),
        ColumnId::State => c.state.map(state_text).unwrap_or_default(),
        ColumnId::LocalAddress => ip_text(c.local.ip()),
        ColumnId::LocalPort => c.local.port().to_string(),
        ColumnId::RemoteAddress => c.remote.map(|r| ip_text(r.ip())).unwrap_or_default(),
        ColumnId::RemotePort => c.remote.map(|r| r.port().to_string()).unwrap_or_default(),
        ColumnId::FirstSeen => crate::timefmt::clock(row.first_seen),
        ColumnId::ProcessPath => row.process.path.clone(),
        ColumnId::RemoteHost => match (&row.remote_host, c.remote) {
            (Some(name), _) => name.to_string(),
            (None, Some(r)) => ip_text(r.ip()),
            (None, None) => String::new(),
        },
        ColumnId::LocalHost => match &row.local_host {
            Some(name) => name.to_string(),
            None => ip_text(c.local.ip()),
        },
        ColumnId::Service => service(row).unwrap_or_default().to_owned(),
        ColumnId::WinServices => win_services(row),
        ColumnId::User => row.process.user.clone(),
        ColumnId::CommandLine => row.process.command_line.clone(),
        ColumnId::Company => row.process.company.clone(),
        ColumnId::Description => row.process.description.clone(),
        ColumnId::Created => c
            .created
            .map(|t| crate::timefmt::stamp(t, crate::model::now_ms()))
            .unwrap_or_default(),
        ColumnId::Duration => c
            .created
            .map(|t| crate::timefmt::duration(crate::model::now_ms().saturating_sub(t)))
            .unwrap_or_default(),
        ColumnId::Module => c.module.as_deref().unwrap_or_default().to_owned(),
        ColumnId::BytesSent => c.stats.map(|s| bytes(s.bytes_out)).unwrap_or_default(),
        ColumnId::BytesReceived => c.stats.map(|s| bytes(s.bytes_in)).unwrap_or_default(),
        ColumnId::RateSent => row.rate_out.map(rate).unwrap_or_default(),
        ColumnId::RateReceived => row.rate_in.map(rate).unwrap_or_default(),
        ColumnId::PacketsSent => c.stats.map(|s| s.segs_out.to_string()).unwrap_or_default(),
        ColumnId::PacketsReceived => c.stats.map(|s| s.segs_in.to_string()).unwrap_or_default(),
    }
}

/// A byte count: `512 B`, `1.5 KB`, `12.3 MB`, `1.21 GB` (1 KB = 1024 B).
pub fn bytes(n: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    if n < 1024 {
        return format!("{n} B");
    }
    let mut v = n as f64;
    let mut u = 0;
    while v >= 1024.0 && u < UNITS.len() - 1 {
        v /= 1024.0;
        u += 1;
    }
    if v < 10.0 && u >= 3 {
        format!("{v:.2} {}", UNITS[u])
    } else {
        format!("{v:.1} {}", UNITS[u])
    }
}

/// A transfer rate: `0 B/s`, `1.5 KB/s` …
pub fn rate(per_s: f64) -> String {
    format!("{}/s", bytes(per_s.round().max(0.0) as u64))
}

/// Windows services of a row: the exact service when the owner module of the endpoint is one of
/// the services of its process (#14), otherwise all services of the process (#7).
pub fn win_services(row: &Row) -> String {
    if let Some(m) = &row.conn.module
        && let Some((name, _)) = row
            .process
            .services
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(m))
    {
        return name.clone();
    }
    row.process.service_list()
}

/// Service of a row: the name of the remote port, for endpoints without a remote side (listeners,
/// UDP) the name of the local port.
pub fn service(row: &Row) -> Option<&'static str> {
    let c = &row.conn;
    let port = c.remote.map_or(c.local.port(), |r| r.port());
    crate::services::name(port, c.proto)
}

/// Order of two rows by a column; numbers and addresses compare as values, not as text.
pub fn compare(a: &Row, b: &Row, col: ColumnId) -> Ordering {
    let (x, y) = (&a.conn, &b.conn);
    match col {
        ColumnId::Process => a
            .process
            .name
            .to_lowercase()
            .cmp(&b.process.name.to_lowercase()),
        ColumnId::Pid => x.pid.cmp(&y.pid),
        ColumnId::Protocol => x.proto.cmp(&y.proto),
        ColumnId::State => x.state.cmp(&y.state),
        ColumnId::LocalAddress => x.local.ip().cmp(&y.local.ip()),
        ColumnId::LocalPort => x.local.port().cmp(&y.local.port()),
        ColumnId::RemoteAddress => x.remote.map(|r| r.ip()).cmp(&y.remote.map(|r| r.ip())),
        ColumnId::RemotePort => x.remote.map(|r| r.port()).cmp(&y.remote.map(|r| r.port())),
        ColumnId::FirstSeen => a.first_seen.cmp(&b.first_seen),
        ColumnId::ProcessPath => a
            .process
            .path
            .to_lowercase()
            .cmp(&b.process.path.to_lowercase()),
        // Names before bare addresses, then alphabetically.
        ColumnId::RemoteHost | ColumnId::LocalHost => {
            let host = |r: &Row| {
                let name = if col == ColumnId::RemoteHost {
                    &r.remote_host
                } else {
                    &r.local_host
                };
                (name.is_none(), cell_text(r, col).to_lowercase())
            };
            host(a).cmp(&host(b))
        }
        ColumnId::Service => service(a).cmp(&service(b)),
        // Older first; unknown times last.
        ColumnId::Created => {
            (x.created.is_none(), x.created).cmp(&(y.created.is_none(), y.created))
        }
        // The longest-living first when ascending would surprise; shorter = created later.
        ColumnId::Duration => (x.created.is_none(), std::cmp::Reverse(x.created))
            .cmp(&(y.created.is_none(), std::cmp::Reverse(y.created))),
        ColumnId::BytesSent => stat(x, |s| s.bytes_out).cmp(&stat(y, |s| s.bytes_out)),
        ColumnId::BytesReceived => stat(x, |s| s.bytes_in).cmp(&stat(y, |s| s.bytes_in)),
        ColumnId::PacketsSent => stat(x, |s| s.segs_out).cmp(&stat(y, |s| s.segs_out)),
        ColumnId::PacketsReceived => stat(x, |s| s.segs_in).cmp(&stat(y, |s| s.segs_in)),
        ColumnId::RateSent => a
            .rate_out
            .partial_cmp(&b.rate_out)
            .unwrap_or(Ordering::Equal),
        ColumnId::RateReceived => a.rate_in.partial_cmp(&b.rate_in).unwrap_or(Ordering::Equal),
        ColumnId::WinServices
        | ColumnId::Module
        | ColumnId::User
        | ColumnId::CommandLine
        | ColumnId::Company
        | ColumnId::Description => cell_text(a, col)
            .to_lowercase()
            .cmp(&cell_text(b, col).to_lowercase()),
    }
}

/// A traffic counter of a connection for sorting (none before any value).
fn stat(
    c: &crate::net::Connection,
    f: impl Fn(&crate::net::estats::TcpStats) -> u64,
) -> Option<u64> {
    c.stats.as_ref().map(f)
}

/// Sorting of the table: by a column, then always by a stable tie-breaker so rows do not jump
/// between refreshes.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct SortSpec {
    pub column: ColumnId,
    pub ascending: bool,
}

impl Default for SortSpec {
    fn default() -> Self {
        Self {
            column: ColumnId::Process,
            ascending: true,
        }
    }
}

impl SortSpec {
    pub fn compare(&self, a: &Row, b: &Row) -> Ordering {
        let o = compare(a, b, self.column);
        let o = if self.ascending { o } else { o.reverse() };
        o.then_with(|| a.conn.key().cmp(&b.conn.key()))
    }

    /// A click on a header: same column → reverse, other column → ascending by it.
    pub fn click(&mut self, col: ColumnId) {
        if self.column == col {
            self.ascending = !self.ascending;
        } else {
            *self = SortSpec {
                column: col,
                ascending: true,
            };
        }
    }
}

#[derive(Clone, Copy, PartialEq, Debug, Serialize, Deserialize)]
pub struct ColumnSetting {
    pub id: ColumnId,
    pub visible: bool,
    /// Width set by the user (#2); `None` = the default [`ColumnId::width`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width: Option<f32>,
}

/// Where a dragged column (or a row of the column window) lands (#2): `edges` are the start and
/// end of the cells along the drag direction, `x` the pointer. Returns the cell under the pointer
/// and whether the drop goes after it (second half); before the first / after the last cell the
/// nearest end. `None` without cells.
pub fn drop_slot(edges: &[(f32, f32)], x: f32) -> Option<(usize, bool)> {
    let last = edges.len().checked_sub(1)?;
    edges
        .iter()
        .enumerate()
        .find(|&(i, &(_, end))| x < end || i == last)
        .map(|(i, &(start, end))| (i, x > (start + end) * 0.5))
}

/// Narrowest and widest width a column keeps (#2).
pub const MIN_WIDTH: f32 = 40.0;
pub const MAX_WIDTH: f32 = 2000.0;

/// Which columns are shown, in which order and how wide. Stored in the settings;
/// [`Self::normalize`] repairs files from older versions (new columns are added hidden at the end).
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ColumnLayout(pub Vec<ColumnSetting>);

impl Default for ColumnLayout {
    fn default() -> Self {
        Self(
            ColumnId::ALL
                .iter()
                .map(|&id| ColumnSetting {
                    id,
                    visible: id.default_visible(),
                    width: None,
                })
                .collect(),
        )
    }
}

impl ColumnLayout {
    /// Removes duplicates, adds missing columns (hidden) and keeps at least one column visible.
    pub fn normalize(&mut self) {
        let mut seen = Vec::new();
        self.0.retain(|c| {
            let dup = seen.contains(&c.id);
            seen.push(c.id);
            !dup
        });
        for id in ColumnId::ALL {
            if !seen.contains(&id) {
                self.0.push(ColumnSetting {
                    id,
                    visible: false,
                    width: None,
                });
            }
        }
        for c in &mut self.0 {
            // A broken width in the file falls back to the default.
            c.width = c
                .width
                .filter(|w| w.is_finite())
                .map(|w| w.clamp(MIN_WIDTH, MAX_WIDTH));
        }
        if !self.0.iter().any(|c| c.visible) {
            self.0[0].visible = true;
        }
    }

    /// The visible columns in their order.
    pub fn visible(&self) -> Vec<ColumnId> {
        self.0.iter().filter(|c| c.visible).map(|c| c.id).collect()
    }

    /// Shows or hides a column; the last visible column cannot be hidden.
    pub fn set_visible(&mut self, id: ColumnId, on: bool) {
        if !on && self.visible() == [id] {
            return;
        }
        if let Some(c) = self.0.iter_mut().find(|c| c.id == id) {
            c.visible = on;
        }
    }

    /// The width of a column: the user's or the default one.
    pub fn width(&self, id: ColumnId) -> f32 {
        self.0
            .iter()
            .find(|c| c.id == id)
            .and_then(|c| c.width)
            .unwrap_or_else(|| id.width())
    }

    /// Remembers the width of a column (clamped); the default width is stored as `None`.
    /// Returns true when the stored value changed.
    pub fn set_width(&mut self, id: ColumnId, width: f32) -> bool {
        if !width.is_finite() {
            return false;
        }
        let w = width.clamp(MIN_WIDTH, MAX_WIDTH);
        let new = ((w - id.width()).abs() >= 0.5).then_some(w.round());
        let Some(c) = self.0.iter_mut().find(|c| c.id == id) else {
            return false;
        };
        let same = match (c.width, new) {
            (Some(a), Some(b)) => (a - b).abs() < 0.5,
            (a, b) => a.is_none() && b.is_none(),
        };
        if !same {
            c.width = new;
        }
        !same
    }

    /// Moves column `id` right before (or with `after` right after) column `target` – the drop
    /// of a dragged header or row of the column window (#2).
    pub fn move_to(&mut self, id: ColumnId, target: ColumnId, after: bool) {
        if id == target {
            return;
        }
        let Some(i) = self.0.iter().position(|c| c.id == id) else {
            return;
        };
        let c = self.0.remove(i);
        let Some(j) = self.0.iter().position(|c| c.id == target) else {
            self.0.insert(i, c);
            return;
        };
        self.0.insert(if after { j + 1 } else { j }, c);
    }

    /// Moves a column by `delta` places in the order (clamped to the ends).
    pub fn move_by(&mut self, id: ColumnId, delta: isize) {
        let Some(i) = self.0.iter().position(|c| c.id == id) else {
            return;
        };
        let j = (i as isize + delta).clamp(0, self.0.len() as isize - 1) as usize;
        let c = self.0.remove(i);
        self.0.insert(j, c);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invalid_state_has_its_own_text() {
        assert_eq!(state_text(TcpState::Listen), "Listen");
        let t = state_text(TcpState::Unknown(0));
        assert!(t.contains('0') && !t.starts_with('?'), "{t}");
    }

    #[test]
    fn layout_default_and_visibility() {
        let mut l = ColumnLayout::default();
        assert_eq!(l.visible().len(), ColumnId::ALL.len() - 16);
        assert!(l.visible().contains(&ColumnId::Created));
        assert!(!l.visible().contains(&ColumnId::ProcessPath));
        assert!(l.visible().contains(&ColumnId::RemoteHost));
        l.set_visible(ColumnId::ProcessPath, true);
        assert!(l.visible().contains(&ColumnId::ProcessPath));
        // The last visible column stays.
        for id in ColumnId::ALL {
            l.set_visible(id, false);
        }
        assert_eq!(l.visible().len(), 1);
    }

    #[test]
    fn layout_move_to() {
        use ColumnId::*;
        let mut l = ColumnLayout::default();
        l.move_to(Pid, State, true);
        let v = l.visible();
        let pos = |id| v.iter().position(|c| *c == id).unwrap();
        assert_eq!(pos(Pid), pos(State) + 1);
        l.move_to(Pid, Process, false);
        assert_eq!(l.visible()[0], Pid);
        // A drop onto itself changes nothing.
        let before = l.clone();
        l.move_to(Pid, Pid, true);
        assert_eq!(l, before);
        // Moving to the end.
        let last = *l.visible().last().unwrap();
        l.move_to(Process, last, true);
        assert_eq!(*l.visible().last().unwrap(), Process);
        assert_eq!(l.0.len(), ColumnId::ALL.len());
    }

    #[test]
    fn drop_slots() {
        let cells = [(0.0, 100.0), (110.0, 160.0), (170.0, 300.0)];
        assert_eq!(drop_slot(&[], 5.0), None);
        assert_eq!(drop_slot(&cells, -20.0), Some((0, false)));
        assert_eq!(drop_slot(&cells, 20.0), Some((0, false)));
        assert_eq!(drop_slot(&cells, 60.0), Some((0, true)));
        // In the gap between two cells: before the next one.
        assert_eq!(drop_slot(&cells, 105.0), Some((1, false)));
        assert_eq!(drop_slot(&cells, 150.0), Some((1, true)));
        assert_eq!(drop_slot(&cells, 900.0), Some((2, true)));
    }

    #[test]
    fn layout_widths() {
        let mut l = ColumnLayout::default();
        assert_eq!(l.width(ColumnId::Process), ColumnId::Process.width());
        assert!(l.set_width(ColumnId::Process, 250.4));
        assert_eq!(l.width(ColumnId::Process), 250.0);
        assert!(!l.set_width(ColumnId::Process, 250.2), "no change");
        // Clamped, and the default width is not stored.
        l.set_width(ColumnId::Pid, 1.0);
        assert_eq!(l.width(ColumnId::Pid), MIN_WIDTH);
        assert!(l.set_width(ColumnId::Pid, ColumnId::Pid.width()));
        assert_eq!(
            l.0.iter().find(|c| c.id == ColumnId::Pid).unwrap().width,
            None
        );
        assert!(!l.set_width(ColumnId::Pid, f32::NAN));
        // Widths survive the settings round trip and a broken value is repaired.
        let json = serde_json::to_string(&l).unwrap();
        assert!(json.contains("250"));
        let mut back: ColumnLayout = serde_json::from_str(&json).unwrap();
        back.normalize();
        assert_eq!(back, l);
        let mut old: ColumnLayout =
            serde_json::from_str(r#"[{"id":"pid","visible":true,"width":-5.0}]"#).unwrap();
        old.normalize();
        assert_eq!(old.width(ColumnId::Pid), MIN_WIDTH);
    }

    #[test]
    fn layout_move() {
        let mut l = ColumnLayout::default();
        l.move_by(ColumnId::Pid, -1);
        assert_eq!(l.0[0].id, ColumnId::Pid);
        l.move_by(ColumnId::Pid, -5);
        assert_eq!(l.0[0].id, ColumnId::Pid);
        l.move_by(ColumnId::Process, 100);
        assert_eq!(l.0.last().unwrap().id, ColumnId::Process);
    }

    #[test]
    fn layout_normalize_old_file() {
        let mut l: ColumnLayout = serde_json::from_str(
            r#"[{"id":"pid","visible":true},{"id":"pid","visible":false},{"id":"state","visible":false}]"#,
        )
        .unwrap();
        l.normalize();
        assert_eq!(l.0.len(), ColumnId::ALL.len());
        assert_eq!(l.0[0].id, ColumnId::Pid);
        assert_eq!(l.visible(), vec![ColumnId::Pid]);
    }

    #[test]
    fn host_and_service_cells() {
        use crate::net::{Connection, Protocol};
        let mut row = Row {
            conn: Connection {
                proto: Protocol::Tcp,
                local: "10.0.0.5:50000".parse().unwrap(),
                remote: Some("140.82.112.3:443".parse().unwrap()),
                state: Some(TcpState::Established),
                pid: 1,
                created: None,
                module: None,
                stats: None,
            },
            process: Default::default(),
            first_seen: 0,
            mark: crate::model::Mark::None,
            mark_until: 0,
            local_host: None,
            remote_host: None,
            rate_out: None,
            rate_in: None,
            updated: 0,
        };
        // Unresolved: the address.
        assert_eq!(cell_text(&row, ColumnId::RemoteHost), "140.82.112.3");
        assert_eq!(cell_text(&row, ColumnId::Service), "https");
        let bare = row.clone();
        row.remote_host = Some("lb-140-82-112-3-iad.github.com".into());
        assert_eq!(
            cell_text(&row, ColumnId::RemoteHost),
            "lb-140-82-112-3-iad.github.com"
        );
        // Names sort before bare addresses.
        assert_eq!(compare(&row, &bare, ColumnId::RemoteHost), Ordering::Less);
        // A listener: the service of the local port, no remote host.
        row.conn.remote = None;
        row.conn.local = "0.0.0.0:3389".parse().unwrap();
        row.remote_host = None;
        assert_eq!(cell_text(&row, ColumnId::RemoteHost), "");
        assert_eq!(cell_text(&row, ColumnId::Service), "rdp");
    }

    #[test]
    fn exact_service_from_the_owner_module() {
        use crate::net::{Connection, Protocol};
        use crate::process::ProcessInfo;
        let mut row = Row {
            conn: Connection {
                proto: Protocol::Udp,
                local: "0.0.0.0:5353".parse().unwrap(),
                remote: None,
                state: None,
                pid: 1,
                created: Some(1_000),
                module: Some("dnscache".into()),
                stats: None,
            },
            process: std::sync::Arc::new(ProcessInfo {
                services: vec![
                    ("Dnscache".into(), "DNS Client".into()),
                    ("NlaSvc".into(), "Network Location Awareness".into()),
                ],
                ..Default::default()
            }),
            first_seen: 0,
            mark: crate::model::Mark::None,
            mark_until: 0,
            local_host: None,
            remote_host: None,
            rate_out: None,
            rate_in: None,
            updated: 0,
        };
        assert_eq!(cell_text(&row, ColumnId::WinServices), "Dnscache");
        assert_eq!(cell_text(&row, ColumnId::Module), "dnscache");
        let older = row.clone();
        // A module that is not a service: all services of the process.
        row.conn.module = Some("svchost.exe".into());
        row.conn.created = Some(2_000);
        assert_eq!(cell_text(&row, ColumnId::WinServices), "Dnscache, NlaSvc");
        // Created: older first; Duration: shorter (created later) first.
        assert_eq!(compare(&older, &row, ColumnId::Created), Ordering::Less);
        assert_eq!(compare(&row, &older, ColumnId::Duration), Ordering::Less);
        row.conn.created = None;
        assert_eq!(compare(&older, &row, ColumnId::Created), Ordering::Less);
        assert_eq!(cell_text(&row, ColumnId::Duration), "");
    }

    #[test]
    fn byte_and_rate_texts() {
        assert_eq!(bytes(0), "0 B");
        assert_eq!(bytes(1023), "1023 B");
        assert_eq!(bytes(1536), "1.5 KB");
        assert_eq!(bytes(12_900_000), "12.3 MB");
        assert_eq!(bytes(1_300_000_000), "1.21 GB");
        assert_eq!(rate(1536.4), "1.5 KB/s");
        assert_eq!(rate(-3.0), "0 B/s");
        assert!(ColumnId::RateSent.traffic() && !ColumnId::Pid.traffic());
        assert!(
            !ColumnLayout::default()
                .visible()
                .iter()
                .any(|c| c.traffic())
        );
    }

    #[test]
    fn sort_click() {
        let mut s = SortSpec::default();
        s.click(ColumnId::Process);
        assert!(!s.ascending);
        s.click(ColumnId::Pid);
        assert_eq!((s.column, s.ascending), (ColumnId::Pid, true));
    }
}
