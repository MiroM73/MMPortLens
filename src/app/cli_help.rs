//! Help → Command line (#38): the options, column names, exit codes and examples of the command
//! line (#16), drawn from the structured help of `clihelp`. [`cli_help_ui`] is also the content of
//! the `--help` window (#39). The window itself is a zoomable tool window of `native_window.rs`
//! (#42).

use crate::app::native_window::ToolSpec;
use crate::app::*;
use crate::clihelp::CliHelp;

/// How long the "Copied" line stays after a click on a copy button.
const COPIED_FOR: Duration = Duration::from_secs(4);

/// Draws the whole help in a vertical scroll area: sections, a monospace options grid, the
/// column names and the examples with copy buttons.
pub fn cli_help_ui(ui: &mut egui::Ui, help: &CliHelp) {
    let copied_id = egui::Id::unique("cli_help_copied");
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            let section = |ui: &mut egui::Ui, key: &str| {
                ui.add_space(8.0);
                ui.label(RichText::new(t!(key)).heading().strong());
                ui.add_space(2.0);
            };
            section(ui, "cli.section.usage");
            // The heading says "Usage" already: the line without its "Usage: " prefix.
            let usage = help
                .usage
                .split_once(": ")
                .map_or(help.usage.as_str(), |(_, u)| u);
            ui.monospace(usage);
            ui.label(&help.file);

            section(ui, "cli.section.options");
            egui::Grid::new("cli_help_options")
                .num_columns(2)
                .spacing([16.0, 4.0])
                .striped(true)
                .show(ui, |ui| {
                    for o in &help.options {
                        ui.label(RichText::new(&o.option).monospace().strong());
                        ui.add(egui::Label::new(&o.description).wrap());
                        ui.end_row();
                    }
                });

            section(ui, "cli.section.columns");
            ui.label(t!("cli.columns.note"));
            egui::Grid::new("cli_help_columns")
                .num_columns(2)
                .spacing([16.0, 2.0])
                .striped(true)
                .show(ui, |ui| {
                    ui.label(RichText::new(t!("cli.column.name")).strong());
                    ui.label(RichText::new(t!("cli.column.title")).strong());
                    ui.end_row();
                    for (name, title) in &help.columns {
                        ui.monospace(name);
                        ui.label(title);
                        ui.end_row();
                    }
                });

            section(ui, "cli.section.exit");
            ui.label(&help.exit);
            ui.label(&help.wait);

            section(ui, "cli.section.examples");
            let copied: Option<(String, Instant)> = ui.data(|d| d.get_temp(copied_id));
            let recent = copied.filter(|(_, at)| at.elapsed() < COPIED_FOR);
            for e in &help.examples {
                ui.add_space(4.0);
                ui.label(RichText::new(&e.what).weak());
                ui.horizontal(|ui| {
                    // The button of the command copied last shows a check mark for a while.
                    let done = recent.as_ref().is_some_and(|(c, _)| *c == e.command);
                    let (icon, tip) = if done {
                        ("✔", t!("cli.copied", command = &e.command))
                    } else {
                        ("📋", t!("cli.copy.tip"))
                    };
                    if ui.small_button(icon).on_hover_text(tip).clicked() {
                        ui.ctx().copy_text(e.command.clone());
                        ui.data_mut(|d| {
                            d.insert_temp(copied_id, (e.command.clone(), Instant::now()))
                        });
                        ui.ctx().request_repaint_after(COPIED_FOR);
                    }
                    ui.add(egui::Label::new(RichText::new(&e.command).monospace()).wrap());
                });
            }
            if let Some((_, at)) = &recent {
                ui.ctx()
                    .request_repaint_after(COPIED_FOR.saturating_sub(at.elapsed()));
            }
            ui.add_space(8.0);
        });
}

impl App {
    /// The Help → Command line window: its own OS window (or inside the main window, #42) with
    /// its own text zoom. The help is built every frame, so a language switch translates it at
    /// once.
    pub(in crate::app) fn cli_help_window(&mut self, ctx: &egui::Context) {
        if !self.cli_help.is_open() {
            return;
        }
        let languages: Vec<String> = i18n::available().into_iter().map(|l| l.code).collect();
        let help = crate::clihelp::build(&languages.join(", "));
        let spec = ToolSpec {
            title: t!("cli.window.title"),
            default_size: [760.0, 560.0],
            min_size: [520.0, 320.0],
        };
        let native = self.cfg.separate_windows && !ctx.embed_viewports();
        let mut state = self.cfg.cli_help_window.clone();
        self.cli_help
            .show(ctx, native, &spec, &mut state, |ui| cli_help_ui(ui, &help));
        if state != self.cfg.cli_help_window {
            self.cfg.cli_help_window = state;
            self.settings_changed();
        }
    }
}
