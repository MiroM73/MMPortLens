//! The command-line help as structured data (#38): usage, options, column names, exit codes and
//! examples. One source for the console text of `--help` ([`CliHelp::text`]), the Help →
//! Command line window and the `--help` window (#39), which both draw it with
//! `app::cli_help::cli_help_ui`. The texts come from the `cli.*` keys of the language files; the
//! example commands themselves stay untranslated.

use crate::cli::{self, MIN_INTERVAL_MS, PUBLIC_OPTIONS};
use crate::columns::ColumnId;
use crate::t;

/// One option: the option with its placeholder (`--preset <NAME>`) and the description.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HelpOption {
    pub option: String,
    pub description: String,
}

/// One example: the command as typed and what it does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Example {
    pub command: String,
    pub what: String,
}

/// The whole help in the current language.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CliHelp {
    pub usage: String,
    /// The optional RECORDING argument.
    pub file: String,
    pub options: Vec<HelpOption>,
    /// `--column` names: machine name and the column title in the UI language.
    pub columns: Vec<(String, String)>,
    pub exit: String,
    /// How a console waits for the program without a window.
    pub wait: String,
    pub examples: Vec<Example>,
}

/// The help; `languages` = the codes `--lang` accepts.
pub fn build(languages: &str) -> CliHelp {
    let meta = |key: &str| format!("<{}>", t!(key));
    let options = PUBLIC_OPTIONS
        .iter()
        .map(|&(name, _)| {
            let (placeholder, description) = match name {
                "--preset" => (meta("cli.meta.name"), t!("cli.opt.preset")),
                "--filter" => (meta("cli.meta.text"), t!("cli.opt.filter")),
                "--column" => (
                    format!("{}={}", meta("cli.meta.column"), meta("cli.meta.filter")),
                    t!("cli.opt.column"),
                ),
                "--interval" => (
                    meta("cli.meta.ms"),
                    t!("cli.opt.interval", min = MIN_INTERVAL_MS),
                ),
                "--paused" => (String::new(), t!("cli.opt.paused")),
                "--export" => (meta("cli.meta.file"), t!("cli.opt.export")),
                "--record" => (meta("cli.meta.file"), t!("cli.opt.record")),
                "--duration" => (meta("cli.meta.time"), t!("cli.opt.duration")),
                "--etw" => (String::new(), t!("cli.opt.etw")),
                "--lang" => (
                    meta("cli.meta.code"),
                    t!("cli.opt.lang", languages = languages),
                ),
                "--help" => (String::new(), t!("cli.opt.help")),
                "--version" => (String::new(), t!("cli.opt.version")),
                "--" => (String::new(), t!("cli.opt.end")),
                other => unreachable!("option {other} without help"),
            };
            let option = if placeholder.is_empty() {
                name.to_owned()
            } else {
                format!("{name} {placeholder}")
            };
            HelpOption {
                option,
                description,
            }
        })
        .collect();
    CliHelp {
        usage: t!("cli.usage"),
        file: t!("cli.file"),
        options,
        columns: ColumnId::ALL
            .iter()
            .map(|&c| (cli::column_name(c).to_owned(), c.title()))
            .collect(),
        exit: t!("cli.exit"),
        wait: t!("cli.wait"),
        examples: examples(),
    }
}

/// The examples; the scheduled task uses the path of this program.
fn examples() -> Vec<Example> {
    let exe = std::env::current_exe()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| "mmportlens.exe".into());
    let exe = if exe.contains(' ') {
        format!("\\\"{exe}\\\"")
    } else {
        exe
    };
    let list = [
        ("mmportlens --preset Listening --paused", "cli.ex.preset"),
        (
            "mmportlens --column state==Established --column \"remote_port=443|80\"",
            "cli.ex.established",
        ),
        (
            "mmportlens --column state==Listen --export listeners.csv | Out-Null",
            "cli.ex.export",
        ),
        (
            "mmportlens --filter chrome --record C:\\Temp\\chrome.plrec --duration 10m | Out-Null",
            "cli.ex.record",
        ),
        (
            "mmportlens --record C:\\Temp\\short.plrec --duration 5m --etw | Out-Null",
            "cli.ex.etw",
        ),
        ("mmportlens C:\\Temp\\chrome.plrec", "cli.ex.replay"),
        ("", "cli.ex.task"),
        ("mmportlens --lang de --help", "cli.ex.lang"),
    ];
    list.into_iter()
        .map(|(command, key)| Example {
            command: if key == "cli.ex.task" {
                format!(
                    "schtasks /Create /SC HOURLY /TN \"MM PortLens recording\" /TR \"{exe} --record C:\\Temp\\hourly.plrec --duration 55m\""
                )
            } else {
                command.to_owned()
            },
            what: t!(key),
        })
        .collect()
}

impl CliHelp {
    /// The help as text: `tabs` separates an option and its description with a tab (message
    /// box) instead of padding (console).
    pub fn text(&self, tabs: bool) -> String {
        let width = self
            .options
            .iter()
            .map(|o| o.option.chars().count())
            .max()
            .unwrap_or(0);
        let mut s = format!("{}\n\n{}\n\n", self.usage, self.file);
        for o in &self.options {
            if tabs {
                s.push_str(&format!("{}\t{}\n", o.option, o.description));
            } else {
                s.push_str(&format!("  {:width$}  {}\n", o.option, o.description));
            }
        }
        s.push('\n');
        let names: Vec<&str> = self.columns.iter().map(|(n, _)| n.as_str()).collect();
        s.push_str(&t!("cli.columns", columns = names.join(", ")));
        s.push_str("\n\n");
        s.push_str(&self.exit);
        s.push(' ');
        s.push_str(&self.wait);
        s.push_str("\n\n");
        s.push_str(&t!("cli.example"));
        for e in &self.examples {
            s.push_str(&format!("\n  {}:\n    {}", e.what, e.command));
        }
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsString;

    #[test]
    fn every_parser_option_is_in_the_help() {
        let help = build("cs, de, en, sk");
        assert_eq!(help.options.len(), PUBLIC_OPTIONS.len());
        for (o, &(name, takes_value)) in help.options.iter().zip(PUBLIC_OPTIONS) {
            let first = o.option.split(' ').next().unwrap();
            assert_eq!(first, name);
            // A value option shows its placeholder, a switch none.
            assert_eq!(o.option.contains('<'), takes_value, "{}", o.option);
            assert!(!o.description.is_empty() && !o.description.contains("cli."));
            // The parser knows it.
            if name != "--" {
                let r = cli::parse([OsString::from(name), OsString::from("x")]);
                assert!(
                    !matches!(r, Err(cli::CliError::UnknownOption(_))),
                    "{name}: {r:?}"
                );
            }
        }
        assert_eq!(help.columns.len(), ColumnId::ALL.len());
        assert_eq!(help.columns[0].0, "process");
        assert!(help.examples.len() >= 7);
    }

    #[test]
    fn text_is_built_from_the_same_data() {
        let help = build("en");
        let text = help.text(false);
        for o in &help.options {
            assert!(text.contains(&o.option) && text.contains(&o.description));
        }
        for e in &help.examples {
            assert!(text.contains(&e.command), "{}", e.command);
        }
        assert!(text.contains("--column <") && !text.contains("--restore"));
        assert!(
            !text.contains("--elevated") && !text.contains("cli."),
            "{text}"
        );
        assert!(help.text(true).contains("--paused\t"));
    }

    #[test]
    fn examples_parse() {
        // Every mmportlens example is a valid command line (quotes and the pipe removed, as the
        // shell does).
        for e in build("en").examples {
            let Some(rest) = e.command.strip_prefix("mmportlens ") else {
                continue;
            };
            let rest = rest.trim_end_matches(" | Out-Null");
            let args: Vec<OsString> = rest
                .split(' ')
                .map(|a| OsString::from(a.trim_matches('"')))
                .collect();
            assert!(cli::parse(args).is_ok(), "{}", e.command);
        }
    }
}
