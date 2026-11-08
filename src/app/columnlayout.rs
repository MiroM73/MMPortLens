//! Column layout in the table (#2): a header dragged to a new place, the widths remembered in the
//! settings, a double click on a header border fits the width to the content, and drag and drop
//! in the column window.

use crate::app::*;
use crate::columns::drop_slot;

/// How many rows a double click on a header border measures (the view can be long).
const FIT_ROWS: usize = 5000;

/// How far from the left / right edge a dragged header scrolls the table, and how fast.
const EDGE_SCROLL_ZONE: f32 = 30.0;
const EDGE_SCROLL_STEP: f32 = 12.0;

/// A column being dragged (#2) and the drop place shown in the last frame (target column, after
/// it). The drop uses the place shown, not the pointer of the release frame, which may already
/// be elsewhere.
#[derive(Clone, Copy)]
pub(in crate::app) struct ColumnDrag {
    id: ColumnId,
    slot: Option<(ColumnId, bool)>,
}

impl ColumnDrag {
    fn new(id: ColumnId) -> Self {
        Self { id, slot: None }
    }
}

impl App {
    /// After the table is drawn: remembers the widths the user dragged (once the mouse button is
    /// released, so a resize does not write the settings every frame). `cells` are the header
    /// cell rects of `cols`; the last column fills the rest of the window and keeps its width.
    pub(in crate::app) fn remember_column_widths(
        &mut self,
        ui: &egui::Ui,
        cols: &[ColumnId],
        cells: &[egui::Rect],
    ) {
        if ui.input(|i| i.pointer.any_down()) || cells.len() != cols.len() {
            return;
        }
        let mut changed = false;
        for (c, cell) in cols.iter().zip(cells).take(cols.len().saturating_sub(1)) {
            changed |= self.cfg.columns.set_width(*c, cell.width());
        }
        if changed {
            self.settings_changed();
        }
    }

    /// A header being dragged to a new place: drop indicator, the title at the pointer, scrolling
    /// at the edges, and the move when the mouse button is released (Esc cancels).
    pub(in crate::app) fn header_drag(
        &mut self,
        ui: &egui::Ui,
        cols: &[ColumnId],
        cells: &[egui::Rect],
        view: egui::Rect,
        started: Option<ColumnId>,
    ) {
        if let Some(c) = started {
            self.header_drag = Some(ColumnDrag::new(c));
        }
        let Some(drag) = self.header_drag else {
            return;
        };
        let dragged = drag.id;
        let (pos, down, esc) = ui.input(|i| {
            (
                i.pointer.hover_pos(),
                i.pointer.primary_down(),
                i.key_pressed(egui::Key::Escape),
            )
        });
        if esc || !cols.contains(&dragged) || cells.len() != cols.len() {
            self.header_drag = None;
            return;
        }
        let edges: Vec<(f32, f32)> = cells.iter().map(|r| (r.left(), r.right())).collect();
        if !down {
            if let Some((target, after)) = drag.slot {
                self.cfg.columns.move_to(dragged, target, after);
                self.settings_changed();
            }
            self.header_drag = None;
            return;
        }
        let slot = pos.and_then(|p| drop_slot(&edges, p.x));
        self.header_drag = Some(ColumnDrag {
            id: dragged,
            slot: slot.map(|(i, after)| (cols[i], after)),
        });

        ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
        let painter = ui
            .ctx()
            .layer_painter(egui::LayerId::new(
                egui::Order::Foreground,
                egui::Id::unique("column_drop"),
            ))
            .with_clip_rect(view);
        let accent = ui.visuals().selection.stroke.color;
        if let Some((i, after)) = slot {
            let x = if after {
                cells[i].right()
            } else {
                cells[i].left()
            };
            painter.line_segment(
                [egui::pos2(x, view.top()), egui::pos2(x, cells[i].bottom())],
                egui::Stroke::new(3.0, accent),
            );
        }
        if let Some(p) = pos {
            let galley = painter.layout_no_wrap(
                dragged.title(),
                egui::TextStyle::Button.resolve(ui.style()),
                ui.visuals().strong_text_color(),
            );
            let at = p + egui::vec2(14.0, -galley.size().y * 0.5);
            let rect = egui::Rect::from_min_size(at, galley.size()).expand(4.0);
            painter.rect_filled(rect, 3.0, ui.visuals().widgets.active.weak_bg_fill);
            painter.rect_stroke(
                rect,
                3.0,
                egui::Stroke::new(1.0, accent),
                egui::StrokeKind::Outside,
            );
            painter.galley(at, galley, Color32::PLACEHOLDER);
            // Near an edge the hidden columns scroll into view (#27).
            if p.x < view.left() + EDGE_SCROLL_ZONE {
                self.table_scroll_x -= EDGE_SCROLL_STEP;
            } else if p.x > view.right() - EDGE_SCROLL_ZONE {
                self.table_scroll_x += EDGE_SCROLL_STEP;
            }
        }
        ui.ctx().request_repaint();
    }

    /// A double click on the border right of a header (where the width is dragged) fits that
    /// column to its widest text. `table` is the area of the header and the rows.
    pub(in crate::app) fn fit_on_border_double_click(
        &mut self,
        ui: &egui::Ui,
        cols: &[ColumnId],
        cells: &[egui::Rect],
        table: egui::Rect,
    ) {
        let Some(p) = ui.input(|i| {
            i.pointer
                .button_double_clicked(egui::PointerButton::Primary)
                .then(|| i.pointer.interact_pos())
                .flatten()
        }) else {
            return;
        };
        if !table.contains(p) || cells.len() != cols.len() {
            return;
        }
        let half_gap = ui.spacing().item_spacing.x * 0.5;
        let grab = ui.style().interaction.resize_grab_radius_side;
        // The last column has no border to drag (it fills the rest of the window).
        let hit = cells
            .iter()
            .take(cells.len().saturating_sub(1))
            .position(|r| (p.x - (r.right() + half_gap)).abs() <= grab);
        if let Some(i) = hit {
            self.fit_column(ui, cols[i]);
        }
    }

    /// Sets the width of a column to its widest cell text (the first `FIT_ROWS` rows shown and
    /// the title with the sort arrow).
    pub(in crate::app) fn fit_column(&mut self, ui: &egui::Ui, c: ColumnId) {
        let painter = ui.painter();
        let width = |text: String, style: egui::TextStyle| {
            painter
                .layout_no_wrap(text, style.resolve(ui.style()), Color32::PLACEHOLDER)
                .size()
                .x
        };
        let mut w = width(format!("{} ⏷", c.title()), egui::TextStyle::Body);
        for r in self.view.iter().take(FIT_ROWS) {
            w = w.max(width(cell_text(r, c), egui::TextStyle::Body));
        }
        if c == ColumnId::Process {
            // The process icon in front of the name.
            w += ui.text_style_height(&egui::TextStyle::Body) + ui.spacing().item_spacing.x;
        }
        w += ui.spacing().item_spacing.x * 2.0 + 4.0;
        if self.cfg.columns.set_width(c, w) {
            self.apply_widths = true;
            self.settings_changed();
        }
    }

    /// Drag and drop in the column window: `rows` are the rects of the rows in the order of
    /// `order` (all columns, visible and hidden); `started` the row whose handle began a drag.
    pub(in crate::app) fn columns_list_drag(
        &mut self,
        ui: &egui::Ui,
        order: &[ColumnId],
        rows: &[egui::Rect],
        started: Option<ColumnId>,
    ) -> bool {
        if let Some(c) = started {
            self.columns_drag = Some(ColumnDrag::new(c));
        }
        let Some(drag) = self.columns_drag else {
            return false;
        };
        let (pos, down) = ui.input(|i| (i.pointer.hover_pos(), i.pointer.primary_down()));
        // Rows turned sideways: the same slot search as for the header cells.
        let edges: Vec<(f32, f32)> = rows.iter().map(|r| (r.top(), r.bottom())).collect();
        if !down {
            self.columns_drag = None;
            if let Some((target, after)) = drag.slot {
                self.cfg.columns.move_to(drag.id, target, after);
                return true;
            }
            return false;
        }
        let slot = pos.and_then(|p| drop_slot(&edges, p.y));
        self.columns_drag = Some(ColumnDrag {
            id: drag.id,
            slot: slot.map(|(i, after)| (order[i], after)),
        });
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
        if let Some((i, after)) = slot {
            let y = if after {
                rows[i].bottom()
            } else {
                rows[i].top()
            };
            ui.painter().line_segment(
                [
                    egui::pos2(rows[i].left(), y),
                    egui::pos2(rows[i].right(), y),
                ],
                egui::Stroke::new(2.0, ui.visuals().selection.stroke.color),
            );
        }
        ui.ctx().request_repaint();
        false
    }
}
