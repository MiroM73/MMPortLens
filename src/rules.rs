//! Custom color rules for rows (#10), adapted from the highlight rules of RusTrace: a rule colors
//! the text and / or the background of every row whose cell matches its pattern. The pattern uses
//! the syntax of the column filters (#1, `crate::colfilter`): `3389`, `>1024`, `10.0.0.0/8`,
//! `private`, `/regex/`, `a|b`, `!not` … A rule checks one column, or every visible column.
//!
//! The rules are tried in their order; the first enabled one that matches colors the row. The
//! new / changed / closed marks have priority over the rules unless `marks_over_rules` is off.
//! Every rule has colors for the light and the dark theme.

use crate::colfilter::{ColumnFilter, FilterError, FilterKind};
use crate::columns::{ColumnId, cell_text};
use crate::model::Row;
use crate::t;
use crate::tabexport::RowLook;
use serde::{Deserialize, Serialize};

#[derive(Clone, PartialEq, Serialize, Deserialize, Debug)]
#[serde(default)]
pub struct ColorRule {
    pub enabled: bool,
    pub name: String,
    /// The column whose value is checked; `None` = any visible column.
    pub column: Option<ColumnId>,
    /// Column filter syntax (#1).
    pub pattern: String,
    pub use_fg: bool,
    pub fg: [u8; 4],
    pub use_bg: bool,
    pub bg: [u8; 4],
    /// Colors used with the dark theme (`fg` / `bg` are for the light theme). Alpha 0 means
    /// "not set" – filled from the light colors after loading (rules written by hand).
    #[serde(default = "unset")]
    pub fg_dark: [u8; 4],
    #[serde(default = "unset")]
    pub bg_dark: [u8; 4],
}

/// Marker for a color that has not been set (fully transparent).
const UNSET: [u8; 4] = [0, 0, 0, 0];

fn unset() -> [u8; 4] {
    UNSET
}

impl Default for ColorRule {
    fn default() -> Self {
        Self {
            enabled: true,
            name: t!("rules.new_name"),
            column: None,
            pattern: String::new(),
            use_fg: false,
            fg: [0, 0, 0, 255],
            use_bg: true,
            bg: [255, 200, 120, 255],
            fg_dark: [255, 255, 255, 255],
            bg_dark: [150, 90, 20, 255],
        }
    }
}

impl ColorRule {
    /// (text, background) colors for the light or dark theme.
    pub fn colors(&self, dark: bool) -> ([u8; 4], [u8; 4]) {
        if dark {
            (self.fg_dark, self.bg_dark)
        } else {
            (self.fg, self.bg)
        }
    }

    pub fn colors_mut(&mut self, dark: bool) -> (&mut [u8; 4], &mut [u8; 4]) {
        if dark {
            (&mut self.fg_dark, &mut self.bg_dark)
        } else {
            (&mut self.fg, &mut self.bg)
        }
    }

    /// Copies the colors of one theme to the other.
    pub fn copy_colors(&mut self, from_dark: bool) {
        let (fg, bg) = self.colors(from_dark);
        let (tf, tb) = self.colors_mut(!from_dark);
        *tf = fg;
        *tb = bg;
    }

    /// Rules without dark colors use the light ones.
    pub fn fill_missing_dark(&mut self) {
        if self.fg_dark == UNSET {
            self.fg_dark = self.fg;
        }
        if self.bg_dark == UNSET {
            self.bg_dark = self.bg;
        }
    }

    /// The colors of the rule as a row look of the given theme.
    pub fn look(&self, dark: bool) -> RowLook {
        let (fg, bg) = self.colors(dark);
        RowLook {
            fg: self.use_fg.then_some(fg),
            bg: self.use_bg.then_some(bg),
        }
    }

    fn kind(&self) -> FilterKind {
        self.column.map_or(FilterKind::Text, |c| c.filter_kind())
    }
}

/// Example rules (names in the current UI language) – the default of the settings.
pub fn default_rules() -> Vec<ColorRule> {
    vec![
        ColorRule {
            name: t!("rules.default.rdp"),
            column: Some(ColumnId::RemotePort),
            pattern: "3389".into(),
            use_fg: true,
            fg: [0, 0, 0, 255],
            bg: [255, 170, 170, 255],
            fg_dark: [255, 235, 235, 255],
            bg_dark: [140, 30, 30, 255],
            ..Default::default()
        },
        ColorRule {
            name: t!("rules.default.powershell"),
            column: Some(ColumnId::Process),
            pattern: "=powershell.exe|=pwsh.exe".into(),
            use_fg: true,
            fg: [0, 0, 0, 255],
            bg: [255, 205, 140, 255],
            fg_dark: [255, 245, 230, 255],
            bg_dark: [150, 85, 15, 255],
            ..Default::default()
        },
        ColorRule {
            name: t!("rules.default.vpn"),
            enabled: false,
            column: Some(ColumnId::RemoteAddress),
            pattern: "10.0.0.0/8".into(),
            use_fg: true,
            fg: [0, 0, 0, 255],
            bg: [175, 205, 255, 255],
            fg_dark: [235, 240, 255, 255],
            bg_dark: [40, 70, 150, 255],
            ..Default::default()
        },
    ]
}

/// The enabled rules compiled; an invalid pattern is kept as an error (the rule colors nothing).
#[derive(Clone, Default)]
pub struct Compiled {
    /// Per rule: `None` = disabled or empty, `Some(Ok)` = usable, `Some(Err)` = invalid pattern.
    rules: Vec<Option<Result<ColumnFilter, FilterError>>>,
}

impl Compiled {
    pub fn new(rules: &[ColorRule]) -> Self {
        let rules = rules
            .iter()
            .map(|r| {
                if !r.enabled {
                    return None;
                }
                ColumnFilter::parse(&r.pattern, r.kind()).transpose()
            })
            .collect();
        Self { rules }
    }

    /// The error of rule `i`, if its pattern is invalid.
    pub fn error(&self, i: usize) -> Option<&FilterError> {
        self.rules.get(i)?.as_ref()?.as_ref().err()
    }

    /// The index of the first rule matching the row; a rule for a column checks that column
    /// (shown or not), a rule for any column checks the visible columns `cols`.
    pub fn first_match(&self, rules: &[ColorRule], row: &Row, cols: &[ColumnId]) -> Option<usize> {
        self.rules.iter().zip(rules).position(|(f, r)| {
            let Some(Ok(f)) = f else { return false };
            match r.column {
                Some(c) => f.matches(&cell_text(row, c)),
                None => cols.iter().any(|&c| f.matches(&cell_text(row, c))),
            }
        })
    }
}

/// The colors of a row: its mark (new / changed / closed) and the matching rule. With
/// `marks_first` a marked row keeps the mark color; otherwise the rule wins where it sets a color.
pub fn row_look(mark: RowLook, rule: Option<RowLook>, marks_first: bool) -> RowLook {
    let Some(rule) = rule else { return mark };
    let marked = mark.bg.is_some() || mark.fg.is_some();
    if marks_first && marked {
        return mark;
    }
    RowLook {
        fg: rule.fg.or(mark.fg),
        bg: rule.bg.or(mark.bg),
    }
}

/// The light colors of a row for an export (Excel / HTML).
pub fn export_look(
    row: &Row,
    rules: &[ColorRule],
    compiled: &Compiled,
    cols: &[ColumnId],
    marks_first: bool,
) -> RowLook {
    let rule = compiled
        .first_match(rules, row, cols)
        .map(|i| rules[i].look(false));
    row_look(RowLook::of_mark(row.mark), rule, marks_first)
}

/// What importing rules did.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct RuleImport {
    pub added: usize,
    pub updated: usize,
    pub skipped: usize,
}

/// Adds imported rules without creating duplicates (from RusTrace #51): a rule with the same name
/// and pattern as an existing one replaces it when anything else differs (the existing rule keeps
/// its On state) and is skipped when it is the same. Other rules are appended in file order.
pub fn import_rules(rules: &mut Vec<ColorRule>, imported: Vec<ColorRule>) -> RuleImport {
    let mut res = RuleImport::default();
    for mut r in imported {
        r.fill_missing_dark();
        match rules
            .iter_mut()
            .find(|e| e.name == r.name && e.pattern == r.pattern)
        {
            Some(e) => {
                r.enabled = e.enabled;
                if *e == r {
                    res.skipped += 1;
                } else {
                    *e = r;
                    res.updated += 1;
                }
            }
            None => {
                rules.push(r);
                res.added += 1;
            }
        }
    }
    res
}

/// Parses an exported rules file (a JSON array of rules).
pub fn parse_rules(text: &str) -> Result<Vec<ColorRule>, String> {
    let mut rules: Vec<ColorRule> = serde_json::from_str(text).map_err(|e| e.to_string())?;
    rules.iter_mut().for_each(ColorRule::fill_missing_dark);
    Ok(rules)
}

/// Moves item `i` up (`d` = -1) or down (1), or deletes it (0) – the rules window and its
/// export choice.
pub fn move_item<T>(v: &mut Vec<T>, i: usize, d: i32) {
    match d {
        -1 if i > 0 => v.swap(i, i - 1),
        1 if i + 1 < v.len() => v.swap(i, i + 1),
        0 if i < v.len() => {
            v.remove(i);
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Mark;
    use crate::tabexport::tests::sample;

    fn rule(column: Option<ColumnId>, pattern: &str) -> ColorRule {
        ColorRule {
            name: pattern.into(),
            column,
            pattern: pattern.into(),
            ..Default::default()
        }
    }

    #[test]
    fn first_matching_rule_wins() {
        let rows = sample(); // chrome.exe 1.2.3.4:443, System UDP 5353, ::1:8080 listener
        let rules = vec![
            rule(Some(ColumnId::RemotePort), "3389"),
            rule(Some(ColumnId::RemotePort), ">400"),
            rule(Some(ColumnId::Process), "chrome"),
            rule(Some(ColumnId::LocalAddress), "loopback"),
        ];
        let c = Compiled::new(&rules);
        let cols = [ColumnId::Process];
        assert_eq!(c.first_match(&rules, &rows[0], &cols), Some(1));
        assert_eq!(c.first_match(&rules, &rows[1], &cols), None);
        assert_eq!(c.first_match(&rules, &rows[2], &cols), Some(3));
    }

    #[test]
    fn any_column_checks_the_visible_columns_only() {
        let rows = sample();
        let rules = vec![rule(None, "chrome")];
        let c = Compiled::new(&rules);
        assert_eq!(
            c.first_match(&rules, &rows[0], &[ColumnId::Pid, ColumnId::Process]),
            Some(0)
        );
        assert_eq!(c.first_match(&rules, &rows[0], &[ColumnId::Pid]), None);
    }

    #[test]
    fn disabled_empty_and_invalid_rules_color_nothing() {
        let rows = sample();
        let mut off = rule(Some(ColumnId::Process), "chrome");
        off.enabled = false;
        let rules = vec![
            off,
            rule(Some(ColumnId::Process), ""),
            rule(Some(ColumnId::Process), "/chr(/"),
            rule(Some(ColumnId::RemoteAddress), "1.2.3.0/40"),
        ];
        let c = Compiled::new(&rules);
        assert_eq!(c.first_match(&rules, &rows[0], &[]), None);
        assert!(c.error(0).is_none() && c.error(1).is_none());
        assert!(matches!(c.error(2), Some(FilterError::Regex(_))));
        assert!(matches!(c.error(3), Some(FilterError::Network(_))));
    }

    #[test]
    fn marks_over_rules() {
        let mark = RowLook::of_mark(Mark::New);
        let rule = RowLook {
            fg: Some([1, 2, 3, 255]),
            bg: Some([9, 9, 9, 255]),
        };
        assert_eq!(row_look(mark, Some(rule), true), mark);
        assert_eq!(row_look(mark, Some(rule), false), rule);
        // An unmarked row takes the rule either way; a rule without a background keeps the mark's.
        let none = RowLook::default();
        assert_eq!(row_look(none, Some(rule), true), rule);
        let fg_only = RowLook {
            fg: rule.fg,
            bg: None,
        };
        assert_eq!(
            row_look(mark, Some(fg_only), false),
            RowLook {
                fg: rule.fg,
                bg: mark.bg
            }
        );
        assert_eq!(row_look(mark, None, false), mark);
    }

    #[test]
    fn export_look_uses_the_light_colors() {
        let rows = sample();
        let rules = vec![rule(Some(ColumnId::Process), "system")];
        let c = Compiled::new(&rules);
        let look = export_look(&rows[1], &rules, &c, &[], true);
        assert_eq!(look.bg, Some(rules[0].bg));
        assert_eq!(look.fg, None);
    }

    #[test]
    fn default_rules_are_valid() {
        let rules = default_rules();
        let c = Compiled::new(&rules);
        assert!((0..rules.len()).all(|i| c.error(i).is_none()));
        assert!(rules.iter().all(|r| r.column.is_some()));
    }

    #[test]
    fn import_without_duplicates_and_old_files() {
        let mut rules = default_rules();
        let n = rules.len();
        let mut changed = rules[0].clone();
        changed.bg = [1, 1, 1, 255];
        changed.enabled = false;
        let same = rules[1].clone();
        let res = import_rules(&mut rules, vec![same, changed, rule(None, "x")]);
        assert_eq!(
            res,
            RuleImport {
                added: 1,
                updated: 1,
                skipped: 1
            }
        );
        assert_eq!(rules.len(), n + 1);
        assert_eq!(rules[0].bg, [1, 1, 1, 255]);
        assert!(rules[0].enabled, "keeps its On state");
        // A file without dark colors takes the light ones.
        let back = parse_rules(r#"[{"name":"a","pattern":"1","bg":[5,6,7,255]}]"#).unwrap();
        assert_eq!(back[0].bg_dark, [5, 6, 7, 255]);
        assert!(back[0].column.is_none());
        let json = serde_json::to_string(&rules).unwrap();
        assert_eq!(parse_rules(&json).unwrap(), rules);
        assert!(parse_rules("{").is_err());
    }

    #[test]
    fn moving_and_deleting() {
        let mut v = vec![1, 2, 3];
        move_item(&mut v, 1, -1);
        assert_eq!(v, [2, 1, 3]);
        move_item(&mut v, 2, 1);
        assert_eq!(v, [2, 1, 3]);
        move_item(&mut v, 0, 0);
        assert_eq!(v, [1, 3]);
    }
}
