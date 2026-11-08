//! The ETW capture of short connections (#4): the View menu switch and the status bar item.
//! The session runs in the poll thread (`poller.rs`), the rows are added by
//! `Tracker::merge_etw`, the table marks them with ⚡ (`table.rs`).

use crate::app::*;
use crate::net::etw::{self, EtwError, EtwStatus};

impl App {
    /// View → Capture short connections changed: start / stop the session.
    pub(in crate::app) fn set_etw_capture(&mut self) {
        self.ctl.etw.store(self.cfg.etw_capture, Ordering::Relaxed);
        if !self.cfg.etw_capture {
            // The next snapshot would say so too; a paused table takes none.
            self.etw_status = EtwStatus::Off;
        }
        self.etw_rows = 0;
        self.settings_changed();
    }

    /// "⚡ ETW: N" while the capture runs, the reason when it cannot.
    pub(in crate::app) fn etw_status_item(&mut self, ui: &mut egui::Ui) {
        match &self.etw_status {
            EtwStatus::Off => {}
            EtwStatus::Running => {
                ui.separator();
                ui.label(t!("status.etw", count = self.etw_rows))
                    .on_hover_text(t!("status.etw.tip"));
            }
            EtwStatus::Error(e) => {
                ui.separator();
                let tip = match e {
                    EtwError::Denied => t!("status.etw_denied.tip"),
                    _ => t!("status.etw_failed.tip"),
                };
                ui.colored_label(ui.visuals().warn_fg_color, etw::error_text(e))
                    .on_hover_text(tip);
            }
        }
    }
}
