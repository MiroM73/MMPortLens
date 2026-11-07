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
}

impl ColumnId {
    /// All columns in the default order (the order of TCPView, then the additional ones).
    pub const ALL: [ColumnId; 18] = [
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
        ColumnId::FirstSeen,
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
        }
    }

    /// Which alternatives the column filter understands (#1).
    pub fn filter_kind(self) -> crate::colfilter::FilterKind {
        use crate::colfilter::FilterKind;
        match self {
            ColumnId::Pid | ColumnId::LocalPort | ColumnId::RemotePort => FilterKind::Number,
            ColumnId::LocalAddress | ColumnId::RemoteAddress => FilterKind::Address,
            _ => FilterKind::Text,
        }
    }

    /// Numbers are right-aligned.
    pub fn numeric(self) -> bool {
        matches!(
            self,
            ColumnId::Pid | ColumnId::LocalPort | ColumnId::RemotePort
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
        ColumnId::WinServices => row.process.service_list(),
        ColumnId::User => row.process.user.clone(),
        ColumnId::CommandLine => row.process.command_line.clone(),
        ColumnId::Company => row.process.company.clone(),
        ColumnId::Description => row.process.description.clone(),
    }
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
        ColumnId::WinServices
        | ColumnId::User
        | ColumnId::CommandLine
        | ColumnId::Company
        | ColumnId::Description => cell_text(a, col)
            .to_lowercase()
            .cmp(&cell_text(b, col).to_lowercase()),
    }
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

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct ColumnSetting {
    pub id: ColumnId,
    pub visible: bool,
}

/// Which columns are shown and in which order. Stored in the settings; [`Self::normalize`]
/// repairs files from older versions (new columns are added hidden at the end).
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
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
                self.0.push(ColumnSetting { id, visible: false });
            }
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
        assert_eq!(l.visible().len(), ColumnId::ALL.len() - 7);
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
            },
            process: Default::default(),
            first_seen: 0,
            mark: crate::model::Mark::None,
            mark_until: 0,
            local_host: None,
            remote_host: None,
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
    fn sort_click() {
        let mut s = SortSpec::default();
        s.click(ColumnId::Process);
        assert!(!s.ascending);
        s.click(ColumnId::Pid);
        assert_eq!((s.column, s.ascending), (ColumnId::Pid, true));
    }
}
