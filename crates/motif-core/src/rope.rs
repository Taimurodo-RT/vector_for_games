//! Ropes and cloth: the outline of a shape becomes a chain of points (`rope`) or a grid of them
//! (`cloth`) held at fixed distances, pinned where it is fastened, pulled by gravity and wind,
//! and draped over `solid` shapes. Objects can hang on a rope (`hang`) and a shape can be
//! drawn as the links of a chain along it (`links`). Like the rest of the simulation it runs
//! from the start of the animation in fixed steps, so export bakes it.

use crate::soft::{Wall, inside, nearest_edge, smooth, solids};
use crate::{Document, SIM_FPS};
use kurbo::{Affine, BezPath, PathEl, Point, Vec2};

/// Where a rope or a cloth is fastened.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pin {
    /// Rope: its first point.
    Start,
    /// Rope: its last point.
    End,
    /// Cloth: a whole edge.
    Top,
    Bottom,
    Left,
    Right,
    /// Cloth: one corner.
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

impl Pin {
    pub const ALL: [Pin; 10] =
        [Pin::Start, Pin::End, Pin::Top, Pin::Bottom, Pin::Left, Pin::Right, Pin::TopLeft, Pin::TopRight, Pin::BottomLeft, Pin::BottomRight];

    pub fn word(self) -> &'static str {
        match self {
            Pin::Start => "start",
            Pin::End => "end",
            Pin::Top => "top",
            Pin::Bottom => "bottom",
            Pin::Left => "left",
            Pin::Right => "right",
            Pin::TopLeft => "top-left",
            Pin::TopRight => "top-right",
            Pin::BottomLeft => "bottom-left",
            Pin::BottomRight => "bottom-right",
        }
    }

    pub fn from_word(w: &str) -> Option<Pin> {
        Pin::ALL.into_iter().find(|p| p.word() == w)
    }

    /// True for the ends of a rope, false for the edges and corners of a cloth.
    pub fn of_rope(self) -> bool {
        matches!(self, Pin::Start | Pin::End)
    }
}

/// Settings of a rope or a cloth (`rope 1 0.02 pin start gravity 900 wind 0`).
#[derive(Clone, Debug, PartialEq)]
pub struct Rope {
    /// A grid of points filling the shape instead of a line along its outline.
    pub cloth: bool,
    /// How much of its stretch the rope takes back each frame, 0..1: 1 does not stretch,
    /// lower is elastic.
    pub stiff: f64,
    /// How fast swinging dies out, 0..1 per frame.
    pub damp: f64,
    /// Downward acceleration, artboard units per second squared.
    pub gravity: f64,
    /// Strength of the gusting wind to the right (to the left when below zero).
    pub wind: f64,
    /// Points held in place: they follow the shape as keys and bones move it.
    pub pins: Vec<Pin>,
}

impl Default for Rope {
    /// A rope pinned at its start.
    fn default() -> Self {
        Rope { cloth: false, stiff: 1.0, damp: 0.02, gravity: 900.0, wind: 0.0, pins: vec![Pin::Start] }
    }
}

impl Rope {
    pub fn cloth() -> Rope {
        Rope { cloth: true, pins: vec![Pin::Top], ..Rope::default() }
    }

    /// The pins written when none are: the rope's start, the cloth's top edge.
    pub fn default_pins(cloth: bool) -> Vec<Pin> {
        if cloth { vec![Pin::Top] } else { vec![Pin::Start] }
    }
}

/// Distance between the points of a rope, and the most points it gets.
const ROPE_SPACING: f64 = 6.0;
const ROPE_MAX: usize = 160;
/// Size of a cloth's grid cell, and the most points along a side.
const CLOTH_CELL: f64 = 12.0;
const CLOTH_MAX: usize = 28;
/// Passes over the distances in each step.
const ITERATIONS: usize = 6;
/// How much of a sliding point's speed a wall takes per step.
const FRICTION: f64 = 0.3;

/// What holds a point in place.
#[derive(Clone, Debug, PartialEq)]
enum Anchor {
    /// Its spot on the shape, in the shape's own space.
    Own(Point),
    /// The pivot of another object.
    Tie(usize),
}

/// A cloth's outline: pieces of it, each point as a grid cell and a spot in it, and whether
/// the piece is closed.
type Outline = Vec<(Vec<(usize, f64, f64)>, bool)>;

#[derive(Clone, Debug, PartialEq)]
enum Form {
    Line {
        closed: bool,
    },
    /// Points row by row; the outline is kept as cells and spots within them.
    Grid {
        cols: usize,
        rows: usize,
        outline: Outline,
    },
}

/// A rope or a cloth being simulated.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Strand {
    node: usize,
    rope: Rope,
    p: Vec<Point>,
    old: Vec<Point>,
    /// Inverse weight of each point: 0 holds it, objects hanging on a point make it heavier.
    inv: Vec<f64>,
    anchors: Vec<Option<Anchor>>,
    /// Pairs of points kept apart: their rest distance and how firmly.
    sticks: Vec<(usize, usize, f64, f64)>,
    form: Form,
    /// Half the stroke: the rope stops at a wall with its edge, not its middle.
    margin: f64,
    time: f64,
    /// Where each point is in the wind's waves, from its place on the drawn shape.
    phase: Vec<f64>,
}

/// The first piece of `path`'s outline as a polyline with points at most `step` apart, and
/// whether it is closed.
fn polyline(path: &BezPath, step: f64) -> (Vec<Point>, bool) {
    let (mut out, mut closed, mut started) = (Vec::<Point>::new(), false, false);
    let mut more = true;
    kurbo::flatten(path, 0.1, |el| {
        if !more {
            return;
        }
        match el {
            PathEl::MoveTo(p) if !started => {
                started = true;
                out.push(p);
            }
            PathEl::MoveTo(_) => more = false,
            PathEl::LineTo(p) => {
                let a = *out.last().unwrap_or(&p);
                let n = ((p - a).hypot() / step).ceil().max(1.0) as usize;
                for k in 1..=n {
                    out.push(a.lerp(p, k as f64 / n as f64));
                }
            }
            PathEl::ClosePath => {
                closed = true;
                more = false;
            }
            _ => {}
        }
    });
    if out.len() > 2 && (out[0] - *out.last().unwrap()).hypot() < 1e-6 {
        closed = true;
    }
    if closed && out.len() > 1 && (out[0] - *out.last().unwrap()).hypot() < 1e-6 {
        out.pop();
    }
    (out, closed)
}

/// Points along a polyline at equal distances, `count` of them; the last one is its end unless
/// it is closed.
fn resample(line: &[Point], closed: bool, count: usize) -> Vec<Point> {
    let mut pts = line.to_vec();
    if closed && !line.is_empty() {
        pts.push(line[0]);
    }
    let lens: Vec<f64> = pts.windows(2).map(|w| (w[1] - w[0]).hypot()).collect();
    let total: f64 = lens.iter().sum();
    if total < 1e-9 || count < 2 {
        return pts.into_iter().take(1).collect();
    }
    let gaps = if closed { count } else { count - 1 };
    let (mut out, mut seg, mut at) = (Vec::with_capacity(count), 0, 0.0);
    for k in 0..count {
        let want = total * k as f64 / gaps as f64;
        while seg + 1 < lens.len() && at + lens[seg] < want {
            at += lens[seg];
            seg += 1;
        }
        let t = if lens[seg] > 0.0 { ((want - at) / lens[seg]).clamp(0.0, 1.0) } else { 0.0 };
        out.push(pts[seg].lerp(pts[seg + 1], t));
    }
    out
}

/// A point and the direction of the polyline `p` at `f` of its length (0 start, 1 end).
fn along(p: &[Point], f: f64) -> (Point, Vec2) {
    let lens: Vec<f64> = p.windows(2).map(|w| (w[1] - w[0]).hypot()).collect();
    let total: f64 = lens.iter().sum();
    if p.len() < 2 || total < 1e-9 {
        return (p.first().copied().unwrap_or_default(), Vec2::new(0.0, 1.0));
    }
    let mut want = total * f.clamp(0.0, 1.0);
    for (i, l) in lens.iter().enumerate() {
        if want <= *l || i + 1 == lens.len() {
            let t = if *l > 0.0 { (want / l).clamp(0.0, 1.0) } else { 0.0 };
            return (p[i].lerp(p[i + 1], t), (p[i + 1] - p[i]).normalize());
        }
        want -= l;
    }
    unreachable!()
}

/// An open smooth curve through `p` (Catmull-Rom as cubic Béziers).
fn smooth_open(p: &[Point]) -> BezPath {
    let mut out = BezPath::new();
    let n = p.len();
    if n == 0 {
        return out;
    }
    out.move_to(p[0]);
    for i in 0..n.saturating_sub(1) {
        let (a, b, c, d) = (p[i.saturating_sub(1)], p[i], p[i + 1], p[(i + 2).min(n - 1)]);
        out.curve_to(b + (c - a) / 6.0, c - (d - b) / 6.0, c);
    }
    out
}

/// Where the pivot of object `i` is on the artboard.
fn pivot_of(doc: &Document, i: usize) -> Point {
    doc.world_transform(i) * doc.nodes[i].pivot.to_point()
}

impl Strand {
    fn new(doc: &Document, node: usize, rope: Rope) -> Option<Strand> {
        let n = &doc.nodes[node];
        let base = n.base_path()?;
        let w = doc.world_transform(node);
        let ties: Vec<usize> = n.tie.iter().filter_map(|id| doc.index_of(id)).filter(|&t| t != node).collect();
        let (rest, sticks, form, mut anchors) = if rope.cloth {
            let b = kurbo::Shape::bounding_box(&base);
            if b.width() < 1e-6 || b.height() < 1e-6 {
                return None;
            }
            let cols = ((b.width() / CLOTH_CELL).ceil() as usize).clamp(2, CLOTH_MAX) + 1;
            let rows = ((b.height() / CLOTH_CELL).ceil() as usize).clamp(2, CLOTH_MAX) + 1;
            let (cw, ch) = (b.width() / (cols - 1) as f64, b.height() / (rows - 1) as f64);
            let rest: Vec<Point> = (0..rows * cols).map(|k| Point::new(b.x0 + (k % cols) as f64 * cw, b.y0 + (k / cols) as f64 * ch)).collect();
            let mut sticks = Vec::new();
            let at = |c: usize, r: usize| r * cols + c;
            let mut stick = |a: usize, b: usize, k: f64| sticks.push((a, b, (rest[b] - rest[a]).hypot(), k));
            for r in 0..rows {
                for c in 0..cols {
                    if c + 1 < cols {
                        stick(at(c, r), at(c + 1, r), 1.0);
                    }
                    if r + 1 < rows {
                        stick(at(c, r), at(c, r + 1), 1.0);
                    }
                    if c + 1 < cols && r + 1 < rows {
                        stick(at(c, r), at(c + 1, r + 1), 0.1);
                        stick(at(c + 1, r), at(c, r + 1), 0.1);
                    }
                    // Across two cells: keeps the cloth from crumpling into a heap.
                    if c + 2 < cols {
                        stick(at(c, r), at(c + 2, r), 0.1);
                    }
                    if r + 2 < rows {
                        stick(at(c, r), at(c, r + 2), 0.1);
                    }
                }
            }
            // Every subpath of the outline, each point as a cell and a spot in it.
            let mut outline = Vec::new();
            let mut cur: Vec<Point> = Vec::new();
            let mut flush = |cur: &mut Vec<Point>, closed: bool| {
                if cur.len() > 1 {
                    let spots = cur
                        .iter()
                        .map(|q| {
                            let u = ((q.x - b.x0) / cw).clamp(0.0, (cols - 1) as f64);
                            let v = ((q.y - b.y0) / ch).clamp(0.0, (rows - 1) as f64);
                            let (c, r) = ((u.floor() as usize).min(cols - 2), (v.floor() as usize).min(rows - 2));
                            (at(c, r), u - c as f64, v - r as f64)
                        })
                        .collect();
                    outline.push((spots, closed));
                }
                cur.clear();
            };
            kurbo::flatten(&base, 0.1, |el| match el {
                PathEl::MoveTo(p) => {
                    flush(&mut cur, false);
                    cur.push(p);
                }
                PathEl::LineTo(p) => {
                    let a = *cur.last().unwrap_or(&p);
                    let n = ((p - a).hypot() / 4.0).ceil().max(1.0) as usize;
                    cur.extend((1..=n).map(|k| a.lerp(p, k as f64 / n as f64)));
                }
                PathEl::ClosePath => flush(&mut cur, true),
                _ => {}
            });
            flush(&mut cur, false);
            let mut anchors = vec![None; rest.len()];
            let (last_c, last_r) = (cols - 1, rows - 1);
            for pin in &rope.pins {
                let held: Vec<usize> = match pin {
                    Pin::Top => (0..cols).map(|c| at(c, 0)).collect(),
                    Pin::Bottom => (0..cols).map(|c| at(c, last_r)).collect(),
                    Pin::Left => (0..rows).map(|r| at(0, r)).collect(),
                    Pin::Right => (0..rows).map(|r| at(last_c, r)).collect(),
                    Pin::TopLeft => vec![at(0, 0)],
                    Pin::TopRight => vec![at(last_c, 0)],
                    Pin::BottomLeft => vec![at(0, last_r)],
                    Pin::BottomRight => vec![at(last_c, last_r)],
                    Pin::Start | Pin::End => Vec::new(),
                };
                for k in held {
                    anchors[k] = Some(Anchor::Own(rest[k]));
                }
            }
            (rest, sticks, Form::Grid { cols, rows, outline }, anchors)
        } else {
            let (line, closed) = polyline(&base, 1.0);
            if line.len() < 2 {
                return None;
            }
            let length: f64 = line.windows(2).map(|w| (w[1] - w[0]).hypot()).sum::<f64>() + if closed { (line[0] - line[line.len() - 1]).hypot() } else { 0.0 };
            let count = ((length / ROPE_SPACING).ceil() as usize + 1).clamp(3, ROPE_MAX);
            let rest = resample(&line, closed, count);
            let n = rest.len();
            let mut sticks: Vec<_> = (0..n - 1).map(|i| (i, i + 1, (rest[i + 1] - rest[i]).hypot(), 1.0)).collect();
            if closed {
                sticks.push((n - 1, 0, (rest[0] - rest[n - 1]).hypot(), 1.0));
            }
            let mut anchors = vec![None; n];
            for pin in &rope.pins {
                match pin {
                    Pin::Start => anchors[0] = Some(Anchor::Own(rest[0])),
                    Pin::End => anchors[n - 1] = Some(Anchor::Own(rest[n - 1])),
                    _ => {}
                }
            }
            (rest, sticks, Form::Line { closed }, anchors)
        };
        // Tied ends: one object holds the end, two hold the start and the end.
        if !rope.cloth {
            let last = rest.len() - 1;
            match ties.as_slice() {
                [end] => anchors[last] = Some(Anchor::Tie(*end)),
                [start, end, ..] => {
                    anchors[0] = Some(Anchor::Tie(*start));
                    anchors[last] = Some(Anchor::Tie(*end));
                }
                [] => {}
            }
        }
        let p: Vec<Point> = rest.iter().map(|q| w * *q).collect();
        let mut inv = vec![1.0; p.len()];
        // Objects hanging on the rope weigh it down where they hang.
        if let Form::Line { .. } = form {
            for h in &doc.nodes {
                if let Some((id, f)) = &h.hang
                    && *id == n.id
                {
                    let (spot, _) = along(&p, *f);
                    let near = (0..p.len()).min_by(|&a, &b| (p[a] - spot).hypot2().total_cmp(&(p[b] - spot).hypot2())).unwrap();
                    inv[near] = 0.25;
                }
            }
        }
        for (k, a) in anchors.iter().enumerate() {
            if a.is_some() {
                inv[k] = 0.0;
            }
        }
        let margin = if n.stroke.is_some() && !rope.cloth { n.stroke_width / 2.0 } else { 0.0 };
        let phase = rest.iter().map(|q| (q.x + q.y * 0.5) * 0.06).collect();
        Some(Strand { node, rope, old: p.clone(), p, inv, anchors, sticks, form, margin, time: 0.0, phase })
    }

    fn step(&mut self, pose: &Document, walls: &[Wall], dt: f64, steps: usize) {
        let w = pose.world_transform(self.node);
        let r = &self.rope;
        let keep = (1.0 - r.damp.clamp(0.0, 1.0)).powf(dt * SIM_FPS);
        // Firmness is the share of the stretch taken back per frame; this is that share per pass.
        let passes = (ITERATIONS * steps) as f64;
        let per_pass = |k: f64| if k >= 1.0 { 1.0 } else { 1.0 - (1.0 - k.max(0.0)).powf(1.0 / passes) };
        let stiff = r.stiff.clamp(0.0, 1.0);
        self.time += dt;
        let t = self.time;
        // Gusts: the wind swells and dies down, and runs along the rope or the cloth in waves.
        let gust = r.wind * (0.7 + 0.3 * (t * 1.3).sin() * (t * 0.47 + 1.0).sin());
        let node = &pose.nodes[self.node];
        let pull = pose.world.pull(node, r.gravity);
        for i in 0..self.p.len() {
            match &self.anchors[i] {
                Some(a) => {
                    let to = match a {
                        Anchor::Own(q) => w * *q,
                        Anchor::Tie(o) if *o < pose.nodes.len() => pivot_of(pose, *o),
                        Anchor::Tie(_) => self.p[i],
                    };
                    self.old[i] = to;
                    self.p[i] = to;
                }
                None => {
                    let ph = self.phase[i];
                    let acc = Vec2::new(gust * (1.0 + 0.5 * (t * 5.0 - ph).sin()), gust * (t * 4.0 - ph * 1.3).sin()) + pull + pose.world.blow(node, self.p[i]);
                    let v = (self.p[i] - self.old[i]) * keep;
                    self.old[i] = self.p[i];
                    self.p[i] += v + acc * dt * dt;
                }
            }
        }
        // Cloth is seen flat but folds in depth: it resists stretching, and gives way to
        // squeezing, which is how a fold looks from the front.
        let squeeze = if r.cloth { 0.05 } else { 1.0 };
        let firm: Vec<(f64, f64)> = self.sticks.iter().map(|s| (per_pass(s.3 * stiff), per_pass(s.3 * squeeze))).collect();
        for _ in 0..ITERATIONS {
            for (&(a, b, len, _), &(pull, push)) in self.sticks.iter().zip(&firm) {
                let (wa, wb) = (self.inv[a], self.inv[b]);
                if wa + wb <= 0.0 {
                    continue;
                }
                let d = self.p[b] - self.p[a];
                let l = d.hypot();
                if l < 1e-9 {
                    continue;
                }
                let k = if l < len { push } else { pull };
                let fix = d * ((l - len) / l / (wa + wb) * k);
                self.p[a] += fix * wa;
                self.p[b] -= fix * wb;
            }
        }
        for wall in walls {
            let margin = wall.margin + self.margin;
            for i in 0..self.p.len() {
                if self.inv[i] <= 0.0 {
                    continue;
                }
                let q = self.p[i];
                let into = inside(&wall.poly, q);
                let (on, along) = nearest_edge(&wall.poly, q);
                let gap = (q - on).hypot();
                if !into && gap >= margin {
                    continue;
                }
                let out = if gap > 1e-9 { (if into { on - q } else { q - on }) / gap } else { Vec2::new(along.y, -along.x) };
                self.p[i] = on + out * margin;
                let v = self.p[i] - self.old[i];
                self.old[i] = self.p[i] - along * v.dot(along) * (1.0 - FRICTION);
            }
        }
    }

    /// The outline as simulated, on the artboard: the rope's points, or the cloth's outline
    /// carried by its grid.
    fn outline(&self) -> Vec<(Vec<Point>, bool)> {
        match &self.form {
            Form::Line { closed } => vec![(self.p.clone(), *closed)],
            Form::Grid { cols, outline, .. } => outline
                .iter()
                .map(|(spots, closed)| {
                    let pts = spots
                        .iter()
                        .map(|&(k, u, v)| {
                            let (a, b, c, d) = (self.p[k], self.p[k + 1], self.p[k + cols], self.p[k + cols + 1]);
                            let top = a.lerp(b, u);
                            top.lerp(c.lerp(d, u), v)
                        })
                        .collect();
                    (pts, *closed)
                })
                .collect(),
        }
    }
}

/// Ropes and cloths of `pose`, hanging as they are drawn.
pub(crate) fn strands(pose: &Document) -> Vec<Strand> {
    (0..pose.nodes.len()).filter_map(|i| Strand::new(pose, i, pose.nodes[i].rope.clone()?)).collect()
}

pub(crate) fn step_strands(strands: &mut [Strand], pose: &Document, dt: f64, steps: usize) {
    if strands.is_empty() {
        return;
    }
    let walls = solids(pose);
    let h = dt / steps as f64;
    for s in strands.iter_mut() {
        // A rope does not bump into itself.
        let own: Vec<Wall> = walls.iter().filter(|w| w.node != s.node).cloned().collect();
        for _ in 0..steps {
            s.step(pose, &own, h, steps);
        }
    }
}

/// Bends ropes and cloths of `doc` as simulated, hangs objects on them and lays chain links
/// along them.
pub(crate) fn apply_strands(strands: &[Strand], doc: &mut Document) {
    for s in strands {
        if s.node >= doc.nodes.len() {
            continue;
        }
        let to_local = doc.world_transform(s.node).inverse();
        let mut path = BezPath::new();
        for (pts, closed) in s.outline() {
            let local: Vec<Point> = pts.iter().map(|q| to_local * *q).collect();
            let piece = match (&s.form, closed) {
                (Form::Line { .. }, true) => smooth(&local),
                (Form::Line { .. }, false) => smooth_open(&local),
                (Form::Grid { .. }, _) => {
                    let mut b = BezPath::new();
                    b.move_to(local[0]);
                    for q in &local[1..] {
                        b.line_to(*q);
                    }
                    if closed {
                        b.close_path();
                    }
                    b
                }
            };
            path.extend(piece);
        }
        doc.nodes[s.node].deformed = Some(path);
    }
    let lines: Vec<(String, Vec<Point>)> = strands
        .iter()
        .filter(|s| matches!(s.form, Form::Line { .. }) && s.node < doc.nodes.len())
        .map(|s| (doc.nodes[s.node].id.clone(), s.p.clone()))
        .collect();
    hang_on(doc, &lines);
}

impl Document {
    /// Puts objects that hang on ropes, and chain links, on the ropes: as they are drawn when
    /// nothing is simulated, as simulated after a simulation.
    pub fn hang_ropes(&mut self) {
        let lines: Vec<(String, Vec<Point>)> = (0..self.nodes.len())
            .filter(|&i| self.nodes[i].rope.as_ref().is_some_and(|r| !r.cloth))
            .filter_map(|i| {
                let w = self.world_transform(i);
                // After a simulation the rope's outline is the simulated one.
                let (line, _) = polyline(&self.nodes[i].path()?, 1.0);
                Some((self.nodes[i].id.clone(), line.into_iter().map(|q| w * q).collect()))
            })
            .collect();
        if !lines.is_empty() {
            hang_on(self, &lines);
        }
    }
}

/// Hangs objects on ropes and lays chain links along them, in the order they are listed: a
/// lamp on a rope that hangs from another lamp follows it. `lines` are the ropes' points on the
/// artboard, by id.
fn hang_on(doc: &mut Document, lines: &[(String, Vec<Point>)]) {
    let line = |id: &str| lines.iter().find(|l| l.0 == id).map(|l| &l.1).filter(|p| p.len() > 1);
    for i in 0..doc.nodes.len() {
        if let Some((id, f)) = doc.nodes[i].hang.clone()
            && let Some(p) = line(&id)
        {
            let (at, _) = along(p, f);
            // At the end the object turns with the last bit of the rope, not its last kink.
            let n = p.len();
            let dir = (p[n - 1] - p[n.saturating_sub(5)]).normalize();
            let parent = doc.nodes[i].parent.map_or(Affine::IDENTITY, |p| doc.world_transform(p));
            let local = parent.inverse() * at;
            let n = &mut doc.nodes[i];
            (n.x, n.y) = (local.x - n.pivot.x, local.y - n.pivot.y);
            // At an end the object turns with the rope, like a weight; in between it hangs straight.
            n.swing = if f >= 0.999 { dir.y.atan2(dir.x).to_degrees() - 90.0 } else { 0.0 };
        }
        if let Some(id) = doc.nodes[i].links.clone()
            && let Some(p) = line(&id)
        {
            doc.nodes[i].particles = Some(chain(doc, i, p));
        }
    }
}

/// Links of a chain along `p`: copies of shape `i`, every other one seen from the side.
fn chain(doc: &Document, i: usize, p: &[Point]) -> Vec<(Affine, f64)> {
    let [a, b, c, d, _, _] = doc.world_transform(i).as_coeffs();
    let shape = Affine::new([a, b, c, d, 0.0, 0.0]);
    let size = (shape * Point::new(doc.nodes[i].width.abs(), 0.0)).to_vec2().hypot();
    let step = (size * 0.75).max(1.0);
    let total: f64 = p.windows(2).map(|w| (w[1] - w[0]).hypot()).sum();
    let count = ((total / step).floor() as usize).clamp(1, 2000);
    (0..count)
        .map(|k| {
            let (at, dir) = along(p, ((k as f64 + 0.5) * step / total).min(1.0));
            let side = if k % 2 == 1 { Affine::scale_non_uniform(1.0, 0.35) } else { Affine::IDENTITY };
            (Affine::translate(at.to_vec2()) * Affine::rotate(dir.y.atan2(dir.x)) * side * shape, 1.0)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use crate::{Animation, Artboard, Document, Node, Pin, Rope, ShapeKind, SimCache};
    use kurbo::{BezPath, Point, Shape};

    fn line(id: &str, x: f64, y: f64, len: f64) -> Node {
        let mut n = Node::new(id, ShapeKind::Path);
        let mut g = BezPath::new();
        g.move_to((-len / 2.0, 0.0));
        g.line_to((len / 2.0, 0.0));
        n.geometry = g;
        (n.x, n.y, n.width, n.height) = (x, y, len, 0.0);
        n
    }

    fn world(d: &Document, id: &str) -> kurbo::Rect {
        let i = d.index_of(id).unwrap();
        (d.world_transform(i) * d.nodes[i].path().unwrap()).bounding_box()
    }

    fn still(d: &Document, t: f64) -> Document {
        Animation { name: "Go".into(), duration: 10.0, looping: false, tracks: Vec::new() }.pose_with(d, t, &mut SimCache::default())
    }

    #[test]
    fn a_rope_pinned_at_its_start_falls_and_hangs_down() {
        let mut d = Document::new(Artboard::default());
        let mut r = line("rope", 200.0, 50.0, 100.0);
        r.rope = Some(Rope { damp: 0.1, ..Rope::default() });
        d.nodes.push(r);
        let b = world(&still(&d, 4.0), "rope");
        // It hangs from (150, 50) straight down and keeps its length.
        assert!((b.min_x() - 150.0).abs() < 4.0 && b.width() < 8.0, "{b:?}");
        assert!((b.height() - 100.0).abs() < 4.0, "{b:?}");
        // In design (no simulation) it is drawn as it was drawn.
        let mut plain = d.clone();
        plain.rig();
        assert!(world(&plain, "rope").height() < 1e-6);
    }

    #[test]
    fn a_rope_pinned_at_both_ends_sags_and_a_lamp_hangs_on_it() {
        let mut d = Document::new(Artboard::default());
        let mut r = line("rope", 200.0, 50.0, 100.0);
        r.rope = Some(Rope { pins: vec![Pin::Start, Pin::End], stiff: 0.9, damp: 0.1, ..Rope::default() });
        d.nodes.push(r);
        let mut lamp = Node::new("lamp", ShapeKind::Ellipse);
        (lamp.width, lamp.height) = (10.0, 10.0);
        lamp.hang = Some(("rope".into(), 0.5));
        d.nodes.push(lamp);
        let p = still(&d, 4.0);
        let b = world(&p, "rope");
        assert!(b.height() > 5.0, "sags: {b:?}");
        assert!((b.min_x() - 150.0).abs() < 1.0 && (b.max_x() - 250.0).abs() < 1.0, "ends held: {b:?}");
        let lamp = p.world_transform(1) * Point::ZERO;
        assert!((lamp.x - 200.0).abs() < 2.0 && (lamp.y - b.max_y()).abs() < 2.0, "lamp at the lowest point: {lamp:?} {b:?}");
    }

    #[test]
    fn a_cloth_pinned_at_the_top_drapes_over_a_solid() {
        let mut d = Document::new(Artboard::default());
        let mut c = Node::new("flag", ShapeKind::Rect);
        (c.x, c.y, c.width, c.height) = (200.0, 50.0, 80.0, 60.0);
        c.rope = Some(Rope { damp: 0.1, ..Rope::cloth() });
        d.nodes.push(c);
        // Hanging, it keeps its top edge and its height.
        let b = world(&still(&d, 3.0), "flag");
        assert!((b.min_y() - 20.0).abs() < 1.0 && (b.width() - 80.0).abs() < 2.0, "{b:?}");
        assert!((b.height() - 60.0).abs() < 6.0, "{b:?}");
        // Fastened by its left edge it swings down instead.
        d.nodes[0].rope.as_mut().unwrap().pins = vec![Pin::Left];
        let b = world(&still(&d, 4.0), "flag");
        assert!(b.width() < 70.0 && b.max_y() > 90.0, "{b:?}");
    }

    #[test]
    fn a_rope_lies_on_a_solid_under_it() {
        let mut d = Document::new(Artboard::default());
        let mut r = line("rope", 200.0, 50.0, 100.0);
        r.rope = Some(Rope { damp: 0.1, ..Rope::default() });
        d.nodes.push(r);
        let mut table = Node::new("table", ShapeKind::Rect);
        (table.x, table.y, table.width, table.height) = (200.0, 110.0, 200.0, 20.0);
        table.solid = true;
        d.nodes.push(table);
        let b = world(&still(&d, 4.0), "rope");
        assert!(b.max_y() < 101.0, "stopped by the table: {b:?}");
        assert!(b.width() > 30.0, "lies along it: {b:?}");
    }

    #[test]
    fn a_chain_draws_its_links_along_the_rope() {
        let mut d = Document::new(Artboard::default());
        let mut r = line("rope", 200.0, 50.0, 100.0);
        r.rope = Some(Rope::default());
        d.nodes.push(r);
        let mut link = Node::new("link", ShapeKind::Ellipse);
        (link.width, link.height) = (16.0, 10.0);
        link.links = Some("rope".into());
        d.nodes.push(link);
        let p = still(&d, 3.0);
        let parts = p.nodes[1].particles.as_ref().unwrap();
        assert_eq!(parts.len(), 8);
        // Down the hanging rope, one after another.
        let ys: Vec<f64> = parts.iter().map(|(a, _)| (*a * Point::ZERO).y).collect();
        assert!(ys.windows(2).all(|w| w[1] > w[0]), "{ys:?}");
    }
}
