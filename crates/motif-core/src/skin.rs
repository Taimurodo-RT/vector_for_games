//! Skinning: bones bend the outline of a shape. The outline is cut into short curves, and every
//! point moves with a mix of the bones near it, weighted by distance. The shape stays a vector:
//! it is bent, not turned into a mesh.

use crate::{Document, ShapeKind, SkinBind};
use kurbo::{Affine, BezPath, CubicBez, Line, ParamCurve, ParamCurveArclen, ParamCurveNearest, PathEl, Point, QuadBez, Vec2};

/// Longest piece of outline that is bent as one curve, in the shape's units.
const PIECE: f64 = 6.0;
/// Most pieces one segment of the outline is cut into.
const MAX_PIECES: usize = 64;

impl Document {
    /// Bends every `ik` chain to its target, then bends skinned shapes with their bones.
    /// Animations call it after posing, the editor before drawing.
    pub fn rig(&mut self) {
        self.solve_ik();
        self.skin_all();
    }

    /// Bends skinned shapes with their bones as they are posed now.
    pub fn skin_all(&mut self) {
        for i in 0..self.nodes.len() {
            // Soft bodies get their outline from the simulation.
            if self.nodes[i].soft.is_none() {
                self.nodes[i].deformed = self.deform(i);
            }
        }
    }

    /// Remembers the current pose as the one skinned shapes were drawn in. Run on the setup
    /// pose, before `rig`.
    pub fn bind_skins(&mut self) {
        for i in 0..self.nodes.len() {
            let Some(skin) = &self.nodes[i].skin else { continue };
            let shape = self.world_transform(i);
            let bind = skin
                .bones
                .iter()
                .filter_map(|id| {
                    let b = self.index_of(id).filter(|&b| self.nodes[b].kind == ShapeKind::Bone)?;
                    let to_bone = self.world_transform(b).inverse() * shape;
                    let back = to_bone.inverse();
                    Some(SkinBind { bone: id.clone(), to_bone, a: back * Point::ZERO, b: back * self.nodes[b].bone_tip() })
                })
                .collect();
            let skin = self.nodes[i].skin.as_mut().unwrap();
            skin.bind = bind;
            skin.bind_world = shape;
        }
    }

    fn deform(&self, i: usize) -> Option<BezPath> {
        let node = &self.nodes[i];
        let skin = node.skin.as_ref().filter(|s| !s.bind.is_empty())?;
        let base = node.base_path()?;
        let inv = self.world_transform(i).inverse();
        // How each bone moved since the bind pose, on the artboard.
        let moves: Vec<Affine> = skin
            .bind
            .iter()
            .map(|sb| self.index_of(&sb.bone).map_or(Affine::IDENTITY, |b| self.world_transform(b) * sb.to_bone * skin.bind_world.inverse()))
            .collect();
        // Bones that only turn and move are blended as rotations, which keeps an elbow as thick
        // as the arm; a stretched or skewed bone falls back to mixing the moved points.
        let rigid: Option<Vec<Dual>> = moves.iter().map(|m| Dual::from_affine(*m)).collect();
        let sharp = skin.sharp.max(0.5);
        let weights = |p: Point| skin.bind.iter().map(move |sb| (Line::new(sb.a, sb.b).nearest(p, 1e-6).distance_sq.sqrt() + 0.5).powf(-sharp));
        let bend = |p: Point| {
            let world = skin.bind_world * p;
            let moved = match &rigid {
                Some(duals) => {
                    let w: Vec<f64> = weights(p).collect();
                    let lead = (0..w.len()).max_by(|&a, &b| w[a].total_cmp(&w[b])).unwrap_or(0);
                    let mut sum = Dual::ZERO;
                    for (d, wk) in duals.iter().zip(&w) {
                        // Take each rotation the short way round, as the strongest bone does.
                        let s = if d.r.dot(duals[lead].r) < 0.0 { -wk } else { *wk };
                        sum.r += d.r * s;
                        sum.d += d.d * s;
                    }
                    sum.apply(world)
                }
                None => {
                    let mut sum = Vec2::ZERO;
                    let mut total = 0.0;
                    for (m, w) in moves.iter().zip(weights(p)) {
                        sum += (*m * world).to_vec2() * w;
                        total += w;
                    }
                    (sum / total).to_point()
                }
            };
            inv * moved
        };
        Some(bent(&base, bend))
    }
}

/// A turn and a move of the plane as a pair of complex numbers: `r` is half the turn, `d` carries
/// the move. Unlike matrices, a weighted sum of these is again a turn and a move, so blending
/// bones never shrinks the shape (the 2D form of dual-quaternion skinning).
#[derive(Clone, Copy)]
struct Dual {
    r: Vec2,
    d: Vec2,
}

/// Complex product.
fn mul(a: Vec2, b: Vec2) -> Vec2 {
    Vec2::new(a.x * b.x - a.y * b.y, a.x * b.y + a.y * b.x)
}

impl Dual {
    const ZERO: Dual = Dual { r: Vec2::ZERO, d: Vec2::ZERO };

    /// `None` unless `m` only turns and moves.
    fn from_affine(m: Affine) -> Option<Dual> {
        let [a, b, c, d, e, f] = m.as_coeffs();
        if (a - d).abs() > 1e-6 || (b + c).abs() > 1e-6 || (a * a + b * b - 1.0).abs() > 1e-6 {
            return None;
        }
        let half = b.atan2(a) / 2.0;
        let r = Vec2::new(half.cos(), half.sin());
        // d = t · conj(r) / 2
        Some(Dual { r, d: mul(Vec2::new(e, f), Vec2::new(r.x, -r.y)) / 2.0 })
    }

    fn apply(self, p: Point) -> Point {
        let n = self.r.hypot();
        if n < 1e-12 {
            return p;
        }
        let (r, d) = (self.r / n, self.d / n);
        let t = mul(d, r) * 2.0;
        (mul(mul(r, r), p.to_vec2()) + t).to_point()
    }
}

/// `path` cut into short cubic pieces, each point passed through `f`.
fn bent(path: &BezPath, f: impl Fn(Point) -> Point) -> BezPath {
    let mut out = BezPath::new();
    let mut start = Point::ZERO;
    let mut last = Point::ZERO;
    let push = |out: &mut BezPath, c: CubicBez| {
        let n = ((c.arclen(0.1) / PIECE).ceil() as usize).clamp(1, MAX_PIECES);
        for k in 0..n {
            let s = c.subsegment(k as f64 / n as f64..(k + 1) as f64 / n as f64);
            out.curve_to(f(s.p1), f(s.p2), f(s.p3));
        }
    };
    for el in path.elements() {
        match *el {
            PathEl::MoveTo(p) => {
                out.move_to(f(p));
                (start, last) = (p, p);
            }
            PathEl::LineTo(p) => {
                push(&mut out, line_cubic(last, p));
                last = p;
            }
            PathEl::QuadTo(c, p) => {
                push(&mut out, QuadBez::new(last, c, p).raise());
                last = p;
            }
            PathEl::CurveTo(c1, c2, p) => {
                push(&mut out, CubicBez::new(last, c1, c2, p));
                last = p;
            }
            PathEl::ClosePath => {
                // The closing edge bends too, so it is drawn as a curve before closing.
                if (last - start).hypot() > 1e-9 {
                    push(&mut out, line_cubic(last, start));
                }
                out.close_path();
                last = start;
            }
        }
    }
    out
}

fn line_cubic(a: Point, b: Point) -> CubicBez {
    CubicBez::new(a, a.lerp(b, 1.0 / 3.0), a.lerp(b, 2.0 / 3.0), b)
}

#[cfg(test)]
mod tests {
    use crate::{Artboard, Document, Node, ShapeKind, Skin};
    use kurbo::{Point, Shape};

    /// A 100×20 bar from x 0 to 100 over two bones of 50, the second at `bend` degrees.
    fn arm(bend: f64) -> Document {
        let mut d = Document::new(Artboard::default());
        let mut a = Node::new("a", ShapeKind::Bone);
        a.width = 50.0;
        let mut b = Node::new("b", ShapeKind::Bone);
        (b.width, b.x, b.parent) = (50.0, 50.0, Some(0));
        let mut bar = Node::new("bar", ShapeKind::Rect);
        (bar.width, bar.height, bar.x) = (100.0, 20.0, 50.0);
        bar.skin = Some(Skin::new(vec!["a".into(), "b".into()]));
        d.nodes = vec![a, b, bar];
        d.bind_skins();
        d.nodes[1].rotation = bend;
        d.rig();
        d
    }

    #[test]
    fn bind_pose_keeps_the_outline() {
        let d = arm(0.0);
        let bb = d.nodes[2].path().unwrap().bounding_box();
        assert!((bb.width() - 100.0).abs() < 1e-6 && (bb.height() - 20.0).abs() < 1e-6, "{bb:?}");
        assert!(d.nodes[2].path().unwrap().elements().len() > 30, "cut into short pieces");
    }

    #[test]
    fn bending_a_bone_bends_the_far_end_only() {
        let d = arm(90.0);
        let p = d.nodes[2].path().unwrap();
        let world = d.world_transform(2) * p;
        let bb = world.bounding_box();
        // The far end now hangs down from the elbow at x 50; the near end stays put.
        assert!(bb.max_y() > 45.0, "{bb:?}");
        assert!(bb.min_x() > -1.0 && bb.max_x() < 64.0, "{bb:?}");
        assert!(world.contains(Point::new(10.0, 0.0)));
        assert!(world.contains(Point::new(50.0, 40.0)));
        // The rig is repeatable: posing again from the same state gives the same outline.
        let mut again = d.clone();
        again.rig();
        assert_eq!(again.nodes[2].path(), d.nodes[2].path());
    }
}
