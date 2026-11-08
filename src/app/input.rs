//! Keyboard shortcuts (#17), handled at the start of a frame before the widgets, so a consumed
//! key (Space, arrows) never reaches a focused button as well.
//!
//! egui's `consume_key` ignores an *extra* Shift (`Modifiers::matches_logically`), so a pattern
//! without Shift would also take Shift+key; the navigation keys therefore read the modifiers
//! themselves (same note as in RusTrace #46).

use crate::app::*;
use crate::selection::Nav;
use egui::{Key, Modifiers};

/// Id of the toolbar filter field (Ctrl+F focuses it).
pub(in crate::app) const FILTER_ID: &str = "toolbar_filter";

impl App {
    pub(in crate::app) fn handle_input(&mut self, ctx: &egui::Context) {
        // The confirmation window of #6 takes Enter / Esc itself.
        if self.pending_action.is_some() {
            return;
        }
        // A focused text field gets the keys (typing a space, arrows in the field …).
        let typing = ctx.egui_wants_keyboard_input();
        // Esc / arrows belong to an open menu or combo box.
        let popup_open = egui::Popup::is_any_open(ctx);
        let filter_id = egui::Id::unique(FILTER_ID);
        let filter_focused = ctx.memory(|m| m.has_focus(filter_id));

        let (f5, ctrl_f, ctrl_a, help, export, props) = ctx.input_mut(|i| {
            (
                i.consume_key(Modifiers::NONE, Key::F5),
                i.consume_key(Modifiers::COMMAND, Key::F),
                !typing && i.consume_key(Modifiers::COMMAND, Key::A),
                i.consume_key(Modifiers::NONE, Key::F1),
                i.consume_key(Modifiers::COMMAND, Key::S),
                !typing && i.consume_key(Modifiers::ALT, Key::Enter),
            )
        });
        if props {
            self.open_properties_of_cursor();
        }
        if export {
            self.open_export();
        }
        if f5 {
            self.ctl.refresh_now.store(true, Ordering::Relaxed);
        }
        if ctrl_f {
            ctx.memory_mut(|m| m.request_focus(filter_id));
        }
        if help {
            self.show_shortcuts = true;
        }
        if ctrl_a {
            let keys = self.view_keys();
            self.sel.select_all(&keys);
        }

        // Esc: clears the text filter (also from inside the field), then the selection.
        let esc = !popup_open
            && (!typing || filter_focused)
            && ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Escape));
        if esc {
            if !filter_focused && self.close_properties() {
                // The properties window (#28) closes first.
            } else if !self.filter.is_empty() {
                self.filter.clear();
                self.view_dirty = true;
            } else {
                self.sel.clear();
            }
            if filter_focused {
                ctx.memory_mut(|m| m.surrender_focus(filter_id));
            }
        }

        if typing || popup_open {
            return;
        }
        let space = ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Space));
        if space {
            let paused = self.paused();
            self.ctl.paused.store(!paused, Ordering::Relaxed);
        }
        // Del: close the selected connections (asks first, #6).
        if ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Delete)) {
            self.request_close();
        }
        // Copy: egui turns Ctrl+C into an `Event::Copy` (no key event).
        let copy = ctx.input(|i| i.events.iter().any(|e| matches!(e, egui::Event::Copy)));
        if copy && !self.sel.is_empty() {
            self.copy_selection(ctx);
        }

        let (nav, shift) = ctx.input_mut(|i| {
            let shift = i.modifiers.shift;
            let mut nav = None;
            for (key, n) in [
                (Key::ArrowUp, Nav::Up),
                (Key::ArrowDown, Nav::Down),
                (Key::PageUp, Nav::PageUp),
                (Key::PageDown, Nav::PageDown),
                (Key::Home, Nav::Home),
                (Key::End, Nav::End),
            ] {
                let mods = if shift {
                    Modifiers::SHIFT
                } else {
                    Modifiers::NONE
                };
                if i.consume_key(mods, key) {
                    nav = Some(n);
                }
            }
            (nav, shift)
        });
        if let Some(nav) = nav {
            let keys = self.view_keys();
            if let Some(pos) = nav.target(self.sel.cursor_pos(&keys), keys.len()) {
                if shift {
                    self.sel.extend_to(pos, &keys, false);
                } else {
                    self.sel.select_only(keys[pos]);
                }
                self.scroll_to = Some(pos);
            }
        }
    }

    /// Keys of the rows shown, in table order.
    pub(in crate::app) fn view_keys(&self) -> Vec<ConnKey> {
        self.view.iter().map(|r| r.conn.key()).collect()
    }

    /// Ctrl+C / context menu: the selected rows tab-separated in the visible columns, with a
    /// header line when there are several rows.
    pub(in crate::app) fn copy_selection(&mut self, ctx: &egui::Context) {
        let rows = self.selected_rows();
        if rows.is_empty() {
            return;
        }
        let cols = self.cfg.columns.visible();
        // A value with a tab, a quote or a line break is quoted like in a CSV file (#9), so the
        // rows paste into Excel as they are.
        let line = |values: Vec<String>| {
            let mut s = String::new();
            for (i, v) in values.iter().enumerate() {
                if i > 0 {
                    s.push('\t');
                }
                crate::tabexport::csv::push_field(&mut s, v, '\t', false);
            }
            s
        };
        let mut lines = Vec::with_capacity(rows.len() + 1);
        if rows.len() > 1 {
            lines.push(line(cols.iter().map(|c| c.title()).collect()));
        }
        for r in &rows {
            lines.push(line(cols.iter().map(|&c| cell_text(r, c)).collect()));
        }
        ctx.copy_text(lines.join("\r\n"));
    }
}
