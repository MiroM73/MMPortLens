//! View → Color rules… (#10, Ctrl+H): the rules window, ported from the highlight rules window of
//! RusTrace, and the colors of a table row. The rules themselves are in `crate::rules`.
//!
//! Every change in the window recompiles the rules and rebuilds the view, so the table shows the
//! result at once (live preview); the preview cell shows the colors on the background of the
//! theme being edited.

use crate::app::*;
use crate::colfilter::FilterError;
use crate::rules::{self, ColorRule};
use crate::tabexport::RowLook;

/// Width of the rule name and pattern fields.
const NAME_W: f32 = 140.0;
const PATTERN_W: f32 = 220.0;
/// Height of the list of rules to export.
const PICK_H: f32 = 140.0;

fn theme_name(dark: bool) -> String {
    if dark {
        t!("menu.view.theme.dark")
    } else {
        t!("menu.view.theme.light")
    }
}

fn column_label(c: Option<ColumnId>) -> String {
    c.map_or_else(|| t!("rules.any_column"), |c| c.title())
}

/// A color button for an RGBA color of the settings; true when it changed.
fn color_button(ui: &mut egui::Ui, color: &mut [u8; 4]) -> bool {
    let mut c = c32(*color);
    let changed = ui.color_edit_button_srgba(&mut c).changed();
    if changed {
        *color = c.to_srgba_unmultiplied();
    }
    changed
}

/// Sample text on the background of the given theme.
fn preview(ui: &mut egui::Ui, dark: bool, look: RowLook) {
    let v = if dark {
        egui::Visuals::dark()
    } else {
        egui::Visuals::light()
    };
    let mut text = RichText::new(format!(" {} ", t!("rules.preview_text")))
        .color(look.fg.map_or(v.text_color(), c32));
    if let Some(bg) = look.bg {
        text = text.background_color(c32(bg));
    }
    egui::Frame::new()
        .fill(v.extreme_bg_color)
        .inner_margin(egui::Margin::symmetric(6, 2))
        .corner_radius(3.0)
        .show(ui, |ui| {
            ui.label(text);
        });
}

fn error_text(e: &FilterError) -> String {
    match e {
        FilterError::Regex(error) => t!("filter.error.regex", error = error),
        FilterError::Network(value) => t!("filter.error.network", value = value),
    }
}

impl App {
    /// Compiles the rules after a change and rebuilds the view.
    pub(in crate::app) fn recompile_rules(&mut self) {
        self.rules = rules::Compiled::new(&self.cfg.color_rules);
        self.view_dirty = true;
    }

    /// Text and background of row `i` of the view: its mark and its color rule.
    pub(in crate::app) fn row_colors(
        &self,
        i: usize,
        r: &Row,
        dark: bool,
    ) -> (Option<Color32>, Option<Color32>) {
        let mark = RowLook {
            fg: None,
            bg: mark_color(r.mark, dark).map(|c| c.to_srgba_unmultiplied()),
        };
        let rule = self
            .view_rule
            .get(i)
            .copied()
            .flatten()
            .and_then(|k| self.cfg.color_rules.get(k))
            .map(|k| k.look(dark));
        let look = rules::row_look(mark, rule, self.cfg.marks_over_rules);
        (look.fg.map(c32), look.bg.map(c32))
    }

    /// The colors of the rows of an export (light theme), or none when the export has no colors.
    pub(in crate::app) fn export_looks(&self, rows: &[Row], colors: bool) -> Vec<RowLook> {
        if !colors {
            return Vec::new();
        }
        let cols = self.cfg.columns.visible();
        rows.iter()
            .map(|r| {
                rules::export_look(
                    r,
                    &self.cfg.color_rules,
                    &self.rules,
                    &cols,
                    self.cfg.marks_over_rules,
                )
            })
            .collect()
    }

    pub(in crate::app) fn rules_window(&mut self, ctx: &egui::Context) {
        if !self.show_rules {
            return;
        }
        let mut open = true;
        let mut changed = false;
        egui::Window::new(format!("🎨 {}", t!("rules.title")))
            .id(egui::Id::unique("rules_window"))
            .open(&mut open)
            .default_width(980.0_f32.min(ctx.content_rect().width() - 40.0))
            .default_height(380.0)
            .resizable(true)
            .show(ctx, |ui| {
                ui.label(t!("rules.help"));
                changed |= ui
                    .checkbox(&mut self.cfg.marks_over_rules, t!("rules.marks_first"))
                    .on_hover_text(t!("rules.marks_first.tip"))
                    .changed();
                ui.separator();
                let edit_dark = self.rules_edit_dark;
                ui.horizontal(|ui| {
                    ui.label(t!("rules.for_theme"));
                    ui.selectable_value(
                        &mut self.rules_edit_dark,
                        false,
                        format!("☀ {}", theme_name(false)),
                    );
                    ui.selectable_value(
                        &mut self.rules_edit_dark,
                        true,
                        format!("🌙 {}", theme_name(true)),
                    );
                    if ui
                        .button(format!(
                            "⇄ {}",
                            t!("rules.copy_from", theme = theme_name(!edit_dark))
                        ))
                        .on_hover_text(t!("rules.copy_from.tip"))
                        .clicked()
                    {
                        for r in &mut self.cfg.color_rules {
                            r.copy_colors(!edit_dark);
                        }
                        changed = true;
                    }
                });
                ui.separator();
                changed |= self.rules_grid(ui, edit_dark);
                ui.separator();
                changed |= self.rules_buttons(ui);
                self.rules_export_choice(ui);
                if let Some(m) = &self.rules_msg {
                    ui.label(m.as_str());
                }
            });
        if !open {
            self.show_rules = false;
            self.rules_export_pick = None;
            self.rules_msg = None;
        }
        if changed {
            self.recompile_rules();
            self.settings_changed();
        }
    }

    /// The table of the rules; true when a rule changed.
    fn rules_grid(&mut self, ui: &mut egui::Ui, edit_dark: bool) -> bool {
        let mut changed = false;
        let mut act: Option<(usize, i32)> = None; // (index, -1 up / 1 down / 0 delete)
        ui.spacing_mut().scroll = egui::style::ScrollStyle::solid();
        let below =
            40.0 + if self.rules_export_pick.is_some() {
                PICK_H + 60.0
            } else {
                0.0
            } + if self.rules_msg.is_some() { 20.0 } else { 0.0 };
        egui::ScrollArea::both()
            .id_salt("rules_scroll")
            .max_height((ui.available_height() - below).max(80.0))
            .show(ui, |ui| {
                egui::Grid::new("rules_grid")
                    .striped(true)
                    .spacing([8.0, 6.0])
                    .show(ui, |ui| {
                        for h in [
                            t!("rules.col.on"),
                            t!("rules.col.name"),
                            t!("rules.col.column"),
                            t!("rules.col.pattern"),
                            String::new(),
                            t!("rules.col.text"),
                            t!("rules.col.background"),
                            t!("rules.col.preview"),
                            String::new(),
                        ] {
                            ui.strong(h);
                        }
                        ui.end_row();
                        let n = self.cfg.color_rules.len();
                        for i in 0..n {
                            let err = self.rules.error(i).map(error_text);
                            let r = &mut self.cfg.color_rules[i];
                            ui.push_id(i, |ui| {
                                changed |= ui.checkbox(&mut r.enabled, "").changed();
                            });
                            changed |= ui
                                .add_sized([NAME_W, 20.0], egui::TextEdit::singleline(&mut r.name))
                                .changed();
                            let before = r.column;
                            egui::ComboBox::from_id_salt(("rule_column", i))
                                .selected_text(column_label(r.column))
                                .width(130.0)
                                .show_ui(ui, |ui| {
                                    ui.selectable_value(&mut r.column, None, column_label(None));
                                    for c in ColumnId::ALL {
                                        ui.selectable_value(&mut r.column, Some(c), c.title());
                                    }
                                });
                            changed |= before != r.column;
                            let pattern = ui.add_sized(
                                [PATTERN_W, 20.0],
                                egui::TextEdit::singleline(&mut r.pattern)
                                    .font(egui::TextStyle::Monospace)
                                    .hint_text(t!("filter.hint")),
                            );
                            changed |= pattern.changed();
                            pattern.on_hover_text(t!("rules.pattern.tip"));
                            match err {
                                Some(e) => {
                                    ui.colored_label(ui.visuals().error_fg_color, "⚠")
                                        .on_hover_text(e);
                                }
                                None => {
                                    ui.label("");
                                }
                            }
                            ui.horizontal(|ui| {
                                changed |= ui.checkbox(&mut r.use_fg, "").changed();
                                let (fg, _) = r.colors_mut(edit_dark);
                                changed |= color_button(ui, fg);
                            });
                            ui.horizontal(|ui| {
                                changed |= ui.checkbox(&mut r.use_bg, "").changed();
                                let (_, bg) = r.colors_mut(edit_dark);
                                changed |= color_button(ui, bg);
                            });
                            preview(ui, edit_dark, r.look(edit_dark));
                            ui.horizontal(|ui| {
                                if ui
                                    .add_enabled(i > 0, egui::Button::new("⬆").small())
                                    .on_hover_text(t!("rules.up"))
                                    .clicked()
                                {
                                    act = Some((i, -1));
                                }
                                if ui
                                    .add_enabled(i + 1 < n, egui::Button::new("⬇").small())
                                    .on_hover_text(t!("rules.down"))
                                    .clicked()
                                {
                                    act = Some((i, 1));
                                }
                                if ui
                                    .small_button("🗑")
                                    .on_hover_text(t!("rules.delete"))
                                    .clicked()
                                {
                                    act = Some((i, 0));
                                }
                            });
                            ui.end_row();
                        }
                    });
            });
        if let Some((i, d)) = act {
            // The export choice moves with its rule.
            if let Some(p) = &mut self.rules_export_pick {
                p.resize(self.cfg.color_rules.len(), true);
                rules::move_item(p, i, d);
            }
            rules::move_item(&mut self.cfg.color_rules, i, d);
            changed = true;
        }
        changed
    }

    /// Add, import, export, restore the examples; true when the rules changed.
    fn rules_buttons(&mut self, ui: &mut egui::Ui) -> bool {
        let mut changed = false;
        ui.horizontal_wrapped(|ui| {
            if ui.button(format!("➕ {}", t!("rules.add"))).clicked() {
                self.cfg.color_rules.push(ColorRule::default());
                changed = true;
            }
            if ui.button(format!("📥 {}", t!("rules.import"))).clicked()
                && let Some(p) = rfd::FileDialog::new()
                    .add_filter("JSON", &["json"])
                    .pick_file()
            {
                match std::fs::read_to_string(&p)
                    .map_err(|e| e.to_string())
                    .and_then(|s| rules::parse_rules(&s))
                {
                    Ok(imported) => {
                        let res = rules::import_rules(&mut self.cfg.color_rules, imported);
                        self.rules_msg = Some(t!(
                            "rules.import.done",
                            added = res.added,
                            updated = res.updated,
                            skipped = res.skipped
                        ));
                        changed = true;
                    }
                    Err(e) => self.rules_msg = Some(t!("rules.import.failed", error = e)),
                }
            }
            let picking = self.rules_export_pick.is_some();
            if ui
                .selectable_label(picking, format!("📤 {}", t!("rules.export")))
                .on_hover_text(t!("rules.export.tip"))
                .clicked()
            {
                self.rules_export_pick = (!picking).then(|| vec![true; self.cfg.color_rules.len()]);
            }
            if ui.button(t!("rules.restore_defaults")).clicked() {
                self.cfg.color_rules = rules::default_rules();
                if self.rules_export_pick.is_some() {
                    self.rules_export_pick = Some(vec![true; self.cfg.color_rules.len()]);
                }
                changed = true;
            }
        });
        changed
    }

    /// The rules to export with a checkbox each (all ticked at first) and the save button.
    fn rules_export_choice(&mut self, ui: &mut egui::Ui) {
        let Some(pick) = &mut self.rules_export_pick else {
            return;
        };
        pick.resize(self.cfg.color_rules.len(), true);
        ui.separator();
        ui.label(t!("rules.export.pick"));
        egui::ScrollArea::vertical()
            .id_salt("rules_export_pick")
            .max_height(PICK_H)
            .auto_shrink([false, true])
            .show(ui, |ui| {
                for (i, (r, p)) in self.cfg.color_rules.iter().zip(pick.iter_mut()).enumerate() {
                    ui.push_id(("pick", i), |ui| {
                        let name = if r.name.is_empty() {
                            &r.pattern
                        } else {
                            &r.name
                        };
                        ui.checkbox(p, name.as_str());
                    });
                }
            });
        let count = pick.iter().filter(|p| **p).count();
        let mut save = false;
        let mut close = false;
        ui.horizontal(|ui| {
            if ui.button(t!("rules.export.all")).clicked() {
                pick.iter_mut().for_each(|p| *p = true);
            }
            if ui.button(t!("rules.export.none")).clicked() {
                pick.iter_mut().for_each(|p| *p = false);
            }
            save = ui
                .add_enabled(
                    count > 0,
                    egui::Button::new(format!("📤 {}", t!("rules.export.save", count = count))),
                )
                .clicked();
            close = ui.button(t!("action.cancel")).clicked();
        });
        if save
            && let Some(path) = rfd::FileDialog::new()
                .add_filter("JSON", &["json"])
                .set_file_name("mmportlens-rules.json")
                .save_file()
        {
            let picked: Vec<ColorRule> = self
                .cfg
                .color_rules
                .iter()
                .zip(pick.iter())
                .filter(|(_, p)| **p)
                .map(|(r, _)| r.clone())
                .collect();
            let json = serde_json::to_string_pretty(&picked).unwrap_or_default();
            self.rules_msg = Some(match std::fs::write(&path, json) {
                Ok(()) => {
                    close = true;
                    t!(
                        "rules.export.done",
                        count = picked.len(),
                        path = path.display()
                    )
                }
                Err(e) => t!("rules.export.failed", error = e),
            });
        }
        if close {
            self.rules_export_pick = None;
        }
    }
}
