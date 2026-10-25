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
            p,
        })
    }

    /// One step of `dt` seconds. `pose` is the document as the keys put it now.
    fn step(&mut self, pose: &Document, walls: &[Wall], dt: f64) {
        let s = self.soft;
        let n = self.p.len();
        // A per-frame strength turned into a per-step one, so it does not depend on the step size.
        let per_step = |k: f64| 1.0 - (1.0 - k.clamp(0.0, 1.0)).powf(dt * SIM_FPS);
        let damp = 1.0 - per_step(s.damp);
        let old = self.p.clone();
        let node = &pose.nodes[self.node];
        let pull = pose.world.pull(node, s.gravity);
        for i in 0..n {
            self.v[i] += (pull + pose.world.blow(node, self.p[i])) * dt;
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
        for (q, r) in self.p.iter_mut().zip(&self.rest) {
            let goal = center + rotate(*r, turn);
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
        // Solid shapes push particles out past their edge, strokes included.
        let mut touch = vec![None; n];
        let own = &pose.nodes[self.node];
        let own = if own.stroke.is_some() { own.stroke_width / 2.0 } else { 0.0 };
        for wall in walls {
            let margin = wall.margin + own;
            for (i, q) in self.p.iter_mut().enumerate() {
                let into = inside(&wall.poly, *q);
                let (on, along) = nearest_edge(&wall.poly, *q);
                let gap = (*q - on).hypot();
                if !into && gap >= margin {
                    continue;
                }
                // Outward: away from the edge when outside, towards it when inside.
                let out = if gap > 1e-9 { (if into { on - *q } else { *q - on }) / gap } else { Vec2::new(along.y, -along.x) };
                *q = on + out * margin;
                touch[i] = Some(along);
            }
        }
        for i in 0..n {
            let mut v = (self.p[i] - old[i]) / dt * damp;
            if let Some(along) = touch[i] {
                // Sliding is slowed by friction; pushing into the wall is already undone.
                let t = along * v.dot(along);
                v = t * (1.0 - s.friction.clamp(0.0, 1.0));
            }
            self.v[i] = v;
        }
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
    for _ in 0..steps {
        for b in bodies.iter_mut() {
            b.step(pose, &walls, dt / steps as f64);
        }
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
        let a = Animation { name: "Fall".into(), duration: 3.0, looping: false, tracks: Vec::new() };
        let end = a.pose_with(&d, 3.0, &mut SimCache::default());
        let b = bounds(&end, "ball");
        assert!((b.max_y() - 200.0).abs() < 2.0, "rests on the floor: {b:?}");
        assert!(b.height() > 30.0, "keeps most of its shape: {b:?}");
    }
}
