//! The live graph of the properties window (#30): TCP segments per second of the process
//! (ESTATS counters of #8, administrator only) and its I/O operations per second, for the last
//! two minutes. The rates come from [`crate::rates::Sampler`], fed once per poller snapshot by
//! [`App::sample_properties`]; this file only draws them.

use crate::app::*;
use crate::rates::{Sample, Sampler, WINDOW_MS};
use egui_plot::{Corner, Legend, Line, Plot, PlotPoints};

/// Height of one plot.
const PLOT_HEIGHT: f32 = 110.0;

impl App {
    /// Feeds the sampler of the open properties window with the snapshot just taken (`at` = time
    /// of the snapshot): the TCP counters of the window's connections and the process I/O.
    pub(in crate::app) fn sample_properties(&mut self, at: u64) {
        let Some(p) = &mut self.properties else {
            return;
        };
        if p.exited {
            return;
        }
        let (pid, started) = (p.pid, p.info.started);
        let conns: Vec<_> = self
            .tracker
            .rows()
            .filter(|r| {
                r.conn.pid == pid
                    && r.mark != Mark::Closed
                    && (started.is_none() || r.process.started == started)
            })
            .filter_map(|r| Some(((r.conn.key(), r.conn.created), r.conn.stats?)))
            .collect();
        let io = crate::process::io_counters(pid);
        p.sampler.push(at, conns, io);
    }
}

/// Points of one series: x = seconds before the newest sample (≤ 0).
fn series(samples: &[Sample], value: impl Fn(&Sample) -> Option<f64>) -> PlotPoints<'static> {
    let newest = samples.last().map_or(0, |s| s.at);
    samples
        .iter()
        .filter_map(|s| {
            let x = -((newest - s.at) as f64) / 1000.0;
            value(s).map(|y| [x, y])
        })
        .collect::<Vec<_>>()
        .into()
}

/// A small plot of the last [`WINDOW_MS`] with the given lines.
fn plot(ui: &mut egui::Ui, id: &str, lines: Vec<Line<'static>>) {
    let window = WINDOW_MS as f64 / 1000.0;
    Plot::new(id)
        .height(PLOT_HEIGHT)
        // The newest values are on the right: the legend goes to the old end.
        .legend(Legend::default().position(Corner::LeftTop))
        .allow_drag(false)
        .allow_zoom(false)
        .allow_scroll(false)
        .allow_boxed_zoom(false)
        .allow_double_click_reset(false)
        .include_y(0.0)
        .include_y(1.0)
        .x_axis_formatter(|mark, _| format!("{} s", mark.value.round()))
        .show(ui, |plot_ui| {
            plot_ui.set_plot_bounds_x(-window..=0.0);
            for line in lines {
                plot_ui.line(line);
            }
        });
}

/// The two plots (or the notes why one cannot be drawn).
pub(in crate::app) fn traffic_graph(ui: &mut egui::Ui, sampler: &Sampler, stats_denied: bool) {
    let samples: Vec<Sample> = sampler.samples().iter().copied().collect();
    egui::CollapsingHeader::new(t!("properties.traffic"))
        .id_salt("properties_traffic")
        .default_open(true)
        .show(ui, |ui| {
            if samples.is_empty() {
                ui.label(RichText::new(t!("properties.traffic.waiting")).weak());
                return;
            }
            let latest = samples.last().copied();
            ui.label(RichText::new(t!("properties.traffic.tcp")).strong());
            if stats_denied {
                let note = if cfg!(windows) {
                    t!("properties.traffic.admin_needed")
                } else {
                    t!("properties.traffic.tcp_unsupported")
                };
                ui.label(RichText::new(note).weak());
            } else {
                if let Some(s) = latest {
                    ui.label(tcp_text(&s));
                }
                plot(
                    ui,
                    "properties_tcp_plot",
                    vec![
                        Line::new(
                            t!("properties.traffic.segs_in"),
                            series(&samples, |s| Some(s.tcp.segs_in)),
                        ),
                        Line::new(
                            t!("properties.traffic.segs_out"),
                            series(&samples, |s| Some(s.tcp.segs_out)),
                        ),
                    ],
                );
            }
            ui.add_space(4.0);
            ui.label(RichText::new(t!("properties.traffic.io")).strong());
            match latest.and_then(|s| s.io) {
                None if !samples.iter().any(|s| s.io.is_some()) => {
                    ui.label(RichText::new(t!("properties.traffic.io_denied")).weak());
                }
                io => {
                    if let Some(io) = io {
                        ui.label(t!(
                            "properties.traffic.io_now",
                            read = format!("{:.0}", io.read),
                            write = format!("{:.0}", io.write),
                            other = format!("{:.0}", io.other)
                        ));
                    }
                    plot(
                        ui,
                        "properties_io_plot",
                        vec![
                            Line::new(
                                t!("properties.traffic.io_read"),
                                series(&samples, |s| s.io.map(|i| i.read)),
                            ),
                            Line::new(
                                t!("properties.traffic.io_write"),
                                series(&samples, |s| s.io.map(|i| i.write)),
                            ),
                            Line::new(
                                t!("properties.traffic.io_other"),
                                series(&samples, |s| s.io.map(|i| i.other)),
                            ),
                        ],
                    );
                }
            }
        });
}

/// The newest TCP rates as one line: segments and bytes per second.
fn tcp_text(s: &Sample) -> String {
    t!(
        "properties.traffic.tcp_now",
        segs_in = format!("{:.0}", s.tcp.segs_in),
        segs_out = format!("{:.0}", s.tcp.segs_out),
        bytes_in = crate::columns::rate(s.tcp.bytes_in),
        bytes_out = crate::columns::rate(s.tcp.bytes_out)
    )
}

/// The newest rates for the Copy text of the window.
pub(in crate::app) fn traffic_text(sampler: &Sampler, stats_denied: bool) -> Vec<String> {
    let Some(s) = sampler.latest() else {
        return Vec::new();
    };
    let mut lines = Vec::new();
    if !stats_denied {
        lines.push(format!("{}: {}", t!("properties.traffic.tcp"), tcp_text(s)));
    }
    if let Some(io) = s.io {
        lines.push(format!(
            "{}: {}",
            t!("properties.traffic.io"),
            t!(
                "properties.traffic.io_now",
                read = format!("{:.0}", io.read),
                write = format!("{:.0}", io.write),
                other = format!("{:.0}", io.other)
            )
        ));
    }
    lines
}
