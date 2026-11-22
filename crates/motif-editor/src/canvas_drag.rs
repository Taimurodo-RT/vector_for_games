//! Dragging on the canvas: panning, moving, resizing, rotating and creating objects, the selection
//! frame, and the handles of the selected object.

use crate::app::{App, Tool};
use crate::canvas::View;
use crate::transform_frame::{Frame, Handle, Pull};
use egui::{Pos2, Rect};
use motif_core::kurbo::{Affine, Point};
use motif_core::{Document, Prop, ShapeKind, Value};
use motif_script::add_node;

pub enum DragOp {
    Pan,
    Move {
        /// Pointer position (artboard space) when the drag started.
        from: Point,
        items: Vec<MoveItem>,
        /// Bounds of everything moving at the start, and what it can line up with.
        start_box: Option<motif_core::kurbo::Rect>,
        targets: crate::guides::Targets,
    },
    /// Rubber-band selection; with Shift it adds to the selection.
    Marquee {
        from: Point,
        to: Point,
        add: bool,
    },
    /// A handle of the transform frame: resizes the selection or turns several objects.
    Transform(Pull),
    Rotate {
        node: String,
        center: Point,
        start_angle: f64,
        start_rot: f64,
    },
    Create {
        kind: ShapeKind,
        from: Point,
        to: Point,
    },
    /// The tip of a bone: turns it, and in Design also sets its length.
    BoneTip {
        node: String,
        /// The bone's joint on the artboard.
        joint: Point,
        /// Direction of the parent's x axis, radians.
        parent_angle: f64,
    },
}

/// One object moved by a drag.
pub struct MoveItem {
    pub(crate) node: String,
    /// Node origin in artboard space when the drag started.
    pub(crate) origin: Point,
    /// Artboard space to the node's parent space.
    pub(crate) to_parent: Affine,
}

impl App {
    /// Where a point being placed at `p` goes ([`crate::snap::Snap::point`]), with the guides and
    /// the mark it shows on the canvas. Also tells whether it stuck to anything.
    pub(crate) fn place_point(&mut self, p: Point, zoom: f32, free: bool) -> (Point, bool) {
        let s = self.snap.point(&self.drag_targets, p, zoom, free);
        self.guides = s.guides;
        self.snap_mark = s.mark;
        (s.at, s.stuck[0] || s.stuck[1])
    }

    /// What a press on the canvas starts: panning, a new shape, a handle of the selection, a key of a
    /// motion path, a bone, the objects under the pointer or a selection frame.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn begin_drag(
        &mut self,
        doc: &Document,
        rect: Rect,
        view: View,
        p: Pos2,
        dp: Point,
        grabbed: Option<(Frame, Handle)>,
        pan_gesture: bool,
        space: bool,
        shift: bool,
        free: bool,
    ) -> Option<DragOp> {
        if pan_gesture {
            if space {
                self.space_used_for_pan = true;
            }
            Some(DragOp::Pan)
        } else if let Tool::Create(kind) = self.tool {
            let mut from = dp;
            if kind != ShapeKind::Bone {
                self.drag_targets = self.snap_targets(doc, &[], true);
                from = self.place_point(dp, view.zoom, free).0;
            }
            Some(DragOp::Create { kind, from, to: from })
        } else if let Some((frame, handle)) = grabbed {
            match (handle, frame.items.as_slice()) {
                // One object turns around its own pivot.
                (Handle::Rotate, &[i]) => {
                    let n = &doc.nodes[i];
                    let center = doc.world_transform(i) * (Point::ZERO + n.pivot);
                    let a = (dp - center).atan2().to_degrees();
                    Some(DragOp::Rotate { node: n.id.clone(), center, start_angle: a, start_rot: n.rotation })
                }
                _ => Some(self.begin_pull(doc, frame, handle, dp)),
            }
        } else if !shift
            && !self.pointer_hits_selection(doc, dp)
            && let Some(hit) = self.motion_hit(rect, view, p)
        {
            Some(self.motion_drag(hit, dp))
        } else if self.tool == Tool::Select
            && !shift
            && let Some(op) = self.bone_drag(doc, rect, view, p, dp)
        {
            Some(op)
        } else {
            match self.pick_shape(doc, dp) {
                Some(i) => {
                    let id = doc.nodes[i].id.clone();
                    if shift && !self.is_selected(&id) {
                        self.toggle_selected(id.clone());
                    } else if !self.is_selected(&id) {
                        self.select_only(Some(id.clone()));
                    }
                    self.sel_keys.clear();
                    let items = self
                        .selection()
                        .into_iter()
                        .filter_map(|id| doc.index_of(&id).map(|k| (id, k)))
                        // A stack places its children itself.
                        .filter(|(_, k)| doc.nodes[*k].parent.is_none_or(|p| doc.nodes[p].stack.is_none()))
                        .map(|(node, k)| MoveItem {
                            node,
                            origin: doc.world_transform(k) * Point::ZERO,
                            to_parent: doc.nodes[k].parent.map_or(Affine::IDENTITY, |pi| doc.world_transform(pi).inverse()),
                        })
                        .collect::<Vec<_>>();
                    if items.is_empty() {
                        self.status = "Объект в стеке: место задаёт стек группы. Порядок меняется перетаскиванием в слоях".into();
                    }
                    let moving: Vec<usize> = items.iter().filter_map(|it| doc.index_of(&it.node)).collect();
                    let start_box = moving.iter().filter_map(|&k| crate::selection::world_bounds(doc, k)).reduce(|a, b| a.union(b));
                    let targets = self.snap_targets(doc, &moving, false);
                    Some(DragOp::Move { from: dp, items, start_box, targets })
                }
                None => Some(DragOp::Marquee { from: dp, to: dp, add: shift }),
            }
        }
    }

    /// Follows the pointer with whatever is being dragged; changes are previewed and written on release.
    ///
    /// Ctrl switches the snapping off. Alt does the same for a move or a new shape; on a size handle
    /// it stretches the frame around its middle instead.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn update_drag(&mut self, doc: &Document, view: View, dp: Point, drag_delta: egui::Vec2, shift: bool, alt: bool, ctrl: bool) {
        let snap = self.snap;
        // The corner of a new shape and a size handle are points being placed: they stick to
        // anchors, edges and centre lines.
        let (placing, aim, free) = match &self.drag {
            Some(DragOp::Create { kind, .. }) => (*kind != ShapeKind::Bone, dp, alt || ctrl),
            Some(DragOp::Transform(pull)) => (pull.sizing(), dp - pull.grab, ctrl),
            _ => (false, dp, alt || ctrl),
        };
        let (placed, placed_stuck) = if placing { self.place_point(aim, view.zoom, free) } else { (dp, false) };
        if let Some(DragOp::Transform(pull)) = &self.drag {
            let mut stuck = placed_stuck;
            if let Some(along_x) = pull.straight_axis() {
                // A side handle moves along one axis: what the other one met does not count.
                self.guides.retain(|g| g.vertical == along_x);
                stuck = !self.guides.is_empty();
            }
            let (edits, note) = pull.edits(snap, placed, stuck, shift, alt);
            for e in edits {
                if e.changed {
                    self.preview(&e.node, e.prop, Value::Num(e.value));
                } else {
                    self.drop_preview(&e.node, e.prop);
                }
            }
            self.drag_label = Some(note.clone());
            self.status = note;
        }
        if let Some(DragOp::BoneTip { node, joint, parent_angle }) = &self.drag {
            let (node, joint, parent_angle) = (node.clone(), *joint, *parent_angle);
            self.drag_bone_tip(doc, &node, joint, parent_angle, dp, shift);
        }
        match &mut self.drag {
            Some(DragOp::Pan) => {
                if let Some(v) = self.view.as_mut() {
                    v.pan += drag_delta;
                }
            }
            Some(DragOp::Create { to, .. } | DragOp::Marquee { to, .. }) => *to = placed,
            Some(DragOp::Move { from, items, start_box, targets }) => {
                let mut delta = dp - *from;
                let mut stuck = [false; 2];
                self.guides.clear();
                // Alt or Ctrl moves freely past the guides.
                if snap.guides
                    && !free
                    && let Some(b) = start_box
                {
                    let (d, s, g) = targets.snap(*b, delta, view.zoom);
                    (delta, stuck, self.guides) = (d, s, g);
                }
                self.drag_label = Some(format!("Δ {} · {}", motif_script::fmt_num(delta.x), motif_script::fmt_num(delta.y)));
                let fine = |v: f64| (v * 10.0).round() / 10.0;
                // What stuck to a guide keeps its exact place, to a hundredth.
                let exact = |v: f64| (v * 100.0).round() / 100.0;
                let moves: Vec<(String, f64, f64)> = items
                    .iter()
                    .map(|it| {
                        let target = it.to_parent * (it.origin + delta);
                        // Stacks and anchors move objects by an offset the script does not hold.
                        let off = doc.node(&it.node).map_or(motif_core::kurbo::Vec2::ZERO, |n| n.offset);
                        let x = if stuck[0] { exact(target.x - off.x) } else { fine(snap.pos(target.x) - off.x) };
                        let y = if stuck[1] { exact(target.y - off.y) } else { fine(snap.pos(target.y) - off.y) };
                        (it.node.clone(), x, y)
                    })
                    .collect();
                for (node, x, y) in moves {
                    let cur = doc.node(&node).map(|n| (n.x, n.y));
                    if cur.is_some_and(|c| c.0 != x) {
                        self.preview(&node, Prop::X, Value::Num(x));
                    }
                    if cur.is_some_and(|c| c.1 != y) {
                        self.preview(&node, Prop::Y, Value::Num(y));
                    }
                }
            }
            Some(DragOp::Rotate { node, center, start_angle, start_rot }) => {
                let a = (dp - *center).atan2().to_degrees();
                let mut rot = *start_rot + (a - *start_angle);
                rot = if shift { (rot / 15.0).round() * 15.0 } else { snap.rot(rot) };
                self.drag_label = Some(format!("Угол {}°", motif_script::fmt_num(rot)));
                let node = node.clone();
                if doc.node(&node).is_some_and(|n| n.rotation != rot) {
                    self.preview(&node, Prop::Rotation, Value::Num(rot));
                }
            }
            Some(DragOp::Transform(_) | DragOp::BoneTip { .. }) | None => {}
        }
    }

    /// Ends the drag: writes the previewed values, makes the new shape or selects what the frame covers.
    pub(crate) fn end_drag(&mut self, doc: &Document, view: View, shift: bool) {
        self.guides.clear();
        self.snap_mark = None;
        self.drag_label = None;
        self.drag_targets = Default::default();
        self.commit_live();
        match self.drag.take() {
            Some(DragOp::Create { kind: ShapeKind::Bone, from, to }) => self.create_bone(from, to, 10.0 / view.zoom as f64),
            Some(DragOp::Create { kind, from, to }) => self.create_shape(kind, from, to, shift),
            Some(DragOp::Marquee { from, to, add }) => {
                let area = motif_core::kurbo::Rect::from_points(from, to);
                // Inside a group the marquee takes its objects only.
                let hits: Vec<String> = self
                    .scope_children(doc)
                    .into_iter()
                    .filter(|&i| doc.is_visible(i) && !doc.is_locked(i))
                    .filter(|&i| crate::selection::world_bounds(doc, i).is_some_and(|b| b.intersect(area).area() > 0.0 || area.contains(b.center())))
                    .map(|i| doc.nodes[i].id.clone())
                    .collect();
                if !add {
                    self.select_only(None);
                }
                for id in hits {
                    if !self.is_selected(&id) {
                        self.toggle_selected(id);
                    }
                }
            }
            _ => {}
        }
        self.drag = None;
    }

    pub(crate) fn create_shape(&mut self, kind: ShapeKind, from: Point, to: Point, square: bool) {
        if !self.can_edit() {
            return;
        }
        // The corners were placed by the snapping: the shape spans them exactly, to a hundredth.
        let exact = |v: f64| (v * 100.0).round() / 100.0;
        let (mut w, mut h) = (exact((to.x - from.x).abs()), exact((to.y - from.y).abs()));
        let mut to = to;
        if w < 4.0 && h < 4.0 {
            (w, h, to) = (80.0, 80.0, from);
        }
        if square || matches!(kind, ShapeKind::Star | ShapeKind::Polygon) && (w - h).abs() < 6.0 {
            // A square grows from the corner where the drag started.
            let m = w.max(h);
            let sign = |d: f64| if d < 0.0 { -1.0 } else { 1.0 };
            if to != from {
                to = Point::new(from.x + sign(to.x - from.x) * m, from.y + sign(to.y - from.y) * m);
            }
            (w, h) = (m, m);
        }
        let (cx, cy) = (exact((from.x + to.x) / 2.0), exact((from.y + to.y) / 2.0));
        let (out, id) = add_node(&self.src, &self.scene, kind, (cx, cy), (w.max(1.0), h.max(1.0)));
        let (out, ids) = self.put_in_scope(out, std::slice::from_ref(&id));
        let id = ids.into_iter().next().unwrap_or(id);
        if self.apply("create", Some(out)) {
            self.selected = Some(id.clone());
            self.status = format!("Добавлен {} «{id}» — строка в скрипте перед блоками anim", kind.keyword());
        }
        self.tool = Tool::Select;
    }
}
