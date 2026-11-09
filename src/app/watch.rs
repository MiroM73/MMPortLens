//! View → Watch rules and alerts (#18): the hits of the watch rules (`crate::watch`) with a jump
//! to the row, the rule editor, and the actions of a hit (notification, sound, window to the
//! front, log file, recording). The window is a zoomable tool window of `native_window.rs`.

use crate::app::native_window::ToolSpec;
use crate::app::*;
use crate::watch::{Condition, Hit, Trigger, WatchRule};

/// At most this many hits are kept in the list (the log file has all of them).
const MAX_HITS: usize = 1000;

/// The tab of the window.
#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub(in crate::app) enum WatchTab {
    #[default]
    Alerts,
    Rules,
}

/// What the user did in the window, applied after drawing.
#[derive(Default)]
struct WatchAction {
    jump: Option<usize>,
    clear: bool,
    rules_changed: bool,
    choose_log: bool,
    open_log: bool,
}

impl App {
    pub(in crate::app) fn recompile_watch(&mut self) {
        self.watch_rules = crate::watch::Compiled::new(&self.cfg.watch.rules);
    }

    /// View → Watch rules and alerts…, or the 🔔 of the status bar (`alerts` = the Alerts tab).
    pub(in crate::app) fn open_watch(&mut self, alerts: bool) {
        if alerts {
            self.watch_tab = WatchTab::Alerts;
        }
        self.watch_win.open();
    }

    /// The events of one refresh against the rules: new hits go to the list, the log file, the
    /// notification and the sound at once; focus and recording wait for `watch_actions`.
    pub(in crate::app) fn watch_events(&mut self, events: &[crate::recording::Event]) {
        if !self.watch_rules.any_on() || self.replaying() {
            return;
        }
        let hits = crate::watch::check(&self.cfg.watch.rules, &self.watch_rules, events);
        if hits.is_empty() {
            return;
        }
        let b = crate::watch::batch(&hits);
        if !b.log.is_empty()
            && let Some(path) = crate::watch::log_path(&self.cfg.watch)
            && let Err(e) = crate::watch::append_log(&path, &b.log)
        {
            self.watch_msg = Some(t!("watch.log.error", error = e));
        }
        if let Some((title, text)) = &b.notify {
            // Without a notification area (Linux) the 🔔 of the status bar still shows the hits.
            let shown = self.notifier.show(self.hwnd, title, text);
            if cfg!(windows) && !shown {
                self.watch_msg = Some(t!("watch.notify.failed"));
            }
        }
        if b.sound {
            crate::notify::beep();
        }
        self.watch_focus |= b.focus;
        if b.record && self.recording.is_none() {
            self.watch_record();
        }
        self.watch_unseen += hits.len();
        self.watch_hits.extend(hits);
        while self.watch_hits.len() > MAX_HITS {
            self.watch_hits.pop_front();
        }
    }

    /// The actions that need the context: the main window to the front.
    pub(in crate::app) fn watch_actions(&mut self, ctx: &egui::Context) {
        if std::mem::take(&mut self.watch_focus) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
            ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
            ctx.send_viewport_cmd(egui::ViewportCommand::RequestUserAttention(
                egui::UserAttentionType::Informational,
            ));
        }
    }

    /// A recording started by a rule: `alert_<time>.plrec` in the folder of the last recording
    /// (else the documents folder), for `record_minutes`.
    fn watch_record(&mut self) {
        let dir = self
            .cfg
            .record
            .folder
            .clone()
            .or_else(dirs::document_dir)
            .unwrap_or_else(std::env::temp_dir);
        let stamp = chrono::Local::now().format("%Y%m%d_%H%M%S");
        let path = dir.join(format!("alert_{stamp}.{}", crate::recording::EXTENSION));
        let minutes = self.cfg.watch.record_minutes.max(1);
        self.start_recording_to(path, Some(minutes * 60_000));
    }

    /// 🔔 n in the status bar while there are hits not seen in the window.
    pub(in crate::app) fn watch_status(&mut self, ui: &mut egui::Ui) {
        if self.watch_unseen == 0 {
            if let Some(msg) = &self.watch_msg {
                ui.separator();
                ui.colored_label(ui.visuals().error_fg_color, msg);
            }
            return;
        }
        ui.separator();
        let text = RichText::new(t!("watch.status", count = self.watch_unseen))
            .color(ui.visuals().warn_fg_color);
        if ui
            .button(text)
            .on_hover_text(t!("watch.status.tip"))
            .clicked()
        {
            self.open_watch(true);
        }
    }

    /// Draws the window when it is open.
    pub(in crate::app) fn watch_window(&mut self, ctx: &egui::Context) {
        if !self.watch_win.is_open() {
            return;
        }
        if self.watch_tab == WatchTab::Alerts {
            self.watch_unseen = 0;
        }
        let spec = ToolSpec {
            title: t!("watch.title"),
            default_size: [900.0, 560.0],
            min_size: [480.0, 300.0],
        };
        let native = self.cfg.separate_windows && !ctx.embed_viewports();
        let mut state = self.cfg.watch_window.clone();
        let mut act = WatchAction::default();
        let mut tab = self.watch_tab;
        let log = crate::watch::log_path(&self.cfg.watch);
        {
            let hits = &self.watch_hits;
            let settings = &mut self.cfg.watch;
            let compiled = &self.watch_rules;
            self.watch_win.show(ctx, native, &spec, &mut state, |ui| {
                ui.horizontal(|ui| {
                    ui.selectable_value(
                        &mut tab,
                        WatchTab::Alerts,
                        t!("watch.tab.alerts", count = hits.len()),
                    );
                    ui.selectable_value(&mut tab, WatchTab::Rules, t!("watch.tab.rules"));
                });
                ui.separator();
                match tab {
                    WatchTab::Alerts => alerts_ui(ui, hits, log.as_deref(), &mut act),
                    WatchTab::Rules => rules_ui(ui, settings, compiled, log.as_deref(), &mut act),
                }
            });
        }
        self.watch_tab = tab;
        if state != self.cfg.watch_window {
            self.cfg.watch_window = state;
            self.settings_changed();
        }
        if act.clear {
            self.watch_hits.clear();
            self.watch_unseen = 0;
        }
        if act.rules_changed {
            self.recompile_watch();
            self.settings_changed();
        }
        if act.choose_log {
            let mut dlg = rfd::FileDialog::new()
                .add_filter("log", &["log", "txt"])
                .set_file_name("alerts.log");
            if let Some(dir) = log.as_ref().and_then(|p| p.parent()) {
                dlg = dlg.set_directory(dir);
            }
            if let Some(path) = dlg.save_file() {
                self.cfg.watch.log_file = Some(path);
                self.settings_changed();
            }
        }
        if act.open_log
            && let Some(path) = &log
            && let Err(e) = open::that_detached(path)
        {
            self.watch_msg = Some(t!("watch.log.error", error = e));
        }
        if let Some(i) = act.jump {
            self.jump_to_hit(i);
        }
    }

    /// Selects the row of the hit `index` in the table and scrolls to it.
    fn jump_to_hit(&mut self, index: usize) {
        let Some(key) = self.watch_hits.get(index).map(Hit::key) else {
            return;
        };
        let keys = self.view_keys();
        let text = match keys.iter().position(|k| *k == key) {
            Some(pos) => {
                self.sel.select_only(key);
                self.scroll_to = Some(pos);
                return;
            }
            None if self.tracker.contains(&key) => t!("watch.jump.filtered"),
            None => t!("watch.jump.gone"),
        };
        self.action_result = Some(actions::ActionResult { text, error: true });
    }
}

/// The Alerts tab: newest first; a click on ➡ selects the row in the table.
fn alerts_ui(
    ui: &mut egui::Ui,
    hits: &std::collections::VecDeque<Hit>,
    log: Option<&std::path::Path>,
    act: &mut WatchAction,
) {
    ui.horizontal(|ui| {
        if ui
            .add_enabled(!hits.is_empty(), egui::Button::new(t!("watch.clear")))
            .clicked()
        {
            act.clear = true;
        }
        if let Some(path) = log
            && ui
                .button(t!("watch.log.open"))
                .on_hover_text(path.display().to_string())
                .clicked()
        {
            act.open_log = true;
        }
    });
    if hits.is_empty() {
        ui.label(t!("watch.no_alerts"));
        return;
    }
    egui::ScrollArea::both()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            egui::Grid::new("watch_hits")
                .striped(true)
                .num_columns(5)
                .spacing([12.0, 4.0])
                .show(ui, |ui| {
                    ui.strong(t!("watch.col.time"));
                    ui.strong(t!("watch.col.rule"));
                    ui.strong(t!("watch.col.event"));
                    ui.strong(t!("watch.col.endpoint"));
                    ui.label("");
                    ui.end_row();
                    for (i, h) in hits.iter().enumerate().rev() {
                        ui.label(crate::timefmt::date_time(h.time));
                        ui.label(&h.rule);
                        ui.label(crate::watch::event_label(h.kind));
                        ui.label(h.summary());
                        if ui
                            .small_button("➡")
                            .on_hover_text(t!("watch.jump.tip"))
                            .clicked()
                        {
                            act.jump = Some(i);
                        }
                        ui.end_row();
                    }
                });
        });
}

/// The Rules tab: one frame per rule with its conditions and actions.
fn rules_ui(
    ui: &mut egui::Ui,
    settings: &mut crate::watch::WatchSettings,
    compiled: &crate::watch::Compiled,
    log: Option<&std::path::Path>,
    act: &mut WatchAction,
) {
    ui.label(t!("watch.rules.intro"));
    let mut changed = false;
    let mut remove = None;
    let mut swap = None;
    let n = settings.rules.len();
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .max_height(ui.available_height() - 70.0)
        .show(ui, |ui| {
            for (i, rule) in settings.rules.iter_mut().enumerate() {
                egui::Frame::group(ui.style()).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    changed |= rule_ui(ui, i, n, rule, compiled.error(i), &mut remove, &mut swap);
                });
            }
        });
    if let Some(i) = remove {
        settings.rules.remove(i);
        changed = true;
    }
    if let Some((a, b)) = swap {
        settings.rules.swap(a, b);
        changed = true;
    }
    ui.separator();
    ui.horizontal(|ui| {
        if ui.button(t!("watch.rules.add")).clicked() {
            settings.rules.push(WatchRule::default());
            changed = true;
        }
        if ui
            .button(t!("watch.rules.examples"))
            .on_hover_text(t!("watch.rules.examples.tip"))
            .clicked()
        {
            for ex in crate::watch::default_rules() {
                if !settings.rules.iter().any(|r| r.name == ex.name) {
                    settings.rules.push(ex);
                }
            }
            changed = true;
        }
        ui.separator();
        ui.label(t!("watch.record_minutes"));
        changed |= ui
            .add(egui::DragValue::new(&mut settings.record_minutes).range(1..=1440))
            .changed();
    });
    ui.horizontal(|ui| {
        ui.label(t!("watch.log.file"));
        // The buttons first: a long path is cut at the window edge, not the buttons.
        if ui.button(t!("watch.log.choose")).clicked() {
            act.choose_log = true;
        }
        if settings.log_file.is_some() && ui.button(t!("watch.log.default")).clicked() {
            settings.log_file = None;
            changed = true;
        }
        let path = log.map(|p| p.display().to_string()).unwrap_or_default();
        ui.add(egui::Label::new(RichText::new(&path).monospace()).truncate())
            .on_hover_text(path);
    });
    act.rules_changed |= changed;
}

/// One rule; returns true when it changed.
fn rule_ui(
    ui: &mut egui::Ui,
    i: usize,
    n: usize,
    rule: &mut WatchRule,
    error: Option<(usize, &crate::colfilter::FilterError)>,
    remove: &mut Option<usize>,
    swap: &mut Option<(usize, usize)>,
) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        changed |= ui
            .checkbox(&mut rule.enabled, "")
            .on_hover_text(t!("watch.rule.enabled"))
            .changed();
        changed |= ui
            .add(egui::TextEdit::singleline(&mut rule.name).desired_width(220.0))
            .changed();
        ui.label(t!("watch.rule.when"));
        egui::ComboBox::from_id_salt(("watch_trigger", i))
            .selected_text(rule.trigger.label())
            .show_ui(ui, |ui| {
                for t in Trigger::ALL {
                    changed |= ui
                        .selectable_value(&mut rule.trigger, t, t.label())
                        .changed();
                }
            });
        if ui.add_enabled(i > 0, egui::Button::new("⬆")).clicked() {
            *swap = Some((i - 1, i));
        }
        if ui.add_enabled(i + 1 < n, egui::Button::new("⬇")).clicked() {
            *swap = Some((i, i + 1));
        }
        if ui
            .button("🗑")
            .on_hover_text(t!("watch.rule.delete"))
            .clicked()
        {
            *remove = Some(i);
        }
    });
    let mut drop_cond = None;
    for (ci, c) in rule.conditions.iter_mut().enumerate() {
        ui.horizontal(|ui| {
            ui.add_space(24.0);
            ui.label(if ci == 0 {
                t!("watch.rule.if")
            } else {
                t!("watch.rule.and")
            });
            egui::ComboBox::from_id_salt(("watch_col", i, ci))
                .selected_text(c.column.title())
                .show_ui(ui, |ui| {
                    for col in ColumnId::ALL {
                        changed |= ui
                            .selectable_value(&mut c.column, col, col.title())
                            .changed();
                    }
                });
            let bad = error.filter(|(e, _)| *e == ci).map(|(_, e)| e.text());
            let mut edit = egui::TextEdit::singleline(&mut c.pattern)
                .desired_width(260.0)
                .hint_text(t!("watch.rule.pattern.hint"));
            if bad.is_some() {
                edit = edit.text_color(ui.visuals().error_fg_color);
            }
            let resp = ui.add(edit);
            changed |= resp.changed();
            match bad {
                Some(e) => resp.on_hover_text(e),
                None => resp.on_hover_text(t!("rules.pattern.tip")),
            };
            if ui.small_button("✖").clicked() {
                drop_cond = Some(ci);
            }
        });
    }
    if let Some(ci) = drop_cond {
        rule.conditions.remove(ci);
        changed = true;
    }
    ui.horizontal(|ui| {
        ui.add_space(24.0);
        if ui.small_button(t!("watch.rule.add_condition")).clicked() {
            rule.conditions.push(Condition {
                column: ColumnId::Process,
                pattern: String::new(),
            });
            changed = true;
        }
        if rule.conditions.is_empty() {
            ui.weak(t!("watch.rule.any_endpoint"));
        }
    });
    ui.horizontal_wrapped(|ui| {
        ui.add_space(24.0);
        ui.label(t!("watch.rule.then"));
        let a = &mut rule.actions;
        changed |= ui
            .checkbox(&mut a.notify, t!("watch.action.notify"))
            .changed();
        changed |= ui
            .checkbox(&mut a.sound, t!("watch.action.sound"))
            .changed();
        changed |= ui
            .checkbox(&mut a.focus, t!("watch.action.focus"))
            .changed();
        changed |= ui.checkbox(&mut a.log, t!("watch.action.log")).changed();
        changed |= ui
            .checkbox(&mut a.record, t!("watch.action.record"))
            .on_hover_text(t!("watch.action.record.tip"))
            .changed();
    });
    changed
}
