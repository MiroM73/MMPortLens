//! Saved filter presets (#13): the protocol switches, the text filter and the column filters of
//! the table under a name, switched with one click in the toolbar or with `--preset <name>` on
//! the command line.

use crate::columns::ColumnId;
use crate::t;
use serde::{Deserialize, Serialize};

/// Command line switch choosing a preset at start (since #13; parsed by `crate::cli`, #16).
pub const PRESET_ARG: &str = "--preset";

/// One preset; without the name it is also a snapshot of the current filters.
#[derive(Clone, PartialEq, Serialize, Deserialize, Debug)]
#[serde(default)]
pub struct FilterPreset {
    pub name: String,
    pub show_tcp: bool,
    pub show_udp: bool,
    pub show_ipv4: bool,
    pub show_ipv6: bool,
    pub show_listening: bool,
    /// The text filter of the toolbar.
    pub text: String,
    /// The column filter texts (#1), in column order.
    pub column_filters: Vec<(ColumnId, String)>,
}

impl Default for FilterPreset {
    /// Everything shown, nothing filtered.
    fn default() -> Self {
        Self {
            name: String::new(),
            show_tcp: true,
            show_udp: true,
            show_ipv4: true,
            show_ipv6: true,
            show_listening: true,
            text: String::new(),
            column_filters: Vec::new(),
        }
    }
}

impl FilterPreset {
    /// The same filters (the name and blank texts do not count, the order of the column filters
    /// does not either).
    pub fn same_filters(&self, other: &FilterPreset) -> bool {
        let norm = |p: &FilterPreset| {
            let mut f: Vec<(ColumnId, String)> = p
                .column_filters
                .iter()
                .filter(|(_, t)| !t.trim().is_empty())
                .map(|(c, t)| (*c, t.trim().to_owned()))
                .collect();
            f.sort_by_key(|(c, _)| c.key());
            (
                [
                    p.show_tcp,
                    p.show_udp,
                    p.show_ipv4,
                    p.show_ipv6,
                    p.show_listening,
                ],
                p.text.trim().to_owned(),
                f,
            )
        };
        norm(self) == norm(other)
    }

    /// The filters as text (recording meta, the HTML export of the command line): the text
    /// filter, the column filters and the switches that are off.
    pub fn description(&self) -> String {
        let mut parts: Vec<String> = Vec::new();
        if !self.text.trim().is_empty() {
            parts.push(format!("\"{}\"", self.text.trim()));
        }
        for (c, text) in &self.column_filters {
            if !text.trim().is_empty() {
                parts.push(format!("{}: {}", c.title(), text.trim()));
            }
        }
        for (on, name) in [
            (self.show_tcp, "TCP"),
            (self.show_udp, "UDP"),
            (self.show_ipv4, "IPv4"),
            (self.show_ipv6, "IPv6"),
            (self.show_listening, "Listening"),
        ] {
            if !on {
                parts.push(format!("-{name}"));
            }
        }
        parts.join(", ")
    }
}

/// The presets of the first start (names in the current UI language).
pub fn default_presets() -> Vec<FilterPreset> {
    let with = |name: String, filters: &[(ColumnId, &str)]| FilterPreset {
        name,
        column_filters: filters.iter().map(|(c, t)| (*c, (*t).to_owned())).collect(),
        ..Default::default()
    };
    vec![
        with(t!("preset.default.all"), &[]),
        with(
            t!("preset.default.established"),
            &[(ColumnId::State, "=Established")],
        ),
        with(
            t!("preset.default.listening"),
            &[(ColumnId::State, "=Listen")],
        ),
        with(
            t!("preset.default.no_loopback"),
            &[(ColumnId::LocalAddress, "!loopback")],
        ),
        with(t!("preset.default.teams"), &[(ColumnId::Process, "teams")]),
    ]
}

/// The preset of that name (exact first, then ignoring case).
pub fn find<'a>(presets: &'a [FilterPreset], name: &str) -> Option<&'a FilterPreset> {
    let name = name.trim();
    presets
        .iter()
        .find(|p| p.name == name)
        .or_else(|| presets.iter().find(|p| p.name.eq_ignore_ascii_case(name)))
}

/// Saves `preset` under its name: replaces a preset of the same name (keeping its place) or
/// appends it. Returns true when an existing preset was replaced.
pub fn save(presets: &mut Vec<FilterPreset>, preset: FilterPreset) -> bool {
    match presets.iter_mut().find(|p| p.name == preset.name) {
        Some(p) => {
            *p = preset;
            true
        }
        None => {
            presets.push(preset);
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_filters_ignores_name_order_and_blanks() {
        let a = FilterPreset {
            name: "a".into(),
            text: "chrome ".into(),
            column_filters: vec![
                (ColumnId::Pid, ">4".into()),
                (ColumnId::State, "=Listen".into()),
            ],
            ..Default::default()
        };
        let mut b = a.clone();
        b.name = "b".into();
        b.text = "chrome".into();
        b.column_filters.reverse();
        b.column_filters.push((ColumnId::Process, "  ".into()));
        assert!(a.same_filters(&b));
        b.show_udp = false;
        assert!(!a.same_filters(&b));
        let mut c = a.clone();
        c.column_filters[0].1 = ">5".into();
        assert!(!a.same_filters(&c));
    }

    #[test]
    fn defaults_find_and_save() {
        let mut p = default_presets();
        assert!(p.len() >= 4);
        assert!(p[0].same_filters(&FilterPreset::default()));
        let name = p[1].name.clone();
        assert_eq!(find(&p, &name.to_uppercase()).unwrap().name, name);
        assert!(find(&p, "nothing like this").is_none());
        let n = p.len();
        let mut changed = p[1].clone();
        changed.text = "x".into();
        assert!(save(&mut p, changed));
        assert_eq!((p.len(), p[1].text.as_str()), (n, "x"));
        assert!(!save(
            &mut p,
            FilterPreset {
                name: "new".into(),
                ..Default::default()
            }
        ));
        assert_eq!(p.len(), n + 1);
    }

    #[test]
    fn json_round_trip() {
        let p = default_presets();
        let back: Vec<FilterPreset> =
            serde_json::from_str(&serde_json::to_string(&p).unwrap()).unwrap();
        assert_eq!(back, p);
        let partial: FilterPreset = serde_json::from_str(r#"{"name":"x"}"#).unwrap();
        assert!(partial.show_tcp && partial.column_filters.is_empty());
    }
}
