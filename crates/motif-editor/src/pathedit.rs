//! The pen tool and point editing of `path` nodes.
//!
//! Outlines are edited as a list of anchors with optional Bézier handles and written back to the
//! script as SVG path data (`d "M … C … Z"`).

use crate::app::{App, Tool};
use crate::canvas::View;
use crate::compound::{Anchor, Compound};
use crate::theme::{self, SEL};
use egui::{Color32, Pos2, Rect, Shape, Stroke, StrokeKind, vec2};
use motif_core::kurbo::{BezPath, CubicBez, ParamCurve, PathEl, Point, Vec2};
use motif_core::{Document, ShapeKind};
use motif_script::{add_path, set_path};

/// An anchor of an outline with its incoming and outgoing control points (absolute positions).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Vertex {
    pub p: Point,
    pub cin: Option<Point>,
    pub cout: Option<Point>,
}

/// How the two curve handles of a point stand. The outline keeps no mark of it: the kind is
/// read from where the handles are.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NodeKind {
    /// No handles, one handle, or two that go their own ways.
    Corner,
    /// Two handles on one line through the point.
    Smooth,
    /// Two handles on one line and of one length.
    Symmetric,
}

/// How far a handle may be off the line or off the length of the other one and still count as
/// smooth or symmetric: handles pulled by the mouse are kept to a tenth of a unit.
const KIND_SLACK: f64 = 0.15;

impl Vertex {
    fn corner(p: Point) -> Self {
        Vertex { p, cin: None, cout: None }
    }

    pub(crate) fn kind(&self) -> NodeKind {
        let (Some(a), Some(b)) = (self.cin, self.cout) else { return NodeKind::Corner };
        let (u, v) = (self.p - a, b - self.p);
        let (lu, lv) = (u.hypot(), v.hypot());
        if lu < 1e-9 || lv < 1e-9 || u.dot(v) <= 0.0 {
            return NodeKind::Corner;
        }
        // How far the end of the shorter handle is off the line of the longer one.
        if u.cross(v).abs() / lu.max(lv) > KIND_SLACK {
            NodeKind::Corner
        } else if (lu - lv).abs() <= KIND_SLACK {
            NodeKind::Symmetric
        } else {
            NodeKind::Smooth
        }
    }

    /// Puts the incoming or the outgoing handle at `to`. With `linked` the opposite handle turns
    /// to stay on one line with it and keeps its length.
    pub(crate) fn set_handle(&mut self, incoming: bool, to: Point, linked: bool) {
        let p = self.p;
        let (this, other) = if incoming { (&mut self.cin, &mut self.cout) } else { (&mut self.cout, &mut self.cin) };
        *this = Some(to);
        if linked && let Some(o) = other.as_mut() {
            let len = o.distance(p);
            let dir = p - to;
            if dir.hypot() > 1e-9 {
                *o = round_hundredth(p + dir.normalize() * len);
            }
        }
    }

    /// Moves the point with its handles. The sums are kept to a millionth, so the file gets
    /// `0.3` and not `0.30000000000000004`, and coordinates written with more digits keep them.
    pub(crate) fn translate(&mut self, d: Vec2) {
        let clean = |p: Point| Point::new((p.x * 1e6).round() / 1e6, (p.y * 1e6).round() / 1e6);
        self.p = clean(self.p + d);
        self.cin = self.cin.map(|c| clean(c + d));
        self.cout = self.cout.map(|c| clean(c + d));
    }
}

/// A single-contour outline that can be edited point by point.
#[derive(Clone, Debug, PartialEq)]
pub struct Outline {
    pub verts: Vec<Vertex>,
    pub closed: bool,
}

impl Outline {
    /// One contour only; `Compound` splits a multi-contour path before calling this.
    pub fn from_bez(path: &BezPath) -> Option<Outline> {
        let mut verts: Vec<Vertex> = Vec::new();
        let mut closed = false;
        for el in path.elements() {
            match *el {
                PathEl::MoveTo(p) => {
                    if !verts.is_empty() {
                        return None;
                    }
                    verts.push(Vertex::corner(p));
                }
                PathEl::LineTo(p) => verts.push(Vertex::corner(p)),
                PathEl::QuadTo(a, p) => {
                    let last = verts.last_mut()?;
                    let q0 = last.p;
                    last.cout = Some(q0 + (a - q0) * (2.0 / 3.0));
                    verts.push(Vertex { p, cin: Some(p + (a - p) * (2.0 / 3.0)), cout: None });
                }
                PathEl::CurveTo(a, b, p) => {
                    verts.last_mut()?.cout = Some(a);
                    verts.push(Vertex { p, cin: Some(b), cout: None });
                }
                PathEl::ClosePath => closed = true,
            }
        }
        // A closing segment written out to the start point duplicates the first anchor.
        if closed && verts.len() > 1 && verts[0].p == verts[verts.len() - 1].p {
            let last = verts.pop()?;
            verts[0].cin = last.cin;
        }
        (!verts.is_empty()).then_some(Outline { verts, closed })
    }

    pub fn to_bez(&self) -> BezPath {
        let mut path = BezPath::new();
        let Some(first) = self.verts.first() else { return path };
        path.move_to(first.p);
        let seg = |path: &mut BezPath, a: &Vertex, b: &Vertex| match (a.cout, b.cin) {
            (None, None) => path.line_to(b.p),
            (ca, cb) => path.curve_to(ca.unwrap_or(a.p), cb.unwrap_or(b.p), b.p),
        };
        for w in self.verts.windows(2) {
            seg(&mut path, &w[0], &w[1]);
        }
        if self.closed {
            let (a, b) = (self.verts[self.verts.len() - 1], self.verts[0]);
            if a.cout.is_some() || b.cin.is_some() {
                seg(&mut path, &a, &b);
            }
            path.close_path();
        }
        path
    }

    pub(crate) fn segment_count(&self) -> usize {
        if self.closed { self.verts.len() } else { self.verts.len().saturating_sub(1) }
    }

    pub(crate) fn segment(&self, i: usize) -> CubicBez {
        let a = self.verts[i];
        let b = self.verts[(i + 1) % self.verts.len()];
        CubicBez::new(a.p, a.cout.unwrap_or(a.p), b.cin.unwrap_or(b.p), b.p)
    }

    /// Splits segment `i` at `t` without changing the shape; returns the new anchor's index.
    pub(crate) fn split(&mut self, i: usize, t: f64) -> usize {
        let j = (i + 1) % self.verts.len();
        let curved = self.verts[i].cout.is_some() || self.verts[j].cin.is_some();
        let seg = self.segment(i);
        let at = i + 1;
        if curved {
            let (l, r) = (seg.subsegment(0.0..t), seg.subsegment(t..1.0));
            self.verts[i].cout = Some(l.p1);
            self.verts[j].cin = Some(r.p2);
            self.verts.insert(at, Vertex { p: l.p3, cin: Some(l.p2), cout: Some(r.p1) });
        } else {
            self.verts.insert(at, Vertex::corner(seg.eval(t)));
        }
        at
    }

    /// Corner ↔ smooth: drops the handles, or adds ones along the neighbours' direction.
    fn toggle_smooth(&mut self, i: usize) {
        let v = self.verts[i];
        self.set_kind(i, if v.cin.is_some() || v.cout.is_some() { NodeKind::Corner } else { NodeKind::Smooth });
    }

    /// Makes point `i` a corner (its handles go), smooth (the handles stand on one line) or
    /// symmetric (and get one length). Handles that are missing are added along the neighbours,
    /// a third of the way to each.
    pub(crate) fn set_kind(&mut self, i: usize, kind: NodeKind) {
        let n = self.verts.len();
        let v = self.verts[i];
        if kind == NodeKind::Corner {
            self.verts[i].cin = None;
            self.verts[i].cout = None;
            return;
        }
        let prev = if i > 0 || self.closed { Some(self.verts[(i + n - 1) % n].p) } else { None };
        let next = if i + 1 < n || self.closed { Some(self.verts[(i + 1) % n].p) } else { None };
        let arm = |c: Option<Point>| c.map(|c| c - v.p).filter(|d| d.hypot() > 1e-9);
        let (back, forth) = (arm(v.cin), arm(v.cout));
        // The line of the handles: between the two that are there, along the only one, or
        // along the neighbours.
        let dir = match (back, forth) {
            (Some(a), Some(b)) => {
                let between = b.normalize() - a.normalize();
                if between.hypot() > 1e-9 { between } else { b }
            }
            (Some(a), None) => -a,
            (None, Some(b)) => b,
            (None, None) => match (prev, next) {
                (Some(a), Some(b)) => b - a,
                (Some(a), None) => v.p - a,
                (None, Some(b)) => b - v.p,
                (None, None) => return,
            },
        };
        if dir.hypot() < 1e-9 {
            return;
        }
        let u = dir.normalize();
        let third = |q: Option<Point>| q.map(|q| (q.distance(v.p) / 3.0).max(1.0));
        let mut len_back = back.map(|a| a.hypot()).or(third(prev));
        let mut len_forth = forth.map(|a| a.hypot()).or(third(next));
        if kind == NodeKind::Symmetric
            && let (Some(a), Some(b)) = (len_back, len_forth)
        {
            (len_back, len_forth) = (Some((a + b) / 2.0), Some((a + b) / 2.0));
        }
        // To a hundredth, as every handle the editor places.
        self.verts[i].cin = len_back.map(|l| round_hundredth(v.p - u * l));
        self.verts[i].cout = len_forth.map(|l| round_hundredth(v.p + u * l));
    }
}

/// What the pointer holds while editing points.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PointGrab {
    Anchor {
        index: Anchor,
        from: Point,
    },
    In(Anchor),
    Out(Anchor),
    /// A handle of the frame around several selected points.
    Frame(crate::point_frame::FramePull),
}

/// Point editing of one path node.
pub struct PointEdit {
    pub node: String,
    /// The active anchor: its handles are shown and the panel edits it.
    pub selected: Option<Anchor>,
    /// The other selected anchors, in any part of the outline.
    pub also: Vec<Anchor>,
    pub grab: Option<(PointGrab, Compound)>,
    /// The outline while a point is being dragged; written to the script on release.
    pub live: Option<Compound>,
    /// A selection frame being pulled over the anchors (screen points); with Shift it adds.
    pub marquee: Option<(Pos2, Pos2, bool)>,
    /// What a dragged point sticks to: other objects, guides and the points that stay.
    pub targets: Option<std::rc::Rc<crate::guides::Targets>>,
}

impl PointEdit {
    pub fn new(node: String) -> Self {
        PointEdit { node, selected: None, also: Vec::new(), grab: None, live: None, marquee: None, targets: None }
    }

    /// Selected anchors, the active one first.
    pub fn anchors(&self) -> Vec<Anchor> {
        self.selected.iter().chain(self.also.iter()).copied().collect()
    }

    pub fn holds(&self, a: Anchor) -> bool {
        self.selected == Some(a) || self.also.contains(&a)
    }

    pub fn select_only(&mut self, a: Option<Anchor>) {
        self.selected = a;
        self.also.clear();
    }

    /// Makes a selected anchor the active one, or adds it as the active one.
    pub fn activate(&mut self, a: Anchor) {
        self.also.retain(|x| *x != a);
        if let Some(old) = self.selected.replace(a)
            && old != a
        {
            self.also.insert(0, old);
        }
    }

    /// Shift-click: adds an anchor to the selection or takes it out.
    pub fn toggle(&mut self, a: Anchor) {
        if self.selected == Some(a) {
            self.selected = if self.also.is_empty() { None } else { Some(self.also.remove(0)) };
        } else if let Some(k) = self.also.iter().position(|x| *x == a) {
            self.also.remove(k);
        } else {
            self.activate(a);
        }
    }

    /// Selects `picked` (the first becomes active); `add` keeps what was selected.
    pub fn select_many(&mut self, picked: &[Anchor], add: bool) {
        if !add {
            self.select_only(None);
        }
        for &a in picked.iter().rev() {
            if !self.holds(a) {
                self.activate(a);
            }
        }
    }
}

/// An anchor placed by the pen in artboard space, with its outgoing handle offset.
#[derive(Clone, Copy, Debug)]
pub struct PenPoint {
    pub p: Point,
    pub handle: Vec2,
}

fn pen_outline(points: &[PenPoint], closed: bool) -> Outline {
    let verts = points
        .iter()
        .map(|q| {
            let smooth = q.handle.hypot() > 0.5;
            Vertex { p: q.p, cin: smooth.then(|| q.p - q.handle), cout: smooth.then(|| q.p + q.handle) }
        })
        .collect();
    Outline { verts, closed }
}

const ANCHOR_HIT: f32 = 7.0;
const HANDLE_HIT: f32 = 6.0;

impl App {
    /// Pen: click for a corner, click and drag for a smooth point, click the first point to close,
    /// Enter or double-click to finish an open line, Esc to cancel.
    pub(crate) fn pen_input(&mut self, ui: &egui::Ui, resp: &egui::Response, rect: Rect, view: View, doc: &Document) {
        let pointer = resp.hover_pos();
        let to_doc = |p: Pos2| view.to_doc(rect, p);
        let (shift, free) = ui.input(|i| (i.modifiers.shift, i.modifiers.command));
        let targets = self.pen_targets(doc);
        let snap = self.snap;
        // Where a point clicked at `p` goes: with Shift along an axis or a diagonal from the
        // previous point, otherwise onto anchors, edges and the step (Ctrl: exactly under the pointer).
        let place = |app: &App, p: Pos2| -> crate::guides::PointSnap {
            match app.pen.last() {
                Some(last) if shift => crate::guides::PointSnap { at: round_pt(along_45(last.p, to_doc(p))), ..Default::default() },
                _ => snap.point(&targets, to_doc(p), view.zoom, free),
            }
        };
        if resp.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Crosshair);
        }
        let near_first = |app: &App, s: Pos2| app.pen.len() >= 2 && view.to_screen(rect, app.pen[0].p).distance(s) <= ANCHOR_HIT + 2.0;
        if resp.drag_started()
            && let Some(origin) = ui.input(|i| i.pointer.press_origin())
        {
            if near_first(self, origin) {
                self.finish_pen(true);
                return;
            }
            let at = place(self, origin).at;
            self.pen.push(PenPoint { p: at, handle: Vec2::ZERO });
            self.pen_dragging = true;
        }
        if resp.dragged()
            && self.pen_dragging
            && let (Some(p), Some(last)) = (resp.interact_pointer_pos(), self.pen.last_mut())
        {
            // Shift keeps the handle on an axis or a diagonal.
            last.handle = if shift { along_45(last.p, to_doc(p)) } else { to_doc(p) } - last.p;
        }
        if resp.drag_stopped() {
            self.pen_dragging = false;
        }
        if resp.double_clicked() {
            // The first click of the pair already added the point.
            self.finish_pen(false);
            return;
        }
        if resp.clicked()
            && let Some(p) = resp.interact_pointer_pos()
        {
            if near_first(self, p) {
                self.finish_pen(true);
                return;
            }
            let at = place(self, p).at;
            self.pen.push(PenPoint { p: at, handle: Vec2::ZERO });
        }

        // Preview: the outline so far and a rubber band to the pointer.
        let painter = ui.painter_at(rect);
        let mut line: Vec<PenPoint> = self.pen.clone();
        if let Some(p) = pointer
            && !self.pen_dragging
        {
            // The next point where it would land, with what it sticks to.
            let s = place(self, p);
            if !near_first(self, p) {
                crate::guides::paint_guides(&painter, rect, view, &s.guides);
                if let Some(m) = s.mark {
                    crate::guides::paint_mark(&painter, rect, view, m);
                }
            }
            if !self.pen.is_empty() {
                line.push(PenPoint { p: s.at, handle: Vec2::ZERO });
            }
        }
        if line.len() >= 2 {
            paint_outline(&painter, &pen_outline(&line, false), |p| view.to_screen(rect, p), Stroke::new(1.5, SEL));
        }
        for (i, q) in self.pen.iter().enumerate() {
            let s = view.to_screen(rect, q.p);
            if q.handle.hypot() > 0.5 {
                for h in [q.p - q.handle, q.p + q.handle] {
                    let hs = view.to_screen(rect, h);
                    painter.line_segment([s, hs], Stroke::new(1.0, SEL));
                    painter.circle(hs, 3.0, Color32::WHITE, Stroke::new(1.0, SEL));
                }
            }
            let closing = i == 0 && pointer.is_some_and(|p| near_first(self, p));
            let size = if closing { 10.0 } else { 7.0 };
            painter.rect(
                Rect::from_center_size(s, vec2(size, size)),
                1.0,
                if i == 0 { SEL } else { Color32::WHITE },
                Stroke::new(1.0, SEL),
                StrokeKind::Middle,
            );
        }
        let hint = if self.pen.is_empty() {
            "Клик — угол, протяжка — плавная точка · Ctrl — без привязки"
        } else {
            "Первая точка — замкнуть · Enter — закончить · Esc — отмена · Shift — по оси или под 45°"
        };
        painter.text(rect.center_bottom() - vec2(0.0, 30.0), egui::Align2::CENTER_BOTTOM, hint, theme::ui_font(12.0), theme::MUTED);
    }

    /// Anchors, edges and centres a pen point can stick to; collected again when the scene or
    /// the time changes.
    fn pen_targets(&mut self, doc: &Document) -> std::rc::Rc<crate::guides::Targets> {
        let key = (self.scene_rev, self.time.to_bits());
        match &self.pen_targets {
            Some((k, t)) if *k == key => t.clone(),
            _ => {
                let t = std::rc::Rc::new(self.snap_targets(doc, &[], true));
                self.pen_targets = Some((key, t.clone()));
                t
            }
        }
    }

    /// Turns the pen points into a new path node (needs two points or more).
    pub(crate) fn finish_pen(&mut self, closed: bool) {
        let points = std::mem::take(&mut self.pen);
        self.pen_dragging = false;
        if points.len() < 2 {
            return;
        }
        if !self.can_edit() {
            return;
        }
        // `add_path` keeps the points to a hundredth: where the snapping put them.
        let path = pen_outline(&points, closed).to_bez();
        let (out, id) = add_path(&self.src, &self.scene, &path);
        let (out, ids) = self.put_in_scope(out, std::slice::from_ref(&id));
        let id = ids.into_iter().next().unwrap_or(id);
        if self.apply("create", Some(out)) {
            self.selected = Some(id.clone());
            self.tool = Tool::Select;
            self.status = format!("Добавлен контур «{id}». Enter — редактировать точки");
        }
    }

    /// Starts point editing of the selected path.
    pub(crate) fn begin_point_edit(&mut self) {
        let Some(id) = self.selected.clone() else { return };
        let doc = self.display_doc();
        let Some(node) = doc.node(&id) else { return };
        if node.kind != ShapeKind::Path {
            return;
        }
        if Compound::from_bez(&node.editable_path()).is_none() {
            self.status = "Контур пуст или содержит неподдерживаемую последовательность команд".into();
            return;
        }
        if self.scene.sites.get(&id).is_some_and(|s| s.generated) {
            self.status = format!("«{id}» создан циклом: его контур правится в скрипте");
            return;
        }
        self.point_edit = Some(PointEdit::new(id));
        self.status = "Клик по линии — новая точка · Shift+клик и рамка — несколько точек, у них свои ручки размера и поворота · двойной клик — угол/плавная · Delete — удалить".into();
    }

    /// Point editing on the canvas. Returns false when editing ended and the click should be
    /// handled as usual.
    pub(crate) fn point_edit_input(&mut self, ui: &egui::Ui, resp: &egui::Response, rect: Rect, view: View, doc: &Document) -> bool {
        let Some((node_id, sel)) = self.point_edit.as_ref().map(|e| (e.node.clone(), e.selected)) else { return false };
        let Some(i) = doc.index_of(&node_id) else {
            self.point_edit = None;
            return false;
        };
        let Some(outline) = Compound::from_bez(&doc.nodes[i].editable_path()) else {
            self.point_edit = None;
            return false;
        };
        let world = doc.world_transform(i);
        if world.determinant().abs() < 1e-12 {
            self.status = "Масштаб контура равен нулю: сначала задайте ненулевой масштаб".into();
            return true;
        }
        let sel = sel.filter(|&a| outline.contains(a));
        if let Some(e) = self.point_edit.as_mut() {
            // The outline may have changed under the selection (undo, a clip, the script).
            e.also.retain(|a| outline.contains(*a));
            if sel.is_none() && e.selected.is_some() {
                e.selected = if e.also.is_empty() { None } else { Some(e.also.remove(0)) };
            }
        }
        let sel = self.point_edit.as_ref().and_then(|e| e.selected);
        let shift = ui.input(|i| i.modifiers.shift);
        let screen = |p: Point| view.to_screen(rect, world * doc.map_lattice_point(i, p));
        let local = |s: Pos2| doc.unmap_lattice_point(i, world.inverse() * view.to_doc(rect, s));
        let (alt, free) = ui.input(|i| (i.modifiers.alt, i.modifiers.command));
        // The outline's own space and the artboard.
        let to_art = |p: Point| world * doc.map_lattice_point(i, p);
        let to_own = |p: Point| doc.unmap_lattice_point(i, world.inverse() * p);
        // Several selected points have a frame that stretches and turns them together.
        let picked: Vec<Anchor> = self.point_edit.as_ref().map(|e| e.anchors()).unwrap_or_default();
        let pulling = self.point_edit.as_ref().and_then(|e| match e.grab {
            Some((PointGrab::Frame(pull), _)) => Some(pull),
            _ => None,
        });
        let frame = crate::point_frame::bounds(&outline, &picked).map(|b| (b, crate::point_frame::handles(b, screen)));
        let frame_hit = |s: Pos2| frame.as_ref().and_then(|(_, h)| h.hit(s));

        let hit = |s: Pos2| -> Option<PointGrab> {
            // Handles of the selected anchor and its neighbours come first: they sit on top.
            for (part, entry) in outline.parts.iter().enumerate() {
                let o = &entry.outline;
                for (vertex, v) in o.verts.iter().enumerate() {
                    let at = Anchor { part, vertex };
                    let near_sel =
                        sel.is_some_and(|a| a.part == part && (a.vertex.abs_diff(vertex) <= 1 || (o.closed && a.vertex.abs_diff(vertex) == o.verts.len() - 1)));
                    if near_sel {
                        if v.cin.is_some_and(|c| screen(c).distance(s) <= HANDLE_HIT) {
                            return Some(PointGrab::In(at));
                        }
                        if v.cout.is_some_and(|c| screen(c).distance(s) <= HANDLE_HIT) {
                            return Some(PointGrab::Out(at));
                        }
                    }
                }
            }
            for (part, entry) in outline.parts.iter().enumerate() {
                if let Some(vertex) = entry.outline.verts.iter().position(|v| screen(v.p).distance(s) <= ANCHOR_HIT) {
                    return Some(PointGrab::Anchor { index: Anchor { part, vertex }, from: local(s) });
                }
            }
            None
        };

        if let Some(p) = resp.hover_pos()
            && resp.hovered()
        {
            let turning = pulling.map(|pull| pull.handle == crate::transform_frame::Handle::Rotate);
            let over = if pulling.is_some() || hit(p).is_some() { None } else { frame_hit(p) };
            match (turning, over) {
                (Some(true), _) | (None, Some(crate::transform_frame::Handle::Rotate)) => crate::transform_frame::paint_rotate_cursor(ui.ctx(), p),
                (None, Some(crate::transform_frame::Handle::Size(hx, hy))) => {
                    ui.ctx().set_cursor_icon(frame.as_ref().map_or(egui::CursorIcon::Default, |(_, h)| h.cursor(hx, hy)));
                }
                _ => ui.ctx().set_cursor_icon(if hit(p).is_some() { egui::CursorIcon::Grab } else { egui::CursorIcon::Crosshair }),
            }
        }

        let mut changed: Option<Compound> = None;
        let mut label: Option<String> = None;
        let press = ui.input(|i| i.pointer.press_origin());
        if resp.drag_started()
            && let Some(origin) = press
        {
            let on_frame =
                || frame.as_ref().and_then(|(b, h)| Some(PointGrab::Frame(crate::point_frame::FramePull::new(h.hit(origin)?, *b, local(origin), world))));
            let grab = hit(origin).or_else(on_frame).or_else(|| {
                near_segment(&outline, origin, screen).map(|(part, seg, t)| {
                    let mut o = outline.clone();
                    let k = o.split(part, seg, t);
                    changed = Some(o);
                    PointGrab::Anchor { index: k, from: local(origin) }
                })
            });
            let inserted = changed.is_some();
            let base = changed.clone().unwrap_or_else(|| outline.clone());
            if let Some(e) = self.point_edit.as_mut() {
                match grab {
                    // A point just put on the line, or one outside the selection, is taken alone;
                    // a selected one takes the whole selection with it.
                    Some(PointGrab::Anchor { index, .. }) if inserted || (!shift && !e.holds(index)) => e.select_only(Some(index)),
                    Some(PointGrab::Anchor { index, .. } | PointGrab::In(index) | PointGrab::Out(index)) => e.activate(index),
                    Some(PointGrab::Frame(_)) => {}
                    // From an empty place: a frame over the anchors.
                    None => e.marquee = Some((origin, origin, shift)),
                }
                e.live = grab.map(|_| base.clone());
                e.grab = grab.map(|g| (g, base));
                changed = None; // insertion + drag commits on release as one undo step
            }
            if let Some(PointGrab::Anchor { .. }) = grab
                && let Some((held, base)) = self.point_edit.as_ref().and_then(|e| Some((e.anchors(), e.grab.as_ref()?.1.clone())))
            {
                // A dragged point sticks to other objects, the guides and the points that stay.
                let mut targets = self.snap_targets(doc, &[i], true);
                for (part, entry) in base.parts.iter().enumerate() {
                    for (vertex, v) in entry.outline.verts.iter().enumerate() {
                        if !held.contains(&Anchor { part, vertex }) {
                            targets.add_point(to_art(v.p));
                        }
                    }
                }
                if let Some(e) = self.point_edit.as_mut() {
                    e.targets = Some(std::rc::Rc::new(targets));
                }
            }
        }
        if resp.dragged()
            && let Some(p) = resp.interact_pointer_pos()
            && let Some(m) = self.point_edit.as_mut().and_then(|e| e.marquee.as_mut())
        {
            m.1 = p;
        }
        if resp.dragged()
            && let Some(p) = resp.interact_pointer_pos()
            && let Some((grab, base)) = self.point_edit.as_ref().and_then(|e| e.grab.clone())
        {
            let at = local(p);
            let mut o = base.clone();
            match grab {
                PointGrab::Frame(pull) => {
                    // Shift keeps the proportions or turns by 15°, Alt stretches around the middle.
                    let (map, note) = pull.map(at, world, shift, alt);
                    crate::point_frame::apply(&mut o, &picked, map);
                    label = Some(note);
                }
                PointGrab::Anchor { index, from } => {
                    let was = base.parts[index.part].outline.verts[index.vertex].p;
                    let aim = was + (at - from);
                    let d = if shift {
                        // Along an axis or a diagonal of the artboard from where the point was.
                        self.guides.clear();
                        self.snap_mark = None;
                        round_hundredth(to_own(along_45(to_art(was), to_art(aim)))) - was
                    } else {
                        // Onto an anchor, an edge, a centre or a guide; Ctrl leaves it under the pointer.
                        let targets = self.point_edit.as_ref().and_then(|e| e.targets.clone());
                        let s = match &targets {
                            Some(t) => self.snap.stick(t, to_art(aim), view.zoom, free),
                            None => crate::guides::PointSnap { at: to_art(aim), ..Default::default() },
                        };
                        let stuck = s.stuck[0] || s.stuck[1];
                        let d = if stuck {
                            // What stuck is kept to a hundredth. In an outline that is not turned
                            // the free axis moves by tenths, as a plain drag does.
                            let c = world.as_coeffs();
                            let straight = c[1].abs() < 1e-9 && c[2].abs() < 1e-9;
                            let exact = round_hundredth(to_own(s.at)) - was;
                            let plain = round_vec(exact);
                            Vec2::new(if s.stuck[0] || !straight { exact.x } else { plain.x }, if s.stuck[1] || !straight { exact.y } else { plain.y })
                        } else {
                            round_vec(at - from)
                        };
                        self.guides = s.guides;
                        self.snap_mark = s.mark;
                        d
                    };
                    // Every selected anchor goes the same way, with its handles.
                    for a in self.point_edit.as_ref().map(|e| e.anchors()).unwrap_or_default() {
                        if o.contains(a) {
                            o.parts[a.part].outline.verts[a.vertex].translate(d);
                        }
                    }
                }
                PointGrab::In(k) | PointGrab::Out(k) => {
                    let v = &mut o.parts[k.part].outline.verts[k.vertex];
                    let at = if shift {
                        // Shift: the handle turns by steps of 15°, as seen on the artboard.
                        let (centre, to) = (to_art(v.p), to_art(at));
                        let step = 15f64.to_radians();
                        let angle = ((to - centre).atan2() / step).round() * step;
                        round_hundredth(to_own(centre + Vec2::from_angle(angle) * (to - centre).hypot()))
                    } else {
                        round_pt(at)
                    };
                    // The opposite handle of a smooth point turns with this one and keeps its
                    // length. Handles of a corner go their own ways, and Alt lets any handle go.
                    let linked = !alt && v.kind() != NodeKind::Corner;
                    v.set_handle(matches!(grab, PointGrab::In(_)), at, linked);
                }
            }
            if let Some(e) = self.point_edit.as_mut() {
                e.live = Some(o);
            }
        }
        if resp.drag_stopped()
            && let Some(e) = self.point_edit.as_mut()
        {
            e.grab = None;
            e.targets = None;
            self.guides.clear();
            self.snap_mark = None;
            changed = e.live.take();
            if let Some((a, b, add)) = e.marquee.take() {
                let area = Rect::from_two_pos(a, b);
                let inside: Vec<Anchor> = outline
                    .parts
                    .iter()
                    .enumerate()
                    .flat_map(|(part, entry)| entry.outline.verts.iter().enumerate().map(move |(vertex, v)| (Anchor { part, vertex }, v.p)))
                    .filter(|(_, p)| area.contains(screen(*p)))
                    .map(|(a, _)| a)
                    .collect();
                e.select_many(&inside, add);
                self.status = match e.anchors().len() {
                    0 => "В рамку не попало ни одной точки".into(),
                    n => format!("Выделено точек: {n}. Тяните любую из них или двигайте стрелками"),
                };
            }
        }
        if resp.double_clicked()
            && let Some(p) = resp.interact_pointer_pos()
            && let Some(PointGrab::Anchor { index, .. }) = hit(p)
        {
            let mut o = outline.clone();
            o.parts[index.part].outline.toggle_smooth(index.vertex);
            changed = Some(o);
        } else if resp.clicked()
            && let Some(p) = resp.interact_pointer_pos()
        {
            match hit(p) {
                Some(PointGrab::Anchor { index, .. }) => {
                    if let Some(e) = self.point_edit.as_mut() {
                        if shift {
                            e.toggle(index);
                        } else {
                            e.select_only(Some(index));
                        }
                    }
                }
                Some(_) => {}
                // A click on a handle of the frame changes nothing.
                None if frame_hit(p).is_some() => {}
                None => match near_segment(&outline, p, screen) {
                    Some((part, seg, t)) => {
                        let mut o = outline.clone();
                        let k = o.split(part, seg, t);
                        changed = Some(o);
                        if let Some(e) = self.point_edit.as_mut() {
                            e.select_only(Some(k));
                        }
                    }
                    // A click away from the outline ends editing and selects as usual.
                    None => {
                        self.point_edit = None;
                        return false;
                    }
                },
            }
        }
        let shown = changed.clone().or_else(|| self.point_edit.as_ref().and_then(|e| e.live.clone())).unwrap_or(outline);
        if let Some(o) = changed {
            self.write_outline(&o);
        }

        // Overlay: the outline, handles and anchors.
        let painter = ui.painter_at(rect);
        let sel = self.point_edit.as_ref().and_then(|e| e.selected);
        let picked = self.point_edit.as_ref().map(|e| e.anchors()).unwrap_or_default();
        // The frame is hidden while it is pulled: the points themselves show the result.
        if pulling.is_none()
            && self.point_edit.as_ref().is_some_and(|e| e.marquee.is_none())
            && let Some(b) = crate::point_frame::bounds(&shown, &picked)
        {
            crate::point_frame::paint(&painter, &crate::point_frame::handles(b, screen), SEL);
        }
        for (part, entry) in shown.parts.iter().enumerate() {
            paint_outline(&painter, &entry.outline, screen, Stroke::new(1.5, SEL));
            for (vertex, v) in entry.outline.verts.iter().enumerate() {
                let s = screen(v.p);
                // Only the active contour's handles: holes stay readable in dense shapes.
                if sel.is_some_and(|a| a.part == part) {
                    for c in [v.cin, v.cout].into_iter().flatten() {
                        let cs = screen(c);
                        painter.line_segment([s, cs], Stroke::new(1.0, SEL.gamma_multiply(0.8)));
                        painter.circle(cs, 3.5, Color32::WHITE, Stroke::new(1.0, SEL));
                    }
                }
                let at = Anchor { part, vertex };
                let on = picked.contains(&at);
                painter.rect(Rect::from_center_size(s, vec2(7.0, 7.0)), 1.0, if on { SEL } else { Color32::WHITE }, Stroke::new(1.0, SEL), StrokeKind::Middle);
                if sel == Some(at) && picked.len() > 1 {
                    // The active one among several: the panel shows its numbers.
                    painter.rect_stroke(Rect::from_center_size(s, vec2(12.0, 12.0)), 2.0, Stroke::new(1.0, SEL), StrokeKind::Middle);
                }
            }
        }
        if let Some((a, b, _)) = self.point_edit.as_ref().and_then(|e| e.marquee) {
            painter.rect(Rect::from_two_pos(a, b), 0.0, SEL.gamma_multiply(0.08), Stroke::new(1.0, SEL), StrokeKind::Middle);
        }
        if let Some(text) = label {
            if let Some(p) = resp.interact_pointer_pos() {
                crate::measure::paint_pointer_label(&painter, p, &text);
            }
            self.status = format!("{text} · Shift — пропорции и шаг 15° · Alt — от середины");
        }
        let badge = "ТОЧКИ · Enter — готово";
        let g = painter.layout_no_wrap(badge.into(), theme::semibold(11.0), Color32::WHITE);
        let b = Rect::from_min_size(rect.left_top() + vec2(12.0, if self.anim.is_some() { 38.0 } else { 10.0 }), g.size() + vec2(16.0, 6.0));
        painter.rect_filled(b, 5.0, SEL.gamma_multiply(0.9));
        painter.galley(b.min + vec2(8.0, 3.0), g, Color32::WHITE);
        true
    }

    /// Deletes the selected anchors. A part keeps at least two points; a part whose points are
    /// all selected goes whole, as long as another part stays.
    pub(crate) fn delete_point(&mut self) -> bool {
        let Some(edit) = &self.point_edit else { return false };
        let doc = self.display_doc();
        let Some(node) = doc.node(&edit.node) else { return true };
        let Some(mut o) = Compound::from_bez(&node.editable_path()) else { return true };
        let picked: Vec<Anchor> = edit.anchors().into_iter().filter(|&a| o.contains(a)).collect();
        if picked.is_empty() {
            return true;
        }
        let mut whole: Vec<usize> = Vec::new();
        for part in 0..o.parts.len() {
            let mut gone: Vec<usize> = picked.iter().filter(|a| a.part == part).map(|a| a.vertex).collect();
            gone.sort_unstable();
            gone.dedup();
            let verts = &mut o.parts[part].outline.verts;
            if gone.len() == verts.len() && gone.len() > 2 {
                whole.push(part);
            } else if verts.len() - gone.len() < 2 {
                self.status = "В части контура должно остаться хотя бы две точки. Выделите все её точки, чтобы удалить часть целиком".into();
                return true;
            } else {
                for v in gone.into_iter().rev() {
                    verts.remove(v);
                }
            }
        }
        if whole.len() == o.parts.len() {
            self.status = "Нельзя удалить все точки контура: удалите сам объект (Enter, затем Delete)".into();
            return true;
        }
        for part in whole.into_iter().rev() {
            o.parts.remove(part);
        }
        if let Some(e) = self.point_edit.as_mut() {
            e.select_only(None);
        }
        self.write_outline(&o);
        if picked.len() > 1 {
            self.status = format!("Удалено точек: {}", picked.len());
        }
        true
    }

    /// The kind buttons of the panel: every selected point becomes a corner, smooth or symmetric.
    pub(crate) fn set_points_kind(&mut self, kind: NodeKind) {
        let Some(edit) = &self.point_edit else { return };
        let doc = self.display_doc();
        let Some(mut o) = doc.node(&edit.node).and_then(|n| Compound::from_bez(&n.editable_path())) else { return };
        for a in edit.anchors() {
            if o.contains(a) {
                o.parts[a.part].outline.set_kind(a.vertex, kind);
            }
        }
        self.write_outline(&o);
    }

    /// Ctrl+A while editing points: every anchor of the outline.
    pub(crate) fn select_all_points(&mut self) -> bool {
        let Some(edit) = &self.point_edit else { return false };
        let doc = self.display_doc();
        let Some(o) = doc.node(&edit.node).and_then(|n| Compound::from_bez(&n.editable_path())) else { return true };
        let all: Vec<Anchor> = o.parts.iter().enumerate().flat_map(|(part, p)| (0..p.outline.verts.len()).map(move |vertex| Anchor { part, vertex })).collect();
        if let Some(e) = self.point_edit.as_mut() {
            let active = e.selected;
            e.select_many(&all, false);
            if let Some(a) = active.filter(|a| all.contains(a)) {
                e.activate(a);
            }
        }
        self.status = format!("Выделено точек: {}", all.len());
        true
    }

    /// Arrow keys while editing points: the selected anchors go `d` artboard units. Returns false
    /// when no points are being edited.
    pub(crate) fn nudge_points(&mut self, d: Vec2) -> bool {
        let Some(edit) = &self.point_edit else { return false };
        let doc = self.display_doc();
        let Some(i) = doc.index_of(&edit.node) else { return true };
        let Some(mut o) = Compound::from_bez(&doc.nodes[i].editable_path()) else { return true };
        let picked: Vec<Anchor> = edit.anchors().into_iter().filter(|&a| o.contains(a)).collect();
        let world = doc.world_transform(i);
        if picked.is_empty() || world.determinant().abs() < 1e-12 {
            return true;
        }
        // The step is in artboard units, as the eye sees it; the points live in the outline's own space.
        let c = world.inverse().as_coeffs();
        let own = round_vec(Vec2::new(c[0] * d.x + c[2] * d.y, c[1] * d.x + c[3] * d.y));
        for a in picked {
            o.parts[a.part].outline.verts[a.vertex].translate(own);
        }
        let id = edit.node.clone();
        if self.can_edit() {
            let out = self.outline_edit_source(&id, &o.to_bez());
            self.apply(&format!("{id}.d.nudge"), out);
        }
        true
    }

    /// Inspector for the active contour and anchor, including topology operations.
    pub(crate) fn point_controls(&mut self, ui: &mut egui::Ui, node: &motif_core::Node) {
        let Some(mut shape) = Compound::from_bez(&node.editable_path()) else {
            ui.label("Пустой контур");
            return;
        };
        ui.label(format!("Частей: {} · точек: {}", shape.parts.len(), shape.parts.iter().map(|p| p.outline.verts.len()).sum::<usize>()));
        let editing = self.point_edit.as_ref().is_some_and(|e| e.node == node.id);
        if ui.button(if editing { "Готово (Enter)" } else { "Редактировать точки (Enter)" }).clicked() {
            if editing {
                self.point_edit = None;
            } else {
                self.begin_point_edit();
            }
        }
        if !editing {
            return;
        }
        let selected = self.point_edit.as_ref().and_then(|e| e.selected).filter(|&a| shape.contains(a));
        let mut at = selected.unwrap_or(Anchor { part: 0, vertex: 0 });
        egui::ComboBox::from_id_salt("contour-part").selected_text(format!("Часть {}", at.part + 1)).show_ui(ui, |ui| {
            for (part, p) in shape.parts.iter().enumerate() {
                if ui
                    .selectable_label(
                        part == at.part,
                        format!("{} · {} точ. · {}", part + 1, p.outline.verts.len(), if p.outline.closed { "замкнута" } else { "открыта" }),
                    )
                    .clicked()
                {
                    at = Anchor { part, vertex: 0 };
                    if let Some(e) = &mut self.point_edit {
                        e.select_only(Some(at));
                    }
                }
            }
        });
        let mut discrete = false;
        let mut numeric = false;
        let count = self.point_edit.as_ref().map_or(0, |e| e.anchors().len());
        if count > 1 {
            ui.label(format!("Выделено точек: {count}"));
            let tips = ["Равные шаги по горизонтали (от 3 точек)", "Равные шаги по вертикали (от 3 точек)"];
            if let Some(how) = crate::selection::align_buttons(ui, count, tips) {
                self.align_points(how);
                return;
            }
            ui.label(egui::RichText::new("Числа ниже — у главной точки (обведена).").size(11.5).color(crate::theme::FAINT));
        }
        if selected.is_some() {
            ui.label(format!("Узел {} · локальные координаты", at.vertex + 1));
            let v = &mut shape.parts[at.part].outline.verts[at.vertex];
            let (mut x, mut y) = (v.p.x, v.p.y);
            ui.horizontal(|ui| {
                numeric |= ui.add(egui::DragValue::new(&mut x).prefix("X ").speed(0.1).max_decimals(6)).changed();
                numeric |= ui.add(egui::DragValue::new(&mut y).prefix("Y ").speed(0.1).max_decimals(6)).changed();
            });
            numeric &= x.is_finite() && y.is_finite();
            if numeric {
                v.translate(Point::new(x, y) - v.p);
            }
            // The curve handles of the point, from the point itself: by offset or by length and angle.
            let was = *v;
            let linked = was.kind() != NodeKind::Corner;
            for (incoming, name, handle) in [(true, "Ручка назад", was.cin), (false, "Ручка вперёд", was.cout)] {
                let Some(d) = handle.map(|c| c - was.p) else { continue };
                let (mut dx, mut dy, mut len, mut angle) = (d.x, d.y, d.hypot(), d.atan2().to_degrees());
                let (mut by_offset, mut by_angle) = (false, false);
                ui.label(egui::RichText::new(name).size(11.5).color(crate::theme::FAINT));
                ui.push_id(("handle", incoming), |ui| {
                    ui.horizontal(|ui| {
                        by_offset |= ui.add(egui::DragValue::new(&mut dx).prefix("ΔX ").speed(0.1).max_decimals(2)).changed();
                        by_offset |= ui.add(egui::DragValue::new(&mut dy).prefix("ΔY ").speed(0.1).max_decimals(2)).changed();
                    });
                    ui.horizontal(|ui| {
                        by_angle |= ui.add(egui::DragValue::new(&mut len).prefix("Длина ").speed(0.1).range(0.0..=1e6).max_decimals(2)).changed();
                        by_angle |= ui.add(egui::DragValue::new(&mut angle).prefix("Угол ").suffix("°").speed(0.5).max_decimals(2)).changed();
                    });
                });
                let to = if by_offset {
                    Vec2::new(dx, dy)
                } else if by_angle {
                    Vec2::from_angle(angle.to_radians()) * len
                } else {
                    continue;
                };
                if to.x.is_finite() && to.y.is_finite() {
                    // The other handle of a smooth point stays on one line with this one.
                    v.set_handle(incoming, round_hundredth(was.p + to), linked);
                    numeric = true;
                }
            }
            let kind = was.kind();
            let mut pick = None;
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                for (k, name, tip) in [
                    (NodeKind::Corner, "Угол", "Убрать ручки: линии сходятся углом. Углом считается и точка, ручки которой смотрят в разные стороны"),
                    (NodeKind::Smooth, "Плавный", "Поставить ручки на одну прямую; длины остаются разными"),
                    (NodeKind::Symmetric, "Симметричный", "Поставить ручки на одну прямую и сделать одной длины"),
                ] {
                    if ui.selectable_label(kind == k, name).on_hover_text(tip).clicked() {
                        pick = Some(k);
                    }
                }
            });
            if let Some(kind) = pick {
                self.set_points_kind(kind);
                return;
            }
            if ui.button("Разорвать в узле").clicked() {
                discrete = shape.break_at(at);
                if !discrete {
                    self.status = "Для разрыва открытой части выберите внутренний узел".into();
                }
                if let Some(e) = &mut self.point_edit {
                    e.select_only(None);
                }
            }
            if shape.endpoint(at) {
                let candidates: Vec<_> = shape
                    .parts
                    .iter()
                    .enumerate()
                    .filter(|(part, p)| *part != at.part && !p.outline.closed)
                    .flat_map(|(part, p)| [Anchor { part, vertex: 0 }, Anchor { part, vertex: p.outline.verts.len() - 1 }])
                    .collect();
                ui.menu_button("Соединить с…", |ui| {
                    for b in candidates {
                        if ui.button(format!("Часть {}, {}", b.part + 1, if b.vertex == 0 { "начало" } else { "конец" })).clicked() {
                            if let Some(joined) = shape.join(at, b) {
                                if let Some(e) = &mut self.point_edit {
                                    e.select_only(Some(joined));
                                }
                                discrete = true;
                            }
                            ui.close();
                        }
                    }
                });
            }
        } else {
            ui.small("Выберите узел на холсте для правки координат и разрыва.");
        }
        // Topology buttons operate on the chosen part; no other part is reoriented or rounded.
        if !discrete {
            let closed = shape.parts[at.part].outline.closed;
            if ui.button(if closed { "Открыть часть" } else { "Замкнуть часть" }).clicked() {
                if closed {
                    shape.break_at(Anchor { part: at.part, vertex: 0 });
                } else {
                    shape.parts[at.part].outline.closed = true;
                }
                discrete = true;
            }
            if ui.button("Обратить направление части").on_hover_text("Меняет направление обхода; для отверстия может изменить заливку").clicked()
            {
                shape.reverse(at.part);
                discrete = true;
                if let Some(e) = &mut self.point_edit {
                    e.select_only(None);
                }
            }
            if ui.add_enabled(shape.parts.len() > 1, egui::Button::new("Удалить часть")).clicked() {
                shape.parts.remove(at.part);
                discrete = true;
                if let Some(e) = &mut self.point_edit {
                    e.select_only(None);
                }
            }
        }
        if discrete {
            self.write_outline(&shape);
        } else if numeric && self.can_edit() {
            let out = self.outline_edit_source(&node.id, &shape.to_bez());
            self.apply(&format!("{}.coordinates.{}.{}", node.id, at.part, at.vertex), out);
        }
    }

    pub(crate) fn outline_edit_source(&self, id: &str, path: &BezPath) -> Option<String> {
        match self.key_time() {
            Some((clip, time)) => {
                motif_script::forms::set_contour_key(&self.src, &self.scene, &clip, id, time, path, self.path.as_ref().and_then(|p| p.parent()))
            }
            None => set_path(&self.src, &self.scene, id, path),
        }
    }

    pub(crate) fn write_outline(&mut self, o: &Compound) {
        let Some(id) = self.point_edit.as_ref().map(|e| e.node.clone()) else { return };
        if !self.can_edit() {
            return;
        }
        let out = self.outline_edit_source(&id, &o.to_bez());
        self.apply_once(&format!("{id}.d"), out);
    }
}

/// The nearest segment within a few screen pixels of local point `p`.
fn near_segment(o: &Compound, p: Pos2, screen: impl Fn(Point) -> Pos2) -> Option<(usize, usize, f64)> {
    o.nearest(Point::new(p.x as f64, p.y as f64), |p| {
        let s = screen(p);
        Point::new(s.x as f64, s.y as f64)
    })
}

fn paint_outline(painter: &egui::Painter, o: &Outline, to_screen: impl Fn(Point) -> Pos2, stroke: Stroke) {
    let path = o.to_bez();
    let mut pts: Vec<Pos2> = Vec::new();
    motif_core::kurbo::flatten(path.iter(), 0.25, |el| match el {
        PathEl::MoveTo(p) => pts.push(to_screen(p)),
        PathEl::LineTo(p) => pts.push(to_screen(p)),
        PathEl::ClosePath => {
            if let Some(&first) = pts.first() {
                pts.push(first);
            }
        }
        _ => {}
    });
    if pts.len() >= 2 {
        painter.add(Shape::line(pts, stroke));
    }
}

/// `to` moved onto the nearest axis or diagonal through `from`.
fn along_45(from: Point, to: Point) -> Point {
    let v = to - from;
    let step = std::f64::consts::FRAC_PI_4;
    let a = (v.atan2() / step).round() * step;
    let dir = Vec2::new(a.cos(), a.sin());
    from + dir * v.dot(dir)
}

fn round_pt(p: Point) -> Point {
    Point::new((p.x * 10.0).round() / 10.0, (p.y * 10.0).round() / 10.0)
}

fn round_hundredth(p: Point) -> Point {
    Point::new((p.x * 100.0).round() / 100.0, (p.y * 100.0).round() / 100.0)
}

fn round_vec(v: Vec2) -> Vec2 {
    Vec2::new((v.x * 10.0).round() / 10.0, (v.y * 10.0).round() / 10.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn square() -> BezPath {
        let mut p = BezPath::new();
        p.move_to((0.0, 0.0));
        p.line_to((10.0, 0.0));
        p.line_to((10.0, 10.0));
        p.line_to((0.0, 10.0));
        p.close_path();
        p
    }

    #[test]
    fn the_kind_of_a_point_is_read_from_its_handles() {
        let v = |cin: Option<(f64, f64)>, cout: Option<(f64, f64)>| Vertex {
            p: Point::new(10.0, 10.0),
            cin: cin.map(|(x, y)| Point::new(x, y)),
            cout: cout.map(|(x, y)| Point::new(x, y)),
        };
        assert_eq!(v(None, None).kind(), NodeKind::Corner);
        assert_eq!(v(Some((0.0, 10.0)), None).kind(), NodeKind::Corner);
        assert_eq!(v(Some((0.0, 10.0)), Some((20.0, 10.0))).kind(), NodeKind::Symmetric);
        assert_eq!(v(Some((0.0, 10.0)), Some((15.0, 10.0))).kind(), NodeKind::Smooth);
        // A handle kept to a tenth of a unit is still on the line; a degree off it is not.
        assert_eq!(v(Some((0.0, 10.0)), Some((15.0, 10.1))).kind(), NodeKind::Smooth);
        assert_eq!(v(Some((0.0, 10.0)), Some((30.0, 10.5))).kind(), NodeKind::Corner);
        // Both handles to one side: a cusp.
        assert_eq!(v(Some((0.0, 10.0)), Some((0.0, 10.0))).kind(), NodeKind::Corner);
        assert_eq!(v(Some((0.0, 10.0)), Some((10.0, 0.0))).kind(), NodeKind::Corner);
    }

    #[test]
    fn a_point_is_made_a_corner_smooth_or_symmetric() {
        let mut o = Outline::from_bez(&square()).unwrap();
        let p = o.verts[1].p;
        // No handles: they are added along the neighbours, a third of the way to each.
        o.set_kind(1, NodeKind::Smooth);
        let (a, b) = (o.verts[1].cin.unwrap(), o.verts[1].cout.unwrap());
        assert!(((p - a).cross(b - p)).abs() < 0.5 && (p - a).dot(b - p) > 0.0);
        assert_ne!(o.verts[1].kind(), NodeKind::Corner);
        // A cusp: the handles are put on the line between them and keep their lengths.
        o.verts[1].cin = Some(p + Vec2::new(-30.0, 0.0));
        o.verts[1].cout = Some(p + Vec2::new(0.0, 10.0));
        assert_eq!(o.verts[1].kind(), NodeKind::Corner);
        o.set_kind(1, NodeKind::Smooth);
        let v = o.verts[1];
        assert_eq!(v.kind(), NodeKind::Smooth);
        assert!((v.cin.unwrap().distance(p) - 30.0).abs() < 0.01 && (v.cout.unwrap().distance(p) - 10.0).abs() < 0.01);
        o.set_kind(1, NodeKind::Symmetric);
        let v = o.verts[1];
        assert_eq!(v.kind(), NodeKind::Symmetric);
        assert!((v.cin.unwrap().distance(p) - 20.0).abs() < 0.01 && (v.cout.unwrap().distance(p) - 20.0).abs() < 0.01);
        // One handle: the other is added on its line.
        o.verts[1].cout = None;
        o.set_kind(1, NodeKind::Smooth);
        assert_ne!(o.verts[1].kind(), NodeKind::Corner);
        o.set_kind(1, NodeKind::Corner);
        assert_eq!((o.verts[1].cin, o.verts[1].cout), (None, None));
    }

    #[test]
    fn a_linked_handle_turns_the_opposite_one_and_keeps_its_length() {
        let mut v = Vertex { p: Point::new(0.0, 0.0), cin: Some(Point::new(-10.0, 0.0)), cout: Some(Point::new(4.0, 0.0)) };
        v.set_handle(false, Point::new(0.0, 8.0), true);
        assert_eq!((v.cin, v.cout), (Some(Point::new(0.0, -10.0)), Some(Point::new(0.0, 8.0))));
        v.set_handle(true, Point::new(-3.0, 0.0), false);
        assert_eq!((v.cin, v.cout), (Some(Point::new(-3.0, 0.0)), Some(Point::new(0.0, 8.0))));
    }

    #[test]
    fn outline_round_trips() {
        let o = Outline::from_bez(&square()).unwrap();
        assert_eq!(o.verts.len(), 4);
        assert!(o.closed);
        assert_eq!(o.to_bez(), square());
        let mut curved = BezPath::new();
        curved.move_to((0.0, 0.0));
        curved.curve_to((5.0, -5.0), (10.0, -5.0), (10.0, 0.0));
        curved.curve_to((10.0, 8.0), (0.0, 8.0), (0.0, 0.0));
        curved.close_path();
        let o = Outline::from_bez(&curved).unwrap();
        assert_eq!(o.verts.len(), 2, "the closing curve back to the start is not a new anchor");
        assert_eq!(o.to_bez(), curved);
    }

    #[test]
    fn split_keeps_shape_and_toggle_adds_handles() {
        let mut o = Outline::from_bez(&square()).unwrap();
        let k = o.split(0, 0.5);
        assert_eq!(k, 1);
        assert_eq!(o.verts[1].p, Point::new(5.0, 0.0));
        o.toggle_smooth(1);
        assert!(o.verts[1].cin.is_some() && o.verts[1].cout.is_some());
        o.toggle_smooth(1);
        assert_eq!(o.verts[1], Vertex::corner(Point::new(5.0, 0.0)));
        // A single Outline cannot contain several contours; Compound handles them.
        let mut two = square();
        two.move_to((20.0, 20.0));
        two.line_to((30.0, 20.0));
        assert!(Outline::from_bez(&two).is_none());
    }
}
