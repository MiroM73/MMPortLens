//! Several process properties windows (#33): one per PID, each in its own native window (egui
//! immediate viewport) that can be moved outside the main window and onto another monitor. When
//! the backend cannot open native windows (`Context::embed_viewports`) or the setting
//! `Config::properties_separate` is off, they are `egui::Window`s inside the main window as
//! before (#28). The content of a window is drawn by `properties.rs`.

use crate::app::properties::{ContentAction, Properties};
use crate::app::*;
use crate::process::ProcessInfo;

/// At most this many properties windows are open (a multi-selection must not open dozens).
pub(in crate::app) const MAX_WINDOWS: usize = 8;
/// Size of a new native window (logical pixels), its minimum, and the offset of the next one.
const DEFAULT_SIZE: [f32; 2] = [540.0, 680.0];
const MIN_SIZE: [f32; 2] = [380.0, 320.0];
const CASCADE: f32 = 28.0;

/// The PIDs to open from rows, in order, without duplicates and PID 0, at most `limit`;
/// `true` when some were left out because of the limit.
pub(in crate::app) fn distinct_pids<T: Clone>(
    rows: impl IntoIterator<Item = (u32, T)>,
    already_open: &[u32],
    limit: usize,
) -> (Vec<(u32, T)>, bool) {
    let mut out: Vec<(u32, T)> = Vec::new();
    let mut seen: Vec<u32> = Vec::new();
    let mut new_windows = 0;
    let mut cut = false;
    for (pid, v) in rows {
        if pid == 0 || seen.contains(&pid) {
            continue;
        }
        seen.push(pid);
        if !already_open.contains(&pid) {
            if new_windows >= limit {
                cut = true;
                continue;
            }
            new_windows += 1;
        }
        out.push((pid, v));
    }
    (out, cut)
}

impl App {
    /// Opens the properties of one process; an open window of the PID comes to the front.
    pub(in crate::app) fn open_properties(&mut self, pid: u32, info: Arc<ProcessInfo>) {
        self.open_properties_many(vec![(pid, info)]);
    }

    /// Opens one window per distinct PID (up to [`MAX_WINDOWS`] open windows); the PIDs that
    /// already have a window are focused.
    pub(in crate::app) fn open_properties_many(&mut self, rows: Vec<(u32, Arc<ProcessInfo>)>) {
        let open: Vec<u32> = self.properties.iter().map(|p| p.pid).collect();
        let free = MAX_WINDOWS.saturating_sub(open.len());
        let (list, cut) = distinct_pids(rows, &open, free);
        for (pid, info) in list {
            if let Some(p) = self.properties.iter_mut().find(|p| p.pid == pid) {
                p.focus = true;
            } else {
                let mut p = Properties::new(pid, info);
                p.focus = true;
                self.properties.push(p);
            }
        }
        if cut {
            self.action_result = Some(actions::ActionResult {
                text: t!("properties.limit", max = MAX_WINDOWS),
                error: true,
            });
        }
    }

    /// Alt+Enter: the properties of the processes of the selected rows (the cursor row when
    /// nothing is selected).
    pub(in crate::app) fn open_properties_of_cursor(&mut self) {
        if self.replaying() {
            return;
        }
        let mut rows: Vec<(u32, Arc<ProcessInfo>)> = self
            .selected_rows()
            .into_iter()
            .map(|r| (r.conn.pid, r.process.clone()))
            .collect();
        if rows.is_empty()
            && let Some(r) = self.details_row()
        {
            rows.push((r.conn.pid, r.process.clone()));
        }
        self.open_properties_many(rows);
    }

    /// Esc in the main window closes the properties window opened last; returns true when there
    /// was one.
    pub(in crate::app) fn close_properties(&mut self) -> bool {
        match self.properties.pop() {
            Some(p) => {
                self.remember_properties_geometry(&p);
                true
            }
            None => false,
        }
    }

    /// Keeps the position and size of a closed native window for the next one.
    fn remember_properties_geometry(&mut self, p: &Properties) {
        if let Some(g) = &p.geometry
            && self.cfg.properties_window.as_ref() != Some(g)
        {
            self.cfg.properties_window = Some(g.clone());
            self.settings_changed();
        }
    }

    /// Draws every properties window and carries out what was clicked in them.
    pub(in crate::app) fn properties_windows(&mut self, ctx: &egui::Context) {
        self.refresh_properties();
        if self.properties.is_empty() {
            return;
        }
        let native = self.cfg.properties_separate && !ctx.embed_viewports();
        let mut windows = std::mem::take(&mut self.properties);
        let mut actions: Vec<ContentAction> = Vec::with_capacity(windows.len());
        for (i, p) in windows.iter_mut().enumerate() {
            let act = if native {
                self.native_properties(ctx, p, i)
            } else {
                self.embedded_properties(ctx, p, i)
            };
            actions.push(act);
        }
        let mut kill: Option<(u32, Arc<ProcessInfo>)> = None;
        let mut keep = Vec::with_capacity(windows.len());
        for (p, act) in windows.into_iter().zip(actions) {
            if let Some(text) = act.failed {
                self.action_result = Some(actions::ActionResult { text, error: true });
            }
            if act.kill {
                kill = Some((p.pid, p.info.clone()));
            }
            if act.close {
                self.remember_properties_geometry(&p);
            } else {
                keep.push(p);
            }
        }
        // Windows opened while drawing (none today) come after the kept ones.
        keep.append(&mut self.properties);
        self.properties = keep;
        if let Some((pid, info)) = kill {
            self.request_kill_process(pid, info);
            // The confirmation is in the main window.
            if native {
                ctx.send_viewport_cmd_to(egui::ViewportId::ROOT, egui::ViewportCommand::Focus);
            }
        }
    }

    /// One properties window in its own native window.
    fn native_properties(
        &mut self,
        ctx: &egui::Context,
        p: &mut Properties,
        index: usize,
    ) -> ContentAction {
        let mut builder = egui::ViewportBuilder::default()
            .with_title(p.title())
            .with_icon(Arc::new(icon_data()))
            .with_min_inner_size(MIN_SIZE);
        // A new window: the remembered place and size, the next ones cascaded; then the same
        // values in every frame.
        let opened = p.opened_at.get_or_insert_with(|| {
            let saved = self.cfg.properties_window.clone().unwrap_or_default();
            let off = CASCADE * index as f32;
            update::WindowState {
                pos: saved.pos.map(|[x, y]| [x + off, y + off]),
                size: Some(saved.size.unwrap_or(DEFAULT_SIZE)),
                maximized: false,
            }
        });
        builder = builder.with_inner_size(opened.size.unwrap_or(DEFAULT_SIZE));
        if let Some(pos) = opened.pos {
            builder = builder.with_position(pos);
        }
        if p.focus {
            p.focus = false;
            ctx.send_viewport_cmd_to(p.viewport, egui::ViewportCommand::Focus);
        }
        let viewport = p.viewport;
        ctx.show_viewport_immediate(viewport, builder, |ui, _class| {
            let ctx = ui.ctx().clone();
            let mut act = ContentAction::default();
            egui::CentralPanel::default().show(ui, |ui| {
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| act = self.properties_content(ui, p, None));
            });
            // Not inside `input`: the context is locked there (a deadlock).
            let zoom = ctx.zoom_factor();
            let (close_requested, esc, geometry) = ctx.input(|i| {
                let vp = i.viewport();
                (
                    vp.close_requested(),
                    i.key_pressed(egui::Key::Escape),
                    window_geometry(vp, zoom),
                )
            });
            if let Some(g) = geometry {
                p.geometry = Some(g);
            }
            if close_requested || esc {
                act.close = true;
            }
            act
        })
    }

    /// One properties window inside the main window (the backend has no native windows, or the
    /// setting is off).
    fn embedded_properties(
        &mut self,
        ctx: &egui::Context,
        p: &mut Properties,
        index: usize,
    ) -> ContentAction {
        let mut open = true;
        let mut act = ContentAction::default();
        let id = egui::Id::unique("properties_window").with(p.pid);
        if p.focus {
            p.focus = false;
            ctx.move_to_top(egui::LayerId::new(egui::Order::Middle, id));
        }
        let off = CASCADE * index as f32;
        egui::Window::new(p.title())
            .id(id)
            .open(&mut open)
            .collapsible(false)
            .resizable(true)
            .default_width(520.0)
            .default_pos([80.0 + off, 80.0 + off])
            .show(ctx, |ui| {
                let h = ctx.content_rect().height() * 0.7;
                act = self.properties_content(ui, p, Some(h));
            });
        if !open {
            act.close = true;
        }
        act
    }
}

/// The position and size of a native window in logical pixels (egui points include the zoom);
/// `None` while minimized or before the size is known. A maximized window keeps the last normal
/// geometry (not stored).
fn window_geometry(vp: &egui::ViewportInfo, zoom: f32) -> Option<update::WindowState> {
    if vp.minimized == Some(true) || vp.maximized == Some(true) {
        return None;
    }
    let inner = vp.inner_rect?;
    Some(update::WindowState {
        pos: vp.outer_rect.map(|r| [r.min.x * zoom, r.min.y * zoom]),
        size: Some([inner.width() * zoom, inner.height() * zoom]),
        maximized: false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn distinct_pids_with_limit() {
        let rows = [(4, 'a'), (0, 'z'), (4, 'b'), (7, 'c'), (9, 'd'), (7, 'e')];
        // Duplicates and PID 0 dropped, order kept.
        let (list, cut) = distinct_pids(rows, &[], 8);
        assert_eq!(list, vec![(4, 'a'), (7, 'c'), (9, 'd')]);
        assert!(!cut);
        // Room for one new window: the open PID 7 still counts (focused), 9 is cut.
        let (list, cut) = distinct_pids(rows, &[7], 1);
        assert_eq!(list, vec![(4, 'a'), (7, 'c')]);
        assert!(cut);
        // No room: only the open ones.
        let (list, cut) = distinct_pids(rows, &[9], 0);
        assert_eq!(list, vec![(9, 'd')]);
        assert!(cut);
    }

    #[test]
    fn geometry_in_logical_pixels() {
        let vp = egui::ViewportInfo {
            outer_rect: Some(egui::Rect::from_min_size(
                egui::pos2(100.0, 50.0),
                egui::vec2(410.0, 330.0),
            )),
            inner_rect: Some(egui::Rect::from_min_size(
                egui::pos2(105.0, 80.0),
                egui::vec2(400.0, 300.0),
            )),
            ..Default::default()
        };
        let g = window_geometry(&vp, 1.5).unwrap();
        assert_eq!(g.pos, Some([150.0, 75.0]));
        assert_eq!(g.size, Some([600.0, 450.0]));
        let min = egui::ViewportInfo {
            minimized: Some(true),
            ..vp
        };
        assert_eq!(window_geometry(&min, 1.0), None);
    }
}
