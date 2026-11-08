//! Application settings – stored as JSON in the user's configuration folder
//! (Windows: %APPDATA%\MMPortLens\config.json). Every field has a default, so older files keep
//! loading when new settings are added.

use crate::columns::{ColumnId, ColumnLayout, SortSpec};
use crate::t;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// What a zoomable tool window remembers (#42): the position and inner size of the last one
/// (logical pixels) and its own text zoom (1.0 = 100 %).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ToolWindowState {
    pub pos: Option<[f32; 2]>,
    pub size: Option<[f32; 2]>,
    pub zoom: f32,
}

impl Default for ToolWindowState {
    fn default() -> Self {
        Self {
            pos: None,
            size: None,
            zoom: 1.0,
        }
    }
}

/// Refresh intervals offered in the toolbar (ms).
pub const INTERVALS: [u64; 6] = [500, 1000, 2000, 5000, 10_000, 30_000];

#[derive(Clone, Serialize, Deserialize, Debug, PartialEq)]
#[serde(default)]
pub struct Config {
    /// UI language code; empty = same as the operating system.
    pub language: String,
    /// "system", "dark" or "light".
    pub theme: String,
    /// Refresh interval in ms.
    pub interval_ms: u64,
    /// How long new / changed / closed rows stay highlighted (ms).
    pub highlight_ms: u64,
    pub columns: ColumnLayout,
    pub sort: SortSpec,
    pub show_tcp: bool,
    pub show_udp: bool,
    pub show_ipv4: bool,
    pub show_ipv6: bool,
    /// Show TCP listeners (and UDP endpoints, which have no remote side either).
    pub show_listening: bool,
    /// Host names of the addresses are looked up (reverse DNS, #5).
    pub resolve_addresses: bool,
    /// The details pane under the table (#7).
    pub show_details: bool,
    /// Capture the connections shorter than a refresh with ETW (Windows, administrator, #4).
    pub etw_capture: bool,
    /// The filter row under the column headers is shown (#1).
    pub show_column_filters: bool,
    /// The column filters are saved and restored at the next start.
    pub remember_column_filters: bool,
    /// The saved column filters (only while `remember_column_filters` is on).
    pub column_filters: Vec<(ColumnId, String)>,
    /// Always start elevated: a program started without administrator rights starts itself
    /// again through UAC (#15).
    pub run_as_admin: bool,
    /// When to look for a new version at start-up (#15).
    pub update_when: UpdateCheck,
    /// Time of the last update check (Unix seconds).
    pub update_last_check: i64,
    /// Version the user chose to skip ("Skip this version"): no window at start-up for it.
    pub update_skipped: String,
    /// File → Export… settings (#9).
    pub export: crate::tabexport::Settings,
    /// Custom color rules for rows (#10); examples on the first start.
    pub color_rules: Vec<crate::rules::ColorRule>,
    /// The new / changed / closed marks have priority over the color rules.
    pub marks_over_rules: bool,
    /// Rows grouped by process (#12).
    pub group_by_process: bool,
    /// Names of the processes whose group is collapsed (#12).
    pub collapsed_processes: Vec<String>,
    /// Saved filter presets (#13); examples on the first start.
    pub filter_presets: Vec<crate::presets::FilterPreset>,
    /// File → Start recording… settings (#3).
    pub record: crate::recording::Settings,
    /// The windows that have a native variant (process properties #33, Help → Command line #42)
    /// open in their own OS windows; off = inside the main window. Was `properties_separate` of
    /// #33 – an old file keeps its choice through the alias.
    #[serde(alias = "properties_separate")]
    pub separate_windows: bool,
    /// Position and size of the last closed properties window (#33).
    pub properties_window: Option<crate::update::WindowState>,
    /// Zoom and size of the `--help` window (#39).
    pub help_window: crate::helpwindow::HelpWindowState,
    /// Position, size and zoom of the Help → Command line window (#42).
    pub cli_help_window: ToolWindowState,
    /// Position, size and zoom of the View → Statistics window (#19).
    pub stats_window: ToolWindowState,
    /// Version of the filter syntax of the saved filters (#35). A file without it (0) is from
    /// before #35: its filters are rewritten once by `colfilter::migrate_v1`. The field default
    /// is 0 (an old file), `Config::default()` has the current version.
    #[serde(default)]
    pub filter_syntax: u32,
}

/// When MM PortLens looks for a new version at start-up.
#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Debug, Default)]
#[serde(rename_all = "lowercase")]
pub enum UpdateCheck {
    /// At every start.
    #[default]
    Start,
    /// At most once a day.
    Daily,
    Never,
}

impl UpdateCheck {
    pub const ALL: [UpdateCheck; 3] = [UpdateCheck::Start, UpdateCheck::Daily, UpdateCheck::Never];

    /// The automatic check runs now (`last` / `now` in Unix seconds).
    pub fn due(self, last: i64, now: i64) -> bool {
        match self {
            UpdateCheck::Start => true,
            UpdateCheck::Daily => now - last >= 24 * 3600,
            UpdateCheck::Never => false,
        }
    }

    pub fn label(self) -> String {
        match self {
            UpdateCheck::Start => t!("update.when.start"),
            UpdateCheck::Daily => t!("update.when.daily"),
            UpdateCheck::Never => t!("update.when.never"),
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            language: String::new(),
            theme: "system".into(),
            interval_ms: 1000,
            highlight_ms: 3000,
            columns: ColumnLayout::default(),
            sort: SortSpec::default(),
            show_tcp: true,
            show_udp: true,
            show_ipv4: true,
            show_ipv6: true,
            show_listening: true,
            resolve_addresses: true,
            etw_capture: false,
            show_details: true,
            show_column_filters: true,
            remember_column_filters: false,
            column_filters: Vec::new(),
            run_as_admin: false,
            update_when: UpdateCheck::default(),
            update_last_check: 0,
            update_skipped: String::new(),
            export: Default::default(),
            color_rules: crate::rules::default_rules(),
            marks_over_rules: true,
            group_by_process: false,
            collapsed_processes: Vec::new(),
            filter_presets: crate::presets::default_presets(),
            record: Default::default(),
            help_window: Default::default(),
            separate_windows: true,
            cli_help_window: Default::default(),
            stats_window: Default::default(),
            properties_window: None,
            filter_syntax: crate::colfilter::SYNTAX_VERSION,
        }
    }
}

impl Config {
    /// `%APPDATA%\MMPortLens\config.json`; the environment variable `MMPORTLENS_CONFIG` points
    /// to another file (checks in the running program without touching the user's settings).
    pub fn path() -> Option<PathBuf> {
        if let Some(p) = std::env::var_os("MMPORTLENS_CONFIG").filter(|p| !p.is_empty()) {
            return Some(PathBuf::from(p));
        }
        dirs::config_dir().map(|d| d.join("MMPortLens").join("config.json"))
    }

    pub fn load() -> Self {
        Self::path()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|s| Self::parse(&s))
            .unwrap_or_default()
    }

    pub fn parse(text: &str) -> Option<Self> {
        let mut cfg: Config = serde_json::from_str(text).ok()?;
        cfg.columns.normalize();
        cfg.interval_ms = cfg.interval_ms.clamp(100, 600_000);
        cfg.color_rules
            .iter_mut()
            .for_each(crate::rules::ColorRule::fill_missing_dark);
        cfg.migrate_filters();
        Some(cfg)
    }

    /// Rewrites the column filters, filter presets and color rule patterns of a file written
    /// before #35 (`!a|b` = neither a nor b → `!a&!b`); once, then the version is current.
    fn migrate_filters(&mut self) {
        use crate::colfilter::{SYNTAX_VERSION, migrate_v1};
        if self.filter_syntax >= SYNTAX_VERSION {
            return;
        }
        let texts = self.column_filters.iter_mut().map(|(_, t)| t).chain(
            self.filter_presets
                .iter_mut()
                .flat_map(|p| p.column_filters.iter_mut().map(|(_, t)| t)),
        );
        for t in texts {
            *t = migrate_v1(t);
        }
        for r in &mut self.color_rules {
            r.pattern = migrate_v1(&r.pattern);
        }
        self.filter_syntax = SYNTAX_VERSION;
    }

    /// Writes the settings through a temporary file, so a crash never leaves half a file.
    pub fn save(&self) -> std::io::Result<()> {
        let Some(p) = Self::path() else { return Ok(()) };
        if let Some(dir) = p.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let json = serde_json::to_string_pretty(self).map_err(std::io::Error::other)?;
        let tmp = p.with_extension(format!("json.{}.tmp", std::process::id()));
        std::fs::write(&tmp, &json)?;
        if std::fs::rename(&tmp, &p).is_err() {
            let _ = std::fs::remove_file(&tmp);
            std::fs::write(&p, json)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_and_defaults() {
        let cfg = Config {
            interval_ms: 2000,
            show_udp: false,
            ..Default::default()
        };
        let back = Config::parse(&serde_json::to_string(&cfg).unwrap()).unwrap();
        assert_eq!(back, cfg);
        // An old / partial file gets defaults for everything else.
        let partial = Config::parse(r#"{"interval_ms": 5}"#).unwrap();
        assert_eq!(partial.interval_ms, 100);
        assert!(partial.show_tcp);
        assert_eq!(partial.columns, ColumnLayout::default());
        assert_eq!(partial.update_when, UpdateCheck::Start);
        assert!(partial.show_column_filters && !partial.remember_column_filters);
        assert!(!partial.run_as_admin);
        assert!(partial.resolve_addresses && partial.show_details);
        let filters =
            Config::parse(r#"{"column_filters": [["remote_port", "443"], ["pid", ">4"]]}"#)
                .unwrap();
        assert_eq!(
            filters.column_filters,
            vec![
                (ColumnId::RemotePort, "443".into()),
                (ColumnId::Pid, ">4".into())
            ]
        );
        // Older settings get the example color rules; an emptied list stays empty (#10).
        assert_eq!(partial.color_rules, crate::rules::default_rules());
        assert!(partial.marks_over_rules);
        assert!(partial.separate_windows && partial.properties_window.is_none());
        assert_eq!(partial.cli_help_window, ToolWindowState::default());
        assert_eq!(partial.stats_window, ToolWindowState::default());
        // The switch of #33 is taken over by the general setting (#42).
        let off = Config::parse(r#"{"properties_separate": false}"#).unwrap();
        assert!(!off.separate_windows);
        let saved = serde_json::to_string(&off).unwrap();
        assert!(
            saved.contains("\"separate_windows\":false") && !saved.contains("properties_separate")
        );
        let zoomed = Config::parse(r#"{"cli_help_window": {"zoom": 1.3}}"#).unwrap();
        assert_eq!(zoomed.cli_help_window.zoom, 1.3);
        assert_eq!(zoomed.cli_help_window.size, None);
        assert_eq!(partial.filter_presets, crate::presets::default_presets());
        assert!(!partial.group_by_process && partial.collapsed_processes.is_empty());
        assert_eq!(partial.record, crate::recording::Settings::default());
        let none = Config::parse(r#"{"color_rules": []}"#).unwrap();
        assert!(none.color_rules.is_empty());
        let daily = Config::parse(r#"{"update_when": "daily"}"#).unwrap();
        assert_eq!(daily.update_when, UpdateCheck::Daily);
    }

    #[test]
    fn old_filters_are_migrated_once() {
        let old = r#"{
            "column_filters": [["process", "!svchost|system"], ["pid", ">4"]],
            "filter_presets": [{"name": "p", "column_filters": [["state", "!listen|=closed"]]}],
            "color_rules": [{"name": "r", "pattern": "!a|b"}]
        }"#;
        let cfg = Config::parse(old).unwrap();
        assert_eq!(cfg.filter_syntax, crate::colfilter::SYNTAX_VERSION);
        assert_eq!(cfg.column_filters[0].1, "!svchost&!system");
        assert_eq!(cfg.column_filters[1].1, ">4");
        assert_eq!(
            cfg.filter_presets[0].column_filters[0].1,
            "!listen&!=closed"
        );
        assert_eq!(cfg.color_rules[0].pattern, "!a&!b");
        // Saved again with the version: a new-syntax "!a|b" stays as it is.
        let mut cfg = cfg;
        cfg.column_filters[0].1 = "!a|b".into();
        let back = Config::parse(&serde_json::to_string(&cfg).unwrap()).unwrap();
        assert_eq!(back.column_filters[0].1, "!a|b");
        // A new configuration has the current version.
        assert_eq!(
            Config::default().filter_syntax,
            crate::colfilter::SYNTAX_VERSION
        );
    }

    #[test]
    fn update_check_due() {
        let day = 24 * 3600;
        assert!(UpdateCheck::Start.due(100, 100));
        assert!(!UpdateCheck::Daily.due(100, 100 + day - 1));
        assert!(UpdateCheck::Daily.due(100, 100 + day));
        assert!(!UpdateCheck::Never.due(0, i64::MAX));
    }
}
