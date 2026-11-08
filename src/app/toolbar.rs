//! The toolbar: pause / refresh, interval, protocol switches, the text filter and clearing the
//! column filters.

use crate::app::*;
use crate::config::INTERVALS;

/// "500 ms", "2 s" …
fn interval_label(ms: u64) -> String {
    // `--interval` (#16) may give any number of ms.
    if ms < 1000 || !ms.is_multiple_of(1000) {
        t!("interval.ms", n = ms)
    } else {
        t!("interval.s", n = ms / 1000)
    }
}

impl App {
    pub(in crate::app) fn toolbar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            let paused = self.paused();
            let (icon, tip) = if paused {
                ("▶", t!("toolbar.resume"))
            } else {
                ("⏸", t!("toolbar.pause"))
            };
            if ui.button(icon).on_hover_text(tip).clicked() {
                self.ctl.paused.store(!paused, Ordering::Relaxed);
            }
            if ui
                .button("⟳")
                .on_hover_text(t!("toolbar.refresh"))
                .clicked()
            {
                self.ctl.refresh_now.store(true, Ordering::Relaxed);
            }
            self.record_button(ui);

            let mut ms = self.cfg.interval_ms;
            egui::ComboBox::from_id_salt("interval")
                .selected_text(interval_label(ms))
                .width(70.0)
                .show_ui(ui, |ui| {
                    for v in INTERVALS {
                        ui.selectable_value(&mut ms, v, interval_label(v));
                    }
                })
                .response
                .on_hover_text(t!("toolbar.interval"));
            if ms != self.cfg.interval_ms {
                self.cfg.interval_ms = ms;
                self.ctl.interval_ms.store(ms, Ordering::Relaxed);
                self.run_only.interval_ms = None;
                self.settings_changed();
            }

            ui.separator();
            self.preset_combo(ui);
            let mut changed = false;
            changed |= ui.toggle_value(&mut self.cfg.show_tcp, "TCP").changed();
            changed |= ui.toggle_value(&mut self.cfg.show_udp, "UDP").changed();
            changed |= ui.toggle_value(&mut self.cfg.show_ipv4, "IPv4").changed();
            changed |= ui.toggle_value(&mut self.cfg.show_ipv6, "IPv6").changed();
            changed |= ui
                .toggle_value(&mut self.cfg.show_listening, t!("toolbar.listening"))
                .on_hover_text(t!("toolbar.listening.tip"))
                .changed();
            changed |= ui
                .toggle_value(&mut self.cfg.resolve_addresses, "DNS")
                .on_hover_text(t!("menu.view.resolve_addresses.tip"))
                .changed();
            if changed {
                self.view_dirty = true;
                self.settings_changed();
            }
            self.group_toggle(ui);

            ui.separator();
            let edit = egui::TextEdit::singleline(&mut self.filter)
                .id(egui::Id::unique(input::FILTER_ID))
                .hint_text(t!("toolbar.filter.hint"))
                .desired_width(260.0);
            if ui
                .add(edit)
                .on_hover_text(t!("toolbar.filter.tip"))
                .changed()
            {
                self.view_dirty = true;
            }
            if !self.filter.is_empty() && ui.small_button("✖").clicked() {
                self.filter.clear();
                self.view_dirty = true;
            }

            // Column filters (#1): shown also while the filter row is hidden, so a hidden
            // filter is never forgotten.
            let count = self.column_filter_count();
            if count > 0 {
                ui.separator();
                if ui
                    .button(format!("✖ {}", t!("filter.clear", count = count)))
                    .on_hover_text(t!("filter.clear.tip"))
                    .clicked()
                {
                    self.clear_column_filters();
                }
            }
        });
    }
}
