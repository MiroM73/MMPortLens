//! The status bar: counts, the last refresh, errors and a newer version (#15).

use crate::app::*;

impl App {
    /// The refresh time (or "Paused") is kept at the right edge of the first line; the other
    /// items get only the width left of it and wrap onto more lines in a narrow window instead
    /// of being painted over (#29).
    pub(in crate::app) fn status_bar(&mut self, ui: &mut egui::Ui) {
        let right = if self.paused() {
            Some(RichText::new(t!("status.paused")).strong())
        } else {
            self.last_poll.map(|t| {
                RichText::new(t!(
                    "status.refreshed",
                    time = crate::timefmt::clock(t),
                    ms = self.last_took.as_millis()
                ))
            })
        };
        let full = ui.available_rect_before_wrap();
        let right_w = right.as_ref().map_or(0.0, |r| {
            let font = egui::TextStyle::Body.resolve(ui.style());
            ui.painter()
                .layout_no_wrap(r.text().to_owned(), font, Color32::PLACEHOLDER)
                .size()
                .x
                + ui.spacing().item_spacing.x * 3.0
        });
        let line_h = ui.spacing().interact_size.y;
        if let Some(right) = right {
            let rect = egui::Rect::from_min_max(
                egui::pos2(full.max.x - right_w, full.min.y),
                egui::pos2(full.max.x, full.min.y + line_h),
            );
            ui.scope_builder(
                egui::UiBuilder::new()
                    .max_rect(rect)
                    .layout(egui::Layout::right_to_left(egui::Align::Center)),
                |ui| ui.label(right),
            );
        }
        // One line high like `horizontal_wrapped` (a separator spans the line, not the panel);
        // an item that does not fit starts a new line instead of being wrapped inside.
        let left = egui::Rect::from_min_max(
            full.min,
            egui::pos2((full.max.x - right_w).max(full.min.x), full.min.y + line_h),
        );
        ui.scope_builder(
            egui::UiBuilder::new()
                .max_rect(left)
                .layout(egui::Layout::left_to_right(egui::Align::Center).with_main_wrap(true)),
            |ui| {
                ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
                self.status_items(ui);
            },
        );
    }

    /// The left part of the status bar: counts, hints, messages, the newer version.
    fn status_items(&mut self, ui: &mut egui::Ui) {
        let c = &self.counts;
        ui.label(t!("status.endpoints", shown = c.shown, total = c.total));
        ui.separator();
        ui.label(t!("status.tcp_udp", tcp = c.tcp, udp = c.udp));
        ui.separator();
        ui.label(t!(
            "status.states",
            established = c.established,
            listening = c.listening,
            time_wait = c.time_wait
        ));
        if c.invalid > 0 {
            // Windows on this computer reports broken TCP states (#21).
            ui.separator();
            ui.colored_label(
                ui.visuals().warn_fg_color,
                t!("status.invalid_states", count = c.invalid),
            )
            .on_hover_text(t!("status.invalid_states.tip", count = c.invalid));
        }
        ui.separator();
        let ch = self.last_changes;
        ui.label(t!(
            "status.changes",
            new = ch.new,
            closed = ch.closed,
            changed = ch.changed
        ))
        .on_hover_text(t!("status.changes.tip"));
        let resolving = self.resolver.pending();
        if resolving > 0 {
            ui.separator();
            ui.label(t!("status.resolving", count = resolving));
        }
        if self.sel.count() > 1 {
            ui.separator();
            ui.label(t!("status.selected", count = self.sel.count()));
        }
        if self.stats_denied {
            ui.separator();
            ui.colored_label(ui.visuals().warn_fg_color, t!("status.traffic_denied"))
                .on_hover_text(t!("status.traffic_denied.tip"));
        }
        self.etw_status_item(ui);
        self.watch_status(ui);
        self.record_status(ui);
        self.admin_status(ui);
        self.action_status(ui);
        self.export_status(ui);
        if let Some(err) = &self.error {
            ui.separator();
            ui.colored_label(ui.visuals().error_fg_color, err);
        }
        if let Some(u) = &self.update_available {
            ui.separator();
            let version = u.latest.version.clone();
            if ui
                .button(RichText::new(format!("⬆ {version}")).color(Color32::from_rgb(60, 170, 80)))
                .on_hover_text(t!("update.available", version = version))
                .clicked()
            {
                self.show_update = true;
                if matches!(self.update, UpdateState::Idle | UpdateState::Failed(_)) {
                    self.update = UpdateState::Checked(u.clone());
                }
            }
        }
    }
}
