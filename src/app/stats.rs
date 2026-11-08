//! View → Statistics (#19): a chart of the established / listening / time-wait counts of the last
//! ten minutes and the top processes (connections, traffic), remote hosts and remote ports of the
//! live table. A click on an entry sets it as the column filter of the main table. The window is a
//! zoomable tool window of `native_window.rs` (own OS window, own text zoom); its content scrolls
//! in both directions. The data and the lists are `crate::stats`.

use crate::app::native_window::ToolSpec;
use crate::app::*;
use crate::stats::{HISTORY_MS, Sample, TOP_N, TopItem, TopKind};
use egui_plot::{Corner, Legend, Line, Plot, PlotPoints};

/// Height of the chart (points, before the text zoom).
const CHART_HEIGHT: f32 = 220.0;
/// Width of one top list (points, before the text zoom); the lists stand side by side and the
/// window scrolls horizontally when they do not fit.
const LIST_WIDTH: f32 = 250.0;
/// Longer values are shortened in the lists (the tooltip has them whole).
const MAX_LABEL: usize = 34;

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
            TOP_N,
        ));
        let traffic = plain(crate::stats::top(
            rows.iter().copied(),
            TopKind::Traffic,
            TOP_N,
        ));
        let has_rates = rows
            .iter()
            .any(|r| r.rate_in.is_some() || r.rate_out.is_some());
        let host_items = crate::stats::top(rows.iter().copied(), TopKind::RemoteHosts, TOP_N);
        let port_items = crate::stats::top(rows.iter().copied(), TopKind::RemotePorts, TOP_N);
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
            ui.horizontal_top(|ui| {
                let lists: [(&str, &[Entry], TopKind); 4] = [
                    ("stats.top.processes", &data.processes, TopKind::Processes),
                    ("stats.top.traffic", &data.traffic, TopKind::Traffic),
                    ("stats.top.hosts", &data.hosts, TopKind::RemoteHosts),
                    ("stats.top.ports", &data.ports, TopKind::RemotePorts),
                ];
                for (title, entries, kind) in lists {
                    ui.vertical(|ui| {
                        ui.set_width(LIST_WIDTH * zoom);
                        ui.label(RichText::new(t!(title)).strong());
                        if kind == TopKind::Traffic && !data.has_rates {
                            ui.add(
                                egui::Label::new(RichText::new(t!("stats.traffic.off")).weak())
                                    .wrap(),
                            );
                        } else if entries.is_empty() {
                            ui.label(RichText::new(t!("stats.none")).weak());
                        } else if let Some(c) = top_list(ui, title, entries, kind) {
                            clicked = Some(c);
                        }
                    });
                    ui.add_space(12.0);
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

/// One top list as a grid: value (a link that filters), count, traffic for processes.
fn top_list(
    ui: &mut egui::Ui,
    id: &str,
    entries: &[Entry],
    kind: TopKind,
) -> Option<(ColumnId, String)> {
    let mut clicked = None;
    let col = kind.column();
    let with_rate = matches!(kind, TopKind::Processes | TopKind::Traffic)
        && entries.iter().any(|e| e.item.rate.is_some());
    egui::Grid::new(id)
        .num_columns(if with_rate { 3 } else { 2 })
        .striped(true)
        .spacing([10.0, 3.0])
        .show(ui, |ui| {
            for e in entries {
                let text = shorten(&e.label, MAX_LABEL);
                let tip = t!(
                    "stats.filter.tip",
                    value = &e.label,
                    column = col.title(),
                    filter = e.item.filter()
                );
                if ui.link(text).on_hover_text(tip).clicked() {
                    clicked = Some((col, e.item.filter()));
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(e.item.count.to_string())
                        .on_hover_text(t!("stats.count.tip", count = e.item.count));
                });
                if with_rate {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(e.item.rate.map(crate::columns::rate).unwrap_or_default());
                    });
                }
                ui.end_row();
            }
        });
    clicked
}

/// `text` cut to `max` characters with an ellipsis.
fn shorten(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        text.to_owned()
    } else {
        let mut s: String = text.chars().take(max.saturating_sub(1)).collect();
        s.push('…');
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shortened_labels() {
        assert_eq!(shorten("chrome.exe", 34), "chrome.exe");
        assert_eq!(shorten("abcdef", 4), "abc…");
        assert_eq!(shorten("čšžťďň", 3), "čš…");
    }
}
