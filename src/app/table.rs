//! The connection table: visible columns in the user's order, sorting by a header click,
//! the column menu on a header right click, the column filter row (#1), highlighted new / changed /
//! closed rows, horizontal scrolling when the columns do not fit (#27), headers dragged to a new
//! place and remembered widths (#2, in `columnlayout.rs`).

use crate::app::*;
use crate::columns::{MAX_WIDTH, MIN_WIDTH};
use crate::grouping::Line;
use egui_extras::{Column, TableBuilder};

impl App {
    pub(in crate::app) fn table(&mut self, ui: &mut egui::Ui) {
        let cols = self.cfg.columns.visible();
        let dark = ui.visuals().dark_mode;
        let row_h = ui.text_style_height(&egui::TextStyle::Body) + 4.0;
        // How far a row color reaches past its cell, so a marked row has no gaps between cells.
        let gap = egui::vec2(
            ui.spacing().item_spacing.x,
            ui.spacing().item_spacing.y * 0.5 + 0.5,
        );

        // Horizontal scrolling (#27, same as RusTrace): egui_extras tables only scroll vertically,
        // so the table is laid out in a child Ui shifted left by the offset and clipped to the
        // visible area; its right edge with the vertical scroll bar stays in view. A horizontal
        // scroll bar below it appears when the columns do not fit (last frame's natural width);
        // Shift + mouse wheel and horizontal touchpad scrolling work as well.
        let parent = ui;
        let full = parent.available_rect_before_wrap();
        let max_offset = (self.table_width - full.width()).max(0.0);
        let hbar_h = if max_offset > 0.0 {
            parent.spacing().scroll.bar_width + 4.0
        } else {
            0.0
        };
        let view = egui::Rect::from_min_max(full.min, egui::pos2(full.max.x, full.max.y - hbar_h));
        if parent.rect_contains_pointer(view) {
            self.table_scroll_x -= parent.input(|i| i.smooth_scroll_delta.x);
        }
        self.table_scroll_x = self.table_scroll_x.clamp(0.0, max_offset);
        let mut child =
            parent.new_child(egui::UiBuilder::new().max_rect(egui::Rect::from_min_max(
                egui::pos2(view.min.x - self.table_scroll_x, view.min.y),
                view.max,
            )));
        child.set_clip_rect(view.intersect(parent.clip_rect()));
        let ui = &mut child;

        let header_h = if self.cfg.show_column_filters {
            22.0 + ui.spacing().interact_size.y + ui.spacing().item_spacing.y
        } else {
            22.0
        };
        // The widths come from the settings (#2): applied when the visible columns change (the
        // table would otherwise keep the widths by position) or after a fit / reset; in between
        // the table keeps the widths the user drags, which are written back after drawing.
        let apply_widths = std::mem::take(&mut self.apply_widths) || self.table_cols != cols;
        if apply_widths {
            self.table_cols = cols.clone();
        }
        let mut tb = TableBuilder::new(ui)
            .id_salt("conn_table")
            .striped(true)
            .resizable(true)
            .sense(Sense::click())
            .auto_shrink([false, false])
            .cell_layout(egui::Layout::left_to_right(egui::Align::Center));
        if let Some(pos) = self.scroll_to.take() {
            let line = self.line_of_row.get(pos).copied().unwrap_or(pos);
            tb = tb.scroll_to_row(line, None);
        }
        for (i, c) in cols.iter().enumerate() {
            let col = if i + 1 == cols.len() {
                Column::remainder().at_least(LAST_COLUMN_MIN)
            } else {
                Column::initial(self.cfg.columns.width(*c)).range(MIN_WIDTH..=MAX_WIDTH)
            };
            // No clipping: labels truncate themselves, and the row color must reach into the gap
            // between the cells.
            tb = tb.column(col.clip(false));
        }

        let sort = self.cfg.sort;
        let mut clicked_header: Option<ColumnId> = None;
        let mut open_columns_menu = false;
        // Row position and whether it was a right click (which keeps a selection containing it).
        let mut clicked_row: Option<(usize, bool)> = None;
        let mut copy = false;
        let mut action: Option<actions::MenuAction> = None;
        let mut properties: Option<(u32, Arc<crate::process::ProcessInfo>)> = None;
        let mut toggled: Option<usize> = None;

        // Filter row under the headers: one field per visible column (edited in a copy, applied
        // after the table is drawn).
        let show_filters = self.cfg.show_column_filters;
        let mut filter_texts: Vec<String> = cols
            .iter()
            .map(|c| self.col_filter_text.get(c).cloned().unwrap_or_default())
            .collect();
        let mut filters_changed = false;
        // The header cell rects: natural table width (#27), widths, drop places (#2).
        let mut cells = vec![egui::Rect::NOTHING; cols.len()];
        let mut drag_started: Option<ColumnId> = None;
        if apply_widths {
            tb.reset();
        }

        tb.header(header_h, |mut h| {
            for (i, &c) in cols.iter().enumerate() {
                h.col(|ui| {
                    cells[i] = ui.max_rect();
                    ui.vertical(|ui| {
                        let arrow = match (sort.column == c, sort.ascending) {
                            (true, true) => " ⏶",
                            (true, false) => " ⏷",
                            _ => "",
                        };
                        let label = ui.add(
                            egui::Label::new(
                                RichText::new(format!("{}{arrow}", c.title())).strong(),
                            )
                            .truncate()
                            .selectable(false),
                        );
                        // The whole width of the title line: a click sorts, a drag moves the
                        // column (#2).
                        let title = egui::Rect::from_x_y_ranges(
                            ui.max_rect().x_range(),
                            label.rect.y_range(),
                        );
                        let resp = ui.interact(
                            title,
                            ui.scope_id().with("title"),
                            Sense::click_and_drag(),
                        );
                        if resp.clicked() {
                            clicked_header = Some(c);
                        }
                        if resp.drag_started() {
                            drag_started = Some(c);
                        }
                        if resp.secondary_clicked() {
                            open_columns_menu = true;
                        }
                        resp.on_hover_text(t!("table.header.tip"));
                        if show_filters {
                            filters_changed |=
                                self.column_filter_field(ui, c, &mut filter_texts[i]);
                        }
                    });
                });
            }
        })
        .body(|body| {
            body.rows(row_h, self.lines.len(), |mut row| {
                // A group line of #12, or the position of the row in the view.
                let i = match self.lines[row.index()] {
                    Line::Group(g) => {
                        if self.group_line(&mut row, g, &cols, row_h, gap, view) {
                            toggled = Some(g);
                        }
                        return;
                    }
                    Line::Row(i) => i,
                };
                let r = &self.view[i];
                let key = r.conn.key();
                row.set_selected(self.sel.contains(&key));
                let (fg, bg) = self.row_colors(i, r, dark);
                for &c in &cols {
                    row.col(|ui| {
                        if let Some(bg) = bg {
                            ui.painter()
                                .rect_filled(ui.max_rect().expand2(gap), 0.0, bg);
                        }
                        let mut text = RichText::new(cell_text(r, c));
                        if let Some(fg) = fg {
                            text = text.color(fg);
                        }
                        let label = egui::Label::new(text).truncate().selectable(false);
                        if c.numeric() {
                            let resp = ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| ui.add(label),
                            );
                            // The service name of a well-known port (#5).
                            let port = match c {
                                ColumnId::LocalPort => Some(r.conn.local.port()),
                                ColumnId::RemotePort => r.conn.remote.map(|a| a.port()),
                                _ => None,
                            };
                            if let Some(name) =
                                port.and_then(|p| crate::services::name(p, r.conn.proto))
                            {
                                resp.inner.on_hover_text(name);
                            }
                        } else if c == ColumnId::Process {
                            // Icon + name; the tooltip says what the process is (#7).
                            if let Some(tex) = self.icon_texture(&r.process) {
                                ui.add(
                                    egui::Image::new(tex)
                                        .fit_to_exact_size(egui::vec2(row_h - 6.0, row_h - 6.0)),
                                );
                            }
                            let resp = ui.add(label);
                            let tip = details::process_tip(&r.process);
                            if !tip.is_empty() {
                                resp.on_hover_text(tip);
                            }
                        } else {
                            let resp = ui.add(label);
                            if c == ColumnId::State
                                && let Some(TcpState::Unknown(v)) = r.conn.state
                            {
                                resp.on_hover_text(t!("state.invalid.tip", value = v));
                            }
                        }
                    });
                }
                let resp = row.response();
                if resp.clicked() {
                    clicked_row = Some((i, false));
                }
                if resp.secondary_clicked() {
                    clicked_row = Some((i, true));
                }
                resp.context_menu(|ui| {
                    if ui
                        .add(
                            egui::Button::new(t!("table.copy_rows"))
                                .shortcut_text(help::kb("Ctrl+C")),
                        )
                        .clicked()
                    {
                        copy = true;
                        ui.close();
                    }
                    // The process of the right-clicked row, not of the selection (#28).
                    if ui
                        .add_enabled(
                            r.conn.pid != 0,
                            egui::Button::new(t!("table.properties"))
                                .shortcut_text(help::kb("Alt+Enter")),
                        )
                        .clicked()
                    {
                        properties = Some((r.conn.pid, r.process.clone()));
                        ui.close();
                    }
                    ui.separator();
                    action = action.or(self.action_menu_items(ui));
                });
            });
        });

        // Natural width: all columns up to the last one plus its minimum width (the last column
        // fills the rest when everything fits).
        if let (Some(first), Some(last)) = (cells.first(), cells.last()) {
            self.table_width = last.left() - first.left() + LAST_COLUMN_MIN;
        }
        self.remember_column_widths(parent, &cols, &cells);
        self.header_drag(parent, &cols, &cells, view, drag_started);
        self.fit_on_border_double_click(parent, &cols, &cells, view);
        parent.advance_cursor_after_rect(view);
        if hbar_h > 0.0 {
            let bar = egui::Rect::from_min_max(egui::pos2(full.min.x, view.max.y), full.max);
            let mut bar_ui = parent.new_child(egui::UiBuilder::new().max_rect(bar));
            // Solid: a floating bar would only appear on hover.
            bar_ui.spacing_mut().scroll = egui::style::ScrollStyle::solid();
            let out = egui::ScrollArea::horizontal()
                .id_salt("conn_table_hscroll")
                .auto_shrink([false, true])
                .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysVisible)
                .horizontal_scroll_offset(self.table_scroll_x)
                .show(&mut bar_ui, |ui| {
                    ui.allocate_space(egui::vec2(self.table_width, 1.0));
                });
            self.table_scroll_x = out.state.clamped_offset().x;
        }

        if filters_changed {
            for (c, text) in cols.iter().zip(filter_texts) {
                if text.trim().is_empty() {
                    self.col_filter_text.remove(c);
                } else {
                    self.col_filter_text.insert(*c, text);
                }
            }
            self.apply_column_filters();
        }
        if let Some(c) = clicked_header {
            self.cfg.sort.click(c);
            self.view_dirty = true;
            self.settings_changed();
        }
        if let Some((pos, secondary)) = clicked_row {
            let keys = self.view_keys();
            if secondary {
                // A right click on a row outside the selection selects only that row (Explorer).
                if !self.sel.contains(&keys[pos]) {
                    self.sel.select_only(keys[pos]);
                }
            } else {
                let m = ui.input(|i| i.modifiers);
                self.sel.click(pos, &keys, m.command, m.shift);
            }
        }
        if copy {
            self.copy_selection(&ui.ctx().clone());
        }
        if let Some(g) = toggled {
            self.toggle_group(g);
        }
        if let Some((pid, info)) = properties {
            self.open_properties(pid, info);
        }
        if let Some(a) = action {
            self.menu_action(a);
        }
        if open_columns_menu {
            // A small window rather than a context menu: it stays open while columns are
            // switched and moved.
            self.show_columns_popup = true;
        }
        self.columns_popup(ui.ctx());
    }

    fn columns_popup(&mut self, ctx: &egui::Context) {
        if !self.show_columns_popup {
            return;
        }
        let mut open = true;
        egui::Window::new(t!("menu.view.columns"))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .show(ctx, |ui| self.columns_menu(ui));
        self.show_columns_popup = open;
    }
}
