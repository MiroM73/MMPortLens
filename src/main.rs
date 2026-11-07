// In release builds on Windows, do not open a console window.
#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

mod app;
mod columns;
mod config;
mod i18n;
mod model;
mod net;
mod poller;
mod process;
mod timefmt;

use eframe::egui;

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1300.0, 750.0])
            .with_min_inner_size([700.0, 350.0])
            .with_title(app::base_title()),
        ..Default::default()
    };
    eframe::run_native(
        "RusNet",
        options,
        Box::new(|cc| Ok(Box::new(app::App::new(cc)))),
    )
}
