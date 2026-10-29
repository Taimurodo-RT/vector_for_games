//! The pen tool and point editing of `path` nodes.
//!
//! Outlines are edited as a list of anchors with optional Bézier handles and written back to the
//! script as SVG path data (`d "M … C … Z"`).

use crate::app::{App, Tool};
use crate::canvas::View;
use crate::compound::{Anchor, Compound};
use crate::theme::{self, SEL};
use egui::{Color32, Pos2, Rect, Shape, Stroke, StrokeKind, vec2};
use motif_core::kurbo::{Affine, BezPath, CubicBez, ParamCurve, PathEl, Point, Vec2};
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
    pub selected: Option<Anchor>,
    pub grab: Option<(PointGrab, Compound)>,
    /// The outline while a point is being dragged; written to the script on release.
    pub live: Option<Compound>,
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
    pub(crate) fn pen_input(&mut self, ui: &egui::Ui, resp: &egui::Response, rect: Rect, view: View) {
        let pointer = resp.hover_pos();
        let to_doc = |p: Pos2| view.to_doc(rect, p);
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
            self.pen.push(PenPoint { p: to_doc(origin), handle: Vec2::ZERO });
            self.pen_dragging = true;
        }
        if resp.dragged()
            && self.pen_dragging
            && let (Some(p), Some(last)) = (resp.interact_pointer_pos(), self.pen.last_mut())
        {
            last.handle = to_doc(p) - last.p;
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
            self.pen.push(PenPoint { p: to_doc(p), handle: Vec2::ZERO });
        }

        // Preview: the outline so far and a rubber band to the pointer.
        let painter = ui.painter_at(rect);
        let mut line: Vec<PenPoint> = self.pen.clone();
        if let Some(p) = pointer
            && !self.pen_dragging
            && !self.pen.is_empty()
        {
            line.push(PenPoint { p: to_doc(p), handle: Vec2::ZERO });
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
            "Клик — угол, протяжка — плавная точка"
        } else {
            "Первая точка — замкнуть · Enter — закончить · Esc — отмена"
        };
        painter.text(rect.center_bottom() - vec2(0.0, 30.0), egui::Align2::CENTER_BOTTOM, hint, theme::ui_font(12.0), theme::MUTED);
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
        let path = pen_outline(&points, closed).to_bez();
        let (out, id) = add_path(&self.src, &self.scene, &round_path(&path));
        if self.apply("create", Some(out)) {
            self.selected = Some(id.clone());
            self.tool = Tool::Select;
            self.status = format!("Добавлен контур «{id}». Enter — редактировать точки");
        }
    }

    /// Starts point editing of the selected path.
    pub(crate) fn begin_point_edit(&mut self) {
        let Some(id) = self.selected.clone() else { return };
        let Some(node) = self.scene.doc.node(&id) else { return };
        if node.kind != ShapeKind::Path {
            return;
        }
        if Compound::from_bez(&node.geometry).is_none() {
            self.status = "Контур пуст или содержит неподдерживаемую последовательность команд".into();
            return;
        }
        if self.scene.sites.get(&id).is_some_and(|s| s.generated) {
            self.status = format!("«{id}» создан циклом: его контур правится в скрипте");
            return;
        }
        self.point_edit = Some(PointEdit { node: id, selected: None, grab: None, live: None });
        self.status = "Клик по линии — новая точка · двойной клик — угол/плавная · Alt — ручки отдельно · Delete — удалить".into();
    }

    /// Point editing on the canvas. Returns false when editing ended and the click should be
    /// handled as usual.
    pub(crate) fn point_edit_input(&mut self, ui: &egui::Ui, resp: &egui::Response, rect: Rect, view: View, doc: &Document) -> bool {
        let Some((node_id, sel)) = self.point_edit.as_ref().map(|e| (e.node.clone(), e.selected)) else { return false };
        let Some(i) = doc.index_of(&node_id) else {
            self.point_edit = None;
            return false;
        };
        let Some(outline) = Compound::from_bez(&doc.nodes[i].geometry) else {
            self.point_edit = None;
            return false;
        };
        let world = doc.world_transform(i);
        if world.determinant().abs() < 1e-12 {
            self.status = "Масштаб контура равен нулю: сначала задайте ненулевой масштаб".into();
            return true;
        }
        let sel = sel.filter(|&a| outline.contains(a));
        let screen = |p: Point| view.to_screen(rect, world * p);
        let local = |s: Pos2| world.inverse() * view.to_doc(rect, s);
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
                None => near_segment(&outline, local(origin), view, world).map(|(part, seg, t)| {
                    let mut o = outline.clone();
                    let k = o.split(part, seg, t);
                    changed = Some(o);
                    PointGrab::Anchor { index: k, from: local(origin) }
                }),
            };
            let base = changed.clone().unwrap_or_else(|| outline.clone());
            if let Some(e) = self.point_edit.as_mut() {
                if let Some(PointGrab::Anchor { index, .. } | PointGrab::In(index) | PointGrab::Out(index)) = grab {
                    e.selected = Some(index);
                }
                e.live = grab.map(|_| base.clone());
                e.grab = grab.map(|g| (g, base));
                changed = None; // insertion + drag commits on release as one undo step
            }
        }
        if resp.dragged()
            && let Some(p) = resp.interact_pointer_pos()
            && let Some((grab, base)) = self.point_edit.as_ref().and_then(|e| e.grab.clone())
        {
            let at = local(p);
            let mut o = base.clone();
            match grab {
                PointGrab::Anchor { index, from } => o.parts[index.part].outline.verts[index.vertex].translate(round_vec(at - from)),
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
                        e.selected = Some(index);
                    }
                }
                Some(_) => {}
                None => match near_segment(&outline, local(p), view, world) {
                    Some((part, seg, t)) => {
                        let mut o = outline.clone();
                        let k = o.split(part, seg, t);
                        changed = Some(o);
                        if let Some(e) = self.point_edit.as_mut() {
                            e.selected = Some(k);
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
                let on = sel == Some(Anchor { part, vertex });
                painter.rect(Rect::from_center_size(s, vec2(7.0, 7.0)), 1.0, if on { SEL } else { Color32::WHITE }, Stroke::new(1.0, SEL), StrokeKind::Middle);
            }
        }
        let badge = "ТОЧКИ · Enter — готово";
        let g = painter.layout_no_wrap(badge.into(), theme::semibold(11.0), Color32::WHITE);
        let b = Rect::from_min_size(rect.left_top() + vec2(12.0, if self.anim.is_some() { 38.0 } else { 10.0 }), g.size() + vec2(16.0, 6.0));
        painter.rect_filled(b, 5.0, SEL.gamma_multiply(0.9));
        painter.galley(b.min + vec2(8.0, 3.0), g, Color32::WHITE);
        true
    }

    /// Deletes the selected anchor; an outline keeps at least two.
    pub(crate) fn delete_point(&mut self) -> bool {
        let Some(edit) = &self.point_edit else { return false };
        let Some(k) = edit.selected else { return true };
        let Some(node) = self.scene.doc.node(&edit.node) else { return true };
        let Some(mut o) = Compound::from_bez(&node.geometry) else { return true };
        if !o.contains(k) {
            return true;
        }
        if o.parts[k.part].outline.verts.len() <= 2 {
            self.status = "В контуре должно остаться хотя бы две точки".into();
            return true;
        }
        o.parts[k.part].outline.verts.remove(k.vertex);
        if let Some(e) = self.point_edit.as_mut() {
            e.selected = None;
        }
        self.write_outline(&o);
        true
    }

    /// Inspector for the active contour and anchor, including topology operations.
    pub(crate) fn point_controls(&mut self, ui: &mut egui::Ui, node: &motif_core::Node) {
        let Some(mut shape) = Compound::from_bez(&node.geometry) else {
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
                        e.selected = Some(at);
                    }
                }
            }
        });
        let mut discrete = false;
        let mut numeric = false;
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
                    e.selected = None;
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
                                    e.selected = Some(joined);
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
                    e.selected = None;
                }
            }
            if ui.add_enabled(shape.parts.len() > 1, egui::Button::new("Удалить часть")).clicked() {
                shape.parts.remove(at.part);
                discrete = true;
                if let Some(e) = &mut self.point_edit {
                    e.selected = None;
                }
            }
        }
        if discrete {
            self.write_outline(&shape);
        } else if numeric && self.can_edit() {
            let out = set_path(&self.src, &self.scene, &node.id, &shape.to_bez());
            self.apply(&format!("{}.coordinates.{}.{}", node.id, at.part, at.vertex), out);
        }
    }

    fn write_outline(&mut self, o: &Compound) {
        let Some(id) = self.point_edit.as_ref().map(|e| e.node.clone()) else { return };
        if !self.can_edit() {
            return;
        }
        let out = set_path(&self.src, &self.scene, &id, &o.to_bez());
        self.apply_once(&format!("{id}.d"), out);
    }
}

/// The nearest segment within a few screen pixels of local point `p`.
fn near_segment(o: &Compound, p: Point, view: View, world: Affine) -> Option<(usize, usize, f64)> {
    let screen = |p: Point| {
        let p = world * p;
        Point::new(p.x * view.zoom as f64, p.y * view.zoom as f64)
    };
    o.nearest(screen(p), screen)
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

fn round_pt(p: Point) -> Point {
    Point::new((p.x * 10.0).round() / 10.0, (p.y * 10.0).round() / 10.0)
}

fn round_vec(v: Vec2) -> Vec2 {
    Vec2::new((v.x * 10.0).round() / 10.0, (v.y * 10.0).round() / 10.0)
}

/// Rounds every coordinate to 0.1 so the script stays readable.
fn round_path(path: &BezPath) -> BezPath {
    let mut out = BezPath::new();
    for el in path.elements() {
        out.push(match *el {
            PathEl::MoveTo(p) => PathEl::MoveTo(round_pt(p)),
            PathEl::LineTo(p) => PathEl::LineTo(round_pt(p)),
            PathEl::QuadTo(a, p) => PathEl::QuadTo(round_pt(a), round_pt(p)),
            PathEl::CurveTo(a, b, p) => PathEl::CurveTo(round_pt(a), round_pt(b), round_pt(p)),
            PathEl::ClosePath => PathEl::ClosePath,
        });
    }
    out
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
