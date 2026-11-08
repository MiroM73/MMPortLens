//! The process properties window (#28): opened from the row context menu (Properties…) or with
//! Alt+Enter, it shows the details of one process – the same values as the details pane (#7) –
//! and stays on its PID, independent of the selection. When the process exits, the last known
//! values stay with the note "(process has exited)".
//!
//! The values come from the poller's `ProcessCache` through the rows of the tracker; the window
//! only checks the start time of its PID (at most once a second) when no row is left. The live
//! traffic / I/O graph below the values is in `properties_graph.rs` (#30). Several windows, one per
//! PID, each in its own OS window (#33): `properties_windows.rs`; this file holds the state of one
//! window and draws its content.

use crate::app::*;
use crate::process::ProcessInfo;

/// How often a PID without rows is checked for having exited.
const EXIT_CHECK: Duration = Duration::from_secs(1);

/// The state of the properties window.
pub(in crate::app) struct Properties {
    pub(in crate::app) pid: u32,
    /// The last known values (kept after the process exits).
    pub(in crate::app) info: Arc<ProcessInfo>,
    /// TCP / UDP endpoints of the process in the table (closed rows not counted).
    tcp: usize,
    udp: usize,
    pub(in crate::app) exited: bool,
    checked: Option<Instant>,
    /// Live TCP / I/O rates for the graph (#30).
    pub(in crate::app) sampler: crate::rates::Sampler,
    /// The native window of this PID (#33).
    pub(in crate::app) viewport: egui::ViewportId,
    /// Bring the window to the front in the next frame (opened again).
    pub(in crate::app) focus: bool,
    /// Position and size of the native window in the last frame (logical pixels), remembered
    /// when it closes.
    pub(in crate::app) geometry: Option<update::WindowState>,
    /// The position and size the native window was opened with, fixed at its first frame (a
    /// changed builder would move the open window).
    pub(in crate::app) opened_at: Option<update::WindowState>,
}

impl Properties {
    pub(in crate::app) fn new(pid: u32, info: Arc<ProcessInfo>) -> Self {
        Self {
            pid,
            info,
            tcp: 0,
            udp: 0,
            exited: false,
            checked: None,
            sampler: Default::default(),
            viewport: egui::ViewportId::from_hash_of(("properties", pid)),
            focus: false,
            geometry: None,
            opened_at: None,
        }
    }

    /// The window title: "Process properties – name" (and the exited mark).
    pub(in crate::app) fn title(&self) -> String {
        if self.exited {
            format!(
                "{} – {} {}",
                t!("properties.title"),
                self.info.name,
                t!("properties.exited")
            )
        } else {
            format!("{} – {}", t!("properties.title"), self.info.name)
        }
    }
}

/// What a click in a properties window asks for.
#[derive(Debug, Default, PartialEq, Eq)]
pub(in crate::app) struct ContentAction {
    pub(in crate::app) kill: bool,
    pub(in crate::app) close: bool,
    pub(in crate::app) failed: Option<String>,
}

/// What the window says about its process when it has no endpoint in the table.
#[derive(Debug, PartialEq, Eq)]
pub(in crate::app) enum Liveness {
    /// The process has rows in the table.
    Rows,
    /// No rows, but the process with the same start time still runs.
    NoRows,
    Exited,
}

/// Decides whether a process without rows has exited: its PID no longer opens, or the PID now
/// belongs to a process with another start time. `known` is the start time the window knows,
/// `now` the one read again (`None` = cannot be opened).
pub(in crate::app) fn liveness(rows: usize, known: Option<u64>, now: Option<u64>) -> Liveness {
    if rows > 0 {
        return Liveness::Rows;
    }
    match (known, now) {
        (Some(k), Some(n)) if k == n => Liveness::NoRows,
        // Without a known start time (access denied) the PID cannot be checked.
        (None, _) => Liveness::NoRows,
        _ => Liveness::Exited,
    }
}

impl App {
    /// Takes the newest values and the endpoint counts of every window's PID from the tracker.
    pub(in crate::app) fn refresh_properties(&mut self) {
        let mut windows = std::mem::take(&mut self.properties);
        for p in &mut windows {
            self.refresh_one(p);
        }
        self.properties = windows;
    }

    fn refresh_one(&self, p: &mut Properties) {
        let (mut tcp, mut udp) = (0, 0);
        let mut newest: Option<&Arc<ProcessInfo>> = None;
        for r in self.tracker.rows() {
            if r.conn.pid != p.pid || r.mark == Mark::Closed {
                continue;
            }
            // A reused PID belongs to another process: not this window's rows.
            if p.info.started.is_some() && r.process.started != p.info.started {
                continue;
            }
            if r.conn.proto.is_tcp() {
                tcp += 1;
            } else {
                udp += 1;
            }
            newest = Some(&r.process);
        }
        p.tcp = tcp;
        p.udp = udp;
        if let Some(info) = newest {
            p.info = info.clone();
            p.exited = false;
            p.checked = None;
            return;
        }
        if p.exited || p.checked.is_some_and(|t| t.elapsed() < EXIT_CHECK) {
            return;
        }
        p.checked = Some(Instant::now());
        let now = crate::process::start_time(p.pid);
        p.exited = liveness(0, p.info.started, now) == Liveness::Exited;
    }

    /// The values of the window as text (Copy button).
    fn properties_text(p: &Properties, stats_denied: bool) -> String {
        let info = &p.info;
        let mut lines = vec![info.name.clone()];
        if !info.description.is_empty() {
            lines.push(info.description.clone());
        }
        lines.push(format!("{}: {}", t!("column.pid"), p.pid));
        for (name, value) in details::process_values(info) {
            if !value.is_empty() {
                lines.push(format!("{name}: {}", value.replace('\n', ", ")));
            }
        }
        lines.push(t!("properties.endpoints", tcp = p.tcp, udp = p.udp));
        lines.extend(properties_graph::traffic_text(&p.sampler, stats_denied));
        lines.join("\r\n")
    }

    /// The content of one properties window: process values, counts, graph and buttons.
    /// `grid_height` limits the process values (an embedded window); `None` = unlimited (the
    /// native window scrolls as a whole).
    pub(in crate::app) fn properties_content(
        &self,
        ui: &mut egui::Ui,
        p: &Properties,
        grid_height: Option<f32>,
    ) -> ContentAction {
        let mut act = ContentAction::default();
        let id = format!("properties_process_{}", p.pid);
        match grid_height {
            Some(h) => {
                egui::ScrollArea::vertical()
                    .id_salt(("properties_values", p.pid))
                    .max_height(h)
                    .show(ui, |ui| self.process_grid(ui, &p.info, &id));
            }
            None => self.process_grid(ui, &p.info, &id),
        }
        ui.separator();
        egui::Grid::new(("properties_counts", p.pid))
            .num_columns(2)
            .spacing([12.0, 3.0])
            .show(ui, |ui| {
                details::detail_row(ui, &t!("column.pid"), &p.pid.to_string());
                details::detail_row(
                    ui,
                    &t!("properties.endpoints.label"),
                    &t!("properties.endpoints", tcp = p.tcp, udp = p.udp),
                );
            });
        if p.exited {
            ui.colored_label(ui.visuals().warn_fg_color, t!("properties.exited.text"));
        } else if p.tcp + p.udp == 0 {
            ui.label(RichText::new(t!("properties.no_rows")).weak());
        }
        ui.separator();
        ui.push_id(("properties_graph", p.pid), |ui| {
            properties_graph::traffic_graph(ui, &p.sampler, self.stats_denied);
        });
        ui.separator();
        let path = &p.info.path;
        ui.horizontal_wrapped(|ui| {
            if ui.button(format!("📋 {}", t!("properties.copy"))).clicked() {
                ui.ctx()
                    .copy_text(Self::properties_text(p, self.stats_denied));
            }
            let has_path = !path.is_empty();
            if ui
                .add_enabled(
                    has_path,
                    egui::Button::new(format!("📂 {}", t!("properties.open_location"))),
                )
                .clicked()
                && let Err(e) = export::show_in_folder(std::path::Path::new(path))
            {
                act.failed = Some(e.to_string());
            }
            #[cfg(windows)]
            if ui
                .add_enabled(
                    has_path,
                    egui::Button::new(t!("properties.file_properties")),
                )
                .clicked()
                && !crate::winproc::file_properties(path)
            {
                act.failed = Some(t!("properties.file_properties.failed"));
            }
            let blocked = Self::kill_blocked(p.pid);
            if ui
                .add_enabled(
                    !p.exited && blocked.is_none(),
                    egui::Button::new(
                        RichText::new(t!("action.kill")).color(ui.visuals().warn_fg_color),
                    ),
                )
                .on_disabled_hover_text(blocked.unwrap_or_default())
                .clicked()
            {
                act.kill = true;
            }
            if ui.button(t!("properties.close")).clicked() {
                act.close = true;
            }
        });
        act
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exit_detection() {
        assert_eq!(liveness(2, Some(5), None), Liveness::Rows);
        // Same process still running without endpoints.
        assert_eq!(liveness(0, Some(5), Some(5)), Liveness::NoRows);
        // PID gone, or reused by a process with another start time.
        assert_eq!(liveness(0, Some(5), None), Liveness::Exited);
        assert_eq!(liveness(0, Some(5), Some(9)), Liveness::Exited);
        // Start time unknown (access denied): cannot be told.
        assert_eq!(liveness(0, None, Some(9)), Liveness::NoRows);
        assert_eq!(liveness(0, None, None), Liveness::NoRows);
    }
}
