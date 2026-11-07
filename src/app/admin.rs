//! Administrator mode (#15): File → Restart as administrator, the "always" setting and the
//! status bar hint while the program runs without administrator rights.

use crate::app::*;
use crate::update::RESTORE_ARG;

/// Text of a failed elevated start.
pub(in crate::app) fn elevate_error_text(e: &ElevateError) -> String {
    match e {
        ElevateError::Cancelled => t!("error.elevate_cancelled"),
        ElevateError::Failed(error) => t!("error.elevate", error = error),
    }
}

impl App {
    /// Starts the program again elevated with the same filters and window; this one closes when
    /// the UAC prompt was confirmed, otherwise the error goes to the status bar.
    pub(in crate::app) fn restart_elevated(&mut self, ctx: &egui::Context) {
        let file = match self.session_state(ctx).save_temp() {
            Ok(file) => file,
            Err(e) => {
                self.elevate_error = Some(t!("error.elevate", error = e));
                return;
            }
        };
        let args = [
            RESTORE_ARG.to_owned(),
            file.to_string_lossy().into_owned(),
            elevation::ELEVATED_ARG.to_owned(),
        ];
        match elevation::relaunch_elevated(&args) {
            Ok(()) => {
                // The settings first, the new program reads them.
                let _ = self.cfg.save();
                self.cfg_dirty = false;
                self.ctl.stop.store(true, Ordering::Relaxed);
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            Err(e) => {
                let _ = std::fs::remove_file(&file);
                self.elevate_error = Some(elevate_error_text(&e));
            }
        }
    }

    /// The administrator items of the File menu (Windows only).
    pub(in crate::app) fn admin_menu(&mut self, ui: &mut egui::Ui) {
        if ui
            .add_enabled(
                !self.elevated,
                egui::Button::new(t!("menu.file.restart_admin")),
            )
            .on_hover_text(t!("status.not_admin.tip"))
            .clicked()
        {
            ui.close();
            self.restart_elevated(&ui.ctx().clone());
        }
        if ui
            .checkbox(&mut self.cfg.run_as_admin, t!("menu.file.always_admin"))
            .on_hover_text(t!("menu.file.always_admin.tip"))
            .changed()
        {
            self.settings_changed();
        }
    }

    /// Status bar: a button while not elevated (click = restart elevated) and the last error.
    pub(in crate::app) fn admin_status(&mut self, ui: &mut egui::Ui) {
        if !cfg!(windows) || self.elevated {
            return;
        }
        ui.separator();
        if ui
            .small_button(t!("status.not_admin"))
            .on_hover_text(t!("status.not_admin.tip"))
            .clicked()
        {
            self.elevate_error = None;
            self.restart_elevated(&ui.ctx().clone());
        }
        if let Some(e) = &self.elevate_error {
            ui.colored_label(ui.visuals().warn_fg_color, e);
        }
    }
}
