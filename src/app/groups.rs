//! Group rows by process (#12): the group line of the table, collapsing / expanding groups and
//! the switches in the toolbar and the View menu. The grouping itself is in `crate::grouping`.

use crate::app::*;
use crate::columns::bytes;

impl App {
    /// Draws the group line `g` across the row; returns true when it was clicked (collapse /
    /// expand).
    pub(in crate::app) fn group_line(
        &self,
        row: &mut egui_extras::TableRow<'_, '_>,
        g: usize,
        cols: &[ColumnId],
        row_h: f32,
        gap: egui::Vec2,
        table: egui::Rect,
    ) -> bool {
        let group = &self.groups[g];
        let sample = self.group_samples.get(g);
        let last = cols.len().saturating_sub(1);
        // Where the text starts (set in the first cell, painted in the last one).
        let mut text_at: Option<egui::Pos2> = None;
        for (n, _) in cols.iter().enumerate() {
            row.col(|ui| {
                // Opaque, so the overlapping cell rects leave no stripes.
                let v = ui.visuals();
                let tint = v.extreme_bg_color.lerp_to_gamma(v.selection.bg_fill, 0.35);
                let arrow = v.strong_text_color();
                ui.painter()
                    .rect_filled(ui.max_rect().expand2(gap), 0.0, tint);
                if n == 0 {
                    text_at = Some(self.group_marker(ui, group, sample, row_h, arrow));
                }
                // Painted, not a label, after every cell's background: the text reaches over the
                // empty cells of the group line without widening the first column (its width
                // is remembered, #2). It is clipped to the visible table.
                if n == last
                    && let Some(at) = text_at
                {
                    let rect = ui.max_rect();
                    let line = egui::Rect::from_x_y_ranges(table.x_range(), rect.y_range());
                    let mut painter = ui.painter().clone();
                    painter.set_clip_rect(line);
                    painter.text(
                        at,
                        egui::Align2::LEFT_CENTER,
                        group_text(group),
                        egui::TextStyle::Body.resolve(ui.style()),
                        arrow,
                    );
                }
            });
        }
        let mut resp = row.response();
        if let Some(r) = sample {
            let tip = details::process_tip(&r.process);
            if !tip.is_empty() {
                resp = resp.on_hover_text(tip);
            }
        }
        resp.clicked()
    }

    /// The triangle and the process icon in the first cell of a group line; returns where the
    /// text starts.
    fn group_marker(
        &self,
        ui: &mut egui::Ui,
        group: &crate::grouping::Group,
        sample: Option<&Row>,
        row_h: f32,
        color: Color32,
    ) -> egui::Pos2 {
        // The triangle is painted: ▶ / ▼ and similar characters are empty boxes in egui's
        // fonts (#22).
        let rect = ui.max_rect();
        let c = egui::pos2(rect.left() + row_h * 0.3, rect.center().y);
        let s = row_h * 0.2;
        let dir = if group.collapsed {
            egui::Vec2::X
        } else {
            egui::Vec2::Y
        };
        crate::app::table::paint_triangle(ui.painter(), c, s, dir, color);
        let mut x = rect.left() + row_h * 0.7;
        if let Some(tex) = sample.and_then(|r| self.icon_texture(&r.process)) {
            let size = row_h - 6.0;
            let img = egui::Rect::from_min_size(
                egui::pos2(x, rect.center().y - size / 2.0),
                egui::vec2(size, size),
            );
            egui::Image::new(tex).paint_at(ui, img);
            x += size + 4.0;
        }
        egui::pos2(x, rect.center().y)
    }

    /// Collapses or expands group `g` (remembered by process name).
    pub(in crate::app) fn toggle_group(&mut self, g: usize) {
        let Some(group) = self.groups.get(g) else {
            return;
        };
        let name = group.name.clone();
        let list = &mut self.cfg.collapsed_processes;
        match list.iter().position(|n| *n == name) {
            Some(i) => {
                list.remove(i);
            }
            None => list.push(name),
        }
        self.view_dirty = true;
        self.settings_changed();
    }

    /// Collapses all groups shown, or expands all.
    fn set_all_groups(&mut self, collapsed: bool) {
        if collapsed {
            for g in &self.groups {
                if !self.cfg.collapsed_processes.contains(&g.name) {
                    self.cfg.collapsed_processes.push(g.name.clone());
                }
            }
        } else {
            self.cfg.collapsed_processes.clear();
        }
        self.view_dirty = true;
        self.settings_changed();
    }

    /// The "Group by process" switch of the toolbar.
    pub(in crate::app) fn group_toggle(&mut self, ui: &mut egui::Ui) {
        if ui
            .toggle_value(&mut self.cfg.group_by_process, t!("toolbar.group"))
            .on_hover_text(t!("toolbar.group.tip"))
            .changed()
        {
            self.view_dirty = true;
            self.settings_changed();
        }
    }

    /// The items of the View menu.
    pub(in crate::app) fn group_menu_items(&mut self, ui: &mut egui::Ui) {
        if ui
            .checkbox(&mut self.cfg.group_by_process, t!("menu.view.group"))
            .changed()
        {
            self.view_dirty = true;
            self.settings_changed();
        }
        let grouped = self.cfg.group_by_process;
        if ui
            .add_enabled(grouped, egui::Button::new(t!("menu.view.expand_all")))
            .clicked()
        {
            ui.close();
            self.set_all_groups(false);
        }
        if ui
            .add_enabled(grouped, egui::Button::new(t!("menu.view.collapse_all")))
            .clicked()
        {
            ui.close();
            self.set_all_groups(true);
        }
    }
}

/// "chrome.exe (PID 1234) – 5 endpoints: 4 TCP, 1 UDP – ↑ 1.2 MB ↓ 3.4 MB".
fn group_text(g: &crate::grouping::Group) -> String {
    let name = if g.name.is_empty() {
        t!("group.unknown")
    } else {
        g.name.clone()
    };
    let mut s = t!(
        "group.line",
        name = name,
        pid = g.pid,
        count = g.count,
        tcp = g.tcp,
        udp = g.udp
    );
    if let (Some(out), Some(inc)) = (g.sent, g.received) {
        s.push_str(&format!("   ↑ {}  ↓ {}", bytes(out), bytes(inc)));
    }
    s
}
