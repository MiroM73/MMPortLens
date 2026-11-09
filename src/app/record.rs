//! Recording (#3): File → Start recording… (duration, events, filter), the events of every
//! refresh handed to the writer thread, ● REC in the status bar, the automatic stop and the
//! result. The database and the threads are in `crate::recording`.

use crate::app::*;
use crate::recording::threads::Recorder;
use crate::recording::{DURATIONS, Event, EventKinds};

/// A running recording.
pub(in crate::app) struct Recording {
    pub recorder: Recorder,
    started: u64,
    /// When it stops by itself (ms since the Unix epoch).
    until: Option<u64>,
    kinds: EventKinds,
    /// The filters of the table at the start ("only matching rows").
    filter: Option<RowFilter>,
}

/// The result of the last recording (status bar).
pub(in crate::app) struct RecordResult {
    text: String,
    error: bool,
    /// The finished file (the "Open" button).
    path: Option<PathBuf>,
    until: Instant,
}

/// How long the result stays in the status bar.
const RESULT_SECS: u64 = 30;

/// `mm:ss` or `h:mm:ss`.
fn clock_len(ms: u64) -> String {
    let s = ms / 1000;
    if s >= 3600 {
        format!("{}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60)
    } else {
        format!("{:02}:{:02}", s / 60, s % 60)
    }
}

fn duration_name(minutes: Option<u64>) -> String {
    match minutes {
        None => t!("record.duration.until_stopped"),
        Some(m) if m >= 60 => t!("record.duration.hours", n = m / 60),
        Some(m) => t!("record.duration.minutes", n = m),
    }
}

impl App {
    pub(in crate::app) fn recording_active(&self) -> bool {
        self.recording.is_some()
    }

    /// The red record button of the toolbar: opens the start window, or stops the recording.
    pub(in crate::app) fn record_button(&mut self, ui: &mut egui::Ui) {
        let rec = self.recording_active();
        let h = ui.text_style_height(&egui::TextStyle::Body);
        let tip = if rec {
            t!("record.stop")
        } else {
            t!("menu.file.record")
        };
        let resp = ui
            .add(egui::Button::new("").min_size(egui::vec2(h + 8.0, h + 4.0)))
            .on_hover_text(tip);
        let c = resp.rect.center();
        let red = Color32::from_rgb(210, 40, 40);
        if rec {
            // A square: stop.
            let r = egui::Rect::from_center_size(c, egui::vec2(h * 0.55, h * 0.55));
            ui.painter().rect_filled(r, 1.0, red);
        } else {
            ui.painter().circle_filled(c, h * 0.3, red);
        }
        if resp.clicked() {
            if rec {
                self.stop_recording();
            } else {
                self.show_record = true;
            }
        }
    }

    /// File → Start recording… / Stop recording.
    pub(in crate::app) fn record_menu_items(&mut self, ui: &mut egui::Ui) {
        if self.recording_active() {
            if ui.button(t!("record.stop")).clicked() {
                ui.close();
                self.stop_recording();
            }
        } else if ui.button(t!("menu.file.record")).clicked() {
            ui.close();
            self.show_record = true;
        }
        if ui.button(t!("menu.file.open_recording")).clicked() {
            ui.close();
            self.open_recording_dialog();
        }
        if self.replay.is_some() && ui.button(t!("replay.close")).clicked() {
            ui.close();
            self.close_replay();
        }
    }

    /// The start window.
    pub(in crate::app) fn record_window(&mut self, ctx: &egui::Context) {
        if !self.show_record {
            return;
        }
        let mut open = true;
        let mut start = false;
        let mut changed = false;
        egui::Window::new(t!("record.title"))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_width(380.0)
            .show(ctx, |ui| {
                if self.recording_active() {
                    ui.label(t!("record.running"));
                    return;
                }
                let s = &mut self.cfg.record;
                ui.label(RichText::new(t!("record.duration")).strong());
                ui.horizontal_wrapped(|ui| {
                    for m in DURATIONS {
                        if ui
                            .radio(!s.use_custom && s.minutes == m, duration_name(m))
                            .clicked()
                        {
                            s.minutes = m;
                            s.use_custom = false;
                            changed = true;
                        }
                    }
                });
                ui.horizontal(|ui| {
                    if ui
                        .radio(s.use_custom, t!("record.duration.custom"))
                        .clicked()
                    {
                        s.use_custom = true;
                        changed = true;
                    }
                    let invalid = s.use_custom && s.duration_ms().is_err();
                    let mut edit = egui::TextEdit::singleline(&mut s.custom).desired_width(80.0);
                    if invalid {
                        edit =
                            edit.background_color(ui.visuals().error_fg_color.gamma_multiply(0.35));
                    }
                    let resp = ui.add(edit).on_hover_text(t!("record.duration.custom.tip"));
                    if resp.changed() {
                        s.use_custom = true;
                        changed = true;
                    }
                });
                ui.add_space(4.0);
                ui.label(RichText::new(t!("record.events")).strong());
                ui.horizontal(|ui| {
                    changed |= ui.checkbox(&mut s.kinds.new, t!("event.new")).changed();
                    changed |= ui
                        .checkbox(&mut s.kinds.closed, t!("event.closed"))
                        .changed();
                    changed |= ui
                        .checkbox(&mut s.kinds.changed, t!("event.changed"))
                        .changed();
                });
                if !s.kinds.new || !s.kinds.closed {
                    ui.colored_label(ui.visuals().warn_fg_color, t!("record.events.partial"));
                }
                ui.add_space(4.0);
                changed |= ui
                    .checkbox(&mut s.only_filtered, t!("record.only_filtered"))
                    .on_hover_text(t!("record.only_filtered.tip"))
                    .changed();
                ui.separator();
                let valid = s.duration_ms().is_ok();
                ui.horizontal(|ui| {
                    if ui
                        .add_enabled(valid, egui::Button::new(t!("record.start")))
                        .clicked()
                    {
                        start = true;
                    }
                    ui.weak(t!("record.start.tip"));
                });
            });
        if changed {
            self.settings_changed();
        }
        if start {
            self.start_recording();
            if self.recording_active() {
                open = false;
            }
        }
        self.show_record = open;
    }

    /// The filters of the table as text (recording meta).
    pub(in crate::app) fn filter_description(&self) -> String {
        self.current_filters().description()
    }

    /// Asks for the file and starts the writer thread with the current endpoints as the
    /// starting state.
    fn start_recording(&mut self) {
        let Ok(duration) = self.cfg.record.duration_ms() else {
            return;
        };
        let stamp = chrono::Local::now().format("%Y%m%d_%H%M%S");
        let ext = crate::recording::EXTENSION;
        let mut dlg = rfd::FileDialog::new()
            .add_filter(t!("record.file_type"), &[ext])
            .set_file_name(format!("recording_{stamp}.{ext}"));
        if let Some(dir) = &self.cfg.record.folder {
            dlg = dlg.set_directory(dir);
        }
        let Some(path) = dlg.save_file() else {
            return;
        };
        self.cfg.record.folder = path.parent().map(|p| p.to_path_buf());
        self.settings_changed();
        self.start_recording_to(path, duration);
    }

    /// Starts the writer thread for `path` with the current endpoints as the starting state;
    /// `duration` in ms, `None` = until stopped. Also used by the watch rules (#18).
    pub(in crate::app) fn start_recording_to(&mut self, path: PathBuf, duration: Option<u64>) {
        // The starting state is the tracker after its last snapshot: snapshots taken while the
        // file dialog was open are still queued and come later, with later times.
        let now = self.last_poll.unwrap_or_else(now_ms);
        let kinds = self.cfg.record.kinds;
        let filter = self.cfg.record.only_filtered.then(|| self.row_filter());
        let first: Vec<Event> = self
            .tracker
            .present_events(now)
            .into_iter()
            .filter(|e| filter.as_ref().is_none_or(|f| f.matches_event(e)))
            .collect();
        let description = self.filter_description();
        let mut meta = vec![
            ("computer".to_string(), crate::tabexport::computer_name()),
            ("start_ms".into(), now.to_string()),
            ("interval_ms".into(), self.cfg.interval_ms.to_string()),
            ("app_version".into(), VERSION.to_string()),
            ("events".into(), kinds.text()),
            (
                "etw".into(),
                if self.etw_status == crate::net::etw::EtwStatus::Running {
                    "on"
                } else {
                    "off"
                }
                .into(),
            ),
        ];
        if let Some(d) = duration {
            meta.push(("duration_ms".into(), d.to_string()));
        }
        if filter.is_some() && !description.is_empty() {
            meta.push(("filter".into(), description));
        }
        let ctx = self.ctx.clone();
        match Recorder::start(path.clone(), meta, first, move || ctx.request_repaint()) {
            Ok(recorder) => {
                self.record_result = None;
                self.recording = Some(Recording {
                    recorder,
                    started: now,
                    until: duration.map(|d| now + d),
                    kinds,
                    filter,
                });
                // The first refresh after the start is part of the recording.
                self.ctl.paused.store(false, Ordering::Relaxed);
            }
            Err(e) => self.record_message(t!("record.error", error = e), true, None),
        }
    }

    /// The events of one refresh: the chosen kinds of the chosen endpoints to the writer.
    pub(in crate::app) fn record_events(&mut self, events: Vec<Event>) {
        let Some(rec) = &self.recording else {
            return;
        };
        if rec.recorder.stopping() {
            return;
        }
        let events: Vec<Event> = events
            .into_iter()
            .filter(|e| rec.kinds.wants(e.kind))
            .filter(|e| rec.filter.as_ref().is_none_or(|f| f.matches_event(e)))
            .collect();
        if !events.is_empty() {
            rec.recorder.send(events);
        }
    }

    pub(in crate::app) fn stop_recording(&mut self) {
        if let Some(rec) = &mut self.recording {
            rec.recorder.stop();
        }
    }

    fn record_message(&mut self, text: String, error: bool, path: Option<PathBuf>) {
        self.record_result = Some(RecordResult {
            text,
            error,
            path,
            until: Instant::now() + Duration::from_secs(RESULT_SECS),
        });
    }

    /// Every frame: the automatic stop, the end of the writer thread, the clock in the status
    /// bar.
    pub(in crate::app) fn check_recording(&mut self, ctx: &egui::Context) {
        let Some(rec) = &mut self.recording else {
            return;
        };
        if !rec.recorder.stopping() && rec.until.is_some_and(|u| now_ms() >= u) {
            rec.recorder.stop();
        }
        match rec.recorder.finished() {
            None => ctx.request_repaint_after(Duration::from_millis(500)),
            Some(res) => {
                let path = rec.recorder.path.clone();
                self.recording = None;
                match res {
                    Ok(n) => self.record_message(
                        t!("record.done", events = n, path = path.display()),
                        false,
                        Some(path),
                    ),
                    Err(e) => self.record_message(t!("record.error", error = e), true, None),
                }
            }
        }
    }

    /// The status bar: ● REC with the time and the events, or the result of the last recording.
    pub(in crate::app) fn record_status(&mut self, ui: &mut egui::Ui) {
        if let Some(rec) = &self.recording {
            ui.separator();
            let elapsed = now_ms().saturating_sub(rec.started);
            let time = match rec.until {
                Some(u) => format!(
                    "{} / {}",
                    clock_len(elapsed),
                    clock_len(u.saturating_sub(rec.started))
                ),
                None => clock_len(elapsed),
            };
            let n = rec.recorder.written.load(Ordering::Relaxed);
            let red = Color32::from_rgb(210, 40, 40);
            let text = if rec.recorder.stopping() {
                t!("record.stopping")
            } else {
                t!("record.status", time = time, events = n)
            };
            ui.colored_label(red, RichText::new(format!("● {text}")).strong())
                .on_hover_text(t!("record.status.tip", path = rec.recorder.path.display()));
            if !rec.recorder.stopping() && ui.small_button(t!("record.stop")).clicked() {
                self.stop_recording();
            }
            return;
        }
        let Some(r) = &self.record_result else {
            return;
        };
        if Instant::now() > r.until {
            self.record_result = None;
            return;
        }
        ui.separator();
        let color = if r.error {
            ui.visuals().error_fg_color
        } else {
            ui.visuals().text_color()
        };
        ui.colored_label(color, r.text.as_str());
        if let Some(path) = r.path.clone()
            && ui.small_button(t!("record.open")).clicked()
        {
            self.record_result = None;
            self.open_recording(path);
        }
        ui.ctx().request_repaint_after(Duration::from_secs(1));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clock_lengths() {
        assert_eq!(clock_len(0), "00:00");
        assert_eq!(clock_len(65_000), "01:05");
        assert_eq!(clock_len(3_725_000), "1:02:05");
    }
}
