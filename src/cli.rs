//! Command-line arguments (#16): `mmportlens [OPTIONS] [RECORDING.plrec]`.
//!
//! [`parse`] is a small hand-written parser (no dependency, the style of RusTrace `src/cli.rs`)
//! that only turns the arguments into [`Options`]; [`check_lang`] and [`filters`] check the values
//! that need the language files or the settings (language codes, column names, preset names,
//! filter syntax). Messages go through `t!()`; the help and the errors of a start with a window
//! are shown with [`show_message`] (a message box on Windows, where the release build has no
//! console, and stdout / stderr), the runs without a window (`--export`, `--record`) write to the
//! console they were started from ([`attach_console`]).
//!
//! Compatibility: a plain `mmportlens.exe "C:\path\capture.plrec"` (a double-clicked recording)
//! is the RECORDING; `--preset <name>` keeps the form and the behaviour it had since #13; the
//! internal `--restore <file>` of the update restart (#15) and `--elevated` of the elevated
//! restart keep their old form, so an older installed version that installs this one still
//! restarts it correctly.

use std::ffi::OsString;
use std::path::PathBuf;

use crate::columns::ColumnId;
use crate::elevation::ELEVATED_ARG;
use crate::presets::{self, FilterPreset, PRESET_ARG};
use crate::t;
use crate::update::RESTORE_ARG;

/// The smallest refresh interval `--interval` accepts (ms).
pub const MIN_INTERVAL_MS: u64 = 100;

/// What the table shows: filters, refresh. Without a window the filters choose the exported or
/// recorded endpoints.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ViewOptions {
    /// `--preset <name>`: a saved filter preset (#13).
    pub preset: Option<String>,
    /// `--filter <text>`: the text filter of the toolbar.
    pub filter: Option<String>,
    /// `--column <column>=<filter>` (repeatable): column name and filter text as entered.
    pub columns: Vec<(String, String)>,
    /// `--interval <ms>`: refresh interval for this run.
    pub interval: Option<u64>,
    /// `--paused`: the automatic refresh starts paused.
    pub paused: bool,
}

impl ViewOptions {
    /// Any filter option was given.
    pub fn has_filters(&self) -> bool {
        self.preset.is_some() || self.filter.is_some() || !self.columns.is_empty()
    }
}

/// The parsed command line.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Options {
    /// A recording to open for the replay (#3).
    pub recording: Option<PathBuf>,
    pub view: ViewOptions,
    /// UI language for this run only (not saved).
    pub lang: Option<String>,
    pub help: bool,
    pub version: bool,
    /// State file of the previous version after an update (`--restore <file>`).
    pub restore: Option<PathBuf>,
    /// Started again through UAC (`--elevated`, #15).
    pub elevated: bool,
    /// Write one snapshot to this file and exit without a window.
    pub export: Option<PathBuf>,
    /// Record connection events into this file without a window.
    pub record: Option<PathBuf>,
    /// How long `--record` records (ms); `None` = until Ctrl+C.
    pub duration: Option<u64>,
}

impl Options {
    /// A run without a window (`--export`, `--record`).
    pub fn headless(&self) -> bool {
        self.export.is_some() || self.record.is_some()
    }
}

/// An invalid command line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CliError {
    UnknownOption(String),
    MissingValue(String),
    UnexpectedValue(String),
    /// A second FILE, or a FILE that is not a recording.
    UnexpectedArgument(String),
    InvalidLanguage {
        value: String,
        available: String,
    },
    InvalidInterval(String),
    InvalidDuration(String),
    /// `--column` without `=`.
    ColumnSyntax(String),
    UnknownColumn(String),
    ColumnRegex {
        column: String,
        error: String,
    },
    ColumnNetwork {
        column: String,
        value: String,
    },
    UnknownPreset(String),
    /// `option` only makes sense together with `needs`.
    Needs {
        option: String,
        needs: String,
    },
    Conflict {
        option: String,
        other: String,
    },
    ExportFormat(String),
}

/// The column names `--column` accepts: `process`, `local_port`, `remote_address` …
pub fn column_names() -> String {
    ColumnId::ALL
        .iter()
        .map(|c| column_name(*c))
        .collect::<Vec<_>>()
        .join(", ")
}

fn column_name(c: ColumnId) -> &'static str {
    c.key().strip_prefix("column.").unwrap_or(c.key())
}

/// The column of a `--column` name: the machine name (`-` or a space for `_` allowed) or the
/// title in the UI language, ignoring case.
pub fn find_column(name: &str) -> Option<ColumnId> {
    let n = name.trim().to_lowercase();
    let machine = n.replace(['-', ' '], "_");
    ColumnId::ALL
        .into_iter()
        .find(|&c| column_name(c) == machine)
        .or_else(|| {
            ColumnId::ALL
                .into_iter()
                .find(|c| c.title().to_lowercase() == n)
        })
}

impl CliError {
    /// The translated message, with a hint to `--help`.
    pub fn message(&self) -> String {
        let msg = match self {
            CliError::UnknownOption(o) => t!("cli.error.unknown", option = o),
            CliError::MissingValue(o) => t!("cli.error.missing_value", option = o),
            CliError::UnexpectedValue(o) => t!("cli.error.unexpected_value", option = o),
            CliError::UnexpectedArgument(v) => t!("cli.error.argument", value = v),
            CliError::InvalidLanguage { value, available } => {
                t!("cli.error.lang", value = value, languages = available)
            }
            CliError::InvalidInterval(v) => {
                t!("cli.error.interval", value = v, min = MIN_INTERVAL_MS)
            }
            CliError::InvalidDuration(v) => t!("cli.error.duration", value = v),
            CliError::ColumnSyntax(v) => t!("cli.error.column_syntax", value = v),
            CliError::UnknownColumn(v) => {
                t!("cli.error.column", value = v, columns = column_names())
            }
            CliError::ColumnRegex { column, error } => {
                t!("cli.error.column_regex", column = column, error = error)
            }
            CliError::ColumnNetwork { column, value } => {
                t!("cli.error.column_network", column = column, value = value)
            }
            CliError::UnknownPreset(n) => t!("cli.error.preset", name = n),
            CliError::Needs { option, needs } => {
                t!("cli.error.needs", option = option, needs = needs)
            }
            CliError::Conflict { option, other } => {
                t!("cli.error.conflict", option = option, other = other)
            }
            CliError::ExportFormat(f) => t!("cli.error.export_format", file = f),
        };
        format!("{msg}\n\n{}", t!("cli.error.hint"))
    }
}

/// Parses the arguments (without the program name). Options may come before or after the
/// RECORDING, values as `--interval 500` or `--interval=500`, `--` ends the options. A repeated
/// option: the last one counts (`--column` adds one filter each time).
pub fn parse<I: IntoIterator<Item = OsString>>(args: I) -> Result<Options, CliError> {
    let mut o = Options::default();
    let mut args = args.into_iter();
    let mut only_files = false;
    let file = |o: &mut Options, arg: OsString| -> Result<(), CliError> {
        let path = PathBuf::from(&arg);
        let is_recording = path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case(crate::recording::EXTENSION));
        if o.recording.is_some() || !is_recording {
            return Err(CliError::UnexpectedArgument(
                arg.to_string_lossy().into_owned(),
            ));
        }
        o.recording = Some(path);
        Ok(())
    };
    while let Some(arg) = args.next() {
        if only_files {
            file(&mut o, arg)?;
            continue;
        }
        let text = arg.to_string_lossy();
        let is_option = (text.len() > 1 && text.starts_with('-')) || text == "/?";
        if !is_option {
            file(&mut o, arg)?;
            continue;
        }
        if text == "--" {
            only_files = true;
            continue;
        }
        let (name, inline) = match text.split_once('=') {
            Some((n, v)) if n.starts_with("--") => (n.to_owned(), Some(v.to_owned())),
            _ => (text.into_owned(), None),
        };
        let takes_value = matches!(
            name.as_str(),
            "--filter"
                | "--column"
                | "--interval"
                | "--lang"
                | "--export"
                | "--record"
                | "--duration"
        ) || name == PRESET_ARG
            || name == RESTORE_ARG;
        let value = if takes_value {
            match inline {
                Some(v) => Some(v),
                None => Some(
                    args.next()
                        .map(|v| v.to_string_lossy().into_owned())
                        .ok_or_else(|| CliError::MissingValue(name.clone()))?,
                ),
            }
        } else if inline.is_some() {
            return Err(CliError::UnexpectedValue(name));
        } else {
            None
        };
        let value = value.unwrap_or_default();
        match name.as_str() {
            n if n == PRESET_ARG => o.view.preset = Some(value),
            "--filter" => o.view.filter = Some(value),
            "--column" => {
                let (col, filter) = value
                    .split_once('=')
                    .filter(|(c, _)| !c.trim().is_empty())
                    .ok_or_else(|| CliError::ColumnSyntax(value.clone()))?;
                o.view
                    .columns
                    .push((col.trim().to_owned(), filter.to_owned()));
            }
            "--interval" => {
                o.view.interval = Some(
                    value
                        .trim()
                        .parse::<u64>()
                        .ok()
                        .filter(|&ms| ms >= MIN_INTERVAL_MS)
                        .ok_or(CliError::InvalidInterval(value))?,
                );
            }
            "--paused" => o.view.paused = true,
            "--lang" => o.lang = Some(value),
            "--export" => o.export = Some(PathBuf::from(value)),
            "--record" => o.record = Some(PathBuf::from(value)),
            "--duration" => {
                o.duration = Some(
                    crate::recording::parse_duration(&value)
                        .ok_or(CliError::InvalidDuration(value))?,
                );
            }
            "--help" | "-h" | "-?" | "/?" => o.help = true,
            "--version" | "-V" => o.version = true,
            n if n == RESTORE_ARG => o.restore = Some(PathBuf::from(value)),
            n if n == ELEVATED_ARG => o.elevated = true,
            _ => return Err(CliError::UnknownOption(name)),
        }
    }
    check_combinations(&o)?;
    Ok(o)
}

/// Options that do not go together.
fn check_combinations(o: &Options) -> Result<(), CliError> {
    let conflict = |a: &str, b: &str| {
        Err(CliError::Conflict {
            option: a.into(),
            other: b.into(),
        })
    };
    if o.export.is_some() && o.record.is_some() {
        return conflict("--export", "--record");
    }
    if o.duration.is_some() && o.record.is_none() {
        return Err(CliError::Needs {
            option: "--duration".into(),
            needs: "--record".into(),
        });
    }
    for (set, name) in [
        (o.export.is_some(), "--export"),
        (o.record.is_some(), "--record"),
    ] {
        if !set {
            continue;
        }
        if o.recording.is_some() {
            return conflict(name, "RECORDING");
        }
        if o.view.paused {
            return conflict(name, "--paused");
        }
    }
    if o.export.is_some() && o.view.interval.is_some() {
        return conflict("--export", "--interval");
    }
    if let Some(f) = &o.export
        && crate::tabexport::TableFormat::from_path(f).is_none()
    {
        return Err(CliError::ExportFormat(f.display().to_string()));
    }
    Ok(())
}

/// The language code of `--lang` as the language files name it (case-insensitive match).
pub fn check_lang(value: &str, available: &[String]) -> Result<String, CliError> {
    available
        .iter()
        .find(|c| c.eq_ignore_ascii_case(value.trim()))
        .cloned()
        .ok_or_else(|| CliError::InvalidLanguage {
            value: value.to_owned(),
            available: available.join(", "),
        })
}

/// The filters the command line asks for, on top of `base` (the filters the table would start
/// with; an empty preset without a window): `--preset` replaces them all, `--filter` the text,
/// every `--column` the filter of its column. `None` when no filter option was given. Checks the
/// preset and column names and the syntax of the column filters.
pub fn filters(
    view: &ViewOptions,
    saved: &[FilterPreset],
    base: FilterPreset,
) -> Result<Option<FilterPreset>, CliError> {
    if !view.has_filters() {
        return Ok(None);
    }
    let mut p = match &view.preset {
        Some(name) => presets::find(saved, name)
            .cloned()
            .ok_or_else(|| CliError::UnknownPreset(name.clone()))?,
        None => FilterPreset {
            name: String::new(),
            ..base
        },
    };
    if let Some(text) = &view.filter {
        p.text = text.clone();
    }
    for (name, text) in &view.columns {
        let id = find_column(name).ok_or_else(|| CliError::UnknownColumn(name.clone()))?;
        p.column_filters.retain(|(c, _)| *c != id);
        p.column_filters.push((id, text.clone()));
    }
    if let Err((id, e)) = crate::rowfilter::compile(&p.column_filters) {
        let column = id.title();
        return Err(match e {
            crate::colfilter::FilterError::Regex(error) => CliError::ColumnRegex { column, error },
            crate::colfilter::FilterError::Network(value) => {
                CliError::ColumnNetwork { column, value }
            }
        });
    }
    Ok(Some(p))
}

/// The help text (`--help`); `languages` = the codes `--lang` accepts. `tabs`: the option and
/// its description are separated by a tab (message box) instead of padding (console).
pub fn help_text(languages: &str, tabs: bool) -> String {
    let meta = |key: &str| format!("<{}>", t!(key));
    let opts: [(String, String); 12] = [
        (
            format!("--preset {}", meta("cli.meta.name")),
            t!("cli.opt.preset"),
        ),
        (
            format!("--filter {}", meta("cli.meta.text")),
            t!("cli.opt.filter"),
        ),
        (
            format!(
                "--column {}={}",
                meta("cli.meta.column"),
                meta("cli.meta.filter")
            ),
            t!("cli.opt.column"),
        ),
        (
            format!("--interval {}", meta("cli.meta.ms")),
            t!("cli.opt.interval", min = MIN_INTERVAL_MS),
        ),
        ("--paused".into(), t!("cli.opt.paused")),
        (
            format!("--export {}", meta("cli.meta.file")),
            t!("cli.opt.export"),
        ),
        (
            format!("--record {}", meta("cli.meta.file")),
            t!("cli.opt.record"),
        ),
        (
            format!("--duration {}", meta("cli.meta.time")),
            t!("cli.opt.duration"),
        ),
        (
            format!("--lang {}", meta("cli.meta.code")),
            t!("cli.opt.lang", languages = languages),
        ),
        ("--help".into(), t!("cli.opt.help")),
        ("--version".into(), t!("cli.opt.version")),
        ("--".into(), t!("cli.opt.end")),
    ];
    let width = opts
        .iter()
        .map(|(o, _)| o.chars().count())
        .max()
        .unwrap_or(0);
    let mut s = format!("{}\n\n{}\n\n", t!("cli.usage"), t!("cli.file"));
    for (opt, desc) in opts {
        if tabs {
            s.push_str(&format!("{opt}\t{desc}\n"));
        } else {
            s.push_str(&format!("  {opt:width$}  {desc}\n"));
        }
    }
    s.push('\n');
    s.push_str(&t!("cli.columns", columns = column_names()));
    s.push_str("\n\n");
    s.push_str(&t!("cli.exit"));
    s.push_str("\n\n");
    s.push_str(&t!("cli.example"));
    s.push_str("\nmmportlens --preset Listening --paused");
    s.push_str("\nmmportlens --column state==Established --column remote_port=443|80 --export connections.xlsx | Out-Null");
    s.push_str(
        "\nmmportlens --filter chrome --record C:\\Temp\\chrome.plrec --duration 10m | Out-Null",
    );
    s
}

/// Shows a message: `console` on stdout / stderr (`error`) and, on Windows (the release build
/// has no console), `boxed` in a message box.
pub fn show_message(title: &str, console: &str, boxed: &str, error: bool) {
    if error {
        eprintln!("{console}");
    } else {
        println!("{console}");
    }
    #[cfg(windows)]
    message_box(title, boxed, error);
    #[cfg(not(windows))]
    let _ = (title, boxed);
}

#[cfg(windows)]
fn message_box(title: &str, text: &str, error: bool) {
    #[link(name = "user32")]
    unsafe extern "system" {
        fn MessageBoxW(hwnd: isize, text: *const u16, caption: *const u16, kind: u32) -> i32;
    }
    const MB_ICONERROR: u32 = 0x10;
    const MB_ICONINFORMATION: u32 = 0x40;
    let wide = |s: &str| s.encode_utf16().chain([0]).collect::<Vec<u16>>();
    let (text, title) = (wide(text), wide(title));
    let icon = if error {
        MB_ICONERROR
    } else {
        MB_ICONINFORMATION
    };
    // SAFETY: both strings are NUL-terminated UTF-16 and live until the call returns.
    unsafe {
        MessageBoxW(0, text.as_ptr(), title.as_ptr(), icon);
    }
}

/// Lets a program without a console (the release build) print to the console it was started
/// from (`cmd`, PowerShell) and receive its Ctrl+C; does nothing when there is none.
pub fn attach_console() {
    #[cfg(windows)]
    {
        #[link(name = "kernel32")]
        unsafe extern "system" {
            fn AttachConsole(process: u32) -> i32;
        }
        const ATTACH_PARENT_PROCESS: u32 = u32::MAX;
        // SAFETY: no pointers; failing (no parent console, or a console already) is fine.
        unsafe {
            AttachConsole(ATTACH_PARENT_PROCESS);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(args: &[&str]) -> Result<Options, CliError> {
        parse(args.iter().map(OsString::from))
    }

    #[test]
    fn recording_file_and_no_arguments() {
        assert_eq!(p(&[]).unwrap(), Options::default());
        let o = p(&[r"C:\Captures\a b.PLREC"]).unwrap();
        assert_eq!(o.recording, Some(PathBuf::from(r"C:\Captures\a b.PLREC")));
        assert!(!o.headless());
        assert_eq!(
            p(&["notes.txt"]),
            Err(CliError::UnexpectedArgument("notes.txt".into()))
        );
        assert_eq!(
            p(&["a.plrec", "b.plrec"]),
            Err(CliError::UnexpectedArgument("b.plrec".into()))
        );
        // After `--` an option-like name is the recording.
        let o = p(&["--", "--x.plrec"]).unwrap();
        assert_eq!(o.recording, Some(PathBuf::from("--x.plrec")));
    }

    #[test]
    fn view_options() {
        let o = p(&[
            "--preset=Listening",
            "--filter",
            "chrome",
            "--column",
            "state==Established",
            "--column=Remote port = 443|80",
            "--interval",
            "500",
            "--paused",
            "--lang",
            "de",
        ])
        .unwrap();
        assert_eq!(
            o.view,
            ViewOptions {
                preset: Some("Listening".into()),
                filter: Some("chrome".into()),
                columns: vec![
                    ("state".into(), "=Established".into()),
                    ("Remote port".into(), " 443|80".into()),
                ],
                interval: Some(500),
                paused: true,
            }
        );
        assert_eq!(o.lang.as_deref(), Some("de"));
        // The form of #13 keeps working; the last one counts.
        let o = p(&["--preset", "a", "--preset", "b"]).unwrap();
        assert_eq!(o.view.preset.as_deref(), Some("b"));
        // A filter text that looks like an option is a value.
        let o = p(&["--filter", "--help"]).unwrap();
        assert_eq!(o.view.filter.as_deref(), Some("--help"));
        assert!(!o.help);
    }

    #[test]
    fn headless_options() {
        let o = p(&["--export", "x.csv"]).unwrap();
        assert_eq!(o.export, Some(PathBuf::from("x.csv")));
        assert!(o.headless());
        let o = p(&["--record=r.plrec", "--duration", "1h30m", "--interval=2000"]).unwrap();
        assert_eq!(o.record, Some(PathBuf::from("r.plrec")));
        assert_eq!(o.duration, Some(5_400_000));
        assert_eq!(o.view.interval, Some(2000));
        assert!(p(&["--record", "r.plrec"]).unwrap().duration.is_none());
    }

    #[test]
    fn help_version_restore_elevated() {
        for h in ["--help", "-h", "-?", "/?"] {
            assert!(p(&[h]).unwrap().help, "{h}");
        }
        assert!(p(&["--version"]).unwrap().version);
        assert!(p(&["-V"]).unwrap().version);
        let o = p(&["--restore", r"C:\Temp\mmportlens-restore-1.json"]).unwrap();
        assert_eq!(
            o.restore,
            Some(PathBuf::from(r"C:\Temp\mmportlens-restore-1.json"))
        );
        // The elevated restart passes the original arguments on.
        let o = p(&["--preset", "x", "a.plrec", "--elevated"]).unwrap();
        assert!(o.elevated && o.recording.is_some());
    }

    #[test]
    fn errors() {
        let err = |a: &[&str]| p(a).unwrap_err();
        assert_eq!(err(&["--bogus"]), CliError::UnknownOption("--bogus".into()));
        assert_eq!(err(&["-x"]), CliError::UnknownOption("-x".into()));
        assert_eq!(
            err(&["--filter"]),
            CliError::MissingValue("--filter".into())
        );
        assert_eq!(
            err(&["--preset"]),
            CliError::MissingValue("--preset".into())
        );
        assert_eq!(
            err(&["--paused=yes"]),
            CliError::UnexpectedValue("--paused".into())
        );
        for bad in ["0", "99", "-5", "1s", ""] {
            assert_eq!(
                err(&["--interval", bad]),
                CliError::InvalidInterval(bad.into()),
                "{bad}"
            );
        }
        assert_eq!(
            err(&["--record", "r.plrec", "--duration", "soon"]),
            CliError::InvalidDuration("soon".into())
        );
        for bad in ["state", "=Listen", " =x"] {
            assert_eq!(
                err(&["--column", bad]),
                CliError::ColumnSyntax(bad.into()),
                "{bad}"
            );
        }
        let conflict = |a: &str, b: &str| CliError::Conflict {
            option: a.into(),
            other: b.into(),
        };
        assert_eq!(
            err(&["--export", "x.csv", "--record", "r.plrec"]),
            conflict("--export", "--record")
        );
        assert_eq!(
            err(&["--duration", "5m"]),
            CliError::Needs {
                option: "--duration".into(),
                needs: "--record".into()
            }
        );
        assert_eq!(
            err(&["--export", "x.csv", "a.plrec"]),
            conflict("--export", "RECORDING")
        );
        assert_eq!(
            err(&["--record", "r.plrec", "--paused"]),
            conflict("--record", "--paused")
        );
        assert_eq!(
            err(&["--export", "x.csv", "--interval", "500"]),
            conflict("--export", "--interval")
        );
        assert_eq!(
            err(&["--export", "x.pdf"]),
            CliError::ExportFormat("x.pdf".into())
        );
        // Every error has a translated message.
        let msg = CliError::UnknownOption("--bogus".into()).message();
        assert!(
            msg.contains("--bogus") && !msg.contains("cli.error"),
            "{msg}"
        );
        let msg = CliError::InvalidInterval("5".into()).message();
        assert!(msg.contains("100") && !msg.contains("{min}"), "{msg}");
    }

    #[test]
    fn filters_on_top_of_the_base() {
        let saved = vec![FilterPreset {
            name: "Listening".into(),
            show_udp: false,
            column_filters: vec![(ColumnId::State, "=Listen".into())],
            ..Default::default()
        }];
        let base = FilterPreset {
            show_ipv6: false,
            column_filters: vec![(ColumnId::Process, "svchost".into())],
            ..Default::default()
        };
        let view =
            |preset: Option<&str>, filter: Option<&str>, cols: &[(&str, &str)]| ViewOptions {
                preset: preset.map(Into::into),
                filter: filter.map(Into::into),
                columns: cols
                    .iter()
                    .map(|(c, t)| ((*c).into(), (*t).into()))
                    .collect(),
                ..Default::default()
            };
        assert_eq!(
            filters(&view(None, None, &[]), &saved, base.clone()),
            Ok(None)
        );
        // A preset replaces the base (and keeps its name); --column replaces one column.
        let p = filters(
            &view(Some("listening"), None, &[("STATE", "=Established")]),
            &saved,
            base.clone(),
        )
        .unwrap()
        .unwrap();
        assert_eq!(p.name, "Listening");
        assert!(!p.show_udp && p.show_ipv6);
        assert_eq!(
            p.column_filters,
            vec![(ColumnId::State, "=Established".to_string())]
        );
        // Without a preset: the base with the text and the added column.
        let p = filters(
            &view(None, Some("chrome"), &[("local-port", ">1024")]),
            &saved,
            base.clone(),
        )
        .unwrap()
        .unwrap();
        assert!(p.name.is_empty() && !p.show_ipv6 && p.text == "chrome");
        assert_eq!(p.column_filters.len(), 2);
        // Errors.
        assert_eq!(
            filters(&view(Some("nope"), None, &[]), &saved, base.clone()),
            Err(CliError::UnknownPreset("nope".into()))
        );
        assert_eq!(
            filters(&view(None, None, &[("colour", "x")]), &saved, base.clone()),
            Err(CliError::UnknownColumn("colour".into()))
        );
        assert!(matches!(
            filters(&view(None, None, &[("state", "/(/")]), &saved, base.clone()),
            Err(CliError::ColumnRegex { .. })
        ));
        assert!(matches!(
            filters(
                &view(None, None, &[("remote_address", "10.0.0.0/40")]),
                &saved,
                base
            ),
            Err(CliError::ColumnNetwork { .. })
        ));
    }

    #[test]
    fn columns_and_help() {
        assert_eq!(find_column("process"), Some(ColumnId::Process));
        assert_eq!(find_column("Remote_Address"), Some(ColumnId::RemoteAddress));
        assert_eq!(find_column("bytes-sent"), Some(ColumnId::BytesSent));
        assert_eq!(find_column(&ColumnId::Pid.title()), Some(ColumnId::Pid));
        assert_eq!(find_column("nothing"), None);
        assert!(column_names().starts_with("process, pid, "));
        let langs = vec!["cs".to_owned(), "de".into(), "en".into(), "sk".into()];
        assert_eq!(check_lang("SK", &langs), Ok("sk".into()));
        assert!(matches!(
            check_lang("xx", &langs),
            Err(CliError::InvalidLanguage { .. })
        ));
        let help = help_text("cs, de, en, sk", false);
        assert!(help.contains("--column <") && !help.contains("--restore"));
        assert!(
            !help.contains("--elevated") && !help.contains("cli."),
            "{help}"
        );
        assert!(help_text("en", true).contains("--paused\t"));
    }
}
