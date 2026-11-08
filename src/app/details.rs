//! The details pane under the table (#7): the connection and the process of the cursor row
//! (or the first selected row), and the process icons of the table.

use crate::app::*;
use crate::process::ProcessInfo;

/// Tooltip of the Process cell: description, company, services, command line.
pub(in crate::app) fn process_tip(p: &ProcessInfo) -> String {
    let mut lines: Vec<String> = Vec::new();
    match (p.description.is_empty(), p.company.is_empty()) {
        (false, false) => lines.push(format!("{} – {}", p.description, p.company)),
        (false, true) => lines.push(p.description.clone()),
        (true, false) => lines.push(p.company.clone()),
        (true, true) => {}
    }
    if !p.services.is_empty() {
        lines.push(t!("details.services_line", services = p.service_list()));
    }
    if !p.command_line.is_empty() {
        lines.push(p.command_line.clone());
    }
    lines.join("\n")
}

impl App {
    /// The texture of a process icon (created by [`Self::load_icon_textures`]).
    pub(in crate::app) fn icon_texture(&self, p: &ProcessInfo) -> Option<&egui::TextureHandle> {
        p.icon.as_ref()?;
        self.icons.get(&p.path)
    }

    /// Creates the textures of the icons of the rows shown that have none yet, once per
    /// executable path (before the table borrows `self`).
    pub(in crate::app) fn load_icon_textures(&mut self, ctx: &egui::Context) {
        let paths: Vec<(String, Arc<crate::process::Icon>)> = self
            .view
            .iter()
            .filter_map(|r| Some((r.process.path.clone(), r.process.icon.clone()?)))
            .filter(|(p, _)| !self.icons.contains_key(p))
            .collect();
        for (path, icon) in paths {
            let image = egui::ColorImage::from_rgba_unmultiplied(icon.size, &icon.rgba);
            let tex = ctx.load_texture(format!("icon:{path}"), image, egui::TextureOptions::LINEAR);
            self.icons.insert(path, tex);
        }
    }

    /// The row shown in the details pane: the cursor row when it is selected and shown, else
    /// the first selected row shown.
    pub(in crate::app) fn details_row(&self) -> Option<&Row> {
        let keys = self.view_keys();
        if let Some(c) = self.sel.cursor()
            && self.sel.contains(&c)
            && let Some(i) = keys.iter().position(|k| *k == c)
        {
            return Some(&self.view[i]);
        }
        self.sel.positions(&keys).first().map(|&i| &self.view[i])
    }

    pub(in crate::app) fn details_pane(&mut self, ui: &mut egui::Ui) {
        if !self.cfg.show_details {
            return;
        }
        let Some(r) = self.details_row().cloned() else {
            return;
        };
        egui::Panel::bottom("details")
            .resizable(true)
            .default_size(170.0)
            .min_size(80.0)
            .show(ui, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    ui.columns(2, |cols| {
                        connection_grid(&mut cols[0], &r);
                        self.process_grid(&mut cols[1], &r.process, "details_process");
                    });
                });
            });
    }

    /// Icon, name, description and the process values (details pane, properties window #28);
    /// `id` keeps the grids apart when both are on screen.
    pub(in crate::app) fn process_grid(&self, ui: &mut egui::Ui, p: &ProcessInfo, id: &str) {
        ui.horizontal(|ui| {
            if let Some(t) = self.icon_texture(p) {
                ui.add(egui::Image::new(t).fit_to_exact_size(egui::vec2(32.0, 32.0)));
            }
            ui.vertical(|ui| {
                ui.strong(&p.name);
                if !p.description.is_empty() {
                    ui.label(&p.description);
                }
            });
        });
        egui::Grid::new(id)
            .num_columns(2)
            .spacing([12.0, 3.0])
            .show(ui, |ui| {
                for (name, value) in process_values(p) {
                    detail_row(ui, &name, &value);
                }
            });
        if self.no_access_hint(p) {
            ui.label(RichText::new(t!("details.no_access")).weak());
        }
    }

    /// The details of `p` cannot be read without administrator rights.
    pub(in crate::app) fn no_access_hint(&self, p: &ProcessInfo) -> bool {
        p.path.is_empty() && !self.elevated && cfg!(windows)
    }
}

/// The (name, value) lines of a process below its name and description; empty values are kept
/// ([`detail_row`] leaves them out).
pub(in crate::app) fn process_values(p: &ProcessInfo) -> Vec<(String, String)> {
    let started = p.started.map(crate::timefmt::date_time).unwrap_or_default();
    let mut v: Vec<(String, String)> = [
        ("column.process_path", p.path.clone()),
        ("column.company", p.company.clone()),
        ("column.user", p.user.clone()),
        ("details.started", started),
        ("column.command_line", p.command_line.clone()),
    ]
    .into_iter()
    .map(|(k, value)| (t!(k), value))
    .collect();
    if !p.services.is_empty() {
        let list: Vec<String> = p
            .services
            .iter()
            .map(|(n, d)| format!("{n} ({d})"))
            .collect();
        v.push((t!("column.win_services"), list.join("\n")));
    }
    v
}

fn connection_grid(ui: &mut egui::Ui, r: &Row) {
    egui::Grid::new("details_connection")
        .num_columns(2)
        .spacing([12.0, 3.0])
        .show(ui, |ui| {
            for c in [
                ColumnId::Protocol,
                ColumnId::State,
                ColumnId::LocalAddress,
                ColumnId::LocalPort,
                ColumnId::LocalHost,
                ColumnId::RemoteAddress,
                ColumnId::RemotePort,
                ColumnId::RemoteHost,
                ColumnId::Service,
                ColumnId::Pid,
                ColumnId::Module,
                ColumnId::Created,
                ColumnId::Duration,
                ColumnId::FirstSeen,
                ColumnId::BytesSent,
                ColumnId::BytesReceived,
                ColumnId::RateSent,
                ColumnId::RateReceived,
            ] {
                detail_row(ui, &c.title(), &cell_text(r, c));
            }
        });
}

/// One "name: value" line; empty values are left out. The value can be selected and copied.
pub(in crate::app) fn detail_row(ui: &mut egui::Ui, name: &str, value: &str) {
    if value.is_empty() {
        return;
    }
    ui.label(RichText::new(name).weak());
    ui.add(egui::Label::new(value).selectable(true).wrap());
    ui.end_row();
}
