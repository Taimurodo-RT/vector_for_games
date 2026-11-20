//! Dragging on the canvas: panning, moving, resizing, rotating and creating objects, the selection
//! frame, and the handles of the selected object.

use crate::app::{App, Tool};
use crate::canvas::View;
use egui::{Pos2, Rect, vec2};
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
    Resize {
        node: String,
        /// Artboard space to the node's local space, fixed at drag start.
        to_local: Affine,
        start: (f64, f64),
    },
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

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Handle {
    Corner,
    Rotate,
}

pub(crate) struct Handles {
    pub(crate) corners: [Pos2; 4],
    pub(crate) rotate: Pos2,
    pub(crate) resizable: bool,
}

pub(crate) fn handle_points(rect: Rect, view: View, doc: &Document, i: usize) -> Option<Handles> {
    let node = &doc.nodes[i];
    // Bones have their own handles: the joint and the tip.
    if node.kind == ShapeKind::Bone {
        return None;
    }
    let bounds = crate::app::node_bounds(doc, i)?;
    let t = doc.world_transform(i);
    let corners = [(bounds.x0, bounds.y0), (bounds.x1, bounds.y0), (bounds.x1, bounds.y1), (bounds.x0, bounds.y1)]
        .map(|(x, y)| view.to_screen(rect, t * Point::new(x, y)));
    let top_mid = corners[0].lerp(corners[1], 0.5);
    let center = corners[0].lerp(corners[2], 0.5);
    let up = (top_mid - center).normalized();
    let up = if up.is_finite() { up } else { vec2(0.0, -1.0) };
    Some(Handles { corners, rotate: top_mid + up * 22.0, resizable: !matches!(node.kind, ShapeKind::Group | ShapeKind::Path | ShapeKind::Text) })
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
        grabbed: Option<Handle>,
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
                self.drag_targets = crate::guides::Targets::collect_points(doc, &[]);
                from = self.place_point(dp, view.zoom, free).0;
            }
            Some(DragOp::Create { kind, from, to: from })
        } else if let Some(h) = grabbed
            && let Some(id) = self.selected.clone()
            && let Some(i) = doc.index_of(&id)
        {
            let n = &doc.nodes[i];
            match h {
                Handle::Corner => {
                    self.drag_targets = crate::guides::Targets::collect_points(doc, &[i]);
                    Some(DragOp::Resize { node: id, to_local: doc.world_transform(i).inverse(), start: (n.width, n.height) })
                }
                Handle::Rotate => {
                    let center = doc.world_transform(i) * (Point::ZERO + n.pivot);
                    let a = (dp - center).atan2().to_degrees();
                    Some(DragOp::Rotate { node: id, center, start_angle: a, start_rot: n.rotation })
                }
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
            match doc.hit_test(dp) {
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
                    let targets = crate::guides::Targets::collect(doc, &moving);
                    Some(DragOp::Move { from: dp, items, start_box, targets })
                }
                None => Some(DragOp::Marquee { from: dp, to: dp, add: shift }),
            }
        }
    }

    /// Follows the pointer with whatever is being dragged; changes are previewed and written on release.
    pub(crate) fn update_drag(&mut self, doc: &Document, view: View, dp: Point, drag_delta: egui::Vec2, shift: bool, free: bool) {
        let snap = self.snap;
        // The corner of a new shape and a size handle are points being placed: they stick to
        // anchors, edges and centre lines.
        let placing = match &self.drag {
            Some(DragOp::Create { kind, .. }) => *kind != ShapeKind::Bone,
            Some(DragOp::Resize { .. }) => true,
            _ => false,
        };
        let (placed, placed_stuck) = if placing { self.place_point(dp, view.zoom, free) } else { (dp, false) };
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
            Some(DragOp::Resize { node, to_local, start }) => {
                let local = *to_local * placed;
                // A handle that stuck to something keeps the size that reaches it exactly.
                let size = |v: f64| if placed_stuck { (v * 100.0).round() / 100.0 } else { snap.size(v) };
                let (mut w, mut h) = (size(local.x.abs() * 2.0), size(local.y.abs() * 2.0));
                if shift && start.0 > 0.0 && start.1 > 0.0 {
                    let k = (w / start.0).max(h / start.1);
                    (w, h) = (size(start.0 * k), size(start.1 * k));
                }
                let node = node.clone();
                let cur = doc.node(&node).map(|n| (n.width, n.height));
                if cur.is_some_and(|c| c.0 != w) {
                    self.preview(&node, Prop::Width, Value::Num(w.max(1.0)));
                }
                if cur.is_some_and(|c| c.1 != h) {
                    self.preview(&node, Prop::Height, Value::Num(h.max(1.0)));
                }
            }
            Some(DragOp::Rotate { node, center, start_angle, start_rot }) => {
                let a = (dp - *center).atan2().to_degrees();
                let mut rot = *start_rot + (a - *start_angle);
                rot = if shift { (rot / 15.0).round() * 15.0 } else { snap.rot(rot) };
                let node = node.clone();
                if doc.node(&node).is_some_and(|n| n.rotation != rot) {
                    self.preview(&node, Prop::Rotation, Value::Num(rot));
                }
            }
            Some(DragOp::BoneTip { .. }) | None => {}
        }
    }

    /// Ends the drag: writes the previewed values, makes the new shape or selects what the frame covers.
    pub(crate) fn end_drag(&mut self, doc: &Document, view: View, shift: bool) {
        self.guides.clear();
        self.snap_mark = None;
        self.drag_targets = Default::default();
        self.commit_live();
        match self.drag.take() {
            Some(DragOp::Create { kind: ShapeKind::Bone, from, to }) => self.create_bone(from, to, 10.0 / view.zoom as f64),
            Some(DragOp::Create { kind, from, to }) => self.create_shape(kind, from, to, shift),
            Some(DragOp::Marquee { from, to, add }) => {
                let area = motif_core::kurbo::Rect::from_points(from, to);
                let hits: Vec<String> = (0..doc.nodes.len())
                    .filter(|&i| doc.nodes[i].parent.is_none() && doc.is_visible(i) && !doc.is_locked(i))
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
        if self.apply("create", Some(out)) {
            self.selected = Some(id.clone());
            self.status = format!("Добавлен {} «{id}» — строка в скрипте перед блоками anim", kind.keyword());
        }
        self.tool = Tool::Select;
    }
}
