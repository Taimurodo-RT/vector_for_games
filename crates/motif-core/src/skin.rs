//! Skinning: bones bend the outline of a shape. The outline is cut into short curves, and every
//! point moves with a mix of the bones near it, weighted by distance. The shape stays a vector:
//! it is bent, not turned into a mesh.

use crate::{Document, ShapeKind, Skin, SkinBind, WeightStroke};
use kurbo::{Affine, BezPath, CubicBez, Line, ParamCurve, ParamCurveArclen, ParamCurveNearest, PathEl, Point, QuadBez, Shape, Vec2};

/// Longest piece of outline that is bent as one curve, in the shape's units.
const PIECE: f64 = 6.0;
/// Most pieces one segment of the outline is cut into.
const MAX_PIECES: usize = 64;

impl Document {
    /// Places objects on balls, bends every `ik` chain to its target, then bends skinned shapes
    /// with their bones and wrapped shapes over their balls.
    /// Animations call it after posing, the editor before drawing.
    pub fn rig(&mut self) {
        self.layout();
        self.morph_all();
        self.place_orbits();
        self.solve_ik();
        self.skin_all();
        self.hang_ropes();
    }

    /// Bends skinned shapes with their bones as they are posed now.
    pub fn skin_all(&mut self) {
        for i in 0..self.nodes.len() {
            // Soft bodies, ropes, cloth and fluids get their outline from the simulation.
            let n = &self.nodes[i];
            if n.soft.is_none() && n.rope.is_none() && n.fluid.is_none() {
                self.nodes[i].deformed = self.deform(i).or_else(|| self.wrapped(i));
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
        let base = self.nodes[i].base_path()?;
        let bend = self.bender(i)?;
        Some(bent(&base, |p| bend.point(p)))
    }

    /// Where the point `p` of skinned shape `i` (in its own space, as drawn) is moved to by its
    /// bones in the current pose, again in its own space. `None` when the shape has no skin.
    /// The weight brush draws its overlay with it.
    pub fn skin_point(&self, i: usize, p: Point) -> Option<Point> {
        Some(self.bender(i)?.point(p))
    }

    /// The point of skinned shape `i` as drawn that its bones move to `target` (both in its own
    /// space): the inverse of `skin_point`. Newton steps start from `near` (where a brush stroke
    /// was a moment ago), from the spot each bone alone would have come from, and from `target`.
    /// A strong bend can fold the plane, so several points may land on `target`: one inside the
    /// shape wins, then the one closest to `near`. The weight brush paints through it.
    pub fn unskin_point(&self, i: usize, target: Point, near: Option<Point>) -> Option<Point> {
        let bend = self.bender(i)?;
        let base = self.nodes[i].base_path();
        let outer = bend.inv.inverse();
        let back = bend.skin.bind_world.inverse();
        let starts = near.into_iter().chain(bend.moves.iter().map(|m| back * m.inverse() * outer * target)).chain(std::iter::once(target));
        let mut best = (f64::INFINITY, target);
        let mut found: Vec<Point> = Vec::new();
        for start in starts {
            let mut p = start;
            for _ in 0..24 {
                let miss = bend.point(p) - target;
                let err = miss.hypot();
                if err < best.0 {
                    best = (err, p);
                }
                if err < 1e-3 {
                    found.push(p);
                    break;
                }
                // The bend near p, measured: solve J·step = miss.
                let h = 0.25;
                let dx = (bend.point(p + Vec2::new(h, 0.0)) - bend.point(p - Vec2::new(h, 0.0))) / (2.0 * h);
                let dy = (bend.point(p + Vec2::new(0.0, h)) - bend.point(p - Vec2::new(0.0, h))) / (2.0 * h);
                let det = dx.x * dy.y - dx.y * dy.x;
                if det.abs() < 1e-9 {
                    break;
                }
                let step = Vec2::new(dy.y * miss.x - dy.x * miss.y, -dx.y * miss.x + dx.x * miss.y) / det;
                // Long jumps leave the part of the shape the measurement holds for.
                let len = step.hypot();
                p -= if len > 20.0 { step * (20.0 / len) } else { step };
            }
        }
        let inside = |p: &Point| base.as_ref().is_some_and(|b| b.contains(*p));
        let dist = |p: &Point| near.map_or(0.0, |n| (*p - n).hypot());
        let pick =
            found.iter().filter(|p| inside(p)).min_by(|a, b| dist(a).total_cmp(&dist(b))).or_else(|| found.iter().min_by(|a, b| dist(a).total_cmp(&dist(b))));
        Some(pick.copied().unwrap_or(best.1))
    }

    fn bender(&self, i: usize) -> Option<Bender<'_>> {
        let node = &self.nodes[i];
        let skin = node.skin.as_ref().filter(|s| !s.bind.is_empty())?;
        // How each bone moved since the bind pose, on the artboard.
        let moves: Vec<Affine> = skin
            .bind
            .iter()
            .map(|sb| self.index_of(&sb.bone).map_or(Affine::IDENTITY, |b| self.world_transform(b) * sb.to_bone * skin.bind_world.inverse()))
            .collect();
        // Bones that only turn and move are blended as rotations, which keeps an elbow as thick
        // as the arm; a stretched or skewed bone falls back to mixing the moved points.
        let rigid = moves.iter().map(|m| Dual::from_affine(*m)).collect();
        Some(Bender { skin, moves, rigid, inv: self.world_transform(i).inverse() })
    }
}

impl Skin {
    /// How much each bone of `bind` pulls the point `p` (in the shape's space): the distance
    /// weights, then every painted stroke over them in order. The weights add up to 1.
    pub fn weights(&self, p: Point) -> Vec<f64> {
        let sharp = self.sharp.max(0.5);
        let mut w: Vec<f64> = self.bind.iter().map(|sb| (Line::new(sb.a, sb.b).nearest(p, 1e-6).distance_sq.sqrt() + 0.5).powf(-sharp)).collect();
        normalize(&mut w);
        for stroke in &self.paint {
            let Some(k) = self.bind.iter().position(|sb| sb.bone == stroke.bone) else { continue };
            let a = stroke_reach(stroke, p) * stroke.amount.abs().min(1.0);
            if a <= 0.0 {
                continue;
            }
            if stroke.amount > 0.0 {
                for (j, wj) in w.iter_mut().enumerate() {
                    *wj = (1.0 - a) * *wj + if j == k { a } else { 0.0 };
                }
            } else {
                w[k] *= 1.0 - a;
                // A point taken away from its only bone goes to the next nearest one instead.
                if w.iter().sum::<f64>() < 1e-9 {
                    w = self.bind.iter().map(|sb| (Line::new(sb.a, sb.b).nearest(p, 1e-6).distance_sq.sqrt() + 0.5).powf(-sharp)).collect();
                    w[k] = 0.0;
                }
            }
            normalize(&mut w);
        }
        w
    }
}

/// How strongly a stroke reaches `p`: fully within half its radius of its line, fading
/// smoothly to nothing at the radius.
fn stroke_reach(stroke: &WeightStroke, p: Point) -> f64 {
    let r = stroke.radius.max(1e-6);
    let d = match stroke.points.as_slice() {
        [] => return 0.0,
        [only] => (p - *only).hypot(),
        pts => pts.windows(2).map(|s| Line::new(s[0], s[1]).nearest(p, 1e-6).distance_sq).fold(f64::INFINITY, f64::min).sqrt(),
    };
    let t = ((r - d) / (r * 0.5)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn normalize(w: &mut [f64]) {
    let total: f64 = w.iter().sum();
    if total > 1e-12 {
        w.iter_mut().for_each(|x| *x /= total);
    }
}

/// The bones of one skinned shape as they are posed now.
struct Bender<'a> {
    skin: &'a Skin,
    moves: Vec<Affine>,
    rigid: Option<Vec<Dual>>,
    inv: Affine,
}

impl Bender<'_> {
    fn point(&self, p: Point) -> Point {
        let world = self.skin.bind_world * p;
        let w = self.skin.weights(p);
        let moved = match &self.rigid {
            Some(duals) => {
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
                for (m, wk) in self.moves.iter().zip(&w) {
                    sum += (*m * world).to_vec2() * *wk;
                }
                sum.to_point()
            }
        };
        self.inv * moved
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
pub(crate) fn bent(path: &BezPath, f: impl Fn(Point) -> Point) -> BezPath {
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
    use crate::{Artboard, Document, Node, ShapeKind, Skin, WeightStroke};
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

    #[test]
    fn painted_weights_move_the_bend() {
        let plain = arm(90.0);
        let near = Point::new(-10.0, 0.0); // 40 along the bar, in the shape's space (x 10 → -40)
        let w = plain.nodes[2].skin.as_ref().unwrap().weights(near);
        assert!((w.iter().sum::<f64>() - 1.0).abs() < 1e-9 && w[0] > 0.6, "{w:?}");
        // Paint the second bone over the upper arm: those points now swing with the forearm.
        let mut d = arm(0.0);
        d.nodes[1].rotation = 0.0;
        let mut d2 = d.clone();
        d2.nodes[2].skin.as_mut().unwrap().paint.push(WeightStroke {
            bone: "b".into(),
            radius: 15.0,
            amount: 1.0,
            points: vec![Point::new(-10.0, -10.0), Point::new(-10.0, 10.0)],
        });
        let w = d2.nodes[2].skin.as_ref().unwrap().weights(near);
        assert!(w[1] > 0.99, "{w:?}");
        // Far from the stroke nothing changes.
        let far = Point::new(-45.0, 0.0);
        assert_eq!(d2.nodes[2].skin.as_ref().unwrap().weights(far), d.nodes[2].skin.as_ref().unwrap().weights(far));
        d2.nodes[1].rotation = 90.0;
        d2.rig();
        let moved = d2.skin_point(2, near).unwrap();
        let before = plain.skin_point(2, near).unwrap();
        assert!((moved - near).hypot() > (before - near).hypot() + 5.0, "{moved:?} vs {before:?}");
        // The brush finds its way back from the bent outline to the shape as drawn.
        for p in [near, Point::new(30.0, 5.0), Point::new(40.0, -8.0), Point::new(-40.0, 8.0)] {
            let back = d2.unskin_point(2, d2.skin_point(2, p).unwrap(), None).unwrap();
            assert!((back - p).hypot() < 0.01, "{p:?} → {back:?}");
        }
        // Erasing hands the point back to the other bone.
        d2.nodes[2].skin.as_mut().unwrap().paint.push(WeightStroke { bone: "b".into(), radius: 15.0, amount: -1.0, points: vec![near] });
        let w = d2.nodes[2].skin.as_ref().unwrap().weights(near);
        assert!(w[1] < 1e-6 && (w[0] - 1.0).abs() < 1e-6, "{w:?}");
    }
}
