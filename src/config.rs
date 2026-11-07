//! Application settings – stored as JSON in the user's configuration folder
//! (Windows: %APPDATA%\RusNet\config.json). Every field has a default, so older files keep
//! loading when new settings are added.

use crate::columns::{ColumnLayout, SortSpec};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

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
        }
    }
}

impl Config {
    pub fn path() -> Option<PathBuf> {
        dirs::config_dir().map(|d| d.join("RusNet").join("config.json"))
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
        Some(cfg)
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
    }
}
