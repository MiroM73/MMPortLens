//! View → Statistics (#19): a chart of the established / listening / time-wait counts of the last
//! ten minutes and the top processes (connections, traffic), remote hosts and remote ports of the
//! live table. A click on an entry sets it as the column filter of the main table. The window is a
//! zoomable tool window of `native_window.rs` (own OS window, own text zoom); its content scrolls
//! in both directions. The data and the lists are `crate::stats`.

use crate::app::native_window::ToolSpec;
use crate::app::*;
use crate::stats::{HISTORY_MS, Sample, TOP_N, TopItem, TopKind};
use egui::TextWrapMode;
use egui_plot::{Corner, Legend, Line, Plot, PlotPoints};

/// Height of the chart (points, before the text zoom).
const CHART_HEIGHT: f32 = 220.0;
/// Minimum width of one top list (points, before the text zoom); the lists stand side by side,
/// share a wider window evenly and the window scrolls horizontally when they do not fit.
const LIST_WIDTH: f32 = 250.0;
/// Space between two lists (points).
const LIST_GAP: f32 = 12.0;
/// Space between the cells of a list row (points).
const CELL_GAP: f32 = 10.0;
/// Most rows of a list that are computed; a taller window shows more of them (up to this), a
/// small one at least `TOP_N`. Longer labels are cut by the list width (the tooltip has them whole).
const MAX_ROWS: usize = 40;

/// One entry as shown: the item and its label (a host name, a port with its service name).
struct Entry {
    item: TopItem,
    label: String,
}

/// What the window shows, computed once per refresh while it is open.
#[derive(Default)]
pub(in crate::app) struct StatsData {
    /// The refresh it was computed for.
    computed_for: Option<u64>,
    samples: Vec<Sample>,
    processes: Vec<Entry>,
    traffic: Vec<Entry>,
    hosts: Vec<Entry>,
    ports: Vec<Entry>,
    /// Some endpoint has a traffic rate (a traffic column is shown, #8).
    has_rates: bool,
}

impl App {
    /// Adds the counts of the snapshot just merged into the tracker to the chart history (also
    /// while the window is closed, so the chart has data when it opens – one counting pass).
    pub(in crate::app) fn sample_stats(&mut self, at: u64) {
        let s = crate::stats::sample(self.tracker.rows(), at);
        self.stats_history.push(s);
    }

    /// View → Statistics.
    pub(in crate::app) fn open_stats(&mut self) {
        self.stats_win.open();
    }

    /// The data of the window, recomputed when a new refresh arrived.
    fn refresh_stats_data(&mut self) {
        if self.stats_data.computed_for == self.last_poll && self.last_poll.is_some() {
            return;
        }
        let rows: Vec<&Row> = self.tracker.rows().collect();
        let now = now_ms();
        let resolve = self.cfg.resolve_addresses;
        let plain = |list: Vec<TopItem>| -> Vec<Entry> {
            list.into_iter()
                .map(|item| Entry {
                    label: item.key.clone(),
                    item,
                })
                .collect()
        };
        let processes = plain(crate::stats::top(
            rows.iter().copied(),
            TopKind::Processes,
            MAX_ROWS,
        ));
        let traffic = plain(crate::stats::top(
            rows.iter().copied(),
            TopKind::Traffic,
            MAX_ROWS,
        ));
        let has_rates = rows
            .iter()
            .any(|r| r.rate_in.is_some() || r.rate_out.is_some());
        let host_items = crate::stats::top(rows.iter().copied(), TopKind::RemoteHosts, MAX_ROWS);
        let port_items = crate::stats::top(rows.iter().copied(), TopKind::RemotePorts, MAX_ROWS);
        drop(rows);
        let hosts = host_items
            .into_iter()
            .map(|item| {
                let name = resolve
                    .then(|| item.key.parse().ok())
                    .flatten()
                    .and_then(|ip| self.resolver.name(ip, now));
                let label = match name {
                    Some(n) if *n != *item.key => format!("{n} ({})", item.key),
                    _ => item.key.clone(),
                };
                Entry { item, label }
            })
            .collect();
        let ports = port_items
            .into_iter()
            .map(|item| {
                let label = item.key.parse::<u16>().map_or(item.key.clone(), |p| {
                    crate::columns::port_with_name(p, crate::net::Protocol::Tcp)
                });
                Entry { item, label }
            })
            .collect();
        self.stats_data = StatsData {
            computed_for: self.last_poll,
            samples: self.stats_history.samples().iter().copied().collect(),
            processes,
            traffic,
            hosts,
            ports,
            has_rates,
        };
    }

    /// Draws the statistics window when it is open and applies a clicked entry as a filter.
    pub(in crate::app) fn stats_window(&mut self, ctx: &egui::Context) {
        if !self.stats_win.is_open() {
            return;
        }
        self.refresh_stats_data();
        let spec = ToolSpec {
            title: t!("stats.title"),
            default_size: [1120.0, 640.0],
            min_size: [420.0, 300.0],
        };
        let native = self.cfg.separate_windows && !ctx.embed_viewports();
        let mut state = self.cfg.stats_window.clone();
        let zoom = state.zoom;
        let replaying = self.replaying();
        let data = &self.stats_data;
        let mut clicked: Option<(ColumnId, String)> = None;
        self.stats_win.show(ctx, native, &spec, &mut state, |ui| {
            clicked = stats_ui(ui, data, zoom, replaying);
        });
        if state != self.cfg.stats_window {
            self.cfg.stats_window = state;
            self.settings_changed();
        }
        if let Some((col, filter)) = clicked {
            self.filter_from_stats(col, filter);
        }
    }

    /// Sets the filter of one column of the main table (the other filters stay); the column and
    /// the filter row are shown, because the filters of hidden columns do not apply.
    fn filter_from_stats(&mut self, col: ColumnId, filter: String) {
        self.cfg.columns.set_visible(col, true);
        self.cfg.show_column_filters = true;
        self.col_filter_text.insert(col, filter);
        self.apply_column_filters();
        self.settings_changed();
    }
}

/// The content of the window; returns the column filter of a clicked entry.
fn stats_ui(
    ui: &mut egui::Ui,
    data: &StatsData,
    zoom: f32,
    replaying: bool,
) -> Option<(ColumnId, String)> {
    let mut clicked = None;
    // The visible width before the scroll area (inside it the width is unbounded).
    let visible = ui.available_width();
    let visible_height = ui.available_height();
    egui::ScrollArea::both()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.label(RichText::new(t!("stats.chart", minutes = HISTORY_MS / 60_000)).heading());
            if let Some(s) = data.samples.last() {
                ui.label(t!(
                    "stats.now",
                    established = s.established,
                    listening = s.listening,
                    time_wait = s.time_wait,
                    total = s.total
                ));
            }
            if replaying {
                ui.label(RichText::new(t!("stats.live_note")).weak());
            }
            if data.samples.len() < 2 {
                ui.label(RichText::new(t!("stats.waiting")).weak());
            } else {
                chart(
                    ui,
                    &data.samples,
                    (visible - 16.0).max(LIST_WIDTH * zoom),
                    zoom,
                );
            }
            ui.add_space(8.0);
            // Share the width evenly; the rows fill the height that is left (the title above and
            // the hint below the lists take a row each).
            let gap = LIST_GAP + ui.spacing().item_spacing.x;
            let list_width = list_width(visible - 16.0, LIST_WIDTH * zoom, gap, 4);
            let row_height = ui.spacing().interact_size.y + 3.0;
            let rows = rows_that_fit(
                visible_height - ui.min_rect().height() - 2.0 * row_height - 14.0,
                row_height,
                TOP_N,
                MAX_ROWS,
            );
            ui.horizontal_top(|ui| {
                let lists: [(&str, &[Entry], TopKind); 4] = [
                    ("stats.top.processes", &data.processes, TopKind::Processes),
                    ("stats.top.traffic", &data.traffic, TopKind::Traffic),
                    ("stats.top.hosts", &data.hosts, TopKind::RemoteHosts),
                    ("stats.top.ports", &data.ports, TopKind::RemotePorts),
                ];
                for (i, (title, entries, kind)) in lists.into_iter().enumerate() {
                    ui.vertical(|ui| {
                        ui.set_width(list_width);
                        ui.label(RichText::new(t!(title)).strong());
                        if kind == TopKind::Traffic && !data.has_rates {
                            ui.add(
                                egui::Label::new(RichText::new(t!("stats.traffic.off")).weak())
                                    .wrap(),
                            );
                        } else if entries.is_empty() {
                            ui.label(RichText::new(t!("stats.none")).weak());
                        } else if let Some(c) = top_list(
                            ui,
                            title,
                            &entries[..rows.min(entries.len())],
                            kind,
                            list_width,
                        ) {
                            clicked = Some(c);
                        }
                    });
                    if i + 1 < 4 {
                        ui.add_space(LIST_GAP);
                    }
                }
            });
            ui.add_space(6.0);
            ui.label(RichText::new(t!("stats.click_hint")).weak());
        });
    clicked
}

/// The chart of the three state counts, x = minutes before the newest sample.
fn chart(ui: &mut egui::Ui, samples: &[Sample], width: f32, zoom: f32) {
    let newest = samples.last().map_or(0, |s| s.at);
    let series = |value: fn(&Sample) -> u32| -> PlotPoints<'static> {
        samples
            .iter()
            .map(|s| {
                let x = -((newest - s.at) as f64) / 60_000.0;
                [x, value(s) as f64]
            })
            .collect::<Vec<_>>()
            .into()
    };
    let window = HISTORY_MS as f64 / 60_000.0;
    Plot::new("stats_chart")
        .height(CHART_HEIGHT * zoom)
        .width(width)
        .legend(Legend::default().position(Corner::LeftTop))
        // Ctrl + wheel zooms the text of the window, not the chart.
        .allow_drag(false)
        .allow_zoom(false)
        .allow_scroll(false)
        .allow_boxed_zoom(false)
        .allow_double_click_reset(false)
        .include_y(0.0)
        .include_y(1.0)
        .x_axis_formatter(|mark, _| {
            t!(
                "stats.axis.minutes",
                value = format!("{:.0}", mark.value.round())
            )
        })
        .show(ui, |plot_ui| {
            plot_ui.set_plot_bounds_x(-window..=0.0);
            plot_ui.line(Line::new(
                t!("stats.series.established"),
                series(|s| s.established),
            ));
            plot_ui.line(Line::new(
                t!("stats.series.listening"),
                series(|s| s.listening),
            ));
            plot_ui.line(Line::new(
                t!("stats.series.time_wait"),
                series(|s| s.time_wait),
            ));
        });
}

/// One top list as a grid: value (a link that filters), count, traffic for processes. The number
/// columns have a fixed width and the value gets the rest of `width` (cut with an ellipsis), so the
/// lists never run into each other and their numbers line up.
fn top_list(
    ui: &mut egui::Ui,
    id: &str,
    entries: &[Entry],
    kind: TopKind,
    width: f32,
) -> Option<(ColumnId, String)> {
    let mut clicked = None;
    let col = kind.column();
    let with_rate = matches!(kind, TopKind::Processes | TopKind::Traffic)
        && entries.iter().any(|e| e.item.rate.is_some());
    // Room for "00000" and "1234.5 MB/s" in the current text size.
    let em = ui.text_style_height(&egui::TextStyle::Body);
    let count_width = em * 3.0;
    let rate_width = em * 6.0;
    let numbers = [count_width, rate_width];
    let label_width = label_width(width, &numbers[..if with_rate { 2 } else { 1 }], CELL_GAP);
    egui::Grid::new(id)
        .num_columns(if with_rate { 3 } else { 2 })
        .striped(true)
        .spacing([CELL_GAP, 3.0])
        .show(ui, |ui| {
            for e in entries {
                let tip = t!(
                    "stats.filter.tip",
                    value = &e.label,
                    column = col.title(),
                    filter = e.item.filter()
                );
                let link = ui.scope(|ui| {
                    ui.set_width(label_width);
                    ui.style_mut().wrap_mode = Some(TextWrapMode::Truncate);
                    ui.link(&e.label).on_hover_text(tip)
                });
                if link.inner.clicked() {
                    clicked = Some((col, e.item.filter()));
                }
                right_cell(ui, count_width, |ui| {
                    ui.label(e.item.count.to_string())
                        .on_hover_text(t!("stats.count.tip", count = e.item.count));
                });
                if with_rate {
                    right_cell(ui, rate_width, |ui| {
                        ui.label(e.item.rate.map(crate::columns::rate).unwrap_or_default());
                    });
                }
                ui.end_row();
            }
        });
    clicked
}

/// A cell of `width` whose content is aligned to the right.
fn right_cell(ui: &mut egui::Ui, width: f32, content: impl FnOnce(&mut egui::Ui)) {
    ui.allocate_ui_with_layout(
        egui::vec2(width, ui.spacing().interact_size.y),
        egui::Layout::right_to_left(egui::Align::Center),
        content,
    );
}

/// The width of each of `n` lists that share `total` with `gap` between them, at least `min`.
fn list_width(total: f32, min: f32, gap: f32, n: usize) -> f32 {
    ((total - gap * n.saturating_sub(1) as f32) / n.max(1) as f32).max(min)
}

/// What is left of a list of `width` for the value after the fixed `numbers` columns and the
/// gaps between all columns.
fn label_width(width: f32, numbers: &[f32], gap: f32) -> f32 {
    (width - numbers.iter().sum::<f32>() - gap * numbers.len() as f32).max(40.0)
}

/// How many rows of `row_height` fit in `remaining`, between `min` (a small window scrolls) and
/// `max` (rows that exist).
fn rows_that_fit(remaining: f32, row_height: f32, min: usize, max: usize) -> usize {
    if remaining <= 0.0 || row_height <= 0.0 {
        return min;
    }
    ((remaining / row_height) as usize).clamp(min, max)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lists_share_the_width_but_keep_the_minimum() {
        // 4 lists, 3 gaps of 12: (1048 - 36) / 4
        assert_eq!(list_width(1048.0, 250.0, 12.0, 4), 253.0);
        assert_eq!(list_width(2000.0, 250.0, 12.0, 4), 491.0);
        assert_eq!(list_width(600.0, 250.0, 12.0, 4), 250.0);
    }

    #[test]
    fn the_value_gets_what_the_numbers_leave() {
        assert_eq!(label_width(250.0, &[30.0], 10.0), 210.0);
        assert_eq!(label_width(250.0, &[30.0, 60.0], 10.0), 140.0);
        assert_eq!(label_width(50.0, &[30.0, 60.0], 10.0), 40.0);
    }

    #[test]
    fn rows_follow_the_height() {
        assert_eq!(rows_that_fit(100.0, 20.0, 10, 40), 10);
        assert_eq!(rows_that_fit(500.0, 20.0, 10, 40), 25);
        assert_eq!(rows_that_fit(5000.0, 20.0, 10, 40), 40);
        assert_eq!(rows_that_fit(-5.0, 20.0, 10, 40), 10);
    }
}
