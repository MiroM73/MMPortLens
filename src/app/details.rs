//! The details pane under the table (#7): the connection and the process of the cursor row
//! (or the first selected row), and the process icons of the table.

use crate::app::*;
use crate::process::ProcessInfo;

/// Longest a connection value may stretch its column (points); longer ones are wrapped.
const VALUE_CAP: f32 = 300.0;
/// Space between the columns of the details pane (points, with the item spacing).
const COLUMN_GAP: f32 = 24.0;
/// Least width the process block (right) keeps for its values and the command line.
const PROCESS_MIN: f32 = 340.0;
/// Most columns the connection values are spread over; the process block is one more.
const MAX_CONNECTION_COLUMNS: usize = 3;

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
            .chain(&self.group_samples)
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
                // The connection values are spread over as many columns as fit, each as wide as
                // its values; the process block gets all that is left (#48).
                let groups: Vec<Vec<(String, String)>> = connection_groups(&r)
                    .into_iter()
                    .filter(|g| !g.is_empty())
                    .collect();
                let sizes: Vec<usize> = groups.iter().map(Vec::len).collect();
                let font = egui::TextStyle::Body.resolve(ui.style());
                let name_gap = 12.0;
                let widths: Vec<f32> = groups
                    .iter()
                    .map(|g| {
                        let measure = |text: &str| {
                            ui.painter()
                                .layout_no_wrap(text.to_owned(), font.clone(), egui::Color32::WHITE)
                                .size()
                                .x
                        };
                        let name = g.iter().map(|(n, _)| measure(n)).fold(0.0, f32::max);
                        let value = g.iter().map(|(_, v)| measure(v)).fold(0.0, f32::max);
                        name + name_gap + value.min(VALUE_CAP)
                    })
                    .collect();
                // The scroll bar takes some room.
                let avail = ui.available_width() - 14.0;
                let (split, col_widths) = pick_columns(&sizes, &widths, avail);
                egui::ScrollArea::vertical().show(ui, |ui| {
                    ui.horizontal_top(|ui| {
                        let mut from = 0;
                        let mut used = 0.0;
                        for (i, count) in split.iter().enumerate() {
                            let rows: Vec<&(String, String)> =
                                groups[from..from + count].iter().flatten().collect();
                            from += count;
                            ui.vertical(|ui| {
                                ui.set_width(col_widths[i]);
                                connection_grid(ui, &rows, i);
                            });
                            used += col_widths[i] + COLUMN_GAP;
                            ui.add_space(COLUMN_GAP - ui.spacing().item_spacing.x);
                        }
                        ui.vertical(|ui| {
                            ui.set_width((avail - used).max(PROCESS_MIN));
                            self.process_grid(ui, &r.process, "details_process");
                        });
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

/// The connection values of `r` in logical groups (endpoints, process, times, traffic); empty
/// values are left out.
fn connection_groups(r: &Row) -> [Vec<(String, String)>; 4] {
    let group = |columns: &[ColumnId]| -> Vec<(String, String)> {
        columns
            .iter()
            .filter_map(|&c| {
                // Known ports with their name: `443 (https)` (#34).
                let value = match crate::columns::cell_port(r, c) {
                    Some(p) => crate::columns::port_with_name(p, r.conn.proto),
                    None => cell_text(r, c),
                };
                (!value.is_empty()).then(|| (c.title(), value))
            })
            .collect()
    };
    [
        group(&[
            ColumnId::Protocol,
            ColumnId::State,
            ColumnId::LocalAddress,
            ColumnId::LocalPort,
            ColumnId::LocalHost,
            ColumnId::RemoteAddress,
            ColumnId::RemotePort,
            ColumnId::RemoteHost,
        ]),
        group(&[ColumnId::Service, ColumnId::Pid, ColumnId::Module]),
        group(&[ColumnId::Created, ColumnId::Duration, ColumnId::FirstSeen]),
        group(&[
            ColumnId::BytesSent,
            ColumnId::BytesReceived,
            ColumnId::RateSent,
            ColumnId::RateReceived,
        ]),
    ]
}

fn connection_grid(ui: &mut egui::Ui, rows: &[&(String, String)], index: usize) {
    egui::Grid::new(("details_connection", index))
        .num_columns(2)
        .spacing([12.0, 3.0])
        .show(ui, |ui| {
            for (name, value) in rows {
                detail_row(ui, name, value);
            }
        });
}

/// Chooses the columns of the connection values in a pane of `avail` points: the most columns
/// (at most three) whose natural widths (`widths` per group, the widest group of a column) still
/// leave `PROCESS_MIN` for the process block; at least one. Returns the groups per column (see
/// [`split_groups`]) and the width of each column.
fn pick_columns(sizes: &[usize], widths: &[f32], avail: f32) -> (Vec<usize>, Vec<f32>) {
    let widths_of = |split: &[usize]| -> Vec<f32> {
        let mut from = 0;
        split
            .iter()
            .map(|&n| {
                let w = widths[from..from + n].iter().copied().fold(0.0, f32::max);
                from += n;
                w
            })
            .collect()
    };
    let mut chosen = None;
    for n in (1..=MAX_CONNECTION_COLUMNS).rev() {
        let split = split_groups(sizes, n);
        let w = widths_of(&split);
        let total: f32 = w.iter().map(|w| w + COLUMN_GAP).sum();
        if total + PROCESS_MIN <= avail || n == 1 {
            chosen = Some((split, w));
            break;
        }
    }
    let (split, mut w) = chosen.unwrap_or_default();
    // A single column that does not fit leaves the process block half of the pane at most.
    if w.len() == 1 {
        w[0] = w[0].min((avail / 2.0 - COLUMN_GAP).max(120.0));
    }
    (split, w)
}

/// Splits the groups of the given `sizes` (rows) into at most `n` columns of consecutive groups
/// so that the tallest column is as low as possible (a group is never split); returns the
/// number of groups of each column.
fn split_groups(sizes: &[usize], n: usize) -> Vec<usize> {
    if sizes.is_empty() {
        return Vec::new();
    }
    best_split(sizes, n.clamp(1, sizes.len())).1
}

/// The (height of the tallest column, groups per column) of the best split into `n` columns.
fn best_split(sizes: &[usize], n: usize) -> (usize, Vec<usize>) {
    if n <= 1 || sizes.len() <= 1 {
        return (sizes.iter().sum(), vec![sizes.len()]);
    }
    let mut best: Option<(usize, Vec<usize>)> = None;
    for first in 1..=sizes.len() - (n - 1) {
        let head: usize = sizes[..first].iter().sum();
        let (rest_height, rest) = best_split(&sizes[first..], n - 1);
        let height = head.max(rest_height);
        if best.as_ref().is_none_or(|b| height < b.0) {
            let mut parts = vec![first];
            parts.extend(rest);
            best = Some((height, parts));
        }
    }
    best.unwrap_or_default()
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn columns_are_as_wide_as_their_values() {
        let sizes = [8, 3, 3, 4];
        let widths = [200.0, 120.0, 150.0, 100.0];
        // Wide: three columns, each as wide as its widest group (8 | 3 + 3 | 4).
        let (split, w) = pick_columns(&sizes, &widths, 1500.0);
        assert_eq!(split, vec![1, 2, 1]);
        assert_eq!(w, vec![200.0, 150.0, 100.0]);
        // 200 + 150 + 2 gaps + 340 does not fit in 700, two columns (8 | 3 + 3 + 4) do not either
        // (200 + 150 + 2 gaps + 340 = 738), so one column.
        let (split, w) = pick_columns(&sizes, &widths, 700.0);
        assert_eq!(split, vec![4]);
        assert_eq!(w, vec![200.0]);
    }

    #[test]
    fn a_column_never_takes_more_than_half_of_a_narrow_pane() {
        let (_, w) = pick_columns(&[3], &[500.0], 400.0);
        assert_eq!(w, vec![176.0]);
    }

    #[test]
    fn groups_are_kept_whole_and_the_columns_balanced() {
        // endpoints 8, process 3, times 3, traffic 4
        let sizes = [8, 3, 3, 4];
        assert_eq!(split_groups(&sizes, 1), vec![4]);
        assert_eq!(split_groups(&sizes, 2), vec![1, 3]); // 8 | 10
        assert_eq!(split_groups(&sizes, 3), vec![1, 2, 1]); // 8 | 6 | 4
        assert_eq!(split_groups(&sizes, 9), vec![1, 1, 1, 1]); // not more columns than groups
    }

    #[test]
    fn few_groups_and_no_groups() {
        assert_eq!(split_groups(&[5], 3), vec![1]);
        assert_eq!(split_groups(&[2, 2], 3), vec![1, 1]);
        assert!(split_groups(&[], 3).is_empty());
    }
}
