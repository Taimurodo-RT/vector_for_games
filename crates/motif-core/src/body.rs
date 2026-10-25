//! 3D bodies: a flat shape pushed out into depth (`extrude 60`), optionally narrowing towards
//! the back (`extrude 120 0` is a cone or a pyramid). The body turns in space with `yaw` and
//! `pitch` and is drawn as flat vector faces: hidden faces are dropped, the rest are sorted back
//! to front and shaded by the scene's light. With a stroke, only the body's real edges are inked:
//! its silhouette and sharp creases, not the seams of a curved side.

use crate::orbit::{Turn, V3};
use crate::{Color, Node};
use kurbo::{BezPath, PathEl, Point, Shape};

/// How a shape is pushed into depth.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Body {
    /// Depth along the axis pointing at the viewer, in the shape's units.
    pub depth: f64,
    /// Size of the back face against the front: 1 a prism or a cylinder, 0 a pyramid or a cone.
    pub taper: f64,
}

impl Body {
    pub fn new(depth: f64) -> Body {
        Body { depth, taper: 1.0 }
    }
}

/// One face of a body as drawn, in the node's local space.
#[derive(Clone, Debug, PartialEq)]
pub struct Face {
    pub path: BezPath,
    pub color: Color,
    /// Fill the face even-odd: the front and back of a shape with holes.
    pub even_odd: bool,
    /// A curved stretch of side: a linear gradient from one point to the other through these
    /// colours, evenly spaced. `color` is its middle.
    pub gradient: Option<(Point, Point, Vec<Color>)>,
    /// Edges to ink right after the face, as polylines: faces in front cover part of them.
    pub edges: Vec<Vec<Point>>,
    /// Edges nothing covers, inked over all the faces so that the next face does not cut into
    /// a thick line.
    pub top_edges: Vec<Vec<Point>>,
}

/// Outline pieces flattened finer than this are smooth: their sides shade as one curved surface.
const TOLERANCE: f64 = 0.1;
/// Turns sharper than this (radians) between path segments are creases, inked as edges.
const CREASE: f64 = 0.2;
/// Faces whose normals differ by more than this (cosine) meet at an inked edge.
const SHARP: f64 = 0.94;

/// One piece of the outline: its points and, for each point, whether the side bends sharply there.
struct Ring {
    pts: Vec<Point>,
    sharp: Vec<bool>,
    closed: bool,
}

fn rings(path: &BezPath) -> Vec<Ring> {
    let mut out = Vec::new();
    let mut ring = Ring { pts: Vec::new(), sharp: Vec::new(), closed: false };
    // Direction of the last flattened step, to find corners where path segments meet.
    let mut last_dir: Option<kurbo::Vec2> = None;
    let mut first_dir: Option<kurbo::Vec2> = None;
    let turn = |a: kurbo::Vec2, b: kurbo::Vec2| a.cross(b).atan2(a.dot(b)).abs();
    let mut flush = |ring: &mut Ring, closed: bool, last: Option<kurbo::Vec2>, first: Option<kurbo::Vec2>| {
        let mut r = std::mem::replace(ring, Ring { pts: Vec::new(), sharp: Vec::new(), closed: false });
        let n = r.pts.len();
        if n > 2 && (r.pts[0] - r.pts[n - 1]).hypot() < 1e-6 {
            r.pts.pop();
            r.sharp.pop();
        }
        let closed = closed || n > 2 && r.pts.len() < n;
        if closed && let (Some(a), Some(b)) = (last, first) {
            r.sharp[0] = turn(a, b) > CREASE;
        }
        if r.pts.len() > 1 {
            r.closed = closed && r.pts.len() > 2;
            out.push(r);
        }
    };
    let mut cur = Point::ZERO;
    for el in path.elements() {
        match *el {
            PathEl::MoveTo(p) => {
                flush(&mut ring, false, last_dir, first_dir);
                (last_dir, first_dir) = (None, None);
                ring.pts.push(p);
                ring.sharp.push(true);
                cur = p;
            }
            PathEl::ClosePath => {
                if let Some(&p0) = ring.pts.first()
                    && (p0 - cur).hypot() > 1e-9
                {
                    let d = p0 - cur;
                    if let Some(a) = last_dir {
                        *ring.sharp.last_mut().unwrap() = turn(a, d) > CREASE;
                    }
                    last_dir = Some(d);
                }
                flush(&mut ring, true, last_dir, first_dir);
                (last_dir, first_dir) = (None, None);
                cur = ring.pts.first().copied().unwrap_or(cur);
            }
            seg => {
                if ring.pts.is_empty() {
                    ring.pts.push(cur);
                    ring.sharp.push(true);
                }
                let mut pts = Vec::new();
                kurbo::flatten([PathEl::MoveTo(cur), seg], TOLERANCE, |e| {
                    if let PathEl::LineTo(p) = e {
                        pts.push(p);
                    }
                });
                let mut prev = cur;
                for (k, p) in pts.into_iter().enumerate() {
                    let d = p - prev;
                    if d.hypot() < 1e-9 {
                        continue;
                    }
                    if k == 0 || ring.pts.len() == 1 {
                        // Where this segment starts: a corner if it turns from the last one.
                        let at = ring.sharp.len() - 1;
                        ring.sharp[at] = last_dir.is_none_or(|a| turn(a, d) > CREASE);
                    }
                    first_dir = first_dir.or(Some(d));
                    last_dir = Some(d);
                    ring.pts.push(p);
                    ring.sharp.push(false);
                    prev = p;
                }
                cur = prev;
            }
        }
    }
    flush(&mut ring, false, last_dir, first_dir);
    out
}

fn area(pts: &[Point]) -> f64 {
    let n = pts.len();
    (0..n).map(|i| pts[i].to_vec2().cross(pts[(i + 1) % n].to_vec2())).sum::<f64>() / 2.0
}

/// A face before sorting: its corners in turned space, its normal and what it is.
struct Raw {
    pts: Vec<V3>,
    normal: V3,
    /// The front or back cap: drawn from the whole outline with its holes.
    cap: Option<bool>,
    /// The next side round the outline, when the surface runs on smoothly into it.
    smooth_next: Option<usize>,
}

/// An edge between two faces (or a face and nothing, at the end of a ribbon).
struct Edge {
    a: usize,
    b: Option<usize>,
    from: V3,
    to: V3,
    /// A corner of the shape itself, inked even where both faces show.
    crease: bool,
}

/// The faces of `path` pushed into depth by `body`, turned by `yaw` and `pitch` (degrees), back
/// to front, shaded by a light falling towards `light` (degrees, as the scene's shadows fall).
/// Faces turned away are left out. `fill` colours the faces, `ink` marks which edges to draw,
/// `bands` (when not 0) snaps the light to that many flat steps.
#[allow(clippy::too_many_arguments)]
pub fn faces(path: &BezPath, body: Body, yaw: f64, pitch: f64, light: f64, fill: Color, ink: bool, bands: u32) -> Vec<Face> {
    let turn = Turn { yaw, pitch };
    let rings = rings(path);
    let half = body.depth / 2.0;
    let taper = body.taper.max(0.0);
    let front = |p: Point| turn.apply(V3::new(p.x, p.y, half));
    let back = |p: Point| turn.apply(V3::new(p.x * taper, p.y * taper, -half));
    let mut raw: Vec<Raw> = Vec::new();
    let mut edges: Vec<Edge> = Vec::new();
    let closed: Vec<&Ring> = rings.iter().filter(|r| r.closed).collect();
    // Caps first, then the sides of each piece. Caps are faces 0 (front) and 1 (back).
    let caps = !closed.is_empty() && body.depth.abs() > 1e-9;
    if caps {
        for is_front in [true, false] {
            let n = turn.apply(V3::new(0.0, 0.0, if is_front { 1.0 } else { -1.0 }));
            raw.push(Raw { pts: Vec::new(), normal: n, cap: Some(is_front), smooth_next: None });
        }
    }
    // A piece inside an odd number of others is a hole (the caps fill even-odd).
    let inside = |p: Point, r: &Ring| {
        let n = r.pts.len();
        (0..n)
            .filter(|&i| {
                let (a, b) = (r.pts[i], r.pts[(i + 1) % n]);
                (a.y > p.y) != (b.y > p.y) && p.x < a.x + (p.y - a.y) / (b.y - a.y) * (b.x - a.x)
            })
            .count()
            % 2
            == 1
    };
    let hole = |r: &Ring| closed.iter().filter(|o| !std::ptr::eq(**o, r) && inside(r.pts[0], o)).count() % 2 == 1;
    let flip = if body.depth < 0.0 { -1.0 } else { 1.0 };
    for r in &rings {
        let n = r.pts.len();
        let first = raw.len();
        // Sides face out of the piece, or into it for a hole, whichever way it was drawn.
        let way = if !r.closed {
            1.0
        } else if hole(r) {
            -area(&r.pts).signum()
        } else {
            area(&r.pts).signum()
        };
        for k in 0..edges_n(r) {
            let (a, b) = (r.pts[k], r.pts[(k + 1) % n]);
            let pts = vec![front(a), front(b), back(b), back(a)];
            // Outward in the flat shape: the edge turned a quarter, against the winding.
            let e = b - a;
            let side = V3::new(e.y, -e.x, 0.0).mul(way);
            // Tilted by the taper: the side leans in towards the narrower back.
            let lean = (1.0 - taper) * {
                let m = a.midpoint(b).to_vec2();
                let len = e.hypot().max(1e-12);
                (side.x * m.x + side.y * m.y) / len
            };
            let local = V3::new(side.x / e.hypot().max(1e-12), side.y / e.hypot().max(1e-12), lean / body.depth.abs().max(1e-9) * flip);
            let normal = turn.apply(local.unit()).mul(flip);
            let me = raw.len();
            let next = if k + 1 == edges_n(r) { r.closed.then_some(first) } else { Some(me + 1) };
            let smooth_next = next.filter(|_| !r.sharp[(k + 1) % n]);
            raw.push(Raw { pts: pts.clone(), normal, cap: None, smooth_next });
            // The rims along the caps, or the long edges of a ribbon.
            let rim = |cap: usize| (caps && r.closed).then_some(cap);
            edges.push(Edge { a: me, b: rim(0), from: pts[0], to: pts[1], crease: true });
            edges.push(Edge { a: me, b: rim(1), from: pts[2], to: pts[3], crease: true });
            // Across to the next side, and the first end of a ribbon.
            edges.push(Edge { a: me, b: next, from: pts[1], to: pts[2], crease: r.sharp[(k + 1) % n] });
            if !r.closed && k == 0 {
                edges.push(Edge { a: me, b: None, from: pts[3], to: pts[0], crease: true });
            }
        }
    }
    // Light: from the side opposite the shadows, and from in front.
    let a = light.to_radians();
    let to_light = V3::new(-a.cos() * 0.7, -a.sin() * 0.7, 0.7).unit();
    let shade = |n: V3| {
        let d = crate::brush::band_light(n.dot(to_light).max(0.0), bands);
        let k = 0.4 + 0.75 * d;
        if k > 1.0 { fill.lerp(Color { a: fill.a, ..Color::WHITE }, (k - 1.0) * 0.8) } else { fill.lerp(Color { a: fill.a, ..Color::BLACK }, (1.0 - k) * 0.75) }
    };
    // Open outlines (ribbons) show both sides; closed bodies hide what faces away.
    let shows = |f: &Raw| f.normal.z > 1e-6 || f.cap.is_none() && !rings.iter().all(|r| r.closed);
    let visible: Vec<bool> = raw.iter().map(shows).collect();
    // A cap seen from the front is the body's nearest plane: everything else lies behind it.
    // The sides sort by their middles.
    let depth = |f: &Raw| match f.cap {
        Some(_) => f64::INFINITY,
        None => f.pts.iter().map(|p| p.z).sum::<f64>() / 4.0,
    };
    // Smooth stretches of side (a cylinder's wall) become one face shaded by a gradient, so
    // that no seams show between their slices. A stretch breaks where it folds back on screen.
    let axis = turn.apply(V3::new(0.0, 0.0, 1.0));
    let across = kurbo::Vec2::new(-axis.y, axis.x);
    let across = if across.hypot() < 1e-6 { kurbo::Vec2::new(1.0, 0.0) } else { across.normalize() };
    let at = |f: &Raw| f.pts.iter().fold(kurbo::Vec2::ZERO, |s, p| s + kurbo::Vec2::new(p.x, p.y)).dot(across) / 4.0;
    let joins = |i: usize| raw[i].smooth_next.filter(|&j| visible[i] && visible[j] && raw[i].normal.dot(raw[j].normal) >= SHARP);
    let mut group = vec![usize::MAX; raw.len()];
    let mut groups: Vec<Vec<usize>> = Vec::new();
    for i in (0..raw.len()).filter(|&i| visible[i]) {
        // Start a stretch only where no smooth neighbour runs into this face.
        let fed = (0..raw.len()).any(|h| h != i && joins(h) == Some(i));
        if group[i] != usize::MAX || fed && raw[i].cap.is_none() {
            continue;
        }
        let mut run = vec![i];
        group[i] = groups.len();
        while let Some(j) = joins(*run.last().unwrap()).filter(|&j| group[j] == usize::MAX) {
            // Folding back: the slices stop moving the same way across the screen.
            if run.len() >= 2 {
                let (a, b, c) = (at(&raw[run[run.len() - 2]]), at(&raw[*run.last().unwrap()]), at(&raw[j]));
                if (b - a) * (c - b) < 0.0 {
                    break;
                }
            }
            group[j] = groups.len();
            run.push(j);
        }
        groups.push(run);
    }
    // A closed ring that is smooth all round and fully visible has no start: open it anywhere.
    for i in 0..raw.len() {
        if !visible[i] || group[i] != usize::MAX {
            continue;
        }
        let mut run = vec![i];
        group[i] = groups.len();
        while let Some(j) = joins(*run.last().unwrap()).filter(|&j| group[j] == usize::MAX) {
            group[j] = groups.len();
            run.push(j);
        }
        groups.push(run);
    }
    let gdepth = |g: &[usize]| g.iter().map(|&i| depth(&raw[i])).sum::<f64>() / g.len() as f64;
    let mut order: Vec<usize> = (0..groups.len()).collect();
    order.sort_by(|&x, &y| gdepth(&groups[x]).total_cmp(&gdepth(&groups[y])));
    let mut rank: Vec<Option<usize>> = vec![None; raw.len()];
    for (k, &g) in order.iter().enumerate() {
        for &i in &groups[g] {
            rank[i] = Some(k);
        }
    }
    let flat = |p: V3| Point::new(p.x, p.y);
    // A face facing away from the viewer (a ribbon's back) shades by its other side.
    let lit = |f: &Raw| shade(if f.normal.z < 0.0 { f.normal.mul(-1.0) } else { f.normal });
    let mut out = Vec::new();
    for &g in &order {
        let g = &groups[g];
        let f = &raw[g[0]];
        let mut face = Face { path: BezPath::new(), color: lit(f), even_odd: false, gradient: None, edges: Vec::new(), top_edges: Vec::new() };
        match f.cap {
            Some(front) => {
                for r in &closed {
                    for (k, p) in r.pts.iter().enumerate() {
                        let q = flat(if front { front_pt(&turn, *p, half) } else { back_pt(&turn, *p, half, taper) });
                        if k == 0 { face.path.move_to(q) } else { face.path.line_to(q) }
                    }
                    face.path.close_path();
                }
                face.even_odd = true;
            }
            None => {
                // Along the front edges, then back along the back edges.
                face.path.move_to(flat(f.pts[0]));
                for &i in g {
                    face.path.line_to(flat(raw[i].pts[1]));
                }
                for &i in g.iter().rev() {
                    face.path.line_to(flat(raw[i].pts[2]));
                }
                face.path.line_to(flat(f.pts[3]));
                face.path.close_path();
                if g.len() > 1 {
                    // Colours across the stretch, sampled evenly from slice to slice.
                    let mut stops: Vec<(f64, Color)> = g.iter().map(|&i| (at(&raw[i]), lit(&raw[i]))).collect();
                    stops.sort_by(|a, b| a.0.total_cmp(&b.0));
                    let (lo, hi) = (stops[0].0, stops[stops.len() - 1].0);
                    let count = (g.len() + 1).clamp(2, 24);
                    let colors = (0..count)
                        .map(|k| {
                            let x = lo + (hi - lo) * k as f64 / (count - 1) as f64;
                            let j = stops.partition_point(|s| s.0 < x).clamp(1, stops.len() - 1);
                            let (a, b) = (stops[j - 1], stops[j]);
                            let t = if b.0 - a.0 > 1e-12 { ((x - a.0) / (b.0 - a.0)).clamp(0.0, 1.0) } else { 0.0 };
                            a.1.lerp(b.1, t)
                        })
                        .collect();
                    face.color = stops[stops.len() / 2].1;
                    face.gradient = Some(((across * lo).to_point(), (across * hi).to_point(), colors));
                }
            }
        }
        out.push(face);
    }
    if ink {
        for e in &edges {
            let vb = e.b.is_some_and(|b| visible[b]);
            let (owner, show) = match (visible[e.a], vb) {
                (true, false) => (e.a, true),
                (false, true) => (e.b.unwrap(), true),
                (true, true) => {
                    let b = e.b.unwrap();
                    let owner = if rank[b] > rank[e.a] { b } else { e.a };
                    (owner, e.crease || raw[e.a].normal.dot(raw[b].normal) < SHARP)
                }
                (false, false) => (e.a, false),
            };
            let (p, q) = (flat(e.from), flat(e.to));
            if show && (p - q).hypot() > 1e-6 {
                let k = rank[owner].unwrap();
                // Covered somewhere by a face drawn later that does not touch the edge.
                let touches = |o: usize| groups[o].contains(&e.a) || e.b.is_some_and(|b| groups[o].contains(&b));
                let covered = out[k + 1..]
                    .iter()
                    .zip(&order[k + 1..])
                    .filter(|(_, o)| !touches(**o))
                    .any(|(f, _)| [0.2, 0.5, 0.8].iter().any(|&t| f.path.contains(p.lerp(q, t))));
                let line = vec![p, q];
                if covered { out[k].edges.push(line) } else { out[k].top_edges.push(line) }
            }
        }
    }
    out
}

fn edges_n(r: &Ring) -> usize {
    if r.closed { r.pts.len() } else { r.pts.len() - 1 }
}

fn front_pt(turn: &Turn, p: Point, half: f64) -> V3 {
    turn.apply(V3::new(p.x, p.y, half))
}

fn back_pt(turn: &Turn, p: Point, half: f64, taper: f64) -> V3 {
    turn.apply(V3::new(p.x * taper, p.y * taper, -half))
}

impl Node {
    /// The faces of this node's body as drawn, back to front, lit by `light`. `None` when it is
    /// not a body.
    pub fn body_faces(&self, light: f64) -> Option<Vec<Face>> {
        let body = self.body?;
        let path = self.path()?;
        let fill = self.fill.or(self.gradient.as_ref().and_then(|g| g.colors.first().copied())).or(self.stroke)?;
        let ink = self.fill.is_some() && self.stroke.is_some() && self.stroke_width > 0.0;
        Some(faces(&path, body, self.yaw, self.pitch, light, fill, ink, self.brush.bands))
    }
}

/// Everything the faces cover, as one outline filled nonzero: for shadows and sticker outlines.
pub fn silhouette(faces: &[Face]) -> BezPath {
    let mut out = BezPath::new();
    for f in faces {
        let mut pts: Vec<Point> = Vec::new();
        let mut flush = |pts: &mut Vec<Point>| {
            if pts.len() > 2 {
                if area(pts) < 0.0 {
                    pts.reverse();
                }
                out.move_to(pts[0]);
                for p in &pts[1..] {
                    out.line_to(*p);
                }
                out.close_path();
            }
            pts.clear();
        };
        for el in f.path.elements() {
            match *el {
                PathEl::MoveTo(p) => {
                    flush(&mut pts);
                    pts.push(p);
                }
                PathEl::LineTo(p) => pts.push(p),
                _ => flush(&mut pts),
            }
        }
        flush(&mut pts);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use kurbo::Shape;

    fn square() -> BezPath {
        kurbo::Rect::new(-50.0, -50.0, 50.0, 50.0).to_path(0.1)
    }

    #[test]
    fn a_cube_shows_three_faces_lit_differently() {
        // Turned right and tipped towards the viewer: front, left side and top show.
        let f = faces(&square(), Body::new(100.0), 30.0, 25.0, 90.0, Color::rgb(200, 100, 50), true, 0);
        assert_eq!(f.len(), 3, "{f:?}");
        // The front cap is the nearest plane, so it comes last.
        assert!(f[2].even_odd);
        let colors: Vec<Color> = f.iter().map(|f| f.color).collect();
        assert!(colors[0] != colors[1] && colors[1] != colors[2], "{colors:?}");
        // Inked: the outline and the three edges meeting at the near corner, nothing else.
        let ink: usize = f.iter().map(|f| f.edges.len() + f.top_edges.len()).sum();
        assert_eq!(ink, 9);
        // Seen straight on, only the front shows.
        let flat = faces(&square(), Body::new(100.0), 0.0, 0.0, 90.0, Color::BLACK, false, 0);
        assert_eq!(flat.len(), 1);
        assert_eq!(flat[0].path.bounding_box(), square().bounding_box());
    }

    #[test]
    fn a_cylinder_wall_is_one_smooth_face() {
        let circle = kurbo::Circle::new((0.0, 0.0), 40.0).to_path(0.1);
        let f = faces(&circle, Body::new(120.0), 0.0, -60.0, 90.0, Color::rgb(90, 220, 160), true, 0);
        // The wall and the cap, nothing in slices.
        assert_eq!(f.len(), 2, "{}", f.len());
        assert!(f[0].gradient.is_some());
        // No ink across the curved wall: lines along it only at the silhouette, x = ±40.
        let along: Vec<f64> = f.iter().flat_map(|f| f.edges.iter().chain(&f.top_edges)).filter(|e| (e[1] - e[0]).hypot() > 10.0).map(|e| e[0].x).collect();
        assert_eq!(along.len(), 2, "{along:?}");
        assert!(along.iter().all(|x| (x.abs() - 40.0).abs() < 0.5), "{along:?}");
    }

    #[test]
    fn a_cone_narrows_to_a_point() {
        let circle = kurbo::Circle::new((0.0, 0.0), 40.0).to_path(0.1);
        let side = faces(&circle, Body { depth: 100.0, taper: 0.0 }, 0.0, 90.0, 90.0, Color::WHITE, false, 0);
        // From the side (tipped by 90°) it is a triangle: 80 wide at the base, 100 tall.
        let b = side.iter().map(|f| f.path.bounding_box()).reduce(|a, b| a.union(b)).unwrap();
        assert!((b.width() - 80.0).abs() < 1.0 && (b.height() - 100.0).abs() < 1.0, "{b:?}");
    }

    #[test]
    fn a_ring_shows_its_hole() {
        // A square with a square hole, turned: the hole's inner walls face into it.
        let mut p = square();
        // Drawn the same way round as the outside: still a hole.
        p.extend(kurbo::Rect::new(-20.0, -20.0, 20.0, 20.0).to_path(0.1));
        let f = faces(&p, Body::new(40.0), 40.0, 0.0, 90.0, Color::WHITE, true, 0);
        let cap = f.last().unwrap();
        assert!(cap.even_odd);
        // One outer side and one inner wall show besides the cap.
        assert_eq!(f.len(), 3, "{}", f.len());
    }
}
