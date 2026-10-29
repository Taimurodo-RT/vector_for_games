//! A smooth quadratic 3x3 displacement cage in a group's local space.
use crate::Document;
use kurbo::{Affine, BezPath, CubicBez, Line, ParamCurve, PathEl, PathSeg, Point, QuadBez, Vec2};

#[derive(Clone, Debug, PartialEq)]
pub struct Lattice {
    pub width: f64,
    pub height: f64,
    pub offsets: [Vec2; 9],
    /// Evaluated additive residuals. Source offsets and explicit animation keys stay separate.
    pub(crate) correction: [Vec2; 9],
}

impl Lattice {
    pub fn new(width: f64, height: f64) -> Self {
        Self { width, height, offsets: [Vec2::ZERO; 9], correction: [Vec2::ZERO; 9] }
    }
    pub fn effective_offset(&self, i: usize) -> Vec2 {
        self.offsets[i] + self.correction[i]
    }
    pub fn neutral(&self) -> bool {
        (0..9).all(|i| self.effective_offset(i) == Vec2::ZERO)
    }
    pub fn control(&self, i: usize) -> Point {
        Point::new((i % 3) as f64 * self.width / 2.0 - self.width / 2.0, (i / 3) as f64 * self.height / 2.0 - self.height / 2.0) + self.effective_offset(i)
    }
    pub fn map(&self, p: Point) -> Point {
        let basis = |v: f64| {
            let v = v.clamp(0.0, 1.0);
            [(1.0 - v).powi(2), 2.0 * v * (1.0 - v), v * v]
        };
        let u = basis(p.x / self.width.max(1e-6) + 0.5);
        let v = basis(p.y / self.height.max(1e-6) + 0.5);
        let mut d = Vec2::ZERO;
        for (y, vy) in v.iter().enumerate() {
            for (x, ux) in u.iter().enumerate() {
                d += self.effective_offset(y * 3 + x) * (ux * vy);
            }
        }
        p + d
    }
}

impl Document {
    pub fn map_lattice_point(&self, i: usize, point: Point) -> Point {
        let world = self.world_transform(i);
        if world.determinant().abs() < 1e-10 {
            return point;
        }
        let mut p = world * point;
        let mut parent = self.nodes[i].parent;
        while let Some(k) = parent {
            if let Some(l) = &self.nodes[k].lattice {
                let w = self.world_transform(k);
                if w.determinant().abs() > 1e-10 {
                    p = w * l.map(w.inverse() * p);
                }
            }
            parent = self.nodes[k].parent;
        }
        world.inverse() * p
    }

    /// Inverse for editing handles inside an ancestor cage. Folded cages can have
    /// several solutions; this uses the nearby solution and never changes source data.
    pub fn unmap_lattice_point(&self, i: usize, point: Point) -> Point {
        let mut p = point;
        for _ in 0..16 {
            let q = self.map_lattice_point(i, p);
            let error = q - point;
            if error.hypot() < 1e-5 {
                break;
            }
            let dx = (self.map_lattice_point(i, p + Vec2::new(0.01, 0.0)) - q) / 0.01;
            let dy = (self.map_lattice_point(i, p + Vec2::new(0.0, 0.01)) - q) / 0.01;
            let det = dx.cross(dy);
            if det.abs() < 1e-9 {
                break;
            }
            p -= Vec2::new(error.cross(dy) / det, dx.cross(error) / det);
            if !p.is_finite() {
                return point;
            }
        }
        p
    }

    /// Deepest cage first, then its ancestors. Paint order and masks stay intact.
    /// A separate derived path avoids accumulating deformation on repeated rig calls.
    pub fn deform_lattices(&mut self) {
        for n in &mut self.nodes {
            n.lattice_path = None;
        }
        if !self.nodes.iter().any(|n| n.lattice.as_ref().is_some_and(|l| !l.neutral())) {
            return;
        }
        for i in 0..self.nodes.len() {
            let mut cages = Vec::new();
            let mut parent = self.nodes[i].parent;
            while let Some(p) = parent {
                if let Some(l) = &self.nodes[p].lattice
                    && !l.neutral()
                {
                    let world = self.world_transform(p);
                    if world.determinant().abs() > 1e-10 {
                        cages.push((world, world.inverse(), l.clone()));
                    }
                }
                parent = self.nodes[p].parent;
            }
            if cages.is_empty() {
                continue;
            }
            let world = self.world_transform(i);
            if world.determinant().abs() < 1e-10 {
                continue;
            }
            let Some(path) = self.nodes[i].path() else {
                continue;
            };
            let inverse = world.inverse();
            let map = |p: Point| {
                let mut q = world * p;
                for (w, inv, l) in &cages {
                    q = *w * l.map(*inv * q);
                }
                inverse * q
            };
            self.nodes[i].lattice_path = Some(map_path(&path, map, world));
        }
    }
}

/// Adaptive subdivision measures error in artboard units, including nested/reflected cages.
/// The editable Bézier source is never replaced by this render-only polyline.
fn map_path(path: &BezPath, map: impl Fn(Point) -> Point, world: Affine) -> BezPath {
    fn segment(out: &mut BezPath, s: PathSeg, map: &impl Fn(Point) -> Point, world: Affine, range: (f64, f64), depth: u8) {
        let (a, b) = range;
        let start = map(s.eval(a));
        let end = map(s.eval(b));
        let error = [0.25, 0.5, 0.75]
            .iter()
            .map(|t| {
                let actual = world * map(s.eval(a + (b - a) * t));
                let chord = world * start.lerp(end, *t);
                (actual - chord).hypot()
            })
            .fold(0.0_f64, f64::max);
        if depth >= 12 || error < 0.15 {
            out.line_to(end);
        } else {
            let mid = (a + b) * 0.5;
            segment(out, s, map, world, (a, mid), depth + 1);
            segment(out, s, map, world, (mid, b), depth + 1);
        }
    }
    let mut out = BezPath::new();
    let mut at = Point::ZERO;
    let mut start = Point::ZERO;
    for el in path.elements() {
        let curve = match *el {
            PathEl::MoveTo(p) => {
                at = p;
                start = p;
                out.move_to(map(p));
                continue;
            }
            PathEl::LineTo(p) => PathSeg::Line(Line::new(at, p)),
            PathEl::QuadTo(a, p) => PathSeg::Quad(QuadBez::new(at, a, p)),
            PathEl::CurveTo(a, b, p) => PathSeg::Cubic(CubicBez::new(at, a, b, p)),
            PathEl::ClosePath => {
                if at != start {
                    segment(&mut out, PathSeg::Line(Line::new(at, start)), &map, world, (0.0, 1.0), 0);
                }
                out.close_path();
                at = start;
                continue;
            }
        };
        segment(&mut out, curve, &map, world, (0.0, 1.0), 0);
        at = curve.eval(1.0);
    }
    out
}
