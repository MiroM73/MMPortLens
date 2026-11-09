//! The menu bar: File, View (columns, language, theme) and Help.

use crate::app::*;

impl App {
    pub(in crate::app) fn menu_bar(&mut self, ui: &mut egui::Ui) {
        egui::MenuBar::new().ui(ui, |ui| {
            ui.menu_button(t!("menu.file"), |ui| {
                if ui
                    .add(
                        egui::Button::new(t!("menu.file.export")).shortcut_text(help::kb("Ctrl+S")),
                    )
                    .clicked()
                {
                    ui.close();
                    self.open_export();
                }
                ui.separator();
                self.record_menu_items(ui);
                ui.separator();
                if cfg!(windows) {
                    self.admin_menu(ui);
                    ui.separator();
                }
                if ui.button(t!("menu.file.exit")).clicked() {
                    ui.close();
                    ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                }
            });
            ui.menu_button(t!("menu.view"), |ui| {
                ui.menu_button(t!("menu.view.columns"), |ui| self.columns_menu(ui));
                if ui
                    .button(t!("menu.view.stats"))
                    .on_hover_text(t!("menu.view.stats.tip"))
                    .clicked()
                {
                    ui.close();
                    self.open_stats();
                }
                if ui
                    .button(t!("menu.view.watch"))
                    .on_hover_text(t!("menu.view.watch.tip"))
                    .clicked()
                {
                    ui.close();
                    self.open_watch(false);
                }
                ui.separator();
                if ui
                    .checkbox(&mut self.cfg.show_details, t!("menu.view.details"))
                    .changed()
                {
                    self.settings_changed();
                }
                if ui
                    .checkbox(
                        &mut self.cfg.resolve_addresses,
                        t!("menu.view.resolve_addresses"),
                    )
                    .on_hover_text(t!("menu.view.resolve_addresses.tip"))
                    .changed()
                {
                    self.view_dirty = true;
                    self.settings_changed();
                }
                if ui
                    .checkbox(
                        &mut self.cfg.separate_windows,
                        t!("menu.view.separate_windows"),
                    )
                    .on_hover_text(t!("menu.view.separate_windows.tip"))
                    .changed()
                {
                    self.settings_changed();
                }
                if cfg!(windows)
                    && ui
                        .checkbox(&mut self.cfg.etw_capture, t!("menu.view.etw"))
                        .on_hover_text(t!("menu.view.etw.tip"))
                        .changed()
                {
                    self.set_etw_capture();
                }
                ui.separator();
                if ui
                    .checkbox(
                        &mut self.cfg.show_column_filters,
                        t!("menu.view.column_filters"),
                    )
                    .changed()
                {
                    self.settings_changed();
                }
                if ui
                    .checkbox(
                        &mut self.cfg.remember_column_filters,
                        t!("menu.view.remember_column_filters"),
                    )
                    .on_hover_text(t!("menu.view.remember_column_filters.tip"))
                    .changed()
                {
                    self.save_column_filters();
                    self.settings_changed();
                }
                if ui
                    .add_enabled(
                        self.column_filter_count() > 0,
                        egui::Button::new(t!("menu.view.clear_column_filters")),
                    )
                    .clicked()
                {
                    ui.close();
                    self.clear_column_filters();
                }
                ui.separator();
                self.group_menu_items(ui);
                ui.separator();
                if ui
                    .add(
                        egui::Button::new(t!("menu.view.color_rules"))
                            .shortcut_text(help::kb("Ctrl+H")),
                    )
                    .clicked()
                {
                    ui.close();
                    self.show_rules = true;
                }
                ui.menu_button(t!("menu.view.language"), |ui| self.language_menu(ui));
                ui.menu_button(t!("menu.view.theme"), |ui| self.theme_menu(ui));
            });
            ui.menu_button(t!("menu.help"), |ui| {
                if ui
                    .add(egui::Button::new(t!("menu.help.shortcuts")).shortcut_text(help::kb("F1")))
                    .clicked()
                {
                    ui.close();
                    self.show_shortcuts = true;
                }
                if ui.button(t!("menu.help.cli")).clicked() {
                    ui.close();
                    self.cli_help.open();
                }
                if ui
                    .button(format!("⬆ {}", t!("menu.help.updates")))
                    .clicked()
                {
                    ui.close();
                    self.show_update = true;
                    if !matches!(self.update, UpdateState::Installing { .. }) {
                        self.start_update_check(false);
                    }
                }
                if ui.button(t!("menu.help.about")).clicked() {
                    ui.close();
                    self.show_about = true;
                }
            });
        });
    }

    /// Checkbox per column plus moving it left / right; also opened from a right click on a
    /// column header.
    pub(in crate::app) fn columns_menu(&mut self, ui: &mut egui::Ui) {
        let mut changed = false;
        let order: Vec<_> = self.cfg.columns.0.clone();
        let last = order.len() - 1;
        // Row rects and the row whose handle starts a drag (#2).
        let mut rows = Vec::with_capacity(order.len());
        let mut drag_started = None;
        for (i, c) in order.iter().enumerate() {
            let row = ui.horizontal(|ui| {
                let handle = ui
                    .add(egui::Label::new("≡").selectable(false).sense(Sense::drag()))
                    .on_hover_cursor(egui::CursorIcon::Grab)
                    .on_hover_text(t!("columns.drag"));
                if handle.drag_started() {
                    drag_started = Some(c.id);
                }
                let mut on = c.visible;
                if ui.checkbox(&mut on, c.id.title()).changed() {
                    self.cfg.columns.set_visible(c.id, on);
                    changed = true;
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if crate::app::table::arrow_button(ui, i < last, true)
                        .on_hover_text(t!("columns.move_right"))
                        .clicked()
                    {
                        self.cfg.columns.move_by(c.id, 1);
                        changed = true;
                    }
                    if crate::app::table::arrow_button(ui, i > 0, false)
                        .on_hover_text(t!("columns.move_left"))
                        .clicked()
                    {
                        self.cfg.columns.move_by(c.id, -1);
                        changed = true;
                    }
                });
            });
            rows.push(row.response.rect);
        }
        let ids: Vec<ColumnId> = order.iter().map(|c| c.id).collect();
        changed |= self.columns_list_drag(ui, &ids, &rows, drag_started);
        ui.separator();
        if ui
            .button(t!("columns.reset"))
            .on_hover_text(t!("columns.reset.tip"))
            .clicked()
        {
            // Order, visibility and widths (#2).
            self.cfg.columns = Default::default();
            self.apply_widths = true;
            changed = true;
        }
        if changed {
            self.view_dirty = true;
            self.settings_changed();
        }
    }

    fn language_menu(&mut self, ui: &mut egui::Ui) {
        let mut pick: Option<String> = None;
        if ui
            .radio(
                self.cfg.language.is_empty(),
                t!("menu.view.language.system"),
            )
            .clicked()
        {
            pick = Some(String::new());
        }
        ui.separator();
        for l in i18n::available() {
            let on = self.cfg.language.eq_ignore_ascii_case(&l.code);
            if ui.radio(on, format!("{} ({})", l.name, l.code)).clicked() {
                pick = Some(l.code.clone());
            }
        }
        if let Some(code) = pick {
            ui.close();
            i18n::set_language(&code);
            self.cfg.language = code;
            self.run_only.language = None;
            self.settings_changed();
        }
    }

    fn theme_menu(&mut self, ui: &mut egui::Ui) {
        for (code, key) in [
            ("system", "menu.view.theme.system"),
            ("dark", "menu.view.theme.dark"),
            ("light", "menu.view.theme.light"),
        ] {
            if ui.radio(self.cfg.theme == code, t!(key)).clicked() {
                ui.close();
                self.cfg.theme = code.into();
                self.apply_theme(ui.ctx());
                self.settings_changed();
            }
        }
    }
}
