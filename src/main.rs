// In release builds on Windows, do not open a console window.
#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

mod actions;
mod app;
mod cli;
mod clihelp;
mod colfilter;
mod columns;
mod config;
mod console;
mod dns;
mod elevation;
mod filterhelp;
mod gpu;
mod grouping;
mod headless;
mod helpwindow;
mod http;
mod i18n;
mod icon;
mod model;
mod net;
mod poller;
mod presets;
mod process;
mod rates;
mod recording;
mod rowfilter;
mod rules;
mod selection;
mod services;
mod stats;
mod tabexport;
mod timefmt;
mod update;
mod winproc;

use eframe::egui;
use std::path::PathBuf;
use std::sync::Arc;

/// Reads the command line (#16). `--help`, `--version`, invalid options and the runs without a
/// window (`--export`, `--record`) end the program here with an exit code (0 = done, 1 = error,
/// 2 = invalid command line); otherwise the window starts with what the command line asks for,
/// with the restore file of `--restore` still to be read.
fn command_line(cfg: &config::Config, raw: &[String]) -> (app::Startup, Option<PathBuf>) {
    // Without a window the messages go to the console it was started from, never to a message
    // box (an unattended task must not wait for a click) – also when the line is invalid.
    let headless_args = raw.iter().any(|a| {
        ["--export", "--record"]
            .iter()
            .any(|o| a == o || a.starts_with(&format!("{o}=")))
    });
    if headless_args {
        console::attach_console();
    }
    let fail = |e: cli::CliError| -> ! {
        let msg = e.message();
        if headless_args || console::from_companion() {
            console::attach_console();
            console::err(&msg);
        } else {
            cli::show_message(&t!("cli.title.error"), &msg, &msg, true);
        }
        std::process::exit(2)
    };
    let opts = cli::parse(raw.iter().map(std::ffi::OsString::from)).unwrap_or_else(|e| fail(e));
    let languages: Vec<String> = i18n::available().into_iter().map(|l| l.code).collect();
    // The language first, so the help and the messages are already in it.
    let lang = match opts.lang.as_deref().map(|l| cli::check_lang(l, &languages)) {
        Some(Ok(code)) => {
            i18n::set_language(&code);
            Some(code)
        }
        Some(Err(e)) if !opts.help && !opts.version => fail(e),
        _ => None,
    };
    if opts.help {
        // The aligned text for the console it was started from (scripts, `| more`), then the
        // help window (#39); closing it ends the program with exit code 0.
        let help = clihelp::build(&languages.join(", "));
        console::attach_console();
        console::out(&help.text(false));
        // The text is out: the shell gets its prompt back while the window stays (#44).
        console::release_companion();
        if let Err(e) = helpwindow::run(cfg, help, app::icon_data()) {
            console::err(&e.to_string());
        }
        std::process::exit(0)
    }
    if opts.version {
        let text = format!("MM PortLens {}", app::VERSION);
        cli::show_message(&t!("cli.title.version"), &text, &text, false);
        std::process::exit(0)
    }
    if opts.restore.is_some() {
        // The state of the previous version after an update or an elevated restart.
        let startup = app::Startup {
            lang,
            ..Default::default()
        };
        return (startup, opts.restore);
    }
    if opts.headless() {
        let filters = cli::filters(&opts.view, &cfg.filter_presets, Default::default())
            .unwrap_or_else(|e| fail(e));
        let res = match (&opts.export, &opts.record) {
            (Some(target), _) => headless::export(target, filters.as_ref(), cfg),
            (None, Some(target)) => headless::record(
                target,
                opts.duration,
                filters.as_ref(),
                opts.view.interval.unwrap_or(cfg.interval_ms),
                opts.etw || cfg.etw_capture,
                cfg,
            ),
            (None, None) => unreachable!("headless without --export / --record"),
        };
        match res {
            Ok(msg) => {
                console::out(&msg);
                std::process::exit(0)
            }
            Err(e) => {
                console::err(&e.to_string());
                std::process::exit(1)
            }
        }
    }
    // The window starts with the saved switches and the remembered column filters; the
    // command line goes on top. An unknown preset is reported in the status bar (as since #13).
    let base = presets::FilterPreset {
        show_tcp: cfg.show_tcp,
        show_udp: cfg.show_udp,
        show_ipv4: cfg.show_ipv4,
        show_ipv6: cfg.show_ipv6,
        show_listening: cfg.show_listening,
        column_filters: if cfg.remember_column_filters {
            cfg.column_filters.clone()
        } else {
            Vec::new()
        },
        ..Default::default()
    };
    let (filters, unknown_preset) =
        match cli::filters(&opts.view, &cfg.filter_presets, base.clone()) {
            Ok(f) => (f, None),
            Err(cli::CliError::UnknownPreset(name)) => {
                let view = cli::ViewOptions {
                    preset: None,
                    ..opts.view.clone()
                };
                let f = cli::filters(&view, &cfg.filter_presets, base).unwrap_or_else(|e| fail(e));
                (f, Some(name))
            }
            Err(e) => fail(e),
        };
    let startup = app::Startup {
        restore: None,
        elevate_error: None,
        recording: opts.recording,
        filters,
        unknown_preset,
        interval: opts.view.interval,
        paused: opts.view.paused,
        lang,
    };
    (startup, None)
}

fn main() -> eframe::Result {
    let args: Vec<String> = std::env::args_os()
        .skip(1)
        .map(|a| a.to_string_lossy().into_owned())
        .collect();
    let cfg = config::Config::load();
    i18n::set_language(&cfg.language);
    // Port names of the system services database (#34), read while the program starts.
    services::load_system();
    let (mut startup, restore_file) = command_line(&cfg, &args);
    // The window opens: `mmportlens.com` stops waiting (#44).
    console::release_companion();

    // "Always start as administrator": start again through UAC and end this one. The elevated
    // child gets ELEVATED_ARG and never tries again; it reads the same command line. Before the
    // restore file is read, which deletes the file the child still needs.
    if cfg.run_as_admin
        && !args.iter().any(|a| a == elevation::ELEVATED_ARG)
        && !elevation::is_elevated()
    {
        let mut child_args = args.clone();
        child_args.push(elevation::ELEVATED_ARG.into());
        match elevation::relaunch_elevated(&child_args) {
            Ok(()) => return Ok(()),
            Err(e) => startup.elevate_error = Some(e),
        }
    }
    if let Some(file) = restore_file {
        startup.restore = update::SessionState::take(&file);
    }
    update::cleanup_old();

    let mut viewport = egui::ViewportBuilder::default().with_inner_size([1300.0, 750.0]);
    if let Some(w) = startup.restore.as_ref().and_then(|s| s.window.as_ref()) {
        if let Some(pos) = w.pos {
            viewport = viewport.with_position(pos);
        }
        if let Some(size) = w.size {
            viewport = viewport.with_inner_size(size);
        }
        viewport = viewport.with_maximized(w.maximized);
    }
    let options = gpu::native_options(
        viewport
            .with_min_inner_size([700.0, 350.0])
            .with_title(app::base_title())
            .with_icon(Arc::new(app::icon_data())),
    );
    eframe::run_native(
        "MMPortLens",
        options,
        Box::new(|cc| Ok(Box::new(app::App::new(cc, startup)))),
    )
}
