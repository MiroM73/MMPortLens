//! New versions (#15, taken from RusTrace): check, download + install, restart with the same
//! filters and window.

use crate::app::*;

impl App {
    /// Asks GitHub for the releases in a background thread.
    pub(in crate::app) fn start_update_check(&mut self, silent: bool) {
        if matches!(
            self.update,
            UpdateState::Checking { .. } | UpdateState::Installing { .. }
        ) {
            return;
        }
        let (tx, rx) = mpsc::channel();
        let ctx = self.ctx.clone();
        let spawned = std::thread::Builder::new()
            .name("mmportlens-update".into())
            .spawn(move || {
                let _ = tx.send(update::fetch_updates());
                ctx.request_repaint();
            });
        self.update = match spawned {
            Ok(_) => UpdateState::Checking { rx, silent },
            Err(e) => UpdateState::Failed(e.to_string().into()),
        };
    }

    /// Takes over the results of the check / installation.
    pub(in crate::app) fn poll_update(&mut self, ctx: &egui::Context) {
        match &self.update {
            UpdateState::Checking { rx, silent } => {
                let silent = *silent;
                let res = match rx.try_recv() {
                    Ok(res) => res,
                    Err(mpsc::TryRecvError::Empty) => return,
                    Err(mpsc::TryRecvError::Disconnected) => Err(t!("error.internal").into()),
                };
                // A check that failed because GitHub or the network had a passing problem does
                // not count, so "once a day" tries again at the next start (#26).
                if !matches!(&res, Err(e) if e.transient) {
                    self.cfg.update_last_check = chrono::Local::now().timestamp();
                    self.settings_changed();
                }
                self.update = match res {
                    Ok(u) => {
                        let newer = u.is_newer();
                        self.update_available = newer.then(|| u.clone());
                        // A skipped version opens no window at start-up, a newer one does.
                        if silent && !(newer && u.latest.version != self.cfg.update_skipped) {
                            UpdateState::Idle
                        } else {
                            self.show_update = true;
                            UpdateState::Checked(u)
                        }
                    }
                    // A failed automatic check (e.g. offline) is not reported.
                    Err(_) if silent => UpdateState::Idle,
                    Err(e) => UpdateState::Failed(e),
                };
            }
            UpdateState::Installing {
                rx, release, exe, ..
            } => {
                let res = match rx.try_recv() {
                    Ok(res) => res,
                    Err(mpsc::TryRecvError::Empty) => {
                        ctx.request_repaint_after(Duration::from_millis(100));
                        return;
                    }
                    Err(mpsc::TryRecvError::Disconnected) => Err(t!("error.internal")),
                };
                let (release, exe) = (release.clone(), exe.clone());
                let res = res.and_then(|warnings| {
                    for w in warnings {
                        eprintln!("update: {w}");
                    }
                    // The settings first, the new program reads them.
                    let _ = self.cfg.save();
                    self.cfg_dirty = false;
                    update::restart(&exe, &self.session_state(ctx))
                        .map_err(|e| t!("update.restart_failed", error = e))
                });
                match res {
                    Ok(()) => {
                        self.ctl.stop.store(true, Ordering::Relaxed);
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                    Err(error) => self.update = UpdateState::InstallFailed { error, release },
                }
            }
            _ => {}
        }
    }

    /// Downloads and installs the release in a background thread.
    fn start_install(&mut self, release: Release) {
        let exe = match std::env::current_exe() {
            Ok(exe) => exe,
            Err(e) => {
                self.update = UpdateState::InstallFailed {
                    error: e.to_string(),
                    release,
                };
                return;
            }
        };
        let progress = Arc::new(update::Progress::default());
        let (tx, rx) = mpsc::channel();
        let (prog, rel, path, ctx) = (
            progress.clone(),
            release.clone(),
            exe.clone(),
            self.ctx.clone(),
        );
        let spawned = std::thread::Builder::new()
            .name("mmportlens-update".into())
            .spawn(move || {
                let _ = tx.send(update::download_and_install(&rel, &path, &prog));
                ctx.request_repaint();
            });
        self.update = match spawned {
            Ok(_) => UpdateState::Installing {
                rx,
                progress,
                release,
                exe,
            },
            Err(e) => UpdateState::InstallFailed {
                error: e.to_string(),
                release,
            },
        };
    }

    /// The filters and the window, for the restarted program.
    pub(in crate::app) fn session_state(&self, ctx: &egui::Context) -> SessionState {
        // egui points include the UI zoom, the window is placed in unzoomed logical pixels.
        let zoom = ctx.zoom_factor();
        let window = ctx.input(|i| {
            let vp = i.viewport();
            let maximized = vp.maximized.unwrap_or(false);
            WindowState {
                // A maximized window keeps its normal size from the default.
                pos: vp
                    .outer_rect
                    .filter(|_| !maximized)
                    .map(|r| [r.min.x * zoom, r.min.y * zoom]),
                size: vp
                    .inner_rect
                    .filter(|_| !maximized)
                    .map(|r| [r.width() * zoom, r.height() * zoom]),
                maximized,
            }
        });
        SessionState {
            filter: self.filter.clone(),
            paused: self.paused(),
            col_filters: self.column_filter_list(),
            window: Some(window),
        }
    }

    /// Takes over the filters of the previous version after an update (the window position is
    /// applied in `main`).
    pub(in crate::app) fn restore_session(&mut self, state: SessionState) {
        self.filter = state.filter;
        self.ctl.paused.store(state.paused, Ordering::Relaxed);
        if !state.col_filters.is_empty() {
            self.set_column_filters(state.col_filters);
        }
        self.view_dirty = true;
    }

    /// Help → Check for updates.
    pub(in crate::app) fn update_window(&mut self, ctx: &egui::Context) {
        if !self.show_update {
            return;
        }
        let installing = matches!(self.update, UpdateState::Installing { .. });
        let mut open = true;
        let mut action = None;
        let mut auto_changed = false;
        egui::Window::new(t!("update.title"))
            .id(egui::Id::unique("update_window"))
            .open(&mut open)
            .resizable(false)
            .collapsible(false)
            .default_width(520.0)
            .show(ctx, |ui| {
                let red = ui.visuals().error_fg_color;
                let orange = ui.visuals().warn_fg_color;
                ui.label(t!("update.current", version = update::current_version()));
                ui.add_space(6.0);
                match &self.update {
                    UpdateState::Idle => {}
                    UpdateState::Checking { .. } => {
                        ui.horizontal(|ui| {
                            ui.spinner();
                            ui.label(t!("update.checking"));
                        });
                    }
                    UpdateState::Failed(e) => {
                        ui.colored_label(red, t!("update.check_failed", error = e));
                        if e.server {
                            ui.hyperlink_to(t!("update.github_status"), update::GITHUB_STATUS_URL);
                        }
                    }
                    UpdateState::Checked(u) if u.is_newer() => {
                        let r = &u.latest;
                        ui.label(
                            RichText::new(t!("update.new_version", version = r.version)).strong(),
                        );
                        if u.missed.len() > 1 {
                            ui.label(t!("update.behind", count = u.missed.len()));
                        }
                        ui.add_space(4.0);
                        egui::ScrollArea::vertical()
                            .max_height(360.0)
                            .auto_shrink([false, true])
                            .show(ui, |ui| match u.missed.as_slice() {
                                [only] => notes_ui(ui, &only.notes),
                                // One section per missed version, the newest one open.
                                missed => {
                                    for (i, m) in missed.iter().enumerate() {
                                        let title = if m.date.is_empty() {
                                            m.version.clone()
                                        } else {
                                            format!("{} – {}", m.version, m.date)
                                        };
                                        egui::CollapsingHeader::new(RichText::new(title).strong())
                                            .id_salt(("update_notes", &m.version))
                                            .default_open(i == 0)
                                            .show(ui, |ui| notes_ui(ui, &m.notes));
                                    }
                                }
                            });
                        ui.add_space(6.0);
                        let can_install = cfg!(windows) && r.zip.is_some() && r.sha256.is_some();
                        if can_install {
                            let dir = std::env::current_exe()
                                .ok()
                                .and_then(|p| p.parent().map(|d| d.display().to_string()))
                                .unwrap_or_default();
                            ui.add(egui::Label::new(t!("update.install_hint", dir = dir)).wrap());
                        } else {
                            ui.colored_label(orange, t!("update.no_package"));
                        }
                        ui.add_space(4.0);
                        ui.horizontal(|ui| {
                            if ui
                                .add_enabled(
                                    can_install,
                                    egui::Button::new(format!("⬇ {}", t!("update.install"))),
                                )
                                .clicked()
                            {
                                action = Some(UpdateAction::Install(r.clone()));
                            }
                            if ui
                                .button(format!("🌐 {}", t!("update.release_page")))
                                .clicked()
                            {
                                ui.ctx().open_url(egui::OpenUrl::new_tab(&r.page));
                            }
                            if ui.button(t!("update.skip")).clicked() {
                                action = Some(UpdateAction::Skip(r.version.clone()));
                            }
                            if ui.button(t!("update.later")).clicked() {
                                action = Some(UpdateAction::Close);
                            }
                        });
                    }
                    UpdateState::Checked(Updates { latest: r, .. }) => {
                        ui.colored_label(
                            Color32::from_rgb(60, 170, 80),
                            format!("✔ {}", t!("update.up_to_date", version = r.version)),
                        );
                        ui.hyperlink_to(t!("update.release_page"), &r.page);
                    }
                    UpdateState::Installing {
                        progress, release, ..
                    } => {
                        ui.horizontal(|ui| {
                            ui.spinner();
                            ui.label(t!("update.downloading", version = release.version));
                        });
                        let received = progress.received.load(Ordering::Relaxed);
                        let total = progress.total.load(Ordering::Relaxed);
                        let (fraction, text) = if total > 0 {
                            let text =
                                format!("{} / {}", format_size(received), format_size(total));
                            (received as f32 / total as f32, text)
                        } else {
                            (0.0, format_size(received))
                        };
                        ui.add(egui::ProgressBar::new(fraction).text(text));
                        if ui.button(t!("update.cancel")).clicked() {
                            progress.cancel.store(true, Ordering::Relaxed);
                        }
                    }
                    UpdateState::InstallFailed { error, release } => {
                        ui.colored_label(red, t!("update.install_failed", error = error));
                        ui.add(egui::Label::new(t!("update.install_failed_hint")).wrap());
                        ui.hyperlink_to(t!("update.release_page"), &release.page);
                    }
                }
                ui.add_space(6.0);
                let busy = matches!(
                    self.update,
                    UpdateState::Checking { .. } | UpdateState::Installing { .. }
                );
                if ui
                    .add_enabled(
                        !busy,
                        egui::Button::new(format!("🔄 {}", t!("update.check_again"))),
                    )
                    .clicked()
                {
                    action = Some(UpdateAction::Check);
                }
                ui.separator();
                ui.horizontal(|ui| {
                    ui.label(t!("update.when"));
                    for when in UpdateCheck::ALL {
                        auto_changed |= ui
                            .radio_value(&mut self.cfg.update_when, when, when.label())
                            .changed();
                    }
                });
            });
        if auto_changed {
            self.settings_changed();
        }
        // The window stays open while the update is being installed.
        self.show_update = open || installing;
        match action {
            Some(UpdateAction::Install(r)) => self.start_install(r),
            Some(UpdateAction::Check) => self.start_update_check(false),
            Some(UpdateAction::Skip(v)) => {
                self.cfg.update_skipped = v;
                self.settings_changed();
                self.update_available = None;
                self.show_update = false;
            }
            Some(UpdateAction::Close) => self.show_update = false,
            None => {}
        }
    }
}

/// Release notes as plain lines (headings bold).
fn notes_ui(ui: &mut egui::Ui, notes: &str) {
    for (heading, line) in update::plain_notes(notes) {
        if heading {
            ui.add_space(4.0);
            ui.label(RichText::new(line).strong());
        } else {
            ui.add(egui::Label::new(line).wrap());
        }
    }
}

/// "1.4 MB" / "830 KB" for the download progress.
fn format_size(bytes: u64) -> String {
    if bytes >= 1 << 20 {
        format!("{:.1} MB", bytes as f64 / (1u64 << 20) as f64)
    } else {
        format!("{} KB", bytes.div_ceil(1024))
    }
}

/// A button pressed in the update window.
enum UpdateAction {
    Install(Release),
    Check,
    Skip(String),
    Close,
}

#[cfg(test)]
mod tests {
    use super::format_size;

    #[test]
    fn sizes() {
        assert_eq!(format_size(0), "0 KB");
        assert_eq!(format_size(1500), "2 KB");
        assert_eq!(format_size(3 << 20), "3.0 MB");
    }
}
