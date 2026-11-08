//! The user interface: `App` holds the state and runs the frame loop; every other file in this
//! folder is an `impl App` block for one area (menu, toolbar, table, status bar).
//!
//! Conventions (same as RusTrace): each submodule starts with `use crate::app::*;`, methods used
//! outside their own file are `pub(in crate::app) fn`.

use crate::colfilter::{ColumnFilter, FilterError};
use crate::columns::{ColumnId, cell_text};
use crate::config::{Config, UpdateCheck};
use crate::dns::Resolver;
use crate::elevation::{self, ElevateError};
use crate::i18n;
use crate::model::{Changes, Mark, Row, Tracker, now_ms};
use crate::net::{ConnKey, TcpState};
use crate::poller::{self, PollControl, PollResult};
use crate::selection::Selection;
use crate::t;
use crate::update::{self, CheckError, Release, SessionState, Updates, WindowState};
use eframe::egui::{self, Color32, RichText, Sense};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::{Arc, mpsc};
use std::time::{Duration, Instant};

mod actions;
mod admin;
mod colfilters;
mod columnlayout;
mod details;
mod export;
mod groups;
mod help;
mod input;
mod menu;
mod presets;
mod properties;
mod properties_graph;
mod record;
mod replay;
mod replay_events;
mod rules;
mod statusbar;
mod table;
mod toolbar;
mod update_window;

/// Program version from Cargo.toml (window title, About window).
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
/// Project page on GitHub (Cargo.toml `repository`).
const REPOSITORY: &str = env!("CARGO_PKG_REPOSITORY");

/// Window title; an elevated program says so (#15).
pub fn base_title() -> String {
    if elevation::is_elevated() {
        format!("MM PortLens {VERSION} ({})", t!("title.admin"))
    } else {
        format!("MM PortLens {VERSION}")
    }
}

/// Update check and installation (#15).
enum UpdateState {
    Idle,
    /// Asking GitHub; `silent` = automatic check at start-up (no window unless a new version).
    Checking {
        rx: mpsc::Receiver<Result<Updates, CheckError>>,
        silent: bool,
    },
    Failed(CheckError),
    /// The latest release (newer or not) and the missed ones.
    Checked(Updates),
    /// Download + installation; `exe` = the program path before it was renamed.
    Installing {
        rx: mpsc::Receiver<Result<Vec<String>, String>>,
        progress: Arc<update::Progress>,
        release: Release,
        exe: PathBuf,
    },
    InstallFailed {
        error: String,
        release: Release,
    },
}

/// Counters for the status bar, computed when the view is rebuilt.
#[derive(Default)]
struct Counts {
    total: usize,
    shown: usize,
    tcp: usize,
    udp: usize,
    established: usize,
    listening: usize,
    time_wait: usize,
    /// TCP rows with a state Windows should never report (0 or above 12, #21).
    invalid: usize,
}

pub struct App {
    cfg: Config,
    cfg_dirty: bool,
    last_save: Instant,
    ctl: Arc<PollControl>,
    rx: mpsc::Receiver<PollResult>,
    tracker: Tracker,
    /// The rows shown: filtered and sorted copy of the tracker, rebuilt when `view_dirty`.
    view: Vec<Row>,
    /// Group by process (#12): the groups, the table lines (group lines and rows of `view`)
    /// and the line of each row of `view`; without grouping one line per row.
    groups: Vec<crate::grouping::Group>,
    lines: Vec<crate::grouping::Line>,
    line_of_row: Vec<usize>,
    /// A row of each group (icon and tooltip of the group line).
    group_samples: Vec<Row>,
    /// The color rule of each row of `view` (#10), matched when the view is rebuilt.
    view_rule: Vec<Option<usize>>,
    view_dirty: bool,
    /// The color rules compiled (#10).
    rules: crate::rules::Compiled,
    /// View → Color rules… window (#10): open, theme of the edited colors, the rules ticked
    /// for an export, the last import / export message.
    show_rules: bool,
    rules_edit_dark: bool,
    rules_export_pick: Option<Vec<bool>>,
    rules_msg: Option<String>,
    counts: Counts,
    /// Text filter of the toolbar (any visible column contains it, case-insensitive).
    filter: String,
    /// Texts of the column filter fields (#1), without empty ones.
    col_filter_text: HashMap<ColumnId, String>,
    /// The valid column filters, compiled.
    col_filters: Vec<(ColumnId, ColumnFilter)>,
    /// Column filter texts that are invalid (shown in red, not applied).
    col_filter_errors: HashMap<ColumnId, FilterError>,
    /// Selected rows and the cursor row, by key (#17).
    sel: Selection,
    /// Reverse DNS cache and its worker threads (#5).
    resolver: Resolver,
    /// Table position to scroll to in the next frame (keyboard navigation).
    scroll_to: Option<usize>,
    /// Horizontal scroll offset of the table (#27), runtime only.
    table_scroll_x: f32,
    /// Natural width of the table columns, measured in the last frame (#27).
    table_width: f32,
    /// The visible columns the table widths were last applied for (#2).
    table_cols: Vec<ColumnId>,
    /// Apply the widths of the settings to the table in the next frame (fit, reset; #2).
    apply_widths: bool,
    /// The header being dragged to a new place (#2).
    header_drag: Option<columnlayout::ColumnDrag>,
    /// The row of the column window being dragged (#2).
    columns_drag: Option<columnlayout::ColumnDrag>,
    /// Help → Keyboard shortcuts.
    show_shortcuts: bool,
    /// Close connection / End process waiting for confirmation (#6).
    pending_action: Option<actions::Pending>,
    /// The name of the filter preset chosen last (#13; runtime only, empty = none).
    active_preset: String,
    /// "Save current as…" / "Manage presets…" (#13).
    preset_windows: presets::PresetWindows,
    /// The process properties window (#28).
    properties: Option<properties::Properties>,
    /// Result of the last action (status bar).
    action_result: Option<actions::ActionResult>,
    /// The traffic counters were refused by Windows (not elevated, #8).
    stats_denied: bool,
    /// Textures of the process icons by executable path (#7).
    icons: HashMap<String, egui::TextureHandle>,
    last_changes: Changes,
    last_took: Duration,
    last_poll: Option<u64>,
    error: Option<String>,
    show_about: bool,
    /// The column window opened by a right click on a header.
    show_columns_popup: bool,
    /// File → Export… (#9): the window, the running export, its result (text, error, until),
    /// the last exported file.
    show_export: bool,
    export_job: Option<export::ExportJob>,
    export_result: Option<(String, bool, Instant)>,
    export_last: Option<PathBuf>,
    ctx: egui::Context,
    update: UpdateState,
    show_update: bool,
    /// A newer version found by a check (button in the status bar).
    update_available: Option<Updates>,
    /// Runs with an administrator token (checked once at start).
    elevated: bool,
    /// The last elevated start failed or was cancelled (shown in the status bar).
    elevate_error: Option<String>,
    /// A running recording (#3), the File → Start recording… window, the result of the last
    /// recording (status bar).
    recording: Option<record::Recording>,
    show_record: bool,
    record_result: Option<record::RecordResult>,
    /// An opened recording (#3): the table shows its state at the slider time instead of the
    /// live endpoints.
    replay: Option<replay::Replay>,
}

/// What decides whether a row is shown: the protocol switches, the text filter and the column
/// filters of the visible columns. A copy is kept by a recording "only rows matching the
/// current filters" (#3).
#[derive(Clone)]
pub(in crate::app) struct RowFilter {
    show_tcp: bool,
    show_udp: bool,
    show_ipv4: bool,
    show_ipv6: bool,
    show_listening: bool,
    /// The text filter, lower case.
    needle: String,
    cols: Vec<ColumnId>,
    col_filters: Vec<(ColumnId, ColumnFilter)>,
}

impl RowFilter {
    pub(in crate::app) fn matches(&self, r: &Row) -> bool {
        let p = r.conn.proto;
        if (p.is_tcp() && !self.show_tcp) || (!p.is_tcp() && !self.show_udp) {
            return false;
        }
        if (p.is_v6() && !self.show_ipv6) || (!p.is_v6() && !self.show_ipv4) {
            return false;
        }
        if !self.show_listening && r.conn.remote.is_none() {
            return false;
        }
        let text_ok = self.needle.is_empty()
            || self
                .cols
                .iter()
                .any(|&col| cell_text(r, col).to_lowercase().contains(&self.needle));
        text_ok
            && self
                .col_filters
                .iter()
                .filter(|(id, _)| self.cols.contains(id))
                .all(|(id, f)| f.matches(&cell_text(r, *id)))
    }
}

impl App {
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        restore: Option<SessionState>,
        elevate_error: Option<ElevateError>,
        preset: Option<String>,
        recording: Option<PathBuf>,
    ) -> Self {
        let cfg = Config::load();
        i18n::set_language(&cfg.language);
        let ctx = cc.egui_ctx.clone();
        let (ctl, rx) = poller::start(cfg.interval_ms, move || ctx.request_repaint());
        let mut app = Self {
            cfg,
            cfg_dirty: false,
            last_save: Instant::now(),
            ctl,
            rx,
            tracker: Tracker::default(),
            view: Vec::new(),
            view_rule: Vec::new(),
            groups: Vec::new(),
            lines: Vec::new(),
            line_of_row: Vec::new(),
            group_samples: Vec::new(),
            view_dirty: true,
            rules: crate::rules::Compiled::default(),
            show_rules: false,
            rules_edit_dark: cc.egui_ctx.global_style().visuals.dark_mode,
            rules_export_pick: None,
            rules_msg: None,
            counts: Counts::default(),
            filter: String::new(),
            col_filter_text: HashMap::new(),
            col_filters: Vec::new(),
            col_filter_errors: HashMap::new(),
            sel: Selection::default(),
            resolver: {
                let ctx = cc.egui_ctx.clone();
                Resolver::new(
                    Arc::new(crate::dns::system_lookup),
                    Arc::new(move || ctx.request_repaint()),
                )
            },
            scroll_to: None,
            table_scroll_x: 0.0,
            table_width: 0.0,
            table_cols: Vec::new(),
            apply_widths: false,
            header_drag: None,
            columns_drag: None,
            show_shortcuts: false,
            pending_action: None,
            action_result: None,
            properties: None,
            active_preset: String::new(),
            preset_windows: Default::default(),
            icons: HashMap::new(),
            stats_denied: false,
            last_changes: Changes::default(),
            last_took: Duration::ZERO,
            last_poll: None,
            error: None,
            show_about: false,
            show_columns_popup: false,
            show_export: false,
            export_job: None,
            export_result: None,
            export_last: None,
            ctx: cc.egui_ctx.clone(),
            update: UpdateState::Idle,
            show_update: false,
            update_available: None,
            elevated: elevation::is_elevated(),
            elevate_error: elevate_error.as_ref().map(admin::elevate_error_text),
            recording: None,
            show_record: false,
            record_result: None,
            replay: None,
        };
        app.apply_theme(&cc.egui_ctx);
        app.recompile_rules();
        if app.cfg.remember_column_filters {
            let saved = app.cfg.column_filters.clone();
            app.set_column_filters(saved);
        }
        if let Some(state) = restore {
            app.restore_session(state);
        } else if let Some(name) = preset {
            app.apply_preset_by_name(&name);
        }
        if let Some(path) = recording {
            app.open_recording(path);
        }
        let now = chrono::Local::now().timestamp();
        if app.cfg.update_when.due(app.cfg.update_last_check, now) {
            app.start_update_check(true);
        }
        app
    }

    fn apply_theme(&self, ctx: &egui::Context) {
        let pref = match self.cfg.theme.as_str() {
            "dark" => egui::ThemePreference::Dark,
            "light" => egui::ThemePreference::Light,
            _ => egui::ThemePreference::System,
        };
        ctx.set_theme(pref);
    }

    /// Marks the settings for saving (debounced in the frame loop).
    fn settings_changed(&mut self) {
        self.cfg_dirty = true;
    }

    fn paused(&self) -> bool {
        self.ctl.paused.load(Ordering::Relaxed)
    }

    /// Takes the snapshots the poll thread delivered.
    fn receive(&mut self) {
        while let Ok(res) = self.rx.try_recv() {
            match res {
                Ok(snap) => {
                    let mut events = self.recording.as_ref().map(|_| Vec::new());
                    self.last_changes = self.tracker.update_with_events(
                        snap.rows,
                        snap.taken,
                        self.cfg.highlight_ms,
                        events.as_mut(),
                    );
                    if let Some(events) = events {
                        self.record_events(events);
                    }
                    self.last_took = snap.took;
                    self.last_poll = Some(snap.taken);
                    self.stats_denied = snap.stats_denied;
                    self.error = None;
                    self.sample_properties(snap.taken);
                }
                Err(e) => self.error = Some(t!("error.poll", error = e)),
            }
            self.view_dirty = true;
        }
        if self.tracker.expire(now_ms()) {
            self.view_dirty = true;
        }
        if self.resolver.receive(now_ms()) {
            self.view_dirty = true;
        }
        self.receive_replay();
        if self.view_dirty {
            // Rows removed from the tracker (or from the replayed state) cannot stay selected.
            match &self.replay {
                Some(rp) => {
                    let keys: std::collections::HashSet<ConnKey> =
                        rp.rows.iter().map(|r| r.conn.key()).collect();
                    self.sel.retain(|k| keys.contains(k));
                }
                None => {
                    let tracker = &self.tracker;
                    self.sel.retain(|k| tracker.contains(k));
                }
            }
        }
    }

    /// The filters of the table right now.
    pub(in crate::app) fn row_filter(&self) -> RowFilter {
        let c = &self.cfg;
        RowFilter {
            show_tcp: c.show_tcp,
            show_udp: c.show_udp,
            show_ipv4: c.show_ipv4,
            show_ipv6: c.show_ipv6,
            show_listening: c.show_listening,
            needle: self.filter.trim().to_lowercase(),
            cols: c.columns.visible(),
            col_filters: self.col_filters.clone(),
        }
    }

    /// The rows the table is built from: the live endpoints, or the state of an opened
    /// recording at the slider time (#3).
    pub(in crate::app) fn source_rows(&self) -> Vec<Row> {
        match &self.replay {
            Some(rp) => rp.rows.clone(),
            None => self.tracker.rows().cloned().collect(),
        }
    }

    /// Host names of a row from the reverse DNS cache (the local one only while its column is
    /// shown); asks for the missing ones.
    fn fill_host_names(&mut self, r: &mut Row, cols: &[ColumnId], now: u64) {
        if !self.cfg.resolve_addresses {
            return;
        }
        if cols.contains(&ColumnId::RemoteHost) || cols.contains(&ColumnId::LocalHost) {
            r.remote_host = r.conn.remote.and_then(|a| self.resolver.name(a.ip(), now));
        }
        if cols.contains(&ColumnId::LocalHost) {
            r.local_host = self.resolver.name(r.conn.local.ip(), now);
        }
    }

    fn rebuild_view(&mut self) {
        use crate::net::TcpState;
        let cols = self.cfg.columns.visible();
        // Traffic counters only while a traffic column is shown (they cost CPU, #8) or the
        // properties window draws its graph (#30).
        self.ctl.collect_stats.store(
            cols.iter().any(|c| c.traffic()) || self.properties.is_some(),
            Ordering::Relaxed,
        );
        let filter = self.row_filter();
        let mut counts = Counts::default();
        let mut view = Vec::with_capacity(self.tracker.len());
        // Host names only while a host column is shown (the filters search the shown columns).
        let resolve = self.cfg.resolve_addresses
            && cols
                .iter()
                .any(|c| matches!(c, ColumnId::RemoteHost | ColumnId::LocalHost));
        let now = now_ms();
        for mut r in self.source_rows() {
            counts.total += 1;
            if r.conn.proto.is_tcp() {
                counts.tcp += 1;
            } else {
                counts.udp += 1;
            }
            match r.conn.state {
                Some(TcpState::Established) => counts.established += 1,
                Some(TcpState::Listen) => counts.listening += 1,
                Some(TcpState::TimeWait) => counts.time_wait += 1,
                Some(TcpState::Unknown(_)) => counts.invalid += 1,
                _ => {}
            }
            if resolve {
                self.fill_host_names(&mut r, &cols, now);
            }
            if filter.matches(&r) {
                view.push(r);
            }
        }
        let sort = self.cfg.sort;
        view.sort_by(|a, b| sort.compare(a, b));
        counts.shown = view.len();
        let grouped = if self.cfg.group_by_process {
            let collapsed: std::collections::HashSet<String> =
                self.cfg.collapsed_processes.iter().cloned().collect();
            crate::grouping::group(view, &collapsed)
        } else {
            crate::grouping::ungrouped(view)
        };
        let view = grouped.rows;
        self.groups = grouped.groups;
        self.lines = grouped.lines;
        self.line_of_row = grouped.line_of_row;
        self.group_samples = grouped.samples;
        self.view_rule = view
            .iter()
            .map(|r| self.rules.first_match(&self.cfg.color_rules, r, &cols))
            .collect();
        self.view = view;
        self.counts = counts;
        self.view_dirty = false;
    }

    fn about_window(&mut self, ctx: &egui::Context) {
        let mut open = self.show_about;
        egui::Window::new(t!("about.title"))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .show(ctx, |ui| {
                ui.heading(base_title());
                ui.label(t!("about.text"));
                ui.hyperlink(REPOSITORY);
            });
        self.show_about = open;
    }
}

/// Minimum width of the last table column; it fills the rest of the window when the columns fit.
const LAST_COLUMN_MIN: f32 = 150.0;

/// An RGBA color of the settings as an egui color.
fn c32(c: [u8; 4]) -> Color32 {
    Color32::from_rgba_unmultiplied(c[0], c[1], c[2], c[3])
}

/// Background of a marked row, readable in both themes.
fn mark_color(mark: Mark, dark: bool) -> Option<Color32> {
    let (light, dark_c) = match mark {
        Mark::None => return None,
        Mark::New => ([190, 240, 190], [30, 90, 40]),
        Mark::Changed => ([250, 235, 160], [105, 90, 20]),
        Mark::Closed => ([250, 190, 190], [110, 35, 35]),
    };
    let c = if dark { dark_c } else { light };
    Some(Color32::from_rgb(c[0], c[1], c[2]))
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.receive();
        if self.view_dirty {
            self.rebuild_view();
        }
        self.handle_input(&ctx);
        if self.view_dirty {
            self.rebuild_view();
        }
        self.load_icon_textures(&ctx);

        egui::Panel::top("menu").show(ui, |ui| self.menu_bar(ui));
        egui::Panel::top("toolbar").show(ui, |ui| self.toolbar(ui));
        self.replay_bar(ui);
        egui::Panel::bottom("status").show(ui, |ui| self.status_bar(ui));
        self.replay_events_panel(ui);
        self.details_pane(ui);
        egui::CentralPanel::default().show(ui, |ui| self.table(ui));
        self.about_window(&ctx);
        self.shortcuts_window(&ctx);
        self.properties_window(&ctx);
        self.confirm_window(&ctx);
        self.rules_window(&ctx);
        self.preset_windows(&ctx);
        self.record_window(&ctx);
        self.check_recording(&ctx);
        self.check_export();
        self.export_window(&ctx);
        self.poll_update(&ctx);
        self.update_window(&ctx);

        // Repaint when the next highlight ends (new / closed rows lose their color).
        if let Some(t) = self.tracker.next_expiry() {
            let wait = t.saturating_sub(now_ms()).max(50);
            ctx.request_repaint_after(Duration::from_millis(wait));
        }

        // Save settings (debounced, so we do not write on every click).
        let closing = ctx.input(|i| i.viewport().close_requested());
        if self.cfg_dirty && (closing || self.last_save.elapsed() > Duration::from_millis(800)) {
            if let Err(e) = self.cfg.save() {
                self.error = Some(t!("error.save_settings", error = e));
            }
            self.cfg_dirty = false;
            self.last_save = Instant::now();
        } else if self.cfg_dirty {
            ctx.request_repaint_after(Duration::from_millis(900));
        }
        if closing {
            self.ctl.stop.store(true, Ordering::Relaxed);
            // The recording is finished into a complete file before the program ends.
            if let Some(rec) = self.recording.take() {
                let _ = rec.recorder.wait();
            }
        }
    }
}
