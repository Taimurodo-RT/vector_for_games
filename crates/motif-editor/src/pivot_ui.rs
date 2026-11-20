//! The pivot of the selected object as a handle on the canvas. The object turns and scales
//! around this point; dragging the handle moves the point and leaves the object where it is.
//!
//! The mark is always shown, but it is taken by the mouse only in the pivot mode (the Y key or
//! the checkbox by the pivot fields): it sits in the middle of the object, right where people
//! grab the object to move it.

use crate::app::{App, Tool};
use crate::canvas::View;
use crate::theme::SEL;
use crate::transform_frame::Frame;
use egui::{Color32, Pos2, Rect, Stroke, vec2};
use motif_core::kurbo::{Affine, Point, Vec2};
use motif_core::{Document, Prop, ShapeKind, Value};
use motif_script::fmt_num;

/// How close the pointer must be to the mark to take it, in screen points.
const GRAB: f32 = 6.0;
/// On a frame smaller than this on the screen a pivot at its default place is not shown: it
/// would cover the object.
const ROOM: f32 = 36.0;

/// The pivot being dragged: where it is now on the artboard.
pub(crate) struct PivotDrag {
    pub(crate) node: String,
    pub(crate) at: Point,
}

/// The pivot as written for an object whose pivot is moved to `local` (its own space), and the
/// position that keeps the object where it is: `T(o + p) · N · T(−p)` must not change, so the
/// origin goes by `(N − 1)(p′ − p)`.
pub(crate) fn moved_pivot(n: &motif_core::Node, local: Point) -> (Vec2, Point) {
    let exact = |v: f64| {
        let r = (v * 100.0).round() / 100.0;
        if r == 0.0 { 0.0 } else { r }
    };
    let pivot = Vec2::new(exact(local.x), exact(local.y));
    let tan = |deg: f64| deg.clamp(-89.0, 89.0).to_radians().tan();
    let turn = n.warp.unwrap_or(Affine::IDENTITY).with_translation(Vec2::ZERO)
        * Affine::rotate((n.rotation + n.swing).to_radians())
        * Affine::skew(tan(n.skew_x), tan(n.skew_y))
        * Affine::scale_non_uniform(n.scale_x, n.scale_y);
    let d = pivot - n.pivot;
    let shift = (turn * d.to_point()).to_vec2() - d;
    (pivot, Point::new(exact(n.x + shift.x), exact(n.y + shift.y)))
}

impl App {
    /// The object whose pivot is shown, and the pivot on the artboard.
    fn shown_pivot(&self, doc: &Document, frame: &Frame, rect: Rect, view: View) -> Option<(usize, Point)> {
        let &[i] = frame.items.as_slice() else { return None };
        let n = &doc.nodes[i];
        if self.tool != Tool::Select || !self.also.is_empty() || n.kind == ShapeKind::Bone {
            return None;
        }
        let h = frame.handles(rect, view);
        let room = h.corners[0].distance(h.corners[1]).min(h.corners[1].distance(h.corners[2]));
        (self.pivot_mode || n.pivot != Vec2::ZERO || room >= ROOM).then(|| (i, doc.world_transform(i) * n.pivot.to_point()))
    }

    /// The pointer on the pivot mark, in the pivot mode. Returns true when it is busy with the
    /// mark and the canvas should leave the press alone. The mark wins over a handle of the frame
    /// under it: a pivot put on a corner can be taken again, and the other handles still resize.
    pub(crate) fn pivot_input(&mut self, ui: &egui::Ui, resp: &egui::Response, rect: Rect, view: View, doc: &Document, frame: Option<&Frame>) -> bool {
        let pointer = resp.interact_pointer_pos().or(resp.hover_pos());
        self.pivot_hover = false;
        if self.pivot_drag.is_some() {
            if let (Some(p), Some(frame)) = (pointer, frame) {
                // The mark sticks to the nine points of the frame and to everything a pen point does.
                let mut targets = self.snap_targets(doc, &[], true);
                for hx in [-1, 0, 1] {
                    for hy in [-1, 0, 1] {
                        targets.add_point(frame.point(hx, hy));
                    }
                }
                let off = ui.input(|i| i.modifiers.command);
                let s = self.snap.point(&targets, view.to_doc(rect, p), view.zoom, off);
                (self.guides, self.snap_mark) = (s.guides, s.mark);
                if let Some(d) = self.pivot_drag.as_mut() {
                    d.at = s.at;
                }
                ui.ctx().set_cursor_icon(egui::CursorIcon::Crosshair);
            }
            if resp.drag_stopped() || !ui.input(|i| i.pointer.primary_down()) {
                self.finish_pivot_drag(doc);
            }
            return true;
        }
        if self.drag.is_some() || !self.pivot_mode {
            return false;
        }
        let Some(p) = pointer.filter(|_| resp.hovered() || resp.dragged()) else { return false };
        let Some((i, at)) = frame.and_then(|f| self.shown_pivot(doc, f, rect, view)) else { return false };
        let mark = view.to_screen(rect, at);
        let started = resp.drag_started_by(egui::PointerButton::Primary);
        let press = ui.input(|i| i.pointer.press_origin()).unwrap_or(p);
        if mark.distance(if started { press } else { p }) > GRAB {
            return false;
        }
        // The mark answers before the press: it grows, the cursor changes and a note names it.
        ui.ctx().set_cursor_icon(egui::CursorIcon::Crosshair);
        self.pivot_hover = true;
        let node = doc.nodes[i].id.clone();
        if resp.double_clicked() {
            // Back to the object's own point: the centre of a shape.
            self.set_pivot(doc, &node, doc.world_transform(i) * Point::ZERO);
        } else if started {
            if self.key_time().is_some() {
                self.status = "Опорная точка — часть рисунка: её переносят в «Рисовании» или без выбранного клипа".into();
            } else if self.can_edit() {
                self.pivot_drag = Some(PivotDrag { node, at });
            }
        }
        true
    }

    fn finish_pivot_drag(&mut self, doc: &Document) {
        self.guides.clear();
        self.snap_mark = None;
        if let Some(d) = self.pivot_drag.take() {
            self.set_pivot(doc, &d.node, d.at);
        }
    }

    /// Moves the pivot of `node` to the point `at` of the artboard, as one undo step. The object
    /// stays where it is: its position takes up the difference.
    pub(crate) fn set_pivot(&mut self, doc: &Document, node: &str, at: Point) {
        if !self.can_edit() {
            return;
        }
        if self.key_time().is_some() {
            self.status = "Опорная точка — часть рисунка: её переносят в «Рисовании» или без выбранного клипа".into();
            return;
        }
        let (Some(i), Some(base)) = (doc.index_of(node), self.scene.doc.node(node)) else { return };
        let (pivot, origin) = moved_pivot(base, doc.world_transform(i).inverse() * at);
        if pivot == base.pivot {
            return;
        }
        let (x, y) = (base.x, base.y);
        let text = (pivot != Vec2::ZERO).then(|| format!("pivot {} {}", fmt_num(pivot.x), fmt_num(pivot.y)));
        let Some(out) = motif_script::set_style(&self.src, &self.scene, node, "pivot", text.as_deref()) else {
            self.status = "Объект создан циклом или копией символа: опорная точка задаётся в скрипте".into();
            return;
        };
        self.last_checkpoint.0.clear();
        self.edit_group = Some("pivot".into());
        if self.apply("pivot", Some(out)) {
            if origin.x != x {
                self.edit(node, Prop::X, Value::Num(origin.x));
            }
            if origin.y != y {
                self.edit(node, Prop::Y, Value::Num(origin.y));
            }
            self.status = format!("Опорная точка {} {}: объект остался на месте", fmt_num(pivot.x), fmt_num(pivot.y));
        }
        self.edit_group = None;
        self.last_checkpoint.0.clear();
    }

    /// The pivot mark of the selected object: a ring with a cross.
    pub(crate) fn paint_pivot(&self, painter: &egui::Painter, rect: Rect, view: View, doc: &Document, frame: Option<&Frame>) {
        let at = match (&self.pivot_drag, frame.and_then(|f| self.shown_pivot(doc, f, rect, view))) {
            (Some(d), _) => d.at,
            (None, Some((_, at))) => at,
            (None, None) => return,
        };
        let c: Pos2 = view.to_screen(rect, at);
        let held = self.pivot_hover || self.pivot_drag.is_some();
        if self.pivot_mode {
            // In the pivot mode the mark is the thing to take: it stands out.
            painter.circle_filled(c, if held { 10.0 } else { 8.0 }, SEL.gamma_multiply(if held { 0.4 } else { 0.22 }));
        }
        if held && self.snap.labels {
            let text = match &self.pivot_drag {
                Some(_) => "Опорная точка: объект остаётся на месте",
                None => "Опорная точка: тяните, чтобы перенести",
            };
            crate::measure::paint_pointer_label(painter, c, text);
        }
        painter.circle(c, 4.5, Color32::WHITE, Stroke::new(1.25, SEL));
        for d in [vec2(7.5, 0.0), vec2(0.0, 7.5)] {
            painter.line_segment([c - d, c - d * 0.35], Stroke::new(1.25, SEL));
            painter.line_segment([c + d * 0.35, c + d], Stroke::new(1.25, SEL));
        }
        painter.circle_filled(c, 1.2, SEL);
    }
}
