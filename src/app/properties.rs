//! The process properties window (#28): opened from the row context menu (Properties…) or with
//! Alt+Enter, it shows the details of one process – the same values as the details pane (#7) –
//! and stays on its PID, independent of the selection. When the process exits, the last known
//! values stay with the note "(process has exited)".
//!
//! The values come from the poller's `ProcessCache` through the rows of the tracker; the window
//! only checks the start time of its PID (at most once a second) when no row is left. The live
//! traffic / I/O graph below the values is in `properties_graph.rs` (#30).

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
    /// Opens the window for `pid` (reuses an open window).
    pub(in crate::app) fn open_properties(&mut self, pid: u32, info: Arc<ProcessInfo>) {
        if pid == 0 {
            return;
        }
        self.properties = Some(Properties {
            pid,
            info,
            tcp: 0,
            udp: 0,
            exited: false,
            checked: None,
            sampler: Default::default(),
        });
    }

    /// Alt+Enter: the properties of the process of the cursor row.
    pub(in crate::app) fn open_properties_of_cursor(&mut self) {
        if self.replaying() {
            return;
        }
        if let Some(r) = self.details_row() {
            let (pid, info) = (r.conn.pid, r.process.clone());
            self.open_properties(pid, info);
        }
    }

    /// Esc closes the window; returns true when it was open.
    pub(in crate::app) fn close_properties(&mut self) -> bool {
        self.properties.take().is_some()
    }

    /// Takes the newest values and the endpoint counts of the window's PID from the tracker.
    fn refresh_properties(&mut self) {
        let Some(p) = &mut self.properties else {
            return;
        };
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

    pub(in crate::app) fn properties_window(&mut self, ctx: &egui::Context) {
        self.refresh_properties();
        let Some(p) = &self.properties else {
            return;
        };
        let mut open = true;
        let mut kill = false;
        let mut close = false;
        let mut failed: Option<String> = None;
        let path = p.info.path.clone();
        let title = if p.exited {
            format!(
                "{} – {} {}",
                t!("properties.title"),
                p.info.name,
                t!("properties.exited")
            )
        } else {
            format!("{} – {}", t!("properties.title"), p.info.name)
        };
        egui::Window::new(title)
            .id(egui::Id::unique("properties_window"))
            .open(&mut open)
            .collapsible(false)
            .resizable(true)
            .default_width(520.0)
            .show(ctx, |ui| {
                egui::ScrollArea::vertical()
                    .max_height(ctx.content_rect().height() * 0.7)
                    .show(ui, |ui| {
                        self.process_grid(ui, &p.info, "properties_process");
                    });
                ui.separator();
                egui::Grid::new("properties_counts")
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
                properties_graph::traffic_graph(ui, &p.sampler, self.stats_denied);
                ui.separator();
                ui.horizontal_wrapped(|ui| {
                    if ui.button(format!("📋 {}", t!("properties.copy"))).clicked() {
                        ctx.copy_text(Self::properties_text(p, self.stats_denied));
                    }
                    let has_path = !path.is_empty();
                    if ui
                        .add_enabled(
                            has_path,
                            egui::Button::new(format!("📂 {}", t!("properties.open_location"))),
                        )
                        .clicked()
                        && let Err(e) = export::show_in_folder(std::path::Path::new(&path))
                    {
                        failed = Some(e.to_string());
                    }
                    #[cfg(windows)]
                    if ui
                        .add_enabled(
                            has_path,
                            egui::Button::new(t!("properties.file_properties")),
                        )
                        .clicked()
                        && !crate::winproc::file_properties(&path)
                    {
                        failed = Some(t!("properties.file_properties.failed"));
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
                        kill = true;
                    }
                    if ui.button(t!("properties.close")).clicked() {
                        close = true;
                    }
                });
            });
        if let Some(text) = failed {
            self.action_result = Some(actions::ActionResult { text, error: true });
        }
        if kill && let Some(p) = &self.properties {
            let (pid, info) = (p.pid, p.info.clone());
            self.request_kill_process(pid, info);
        }
        if !open || close {
            self.properties = None;
        }
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
