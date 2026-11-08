//! Saved filter presets (#13): the toolbar combo box, "Save current as…" and "Manage presets…".
//! The preset model is in `crate::presets`.

use crate::app::*;
use crate::presets::{self, FilterPreset};

/// The state of the preset windows.
#[derive(Default)]
pub(in crate::app) struct PresetWindows {
    /// "Save current as…" is open with this name.
    save_name: Option<String>,
    /// "Manage presets…" is open.
    manage: bool,
}

impl App {
    /// The current filters as a preset without a name.
    pub(in crate::app) fn current_filters(&self) -> FilterPreset {
        let c = &self.cfg;
        FilterPreset {
            name: String::new(),
            show_tcp: c.show_tcp,
            show_udp: c.show_udp,
            show_ipv4: c.show_ipv4,
            show_ipv6: c.show_ipv6,
            show_listening: c.show_listening,
            text: self.filter.clone(),
            column_filters: self.column_filter_list(),
        }
    }

    /// Sets all filters of a preset at once. A column with a filter is shown, because the filters
    /// of hidden columns do not apply.
    pub(in crate::app) fn apply_preset(&mut self, p: &FilterPreset) {
        let c = &mut self.cfg;
        c.show_tcp = p.show_tcp;
        c.show_udp = p.show_udp;
        c.show_ipv4 = p.show_ipv4;
        c.show_ipv6 = p.show_ipv6;
        c.show_listening = p.show_listening;
        for (id, text) in &p.column_filters {
            if !text.trim().is_empty() {
                c.columns.set_visible(*id, true);
            }
        }
        self.active_preset = p.name.clone();
        self.filter = p.text.clone();
        self.set_column_filters(p.column_filters.clone());
        self.view_dirty = true;
        self.settings_changed();
    }

    /// The active preset and whether the filters were changed since it was chosen.
    fn chosen_preset(&self) -> Option<(&FilterPreset, bool)> {
        let p = self
            .cfg
            .filter_presets
            .iter()
            .find(|p| !p.name.is_empty() && p.name == self.active_preset)?;
        Some((p, !p.same_filters(&self.current_filters())))
    }

    /// The preset combo box of the toolbar.
    pub(in crate::app) fn preset_combo(&mut self, ui: &mut egui::Ui) {
        let text = match self.chosen_preset() {
            Some((p, true)) => format!("⭐ {} *", p.name),
            Some((p, false)) => format!("⭐ {}", p.name),
            None => format!("⭐ {}", t!("preset.none")),
        };
        let tip = match self.chosen_preset() {
            Some((_, true)) => t!("preset.modified.tip"),
            _ => t!("preset.tip"),
        };
        let mut chosen: Option<FilterPreset> = None;
        let mut save = false;
        let mut manage = false;
        egui::ComboBox::from_id_salt("filter_preset")
            .selected_text(text)
            .width(150.0)
            .height(400.0)
            .show_ui(ui, |ui| {
                for p in &self.cfg.filter_presets {
                    let active = p.name == self.active_preset;
                    if ui.selectable_label(active, &p.name).clicked() {
                        chosen = Some(p.clone());
                    }
                }
                ui.separator();
                save = ui.button(t!("preset.save_as")).clicked();
                manage = ui.button(t!("preset.manage")).clicked();
            })
            .response
            .on_hover_text(tip);
        if let Some(p) = chosen {
            self.apply_preset(&p);
        }
        if save {
            let name = self
                .chosen_preset()
                .map(|(p, _)| p.name.clone())
                .unwrap_or_default();
            self.preset_windows.save_name = Some(name);
        }
        if manage {
            self.preset_windows.manage = true;
        }
    }

    pub(in crate::app) fn preset_windows(&mut self, ctx: &egui::Context) {
        self.save_preset_window(ctx);
        self.manage_presets_window(ctx);
    }

    fn save_preset_window(&mut self, ctx: &egui::Context) {
        let Some(name) = &mut self.preset_windows.save_name else {
            return;
        };
        let mut open = true;
        let mut save = false;
        let mut cancel = false;
        let trimmed = name.trim().to_owned();
        let exists = self.cfg.filter_presets.iter().any(|p| p.name == trimmed);
        egui::Window::new(t!("preset.save_as.title"))
            .id(egui::Id::unique("save_preset_window"))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .show(ctx, |ui| {
                ui.label(t!("preset.save_as.text"));
                let edit = ui.add(
                    egui::TextEdit::singleline(name)
                        .hint_text(t!("preset.name.hint"))
                        .desired_width(260.0),
                );
                edit.request_focus();
                if exists {
                    ui.label(
                        RichText::new(t!("preset.overwrite", name = trimmed))
                            .color(ui.visuals().warn_fg_color),
                    );
                }
                ui.horizontal(|ui| {
                    save = ui
                        .add_enabled(!trimmed.is_empty(), egui::Button::new(t!("preset.save")))
                        .clicked();
                    cancel = ui.button(t!("action.cancel")).clicked();
                });
                if !trimmed.is_empty() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    save = true;
                }
                if ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
                    cancel = true;
                }
            });
        if save {
            let mut p = self.current_filters();
            p.name = trimmed;
            self.active_preset = p.name.clone();
            presets::save(&mut self.cfg.filter_presets, p);
            self.settings_changed();
        }
        if save || cancel || !open {
            self.preset_windows.save_name = None;
        }
    }

    fn manage_presets_window(&mut self, ctx: &egui::Context) {
        if !self.preset_windows.manage {
            return;
        }
        let mut open = true;
        let mut changed = false;
        let mut act: Option<(usize, i32)> = None; // (index, -1 up / 1 down / 0 delete)
        let mut overwrite: Option<usize> = None;
        let mut restore = false;
        egui::Window::new(t!("preset.manage.title"))
            .id(egui::Id::unique("manage_presets_window"))
            .open(&mut open)
            .collapsible(false)
            .resizable(true)
            .default_width(520.0)
            .show(ctx, |ui| {
                ui.label(t!("preset.manage.text"));
                ui.separator();
                egui::ScrollArea::vertical()
                    .max_height(360.0)
                    .show(ui, |ui| {
                        egui::Grid::new("presets_grid")
                            .striped(true)
                            .spacing([8.0, 6.0])
                            .show(ui, |ui| {
                                let n = self.cfg.filter_presets.len();
                                for i in 0..n {
                                    let p = &mut self.cfg.filter_presets[i];
                                    let was_active = p.name == self.active_preset;
                                    let edit = ui.add_sized(
                                        [180.0, 20.0],
                                        egui::TextEdit::singleline(&mut p.name),
                                    );
                                    if edit.changed() {
                                        // A renamed active preset stays active.
                                        if was_active {
                                            self.active_preset = p.name.clone();
                                        }
                                        changed = true;
                                    }
                                    ui.label(RichText::new(summary(p)).weak())
                                        .on_hover_text(summary(p));
                                    ui.horizontal(|ui| {
                                        if ui
                                            .add_enabled(i > 0, egui::Button::new("⬆").small())
                                            .clicked()
                                        {
                                            act = Some((i, -1));
                                        }
                                        if ui
                                            .add_enabled(i + 1 < n, egui::Button::new("⬇").small())
                                            .clicked()
                                        {
                                            act = Some((i, 1));
                                        }
                                        if ui
                                            .small_button("💾")
                                            .on_hover_text(t!("preset.overwrite.tip"))
                                            .clicked()
                                        {
                                            overwrite = Some(i);
                                        }
                                        if ui
                                            .small_button("🗑")
                                            .on_hover_text(t!("preset.delete"))
                                            .clicked()
                                        {
                                            act = Some((i, 0));
                                        }
                                    });
                                    ui.end_row();
                                }
                            });
                    });
                ui.separator();
                restore = ui.button(t!("preset.restore_defaults")).clicked();
            });
        if let Some((i, d)) = act {
            crate::rules::move_item(&mut self.cfg.filter_presets, i, d);
            changed = true;
        }
        if let Some(i) = overwrite {
            let mut p = self.current_filters();
            p.name = self.cfg.filter_presets[i].name.clone();
            self.active_preset = p.name.clone();
            self.cfg.filter_presets[i] = p;
            changed = true;
        }
        if restore {
            self.cfg.filter_presets = presets::default_presets();
            changed = true;
        }
        if changed {
            self.settings_changed();
        }
        self.preset_windows.manage = open;
    }
}

/// A short description of the filters of a preset ("TCP, IPv4 · State: =Listen").
fn summary(p: &FilterPreset) -> String {
    let mut parts: Vec<String> = Vec::new();
    let switches: Vec<&str> = [
        (p.show_tcp, "TCP"),
        (p.show_udp, "UDP"),
        (p.show_ipv4, "IPv4"),
        (p.show_ipv6, "IPv6"),
    ]
    .into_iter()
    .filter_map(|(on, s)| on.then_some(s))
    .collect();
    if switches.len() < 4 {
        parts.push(switches.join(", "));
    }
    if !p.show_listening {
        parts.push(t!("preset.no_listening"));
    }
    if !p.text.trim().is_empty() {
        parts.push(format!("\"{}\"", p.text.trim()));
    }
    for (c, text) in &p.column_filters {
        parts.push(format!("{}: {text}", c.title()));
    }
    if parts.is_empty() {
        t!("preset.everything")
    } else {
        parts.join(" · ")
    }
}
