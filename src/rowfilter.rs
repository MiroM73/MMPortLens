//! What decides whether a row is shown: the protocol switches, the text filter of the toolbar and
//! the column filters (#1) of the visible columns. Used by the table, by a recording "only rows
//! matching the current filters" (#3) and by the command line without a window (#16).

use crate::colfilter::{ColumnFilter, FilterError};
use crate::columns::{ColumnId, cell_text};
use crate::model::{Mark, Row};
use crate::presets::FilterPreset;
use crate::recording::Event;

#[derive(Clone)]
pub struct RowFilter {
    pub show_tcp: bool,
    pub show_udp: bool,
    pub show_ipv4: bool,
    pub show_ipv6: bool,
    pub show_listening: bool,
    /// The text filter, lower case.
    pub needle: String,
    /// The visible columns: the text filter searches them, only their column filters apply.
    pub cols: Vec<ColumnId>,
    pub col_filters: Vec<(ColumnId, ColumnFilter)>,
}

impl RowFilter {
    /// The filters of a preset (switches, text, column filters) for the visible columns `cols`;
    /// the first invalid column filter is the error.
    pub fn from_preset(
        p: &FilterPreset,
        cols: Vec<ColumnId>,
    ) -> Result<RowFilter, (ColumnId, FilterError)> {
        Ok(RowFilter {
            show_tcp: p.show_tcp,
            show_udp: p.show_udp,
            show_ipv4: p.show_ipv4,
            show_ipv6: p.show_ipv6,
            show_listening: p.show_listening,
            needle: p.text.trim().to_lowercase(),
            cols,
            col_filters: compile(&p.column_filters)?,
        })
    }

    pub fn matches(&self, r: &Row) -> bool {
        let p = r.conn.proto;
        if (p.is_tcp() && !self.show_tcp) || (!p.is_tcp() && !self.show_udp) {
            return false;
        }
        if (p.is_v6() && !self.show_ipv6) || (!p.is_v6() && !self.show_ipv4) {
            return false;
        }
        if !self.show_listening && r.conn.remote.is_none() {
            return false;
        }
        let text_ok = self.needle.is_empty()
            || self
                .cols
                .iter()
                .any(|&col| cell_text(r, col).to_lowercase().contains(&self.needle));
        text_ok
            && self
                .col_filters
                .iter()
                .filter(|(id, _)| self.cols.contains(id))
                .all(|(id, f)| f.matches(&cell_text(r, *id)))
    }

    /// The filter of a recorded event (a closed endpoint keeps its last state).
    pub fn matches_event(&self, e: &Event) -> bool {
        self.matches(&event_row(e))
    }
}

/// Compiles column filter texts; blank texts filter nothing, the first invalid one is the error.
pub fn compile(
    list: &[(ColumnId, String)],
) -> Result<Vec<(ColumnId, ColumnFilter)>, (ColumnId, FilterError)> {
    let mut out = Vec::new();
    for (id, text) in list {
        match ColumnFilter::parse(text, id.filter_kind()) {
            Ok(Some(f)) => out.push((*id, f)),
            Ok(None) => {}
            Err(e) => return Err((*id, e)),
        }
    }
    Ok(out)
}

/// A row for the filter of an event.
fn event_row(e: &Event) -> Row {
    let mut conn = e.conn.clone();
    conn.state = e.shown_state();
    Row {
        conn,
        process: e.process.clone(),
        first_seen: e.time,
        mark: Mark::None,
        mark_until: 0,
        local_host: None,
        remote_host: None,
        rate_out: None,
        rate_in: None,
        updated: e.time,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::net::{Connection, Protocol as Proto, TcpState};
    use crate::process::ProcessInfo;
    use std::sync::Arc;

    fn row(proto: Proto, state: Option<TcpState>, local: &str, remote: Option<&str>) -> Row {
        Row {
            conn: Connection {
                proto,
                local: local.parse().unwrap(),
                remote: remote.map(|r| r.parse().unwrap()),
                state,
                pid: 4,
                created: None,
                module: None,
                stats: None,
            },
            process: Arc::new(ProcessInfo {
                name: "chrome.exe".into(),
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
        }
    }

    #[test]
    fn preset_filters() {
        let est = row(
            Proto::Tcp,
            Some(TcpState::Established),
            "10.0.0.5:50000",
            Some("1.2.3.4:443"),
        );
        let listen = row(Proto::Tcp, Some(TcpState::Listen), "0.0.0.0:135", None);
        let udp = row(Proto::Udp, None, "0.0.0.0:53", None);
        let cols = vec![ColumnId::Process, ColumnId::State, ColumnId::RemotePort];
        let all = RowFilter::from_preset(&FilterPreset::default(), cols.clone()).unwrap();
        assert!(all.matches(&est) && all.matches(&listen) && all.matches(&udp));

        let p = FilterPreset {
            column_filters: vec![(ColumnId::State, "=Established".into())],
            ..Default::default()
        };
        let f = RowFilter::from_preset(&p, cols.clone()).unwrap();
        assert!(f.matches(&est) && !f.matches(&listen) && !f.matches(&udp));

        let p = FilterPreset {
            show_udp: false,
            show_listening: false,
            text: " CHROME ".into(),
            ..Default::default()
        };
        let f = RowFilter::from_preset(&p, cols.clone()).unwrap();
        assert!(f.matches(&est) && !f.matches(&listen) && !f.matches(&udp));

        // A filter of a hidden column does not apply.
        let p = FilterPreset {
            column_filters: vec![(ColumnId::LocalPort, "=1".into())],
            ..Default::default()
        };
        assert!(
            RowFilter::from_preset(&p, cols.clone())
                .unwrap()
                .matches(&est)
        );

        let bad = FilterPreset {
            column_filters: vec![(ColumnId::RemotePort, "/(/".into())],
            ..Default::default()
        };
        assert!(matches!(
            RowFilter::from_preset(&bad, cols),
            Err((ColumnId::RemotePort, FilterError::Regex(_)))
        ));
    }
}
