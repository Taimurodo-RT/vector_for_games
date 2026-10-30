//! Automatic squash and stretch (`squash`): a moving object stretches along its motion and
//! squashes when it stops, keeping its area. The stretch follows the speed on a spring, so a
//! sudden stop swings it past zero into a squash, and it wobbles back to the drawn shape. A hit
//! that turns the motion around (a bounce) kicks the spring towards a squash as well.

use crate::{Document, Spring};
use kurbo::{Affine, Point, Vec2};

/// Settings of `squash A F D`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Squash {
    /// Stretch per speed: 1 stretches by half at 500 artboard units per second.
    pub amount: f64,
    /// How the stretch follows the speed: a low damping squashes harder on a stop.
    pub spring: Spring,
}

impl Default for Squash {
    fn default() -> Self {
        Squash { amount: 0.5, spring: Spring { freq: 4.0, damp: 0.3 } }
    }
}

/// Stretch at amount 1, per artboard unit per second.
const PER_SPEED: f64 = 0.5 / 500.0;
/// Limits of the stretch: 1.5 makes an object 2.5 times longer, -0.6 squashes it to 0.4.
const MAX: f64 = 1.5;
const MIN: f64 = -0.6;
/// Slower than this, the direction of motion is kept from before (a stop squashes along it).
const STILL: f64 = 5.0;

/// A squashing object: where it was, which way it moved, and how stretched it is.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Stretch {
    node: usize,
    squash: Squash,
    prev: Point,
    /// Velocity measured on the step before, artboard units per second.
    vel: Vec2,
    dir: Vec2,
    e: f64,
    v: f64,
}

/// The point a node turns and scales about, on the artboard.
fn anchor(doc: &Document, i: usize) -> Point {
    let n = &doc.nodes[i];
    let at = Point::new(n.x, n.y) + n.pivot;
    n.parent.map_or(at, |p| doc.world_transform(p) * at)
}

/// Every squashing node of `pose`, at rest.
pub(crate) fn stretches(pose: &Document) -> Vec<Stretch> {
    (0..pose.nodes.len())
        .filter_map(|node| {
            let squash = pose.nodes[node].squash?;
            Some(Stretch { node, squash, prev: anchor(pose, node), vel: Vec2::ZERO, dir: Vec2::new(1.0, 0.0), e: 0.0, v: 0.0 })
        })
        .collect()
}

/// Moves the stretches `dt` seconds on, measuring the motion in `pose`, and warps `pose`.
pub(crate) fn step_stretches(items: &mut [Stretch], pose: &mut Document, dt: f64, steps: usize) {
    if dt <= 0.0 {
        return;
    }
    for s in items.iter_mut() {
        if s.node >= pose.nodes.len() {
            continue;
        }
        let at = anchor(pose, s.node);
        let vel = (at - s.prev) / dt;
        s.prev = at;
        // Slowing down along the motion (hitting something) pushes the stretch towards a squash,
        // speeding up pushes it towards a stretch.
        let along = (vel - s.vel).dot(s.dir);
        s.vel = vel;
        let w = std::f64::consts::TAU * s.squash.spring.freq;
        s.v += s.squash.amount * along * PER_SPEED * w;
        let speed = vel.hypot();
        if speed > STILL {
            s.dir = vel / speed;
        }
        let target = (s.squash.amount * speed * PER_SPEED).clamp(0.0, MAX);
        let h = dt / steps as f64;
        for _ in 0..steps {
            s.v += s.squash.spring.pull(s.e, s.v, target) * h;
            s.e += s.v * h;
        }
    }
    apply_stretches(items, pose);
}

/// Warps the nodes of `doc` by their stretches.
pub(crate) fn apply_stretches(items: &[Stretch], doc: &mut Document) {
    for s in items {
        if s.node >= doc.nodes.len() {
            continue;
        }
        // The direction of motion in the space the node is placed in.
        let dir = match doc.nodes[s.node].parent {
            Some(p) => {
                let inv = doc.world_transform(p).inverse();
                let d = inv * s.dir.to_point() - inv * Point::ZERO;
                if d.hypot2() > 1e-12 { d.normalize() } else { s.dir }
            }
            None => s.dir,
        };
        let k = 1.0 + s.e.clamp(MIN, MAX);
        let turn = Affine::rotate(dir.y.atan2(dir.x));
        doc.nodes[s.node].warp = Some(turn * Affine::scale_non_uniform(k, 1.0 / k) * turn.inverse());
    }
}

impl crate::sim::Carry for Stretch {
    fn node(&self) -> usize {
        self.node
    }
    fn carry(&mut self, old: &Self) {
        (self.prev, self.vel, self.dir, self.e, self.v) = (old.prev, old.vel, old.dir, old.e, old.v);
    }
}
