// In release builds on Windows, do not open a console window.
#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

mod actions;
mod app;
mod colfilter;
mod columns;
mod config;
mod dns;
mod elevation;
mod http;
mod i18n;
mod icon;
mod model;
mod net;
mod poller;
mod process;
mod rules;
mod selection;
mod services;
mod tabexport;
mod timefmt;
mod update;
mod winproc;

use eframe::egui;
use std::sync::Arc;

/// The state of the previous version after an update or an elevated restart
/// (`--restore <file>`), if the program was started that way. Reading deletes the file.
fn restore_arg(args: &[String]) -> Option<update::SessionState> {
    let i = args.iter().position(|a| a == update::RESTORE_ARG)?;
    update::SessionState::take(std::path::Path::new(args.get(i + 1)?))
}

fn main() -> eframe::Result {
    let args: Vec<String> = std::env::args_os()
        .skip(1)
        .map(|a| a.to_string_lossy().into_owned())
        .collect();
    let cfg = config::Config::load();
    i18n::set_language(&cfg.language);

    // "Always start as administrator": start again through UAC and end this one. The elevated
    // child gets ELEVATED_ARG and never tries again. Before `restore_arg`, which deletes the
    // file the child still needs.
    let mut elevate_error = None;
    if cfg.run_as_admin
        && !args.iter().any(|a| a == elevation::ELEVATED_ARG)
        && !elevation::is_elevated()
    {
        let mut child_args = args.clone();
        child_args.push(elevation::ELEVATED_ARG.into());
        match elevation::relaunch_elevated(&child_args) {
            Ok(()) => return Ok(()),
            Err(e) => elevate_error = Some(e),
        }
    }

    let restore = restore_arg(&args);
    update::cleanup_old();

    let mut viewport = egui::ViewportBuilder::default().with_inner_size([1300.0, 750.0]);
    if let Some(w) = restore.as_ref().and_then(|s| s.window.as_ref()) {
        if let Some(pos) = w.pos {
            viewport = viewport.with_position(pos);
        }
        if let Some(size) = w.size {
            viewport = viewport.with_inner_size(size);
        }
        viewport = viewport.with_maximized(w.maximized);
    }
    let options = eframe::NativeOptions {
        viewport: viewport
            .with_min_inner_size([700.0, 350.0])
            .with_title(app::base_title())
            .with_icon(Arc::new(egui::IconData {
                rgba: icon::rgba(64),
                width: 64,
                height: 64,
            })),
        ..Default::default()
    };
    eframe::run_native(
        "MMPortLens",
        options,
        Box::new(|cc| Ok(Box::new(app::App::new(cc, restore, elevate_error)))),
    )
}
