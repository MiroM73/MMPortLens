//! Column filters (#1): the texts typed into the filter row under the headers, compiled into
//! `ColumnFilter`s, and the field drawn in a header cell. The syntax is in `crate::colfilter`.

use crate::app::*;
use crate::colfilter::{ColumnFilter, FilterError, FilterKind};

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
        let tip = match error {
            Some(e) => {
                edit = edit.background_color(ui.visuals().error_fg_color.gamma_multiply(0.35));
                filter_error_text(e)
            }
            None => {
                if self.col_filters.iter().any(|(c, _)| *c == id) {
                    edit =
                        edit.background_color(ui.visuals().selection.bg_fill.gamma_multiply(0.6));
                }
                filter_tip(id.filter_kind())
            }
        };
        ui.add(edit).on_hover_text(tip).changed()
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

fn filter_error_text(e: &FilterError) -> String {
    match e {
        FilterError::Regex(error) => t!("filter.error.regex", error = error),
        FilterError::Network(value) => t!("filter.error.network", value = value),
    }
}
