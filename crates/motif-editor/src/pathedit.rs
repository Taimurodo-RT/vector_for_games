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

impl Vertex {
    fn corner(p: Point) -> Self {
        Vertex { p, cin: None, cout: None }
    }

    pub(crate) fn translate(&mut self, d: Vec2) {
        self.p += d;
        self.cin = self.cin.map(|c| c + d);
        self.cout = self.cout.map(|c| c + d);
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
        let n = self.verts.len();
        let v = self.verts[i];
        if v.cin.is_some() || v.cout.is_some() {
            self.verts[i].cin = None;
            self.verts[i].cout = None;
            return;
        }
        let prev = if i > 0 || self.closed { Some(self.verts[(i + n - 1) % n].p) } else { None };
        let next = if i + 1 < n || self.closed { Some(self.verts[(i + 1) % n].p) } else { None };
        let dir = match (prev, next) {
            (Some(a), Some(b)) => b - a,
            (Some(a), None) => v.p - a,
            (None, Some(b)) => b - v.p,
            (None, None) => return,
        };
        if dir.hypot() < 1e-9 {
            return;
        }
        let u = dir.normalize();
        let len = |q: Option<Point>| q.map_or(0.0, |q| q.distance(v.p) / 3.0);
        self.verts[i].cin = prev.map(|_| v.p - u * len(prev).max(1.0));
        self.verts[i].cout = next.map(|_| v.p + u * len(next).max(1.0));
    }
}

/// What the pointer holds while editing points.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PointGrab {
    Anchor { index: Anchor, from: Point },
    In(Anchor),
    Out(Anchor),
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
}

impl PointEdit {
    pub fn new(node: String) -> Self {
        PointEdit { node, selected: None, also: Vec::new(), grab: None, live: None, marquee: None }
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
        self.status = "Клик по линии — новая точка · Shift+клик и рамка — несколько точек · двойной клик — угол/плавная · Delete — удалить".into();
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
        let alt = ui.input(|i| i.modifiers.alt);

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
            let cursor = match hit(p) {
                Some(_) => egui::CursorIcon::Grab,
                None => egui::CursorIcon::Crosshair,
            };
            ui.ctx().set_cursor_icon(cursor);
        }

        let mut changed: Option<Compound> = None;
        let press = ui.input(|i| i.pointer.press_origin());
        if resp.drag_started()
            && let Some(origin) = press
        {
            let grab = match hit(origin) {
                Some(g) => Some(g),
                None => near_segment(&outline, origin, screen).map(|(part, seg, t)| {
                    let mut o = outline.clone();
                    let k = o.split(part, seg, t);
                    changed = Some(o);
                    PointGrab::Anchor { index: k, from: local(origin) }
                }),
            };
            let inserted = changed.is_some();
            let base = changed.clone().unwrap_or_else(|| outline.clone());
            if let Some(e) = self.point_edit.as_mut() {
                match grab {
                    // A point just put on the line, or one outside the selection, is taken alone;
                    // a selected one takes the whole selection with it.
                    Some(PointGrab::Anchor { index, .. }) if inserted || (!shift && !e.holds(index)) => e.select_only(Some(index)),
                    Some(PointGrab::Anchor { index, .. } | PointGrab::In(index) | PointGrab::Out(index)) => e.activate(index),
                    // From an empty place: a frame over the anchors.
                    None => e.marquee = Some((origin, origin, shift)),
                }
                e.live = grab.map(|_| base.clone());
                e.grab = grab.map(|g| (g, base));
                changed = None; // insertion + drag commits on release as one undo step
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
                PointGrab::Anchor { from, .. } => {
                    // Every selected anchor goes the same way, with its handles.
                    let d = round_vec(at - from);
                    for a in self.point_edit.as_ref().map(|e| e.anchors()).unwrap_or_default() {
                        if o.contains(a) {
                            o.parts[a.part].outline.verts[a.vertex].translate(d);
                        }
                    }
                }
                PointGrab::In(k) | PointGrab::Out(k) => {
                    let v = &mut o.parts[k.part].outline.verts[k.vertex];
                    let at = round_pt(at);
                    let (this, other) = if matches!(grab, PointGrab::In(_)) { (&mut v.cin, &mut v.cout) } else { (&mut v.cout, &mut v.cin) };
                    *this = Some(at);
                    // Without Alt the opposite handle mirrors the direction and keeps its length.
                    if !alt && let Some(o) = other.as_mut() {
                        let len = o.distance(v.p);
                        let dir = v.p - at;
                        if dir.hypot() > 1e-9 {
                            *o = round_pt(v.p + dir.normalize() * len);
                        }
                    }
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
            ui.label(format!("Выделено точек: {count}. Числа ниже — у главной (в рамке)"));
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
            if ui.button("Угол / плавный узел").clicked() {
                shape.parts[at.part].outline.toggle_smooth(at.vertex);
                discrete = true;
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

    fn write_outline(&mut self, o: &Compound) {
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
