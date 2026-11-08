//! Close connection / End process (#6): the context menu items, the confirmation window and the
//! result in the status bar. The system calls are in `crate::actions`.

use crate::actions::{self, ActionError, CloseBlock, KillBlock};
use crate::app::*;
use crate::net::Connection;
use crate::process::ProcessInfo;

/// Lines listed in the confirmation window; the rest is summarized.
const LIST_MAX: usize = 12;

/// An action waiting for the user's confirmation.
pub(in crate::app) enum Pending {
    Close(Vec<(Connection, Arc<ProcessInfo>)>),
    Kill(Vec<(u32, Arc<ProcessInfo>)>),
}

/// An item chosen in the row context menu (run after the table is drawn).
#[derive(Clone, Copy, PartialEq, Eq)]
pub(in crate::app) enum MenuAction {
    Close,
    Kill,
}

/// Result of the last action, shown in the status bar until the next one.
pub(in crate::app) struct ActionResult {
    pub text: String,
    pub error: bool,
}

fn close_block_text(b: CloseBlock) -> String {
    match b {
        CloseBlock::Udp => t!("action.close.udp"),
        CloseBlock::Ipv6 => t!("action.close.ipv6"),
        CloseBlock::Listener => t!("action.close.listener"),
        CloseBlock::AlreadyClosed => t!("action.close.closed"),
        CloseBlock::Platform => t!("action.unsupported"),
    }
}

fn kill_block_text(b: KillBlock) -> String {
    match b {
        KillBlock::System => t!("action.kill.system"),
        KillBlock::Own => t!("action.kill.own"),
    }
}

impl App {
    /// The selected rows that are shown, in table order.
    pub(in crate::app) fn selected_rows(&self) -> Vec<&Row> {
        let keys = self.view_keys();
        self.sel
            .positions(&keys)
            .into_iter()
            .map(|i| &self.view[i])
            .collect()
    }

    /// The selected connections that can be closed, and the reason of the first one that cannot.
    fn closable(&self) -> (Vec<(Connection, Arc<ProcessInfo>)>, Option<CloseBlock>) {
        let mut ok = Vec::new();
        let mut why = None;
        for r in self.selected_rows() {
            match actions::can_close(&r.conn) {
                Ok(()) => ok.push((r.conn.clone(), r.process.clone())),
                Err(b) => why = why.or(Some(b)),
            }
        }
        (ok, why)
    }

    /// The distinct processes of the selected rows that can be ended.
    fn killable(&self) -> (Vec<(u32, Arc<ProcessInfo>)>, Option<KillBlock>) {
        let mut ok: Vec<(u32, Arc<ProcessInfo>)> = Vec::new();
        let mut why = None;
        for r in self.selected_rows() {
            let pid = r.conn.pid;
            match actions::can_kill(pid) {
                Ok(()) if !ok.iter().any(|(p, _)| *p == pid) => ok.push((pid, r.process.clone())),
                Ok(()) => {}
                Err(b) => why = why.or(Some(b)),
            }
        }
        (ok, why)
    }

    /// Del / the context menu: asks before closing the selected connections.
    pub(in crate::app) fn request_close(&mut self) {
        let (conns, why) = self.closable();
        if conns.is_empty() {
            if let Some(b) = why {
                self.action_result = Some(ActionResult {
                    text: close_block_text(b),
                    error: true,
                });
            }
            return;
        }
        self.pending_action = Some(Pending::Close(conns));
    }

    pub(in crate::app) fn request_kill(&mut self) {
        let (procs, why) = self.killable();
        if procs.is_empty() {
            if let Some(b) = why {
                self.action_result = Some(ActionResult {
                    text: kill_block_text(b),
                    error: true,
                });
            }
            return;
        }
        self.pending_action = Some(Pending::Kill(procs));
    }

    /// End process of one process (the properties window, #28), with the same confirmation.
    pub(in crate::app) fn request_kill_process(&mut self, pid: u32, info: Arc<ProcessInfo>) {
        match actions::can_kill(pid) {
            Ok(()) => self.pending_action = Some(Pending::Kill(vec![(pid, info)])),
            Err(b) => {
                self.action_result = Some(ActionResult {
                    text: kill_block_text(b),
                    error: true,
                })
            }
        }
    }

    /// The reason why the process `pid` cannot be ended, if any.
    pub(in crate::app) fn kill_blocked(pid: u32) -> Option<String> {
        actions::can_kill(pid).err().map(kill_block_text)
    }

    /// The items of the row context menu; the chosen one is run by [`Self::menu_action`].
    pub(in crate::app) fn action_menu_items(&self, ui: &mut egui::Ui) -> Option<MenuAction> {
        let mut chosen = None;
        let (conns, why) = self.closable();
        let close = ui
            .add_enabled(
                !conns.is_empty(),
                egui::Button::new(t!("action.close")).shortcut_text(help::kb("Del")),
            )
            .on_disabled_hover_text(why.map(close_block_text).unwrap_or_default());
        if close.clicked() {
            ui.close();
            chosen = Some(MenuAction::Close);
        }
        let (procs, why) = self.killable();
        let kill = ui
            .add_enabled(!procs.is_empty(), egui::Button::new(t!("action.kill")))
            .on_disabled_hover_text(why.map(kill_block_text).unwrap_or_default());
        if kill.clicked() {
            ui.close();
            chosen = Some(MenuAction::Kill);
        }
        chosen
    }

    pub(in crate::app) fn menu_action(&mut self, a: MenuAction) {
        match a {
            MenuAction::Close => self.request_close(),
            MenuAction::Kill => self.request_kill(),
        }
    }

    /// The confirmation window of a pending action.
    pub(in crate::app) fn confirm_window(&mut self, ctx: &egui::Context) {
        let Some(pending) = &self.pending_action else {
            return;
        };
        let (title, question, button, lines): (String, String, String, Vec<String>) = match pending
        {
            Pending::Close(conns) => (
                t!("action.close"),
                t!("action.close.confirm", count = conns.len()),
                t!("action.close.button"),
                conns
                    .iter()
                    .map(|(c, p)| {
                        let remote = c.remote.map(|r| r.to_string()).unwrap_or_default();
                        format!("{} ({})   {} → {remote}", p.name, c.pid, c.local)
                    })
                    .collect(),
            ),
            Pending::Kill(procs) => (
                t!("action.kill"),
                t!("action.kill.confirm", count = procs.len()),
                t!("action.kill.button"),
                procs
                    .iter()
                    .map(|(pid, p)| {
                        if p.path.is_empty() {
                            format!("{} ({pid})", p.name)
                        } else {
                            format!("{} ({pid})   {}", p.name, p.path)
                        }
                    })
                    .collect(),
            ),
        };
        let mut answer: Option<bool> = None;
        let mut open = true;
        egui::Window::new(title)
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .show(ctx, |ui| {
                ui.label(question);
                ui.add_space(4.0);
                for l in lines.iter().take(LIST_MAX) {
                    ui.monospace(l);
                }
                if lines.len() > LIST_MAX {
                    ui.label(t!("action.more", count = lines.len() - LIST_MAX));
                }
                if !self.elevated && cfg!(windows) {
                    ui.add_space(4.0);
                    ui.label(RichText::new(t!("action.admin_hint")).weak());
                }
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    let ok = ui.button(RichText::new(&button).color(ui.visuals().warn_fg_color));
                    if ok.clicked() {
                        answer = Some(true);
                    }
                    if ui.button(t!("action.cancel")).clicked() {
                        answer = Some(false);
                    }
                });
                // Enter confirms, Esc cancels (also when the buttons have no focus).
                if ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Enter)) {
                    answer = Some(true);
                }
                if ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
                    answer = Some(false);
                }
            });
        if !open {
            answer = Some(false);
        }
        match answer {
            Some(true) => {
                if let Some(p) = self.pending_action.take() {
                    self.run_action(p);
                }
            }
            Some(false) => self.pending_action = None,
            None => {}
        }
    }

    /// Runs a confirmed action (the calls return at once) and refreshes the table, so closed
    /// rows are marked right away.
    fn run_action(&mut self, p: Pending) {
        let (done, errors): (usize, Vec<ActionError>) = match p {
            Pending::Close(conns) => {
                let res: Vec<_> = conns
                    .iter()
                    .map(|(c, _)| actions::close_connection(c))
                    .collect();
                count(res)
            }
            Pending::Kill(procs) => {
                let res: Vec<_> = procs
                    .iter()
                    .map(|(pid, _)| actions::kill_process(*pid))
                    .collect();
                count(res)
            }
        };
        let text = match errors.first() {
            None => t!("action.done", count = done),
            Some(e) => {
                let why = match e {
                    ActionError::AccessDenied => t!("action.access_denied"),
                    ActionError::Other(s) => s.clone(),
                };
                t!(
                    "action.failed",
                    done = done,
                    failed = errors.len(),
                    error = why
                )
            }
        };
        self.action_result = Some(ActionResult {
            text,
            error: !errors.is_empty(),
        });
        self.ctl.refresh_now.store(true, Ordering::Relaxed);
    }

    /// The result in the status bar, with a button to dismiss it.
    pub(in crate::app) fn action_status(&mut self, ui: &mut egui::Ui) {
        let Some(r) = &self.action_result else {
            return;
        };
        ui.separator();
        let color = if r.error {
            ui.visuals().error_fg_color
        } else {
            ui.visuals().text_color()
        };
        ui.colored_label(color, &r.text);
        if ui.small_button("✖").clicked() {
            self.action_result = None;
        }
    }
}

fn count(res: Vec<Result<(), ActionError>>) -> (usize, Vec<ActionError>) {
    let done = res.iter().filter(|r| r.is_ok()).count();
    (done, res.into_iter().filter_map(Result::err).collect())
}
