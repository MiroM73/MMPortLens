//! The connection table: visible columns in the user's order, sorting by a header click,
//! the column menu on a header right click, the column filter row (#1), highlighted new / changed /
//! closed rows.

use crate::app::*;
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

        let header_h = if self.cfg.show_column_filters {
            22.0 + ui.spacing().interact_size.y + ui.spacing().item_spacing.y
        } else {
            22.0
        };
        let mut tb = TableBuilder::new(ui)
            // A new column set gets fresh widths instead of shifted old ones.
            .id_salt(("conn_table", &cols))
            .striped(true)
            .resizable(true)
            .sense(Sense::click())
            .auto_shrink([false, false])
            .cell_layout(egui::Layout::left_to_right(egui::Align::Center));
        if let Some(pos) = self.scroll_to.take() {
            tb = tb.scroll_to_row(pos, None);
        }
        for (i, c) in cols.iter().enumerate() {
            let col = if i + 1 == cols.len() {
                Column::remainder().at_least(60.0)
            } else {
                Column::initial(c.width()).at_least(40.0)
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

        // Filter row under the headers: one field per visible column (edited in a copy, applied
        // after the table is drawn).
        let show_filters = self.cfg.show_column_filters;
        let mut filter_texts: Vec<String> = cols
            .iter()
            .map(|c| self.col_filter_text.get(c).cloned().unwrap_or_default())
            .collect();
        let mut filters_changed = false;

        tb.header(header_h, |mut h| {
            for (i, &c) in cols.iter().enumerate() {
                h.col(|ui| {
                    ui.vertical(|ui| {
                        let arrow = match (sort.column == c, sort.ascending) {
                            (true, true) => " ⏶",
                            (true, false) => " ⏷",
                            _ => "",
                        };
                        let resp = ui.add(
                            egui::Label::new(
                                RichText::new(format!("{}{arrow}", c.title())).strong(),
                            )
                            .truncate()
                            .selectable(false)
                            .sense(Sense::click()),
                        );
                        if resp.clicked() {
                            clicked_header = Some(c);
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
            body.rows(row_h, self.view.len(), |mut row| {
                let r = &self.view[row.index()];
                let key = r.conn.key();
                row.set_selected(self.sel.contains(&key));
                let bg = mark_color(r.mark, dark);
                for &c in &cols {
                    row.col(|ui| {
                        if let Some(bg) = bg {
                            ui.painter()
                                .rect_filled(ui.max_rect().expand2(gap), 0.0, bg);
                        }
                        let text = cell_text(r, c);
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
                    clicked_row = Some((row.index(), false));
                }
                if resp.secondary_clicked() {
                    clicked_row = Some((row.index(), true));
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
                    ui.separator();
                    action = action.or(self.action_menu_items(ui));
                });
            });
        });

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
