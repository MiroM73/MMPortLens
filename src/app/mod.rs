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
use crate::update::{self, Release, SessionState, Updates, WindowState};
use eframe::egui::{self, Color32, RichText, Sense};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::{Arc, mpsc};
use std::time::{Duration, Instant};

mod actions;
mod admin;
mod colfilters;
mod details;
mod help;
mod input;
mod menu;
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
        rx: mpsc::Receiver<Result<Updates, String>>,
        silent: bool,
    },
    Failed(String),
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
    view_dirty: bool,
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
    /// Help → Keyboard shortcuts.
    show_shortcuts: bool,
    /// Close connection / End process waiting for confirmation (#6).
    pending_action: Option<actions::Pending>,
    /// Result of the last action (status bar).
    action_result: Option<actions::ActionResult>,
    /// Textures of the process icons by executable path (#7).
    icons: HashMap<String, egui::TextureHandle>,
    last_changes: Changes,
    last_took: Duration,
    last_poll: Option<u64>,
    error: Option<String>,
    show_about: bool,
    /// The column window opened by a right click on a header.
    show_columns_popup: bool,
    ctx: egui::Context,
    update: UpdateState,
    show_update: bool,
    /// A newer version found by a check (button in the status bar).
    update_available: Option<Updates>,
    /// Runs with an administrator token (checked once at start).
    elevated: bool,
    /// The last elevated start failed or was cancelled (shown in the status bar).
    elevate_error: Option<String>,
}

impl App {
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        restore: Option<SessionState>,
        elevate_error: Option<ElevateError>,
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
            view_dirty: true,
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
            show_shortcuts: false,
            pending_action: None,
            action_result: None,
            icons: HashMap::new(),
            last_changes: Changes::default(),
            last_took: Duration::ZERO,
            last_poll: None,
            error: None,
            show_about: false,
            show_columns_popup: false,
            ctx: cc.egui_ctx.clone(),
            update: UpdateState::Idle,
            show_update: false,
            update_available: None,
            elevated: elevation::is_elevated(),
            elevate_error: elevate_error.as_ref().map(admin::elevate_error_text),
        };
        app.apply_theme(&cc.egui_ctx);
        if app.cfg.remember_column_filters {
            let saved = app.cfg.column_filters.clone();
            app.set_column_filters(saved);
        }
        if let Some(state) = restore {
            app.restore_session(state);
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
                    self.last_changes =
                        self.tracker
                            .update(snap.rows, snap.taken, self.cfg.highlight_ms);
                    self.last_took = snap.took;
                    self.last_poll = Some(snap.taken);
                    self.error = None;
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
        if self.view_dirty {
            // Rows removed from the tracker cannot stay selected.
            let tracker = &self.tracker;
            self.sel.retain(|k| tracker.contains(k));
        }
    }

    /// Does a row pass the protocol switches, the text filter and the column filters?
    fn row_shown(&self, r: &Row, cols: &[ColumnId], needle: &str) -> bool {
        let p = r.conn.proto;
        let c = &self.cfg;
        if (p.is_tcp() && !c.show_tcp) || (!p.is_tcp() && !c.show_udp) {
            return false;
        }
        if (p.is_v6() && !c.show_ipv6) || (!p.is_v6() && !c.show_ipv4) {
            return false;
        }
        if !c.show_listening && r.conn.remote.is_none() {
            return false;
        }
        let text_ok = needle.is_empty()
            || cols
                .iter()
                .any(|&col| cell_text(r, col).to_lowercase().contains(needle));
        text_ok && self.column_filters_match(r, cols)
    }

    fn rebuild_view(&mut self) {
        use crate::net::TcpState;
        let cols = self.cfg.columns.visible();
        let needle = self.filter.trim().to_lowercase();
        let mut counts = Counts::default();
        let mut view = Vec::with_capacity(self.tracker.len());
        // Host names only while a host column is shown (the filters search the shown columns).
        let resolve = self.cfg.resolve_addresses
            && cols
                .iter()
                .any(|c| matches!(c, ColumnId::RemoteHost | ColumnId::LocalHost));
        let local_names = resolve && cols.contains(&ColumnId::LocalHost);
        let now = now_ms();
        let rows: Vec<Row> = self.tracker.rows().cloned().collect();
        for mut r in rows {
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
                _ => {}
            }
            if resolve {
                r.remote_host = r.conn.remote.and_then(|a| self.resolver.name(a.ip(), now));
                if local_names {
                    r.local_host = self.resolver.name(r.conn.local.ip(), now);
                }
            }
            if self.row_shown(&r, &cols, &needle) {
                view.push(r);
            }
        }
        let sort = self.cfg.sort;
        view.sort_by(|a, b| sort.compare(a, b));
        counts.shown = view.len();
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
        egui::Panel::bottom("status").show(ui, |ui| self.status_bar(ui));
        self.details_pane(ui);
        egui::CentralPanel::default().show(ui, |ui| self.table(ui));
        self.about_window(&ctx);
        self.shortcuts_window(&ctx);
        self.confirm_window(&ctx);
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
        }
    }
}
