//! The event list of an opened recording (#3): every event with its time, filtered by a text
//! and by the event kinds, oldest or newest first. Only the pages on screen are asked from the
//! replay worker, so a long recording is never loaded into memory. A click on an event moves the
//! replay slider to its time.

use crate::app::*;
use crate::recording::db::EventQuery;
use crate::recording::threads::{PAGE, Request};
use crate::recording::{Event, EventKind};
use egui_extras::{Column, TableBuilder};
use std::collections::{HashMap, HashSet};

/// The pages of the current query that are loaded or asked for.
#[derive(Default)]
pub(in crate::app) struct EventList {
    query: EventQuery,
    generation: u64,
    total: Option<u64>,
    pages: HashMap<u64, Vec<Event>>,
    asked: HashSet<u64>,
    /// Position of the clicked event.
    selected: Option<u64>,
}

/// Pages kept around the visible one; others are dropped.
const KEEP_PAGES: u64 = 4;

impl EventList {
    /// A new query (or a newly opened file): everything is asked again.
    pub(in crate::app) fn restart(&mut self) {
        self.generation += 1;
        self.total = None;
        self.pages.clear();
        self.asked.clear();
        self.selected = None;
    }

    pub(in crate::app) fn receive(
        &mut self,
        generation: u64,
        page: u64,
        total: u64,
        events: Vec<Event>,
    ) {
        if generation != self.generation {
            return;
        }
        self.total = Some(total);
        self.pages.insert(page, events);
    }

    fn get(&self, pos: u64) -> Option<&Event> {
        self.pages.get(&(pos / PAGE))?.get((pos % PAGE) as usize)
    }

    /// The requests for the pages of the rows `first..=last` that are missing.
    fn missing(&mut self, first: u64, last: u64) -> Vec<Request> {
        let (a, b) = (first / PAGE, last / PAGE);
        self.pages
            .retain(|p, _| *p + KEEP_PAGES >= a && *p <= b + KEEP_PAGES);
        let pages = &self.pages;
        self.asked
            .retain(|p| pages.contains_key(p) || (*p + KEEP_PAGES >= a && *p <= b));
        let mut out = Vec::new();
        for p in a..=b {
            if !self.pages.contains_key(&p) && self.asked.insert(p) {
                out.push(Request::Events {
                    generation: self.generation,
                    query: self.query.clone(),
                    page: p,
                });
            }
        }
        out
    }
}

fn kind_color(k: EventKind, dark: bool) -> Option<Color32> {
    let mark = match k {
        EventKind::Present => return None,
        EventKind::New => Mark::New,
        EventKind::Closed => Mark::Closed,
        EventKind::Changed => Mark::Changed,
    };
    mark_color(mark, dark)
}

fn addr(a: std::net::SocketAddr) -> String {
    match a {
        std::net::SocketAddr::V4(v) => v.to_string(),
        std::net::SocketAddr::V6(v) => format!("[{}]:{}", v.ip(), v.port()),
    }
}

/// The texts of the columns of an event.
fn event_cells(e: &Event, first: u64) -> [String; 7] {
    let state = |s: Option<crate::net::TcpState>| s.map(crate::columns::state_text);
    let states = match (e.kind, state(e.state_before), state(e.conn.state)) {
        (EventKind::Changed, Some(a), Some(b)) => format!("{a} → {b}"),
        (EventKind::Closed, Some(a), _) => a,
        (_, _, Some(b)) => b,
        _ => String::new(),
    };
    [
        crate::timefmt::stamp(e.time, first),
        t!(e.kind.key()),
        e.conn.proto.label().to_owned(),
        addr(e.conn.local),
        e.conn.remote.map(addr).unwrap_or_default(),
        states,
        format!("{} ({})", e.process.name, e.conn.pid),
    ]
}

impl App {
    /// The event list under the table while a recording is open.
    pub(in crate::app) fn replay_events_panel(&mut self, ui: &mut egui::Ui) {
        if self.replay.is_none() {
            return;
        }
        egui::Panel::bottom("replay_events")
            .resizable(true)
            .default_size(220.0)
            .min_size(90.0)
            .show(ui, |ui| self.replay_events_ui(ui));
    }

    fn replay_events_ui(&mut self, ui: &mut egui::Ui) {
        let Some(rp) = &mut self.replay else {
            return;
        };
        let list = &mut rp.events;
        let mut changed = false;
        ui.horizontal(|ui| {
            ui.label(RichText::new(t!("replay.events")).strong());
            match list.total {
                Some(n) => ui.weak(t!("replay.events.count", count = n)),
                None => ui.spinner(),
            };
            ui.separator();
            let q = &mut list.query;
            changed |= ui.checkbox(&mut q.present, t!("event.present")).changed();
            changed |= ui.checkbox(&mut q.kinds.new, t!("event.new")).changed();
            changed |= ui
                .checkbox(&mut q.kinds.closed, t!("event.closed"))
                .changed();
            changed |= ui
                .checkbox(&mut q.kinds.changed, t!("event.changed"))
                .changed();
            ui.separator();
            changed |= ui
                .add(
                    egui::TextEdit::singleline(&mut q.text)
                        .hint_text(t!("toolbar.filter.hint"))
                        .desired_width(200.0),
                )
                .on_hover_text(t!("replay.events.filter.tip"))
                .changed();
            if !q.text.is_empty() && ui.small_button("✖").clicked() {
                q.text.clear();
                changed = true;
            }
        });
        if changed {
            list.restart();
        }
        let first = rp.info.as_ref().and_then(|i| i.first).unwrap_or(0);
        let total = list.total.unwrap_or(0);
        let dark = ui.visuals().dark_mode;
        let row_h = ui.text_style_height(&egui::TextStyle::Body) + 4.0;
        let gap = egui::vec2(
            ui.spacing().item_spacing.x,
            ui.spacing().item_spacing.y * 0.5 + 0.5,
        );
        let mut clicked: Option<(u64, u64)> = None;
        let mut sort_click = false;
        let mut visible: Option<(u64, u64)> = None;
        let titles = [
            t!("replay.events.time"),
            t!("replay.events.event"),
            t!("column.protocol"),
            t!("replay.events.local"),
            t!("replay.events.remote"),
            t!("column.state"),
            t!("column.process"),
        ];
        let widths = [130.0, 120.0, 60.0, 190.0, 230.0, 190.0];
        let mut tb = TableBuilder::new(ui)
            .id_salt("replay_events_table")
            .striped(true)
            .resizable(true)
            .sense(Sense::click())
            .auto_shrink([false, false])
            .cell_layout(egui::Layout::left_to_right(egui::Align::Center));
        for w in widths {
            // No clipping: the labels truncate themselves and the colors reach into the gaps.
            tb = tb.column(Column::initial(w).clip(false));
        }
        tb = tb.column(Column::remainder().at_least(LAST_COLUMN_MIN));
        let newest_first = list.query.newest_first;
        tb.header(20.0, |mut h| {
            for (i, title) in titles.iter().enumerate() {
                h.col(|ui| {
                    // Only the time sorts (oldest / newest first).
                    let arrow = (i == 0).then_some(!newest_first);
                    crate::app::table::header_title(ui, title, arrow);
                    if i == 0 {
                        let resp = ui.interact(
                            ui.max_rect(),
                            ui.scope_id().with("sort_time"),
                            Sense::click(),
                        );
                        if resp.on_hover_text(t!("replay.events.sort.tip")).clicked() {
                            sort_click = true;
                        }
                    }
                });
            }
        })
        .body(|body| {
            body.rows(row_h, total as usize, |mut row| {
                let pos = row.index() as u64;
                visible = Some(match visible {
                    None => (pos, pos),
                    Some((a, b)) => (a.min(pos), b.max(pos)),
                });
                row.set_selected(list.selected == Some(pos));
                let Some(e) = list.get(pos) else {
                    row.col(|ui| {
                        ui.weak("…");
                    });
                    return;
                };
                let cells = event_cells(e, first);
                let bg = kind_color(e.kind, dark);
                for text in cells {
                    row.col(|ui| {
                        if let Some(bg) = bg {
                            ui.painter()
                                .rect_filled(ui.max_rect().expand2(gap), 0.0, bg);
                        }
                        ui.add(egui::Label::new(text).truncate().selectable(false));
                    });
                }
                if row.response().clicked() {
                    clicked = Some((pos, e.time));
                }
            });
        });
        if sort_click {
            list.query.newest_first = !list.query.newest_first;
            list.restart();
        }
        let requests = match visible {
            Some((a, b)) => list.missing(a, b),
            // Nothing drawn yet (no count): the first page brings the count.
            None if list.total.is_none() => list.missing(0, 0),
            None => Vec::new(),
        };
        for r in requests {
            rp.ask(r);
        }
        if let Some((pos, time)) = clicked {
            rp.events.selected = Some(pos);
            self.replay_seek(time);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_pages_are_asked_once() {
        let mut l = EventList::default();
        l.restart();
        let pages = |r: Vec<Request>| -> Vec<u64> {
            r.into_iter()
                .map(|r| match r {
                    Request::Events { page, .. } => page,
                    _ => unreachable!(),
                })
                .collect()
        };
        assert_eq!(pages(l.missing(0, 0)), [0]);
        assert!(l.missing(0, 10).is_empty(), "asked already");
        l.receive(l.generation, 0, 1000, vec![]);
        assert_eq!(pages(l.missing(150, 450)), [1, 2]);
        // An answer of an old query is ignored.
        l.receive(l.generation + 5, 1, 1000, vec![]);
        assert!(!l.pages.contains_key(&1));
        l.restart();
        assert_eq!(l.total, None);
        assert_eq!(pages(l.missing(0, 0)), [0]);
    }
}
