//! Springs: keyed numbers that overshoot and settle instead of stopping dead (`spring`), and
//! bones that lag behind and swing after the bone they hang from (`jiggle`). Both are stepped by
//! the simulation in `sim.rs`, so the same time always gives the same pose and export bakes them.

use crate::{Document, Prop, ShapeKind, Value};
use kurbo::{Point, Vec2};

/// How a value chases its target: `freq` swings per second, `damp` how fast the swinging dies
/// out (1 and above settles without overshooting).
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Spring {
    pub freq: f64,
    pub damp: f64,
}

impl Default for Spring {
    fn default() -> Self {
        Spring { freq: 3.0, damp: 0.3 }
    }
}

impl Spring {
    /// Highest frequency the fixed simulation steps follow without blowing up.
    pub const MAX_FREQ: f64 = 30.0;

    /// Acceleration towards `target` for a value at `x` moving at `v`.
    pub(crate) fn pull<T>(self, x: T, v: T, target: T) -> T
    where
        T: std::ops::Sub<Output = T> + std::ops::Mul<f64, Output = T>,
    {
        let w = std::f64::consts::TAU * self.freq.clamp(0.01, Self::MAX_FREQ);
        (target - x) * (w * w) - v * (2.0 * self.damp.max(0.0) * w)
    }
}

/// Properties a `spring` moves: the ones that place and size a shape. Colours and opacity are
/// left alone, since overshooting them flickers instead of bouncing.
pub const SPRUNG: [Prop; 15] = [
    Prop::X,
    Prop::Y,
    Prop::Width,
    Prop::Height,
    Prop::Rotation,
    Prop::ScaleX,
    Prop::ScaleY,
    Prop::SkewX,
    Prop::SkewY,
    Prop::Radius,
    Prop::Lat,
    Prop::Lon,
    Prop::Alt,
    Prop::Yaw,
    Prop::Pitch,
];

/// One property of one node following its keyed value on a spring.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Sprung {
    node: usize,
    prop: Prop,
    spring: Spring,
    x: f64,
    v: f64,
}

/// The tip of a `jiggle` bone, moving on its own and pulled towards where the pose puts it.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Tip {
    node: usize,
    spring: Spring,
    p: Point,
    v: Vec2,
}

fn num(doc: &Document, node: usize, prop: Prop) -> Option<f64> {
    match doc.nodes.get(node)?.get(prop) {
        Value::Num(v) => Some(v),
        _ => None,
    }
}

/// Every sprung property of `keys`, at rest on its keyed value.
pub(crate) fn sprung(keys: &Document) -> Vec<Sprung> {
    let mut out = Vec::new();
    for (node, n) in keys.nodes.iter().enumerate() {
        let Some(spring) = n.spring else { continue };
        for prop in SPRUNG {
            if let Some(x) = num(keys, node, prop) {
                out.push(Sprung { node, prop, spring, x, v: 0.0 });
            }
        }
    }
    out
}

/// Moves sprung values `dt` seconds towards `keys`, in `steps` equal steps.
pub(crate) fn step_values(values: &mut [Sprung], keys: &Document, dt: f64, steps: usize) {
    let h = dt / steps as f64;
    for s in values {
        let Some(target) = num(keys, s.node, s.prop) else { continue };
        for _ in 0..steps {
            s.v += s.spring.pull(s.x, s.v, target) * h;
            s.x += s.v * h;
        }
    }
}

/// Writes the sprung values into `doc`.
pub(crate) fn apply_values(values: &[Sprung], doc: &mut Document) {
    for s in values {
        if let Some(n) = doc.nodes.get_mut(s.node) {
            n.set(s.prop, Value::Num(s.x));
        }
    }
}

fn depth(doc: &Document, mut i: usize) -> usize {
    let mut d = 0;
    while let Some(p) = doc.nodes[i].parent {
        (i, d) = (p, d + 1);
    }
    d
}

/// The tips of `jiggle` bones where `pose` puts them, parents before children.
pub(crate) fn tips(pose: &Document) -> Vec<Tip> {
    let mut bones: Vec<usize> = (0..pose.nodes.len()).filter(|&i| pose.nodes[i].kind == ShapeKind::Bone && pose.nodes[i].jiggle.is_some()).collect();
    bones.sort_by_key(|&i| depth(pose, i));
    bones.into_iter().map(|node| Tip { node, spring: pose.nodes[node].jiggle.unwrap(), p: ends(pose, node).1, v: Vec2::ZERO }).collect()
}

/// A bone's base and tip on the artboard.
fn ends(doc: &Document, i: usize) -> (Point, Point) {
    let w = doc.world_transform(i);
    (w * Point::ZERO, w * doc.nodes[i].bone_tip())
}

/// Turns bone `i` about its base so that its tip points at `p`.
fn aim(doc: &mut Document, i: usize, p: Point) {
    let (base, tip) = ends(doc, i);
    let (to, from) = (p - base, tip - base);
    if to.hypot2() < 1e-12 || from.hypot2() < 1e-12 {
        return;
    }
    let turn = (from.cross(to)).atan2(from.dot(to));
    // A mirrored parent turns the bone the other way round.
    let mirrored = doc.nodes[i].parent.is_some_and(|p| doc.world_transform(p).determinant() < 0.0);
    doc.nodes[i].rotation += if mirrored { -turn } else { turn }.to_degrees();
}

/// Moves the tips `dt` seconds towards where `pose` puts them, in `steps` equal steps, and turns
/// the bones of `pose` to follow them.
pub(crate) fn step_tips(tips: &mut [Tip], pose: &mut Document, dt: f64, steps: usize) {
    let h = dt / steps as f64;
    let rest: Vec<f64> = tips.iter().map(|t| pose.nodes.get(t.node).map_or(0.0, |n| n.rotation)).collect();
    for _ in 0..steps {
        for (t, &rot) in tips.iter_mut().zip(&rest) {
            if t.node >= pose.nodes.len() {
                continue;
            }
            pose.nodes[t.node].rotation = rot;
            let (base, target) = ends(pose, t.node);
            t.v += t.spring.pull(t.p.to_vec2(), t.v, target.to_vec2()) * h;
            t.p += t.v * h;
            // Bones keep their length: the tip stays on the circle around the base, and only
            // the motion along that circle is kept.
            let len = (target - base).hypot();
            let out = t.p - base;
            if out.hypot2() > 1e-12 {
                let dir = out.normalize();
                t.p = base + dir * len;
                t.v -= dir * t.v.dot(dir);
            }
            aim(pose, t.node, t.p);
        }
    }
}

/// Turns the bones of `doc` to the tips, parents first.
pub(crate) fn apply_tips(tips: &[Tip], doc: &mut Document) {
    for t in tips {
        if t.node < doc.nodes.len() {
            aim(doc, t.node, t.p);
        }
    }
}

impl crate::sim::Carry for Sprung {
    fn node(&self) -> usize {
        self.node
    }
    fn carry(&mut self, old: &Self) {
        if self.prop == old.prop {
            (self.x, self.v) = (old.x, old.v);
        }
    }
}

impl crate::sim::Carry for Tip {
    fn node(&self) -> usize {
        self.node
    }
    fn carry(&mut self, old: &Self) {
        (self.p, self.v) = (old.p, old.v);
    }
}
