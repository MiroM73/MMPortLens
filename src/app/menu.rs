//! The menu bar: File, View (columns, language, theme) and Help.

use crate::app::*;

impl App {
    pub(in crate::app) fn menu_bar(&mut self, ui: &mut egui::Ui) {
        egui::MenuBar::new().ui(ui, |ui| {
            ui.menu_button(t!("menu.file"), |ui| {
                if ui.button(t!("menu.file.exit")).clicked() {
                    ui.close();
                    ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                }
            });
            ui.menu_button(t!("menu.view"), |ui| {
                ui.menu_button(t!("menu.view.columns"), |ui| self.columns_menu(ui));
                ui.menu_button(t!("menu.view.language"), |ui| self.language_menu(ui));
                ui.menu_button(t!("menu.view.theme"), |ui| self.theme_menu(ui));
            });
            ui.menu_button(t!("menu.help"), |ui| {
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
        for (i, c) in order.iter().enumerate() {
            ui.horizontal(|ui| {
                let mut on = c.visible;
                if ui.checkbox(&mut on, c.id.title()).changed() {
                    self.cfg.columns.set_visible(c.id, on);
                    changed = true;
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui
                        .add_enabled(i < last, egui::Button::new("⏵").small())
                        .on_hover_text(t!("columns.move_right"))
                        .clicked()
                    {
                        self.cfg.columns.move_by(c.id, 1);
                        changed = true;
                    }
                    if ui
                        .add_enabled(i > 0, egui::Button::new("⏴").small())
                        .on_hover_text(t!("columns.move_left"))
                        .clicked()
                    {
                        self.cfg.columns.move_by(c.id, -1);
                        changed = true;
                    }
                });
            });
        }
        ui.separator();
        if ui.button(t!("columns.reset")).clicked() {
            self.cfg.columns = Default::default();
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
