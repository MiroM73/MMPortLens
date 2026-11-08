//! Column filters (#1): the texts typed into the filter row under the headers, compiled into
//! `ColumnFilter`s, and the field drawn in a header cell. The syntax is in `crate::colfilter`.

use crate::app::*;
use crate::colfilter::{ColumnFilter, FilterKind};

impl App {
    /// Recompiles the column filters after their texts changed and rebuilds the view.
    pub(in crate::app) fn apply_column_filters(&mut self) {
        self.col_filter_text.retain(|_, t| !t.trim().is_empty());
        self.col_filters.clear();
        self.col_filter_errors.clear();
        for (&id, text) in &self.col_filter_text {
            match ColumnFilter::parse(text, id.filter_kind()) {
                Ok(Some(f)) => self.col_filters.push((id, f)),
                Ok(None) => {}
                // Invalid input filters nothing; the field shows the error.
                Err(e) => {
                    self.col_filter_errors.insert(id, e);
                }
            }
        }
        self.view_dirty = true;
        self.save_column_filters();
    }

    /// Writes the column filters into the settings when they are to be remembered.
    pub(in crate::app) fn save_column_filters(&mut self) {
        let saved = if self.cfg.remember_column_filters {
            self.column_filter_list()
        } else {
            Vec::new()
        };
        if saved != self.cfg.column_filters {
            self.cfg.column_filters = saved;
            self.settings_changed();
        }
    }

    /// The filter texts in the column order (settings, restart after an update).
    pub(in crate::app) fn column_filter_list(&self) -> Vec<(ColumnId, String)> {
        ColumnId::ALL
            .iter()
            .filter_map(|id| Some((*id, self.col_filter_text.get(id)?.clone())))
            .collect()
    }

    pub(in crate::app) fn set_column_filters(&mut self, list: Vec<(ColumnId, String)>) {
        self.col_filter_text = list.into_iter().collect();
        self.apply_column_filters();
    }

    pub(in crate::app) fn clear_column_filters(&mut self) {
        self.col_filter_text.clear();
        self.apply_column_filters();
    }

    /// Number of filter fields with text (also invalid ones).
    pub(in crate::app) fn column_filter_count(&self) -> usize {
        self.col_filter_text.len()
    }

    /// The filter field in the header cell of column `id`; returns true when its text changed.
    pub(in crate::app) fn column_filter_field(
        &self,
        ui: &mut egui::Ui,
        id: ColumnId,
        text: &mut String,
    ) -> bool {
        let mut edit = egui::TextEdit::singleline(text)
            .id_salt(("column_filter", id))
            .hint_text(t!("filter.hint"))
            .desired_width(f32::INFINITY);
        let error = self.col_filter_errors.get(&id);
        if error.is_some() {
            edit = edit.background_color(ui.visuals().error_fg_color.gamma_multiply(0.35));
        } else if self.col_filters.iter().any(|(c, _)| *c == id) {
            edit = edit.background_color(ui.visuals().selection.bg_fill.gamma_multiply(0.6));
        }
        let resp = ui.add(edit);
        let changed = resp.changed();
        // An error replaces the help (#36: syntax, examples of the column, its values).
        match error {
            Some(e) => {
                resp.on_hover_text(e.text());
            }
            None => {
                resp.on_hover_ui(|ui| filter_help_ui(ui, id, false));
            }
        }
        changed
    }
}

/// The syntax help of a filter field.
fn filter_tip(kind: FilterKind) -> String {
    let mut tip = t!("filter.tip.text");
    match kind {
        FilterKind::Text => {}
        FilterKind::Number => tip.push_str(&format!("\n{}", t!("filter.tip.number"))),
        FilterKind::Address => tip.push_str(&format!("\n{}", t!("filter.tip.address"))),
    }
    tip
}

/// The help of a filter field (#36): the syntax (`short` = the one-line pattern tip of a color
/// rule instead of the full list), a grid of examples for the column and, for State and
/// Protocol, the values the column can have.
pub(in crate::app) fn filter_help_ui(ui: &mut egui::Ui, id: ColumnId, short: bool) {
    ui.set_max_width(520.0);
    if short {
        ui.label(t!("rules.pattern.tip"));
    } else {
        ui.label(filter_tip(id.filter_kind()));
    }
    ui.separator();
    ui.label(RichText::new(t!("fex.title")).strong());
    egui::Grid::new(("filter_examples", id))
        .num_columns(2)
        .spacing([14.0, 2.0])
        .show(ui, |ui| {
            for (text, key) in crate::filterhelp::examples(id) {
                ui.label(RichText::new(*text).monospace());
                ui.label(t!(key));
                ui.end_row();
            }
        });
    if let Some(values) = crate::filterhelp::values(id) {
        ui.separator();
        ui.label(t!("fex.values", values = values.join(", ")));
    }
}
