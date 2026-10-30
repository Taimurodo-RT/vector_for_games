//! Soft bodies: the outline of a shape becomes a ring of particles that keeps its shape and its
//! area, gets pushed around by `solid` shapes and pulled towards where the animation's keys put
//! the shape. The motion is simulated from the start of the animation in fixed steps, so the same
//! time always gives the same pose, and export bakes it into frames like any other animation.

use crate::{Document, SIM_FPS};
use kurbo::{Affine, BezPath, ParamCurveArclen, PathEl, PathSeg, Point, Vec2};

/// Physical settings of a soft body (`soft 0.5 pressure 1 friction 0.3 …`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Soft {
    /// How strongly the body returns to its drawn shape, 0..1.
    pub stiff: f64,
    /// Area kept relative to the drawn one: 1 keeps it, below 1 lets it deflate, above inflates.
    pub pressure: f64,
    /// How much sliding along a solid shape is slowed, 0..1.
    pub friction: f64,
    /// How fast motion dies out, 0..1.
    pub damp: f64,
    /// Downward acceleration, artboard units per second squared.
    pub gravity: f64,
    /// How strongly the body follows its keyed position, 0..1. 0 leaves it to gravity alone.
    pub drive: f64,
}

impl Default for Soft {
    fn default() -> Self {
        Soft { stiff: 0.5, pressure: 1.0, friction: 0.3, damp: 0.05, gravity: 0.0, drive: 0.3 }
    }
}

/// Distance between particles along the outline, in artboard units.
const SPACING: f64 = 5.0;
/// How strongly each step evens out kinks in the outline, 0..1.
const SMOOTH: f64 = 0.5;

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Body {
    node: usize,
    soft: Soft,
    p: Vec<Point>,
    v: Vec<Vec2>,
    /// Particles around the centroid in the drawn pose.
    rest: Vec<Vec2>,
    /// Distances to the next particle in the drawn pose.
    edges: Vec<f64>,
    area: f64,
    /// The centroid in the shape's own space, where the keys pull it.
    local_center: Point,
    /// Rotation of the shape when the simulation started, radians.
    start_angle: f64,
    /// Particles at the start of the current step.
    old: Vec<Point>,
    /// The direction of the solid edge each particle rests on this step, if any.
    touch: Vec<Option<Vec2>>,
}

fn angle(a: Affine) -> f64 {
    let v = a * Point::new(1.0, 0.0) - a * Point::ZERO;
    v.y.atan2(v.x)
}

fn rotate(v: Vec2, a: f64) -> Vec2 {
    let (s, c) = a.sin_cos();
    Vec2::new(v.x * c - v.y * s, v.x * s + v.y * c)
}

/// Points along the first closed outline of `path`, about `SPACING` apart.
fn ring(path: &BezPath, to_world: Affine) -> Vec<Point> {
    let mut out = Vec::new();
    for seg in path.segments() {
        let seg: PathSeg = to_world * seg;
        let n = ((seg.arclen(0.1) / SPACING).ceil() as usize).clamp(1, 400);
        for k in 0..n {
            out.push(kurbo::ParamCurve::eval(&seg, k as f64 / n as f64));
        }
        if out.len() > 2000 {
            break;
        }
    }
    // Only the first piece of the outline: stop where a second one starts.
    let first_close = path.elements().iter().skip(1).position(|e| matches!(e, PathEl::MoveTo(_)));
    if let Some(at) = first_close {
        let mut first = BezPath::new();
        for e in &path.elements()[..=at] {
            first.push(*e);
        }
        return ring(&first, to_world);
    }
    out.dedup_by(|a, b| (*a - *b).hypot() < 1e-6);
    out
}

fn area(p: &[Point]) -> f64 {
    let n = p.len();
    (0..n).map(|i| p[i].x * p[(i + 1) % n].y - p[(i + 1) % n].x * p[i].y).sum::<f64>() / 2.0
}

fn centroid(p: &[Point]) -> Point {
    let sum = p.iter().fold(Vec2::ZERO, |s, q| s + q.to_vec2());
    (sum / p.len().max(1) as f64).to_point()
}

/// A closed smooth curve through `p` (Catmull-Rom as cubic Béziers).
pub(crate) fn smooth(p: &[Point]) -> BezPath {
    let n = p.len();
    let mut out = BezPath::new();
    if n < 3 {
        return out;
    }
    out.move_to(p[0]);
    for i in 0..n {
        let (a, b, c, d) = (p[(i + n - 1) % n], p[i], p[(i + 1) % n], p[(i + 2) % n]);
        out.curve_to(b + (c - a) / 6.0, c - (d - b) / 6.0, c);
    }
    out.close_path();
    out
}

/// A solid shape's outline on the artboard, and half its stroke: bodies stop at the stroke's edge.
#[derive(Clone)]
pub(crate) struct Wall {
    pub(crate) poly: Vec<Point>,
    pub(crate) margin: f64,
    /// The shape it is made from.
    pub(crate) node: usize,
}

/// Solid shapes as closed polygons on the artboard.
pub(crate) fn solids(doc: &Document) -> Vec<Wall> {
    (0..doc.nodes.len())
        .filter(|&i| doc.nodes[i].solid && doc.is_visible(i))
        .filter_map(|i| {
            let n = &doc.nodes[i];
            let margin = if n.stroke.is_some() { n.stroke_width / 2.0 } else { 0.0 };
            let path = doc.world_transform(i) * n.path()?;
            let mut polys = Vec::new();
            let mut cur = Vec::new();
            kurbo::flatten(&path, 0.25, |el| match el {
                PathEl::MoveTo(p) => {
                    if cur.len() > 2 {
                        polys.push(std::mem::take(&mut cur));
                    }
                    cur = vec![p];
                }
                PathEl::LineTo(p) => cur.push(p),
                _ => {}
            });
            if cur.len() > 2 {
                polys.push(cur);
            }
            Some(polys.into_iter().map(move |poly| Wall { poly, margin, node: i }))
        })
        .flatten()
        .collect()
}

pub(crate) fn inside(poly: &[Point], p: Point) -> bool {
    let n = poly.len();
    let mut c = false;
    for i in 0..n {
        let (a, b) = (poly[i], poly[(i + 1) % n]);
        if (a.y > p.y) != (b.y > p.y) && p.x < a.x + (p.y - a.y) / (b.y - a.y) * (b.x - a.x) {
            c = !c;
        }
    }
    c
}

/// The point of the polygon's edge nearest to `p`, and the edge's direction there.
pub(crate) fn nearest_edge(poly: &[Point], p: Point) -> (Point, Vec2) {
    let n = poly.len();
    let mut best = (f64::MAX, poly[0], Vec2::new(1.0, 0.0));
    for i in 0..n {
        let (a, b) = (poly[i], poly[(i + 1) % n]);
        let ab = b - a;
        let t = if ab.hypot2() > 0.0 { ((p - a).dot(ab) / ab.hypot2()).clamp(0.0, 1.0) } else { 0.0 };
        let q = a + ab * t;
        let d = (p - q).hypot2();
        if d < best.0 {
            best = (d, q, ab.normalize());
        }
    }
    (best.1, best.2)
}

impl Body {
    fn new(doc: &Document, node: usize, soft: Soft) -> Option<Body> {
        let n = &doc.nodes[node];
        let w = doc.world_transform(node);
        let p = ring(&n.base_path()?, w);
        if p.len() < 3 {
            return None;
        }
        let c = centroid(&p);
        let count = p.len();
        Some(Body {
            node,
            soft,
            rest: p.iter().map(|q| *q - c).collect(),
            edges: (0..count).map(|i| (p[(i + 1) % count] - p[i]).hypot()).collect(),
            area: area(&p),
            local_center: w.inverse() * c,
            start_angle: angle(w),
            v: vec![Vec2::ZERO; count],
            old: p.clone(),
            touch: vec![None; count],
            p,
        })
    }

    /// Moves the particles on by `dt` seconds and pulls them back into shape; collisions and
    /// velocities come after, in [`step_bodies`].
    fn solve(&mut self, pose: &Document, walls: &[Wall], rim: f64, dt: f64) {
        let s = self.soft;
        let n = self.p.len();
        let per_step = |k: f64| per_step(k, dt);
        let old = self.p.clone();
        let node = &pose.nodes[self.node];
        let pull = pose.world.pull(node, s.gravity);
        let me = self.node;
        for i in 0..n {
            self.v[i] += (pull + pose.push(me, self.p[i])) * dt;
            self.p[i] += self.v[i] * dt;
        }
        // Keep the drawn shape: best-fitting rotation of the rest pose around the centroid.
        let c = centroid(&self.p);
        let (mut dot, mut cross) = (0.0, 0.0);
        for (q, r) in self.p.iter().zip(&self.rest) {
            let d = *q - c;
            dot += d.dot(*r);
            cross += r.cross(d);
        }
        let mut turn = cross.atan2(dot);
        let mut center = c;
        // Follow the keys: pull the centroid and the turn towards the keyed pose.
        let w = pose.world_transform(self.node);
        let drive = per_step(s.drive);
        if drive > 0.0 {
            center += (w * self.local_center - c) * drive;
            let want = angle(w) - self.start_angle;
            let diff = (want - turn + std::f64::consts::PI).rem_euclid(std::f64::consts::TAU) - std::f64::consts::PI;
            turn += diff * drive;
            for q in &mut self.p {
                *q += center - c;
            }
        }
        let stiff = per_step(s.stiff);
        // Walls near the body: a part of the drawn shape that lies inside one is wanted at the
        // wall's nearest edge instead, so the body flows round corners rather than hooking on them.
        let size = self.rest.iter().map(|r| r.hypot()).fold(0.0, f64::max);
        let reach = self.bounds().inflate(size, size);
        let near: Vec<&Wall> = walls.iter().filter(|w| overlaps(reach, poly_bounds(&w.poly), w.margin + rim)).collect();
        for (q, r) in self.p.iter_mut().zip(&self.rest) {
            let mut goal = center + rotate(*r, turn);
            for w in &near {
                if inside(&w.poly, goal) {
                    let (on, along) = nearest_edge(&w.poly, goal);
                    let gap = (goal - on).hypot();
                    let out = if gap > 1e-9 { (on - goal) / gap } else { Vec2::new(along.y, -along.x) };
                    goal = on + out * (w.margin + rim);
                }
            }
            *q += (goal - *q) * stiff;
        }
        // Even spacing along the outline.
        for i in 0..n {
            let j = (i + 1) % n;
            let d = self.p[j] - self.p[i];
            let len = d.hypot();
            if len > 1e-9 {
                let fix = d * ((len - self.edges[i]) / len * 0.5 * (0.2 + 0.8 * stiff));
                self.p[i] += fix;
                self.p[j] -= fix;
            }
        }
        // A smooth outline: each particle keeps its drawn bend relative to its two neighbours,
        // so a dent spreads into a round dimple instead of a kink, like surface tension.
        let mut fix = vec![Vec2::ZERO; n];
        for i in 0..n {
            let (a, b) = ((i + n - 1) % n, (i + 1) % n);
            let bend = self.rest[i] - (self.rest[a] + self.rest[b]) / 2.0;
            let want = (self.p[a].midpoint(self.p[b])) + rotate(bend, turn);
            let d = (want - self.p[i]) * (SMOOTH / 3.0);
            fix[i] += d * 2.0;
            fix[a] -= d;
            fix[b] -= d;
        }
        for (q, f) in self.p.iter_mut().zip(&fix) {
            *q += *f;
        }
        // Keep the area: move every particle along the gradient of the area.
        let grad: Vec<Vec2> = (0..n)
            .map(|i| {
                let d = self.p[(i + 1) % n] - self.p[(i + n - 1) % n];
                Vec2::new(d.y, -d.x) / 2.0
            })
            .collect();
        let norm: f64 = grad.iter().map(|g| g.hypot2()).sum();
        if norm > 1e-12 {
            let lambda = (self.area * s.pressure - area(&self.p)) / norm;
            for (q, g) in self.p.iter_mut().zip(&grad) {
                *q += *g * lambda;
            }
        }
        self.old = old;
    }

    /// Half the stroke: the drawn edge, not the particles, is what touches other shapes.
    fn rim(&self, pose: &Document) -> f64 {
        let n = &pose.nodes[self.node];
        if n.stroke.is_some() { n.stroke_width / 2.0 } else { 0.0 }
    }

    /// Solid shapes push the body out past their edge, strokes included: particles out of the
    /// shapes, and the outline between particles off the shapes' corners.
    fn hit_walls(&mut self, walls: &[Wall], rim: f64) {
        let n = self.p.len();
        let bounds = self.bounds();
        for wall in walls {
            let margin = wall.margin + rim;
            if !overlaps(bounds, poly_bounds(&wall.poly), margin) {
                continue;
            }
            for i in 0..n {
                let q = self.p[i];
                let (on, out, along) = match crossing(&wall.poly, self.old[i], q) {
                    // It went into the shape this step: back out the side it came in by, even when
                    // the far side is nearer (a thin wall, a sharp tip).
                    Some((on, along)) => {
                        let mut out = Vec2::new(along.y, -along.x);
                        if out.dot(self.old[i] - on) < 0.0 {
                            out = -out;
                        }
                        (on, out, along)
                    }
                    None => {
                        let into = inside(&wall.poly, q);
                        let (on, along) = nearest_edge(&wall.poly, q);
                        let gap = (q - on).hypot();
                        if !into && gap >= margin {
                            continue;
                        }
                        let out = if gap > 1e-9 { (if into { on - q } else { q - on }) / gap } else { Vec2::new(along.y, -along.x) };
                        (on, out, along)
                    }
                };
                self.p[i] = on + out * margin;
                self.touch[i] = Some(along);
            }
            // Corners of the shape that poke into the outline between two particles.
            for &w in &wall.poly {
                if !bounds.inflate(margin, margin).contains(w) {
                    continue;
                }
                if let Some((i, t, push)) = self.dent(w, margin) {
                    let j = (i + 1) % n;
                    shift_edge(&mut self.p, i, t, push);
                    // The particles beside the edge give way a little too, for a round dimple
                    // instead of a notch.
                    for (k, share) in [(1, 0.4), (2, 0.15)] {
                        self.p[(i + n - k) % n] += push * share;
                        self.p[(j + k) % n] += push * share;
                    }
                    let along = (self.p[j] - self.p[i]).normalize();
                    self.touch[i] = Some(along);
                    self.touch[j] = Some(along);
                }
            }
        }
    }

    /// Where the outline must move so that point `w` ends up `margin` outside it: the edge
    /// (from particle `i`), the place along it and the shift. None when `w` is clear already.
    fn dent(&self, w: Point, margin: f64) -> Option<(usize, f64, Vec2)> {
        let (i, t, c) = nearest_on_ring(&self.p, w);
        let into = inside(&self.p, w);
        let gap = (w - c).hypot();
        if !into && gap >= margin {
            return None;
        }
        // Into the body, from the point.
        let deeper = if gap > 1e-9 {
            (if into { w - c } else { c - w }) / gap
        } else {
            let d = self.p[(i + 1) % self.p.len()] - self.p[i];
            let n = Vec2::new(-d.y, d.x).normalize();
            if inside(&self.p, c + n * 0.01) { n } else { -n }
        };
        Some((i, t, w + deeper * margin - c))
    }

    fn bounds(&self) -> kurbo::Rect {
        poly_bounds(&self.p)
    }

    /// Velocities from this step's motion; sliding along a solid shape is slowed by friction.
    fn finish(&mut self, dt: f64) {
        let s = self.soft;
        let damp = 1.0 - per_step(s.damp, dt);
        // Per frame, like the other strengths, so that more steps do not mean more grip.
        let keep = 1.0 - per_step(s.friction, dt);
        for i in 0..self.p.len() {
            let mut v = (self.p[i] - self.old[i]) / dt * damp;
            if let Some(along) = self.touch[i].take() {
                // Pushing into the wall is already undone.
                v = along * v.dot(along) * keep;
            }
            self.v[i] = v;
        }
    }
}

/// A per-frame strength turned into a per-step one, so it does not depend on the step size.
fn per_step(k: f64, dt: f64) -> f64 {
    1.0 - (1.0 - k.clamp(0.0, 1.0)).powf(dt * SIM_FPS)
}

fn poly_bounds(p: &[Point]) -> kurbo::Rect {
    p.iter().fold(kurbo::Rect::new(f64::MAX, f64::MAX, f64::MIN, f64::MIN), |r, q| r.union_pt(*q))
}

fn overlaps(a: kurbo::Rect, b: kurbo::Rect, margin: f64) -> bool {
    a.x0 - margin <= b.x1 && b.x0 - margin <= a.x1 && a.y0 - margin <= b.y1 && b.y0 - margin <= a.y1
}

/// The first edge of `poly` that the move from `from` to `to` crosses going in, when `from` is
/// outside: the crossing point and the edge's direction.
fn crossing(poly: &[Point], from: Point, to: Point) -> Option<(Point, Vec2)> {
    if (to - from).hypot2() < 1e-18 || inside(poly, from) {
        return None;
    }
    let d = to - from;
    let n = poly.len();
    let mut best: Option<(f64, Point, Vec2)> = None;
    for i in 0..n {
        let (a, b) = (poly[i], poly[(i + 1) % n]);
        let e = b - a;
        let den = d.cross(e);
        // Moves that only touch the edge, or start on it, do not count: a particle resting on a
        // shape must stay free to slide along it.
        if den.abs() < 1e-12 || (from - a).cross(e).abs() / e.hypot().max(1e-12) < 1e-6 {
            continue;
        }
        let s = (a - from).cross(e) / den;
        let u = (a - from).cross(d) / den;
        if (0.0..=1.0).contains(&s) && (0.0..=1.0).contains(&u) && best.is_none_or(|b| s < b.0) {
            best = Some((s, from + d * s, e.normalize()));
        }
    }
    best.map(|(_, p, e)| (p, e))
}

/// The nearest point of the closed outline through `p` to `w`: the edge (from particle `i`),
/// the place along it, and the point.
fn nearest_on_ring(p: &[Point], w: Point) -> (usize, f64, Point) {
    let n = p.len();
    let mut best = (f64::MAX, 0, 0.0, p[0]);
    for i in 0..n {
        let (a, b) = (p[i], p[(i + 1) % n]);
        let ab = b - a;
        let t = if ab.hypot2() > 0.0 { ((w - a).dot(ab) / ab.hypot2()).clamp(0.0, 1.0) } else { 0.0 };
        let q = a + ab * t;
        let d = (w - q).hypot2();
        if d < best.0 {
            best = (d, i, t, q);
        }
    }
    (best.1, best.2, best.3)
}

/// Moves the point at `t` along the edge from particle `i` by `by`, sharing the move between
/// the edge's two particles by how near each one is.
fn shift_edge(p: &mut [Point], i: usize, t: f64, by: Vec2) {
    let j = (i + 1) % p.len();
    let (a, b) = (1.0 - t, t);
    let s = a * a + b * b;
    p[i] += by * (a / s);
    p[j] += by * (b / s);
}

/// Where two soft bodies touch: a particle of one against an edge of the other.
struct Contact {
    /// The body and particle that hit.
    a: usize,
    i: usize,
    /// The body and edge it hit, and the place along it.
    b: usize,
    j: usize,
    t: f64,
}

/// Keeps soft bodies out of each other: each body's outline acts as a wall for the others,
/// and both sides give way by half.
fn hit_each_other(bodies: &mut [Body], rims: &[f64], contacts: &mut Vec<Contact>) {
    let bounds: Vec<kurbo::Rect> = bodies.iter().map(Body::bounds).collect();
    for a in 0..bodies.len() {
        for b in 0..bodies.len() {
            let margin = rims[a] + rims[b];
            if a == b || !overlaps(bounds[a], bounds[b], margin) {
                continue;
            }
            for i in 0..bodies[a].p.len() {
                let q = bodies[a].p[i];
                if !bounds[b].inflate(margin, margin).contains(q) {
                    continue;
                }
                let Some((j, t, push)) = bodies[b].dent(q, margin) else { continue };
                bodies[a].p[i] -= push * 0.5;
                shift_edge(&mut bodies[b].p, j, t, push * 0.5);
                contacts.push(Contact { a, i, b, j, t });
            }
        }
    }
}

/// Friction between soft bodies that touch: slows their sliding against each other.
fn rub(bodies: &mut [Body], contacts: &[Contact], dt: f64) {
    for c in contacts {
        let n = bodies[c.b].p.len();
        let k = (c.j + 1) % n;
        let keep = 1.0 - per_step((bodies[c.a].soft.friction + bodies[c.b].soft.friction) / 2.0, dt);
        let along = (bodies[c.b].p[k] - bodies[c.b].p[c.j]).normalize();
        let edge_v = bodies[c.b].v[c.j] * (1.0 - c.t) + bodies[c.b].v[k] * c.t;
        let slide = along * (bodies[c.a].v[c.i] - edge_v).dot(along);
        // Each side takes half of the slowdown.
        let fix = slide * ((1.0 - keep) * 0.5);
        bodies[c.a].v[c.i] -= fix;
        let (x, y) = (1.0 - c.t, c.t);
        let s = x * x + y * y;
        bodies[c.b].v[c.j] += fix * (x / s);
        bodies[c.b].v[k] += fix * (y / s);
    }
}

/// Soft bodies of `pose`, at rest in the shapes they are drawn as.
pub(crate) fn bodies(pose: &Document) -> Vec<Body> {
    (0..pose.nodes.len()).filter_map(|i| Body::new(pose, i, pose.nodes[i].soft?)).collect()
}

/// Moves `bodies` on by `dt` seconds towards `pose`, in `steps` equal steps.
pub(crate) fn step_bodies(bodies: &mut [Body], pose: &Document, dt: f64, steps: usize) {
    if bodies.is_empty() {
        return;
    }
    let walls = solids(pose);
    let rims: Vec<f64> = bodies.iter().map(|b| b.rim(pose)).collect();
    let h = dt / steps as f64;
    let mut contacts = Vec::new();
    for _ in 0..steps {
        for (b, rim) in bodies.iter_mut().zip(&rims) {
            b.solve(pose, &walls, *rim, h);
        }
        contacts.clear();
        hit_each_other(bodies, &rims, &mut contacts);
        // Walls last, so that no body is left inside one.
        for (b, rim) in bodies.iter_mut().zip(&rims) {
            b.hit_walls(&walls, *rim);
        }
        for b in bodies.iter_mut() {
            b.finish(h);
        }
        rub(bodies, &contacts, h);
    }
}

/// Bends soft bodies of `doc` to `bodies`, read in the space of the posed document.
pub(crate) fn apply_bodies(bodies: &[Body], doc: &mut Document) {
    for b in bodies {
        if b.node >= doc.nodes.len() {
            continue;
        }
        let to_local = doc.world_transform(b.node).inverse();
        let local: Vec<Point> = b.p.iter().map(|q| to_local * *q).collect();
        doc.nodes[b.node].deformed = Some(smooth(&local));
    }
}

/// Bounds of a soft body's particles, for tests.
impl crate::sim::Carry for Body {
    fn node(&self) -> usize {
        self.node
    }
    /// Only a body of as many particles: one reshaped otherwise starts again as drawn.
    fn carry(&mut self, old: &Self) {
        if self.p.len() == old.p.len() {
            (self.p, self.v, self.old, self.touch, self.start_angle) = (old.p.clone(), old.v.clone(), old.old.clone(), old.touch.clone(), old.start_angle);
        }
    }
}

#[cfg(test)]
fn bounds(doc: &Document, node: &str) -> kurbo::Rect {
    use kurbo::Shape;
    let i = doc.index_of(node).unwrap();
    (doc.world_transform(i) * doc.nodes[i].path().unwrap()).bounding_box()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Animation, Artboard, Keyframe, Node, Prop, ShapeKind, SimCache, Track, Value};

    /// A ball of diameter 60 pulled from x 60 to x 340 through a tunnel 36 high at x 160..240.
    fn tunnel() -> (Document, Animation) {
        let mut d = Document::new(Artboard::default());
        let mut ball = Node::new("ball", ShapeKind::Ellipse);
        (ball.x, ball.y, ball.width, ball.height) = (60.0, 150.0, 60.0, 60.0);
        ball.soft = Some(Soft { stiff: 0.15, drive: 0.5, ..Soft::default() });
        d.nodes.push(ball);
        for (id, y) in [("top", 100.0), ("bottom", 200.0)] {
            let mut w = Node::new(id, ShapeKind::Rect);
            (w.x, w.y, w.width, w.height) = (200.0, y, 80.0, 64.0);
            w.solid = true;
            d.nodes.push(w);
        }
        let key = |time, v| Keyframe { time, value: Value::Num(v), ease: crate::Ease::Linear };
        let a = Animation {
            name: "Go".into(),
            duration: 3.0,
            looping: false,
            tracks: vec![Track { node: "ball".into(), prop: Prop::X, keys: vec![key(0.0, 60.0), key(3.0, 340.0)] }],
            events: Vec::new(),
        };
        (d, a)
    }

    #[test]
    fn a_ball_squeezes_through_a_tunnel() {
        let (d, a) = tunnel();
        let mut cache = SimCache::default();
        // In the middle of the tunnel the ball is flattened to its height.
        let mid = a.pose_with(&d, 1.5, &mut cache);
        let b = bounds(&mid, "ball");
        assert!(b.height() < 40.0 && b.width() > 70.0, "{b:?}");
        assert!(b.min_y() > 130.0 && b.max_y() < 170.0, "{b:?}");
        // Out the other side it is round again.
        let end = a.pose_with(&d, 3.0, &mut cache);
        let b = bounds(&end, "ball");
        assert!((b.height() - 60.0).abs() < 8.0 && (b.width() - 60.0).abs() < 8.0, "{b:?}");
        assert!(b.center().x > 280.0, "{b:?}");
        // The cache gives the same answer as starting over.
        let fresh = a.pose_with(&d, 1.5, &mut SimCache::default());
        assert_eq!(bounds(&fresh, "ball"), bounds(&mid, "ball"));
    }

    #[test]
    fn gravity_drops_a_body_onto_a_floor() {
        let mut d = Document::new(Artboard::default());
        let mut ball = Node::new("ball", ShapeKind::Ellipse);
        (ball.x, ball.y, ball.width, ball.height) = (100.0, 50.0, 40.0, 40.0);
        ball.soft = Some(Soft { gravity: 900.0, drive: 0.0, ..Soft::default() });
        let mut floor = Node::new("floor", ShapeKind::Rect);
        (floor.x, floor.y, floor.width, floor.height) = (100.0, 220.0, 300.0, 40.0);
        floor.solid = true;
        d.nodes = vec![ball, floor];
        let a = Animation { name: "Fall".into(), duration: 3.0, looping: false, tracks: Vec::new(), events: Vec::new() };
        let end = a.pose_with(&d, 3.0, &mut SimCache::default());
        let b = bounds(&end, "ball");
        assert!((b.max_y() - 200.0).abs() < 2.0, "rests on the floor: {b:?}");
        assert!(b.height() > 30.0, "keeps most of its shape: {b:?}");
    }

    /// The drawn outline of `node` on the artboard, as a polygon.
    fn outline(doc: &Document, node: &str) -> Vec<Point> {
        let i = doc.index_of(node).unwrap();
        let mut out = Vec::new();
        kurbo::flatten(doc.world_transform(i) * doc.nodes[i].path().unwrap(), 0.1, |el| {
            if let PathEl::MoveTo(p) | PathEl::LineTo(p) = el {
                out.push(p);
            }
        });
        out
    }

    fn ball(id: &str, x: f64, y: f64, size: f64) -> Node {
        let mut b = Node::new(id, ShapeKind::Ellipse);
        (b.x, b.y, b.width, b.height) = (x, y, size, size);
        b.soft = Some(Soft { stiff: 0.3, gravity: 900.0, drive: 0.0, ..Soft::default() });
        b
    }

    #[test]
    fn a_sharp_tip_dents_the_body_instead_of_poking_through() {
        let mut d = Document::new(Artboard::default());
        let mut spike = Node::new("spike", ShapeKind::Polygon);
        (spike.x, spike.y, spike.width, spike.height) = (200.0, 240.0, 80.0, 70.0);
        spike.solid = true;
        d.nodes = vec![ball("ball", 200.0, 120.0, 70.0), spike];
        let a = Animation { name: "Fall".into(), duration: 1.0, looping: false, tracks: Vec::new(), events: Vec::new() };
        let mut cache = SimCache::default();
        let tip = Point::new(200.0, 205.0);
        let mut touched = false;
        for k in 1..=30 {
            let pose = a.pose_with(&d, k as f64 / 30.0, &mut cache);
            let body = outline(&pose, "ball");
            let wall = outline(&pose, "spike");
            assert!((wall[0] - tip).hypot() < 1e-6, "{:?}", wall[0]);
            let poke = if inside(&body, tip) { (tip - nearest_edge(&body, tip).0).hypot() } else { 0.0 };
            assert!(poke < 0.5, "the tip pokes {poke} into the outline at frame {k}");
            let deepest = body.iter().filter(|q| inside(&wall, **q)).map(|q| (*q - nearest_edge(&wall, *q).0).hypot()).fold(0.0, f64::max);
            assert!(deepest < 0.5, "the outline is {deepest} deep in the spike at frame {k}");
            touched |= body.iter().any(|q| (*q - tip).hypot() < 1.0);
        }
        assert!(touched, "the ball reached the tip");
    }

    #[test]
    fn a_soft_body_falling_on_another_rolls_off_it() {
        let mut d = Document::new(Artboard::default());
        let mut floor = Node::new("floor", ShapeKind::Rect);
        (floor.x, floor.y, floor.width, floor.height) = (200.0, 260.0, 400.0, 40.0);
        floor.solid = true;
        d.nodes = vec![ball("low", 200.0, 180.0, 60.0), ball("high", 205.0, 80.0, 60.0), floor];
        let a = Animation { name: "Fall".into(), duration: 2.0, looping: false, tracks: Vec::new(), events: Vec::new() };
        let mut cache = SimCache::default();
        for k in 1..=20 {
            let pose = a.pose_with(&d, k as f64 / 10.0, &mut cache);
            let (low, high) = (outline(&pose, "low"), outline(&pose, "high"));
            let deep = |a: &[Point], b: &[Point]| a.iter().filter(|q| inside(b, **q)).map(|q| (*q - nearest_edge(b, *q).0).hypot()).fold(0.0, f64::max);
            let overlap = deep(&low, &high).max(deep(&high, &low));
            assert!(overlap < 1.0, "the balls overlap by {overlap} at {}s", k as f64 / 10.0);
        }
        let end = a.pose_with(&d, 2.0, &mut cache);
        let (low, high) = (bounds(&end, "low"), bounds(&end, "high"));
        // Both end up on the floor, side by side.
        assert!((low.max_y() - 240.0).abs() < 2.0 && (high.max_y() - 240.0).abs() < 2.0, "{low:?} {high:?}");
        assert!(high.min_x() > low.max_x() - 1.0, "{low:?} {high:?}");
    }
}
