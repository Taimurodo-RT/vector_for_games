//! Guide lines of the board (`guide x 120`): pulled out of the rulers, moved along the canvas,
//! dropped back on a ruler to remove, and listed with exact numbers in the panel on the right.
//! Objects and points being placed stick to them like to the edges of other objects.

use crate::app::{App, Tool, fold};
use crate::canvas::View;
use crate::guides::{RULER, Targets};
use crate::theme::{self, FAINT, KeyState};
use egui::{Align2, Color32, Pos2, Rect, RichText, Stroke, pos2, vec2};
use motif_core::Document;
use motif_core::kurbo::Point;

/// Guide lines are drawn in this colour: apart from the pink of the smart guides.
pub const LINE: Color32 = Color32::from_rgb(0, 186, 230);
/// How close, in screen points, the pointer must be to a guide to take it.
const GRAB: f32 = 4.0;

/// A guide following the pointer: a new one pulled out of a ruler, or one of the board's.
#[derive(Clone, Debug)]
pub(crate) struct GuideDrag {
    /// Which guide of the scene; `None` for a new one.
    pub index: Option<usize>,
    pub vertical: bool,
    pub at: f64,
    /// The pointer is over the ruler or off the canvas: releasing removes the guide.
    pub remove: bool,
    /// Edges and centres of the objects, collected when the drag started.
    targets: Targets,
}

impl App {
    /// What objects and points can stick to: the objects of `doc` apart from `moving`, with
    /// their anchors when `points` is set, and the guide lines of the board.
    pub(crate) fn snap_targets(&self, doc: &Document, moving: &[usize], points: bool) -> Targets {
        let mut t = if points { Targets::collect_points(doc, moving) } else { Targets::collect(doc, moving) };
        if self.snap.user_guides {
            let (w, h) = (doc.artboard.width, doc.artboard.height);
            for g in &self.scene.guides {
                t.add_line(g.vertical, g.at, if g.vertical { (0.0, h) } else { (0.0, w) });
            }
        }
        t
    }

    /// The pointer on the rulers and on the guide lines. Returns true when it is busy with a
    /// guide and the canvas should leave the press alone. `busy` tells that the pointer is on a
    /// handle of the selection, which wins over a guide under it.
    pub(crate) fn guide_input(&mut self, ui: &egui::Ui, resp: &egui::Response, rect: Rect, view: View, doc: &Document, busy: bool) -> bool {
        if !self.snap.user_guides {
            self.guide_drag = None;
            return false;
        }
        let rulers = self.snap.rulers;
        let top = Rect::from_min_max(rect.min, pos2(rect.right(), rect.top() + RULER));
        let left = Rect::from_min_max(rect.min, pos2(rect.left() + RULER, rect.bottom()));
        let on_ruler = |p: Pos2, vertical: bool| rulers && if vertical { left.contains(p) } else { top.contains(p) };
        let pointer = resp.interact_pointer_pos().or(resp.hover_pos());

        if self.guide_drag.is_some() {
            if let (Some(p), Some(d)) = (pointer, self.guide_drag.as_mut()) {
                let dp = view.to_doc(rect, p);
                let off = ui.input(|i| i.modifiers.command);
                let placed = self.snap.point(&d.targets, dp, view.zoom, off).at;
                d.at = if d.vertical { placed.x } else { placed.y };
                d.remove = on_ruler(p, d.vertical) || !rect.contains(p);
                ui.ctx().set_cursor_icon(if d.remove {
                    egui::CursorIcon::NotAllowed
                } else if d.vertical {
                    egui::CursorIcon::ResizeHorizontal
                } else {
                    egui::CursorIcon::ResizeVertical
                });
            }
            if resp.drag_stopped() || !ui.input(|i| i.pointer.primary_down()) {
                self.finish_guide_drag();
            }
            return true;
        }
        if self.drag.is_some() {
            return false;
        }
        let Some(p) = pointer.filter(|_| resp.hovered() || resp.dragged()) else { return false };
        let press = ui.input(|i| i.pointer.press_origin()).unwrap_or(p);
        // A guide under the pointer: its tab on the ruler always takes it; on the canvas only
        // the selection tool does, and only where no object lies.
        let free_canvas = |q: Pos2| self.tool == Tool::Select && !busy && doc.hit_test(view.to_doc(rect, q)).is_none();
        let grabbed = |q: Pos2| {
            self.scene.guides.iter().enumerate().filter(|(_, g)| !g.locked).find_map(|(i, g)| {
                let s = view.to_screen(rect, Point::new(g.at, g.at));
                let (near, tab) = if g.vertical { ((s.x - q.x).abs() <= GRAB, top.contains(q)) } else { ((s.y - q.y).abs() <= GRAB, left.contains(q)) };
                (near && ((rulers && tab) || free_canvas(q))).then_some(i)
            })
        };
        let started = resp.drag_started_by(egui::PointerButton::Primary);
        if let Some(i) = grabbed(if started { press } else { p }) {
            let g = self.scene.guides[i];
            ui.ctx().set_cursor_icon(if g.vertical { egui::CursorIcon::ResizeHorizontal } else { egui::CursorIcon::ResizeVertical });
            if started {
                self.guide_drag = Some(GuideDrag { index: Some(i), vertical: g.vertical, at: g.at, remove: false, targets: Targets::collect(doc, &[]) });
            }
            return true;
        }
        // A press on a ruler pulls a new guide out of it: a horizontal one from the top ruler, a
        // vertical one from the left.
        let corner = Rect::from_min_size(rect.min, vec2(RULER, RULER));
        if rulers && started && !corner.contains(press) && (top.contains(press) || left.contains(press)) {
            let vertical = left.contains(press);
            let dp = view.to_doc(rect, press);
            self.guide_drag =
                Some(GuideDrag { index: None, vertical, at: if vertical { dp.x } else { dp.y }, remove: true, targets: Targets::collect(doc, &[]) });
            return true;
        }
        rulers && (top.contains(p) || left.contains(p))
    }

    /// Writes the dragged guide to the script: added, moved, or removed when dropped on a ruler.
    pub(crate) fn finish_guide_drag(&mut self) {
        let Some(d) = self.guide_drag.take() else { return };
        if !self.can_edit() {
            return;
        }
        let out = match (d.index, d.remove) {
            (None, true) => return,
            (None, false) => motif_script::add_guide(&self.src, &self.scene, d.vertical, d.at),
            (Some(i), true) => motif_script::remove_guides(&self.src, &self.scene, &[i]),
            (Some(i), false) => {
                if self.scene.guides.get(i).is_none_or(|g| g.at == d.at) {
                    return;
                }
                match motif_script::set_guide(&self.src, &self.scene, i, d.at, false) {
                    Some(out) => out,
                    None => return,
                }
            }
        };
        if self.apply_once("guide", Some(out)) {
            self.status = match (d.index, d.remove) {
                (_, true) => "Направляющая убрана".into(),
                (None, _) => format!(
                    "Направляющая {} = {}. Перетащите её обратно на линейку, чтобы убрать",
                    if d.vertical { "x" } else { "y" },
                    motif_script::fmt_num(d.at)
                ),
                _ => format!("Направляющая {} = {}", if d.vertical { "x" } else { "y" }, motif_script::fmt_num(d.at)),
            };
        }
    }

    /// The guide lines across the canvas, their tabs on the rulers and the one being dragged.
    pub(crate) fn paint_user_guides(&self, painter: &egui::Painter, rect: Rect, view: View) {
        if !self.snap.user_guides {
            return;
        }
        let ruler = if self.snap.rulers { RULER } else { 0.0 };
        let line = |vertical: bool, at: f64, stroke: Stroke| {
            let s = view.to_screen(rect, Point::new(at, at));
            if vertical && s.x >= rect.left() + ruler && s.x <= rect.right() {
                painter.line_segment([pos2(s.x, rect.top()), pos2(s.x, rect.bottom())], stroke);
            } else if !vertical && s.y >= rect.top() + ruler && s.y <= rect.bottom() {
                painter.line_segment([pos2(rect.left(), s.y), pos2(rect.right(), s.y)], stroke);
            }
        };
        let dragged = self.guide_drag.as_ref().and_then(|d| d.index);
        for (i, g) in self.scene.guides.iter().enumerate() {
            if dragged == Some(i) {
                continue;
            }
            line(g.vertical, g.at, Stroke::new(1.0, if g.locked { LINE.gamma_multiply(0.55) } else { LINE }));
        }
        if let Some(d) = &self.guide_drag {
            let color = if d.remove { LINE.gamma_multiply(0.35) } else { LINE };
            line(d.vertical, d.at, Stroke::new(1.5, color));
            if !d.remove {
                let s = view.to_screen(rect, Point::new(d.at, d.at));
                let (at, align) = if d.vertical {
                    (pos2(s.x + 6.0, rect.top() + ruler + 6.0), Align2::LEFT_TOP)
                } else {
                    (pos2(rect.left() + ruler + 6.0, s.y - 4.0), Align2::LEFT_BOTTOM)
                };
                let text = format!("{} {}", if d.vertical { "x" } else { "y" }, motif_script::fmt_num(d.at));
                let galley = painter.layout_no_wrap(text, theme::semibold(11.5), Color32::BLACK);
                let b = align.anchor_size(at, galley.size() + vec2(10.0, 4.0));
                painter.rect_filled(b, 4.0, LINE);
                painter.galley(b.min + vec2(5.0, 2.0), galley, Color32::BLACK);
            }
        }
    }

    /// The guides of the board in the panel on the right: exact places, locks and removal.
    pub(crate) fn guides_section(&mut self, ui: &mut egui::Ui) {
        let guides = self.scene.guides.clone();
        let mut edit: Option<String> = None;
        fold(ui, "Направляющие", |ui| {
            if guides.is_empty() {
                ui.label(
                    RichText::new("Потяните из линейки на холст: из верхней — горизонтальная, из левой — вертикальная. Объекты и точки к ним прилипают.")
                        .size(11.5)
                        .color(FAINT),
                );
                return;
            }
            for (i, g) in guides.iter().enumerate() {
                ui.horizontal(|ui| {
                    let label = if g.vertical { "X" } else { "Y" };
                    let out = theme::num_field(ui, 120.0, label, g.at, 1.0, -100_000.0..=100_000.0, 2, KeyState::Hidden);
                    if let Some(v) = out.changed {
                        edit = motif_script::set_guide(&self.src, &self.scene, i, (v * 100.0).round() / 100.0, g.locked);
                    }
                    let mut locked = g.locked;
                    if ui.checkbox(&mut locked, "замок").on_hover_text("Заблокированную направляющую мышь не сдвигает").changed()
                    {
                        edit = motif_script::set_guide(&self.src, &self.scene, i, g.at, locked);
                    }
                    if theme::button(ui, "×", theme::ButtonKind::Normal).on_hover_text("Убрать направляющую").clicked() {
                        edit = Some(motif_script::remove_guides(&self.src, &self.scene, &[i]));
                    }
                });
            }
            if guides.len() > 1 && theme::button(ui, "Убрать все", theme::ButtonKind::Normal).clicked() {
                let all: Vec<usize> = (0..guides.len()).collect();
                edit = Some(motif_script::remove_guides(&self.src, &self.scene, &all));
            }
            ui.label(RichText::new("Направляющие хранятся в файле сцены и не попадают в рендер, экспорт и игру.").size(11.5).color(FAINT));
        });
        if let Some(out) = edit
            && self.can_edit()
        {
            self.apply("guide", Some(out));
        }
    }
}
