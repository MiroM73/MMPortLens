//! An opened recording (#3, File → Open recording…): the table shows the endpoints of the
//! recording at the time of the slider in the replay bar – with the same columns, filters,
//! sorting, colors and export as the live table; rows whose event is younger than the highlight
//! time are marked new / changed / closed. The queries run in the replay worker
//! (`crate::recording::threads`); the event list under the table is in `replay_events.rs`.
//!
//! The live refresh (and a running recording) go on in the background.

use crate::app::*;
use crate::recording::db::Info;
use crate::recording::threads::{ReplayWorker, Reply, Request};

pub(in crate::app) struct Replay {
    path: PathBuf,
    worker: ReplayWorker,
    /// `None` until the worker opened the file.
    pub(in crate::app) info: Option<Info>,
    /// The endpoints at `shown_at`.
    pub rows: Vec<Row>,
    /// The time of the slider and of the rows shown.
    at: u64,
    shown_at: Option<u64>,
    /// The events before / after `shown_at` (step buttons).
    prev: Option<u64>,
    next: Option<u64>,
    error: Option<String>,
    pub(in crate::app) events: replay_events::EventList,
}

impl Replay {
    fn ask_state(&self, hold: u64) {
        self.worker.ask(Request::State { at: self.at, hold });
    }

    pub(in crate::app) fn ask(&self, r: Request) {
        self.worker.ask(r);
    }
}

impl App {
    pub(in crate::app) fn open_recording_dialog(&mut self) {
        let mut dlg = rfd::FileDialog::new()
            .add_filter(t!("record.file_type"), &[crate::recording::EXTENSION]);
        if let Some(dir) = &self.cfg.record.folder {
            dlg = dlg.set_directory(dir);
        }
        if let Some(path) = dlg.pick_file() {
            self.cfg.record.folder = path.parent().map(|p| p.to_path_buf());
            self.settings_changed();
            self.open_recording(path);
        }
    }

    pub(in crate::app) fn open_recording(&mut self, path: PathBuf) {
        let ctx = self.ctx.clone();
        match ReplayWorker::open(path.clone(), move || ctx.request_repaint()) {
            Ok(worker) => {
                self.replay = Some(Replay {
                    path,
                    worker,
                    info: None,
                    rows: Vec::new(),
                    at: 0,
                    shown_at: None,
                    prev: None,
                    next: None,
                    error: None,
                    events: Default::default(),
                });
                self.sel.clear();
                self.view_dirty = true;
            }
            Err(e) => self.replay_failed(&path, e.to_string()),
        }
    }

    fn replay_failed(&mut self, path: &std::path::Path, error: String) {
        self.error = Some(t!("replay.error", path = path.display(), error = error));
    }

    /// Back to the live endpoints.
    pub(in crate::app) fn close_replay(&mut self) {
        if self.replay.take().is_some() {
            self.sel.clear();
            self.view_dirty = true;
        }
    }

    /// Takes the answers of the replay worker (every frame).
    pub(in crate::app) fn receive_replay(&mut self) {
        let hold = self.cfg.highlight_ms;
        let Some(rp) = &mut self.replay else {
            return;
        };
        let mut failed = None;
        while let Ok(reply) = rp.worker.rx.try_recv() {
            match reply {
                Reply::Opened(Ok(info)) => {
                    rp.at = info.first.unwrap_or(0);
                    rp.info = Some(info);
                    rp.ask_state(hold);
                    rp.events.restart();
                }
                Reply::Opened(Err(e)) => failed = Some(e),
                Reply::State {
                    at,
                    rows,
                    prev,
                    next,
                } => {
                    rp.rows = rows;
                    rp.shown_at = Some(at);
                    rp.prev = prev;
                    rp.next = next;
                    rp.error = None;
                    self.view_dirty = true;
                }
                Reply::Events {
                    generation,
                    page,
                    total,
                    events,
                } => rp.events.receive(generation, page, total, events),
                Reply::Error(e) => rp.error = Some(e),
            }
        }
        if let Some(e) = failed {
            let path = rp.path.clone();
            self.replay = None;
            self.view_dirty = true;
            self.replay_failed(&path, e);
        }
    }

    /// Moves the slider to `at` and asks for the state there.
    pub(in crate::app) fn replay_seek(&mut self, at: u64) {
        let hold = self.cfg.highlight_ms;
        if let Some(rp) = &mut self.replay {
            let (lo, hi) = match &rp.info {
                Some(i) => (i.first.unwrap_or(0), i.last.unwrap_or(0)),
                None => return,
            };
            rp.at = at.clamp(lo, hi);
            rp.ask_state(hold);
        }
    }

    /// The bar under the toolbar while a recording is open: name and range, the time slider,
    /// steps to the previous / next event, back to live.
    pub(in crate::app) fn replay_bar(&mut self, ui: &mut egui::Ui) {
        let Some(rp) = &self.replay else {
            return;
        };
        let name = rp
            .path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let mut seek = None;
        let mut close = false;
        let info = rp.info.clone();
        let (at, prev, next) = (rp.at, rp.prev, rp.next);
        let busy = rp.shown_at != Some(rp.at);
        let error = rp.error.clone();
        let path = rp.path.clone();
        egui::Panel::top("replay").show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new(t!("replay.label", name = name)).strong())
                    .on_hover_text(replay_tip(&path, info.as_ref()));
                let Some(info) = &info else {
                    ui.spinner();
                    ui.weak(t!("replay.opening"));
                    return;
                };
                let (Some(first), Some(last)) = (info.first, info.last) else {
                    ui.weak(t!("replay.empty"));
                    if ui.button(t!("replay.close")).clicked() {
                        close = true;
                    }
                    return;
                };
                if crate::app::table::arrow_button(ui, prev.is_some(), false)
                    .on_hover_text(t!("replay.prev"))
                    .clicked()
                {
                    seek = prev;
                }
                if crate::app::table::arrow_button(ui, next.is_some(), true)
                    .on_hover_text(t!("replay.next"))
                    .clicked()
                {
                    seek = next;
                }
                // The time of the slider: `YYYY-MM-DD HH:MM:SS.mmm` and the offset from the start.
                let label = format!(
                    "{}  (+{})",
                    crate::timefmt::stamp(at, first),
                    crate::timefmt::duration(at - first)
                );
                ui.monospace(label);
                if busy {
                    ui.spinner();
                }
                let right = ui.spacing().interact_size.x * 2.0 + 60.0;
                ui.spacing_mut().slider_width = (ui.available_width() - right).max(120.0);
                let mut v = at;
                let resp = ui
                    .add(
                        egui::Slider::new(&mut v, first..=last.max(first + 1))
                            .show_value(false)
                            .smart_aim(false),
                    )
                    .on_hover_text(t!("replay.slider.tip"));
                if resp.changed() {
                    seek = Some(v);
                }
                if ui
                    .button(t!("replay.close"))
                    .on_hover_text(t!("replay.close.tip"))
                    .clicked()
                {
                    close = true;
                }
            });
            if let Some(e) = &error {
                ui.colored_label(ui.visuals().error_fg_color, e);
            }
        });
        if let Some(t) = seek {
            self.replay_seek(t);
        }
        if close {
            self.close_replay();
        }
    }

    /// While a recording is open the rows are not live: no closing, ending or properties.
    pub(in crate::app) fn replaying(&self) -> bool {
        self.replay.is_some()
    }
}

/// The tooltip of the replay bar: the file and what the recording says about itself.
fn replay_tip(path: &std::path::Path, info: Option<&Info>) -> String {
    let mut lines = vec![path.display().to_string()];
    let Some(info) = info else {
        return lines.join("\n");
    };
    let time = |k: &str| {
        info.meta(k)
            .and_then(|v| v.parse::<u64>().ok())
            .map(crate::timefmt::date_time)
    };
    if let Some(c) = info.meta("computer") {
        lines.push(t!("replay.info.computer", value = c));
    }
    if let Some(s) = time("start_ms") {
        lines.push(t!("replay.info.start", value = s));
    }
    if let Some(e) = time("end_ms") {
        lines.push(t!("replay.info.end", value = e));
    }
    if let Some(i) = info.meta("interval_ms") {
        lines.push(t!("replay.info.interval", value = i));
    }
    if let Some(k) = info.meta("events") {
        lines.push(t!("replay.info.kinds", value = k));
    }
    if let Some(f) = info.meta("filter") {
        lines.push(t!("replay.info.filter", value = f));
    }
    if let Some(v) = info.meta("app_version") {
        lines.push(t!("replay.info.version", value = v));
    }
    lines.push(t!("replay.info.events", value = info.events));
    lines.join("\n")
}
