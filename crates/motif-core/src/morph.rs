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

/// One end of an artist's mark: a piece of a key contour by its place in the path (the first
/// `M` is piece 0), and optionally a place on it as a share of its length from its start, 0..1.
/// A share, not a node number, so that the mark survives editing the outline.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MatchEnd {
    pub piece: usize,
    pub at: Option<f64>,
}

/// An artist's mark between key contour `key` and the next one (`match "0>1 0:0=0:0.5"`).
/// Both ends: those pieces become each other, and with places, those places meet. One end only:
/// that piece has no partner on the other side and grows out of its place `at` (unset, its
/// middle). Key contours count from the shape itself: 0, then each morph target or form.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MorphLink {
    pub key: usize,
    pub from: Option<MatchEnd>,
    pub to: Option<MatchEnd>,
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
    /// Which piece of its path it is, counting from 0 in drawing order.
    index: usize,
}

impl Ring {
    fn length(&self) -> f64 {
        let n = self.pts.len();
        let open: f64 = self.pts.windows(2).map(|w| (w[1] - w[0]).hypot()).sum();
        if self.closed && n > 1 { open + (self.pts[0] - self.pts[n - 1]).hypot() } else { open }
    }

    /// How far along it each point lies, and the whole length.
    fn along(&self) -> (Vec<f64>, f64) {
        let mut at = vec![0.0];
        for w in self.pts.windows(2) {
            at.push(at.last().unwrap() + (w[1] - w[0]).hypot());
        }
        let total = self.length();
        (at, total)
    }

    /// The place a share `f` of its length from its start (0..1; round a closed piece).
    fn point_at(&self, f: f64, along: &(Vec<f64>, f64)) -> Point {
        let (at, total) = along;
        let n = self.pts.len();
        if n == 1 || *total < 1e-9 {
            return self.pts[0];
        }
        let f = if self.closed { f.rem_euclid(1.0) } else { f.clamp(0.0, 1.0) };
        let want = f * total;
        let seg = at.partition_point(|&a| a <= want).saturating_sub(1).min(n - 1);
        let (p, q) = (self.pts[seg], if seg + 1 < n { self.pts[seg + 1] } else { self.pts[0] });
        let next = if seg + 1 < n { at[seg + 1] } else { *total };
        let len = next - at[seg];
        if len > 0.0 { p.lerp(q, ((want - at[seg]) / len).clamp(0.0, 1.0)) } else { p }
    }

    /// The same piece walked the other way, from the same start: a share `f` along it is `1 - f`
    /// along the original.
    fn reversed(&self) -> Ring {
        let mut pts = self.pts.clone();
        if self.closed {
            pts[1..].reverse();
        } else {
            pts.reverse();
        }
        Ring { pts, ..self.clone() }
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

/// The pieces of an outline as polylines, biggest first.
fn rings(path: &BezPath) -> Vec<Ring> {
    let mut out = pieces(path);
    out.sort_by(|a, b| b.size().total_cmp(&a.size()));
    out
}

/// The pieces of an outline as polylines, in drawing order. A piece too small to draw is left
/// out but still counted, so that `index` is its place in the path.
fn pieces(path: &BezPath) -> Vec<Ring> {
    let mut out = Vec::new();
    let mut cur: Vec<Point> = Vec::new();
    let mut index = 0;
    let mut flush = |cur: &mut Vec<Point>, closed: bool, index: usize| {
        // A piece that comes back to its start is closed even without a `Z` (an ellipse).
        let back = cur.len() > 2 && (cur[0] - *cur.last().unwrap()).hypot() < 1e-6;
        if back {
            cur.pop();
        }
        let closed = closed || back;
        if cur.len() > 1 {
            let closed = closed && cur.len() > 2;
            out.push(Ring { pts: std::mem::take(cur), closed, index });
        }
        cur.clear();
    };
    let mut started = false;
    kurbo::flatten(path, 0.05, |el| match el {
        PathEl::MoveTo(p) => {
            if !cur.is_empty() {
                flush(&mut cur, false, index);
            }
            if started {
                index += 1;
            }
            started = true;
            cur.push(p);
        }
        PathEl::LineTo(p) if cur.last().is_none_or(|q| (*q - p).hypot() > 1e-9) => cur.push(p),
        PathEl::ClosePath => flush(&mut cur, true, index),
        _ => {}
    });
    flush(&mut cur, false, index);
    out
}

/// How many pieces an outline has: one for each `M`.
pub fn piece_count(path: &BezPath) -> usize {
    path.elements().iter().filter(|el| matches!(el, PathEl::MoveTo(_))).count()
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

/// Like `pair`, but the places in `pins` (shares of length along `a` and `b`) meet exactly, and
/// the stretches between them are spread evenly. The order of the pins along `b` decides which
/// way round it is walked; with too few to tell, the same rule as `pair`.
fn pair_pinned(a: &Ring, b: &Ring, pins: &[(f64, f64)]) -> (Vec<Point>, Vec<Point>) {
    if pins.is_empty() || a.pts.len() < 2 || b.pts.len() < 2 {
        return pair(a, b);
    }
    let count = ((a.length().max(b.length()) / SPACING).ceil() as usize).clamp(MIN_POINTS, MAX_POINTS);
    let mut pins = pins.to_vec();
    pins.sort_by(|x, y| x.0.total_cmp(&y.0));
    let closed = a.closed && b.closed;
    let bs: Vec<f64> = pins.iter().map(|p| p.1).collect();
    let rev = if closed {
        match order(&bs, true) {
            Some(forward) if pins.len() >= 3 => !forward,
            _ => a.area().signum() != b.area().signum(),
        }
    } else {
        match order(&bs, false) {
            Some(forward) if pins.len() >= 2 => !forward,
            _ => {
                let (ea, eb) = (a.pts[0], b.pts[0]);
                (ea - *b.pts.last().unwrap()).hypot2() < (ea - eb).hypot2()
            }
        }
    };
    let b = if rev { b.reversed() } else { b.clone() };
    if rev {
        for p in &mut pins {
            p.1 = if closed { (1.0 - p.1).rem_euclid(1.0) } else { 1.0 - p.1 };
        }
    }
    // The fixed places, in order along both, and the stretches between them.
    let mut anchors = pins.clone();
    if !closed {
        if anchors.first().is_none_or(|p| p.0 > 1e-9) {
            anchors.insert(0, (0.0, 0.0));
        }
        if anchors.last().is_none_or(|p| p.0 < 1.0 - 1e-9) {
            anchors.push((1.0, 1.0));
        }
    }
    let n = anchors.len();
    let stretches: Vec<(f64, f64, f64, f64)> = (0..if closed { n } else { n - 1 })
        .map(|i| {
            let (a0, b0) = anchors[i];
            let (a1, b1) = anchors[(i + 1) % n];
            let (da, db) = if closed {
                let da = if n == 1 { 1.0 } else { (a1 - a0).rem_euclid(1.0) };
                let db = if n == 1 { 1.0 } else { (b1 - b0).rem_euclid(1.0) };
                (da, db)
            } else {
                ((a1 - a0).max(0.0), (b1 - b0).max(0.0))
            };
            (a0, da, b0, db)
        })
        .collect();
    let weight: f64 = stretches.iter().map(|s| s.1 + s.3).sum::<f64>().max(1e-9);
    let (along_a, along_b) = (a.along(), b.along());
    let (mut fa, mut fb, mut fixed) = (Vec::new(), Vec::new(), Vec::new());
    for &(a0, da, b0, db) in &stretches {
        let steps = ((count as f64 * (da + db) / weight).round() as usize).max(1);
        fixed.push(fa.len());
        for s in 0..steps {
            let k = s as f64 / steps as f64;
            fa.push(a0 + da * k);
            fb.push(b0 + db * k);
        }
    }
    if !closed {
        fixed.push(fa.len());
        fa.push(1.0);
        fb.push(1.0);
    }
    let mut pa: Vec<Point> = fa.iter().map(|&f| a.point_at(f, &along_a)).collect();
    let mut pb: Vec<Point> = fb.iter().map(|&f| b.point_at(f, &along_b)).collect();
    // Sharp corners stay sharp: each lands on the nearest point that is not a pin.
    for (ring, along, shares, pts) in [(a, &along_a, &fa, &mut pa), (&b, &along_b, &fb, &mut pb)] {
        let total = along.1.max(1e-9);
        for c in ring.corners() {
            let fc = along.0[c] / total;
            let gap = |f: f64| {
                if closed {
                    let d = (f - fc).rem_euclid(1.0);
                    d.min(1.0 - d)
                } else {
                    (f - fc).abs()
                }
            };
            let near = (0..shares.len()).filter(|k| !fixed.contains(k)).min_by(|&x, &y| gap(shares[x]).total_cmp(&gap(shares[y])));
            if let Some(k) = near {
                pts[k] = ring.pts[c];
            }
        }
    }
    (pa, pb)
}

/// A piece of one contour, the piece of the next it becomes, and the places that meet on them.
type PiecePair = (usize, usize, Vec<(f64, f64)>);
/// Marks on one pair of pieces (step, piece, piece): each mark's number and its two places.
type PinGroup = ((usize, usize, usize), Vec<(usize, f64, f64)>);

/// Whether `shares` run forward (`Some(true)`), backward (`Some(false)`) or neither. Round a
/// closed piece they may pass its start once.
fn order(shares: &[f64], closed: bool) -> Option<bool> {
    let n = shares.len();
    if n < 2 {
        return Some(true);
    }
    let steps: Vec<f64> = if closed { (0..n).map(|i| shares[(i + 1) % n] - shares[i]).collect() } else { shares.windows(2).map(|w| w[1] - w[0]).collect() };
    let (up, down) = (steps.iter().filter(|d| **d > 1e-9).count(), steps.iter().filter(|d| **d < -1e-9).count());
    let wraps = if closed { 1 } else { 0 };
    if down <= wraps && up + down == steps.len() && (closed || down == 0) {
        Some(true)
    } else if up <= wraps && up + down == steps.len() && (closed || up == 0) {
        Some(false)
    } else {
        None
    }
}

/// The outline `t` of the way from `a` to `b`, keeping the artist's marks between them (the
/// links whose `key` is this step). Pieces the marks do not name are matched as by `blend`.
pub fn blend_linked(a: &BezPath, b: &BezPath, t: f64, links: &[MorphLink]) -> BezPath {
    if links.is_empty() {
        return blend(a, b, t);
    }
    let (ra, rb) = (pieces(a), pieces(b));
    let find = |rs: &[Ring], i: usize| rs.iter().find(|r| r.index == i).cloned();
    let mut pairs: Vec<PiecePair> = Vec::new();
    let mut out = BezPath::new();
    let (mut used_a, mut used_b) = (Vec::new(), Vec::new());
    let emit = |x: &Ring, y: &Ring, (pa, pb): (Vec<Point>, Vec<Point>), out: &mut BezPath| {
        for (k, (p, q)) in pa.iter().zip(&pb).enumerate() {
            let m = p.lerp(*q, t);
            if k == 0 { out.move_to(m) } else { out.line_to(m) }
        }
        if x.closed && y.closed {
            out.close_path();
        }
    };
    let mut grows = Vec::new();
    for l in links {
        match (l.from, l.to) {
            (Some(x), Some(y)) => {
                let at = pairs.iter().position(|p| p.0 == x.piece && p.1 == y.piece).unwrap_or_else(|| {
                    pairs.push((x.piece, y.piece, Vec::new()));
                    pairs.len() - 1
                });
                if let (Some(p), Some(q)) = (x.at, y.at) {
                    pairs[at].2.push((p, q));
                }
            }
            (Some(x), None) => grows.push((true, x)),
            (None, Some(y)) => grows.push((false, y)),
            (None, None) => {}
        }
    }
    // Pieces that become each other first, then those that grow out of a point.
    for (i, j, pins) in &pairs {
        if used_a.contains(i) || used_b.contains(j) {
            continue;
        }
        if let (Some(x), Some(y)) = (find(&ra, *i), find(&rb, *j)) {
            emit(&x, &y, pair_pinned(&x, &y, pins), &mut out);
            used_a.push(*i);
            used_b.push(*j);
        }
    }
    for (first, e) in grows {
        let (rs, used) = if first { (&ra, &mut used_a) } else { (&rb, &mut used_b) };
        let Some(r) = find(rs, e.piece).filter(|_| !used.contains(&e.piece)) else { continue };
        let dot = Ring { pts: vec![r.point_at(e.at.unwrap_or(0.5), &r.along())], closed: r.closed, index: usize::MAX };
        if first {
            emit(&r, &dot, pair(&r, &dot), &mut out);
        } else {
            emit(&dot, &r, pair(&dot, &r), &mut out);
        }
        used.push(e.piece);
    }
    let rest = |rs: Vec<Ring>, used: &[usize]| {
        let mut r: Vec<Ring> = rs.into_iter().filter(|r| !used.contains(&r.index)).collect();
        r.sort_by(|a, b| b.size().total_cmp(&a.size()));
        r
    };
    let (ra, rb) = (rest(ra, &used_a), rest(rb, &used_b));
    if !ra.is_empty() || !rb.is_empty() {
        out.extend(blend_rings(&ra, &rb, t));
    }
    out
}

/// The place on `path` nearest to `p`: which piece, how far along it as a share of its length,
/// the point itself and how far it is from `p`.
pub fn nearest_place(path: &BezPath, p: Point) -> Option<(MatchEnd, Point, f64)> {
    let mut best: Option<(MatchEnd, Point, f64)> = None;
    for r in pieces(path) {
        let (at, total) = r.along();
        let n = r.pts.len();
        let segs = if r.closed { n } else { n - 1 };
        for (k, start) in at.iter().enumerate().take(segs) {
            let (a, b) = (r.pts[k], r.pts[(k + 1) % n]);
            let d = b - a;
            let len2 = d.hypot2();
            let t = if len2 > 1e-12 { ((p - a).dot(d) / len2).clamp(0.0, 1.0) } else { 0.0 };
            let q = a + d * t;
            let dist = (q - p).hypot();
            if best.as_ref().is_none_or(|b| dist < b.2) {
                let share = if total > 1e-9 { ((start + d.hypot() * t) / total).clamp(0.0, 1.0) } else { 0.0 };
                best = Some((MatchEnd { piece: r.index, at: Some(share) }, q, dist));
            }
        }
    }
    best
}

/// Where a mark's end lies on `path`: its place, or the middle of its piece without one.
pub fn place_point(path: &BezPath, end: MatchEnd) -> Option<Point> {
    let r = pieces(path).into_iter().find(|r| r.index == end.piece)?;
    Some(r.point_at(end.at.unwrap_or(0.5), &r.along()))
}

/// What is wrong with the marks `links` on a shape with these key contours, each problem with
/// the number of the mark it is about.
pub fn check_links(keys: &[BezPath], links: &[MorphLink]) -> Vec<(usize, String)> {
    let mut errs = Vec::new();
    let counts: Vec<usize> = keys.iter().map(piece_count).collect();
    for (n, l) in links.iter().enumerate() {
        if l.key + 1 >= keys.len() {
            errs.push((n, format!("у фигуры ключевых контуров: {} — метке {}>{} не к чему привязаться", keys.len(), l.key, l.key + 1)));
            continue;
        }
        for (end, k) in [(l.from, l.key), (l.to, l.key + 1)] {
            if let Some(e) = end
                && e.piece >= counts[k]
            {
                errs.push((n, format!("в контуре {k} нет куска {} (кусков: {})", e.piece, counts[k])));
            }
        }
    }
    if !errs.is_empty() {
        return errs;
    }
    let mut seen: Vec<(usize, bool, usize, Option<usize>)> = Vec::new();
    for (n, l) in links.iter().enumerate() {
        for (side, end, other) in [(false, l.from, l.to), (true, l.to, l.from)] {
            let Some(e) = end else { continue };
            let partner = other.map(|o| o.piece);
            match seen.iter().find(|s| s.0 == l.key && s.1 == side && s.2 == e.piece) {
                Some(s) if s.3 != partner => {
                    let what = |p: Option<usize>| p.map_or("растёт из точки".to_string(), |p| format!("становится куском {p}"));
                    errs.push((n, format!("кусок {} контура {}: в одной метке он {}, в другой {}", e.piece, l.key + side as usize, what(s.3), what(partner))));
                }
                Some(_) => {}
                None => seen.push((l.key, side, e.piece, partner)),
            }
        }
    }
    // Places paired on one pair of pieces must come in the same order along both.
    let mut groups: Vec<PinGroup> = Vec::new();
    for (n, l) in links.iter().enumerate() {
        if let (Some(x), Some(y)) = (l.from, l.to)
            && let (Some(p), Some(q)) = (x.at, y.at)
        {
            let key = (l.key, x.piece, y.piece);
            match groups.iter_mut().find(|g| g.0 == key) {
                Some(g) => g.1.push((n, p, q)),
                None => groups.push((key, vec![(n, p, q)])),
            }
        }
    }
    for ((k, i, j), mut pins) in groups {
        pins.sort_by(|a, b| a.1.total_cmp(&b.1));
        if let Some(w) = pins.windows(2).find(|w| (w[1].1 - w[0].1).abs() < 1e-9 || (w[1].2 - w[0].2).abs() < 1e-9) {
            errs.push((w[1].0, format!("на кусках {i}>{j} две метки в одном месте")));
            continue;
        }
        let closed = |path: &BezPath, piece: usize| pieces(path).iter().find(|r| r.index == piece).is_some_and(|r| r.closed);
        let closed = closed(&keys[k], i) && closed(&keys[k + 1], j);
        let bs: Vec<f64> = pins.iter().map(|p| p.2).collect();
        if order(&bs, closed).is_none() {
            errs.push((pins.last().unwrap().0, format!("метки на кусках {i}>{j} идут в разном порядке — перетекание перекрутится")));
        }
    }
    errs
}

/// The outline `t` of the way from `a` to `b`. Pieces are paired biggest with biggest; a piece
/// left over on one side splits off the nearest piece of the other (or merges into it), so
/// three lines of a menu icon become the two of a cross and a play triangle the two bars of a
/// pause. Without anything on the other side, a piece shrinks into its centre.
pub fn blend(a: &BezPath, b: &BezPath, t: f64) -> BezPath {
    blend_rings(&rings(a), &rings(b), t)
}

/// `blend` on pieces already taken apart, biggest first.
fn blend_rings(ra: &[Ring], rb: &[Ring], t: f64) -> BezPath {
    let nearest =
        |x: &Ring, other: &[Ring]| other.iter().min_by(|p, q| (p.center() - x.center()).hypot2().total_cmp(&(q.center() - x.center()).hypot2())).cloned();
    let mut out = BezPath::new();
    for i in 0..ra.len().max(rb.len()) {
        let (x, y) = match (ra.get(i), rb.get(i)) {
            (Some(x), Some(y)) => (x.clone(), y.clone()),
            (Some(x), None) => (x.clone(), nearest(x, rb).unwrap_or_else(|| Ring { pts: vec![x.center()], closed: x.closed, index: usize::MAX })),
            (None, Some(y)) => (nearest(y, ra).unwrap_or_else(|| Ring { pts: vec![y.center()], closed: y.closed, index: usize::MAX }), y.clone()),
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
    /// The key contours of a morphing shape, in order: its own outline first, then each morph
    /// target's (`morph star heart`) or each of its forms (`forms "…"`). Empty when it has none.
    pub fn key_contours(&self, i: usize) -> Vec<BezPath> {
        let n = &self.nodes[i];
        let Some(own) = n.shape_path() else { return Vec::new() };
        if let Some(m) = &n.morph {
            let mut out = vec![own];
            out.extend(m.targets.iter().filter_map(|id| self.index_of(id).filter(|&t| t != i)).filter_map(|t| self.nodes[t].base_path()));
            return out;
        }
        match &n.forms {
            Some(f) => std::iter::once(own).chain(f.shapes.iter().cloned()).collect(),
            None => Vec::new(),
        }
    }

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
                (Some(pa), Some(pb)) => {
                    let links: Vec<MorphLink> = n.morph_links.iter().filter(|l| l.key == seg).copied().collect();
                    Some(blend_linked(pa, pb, f, &links))
                }
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

    fn svg(d: &str) -> BezPath {
        BezPath::from_svg(d).unwrap()
    }

    fn end(piece: usize, at: f64) -> Option<MatchEnd> {
        Some(MatchEnd { piece, at: Some(at) })
    }

    /// The point of `path` nearest to `p`, and how far it is.
    fn nearest(path: &BezPath, p: Point) -> f64 {
        pieces(path).iter().flat_map(|r| r.pts.clone()).map(|q| (q - p).hypot()).fold(f64::INFINITY, f64::min)
    }

    #[test]
    fn pinned_places_meet_and_the_rest_spreads_between_them() {
        // A closed mouth (a thin lens) opens into a wide oval. The marks hold its corners: the left
        // corner (start, 0) stays the left corner, the right one (half way) stays the right one.
        let shut = svg("M -40 0 C -20 4 20 4 40 0 C 20 6 -20 6 -40 0 Z");
        let open = svg("M -30 -10 C -15 -30 15 -30 30 -10 C 20 30 -20 30 -30 -10 Z");
        let pins = [MorphLink { key: 0, from: end(0, 0.0), to: end(0, 0.0) }, MorphLink { key: 0, from: end(0, 0.5), to: end(0, 0.5) }];
        let (ra, rb) = (&pieces(&shut)[0], &pieces(&open)[0]);
        let right_a = ra.point_at(0.5, &ra.along());
        let right_b = rb.point_at(0.5, &rb.along());
        let half = blend_linked(&shut, &open, 0.5, &pins);
        // Half way, the corners are exactly half way between where they were.
        assert!(nearest(&half, Point::new(-35.0, -5.0)) < 1e-6, "{}", nearest(&half, Point::new(-35.0, -5.0)));
        assert!(nearest(&half, right_a.midpoint(right_b)) < 1e-6);
        // At the ends it is each shape again.
        assert!(nearest(&blend_linked(&shut, &open, 0.0, &pins), Point::new(40.0, 0.0)) < 0.5);
        // Without the marks the right corner goes wherever the automatic match sends it.
        let auto = blend(&shut, &open, 0.5);
        assert!(nearest(&auto, Point::new(-35.0, -5.0)) > 1e-3);
    }

    #[test]
    fn a_mark_survives_extra_nodes_on_the_outline() {
        // The same square drawn with four nodes and with eight: a share of its length is the same
        // place on both, so the blend is the same.
        let four = svg("M -50 -50 L 50 -50 L 50 50 L -50 50 Z");
        let eight = svg("M -50 -50 L 0 -50 L 50 -50 L 50 0 L 50 50 L 0 50 L -50 50 L -50 0 Z");
        let star = svg("M 0 -60 L 15 -15 L 60 0 L 15 15 L 0 60 L -15 15 L -60 0 L -15 -15 Z");
        let pins = [MorphLink { key: 0, from: end(0, 0.125), to: end(0, 0.0) }, MorphLink { key: 0, from: end(0, 0.625), to: end(0, 0.5) }];
        let (a, b) = (blend_linked(&four, &star, 0.4, &pins), blend_linked(&eight, &star, 0.4, &pins));
        let far = pieces(&a)[0].pts.iter().map(|p| nearest(&b, *p)).fold(0.0, f64::max);
        assert!(far < 1.0, "{far}");
        // And the mark is kept: the top middle of the square goes to the star's top tip.
        assert!(nearest(&a, Point::new(0.0, -50.0).lerp(Point::new(0.0, -60.0), 0.4)) < 1e-6);
    }

    #[test]
    fn a_missing_piece_grows_out_of_its_mark() {
        // A disc becomes a ring: the hole has no partner. Marked to grow from its top, at the
        // start it is a speck there, not in the middle.
        let disc = svg("M -50 0 A 50 50 0 1 1 50 0 A 50 50 0 1 1 -50 0 Z");
        let ring = svg("M -50 0 A 50 50 0 1 1 50 0 A 50 50 0 1 1 -50 0 Z M 0 -25 A 25 25 0 1 0 0 25 A 25 25 0 1 0 0 -25 Z");
        let grow = [
            MorphLink { key: 0, from: Some(MatchEnd { piece: 0, at: None }), to: Some(MatchEnd { piece: 0, at: None }) },
            MorphLink { key: 0, from: None, to: end(1, 0.0) },
        ];
        let early = blend_linked(&disc, &ring, 0.02, &grow);
        let hole = &pieces(&early)[1];
        let top = Point::new(0.0, -25.0);
        assert!(hole.pts.iter().all(|p| (*p - top).hypot() < 2.0), "the hole starts at its mark");
        // Without the mark it splits off the disc instead.
        let auto = &pieces(&blend(&disc, &ring, 0.02))[1];
        assert!(auto.pts.iter().any(|p| (*p - top).hypot() > 20.0));
    }

    #[test]
    fn paired_pieces_trade_places() {
        // Two dots, left and right. Marked to swap, half way both are in the middle.
        let dots = svg("M -60 -5 L -50 -5 L -50 5 L -60 5 Z M 50 -5 L 60 -5 L 60 5 L 50 5 Z");
        let swap = [
            MorphLink { key: 0, from: Some(MatchEnd { piece: 0, at: None }), to: Some(MatchEnd { piece: 1, at: None }) },
            MorphLink { key: 0, from: Some(MatchEnd { piece: 1, at: None }), to: Some(MatchEnd { piece: 0, at: None }) },
        ];
        let half = blend_linked(&dots, &dots, 0.5, &swap);
        let w = half.bounding_box().width();
        assert!(w < 15.0, "{w}");
        assert!(blend(&dots, &dots, 0.5).bounding_box().width() > 100.0);
    }

    #[test]
    fn a_place_found_by_pointing_leads_back_to_the_same_point() {
        let two = svg("M 0 0 L 100 0 L 100 100 L 0 100 Z M 200 0 L 300 0 L 300 100 Z");
        let (end, at, dist) = nearest_place(&two, Point::new(103.0, 50.0)).unwrap();
        assert_eq!((end.piece, at, dist), (0, Point::new(100.0, 50.0), 3.0));
        assert!((end.at.unwrap() - 0.375).abs() < 1e-9, "{end:?}");
        assert_eq!(place_point(&two, end), Some(Point::new(100.0, 50.0)));
        let (end, ..) = nearest_place(&two, Point::new(250.0, -4.0)).unwrap();
        assert_eq!(end.piece, 1);
        assert_eq!(place_point(&two, MatchEnd { piece: 2, at: None }), None);
    }

    #[test]
    fn wrong_marks_are_told_apart() {
        let sq = svg("M -50 -50 L 50 -50 L 50 50 L -50 50 Z");
        let keys = [sq.clone(), sq.clone()];
        let at = |a: f64, b: f64| MorphLink { key: 0, from: end(0, a), to: end(0, b) };
        assert!(check_links(&keys, &[at(0.0, 0.0), at(0.3, 0.3), at(0.6, 0.6)]).is_empty());
        // Round a closed piece the marks may pass the start, or all run backwards.
        assert!(check_links(&keys, &[at(0.0, 0.5), at(0.3, 0.8), at(0.6, 0.1)]).is_empty());
        assert!(check_links(&keys, &[at(0.0, 0.6), at(0.3, 0.3), at(0.6, 0.0)]).is_empty());
        let twist = check_links(&keys, &[at(0.0, 0.0), at(0.3, 0.6), at(0.6, 0.3), at(0.8, 0.9)]);
        assert!(twist.len() == 1 && twist[0].1.contains("разном порядке"), "{twist:?}");
        let same = check_links(&keys, &[at(0.2, 0.0), at(0.2, 0.5)]);
        assert!(same[0].1.contains("в одном месте"), "{same:?}");
        let piece = check_links(&keys, &[MorphLink { key: 0, from: end(2, 0.0), to: end(0, 0.0) }]);
        assert_eq!(piece, vec![(0, "в контуре 0 нет куска 2 (кусков: 1)".to_string())]);
        let key = check_links(&keys, &[MorphLink { key: 1, from: end(0, 0.0), to: end(0, 0.0) }]);
        assert!(key[0].1.contains("не к чему"), "{key:?}");
        let both = check_links(&keys, &[at(0.1, 0.1), MorphLink { key: 0, from: end(0, 0.5), to: None }]);
        assert!(both[0].1.contains("растёт из точки"), "{both:?}");
    }
}
