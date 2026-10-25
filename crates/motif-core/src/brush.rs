//! Procedural brushes: a hand-drawn wobble of the outline, hatching, stippled grain and banded
//! light. Everything stays vector and comes from a seed, so the same scene draws the same way.

use crate::{Color, Gradient};
use kurbo::{BezPath, PathEl, Point, Rect, Shape, Vec2};

/// How a shape is drawn by hand. All zero is a clean shape.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Brush {
    /// How far the outline wanders from the true shape, in artboard units.
    pub rough: f64,
    /// Length of one wobble along the outline; 0 takes `Brush::WAVE`.
    pub wave: f64,
    /// How many times a second the wobble is drawn anew (line boil); 0 keeps it still.
    pub boil: f64,
    /// Seed of every random choice; without one the object's id is used.
    pub seed: Option<u32>,
    pub hatch: Option<Hatch>,
    pub grain: Option<Grain>,
    /// Gradients and the light on 3D bodies fall into this many flat steps; 0 keeps them smooth.
    pub bands: u32,
}

/// Parallel strokes over the shape, clipped to it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Hatch {
    /// Direction of the strokes in degrees: 0 is horizontal.
    pub angle: f64,
    /// Distance between neighbouring strokes.
    pub gap: f64,
    pub width: f64,
    /// Without a colour the strokes take the outline's, or a darker fill.
    pub color: Option<Color>,
    /// A second set of strokes across the first.
    pub cross: bool,
}

/// Dots scattered over the shape, clipped to it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Grain {
    /// Share of the area the dots cover, 0..1.
    pub density: f64,
    /// Diameter of a dot.
    pub size: f64,
    pub color: Option<Color>,
}

impl Brush {
    pub const WAVE: f64 = 24.0;

    pub fn is_plain(&self) -> bool {
        self.rough <= 0.0 && self.hatch.is_none() && self.grain.is_none() && self.bands == 0
    }

    /// The seed in effect at `time`: boiling lines take a new one every 1/`boil` seconds.
    pub fn seed_at(&self, id: &str, time: f64) -> u32 {
        let base = self.seed.unwrap_or_else(|| fnv(id));
        let tick = if self.boil > 0.0 { (time * self.boil).floor() as i64 } else { 0 };
        base.wrapping_add((tick as u32).wrapping_mul(0x9e37_79b9))
    }

    fn wave(&self) -> f64 {
        if self.wave > 0.0 { self.wave } else { Self::WAVE }
    }

    /// The outline as drawn by hand: every subpath wanders along its normal by up to `rough`.
    pub fn roughen(&self, path: &BezPath, seed: u32) -> BezPath {
        if self.rough <= 0.0 {
            return path.clone();
        }
        wobble(path, self.rough, self.wave(), seed)
    }

    /// Hatching strokes in the shape's local space, over `bounds`; `rough` makes them wander too.
    pub fn hatch_lines(&self, bounds: Rect, seed: u32) -> Option<BezPath> {
        let h = self.hatch?;
        let gap = h.gap.max(0.5);
        let mut out = BezPath::new();
        let angles: &[f64] = if h.cross { &[0.0, 90.0] } else { &[0.0] };
        for (set, turn) in angles.iter().enumerate() {
            let a = (h.angle + turn).to_radians();
            let d = Vec2::new(a.cos(), a.sin());
            let n = Vec2::new(-d.y, d.x);
            let c = bounds.center().to_vec2();
            let corners = [bounds.origin(), Point::new(bounds.x1, bounds.y0), Point::new(bounds.x0, bounds.y1), Point::new(bounds.x1, bounds.y1)];
            let span = |v: Vec2| corners.iter().map(|p| (p.to_vec2() - c).dot(v)).fold((f64::MAX, f64::MIN), |(lo, hi), x| (lo.min(x), hi.max(x)));
            let (n0, n1) = span(n);
            let (d0, d1) = span(d);
            let count = ((n1 - n0) / gap).ceil() as i64 + 1;
            for k in 0..count.min(4000) {
                let r = |salt: u32| unit(hash(seed, k as u32 * 4 + salt + set as u32 * 7919)) - 0.5;
                // Each stroke sits a little off its place and reaches a little short or long.
                let off = n0 + k as f64 * gap + r(0) * gap * 0.3;
                let a = Point::ZERO + c + n * off + d * (d0 - gap + r(1) * gap);
                let b = Point::ZERO + c + n * off + d * (d1 + gap + r(2) * gap);
                let mut line = BezPath::new();
                line.move_to(a);
                line.line_to(b);
                let line = if self.rough > 0.0 { wobble(&line, self.rough * 0.6, self.wave(), seed ^ (k as u32).wrapping_mul(2_654_435_761)) } else { line };
                out.extend(line);
            }
        }
        Some(out)
    }

    /// Grain dots in the shape's local space over `bounds`, one per cell of a jittered grid.
    pub fn grain_dots(&self, bounds: Rect, seed: u32) -> Option<BezPath> {
        let g = self.grain?;
        let density = g.density.clamp(0.001, 1.0);
        let size = g.size.max(0.2);
        let cell = (std::f64::consts::PI * size * size / 4.0 / density).sqrt();
        let (cols, rows) = ((bounds.width() / cell).ceil() as i64 + 1, (bounds.height() / cell).ceil() as i64 + 1);
        // Too many dots would only slow drawing down: past the limit the cells grow.
        let scale = ((cols * rows) as f64 / 40_000.0).sqrt().max(1.0);
        let cell = cell * scale;
        let (cols, rows) = ((bounds.width() / cell).ceil() as i64 + 1, (bounds.height() / cell).ceil() as i64 + 1);
        let mut out = BezPath::new();
        for y in 0..rows {
            for x in 0..cols {
                let k = (y * cols + x) as u32;
                let r = |salt: u32| unit(hash(seed, k.wrapping_mul(3).wrapping_add(salt)));
                let at = Point::new(bounds.x0 + (x as f64 + r(0)) * cell, bounds.y0 + (y as f64 + r(1)) * cell);
                let radius = size * scale / 2.0 * (0.7 + 0.6 * r(2));
                out.extend(kurbo::Circle::new(at, radius).path_elements(0.1));
            }
        }
        Some(out)
    }

    /// A gradient whose colours fall into `bands` flat steps; `None` when it stays smooth.
    pub fn band_gradient(&self, g: &Gradient) -> Option<Gradient> {
        if self.bands == 0 || g.colors.len() < 2 {
            return None;
        }
        let n = self.bands.max(1) as usize;
        let steps = (0..n).map(|k| sample(&g.colors, if n == 1 { 0.5 } else { k as f64 / (n - 1) as f64 }));
        Some(Gradient { colors: hard(steps.collect()), ..g.clone() })
    }
}

/// Colours spread evenly, each repeated so that a renderer's smooth blend between them becomes
/// a step a fraction of a percent wide.
pub fn hard(colors: Vec<Color>) -> Vec<Color> {
    const REPEAT: usize = 24;
    colors.into_iter().flat_map(|c| std::iter::repeat_n(c, REPEAT)).collect()
}

/// The light `d` (0..1) snapped to one of `bands` levels.
pub fn band_light(d: f64, bands: u32) -> f64 {
    match bands {
        0 => d,
        1 => 0.5,
        n => ((d * n as f64).floor().min(n as f64 - 1.0) / (n - 1) as f64).clamp(0.0, 1.0),
    }
}

fn sample(colors: &[Color], t: f64) -> Color {
    let x = t.clamp(0.0, 1.0) * (colors.len() - 1) as f64;
    let i = (x.floor() as usize).min(colors.len() - 2);
    colors[i].lerp(colors[i + 1], x - i as f64)
}

fn fnv(s: &str) -> u32 {
    s.bytes().fold(0x811c_9dc5, |h, b| (h ^ b as u32).wrapping_mul(0x0100_0193))
}

fn hash(seed: u32, k: u32) -> u32 {
    let mut x = seed ^ k.wrapping_mul(0x27d4_eb2d);
    x = (x ^ (x >> 15)).wrapping_mul(0x85eb_ca6b);
    x = (x ^ (x >> 13)).wrapping_mul(0xc2b2_ae35);
    x ^ (x >> 16)
}

fn unit(h: u32) -> f64 {
    h as f64 / u32::MAX as f64
}

/// Smooth noise in -1..1 along a line of `cells` lattice cells; closed lines wrap around.
fn noise(x: f64, cells: usize, wrap: bool, seed: u32) -> f64 {
    let i = x.floor();
    let f = x - i;
    let at = |k: i64| {
        let k = if wrap { k.rem_euclid(cells as i64) } else { k };
        unit(hash(seed, k as u32)) * 2.0 - 1.0
    };
    let s = f * f * (3.0 - 2.0 * f);
    at(i as i64) * (1.0 - s) + at(i as i64 + 1) * s
}

/// Points of each subpath, evenly spaced along it, and whether it closes.
fn resample(path: &BezPath, step: f64) -> Vec<(Vec<Point>, bool)> {
    let mut raw: Vec<(Vec<Point>, bool)> = Vec::new();
    kurbo::flatten(path, 0.05, |el| match el {
        PathEl::MoveTo(p) => raw.push((vec![p], false)),
        PathEl::LineTo(p) => {
            if let Some((pts, _)) = raw.last_mut() {
                pts.push(p);
            }
        }
        PathEl::ClosePath => {
            if let Some((_, c)) = raw.last_mut() {
                *c = true;
            }
        }
        _ => {}
    });
    raw.into_iter()
        .filter(|(pts, _)| pts.len() >= 2)
        .map(|(mut pts, closed)| {
            // A piece that comes back to its start is closed even without `Z`.
            let closed = closed || pts.first().unwrap().distance(*pts.last().unwrap()) < 1e-6;
            if closed && pts.first().unwrap().distance(*pts.last().unwrap()) > 1e-9 {
                pts.push(pts[0]);
            }
            let len: f64 = pts.windows(2).map(|w| w[0].distance(w[1])).sum();
            let n = ((len / step).ceil() as usize).clamp(if closed { 8 } else { 2 }, 6000);
            let mut out = Vec::with_capacity(n + 1);
            let (mut seg, mut walked) = (0, 0.0);
            for k in 0..=n {
                let target = len * k as f64 / n as f64;
                while seg < pts.len() - 2 && walked + pts[seg].distance(pts[seg + 1]) < target {
                    walked += pts[seg].distance(pts[seg + 1]);
                    seg += 1;
                }
                let l = pts[seg].distance(pts[seg + 1]).max(1e-12);
                out.push(pts[seg].lerp(pts[seg + 1], ((target - walked) / l).clamp(0.0, 1.0)));
            }
            if closed {
                out.pop();
            }
            (out, closed)
        })
        .collect()
}

fn wobble(path: &BezPath, amount: f64, wave: f64, seed: u32) -> BezPath {
    let mut out = BezPath::new();
    for (sub, (pts, closed)) in resample(path, (wave / 6.0).clamp(0.5, 4.0)).into_iter().enumerate() {
        let n = pts.len();
        let len: f64 = (0..n - usize::from(!closed)).map(|i| pts[i].distance(pts[(i + 1) % n])).sum();
        // Two octaves: the long sway of a hand, and a finer tremor.
        let octaves = [(1.0, 1.0), (0.35, 2.7)];
        let seed = seed ^ (sub as u32).wrapping_mul(0x9e37_79b9);
        let moved: Vec<Point> = (0..n)
            .map(|i| {
                let prev = if closed { pts[(i + n - 1) % n] } else { pts[i.saturating_sub(1)] };
                let next = if closed { pts[(i + 1) % n] } else { pts[(i + 1).min(n - 1)] };
                let t = (next - prev).normalize();
                let normal = if t.is_finite() { Vec2::new(-t.y, t.x) } else { Vec2::ZERO };
                let s = i as f64 / if closed { n as f64 } else { (n - 1).max(1) as f64 };
                let off: f64 = octaves
                    .iter()
                    .enumerate()
                    .map(|(o, (amp, freq))| {
                        let cells = ((len * freq / wave).round() as usize).max(if closed { 3 } else { 1 });
                        amp * noise(s * cells as f64, cells, closed, seed.wrapping_add(o as u32 * 101))
                    })
                    .sum();
                pts[i] + normal * off * amount / 1.35
            })
            .collect();
        smooth(&mut out, &moved, closed);
    }
    out
}

/// A Catmull-Rom curve through the points.
fn smooth(out: &mut BezPath, pts: &[Point], closed: bool) {
    let n = pts.len();
    let at = |i: i64| if closed { pts[i.rem_euclid(n as i64) as usize] } else { pts[i.clamp(0, n as i64 - 1) as usize] };
    out.move_to(pts[0]);
    let segs = if closed { n } else { n - 1 };
    for i in 0..segs as i64 {
        let (p0, p1, p2, p3) = (at(i - 1), at(i), at(i + 1), at(i + 2));
        out.curve_to(p1 + (p2 - p0) / 6.0, p2 - (p3 - p1) / 6.0, p2);
    }
    if closed {
        out.close_path();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kurbo::Shape;

    fn square() -> BezPath {
        Rect::new(-50.0, -50.0, 50.0, 50.0).to_path(0.1)
    }

    #[test]
    fn rough_outlines_stay_near_the_shape_and_repeat_with_the_seed() {
        let b = Brush { rough: 4.0, ..Default::default() };
        let a = b.roughen(&square(), 7);
        assert_eq!(a, b.roughen(&square(), 7));
        assert_ne!(a, b.roughen(&square(), 8));
        let bb = a.bounding_box();
        assert!(bb.x0 > -55.0 && bb.x1 < 55.0 && bb.y0 > -55.0 && bb.y1 < 55.0, "{bb:?}");
        assert!(bb.x0 < -49.0 || bb.x1 > 49.0);
        // Still one closed outline with about the same area.
        assert_eq!(a.elements().iter().filter(|e| matches!(e, PathEl::ClosePath)).count(), 1);
        assert!((a.area().abs() - 10_000.0).abs() < 900.0, "{}", a.area());
    }

    #[test]
    fn boil_changes_the_seed_a_set_number_of_times_a_second() {
        let b = Brush { rough: 2.0, boil: 4.0, ..Default::default() };
        assert_eq!(b.seed_at("rock", 0.1), b.seed_at("rock", 0.2));
        assert_ne!(b.seed_at("rock", 0.1), b.seed_at("rock", 0.3));
        let still = Brush { rough: 2.0, ..Default::default() };
        assert_eq!(still.seed_at("rock", 0.1), still.seed_at("rock", 5.0));
        assert_ne!(still.seed_at("rock", 0.0), still.seed_at("tree", 0.0));
    }

    #[test]
    fn hatching_covers_the_bounds_and_crosses() {
        let hatch = |cross| Brush { hatch: Some(Hatch { angle: 45.0, gap: 10.0, width: 1.0, color: None, cross }), ..Default::default() };
        let one = hatch(false).hatch_lines(square().bounding_box(), 1).unwrap();
        let moves = |p: &BezPath| p.elements().iter().filter(|e| matches!(e, PathEl::MoveTo(_))).count();
        // The diagonal of a 100 square is about 141: some 15 strokes 10 apart.
        assert!((14..=17).contains(&moves(&one)), "{}", moves(&one));
        let two = hatch(true).hatch_lines(square().bounding_box(), 1).unwrap();
        assert_eq!(moves(&two), 2 * moves(&one));
    }

    #[test]
    fn grain_scatters_dots_by_density() {
        let grain = |density| Brush { grain: Some(Grain { density, size: 2.0, color: None }), ..Default::default() };
        let count = |p: BezPath| p.elements().iter().filter(|e| matches!(e, PathEl::MoveTo(_))).count();
        let thin = count(grain(0.05).grain_dots(square().bounding_box(), 3).unwrap());
        let thick = count(grain(0.4).grain_dots(square().bounding_box(), 3).unwrap());
        assert!(thick > thin * 5, "{thin} {thick}");
    }

    #[test]
    fn bands_make_flat_steps() {
        let g = Gradient { radial: false, angle: 0.0, colors: vec![Color::BLACK, Color::WHITE] };
        let b = Brush { bands: 3, ..Default::default() };
        let banded = b.band_gradient(&g).unwrap();
        assert_eq!(banded.colors.len(), 3 * 24);
        assert_eq!(banded.colors[0], Color::BLACK);
        assert_eq!(banded.colors[71], Color::WHITE);
        assert_eq!(banded.colors[30], banded.colors[40]);
        assert_eq!(band_light(0.1, 3), 0.0);
        assert_eq!(band_light(0.5, 3), 0.5);
        assert_eq!(band_light(0.99, 3), 1.0);
    }
}
