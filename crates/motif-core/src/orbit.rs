//! 2.5D orbits: an object placed on a ball by latitude and longitude moves over it as over a
//! globe while staying a flat vector shape. Turning the ball (`yaw`, `pitch`) carries everything
//! on it around; what goes round the back is hidden behind the ball. With `wrap` the shape lies
//! on the surface: it bends over the ball and narrows towards the edge, like a print on a globe.

use crate::{Document, ShapeKind};
use kurbo::{Affine, Point, Vec2};

/// A point of the unit sphere: x right, y down, z towards the viewer.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct V3 {
    pub(crate) x: f64,
    pub(crate) y: f64,
    pub(crate) z: f64,
}

impl V3 {
    pub(crate) const ZERO: V3 = V3 { x: 0.0, y: 0.0, z: 0.0 };

    pub(crate) fn new(x: f64, y: f64, z: f64) -> V3 {
        V3 { x, y, z }
    }
    pub(crate) fn add(self, o: V3) -> V3 {
        V3::new(self.x + o.x, self.y + o.y, self.z + o.z)
    }
    pub(crate) fn sub(self, o: V3) -> V3 {
        V3::new(self.x - o.x, self.y - o.y, self.z - o.z)
    }
    pub(crate) fn mul(self, k: f64) -> V3 {
        V3::new(self.x * k, self.y * k, self.z * k)
    }
    pub(crate) fn dot(self, o: V3) -> f64 {
        self.x * o.x + self.y * o.y + self.z * o.z
    }
    pub(crate) fn cross(self, o: V3) -> V3 {
        V3::new(self.y * o.z - self.z * o.y, self.z * o.x - self.x * o.z, self.x * o.y - self.y * o.x)
    }
    pub(crate) fn len(self) -> f64 {
        (self.x * self.x + self.y * self.y + self.z * self.z).sqrt()
    }
    pub(crate) fn unit(self) -> V3 {
        let l = self.len();
        if l < 1e-12 { V3::new(0.0, 0.0, 1.0) } else { self.mul(1.0 / l) }
    }
}

/// How a ball is turned: around the vertical axis by `yaw`, then tipped by `pitch` (degrees).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Turn {
    pub(crate) yaw: f64,
    pub(crate) pitch: f64,
}

impl Turn {
    pub(crate) fn apply(self, p: V3) -> V3 {
        let (sy, cy) = self.yaw.to_radians().sin_cos();
        let (sp, cp) = self.pitch.to_radians().sin_cos();
        // Yaw turns the front towards the right; pitch tips the top towards the viewer.
        let a = V3::new(p.x * cy + p.z * sy, p.y, -p.x * sy + p.z * cy);
        V3::new(a.x, a.y * cp + a.z * sp, -a.y * sp + a.z * cp)
    }

    /// The point that `apply` takes to `q`.
    pub(crate) fn undo(self, q: V3) -> V3 {
        let (sy, cy) = self.yaw.to_radians().sin_cos();
        let (sp, cp) = self.pitch.to_radians().sin_cos();
        let a = V3::new(q.x, q.y * cp - q.z * sp, q.y * sp + q.z * cp);
        V3::new(a.x * cy - a.z * sy, a.y, a.x * sy + a.z * cy)
    }
}

/// The unit-sphere point at latitude `lat` (up is positive) and longitude `lon` (right is
/// positive), with the directions east and north there.
pub(crate) fn frame(lat: f64, lon: f64) -> (V3, V3, V3) {
    let (sf, cf) = lat.to_radians().sin_cos();
    let (sl, cl) = lon.to_radians().sin_cos();
    let at = V3::new(cf * sl, -sf, cf * cl);
    let east = V3::new(cl, 0.0, -sl);
    let north = V3::new(-sf * sl, -cf, -sf * cl);
    (at, east, north)
}

/// A ball as the objects on it see it: its transform to the artboard, radii and turn.
pub(crate) struct Ball {
    pub(crate) world: Affine,
    pub(crate) rx: f64,
    pub(crate) ry: f64,
    pub(crate) turn: Turn,
}

impl Ball {
    pub(crate) fn of(doc: &Document, s: usize) -> Ball {
        let n = &doc.nodes[s];
        Ball { world: doc.world_transform(s), rx: n.width.abs() / 2.0, ry: n.height.abs() / 2.0, turn: Turn { yaw: n.yaw, pitch: n.pitch } }
    }

    /// A unit-sphere point `alt` units above the surface, turned with the ball: where it is
    /// on the artboard and whether it faces the viewer. Behind the ball, `limb` pulls it to
    /// the edge instead, so a shape lying on the surface slides off the visible half.
    fn place(&self, p: V3, alt: f64, limb: bool) -> (Point, f64) {
        let mut q = self.turn.apply(p);
        if limb && q.z < 0.0 {
            let r = (q.x * q.x + q.y * q.y).sqrt().max(1e-9);
            q = V3::new(q.x / r, q.y / r, 0.0);
        }
        let local = Point::new(q.x * (self.rx + alt), q.y * (self.ry + alt));
        (self.world * local, q.z)
    }
}

impl Document {
    /// Places objects that sit on a ball (`on`). Ones gone round its back are marked to be
    /// hidden behind it, and shapes lying on it (`wrap`) to be clipped to it.
    /// Runs after the keys and springs, before IK, so a target on a ball works too.
    pub fn place_orbits(&mut self) {
        for i in 0..self.nodes.len() {
            self.nodes[i].behind = None;
            let Some(s) = self.nodes[i].on.as_deref().and_then(|id| self.index_of(id)).filter(|&s| s != i && self.nodes[s].kind != ShapeKind::Group) else {
                continue;
            };
            let n = &self.nodes[i];
            let ball = Ball::of(self, s);
            let (at, _, _) = frame(n.lat, n.lon);
            let (world, z) = ball.place(at, n.alt, false);
            let local = n.parent.map_or(world, |p| self.world_transform(p).inverse() * world);
            let wrap = n.wrap;
            let n = &mut self.nodes[i];
            (n.x, n.y) = (local.x, local.y);
            if wrap {
                n.behind = Some((s, true));
            } else if z < 0.0 {
                n.behind = Some((s, false));
            }
        }
    }

    /// The outline of a `wrap` shape bent over its ball, in the shape's own space; empty when
    /// it is all on the far side.
    pub(crate) fn wrapped(&self, i: usize) -> Option<kurbo::BezPath> {
        let n = &self.nodes[i];
        if !n.wrap {
            return None;
        }
        let s = n.on.as_deref().and_then(|id| self.index_of(id)).filter(|&s| s != i)?;
        let base = n.base_path()?;
        let ball = Ball::of(self, s);
        let (at, east, north) = frame(n.lat, n.lon);
        // The shape's own turn, scale and skew act on the surface; its position is the ball's.
        let own = Affine::new({
            let [a, b, c, d, _, _] = n.local_transform().as_coeffs();
            [a, b, c, d, 0.0, 0.0]
        });
        let r = (ball.rx.min(ball.ry) + n.alt).max(1e-6);
        let inv = self.world_transform(i).inverse();
        let front = std::cell::Cell::new(f64::MIN);
        let bend = |p: Point| {
            let t: Vec2 = own * p - Point::ZERO;
            // Walk from the centre along the surface: east for x, south for y.
            let step = east.mul(t.x).add(north.mul(-t.y));
            let d = step.len() / r;
            let q = if d < 1e-12 { at } else { at.mul(d.cos()).add(step.mul(d.sin() / step.len())) };
            front.set(front.get().max(ball.turn.apply(q).z));
            inv * ball.place(q, n.alt, true).0
        };
        let out = crate::skin::bent(&base, bend);
        Some(if front.get() < 0.0 { kurbo::BezPath::new() } else { out })
    }
}

#[cfg(test)]
mod tests {
    use crate::{Artboard, Document, Node, ShapeKind};
    use kurbo::{Point, Shape};

    /// A ball of radius 100 at (200, 200) with a moon 20 above it and a spot on its surface.
    fn globe(lon: f64) -> Document {
        let mut d = Document::new(Artboard::default());
        let mut ball = Node::new("ball", ShapeKind::Ellipse);
        (ball.x, ball.y, ball.width, ball.height) = (200.0, 200.0, 200.0, 200.0);
        let mut moon = Node::new("moon", ShapeKind::Ellipse);
        (moon.width, moon.height, moon.alt, moon.lon) = (10.0, 10.0, 20.0, lon);
        moon.on = Some("ball".into());
        let mut spot = Node::new("spot", ShapeKind::Ellipse);
        (spot.width, spot.height, spot.lon, spot.wrap) = (40.0, 40.0, lon, true);
        spot.on = Some("ball".into());
        d.nodes = vec![ball, moon, spot];
        d.rig();
        d
    }

    fn bounds(d: &Document, i: usize) -> kurbo::Rect {
        (d.world_transform(i) * d.nodes[i].path().unwrap()).bounding_box()
    }

    #[test]
    fn objects_go_round_the_ball() {
        // In front: the moon sits 120 from the centre, the spot is round.
        let d = globe(0.0);
        assert!((Point::new(d.nodes[1].x, d.nodes[1].y) - Point::new(200.0, 200.0)).hypot() < 1e-9);
        let b = bounds(&d, 2);
        assert!((b.width() - 40.0).abs() < 1.0 && (b.height() - 40.0).abs() < 1.0, "{b:?}");
        // A quarter round: the moon is at the right edge, the spot is squeezed into a sliver.
        let d = globe(90.0);
        assert!((d.nodes[1].x - 320.0).abs() < 1e-9 && (d.nodes[1].y - 200.0).abs() < 1e-9);
        assert!(d.nodes[1].behind.is_none());
        let b = bounds(&d, 2);
        assert!(b.width() < 12.0 && b.max_x() <= 300.0 + 1e-6, "{b:?}");
        // Round the back: the moon is hidden behind the ball, the spot is gone into the edge.
        let d = globe(180.0);
        assert_eq!(d.nodes[1].behind, Some((0, false)));
        assert!(d.nodes[2].path().unwrap().is_empty());
    }

    #[test]
    fn turning_the_ball_carries_things_on_it() {
        let mut d = globe(0.0);
        d.nodes[0].yaw = 90.0;
        d.rig();
        assert!((d.nodes[1].x - 320.0).abs() < 1e-9, "yaw turns the front to the right: {}", d.nodes[1].x);
        d.nodes[0].yaw = 0.0;
        d.nodes[0].pitch = 90.0;
        d.nodes[1].lat = 90.0;
        d.rig();
        assert!((d.nodes[1].y - 200.0).abs() < 1e-9 && d.nodes[1].behind.is_none(), "the pole tips towards the viewer");
    }
}
