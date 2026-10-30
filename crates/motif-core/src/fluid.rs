//! Fluids: a shape filled with water (`fluid`), or a spout pouring it (`flow`). The water is a
//! crowd of particles kept at an even density (position based fluids), pulled by gravity and
//! held by `solid` shapes. It is drawn as one smooth outline around the particles (metaballs
//! traced into a vector path), with the shape's own fill and stroke. Like the rest of the
//! simulation it runs from the start of the animation in fixed steps, so export bakes it.

use crate::particles::{random, salt};
use crate::soft::{Wall, inside, nearest_edge, smooth, solids};
use crate::{Document, SIM_FPS};
use kurbo::{BezPath, Point, Rect, Shape, Vec2};
use std::collections::HashMap;

/// Settings of a fluid (`fluid 0.1 flow 40 gravity 900`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Fluid {
    /// Thickness, 0..1: 0 runs like water, 0.5 like syrup, 1 barely flows.
    pub visc: f64,
    /// Drops per second poured from the shape; 0 fills the shape once instead.
    pub flow: f64,
    /// Downward acceleration, artboard units per second squared.
    pub gravity: f64,
}

impl Default for Fluid {
    fn default() -> Self {
        Fluid { visc: 0.1, flow: 0.0, gravity: 900.0 }
    }
}

/// Distance between drops at rest, in artboard units; bigger fills get coarser drops.
const SPACING: f64 = 6.0;
/// Most drops in one fluid.
const MAX: usize = 2500;
/// Steps per frame, and density passes per step.
const STEPS: usize = 4;
const ITERATIONS: usize = 4;
/// Kernel radius, in drop spacings.
const H: f64 = 2.0;
/// Relaxation of the density constraint.
const EPS: f64 = 0.02;
/// Radius of each drop's blob in the drawn outline, in spacings, and the outline's level.
const BLOB: f64 = 1.6;
const LEVEL: f64 = 0.5;
/// Distance between rows of a fill, in spacings.
const ROW: f64 = 0.87;

fn poly6(r2: f64) -> f64 {
    let h2 = H * H;
    if r2 >= h2 { 0.0 } else { 4.0 / (std::f64::consts::PI * H.powi(8)) * (h2 - r2).powi(3) }
}

fn spiky(d: Vec2) -> Vec2 {
    let r = d.hypot();
    if !(1e-9..H).contains(&r) { Vec2::ZERO } else { d / r * (-30.0 / (std::f64::consts::PI * H.powi(5)) * (H - r).powi(2)) }
}

/// Density of a drop amid drops packed as a fill packs them: rows one spacing apart along,
/// 0.87 apart across, every other row shifted half a spacing.
fn rest_density() -> f64 {
    let mut rho = 0.0;
    for j in -4i32..=4 {
        for i in -4i32..=4 {
            let x = i as f64 + if j % 2 != 0 { 0.5 } else { 0.0 };
            let y = j as f64 * ROW;
            rho += poly6(x * x + y * y);
        }
    }
    rho
}

/// A fluid being simulated. Positions are in drop spacings, on the artboard.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Pool {
    node: usize,
    fluid: Fluid,
    /// Artboard units per drop spacing.
    unit: f64,
    p: Vec<Point>,
    v: Vec<Vec2>,
    /// Drops owed to the flow.
    owed: f64,
    poured: u64,
    /// Where the spout was a step ago, so drops leave it with its speed.
    spout: Option<Point>,
    rho0: f64,
}

/// Indices of drops by grid cell of size `H`.
fn grid(p: &[Point]) -> HashMap<(i64, i64), Vec<usize>> {
    let mut g: HashMap<(i64, i64), Vec<usize>> = HashMap::new();
    for (i, q) in p.iter().enumerate() {
        g.entry(((q.x / H).floor() as i64, (q.y / H).floor() as i64)).or_default().push(i);
    }
    g
}

fn neighbours(p: &[Point]) -> Vec<Vec<usize>> {
    let g = grid(p);
    p.iter()
        .enumerate()
        .map(|(i, q)| {
            let (cx, cy) = ((q.x / H).floor() as i64, (q.y / H).floor() as i64);
            let mut out = Vec::new();
            for dx in -1..=1 {
                for dy in -1..=1 {
                    if let Some(cell) = g.get(&(cx + dx, cy + dy)) {
                        out.extend(cell.iter().copied().filter(|&j| j != i && (p[j] - *q).hypot2() < H * H));
                    }
                }
            }
            out
        })
        .collect()
}

/// A solid shape in drop spacings, with its bounds for a quick miss.
struct Basin {
    poly: Vec<Point>,
    margin: f64,
    bounds: Rect,
}

fn basins(walls: &[Wall], unit: f64) -> Vec<Basin> {
    walls
        .iter()
        .map(|w| {
            let poly: Vec<Point> = w.poly.iter().map(|q| (q.to_vec2() / unit).to_point()).collect();
            let bounds = poly.iter().fold(Rect::from_points(poly[0], poly[0]), |r, q| r.union_pt(*q));
            // The drawn outline reaches about a spacing past the drops: keep them that far off.
            Basin { poly, margin: w.margin / unit + 0.9, bounds }
        })
        .collect()
}

impl Pool {
    fn new(doc: &Document, node: usize, fluid: Fluid) -> Option<Pool> {
        let n = &doc.nodes[node];
        let path = doc.world_transform(node) * n.base_path()?;
        let b = path.bounding_box();
        // Fill the shape with drops in rows, every other row shifted half a spacing.
        let unit = if fluid.flow > 0.0 { SPACING } else { SPACING.max((path.area().abs() / MAX as f64).sqrt()) };
        let mut p = Vec::new();
        if fluid.flow <= 0.0 {
            let (mut y, mut row) = (b.y0 + unit / 2.0, 0);
            while y < b.y1 && p.len() < MAX {
                let mut x = b.x0 + unit / 2.0 + if row % 2 == 1 { unit / 2.0 } else { 0.0 };
                while x < b.x1 && p.len() < MAX {
                    if path.contains(Point::new(x, y)) {
                        p.push(Point::new(x / unit, y / unit));
                    }
                    x += unit;
                }
                y += unit * ROW;
                row += 1;
            }
        }
        Some(Pool { node, fluid, unit, v: vec![Vec2::ZERO; p.len()], p, owed: 0.0, poured: 0, spout: None, rho0: rest_density() })
    }

    /// New drops from the spout: they leave its lower edge, along the shape's own "down", in
    /// columns a spacing apart, fast enough that each is a spacing behind the one before, and
    /// carried along as the shape moves.
    fn pour(&mut self, pose: &Document, dt: f64) {
        let n = &pose.nodes[self.node];
        let w = pose.world_transform(self.node);
        let (width, height) = (n.width.abs(), n.height.abs());
        let mouth = w * Point::new(0.0, height / 2.0);
        let across = (w * Point::new(1.0, 0.0) - w * Point::ZERO).normalize();
        let down = (w * Point::new(0.0, 1.0) - w * Point::ZERO).normalize();
        let carried = self.spout.map_or(Vec2::ZERO, |s| (mouth - s) / dt / self.unit);
        self.spout = Some(mouth);
        let columns = ((width / self.unit).floor() as usize).max(1);
        let rate = self.fluid.flow / columns as f64;
        self.owed += rate * dt;
        while self.owed >= 1.0 && self.p.len() + columns <= MAX {
            self.owed -= 1.0;
            for c in 0..columns {
                let off = (c as f64 + 0.5 - columns as f64 / 2.0) * self.unit;
                let jitter = (random(((self.node as u64) << 40) ^ self.poured ^ salt(pose.seed)) - 0.5) * 0.2 * self.unit;
                self.p.push(((mouth + across * (off + jitter)).to_vec2() / self.unit).to_point());
                self.v.push(down * rate + carried);
                self.poured += 1;
            }
        }
    }

    fn step(&mut self, pose: &Document, basins: &[Basin], dt: f64, bottom: f64) {
        let f = self.fluid;
        let node = &pose.nodes[self.node];
        let g = pose.world.pull(node, f.gravity) / self.unit;
        let unit = self.unit;
        let me = self.node;
        let old = self.p.clone();
        let cap = 400.0;
        for (q, v) in self.p.iter_mut().zip(&mut self.v) {
            *v += (g + pose.push(me, (q.to_vec2() * unit).to_point()) / unit) * dt;
            if v.hypot() > cap {
                *v = v.normalize() * cap;
            }
            *q += *v * dt;
        }
        let near = neighbours(&self.p);
        let n = self.p.len();
        let corr_w = poly6((0.2 * H).powi(2));
        let mut touch: Vec<Option<Vec2>> = vec![None; n];
        for _ in 0..ITERATIONS {
            let lambda: Vec<f64> = (0..n)
                .map(|i| {
                    let mut rho = poly6(0.0);
                    let (mut grad_i, mut sum2) = (Vec2::ZERO, 0.0);
                    for &j in &near[i] {
                        let d = self.p[i] - self.p[j];
                        rho += poly6(d.hypot2());
                        let gj = spiky(d) / self.rho0;
                        grad_i += gj;
                        sum2 += gj.hypot2();
                    }
                    sum2 += grad_i.hypot2();
                    // Only too dense is pushed apart: a fluid is free to spread out.
                    let c = (rho / self.rho0 - 1.0).max(0.0);
                    -c / (sum2 + EPS)
                })
                .collect();
            let moves: Vec<Vec2> = (0..n)
                .map(|i| {
                    let mut dp = Vec2::ZERO;
                    for &j in &near[i] {
                        let d = self.p[i] - self.p[j];
                        // A little extra push between very close drops keeps them from clumping.
                        let s = -0.05 * (poly6(d.hypot2()) / corr_w).powi(4);
                        dp += spiky(d) * (lambda[i] + lambda[j] + s);
                    }
                    dp / self.rho0
                })
                .collect();
            for (q, m) in self.p.iter_mut().zip(&moves) {
                *q += *m;
            }
            for b in basins {
                for (q, t) in self.p.iter_mut().zip(&mut touch) {
                    if q.x < b.bounds.x0 - b.margin || q.x > b.bounds.x1 + b.margin || q.y < b.bounds.y0 - b.margin || q.y > b.bounds.y1 + b.margin {
                        continue;
                    }
                    let into = inside(&b.poly, *q);
                    let (on, along) = nearest_edge(&b.poly, *q);
                    let gap = (*q - on).hypot();
                    if !into && gap >= b.margin {
                        continue;
                    }
                    let out = if gap > 1e-9 { (if into { on - *q } else { *q - on }) / gap } else { Vec2::new(along.y, -along.x) };
                    *q = on + out * b.margin;
                    *t = Some(along);
                }
            }
        }
        // Thick fluids also stick to what they touch.
        let keep = (1.0 - f.visc.clamp(0.0, 1.0)).powf(dt * SIM_FPS);
        for i in 0..n {
            self.v[i] = (self.p[i] - old[i]) / dt;
            if let Some(along) = touch[i] {
                let slide = along * self.v[i].dot(along);
                self.v[i] += slide * (keep - 1.0);
            }
        }
        // Thickness: drops take on the speed of their neighbours.
        let visc = f.visc.clamp(0.0, 1.0);
        if visc > 0.0 {
            let share = 1.0 - (1.0 - visc).powf(dt * SIM_FPS * 2.0);
            let v0 = self.v.clone();
            for i in 0..n {
                let (mut sum, mut wsum) = (Vec2::ZERO, 0.0);
                for &j in &near[i] {
                    let w = poly6((self.p[i] - self.p[j]).hypot2());
                    sum += (v0[j] - v0[i]) * w;
                    wsum += w;
                }
                if wsum > 0.0 {
                    self.v[i] += sum / wsum * share;
                }
            }
        }
        // Drops that fell far below the artboard are gone.
        let mut k = 0;
        while k < self.p.len() {
            if self.p[k].y > bottom {
                self.p.swap_remove(k);
                self.v.swap_remove(k);
            } else {
                k += 1;
            }
        }
    }

    /// The water's outline on the artboard: a smooth curve around the drops.
    fn outline(&self) -> BezPath {
        let mut out = BezPath::new();
        if self.p.is_empty() {
            return out;
        }
        // The blobs' sum on a grid of half-spacing cells around the drops.
        let cell = 0.5;
        let reach = BLOB;
        let b = self.p.iter().fold(Rect::from_points(self.p[0], self.p[0]), |r, q| r.union_pt(*q)).inflate(reach + cell * 2.0, reach + cell * 2.0);
        let (cols, rows) = ((b.width() / cell).ceil() as usize + 1, (b.height() / cell).ceil() as usize + 1);
        if cols * rows > 4_000_000 {
            return out;
        }
        let mut field = vec![0.0f64; cols * rows];
        let r2 = reach * reach;
        let span = (reach / cell).ceil() as i64;
        for q in &self.p {
            let (ci, cj) = (((q.x - b.x0) / cell).round() as i64, ((q.y - b.y0) / cell).round() as i64);
            for j in (cj - span).max(0)..=(cj + span).min(rows as i64 - 1) {
                for i in (ci - span).max(0)..=(ci + span).min(cols as i64 - 1) {
                    let at = Point::new(b.x0 + i as f64 * cell, b.y0 + j as f64 * cell);
                    let d2 = (at - *q).hypot2();
                    if d2 < r2 {
                        let t = 1.0 - d2 / r2;
                        field[j as usize * cols + i as usize] += t * t;
                    }
                }
            }
        }
        let corner = |i: usize, j: usize| Point::new(b.x0 + i as f64 * cell, b.y0 + j as f64 * cell);
        let value = |i: usize, j: usize| field[j * cols + i];
        // Edges are named by their first corner and direction (0 across, 1 down); each crossing
        // of the level is a point on one edge.
        let cross = |e: (usize, usize, u8)| -> Point {
            let (i, j, dir) = e;
            let (i2, j2) = if dir == 0 { (i + 1, j) } else { (i, j + 1) };
            let (a, c) = (value(i, j), value(i2, j2));
            let t = ((LEVEL - a) / (c - a)).clamp(0.0, 1.0);
            corner(i, j).lerp(corner(i2, j2), t)
        };
        let mut next: HashMap<(usize, usize, u8), (usize, usize, u8)> = HashMap::new();
        for j in 0..rows - 1 {
            for i in 0..cols - 1 {
                // Corners round the cell and the edges between them.
                let cs = [(i, j), (i + 1, j), (i + 1, j + 1), (i, j + 1)];
                let es = [(i, j, 0u8), (i + 1, j, 1u8), (i, j + 1, 0u8), (i, j, 1u8)];
                let ins: Vec<bool> = cs.iter().map(|&(a, c)| value(a, c) > LEVEL).collect();
                let crossed: Vec<usize> = (0..4).filter(|&k| ins[k] != ins[(k + 1) % 4]).collect();
                let pairs: Vec<(usize, usize)> = match crossed.len() {
                    2 => vec![(crossed[0], crossed[1])],
                    4 => {
                        let mid = cs.iter().map(|&(a, c)| value(a, c)).sum::<f64>() / 4.0 > LEVEL;
                        // Join the edges round the corners that are cut off.
                        if ins[0] == mid { vec![(0, 1), (2, 3)] } else { vec![(3, 0), (1, 2)] }
                    }
                    _ => Vec::new(),
                };
                for (ka, kb) in pairs {
                    let (ea, eb) = (es[ka], es[kb]);
                    // Keep the inside on the same side of every piece, so pieces chain into loops.
                    let pa = cross(ea);
                    let pb = cross(eb);
                    let inner = if ins[ka] { cs[ka] } else { cs[(ka + 1) % 4] };
                    let c = corner(inner.0, inner.1);
                    let (from, to) = if (pb - pa).cross(c - pa) > 0.0 { (ea, eb) } else { (eb, ea) };
                    next.insert(from, to);
                }
            }
        }
        while let Some(&start) = next.keys().next() {
            let mut ring = Vec::new();
            let mut e = start;
            while let Some(to) = next.remove(&e) {
                ring.push(cross(e));
                e = to;
            }
            // Every other point is plenty for a smooth curve.
            let ring: Vec<Point> = if ring.len() > 12 { ring.iter().step_by(2).copied().collect() } else { ring };
            if ring.len() >= 3 {
                out.extend(smooth(&ring.iter().map(|q| (q.to_vec2() * self.unit).to_point()).collect::<Vec<_>>()));
            }
        }
        out
    }
}

/// Fluids of `pose`, filled as they are drawn.
pub(crate) fn pools(pose: &Document) -> Vec<Pool> {
    (0..pose.nodes.len()).filter_map(|i| Pool::new(pose, i, pose.nodes[i].fluid?)).collect()
}

pub(crate) fn step_pools(pools: &mut [Pool], pose: &Document, dt: f64) {
    if pools.is_empty() {
        return;
    }
    let walls = solids(pose);
    for pool in pools.iter_mut() {
        if pool.node >= pose.nodes.len() {
            continue;
        }
        let basins = basins(&walls.iter().filter(|w| w.node != pool.node).cloned().collect::<Vec<_>>(), pool.unit);
        let bottom = (pose.artboard.height + 400.0) / pool.unit;
        let h = dt / STEPS as f64;
        for _ in 0..STEPS {
            if pool.fluid.flow > 0.0 {
                pool.pour(pose, h);
            }
            pool.step(pose, &basins, h, bottom);
        }
    }
}

/// Draws each fluid of `doc` as the outline round its drops.
pub(crate) fn apply_pools(pools: &[Pool], doc: &mut Document) {
    for pool in pools {
        if pool.node >= doc.nodes.len() {
            continue;
        }
        let to_local = doc.world_transform(pool.node).inverse();
        doc.nodes[pool.node].deformed = Some(to_local * pool.outline());
    }
}

impl crate::sim::Carry for Pool {
    fn node(&self) -> usize {
        self.node
    }
    fn carry(&mut self, old: &Self) {
        (self.p, self.v, self.owed, self.poured, self.spout) = (old.p.clone(), old.v.clone(), old.owed, old.poured, old.spout);
    }
}

#[cfg(test)]
mod tests {
    use crate::{Animation, Artboard, Document, Fluid, Node, ShapeKind, SimCache};
    use kurbo::Shape;

    fn world(d: &Document, id: &str) -> kurbo::Rect {
        let i = d.index_of(id).unwrap();
        (d.world_transform(i) * d.nodes[i].path().unwrap()).bounding_box()
    }

    fn at(d: &Document, t: f64) -> Document {
        Animation { name: "Go".into(), duration: 10.0, looping: false, tracks: Vec::new(), events: Vec::new() }.pose_with(d, t, &mut SimCache::default())
    }

    /// A box 200 wide (x 100..300) with a floor at y 300, open at the top, made of three solids.
    fn tank(d: &mut Document) {
        for (id, x, y, w, h) in [("floor", 200.0, 310.0, 240.0, 20.0), ("left", 90.0, 200.0, 20.0, 240.0), ("right", 310.0, 200.0, 20.0, 240.0)] {
            let mut n = Node::new(id, ShapeKind::Rect);
            (n.x, n.y, n.width, n.height) = (x, y, w, h);
            n.solid = true;
            d.nodes.push(n);
        }
    }

    #[test]
    fn a_column_of_water_collapses_and_levels_out() {
        let mut d = Document::new(Artboard::default());
        let mut w = Node::new("water", ShapeKind::Rect);
        (w.x, w.y, w.width, w.height) = (140.0, 200.0, 60.0, 120.0);
        w.fluid = Some(Fluid::default());
        d.nodes.push(w);
        tank(&mut d);
        let start = world(&at(&d, 0.0), "water");
        assert!((start.width() - 60.0).abs() < 12.0 && (start.height() - 120.0).abs() < 12.0, "drawn as filled: {start:?}");
        let end = world(&at(&d, 4.0), "water");
        // It spreads over the whole floor, about 7200 / 200 = 36 deep, and stays in the tank.
        assert!(end.width() > 180.0, "{end:?}");
        assert!(end.max_y() < 303.0 && end.min_x() > 97.0 && end.max_x() < 303.0, "{end:?}");
        assert!(end.height() > 22.0 && end.height() < 60.0, "{end:?}");
        // In design it is the shape as drawn.
        let mut plain = d.clone();
        plain.rig();
        assert_eq!(plain.nodes[0].deformed, None);
    }

    #[test]
    fn a_spout_pours_water_that_gathers_below() {
        let mut d = Document::new(Artboard::default());
        let mut s = Node::new("tap", ShapeKind::Rect);
        (s.x, s.y, s.width, s.height) = (200.0, 60.0, 12.0, 12.0);
        s.fluid = Some(Fluid { flow: 120.0, ..Fluid::default() });
        d.nodes.push(s);
        tank(&mut d);
        let early = at(&d, 0.0);
        assert!(early.nodes[0].path().unwrap().is_empty(), "nothing poured yet");
        let end = world(&at(&d, 3.0), "tap");
        assert!(end.min_y() < 80.0 && end.max_y() > 280.0 && end.max_y() < 303.0, "a stream down to the floor: {end:?}");
    }
}
