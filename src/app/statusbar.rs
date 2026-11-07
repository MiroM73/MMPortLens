//! The status bar: counts, the last refresh, errors and a newer version (#15).

use crate::app::*;

impl App {
    pub(in crate::app) fn status_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
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
            ui.separator();
            let ch = self.last_changes;
            ui.label(t!(
                "status.changes",
                new = ch.new,
                closed = ch.closed,
                changed = ch.changed
            ))
            .on_hover_text(t!("status.changes.tip"));
            self.admin_status(ui);
            if let Some(err) = &self.error {
                ui.separator();
                ui.colored_label(ui.visuals().error_fg_color, err);
            }
            if let Some(u) = &self.update_available {
                ui.separator();
                let version = u.latest.version.clone();
                if ui
                    .button(
                        RichText::new(format!("⬆ {version}")).color(Color32::from_rgb(60, 170, 80)),
                    )
                    .on_hover_text(t!("update.available", version = version))
                    .clicked()
                {
                    self.show_update = true;
                    if matches!(self.update, UpdateState::Idle | UpdateState::Failed(_)) {
                        self.update = UpdateState::Checked(u.clone());
                    }
                }
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if self.paused() {
                    ui.label(RichText::new(t!("status.paused")).strong());
                } else if let Some(t) = self.last_poll {
                    ui.label(t!(
                        "status.refreshed",
                        time = crate::timefmt::clock(t),
                        ms = self.last_took.as_millis()
                    ));
                }
            });
        });
    }
}
