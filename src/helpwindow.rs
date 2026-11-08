//! The `--help` window (#39): the command-line help of #38 (`app::cli_help::cli_help_ui`) in a
//! small resizable eframe window in the look of the application instead of a message box, with
//! zoomable text (Ctrl + wheel, Ctrl + `+` / `-` / `0`, the buttons of the bottom bar). The zoom
//! and the window size are kept in `Config::help_window`.

use eframe::egui::{self, Key, RichText};
use serde::{Deserialize, Serialize};

use crate::clihelp::CliHelp;
use crate::config::Config;
use crate::t;

/// Smallest and largest zoom, step of a button / key press.
pub const MIN_ZOOM: f32 = 0.5;
pub const MAX_ZOOM: f32 = 3.0;
const ZOOM_STEP: f32 = 0.1;
const DEFAULT_SIZE: [f32; 2] = [800.0, 600.0];
const MIN_SIZE: [f32; 2] = [520.0, 320.0];

/// What the `--help` window remembers.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct HelpWindowState {
    /// Text zoom (1.0 = 100 %).
    pub zoom: f32,
    /// Inner size in logical pixels of the last closed window.
    pub size: Option<[f32; 2]>,
}

impl Default for HelpWindowState {
    fn default() -> Self {
        Self {
            zoom: 1.0,
            size: None,
        }
    }
}

/// A zoom inside the limits, rounded to whole percent (so ten steps return exactly to 100 %).
pub fn clamp_zoom(z: f32) -> f32 {
    if !z.is_finite() {
        return 1.0;
    }
    (z.clamp(MIN_ZOOM, MAX_ZOOM) * 100.0).round() / 100.0
}

/// Opens the window and returns when it is closed.
pub fn run(cfg: &Config, help: CliHelp, icon: egui::IconData) -> eframe::Result {
    let state = HelpWindowState {
        zoom: clamp_zoom(cfg.help_window.zoom),
        size: cfg.help_window.size,
    };
    let size = state
        .size
        .map(|[w, h]| [w.max(MIN_SIZE[0]), h.max(MIN_SIZE[1])])
        .unwrap_or(DEFAULT_SIZE);
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size(size)
            .with_min_inner_size(MIN_SIZE)
            .with_resizable(true)
            .with_title(t!("cli.title.help"))
            .with_icon(std::sync::Arc::new(icon)),
        ..Default::default()
    };
    let theme = crate::app::theme_preference(&cfg.theme);
    eframe::run_native(
        "MMPortLens-help",
        options,
        Box::new(move |cc| {
            cc.egui_ctx.set_theme(theme);
            // The window handles the zoom keys itself, so it can show and keep the zoom.
            cc.egui_ctx.options_mut(|o| o.zoom_with_keyboard = false);
            cc.egui_ctx.set_zoom_factor(state.zoom);
            Ok(Box::new(HelpWindow { help, state }))
        }),
    )
}

struct HelpWindow {
    help: CliHelp,
    state: HelpWindowState,
}

impl HelpWindow {
    fn set_zoom(&mut self, ctx: &egui::Context, z: f32) {
        self.state.zoom = clamp_zoom(z);
        ctx.set_zoom_factor(self.state.zoom);
    }

    /// Ctrl + wheel and Ctrl + `+` / `-` / `0`.
    fn zoom_input(&mut self, ctx: &egui::Context) {
        let (wheel, keys) = ctx.input(|i| (i.zoom_delta(), zoom_keys(&i.events)));
        let (plus, minus, reset) = keys;
        if wheel != 1.0 {
            self.set_zoom(ctx, self.state.zoom * wheel);
        }
        if plus {
            self.set_zoom(ctx, self.state.zoom + ZOOM_STEP);
        }
        if minus {
            self.set_zoom(ctx, self.state.zoom - ZOOM_STEP);
        }
        if reset {
            self.set_zoom(ctx, 1.0);
        }
    }

    /// Keeps the zoom and the window size for the next `--help` (a fresh copy of the settings,
    /// so nothing else is overwritten).
    fn save(&mut self, ctx: &egui::Context) {
        if let Some(r) = ctx.input(|i| i.viewport().inner_rect) {
            // egui points include the zoom; the window size is in unzoomed logical pixels.
            let z = ctx.zoom_factor();
            self.state.size = Some([r.width() * z, r.height() * z]);
        }
        let mut cfg = Config::load();
        if cfg.help_window != self.state {
            cfg.help_window = self.state.clone();
            let _ = cfg.save();
        }
    }
}

impl eframe::App for HelpWindow {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.zoom_input(&ctx);
        let mut close = ctx.input(|i| i.key_pressed(Key::Escape));
        egui::Panel::bottom("help_bottom").show(ui, |ui| {
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                if ui
                    .button("A−")
                    .on_hover_text(t!("helpwin.zoom_out"))
                    .clicked()
                {
                    self.set_zoom(&ctx, self.state.zoom - ZOOM_STEP);
                }
                let pct = format!("{:.0} %", self.state.zoom * 100.0);
                if ui
                    .button(RichText::new(pct).monospace())
                    .on_hover_text(t!("helpwin.zoom_reset"))
                    .clicked()
                {
                    self.set_zoom(&ctx, 1.0);
                }
                if ui
                    .button("A+")
                    .on_hover_text(t!("helpwin.zoom_in"))
                    .clicked()
                {
                    self.set_zoom(&ctx, self.state.zoom + ZOOM_STEP);
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    // Room for the resize grip in the corner.
                    ui.add_space(14.0);
                    if ui.button(t!("helpwin.close")).clicked() {
                        close = true;
                    }
                    // The hint gives way (shortened) when the zoomed buttons need the room.
                    ui.add(
                        egui::Label::new(RichText::new(t!("helpwin.zoom_hint")).weak()).truncate(),
                    );
                });
            });
            ui.add_space(4.0);
            // A resize grip in the bottom-right corner (the OS window is resized by its edges).
            let rect = ui.max_rect();
            let grip = egui::Rect::from_min_max(rect.max - egui::vec2(12.0, 12.0), rect.max);
            paint_grip(ui, grip);
        });
        egui::CentralPanel::default().show(ui, |ui| {
            crate::app::cli_help::cli_help_ui(ui, &self.help);
        });
        if ctx.input(|i| i.viewport().close_requested()) {
            self.save(&ctx);
        } else if close {
            self.save(&ctx);
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }
}

/// The zoom keys pressed in this frame: (larger, smaller, reset). Ctrl with `+` / `=`, `-`, `0`
/// (also on the numeric keypad); Shift is ignored (`+` needs it on many layouts) and the physical
/// key counts too, so Ctrl + 0 works where the key gives another character (é on the Slovak
/// layout).
pub fn zoom_keys(events: &[egui::Event]) -> (bool, bool, bool) {
    let (mut plus, mut minus, mut reset) = (false, false, false);
    for e in events {
        let egui::Event::Key {
            key,
            physical_key,
            pressed: true,
            modifiers,
            ..
        } = e
        else {
            continue;
        };
        if !modifiers.command || modifiers.alt {
            continue;
        }
        let is = |k: Key| *key == k || *physical_key == Some(k);
        plus |= is(Key::Plus) || is(Key::Equals);
        minus |= is(Key::Minus);
        reset |= is(Key::Num0);
    }
    (plus, minus, reset)
}

/// Diagonal lines in the bottom-right corner of `rect`, like the resize corner of an egui window.
fn paint_grip(ui: &egui::Ui, rect: egui::Rect) {
    let stroke = egui::Stroke::new(1.0, ui.visuals().weak_text_color());
    let corner = rect.max;
    let mut w = 2.0;
    while w <= rect.width() && w <= rect.height() {
        ui.painter().line_segment(
            [corner - egui::vec2(w, 0.0), corner - egui::vec2(0.0, w)],
            stroke,
        );
        w += 4.0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zoom_limits_and_rounding() {
        assert_eq!(clamp_zoom(1.0), 1.0);
        assert_eq!(clamp_zoom(0.1), MIN_ZOOM);
        assert_eq!(clamp_zoom(9.0), MAX_ZOOM);
        assert_eq!(clamp_zoom(f32::NAN), 1.0);
        // Ten steps up and down end at 100 % again.
        let mut z = 1.0;
        for _ in 0..10 {
            z = clamp_zoom(z + ZOOM_STEP);
        }
        assert_eq!(z, 2.0);
        for _ in 0..10 {
            z = clamp_zoom(z - ZOOM_STEP);
        }
        assert_eq!(z, 1.0);
    }

    #[test]
    fn zoom_keys_with_ctrl_and_physical_keys() {
        let key = |key: Key, physical: Option<Key>, modifiers: egui::Modifiers| egui::Event::Key {
            key,
            physical_key: physical,
            pressed: true,
            repeat: false,
            modifiers,
        };
        let ctrl = egui::Modifiers::COMMAND;
        let ctrl_shift = egui::Modifiers {
            shift: true,
            ..egui::Modifiers::COMMAND
        };
        assert_eq!(
            zoom_keys(&[key(Key::Plus, None, ctrl_shift)]),
            (true, false, false)
        );
        assert_eq!(
            zoom_keys(&[key(Key::Equals, None, ctrl)]),
            (true, false, false)
        );
        assert_eq!(
            zoom_keys(&[key(Key::Minus, None, ctrl)]),
            (false, true, false)
        );
        assert_eq!(
            zoom_keys(&[key(Key::Num0, None, ctrl)]),
            (false, false, true)
        );
        // Slovak layout: the 0 key types another character, the physical key is Num0.
        assert_eq!(
            zoom_keys(&[key(Key::Space, Some(Key::Num0), ctrl)]),
            (false, false, true)
        );
        // Without Ctrl nothing.
        assert_eq!(
            zoom_keys(&[key(Key::Minus, None, egui::Modifiers::NONE)]),
            (false, false, false)
        );
    }

    #[test]
    fn state_defaults_in_old_settings() {
        let cfg = Config::parse("{}").unwrap();
        assert_eq!(cfg.help_window, HelpWindowState::default());
        let cfg = Config::parse(r#"{"help_window": {"zoom": 1.5}}"#).unwrap();
        assert_eq!(cfg.help_window.zoom, 1.5);
        assert_eq!(cfg.help_window.size, None);
    }
}
