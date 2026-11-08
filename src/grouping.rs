//! Rows grouped by process (#12): one collapsible group line per process with its endpoints
//! under it.
//!
//! The rows come in already filtered and sorted. A group appears where its first row is in that
//! order, so the groups follow the sort column too (Process → by name, PID → by PID, other
//! columns → by their best row), and the rows keep the sort order inside their group. Only rows
//! that passed the filters are grouped, so a group without matching rows never appears. The rows
//! of a collapsed group are left out of the view, so selection, keyboard navigation, copying and
//! the export work on the shown rows as before.

use crate::model::Row;
use std::collections::{HashMap, HashSet};

/// One process with its endpoints.
#[derive(Clone, Debug, PartialEq)]
pub struct Group {
    pub pid: u32,
    pub name: String,
    /// Endpoints of the process that passed the filters.
    pub count: usize,
    pub tcp: usize,
    pub udp: usize,
    /// Sum of the traffic counters (#8), when any row has them.
    pub sent: Option<u64>,
    pub received: Option<u64>,
    pub collapsed: bool,
    /// A row of the group (process icon and tooltip of the group line), index into the input.
    pub sample: usize,
}

/// A line of the grouped table.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Line {
    /// Index into the groups.
    Group(usize),
    /// Index into the rows of the view.
    Row(usize),
}

/// The grouped table: the rows still shown (collapsed groups left out), the groups, the lines,
/// and for every shown row its line.
#[derive(Default)]
pub struct Grouped {
    pub rows: Vec<Row>,
    pub groups: Vec<Group>,
    pub lines: Vec<Line>,
    pub line_of_row: Vec<usize>,
    /// The process of each group's sample row (the input is consumed).
    pub samples: Vec<Row>,
}

/// Groups the sorted rows by PID; `collapsed` holds the names of the collapsed processes.
pub fn group(rows: Vec<Row>, collapsed: &HashSet<String>) -> Grouped {
    let mut index: HashMap<u32, usize> = HashMap::new();
    let mut members: Vec<Vec<usize>> = Vec::new();
    let mut groups: Vec<Group> = Vec::new();
    for (i, r) in rows.iter().enumerate() {
        let g = *index.entry(r.conn.pid).or_insert_with(|| {
            groups.push(Group {
                pid: r.conn.pid,
                name: r.process.name.clone(),
                count: 0,
                tcp: 0,
                udp: 0,
                sent: None,
                received: None,
                collapsed: collapsed.contains(&r.process.name),
                sample: i,
            });
            members.push(Vec::new());
            groups.len() - 1
        });
        let gr = &mut groups[g];
        gr.count += 1;
        if r.conn.proto.is_tcp() {
            gr.tcp += 1;
        } else {
            gr.udp += 1;
        }
        if let Some(s) = r.conn.stats {
            gr.sent = Some(gr.sent.unwrap_or(0) + s.bytes_out);
            gr.received = Some(gr.received.unwrap_or(0) + s.bytes_in);
        }
        members[g].push(i);
    }
    let samples: Vec<Row> = groups.iter().map(|g| rows[g.sample].clone()).collect();
    let mut slots: Vec<Option<Row>> = rows.into_iter().map(Some).collect();
    let mut out = Grouped {
        rows: Vec::with_capacity(slots.len()),
        lines: Vec::with_capacity(slots.len() + groups.len()),
        line_of_row: Vec::with_capacity(slots.len()),
        groups: Vec::new(),
        samples,
    };
    for (g, m) in groups.iter().zip(&members) {
        out.lines.push(Line::Group(out.groups.len()));
        out.groups.push(g.clone());
        if g.collapsed {
            continue;
        }
        for &i in m {
            if let Some(r) = slots[i].take() {
                out.line_of_row.push(out.lines.len());
                out.lines.push(Line::Row(out.rows.len()));
                out.rows.push(r);
            }
        }
    }
    out
}

/// The table without groups: one line per row.
pub fn ungrouped(rows: Vec<Row>) -> Grouped {
    let n = rows.len();
    Grouped {
        rows,
        groups: Vec::new(),
        lines: (0..n).map(Line::Row).collect(),
        line_of_row: (0..n).collect(),
        samples: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Mark;
    use crate::net::estats::TcpStats;
    use crate::net::{Connection, Protocol, TcpState};
    use crate::process::ProcessInfo;
    use std::sync::Arc;

    fn row(pid: u32, name: &str, port: u16, stats: Option<(u64, u64)>) -> Row {
        Row {
            conn: Connection {
                proto: if port.is_multiple_of(2) {
                    Protocol::Tcp
                } else {
                    Protocol::Udp
                },
                local: format!("10.0.0.1:{port}").parse().unwrap(),
                remote: None,
                state: (port.is_multiple_of(2)).then_some(TcpState::Listen),
                pid,
                created: None,
                module: None,
                stats: stats.map(|(o, i)| TcpStats {
                    bytes_out: o,
                    bytes_in: i,
                    segs_out: 0,
                    segs_in: 0,
                }),
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

    fn ports(g: &Grouped) -> Vec<u16> {
        g.rows.iter().map(|r| r.conn.local.port()).collect()
    }

    #[test]
    fn groups_follow_the_order_of_their_first_row() {
        // Sorted by port: b(10) a(11) b(12) c(13) a(14)
        let rows = vec![
            row(2, "b.exe", 10, Some((5, 1))),
            row(1, "a.exe", 11, None),
            row(2, "b.exe", 12, Some((7, 2))),
            row(3, "c.exe", 13, None),
            row(1, "a.exe", 14, None),
        ];
        let g = group(rows, &HashSet::new());
        let names: Vec<&str> = g.groups.iter().map(|g| g.name.as_str()).collect();
        assert_eq!(names, ["b.exe", "a.exe", "c.exe"]);
        assert_eq!(ports(&g), [10, 12, 11, 14, 13]);
        assert_eq!(
            g.lines,
            [
                Line::Group(0),
                Line::Row(0),
                Line::Row(1),
                Line::Group(1),
                Line::Row(2),
                Line::Row(3),
                Line::Group(2),
                Line::Row(4),
            ]
        );
        assert_eq!(g.line_of_row, [1, 2, 4, 5, 7]);
        let b = &g.groups[0];
        assert_eq!((b.count, b.tcp, b.udp), (2, 2, 0));
        assert_eq!((b.sent, b.received), (Some(12), Some(3)));
        assert_eq!((g.groups[1].tcp, g.groups[1].udp), (1, 1));
        assert_eq!(g.groups[1].sent, None);
        assert_eq!(g.samples[2].process.name, "c.exe");
    }

    #[test]
    fn collapsed_groups_hide_their_rows() {
        let rows = vec![
            row(2, "b.exe", 10, None),
            row(1, "a.exe", 11, None),
            row(2, "b.exe", 12, None),
        ];
        let collapsed: HashSet<String> = ["b.exe".to_owned()].into();
        let g = group(rows, &collapsed);
        assert!(g.groups[0].collapsed && !g.groups[1].collapsed);
        assert_eq!(g.groups[0].count, 2);
        assert_eq!(ports(&g), [11]);
        assert_eq!(g.lines, [Line::Group(0), Line::Group(1), Line::Row(0)]);
        assert_eq!(g.line_of_row, [2]);
    }

    #[test]
    fn same_name_different_pid_are_two_groups() {
        let rows = vec![
            row(5, "svchost.exe", 10, None),
            row(6, "svchost.exe", 12, None),
        ];
        let g = group(rows, &HashSet::new());
        assert_eq!(g.groups.len(), 2);
        let u = ungrouped(vec![row(5, "x", 1, None), row(6, "y", 2, None)]);
        assert_eq!(u.lines, [Line::Row(0), Line::Row(1)]);
        assert_eq!(u.line_of_row, [0, 1]);
    }

    #[test]
    fn five_thousand_rows_of_three_hundred_processes() {
        let rows: Vec<Row> = (0..5000u32)
            .map(|i| {
                row(
                    i % 300,
                    &format!("p{}.exe", i % 300),
                    (i % 60000) as u16,
                    Some((1, 1)),
                )
            })
            .collect();
        let collapsed: HashSet<String> = (0..300).step_by(2).map(|i| format!("p{i}.exe")).collect();
        let start = std::time::Instant::now();
        let g = group(rows, &collapsed);
        let took = start.elapsed();
        assert_eq!(g.groups.len(), 300);
        assert_eq!(g.groups.iter().map(|g| g.count).sum::<usize>(), 5000);
        assert_eq!(g.rows.len() + g.groups.len(), g.lines.len());
        // Generous even for a debug build; the release build needs about a millisecond.
        assert!(took.as_millis() < 200, "{took:?}");
    }
}
