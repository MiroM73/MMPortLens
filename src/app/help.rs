//! Help → Keyboard shortcuts (#17). The same list is in `docs/keyboard-shortcuts.md`.

use crate::app::*;

/// Shortcut (modifier names localized by [`kb`]) and the key of its description.
const SHORTCUTS: &[(&str, &str)] = &[
    ("F5", "help.refresh"),
    ("Space", "help.pause"),
    ("Ctrl+F", "help.filter"),
    ("Esc", "help.escape"),
    ("↑ ↓ PgUp PgDn Home End", "help.navigate"),
    ("Shift + ↑ ↓ PgUp PgDn Home End", "help.extend"),
    ("Ctrl / Shift + click", "help.select_click"),
    ("Shift + wheel", "help.hscroll"),
    ("Ctrl+A", "help.select_all"),
    ("Ctrl+C", "help.copy"),
    ("Del", "help.close"),
    ("F1", "help.shortcuts"),
];

/// Localizes the modifier key names in a shortcut ("Ctrl+F" → "Strg+F" in German).
pub(in crate::app) fn kb(shortcut: &str) -> String {
    shortcut
        .replace("Ctrl", &t!("key.ctrl"))
        .replace("Shift", &t!("key.shift"))
        .replace("Space", &t!("key.space"))
        .replace("Del", &t!("key.del"))
        .replace("wheel", &t!("key.wheel"))
}

impl App {
    pub(in crate::app) fn shortcuts_window(&mut self, ctx: &egui::Context) {
        egui::Window::new(t!("help.title"))
            .open(&mut self.show_shortcuts)
            .collapsible(false)
            .resizable(false)
            .show(ctx, |ui| {
                egui::Grid::new("shortcuts")
                    .spacing([20.0, 4.0])
                    .striped(true)
                    .show(ui, |ui| {
                        for (k, d) in SHORTCUTS {
                            ui.monospace(kb(k));
                            ui.label(t!(d));
                            ui.end_row();
                        }
                    });
            });
    }
}
