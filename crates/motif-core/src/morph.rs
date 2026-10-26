//! Morphing: a shape flows into other shapes (`morph star heart`), driven by a keyable amount:
//! 0 is the shape as drawn, 1 the first target, 2 the second. The outlines are matched point
//! by point: each piece of one outline is paired with a piece of the other, both are walked
//! the same way round from the best-fitting start, and sharp corners are kept exactly. Fill,
//! stroke colour and stroke width blend along.

use crate::{Color, Document};
use kurbo::{BezPath, PathEl, Point, Vec2};

/// What a shape morphs into, and how far.
#[derive(Clone, Debug, PartialEq)]
pub struct Morph {
    /// Shapes to flow into, by id, in order.
    pub targets: Vec<String>,
    /// 0 the shape as drawn, 1 the first target, 2 the second, and fractions in between.
    pub amount: f64,
    /// The shape's own paint before blending, kept so that blending again starts from it.
    pub(crate) base: Option<Paint>,
    /// The blended outline, set by `Document::morph_all`.
    pub(crate) path: Option<BezPath>,
}

impl Morph {
    pub fn new(targets: Vec<String>) -> Morph {
        Morph { targets, amount: 0.0, base: None, path: None }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Paint {
    fill: Option<Color>,
    stroke: Option<Color>,
    stroke_width: f64,
}

/// Distance between matched points along an outline, in the shape's units, and their count.
const SPACING: f64 = 1.5;
const MIN_POINTS: usize = 48;
const MAX_POINTS: usize = 800;
/// Turns sharper than this (radians) are corners, kept exactly in the blend.
const CORNER: f64 = 0.35;

/// One piece of an outline as a polyline.
#[derive(Clone, Debug)]
struct Ring {
    pts: Vec<Point>,
    closed: bool,
}

impl Ring {
    fn length(&self) -> f64 {
        let n = self.pts.len();
        let open: f64 = self.pts.windows(2).map(|w| (w[1] - w[0]).hypot()).sum();
        if self.closed && n > 1 { open + (self.pts[0] - self.pts[n - 1]).hypot() } else { open }
    }

    fn area(&self) -> f64 {
        let p = &self.pts;
        let n = p.len();
        (0..n).map(|i| p[i].to_vec2().cross(p[(i + 1) % n].to_vec2())).sum::<f64>() / 2.0
    }

    fn center(&self) -> Point {
        let s = self.pts.iter().fold(Vec2::ZERO, |s, q| s + q.to_vec2());
        (s / self.pts.len().max(1) as f64).to_point()
    }

    /// How big it is, to pair the pieces of two outlines biggest with biggest.
    fn size(&self) -> f64 {
        if self.closed { self.area().abs() } else { self.length() * self.length() / 16.0 }
    }

    /// Indices of the corners: points where the outline turns sharply.
    fn corners(&self) -> Vec<usize> {
        let (p, n) = (&self.pts, self.pts.len());
        (0..n)
            .filter(|&i| {
                if !self.closed && (i == 0 || i + 1 == n) {
                    return true;
                }
                let (a, b, c) = (p[(i + n - 1) % n], p[i], p[(i + 1) % n]);
                let (u, v) = (b - a, c - b);
                u.hypot() > 1e-9 && v.hypot() > 1e-9 && u.cross(v).atan2(u.dot(v)).abs() > CORNER
            })
            .collect()
    }

    /// `count` points at equal steps along it, with every corner moved onto the nearest one.
    fn resample(&self, count: usize) -> Vec<Point> {
        let mut pts = self.pts.clone();
        if self.closed {
            pts.push(self.pts[0]);
        }
        let lens: Vec<f64> = pts.windows(2).map(|w| (w[1] - w[0]).hypot()).collect();
        let total: f64 = lens.iter().sum();
        if total < 1e-9 {
            return vec![self.pts[0]; count];
        }
        // Where each original point lies along the outline.
        let mut at = vec![0.0];
        for l in &lens {
            at.push(at.last().unwrap() + l);
        }
        let gaps = if self.closed { count } else { count - 1 };
        let (mut out, mut seg) = (Vec::with_capacity(count), 0);
        for k in 0..count {
            let want = total * k as f64 / gaps as f64;
            while seg + 1 < lens.len() && at[seg + 1] < want {
                seg += 1;
            }
            let t = if lens[seg] > 0.0 { ((want - at[seg]) / lens[seg]).clamp(0.0, 1.0) } else { 0.0 };
            out.push(pts[seg].lerp(pts[seg + 1], t));
        }
        for c in self.corners() {
            let k = ((at[c] / total * gaps as f64).round() as usize) % count;
            out[k] = self.pts[c];
        }
        out
    }
}

/// The pieces of an outline as polylines.
fn rings(path: &BezPath) -> Vec<Ring> {
    let mut out = Vec::new();
    let mut cur: Vec<Point> = Vec::new();
    let mut flush = |cur: &mut Vec<Point>, closed: bool| {
        // A piece that comes back to its start is closed even without a `Z` (an ellipse).
        let back = cur.len() > 2 && (cur[0] - *cur.last().unwrap()).hypot() < 1e-6;
        if back {
            cur.pop();
        }
        let closed = closed || back;
        if cur.len() > 1 {
            let closed = closed && cur.len() > 2;
            out.push(Ring { pts: std::mem::take(cur), closed });
        }
        cur.clear();
    };
    kurbo::flatten(path, 0.05, |el| match el {
        PathEl::MoveTo(p) => {
            flush(&mut cur, false);
            cur.push(p);
        }
        PathEl::LineTo(p) => {
            if cur.last().is_none_or(|q| (*q - p).hypot() > 1e-9) {
                cur.push(p);
            }
        }
        PathEl::ClosePath => flush(&mut cur, true),
        _ => {}
    });
    flush(&mut cur, false);
    out.sort_by(|a, b| b.size().total_cmp(&a.size()));
    out
}

/// Points of `a` and `b` paired one to one: the same number, walked the same way round, and
/// for closed pieces starting where they fit best.
fn pair(a: &Ring, b: &Ring) -> (Vec<Point>, Vec<Point>) {
    let count = ((a.length().max(b.length()) / SPACING).ceil() as usize).clamp(MIN_POINTS, MAX_POINTS);
    let pa = a.resample(count);
    let mut b = b.clone();
    if a.closed && b.closed && a.area().signum() != b.area().signum() {
        b.pts.reverse();
    }
    let mut pb = b.resample(count);
    if !(a.closed && b.closed) {
        // Open lines: run the same way, start to start or start to end, whichever is closer.
        let flip = |q: &[Point]| (q[0] - pa[0]).hypot2() + (q[count - 1] - pa[count - 1]).hypot2();
        let mut rev = pb.clone();
        rev.reverse();
        if flip(&rev) < flip(&pb) {
            pb = rev;
        }
        return (pa, pb);
    }
    // The start of `b` that brings the two outlines closest, compared about their centres so
    // that where they stand does not matter,
    // and scaled to the same size, so that a small ball meets a big star tip to tip.
    let (ca, cb) = (a.center().to_vec2(), b.center().to_vec2());
    let spread = |p: &[Point], c: Vec2| (p.iter().map(|q| (q.to_vec2() - c).hypot2()).sum::<f64>() / count as f64).sqrt().max(1e-9);
    let (sa, sb) = (spread(&pa, ca), spread(&pb, cb));
    let cost = |k: usize| (0..count).map(|i| ((pa[i].to_vec2() - ca) / sa - (pb[(i + k) % count].to_vec2() - cb) / sb).hypot2()).sum::<f64>();
    let stride = (count / 64).max(1);
    let mut best = (0..count).step_by(stride).min_by(|&x, &y| cost(x).total_cmp(&cost(y))).unwrap_or(0);
    let near = best;
    for d in 1..stride {
        for k in [(near + d) % count, (near + count - d) % count] {
            if cost(k) < cost(best) {
                best = k;
            }
        }
    }
    pb.rotate_left(best);
    (pa, pb)
}

/// The outline `t` of the way from `a` to `b`. Pieces are paired biggest with biggest; a piece
/// left over on one side splits off the nearest piece of the other (or merges into it), so
/// three lines of a menu icon become the two of a cross and a play triangle the two bars of a
/// pause. Without anything on the other side, a piece shrinks into its centre.
pub fn blend(a: &BezPath, b: &BezPath, t: f64) -> BezPath {
    let (ra, rb) = (rings(a), rings(b));
    let nearest =
        |x: &Ring, other: &[Ring]| other.iter().min_by(|p, q| (p.center() - x.center()).hypot2().total_cmp(&(q.center() - x.center()).hypot2())).cloned();
    let mut out = BezPath::new();
    for i in 0..ra.len().max(rb.len()) {
        let (x, y) = match (ra.get(i), rb.get(i)) {
            (Some(x), Some(y)) => (x.clone(), y.clone()),
            (Some(x), None) => (x.clone(), nearest(x, &rb).unwrap_or_else(|| Ring { pts: vec![x.center()], closed: x.closed })),
            (None, Some(y)) => (nearest(y, &ra).unwrap_or_else(|| Ring { pts: vec![y.center()], closed: y.closed }), y.clone()),
            (None, None) => unreachable!(),
        };
        let (pa, pb) = pair(&x, &y);
        for (k, (p, q)) in pa.iter().zip(&pb).enumerate() {
            let m = p.lerp(*q, t);
            if k == 0 { out.move_to(m) } else { out.line_to(m) }
        }
        if x.closed && y.closed {
            out.close_path();
        }
    }
    out
}

/// Blends two paints: a colour fades in or out where the other side has none.
fn mix(a: Option<Color>, b: Option<Color>, t: f64) -> Option<Color> {
    let clear = |c: Color| Color { a: 0, ..c };
    match (a, b) {
        (Some(a), Some(b)) => Some(a.lerp(b, t)),
        (Some(a), None) => Some(a.lerp(clear(a), t)),
        (None, Some(b)) => Some(clear(b).lerp(b, t)),
        (None, None) => None,
    }
}

impl Document {
    /// Blends every morphing shape towards its targets as far as its amount says.
    pub fn morph_all(&mut self) {
        for i in 0..self.nodes.len() {
            let Some(m) = &self.nodes[i].morph else { continue };
            let n = &self.nodes[i];
            let base = m.base.unwrap_or(Paint { fill: n.fill, stroke: n.stroke, stroke_width: n.stroke_width });
            // The shape itself first, then each target as drawn, in its own space.
            let mut shapes = vec![(n.shape_path(), base)];
            for id in &m.targets {
                let Some(t) = self.index_of(id).filter(|&t| t != i) else { continue };
                let t = &self.nodes[t];
                shapes.push((t.base_path(), Paint { fill: t.fill, stroke: t.stroke, stroke_width: t.stroke_width }));
            }
            let last = shapes.len() - 1;
            let amount = m.amount.clamp(0.0, last as f64);
            let seg = (amount.floor() as usize).min(last.saturating_sub(1));
            let f = amount - seg as f64;
            let ((pa, a), (pb, b)) = (&shapes[seg], &shapes[(seg + 1).min(last)]);
            let path = match (pa, pb) {
                _ if last == 0 || amount <= 0.0 => None,
                (_, Some(pb)) if f >= 1.0 - 1e-9 => Some(pb.clone()),
                (Some(pa), _) if f <= 1e-9 => Some(pa.clone()),
                (Some(pa), Some(pb)) => Some(blend(pa, pb, f)),
                _ => None,
            };
            let paint = if last == 0 {
                base
            } else {
                Paint { fill: mix(a.fill, b.fill, f), stroke: mix(a.stroke, b.stroke, f), stroke_width: a.stroke_width + (b.stroke_width - a.stroke_width) * f }
            };
            let n = &mut self.nodes[i];
            (n.fill, n.stroke, n.stroke_width) = (paint.fill, paint.stroke, paint.stroke_width);
            let m = n.morph.as_mut().unwrap();
            m.base = Some(base);
            m.path = path;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Animation, Artboard, Keyframe, Node, Prop, ShapeKind, SimCache, Track, Value};
    use kurbo::Shape;

    fn doc() -> Document {
        let mut d = Document::new(Artboard::default());
        let mut ball = Node::new("ball", ShapeKind::Ellipse);
        (ball.x, ball.y, ball.width, ball.height) = (100.0, 100.0, 60.0, 60.0);
        ball.fill = Some(Color::rgb(255, 0, 0));
        ball.morph = Some(Morph::new(vec!["star".into()]));
        let mut star = Node::new("star", ShapeKind::Star);
        (star.x, star.y, star.width, star.height) = (300.0, 100.0, 100.0, 100.0);
        star.fill = Some(Color::rgb(0, 0, 255));
        star.visible = false;
        d.nodes = vec![ball, star];
        d
    }

    #[test]
    fn a_ball_flows_into_a_star_with_its_colour() {
        let mut d = doc();
        let round = d.nodes[0].path().unwrap().bounding_box();
        d.nodes[0].morph.as_mut().unwrap().amount = 1.0;
        d.rig();
        // At 1 it is the star, sharp tips and all, where the ball is.
        let star = d.nodes[1].path().unwrap();
        assert_eq!(d.nodes[0].path().unwrap(), star);
        assert_eq!(d.nodes[0].fill, Some(Color::rgb(0, 0, 255)));
        // Half way it is in between in size and colour, and rigging again changes nothing.
        d.nodes[0].morph.as_mut().unwrap().amount = 0.5;
        d.rig();
        let once = (d.nodes[0].path().unwrap(), d.nodes[0].fill);
        d.rig();
        assert_eq!((d.nodes[0].path().unwrap(), d.nodes[0].fill), once);
        let half = once.0.bounding_box();
        assert!(half.width() > round.width() + 5.0 && half.width() < 95.0, "{half:?}");
        let c = once.1.unwrap();
        assert!(c.r > 60 && c.b > 60, "{c:?}");
        // Back at 0 it is the ball again.
        d.nodes[0].morph.as_mut().unwrap().amount = 0.0;
        d.rig();
        assert_eq!(d.nodes[0].path().unwrap().bounding_box(), round);
        assert_eq!(d.nodes[0].fill, Some(Color::rgb(255, 0, 0)));
    }

    #[test]
    fn matched_points_do_not_cross_over() {
        // A square into a circle: every point moves a short way, no twisting through the middle.
        let mut sq = BezPath::new();
        sq.move_to((-50.0, -50.0));
        sq.line_to((50.0, -50.0));
        sq.line_to((50.0, 50.0));
        sq.line_to((-50.0, 50.0));
        sq.close_path();
        // Drawn the other way round and starting elsewhere.
        let circle = kurbo::Circle::new((0.0, 0.0), 50.0).to_path(0.1);
        let mut rev = BezPath::new();
        let pts: Vec<Point> = rings(&circle)[0].pts.iter().rev().cycle().skip(37).take(rings(&circle)[0].pts.len()).copied().collect();
        rev.move_to(pts[0]);
        for p in &pts[1..] {
            rev.line_to(*p);
        }
        rev.close_path();
        let (a, b) = pair(&rings(&sq)[0], &rings(&rev)[0]);
        let far = a.iter().zip(&b).map(|(p, q)| (*p - *q).hypot()).fold(0.0, f64::max);
        assert!(far < 25.0, "a point travels {far}");
        // Half way the outline keeps its area: it does not pinch.
        let mid = blend(&sq, &rev, 0.5);
        let area = mid.area().abs();
        assert!(area > 8000.0, "{area}");
    }

    #[test]
    fn the_morph_amount_is_keyable() {
        let d = doc();
        let key = |time, v| Keyframe { time, value: Value::Num(v), ease: crate::Ease::Linear };
        let a = Animation {
            name: "Go".into(),
            duration: 1.0,
            looping: false,
            tracks: vec![Track { node: "ball".into(), prop: Prop::Morph, keys: vec![key(0.0, 0.0), key(1.0, 1.0)] }],
            events: Vec::new(),
        };
        let end = a.pose_with(&d, 1.0, &mut SimCache::default());
        assert_eq!(end.nodes[0].path().unwrap(), end.nodes[1].path().unwrap());
        let start = a.pose_with(&d, 0.0, &mut SimCache::default());
        assert_eq!(start.nodes[0].fill, Some(Color::rgb(255, 0, 0)));
    }
}
