//! Particles (`emit`): a shape becomes an emitter, and copies of it fly out, fall, spin, shrink
//! and fade. Sparks, dust, leaves, magic, rain. Randomness is seeded by the emitter and the
//! particle's number, so the same time always gives the same picture and export bakes it.

use crate::Document;
use kurbo::{Affine, Point, Vec2};

/// Settings of an emitter (`emit 30 life 1 speed 200 angle -90 spread 40 …`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Emitter {
    /// Particles per second.
    pub rate: f64,
    /// Particles thrown out at once when the animation starts.
    pub burst: f64,
    /// Seconds a particle lives, give or take a quarter.
    pub life: f64,
    /// Starting speed, artboard units per second, give or take a quarter.
    pub speed: f64,
    /// Direction in degrees in the emitter's own space: 0 is right, -90 is up.
    pub angle: f64,
    /// Width of the cone the particles fly out in, degrees; 360 is every way.
    pub spread: f64,
    /// Downward acceleration, units per second squared.
    pub gravity: f64,
    /// Air drag: how much of its speed a particle loses per second, 0..1.
    pub drag: f64,
    /// Opacity at the end of life, relative to the start (0 fades out).
    pub fade: f64,
    /// Size at the end of life, relative to the start (0 shrinks to nothing).
    pub grow: f64,
    /// Greatest turning speed, degrees per second; each particle gets a random one up to it.
    pub spin: f64,
}

impl Default for Emitter {
    fn default() -> Self {
        Emitter { rate: 20.0, burst: 0.0, life: 1.0, speed: 120.0, angle: -90.0, spread: 30.0, gravity: 0.0, drag: 0.0, fade: 0.0, grow: 1.0, spin: 0.0 }
    }
}

/// Most particles alive per emitter.
const MAX: usize = 2000;

/// One particle on the artboard.
#[derive(Clone, Debug, PartialEq)]
struct Particle {
    p: Point,
    v: Vec2,
    age: f64,
    life: f64,
    rot: f64,
    spin: f64,
}

/// An emitter and the particles it has thrown.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Spray {
    node: usize,
    emitter: Emitter,
    /// Part of the next particle accumulated from the rate.
    owed: f64,
    /// Particles thrown so far: the next one's number, which seeds its randomness.
    thrown: u64,
    parts: Vec<Particle>,
}

/// A number in 0..1 from a seed (splitmix64).
pub(crate) fn random(seed: u64) -> f64 {
    (mix(seed) >> 11) as f64 / (1u64 << 53) as f64
}

/// What the game's seed (`Document::seed`) adds to a seed: nothing for 0.
pub(crate) fn salt(game: u64) -> u64 {
    if game == 0 { 0 } else { mix(game ^ 0x6a09_e667_f3bc_c908) }
}

fn mix(seed: u64) -> u64 {
    let mut z = seed.wrapping_add(0x9e37_79b9_7f4a_7c15);
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    z ^ (z >> 31)
}

impl Spray {
    fn throw(&mut self, doc: &Document) {
        if self.parts.len() >= MAX || self.node >= doc.nodes.len() {
            return;
        }
        let w = doc.world_transform(self.node);
        let base = {
            let x = w * Point::new(1.0, 0.0) - w * Point::ZERO;
            x.y.atan2(x.x)
        };
        let seed = ((self.node as u64) << 40) ^ (self.thrown * 8) ^ salt(doc.seed);
        let r = |k: u64| random(seed + k);
        self.thrown += 1;
        let e = &self.emitter;
        let dir = base + e.angle.to_radians() + (r(0) - 0.5) * e.spread.to_radians();
        let speed = e.speed * (1.0 + (r(1) - 0.5) * 0.5);
        self.parts.push(Particle {
            p: w * Point::ZERO,
            v: Vec2::new(dir.cos(), dir.sin()) * speed,
            age: 0.0,
            life: (e.life * (1.0 + (r(2) - 0.5) * 0.5)).max(0.01),
            rot: 0.0,
            spin: (r(3) * 2.0 - 1.0) * e.spin,
        });
    }

    fn step(&mut self, doc: &Document, dt: f64) {
        let e = self.emitter;
        let keep = (1.0 - e.drag.clamp(0.0, 1.0)).powf(dt);
        let node = &doc.nodes[self.node];
        let pull = doc.world.pull(node, e.gravity);
        let me = self.node;
        for q in &mut self.parts {
            q.v += (pull + doc.push(me, q.p)) * dt;
            q.v *= keep;
            q.p += q.v * dt;
            q.age += dt;
            q.rot += q.spin * dt;
        }
        self.parts.retain(|q| q.age < q.life);
        self.owed += e.rate.max(0.0) * dt;
        while self.owed >= 1.0 {
            self.owed -= 1.0;
            self.throw(doc);
        }
    }
}

/// Every emitter of `pose`, with its opening burst thrown.
pub(crate) fn sprays(pose: &Document) -> Vec<Spray> {
    let mut out: Vec<Spray> =
        (0..pose.nodes.len()).filter_map(|node| Some(Spray { node, emitter: pose.nodes[node].emit?, owed: 0.0, thrown: 0, parts: Vec::new() })).collect();
    for s in &mut out {
        for _ in 0..s.emitter.burst.max(0.0).round() as usize {
            s.throw(pose);
        }
    }
    out
}

pub(crate) fn step_sprays(sprays: &mut [Spray], pose: &Document, dt: f64) {
    for s in sprays {
        s.step(pose, dt);
    }
}

/// Hands each emitter of `doc` its particles: where each copy of the shape is drawn and how
/// opaque it is.
pub(crate) fn apply_sprays(sprays: &[Spray], doc: &mut Document) {
    for s in sprays {
        if s.node >= doc.nodes.len() {
            continue;
        }
        let w = doc.world_transform(s.node);
        // The emitter's own size and skew carry over to its particles; its position does not.
        let [a, b, c, d, _, _] = w.as_coeffs();
        let shape = Affine::new([a, b, c, d, 0.0, 0.0]);
        let e = s.emitter;
        doc.nodes[s.node].particles = Some(
            s.parts
                .iter()
                .map(|q| {
                    let k = (q.age / q.life).clamp(0.0, 1.0);
                    let size = 1.0 + (e.grow - 1.0) * k;
                    let opacity = 1.0 + (e.fade.clamp(0.0, 1.0) - 1.0) * k;
                    (Affine::translate(q.p.to_vec2()) * Affine::rotate(q.rot.to_radians()) * Affine::scale(size.max(0.0)) * shape, opacity)
                })
                .collect(),
        );
    }
}

#[cfg(test)]
mod tests {
    use crate::{Animation, Artboard, Document, Emitter, Node, ShapeKind, SimCache};
    use kurbo::Point;

    fn fountain(e: Emitter) -> (Document, Animation) {
        let mut d = Document::new(Artboard::default());
        let mut n = Node::new("spark", ShapeKind::Ellipse);
        (n.x, n.y, n.width, n.height) = (100.0, 200.0, 6.0, 6.0);
        n.emit = Some(e);
        d.nodes.push(n);
        (d, Animation { name: "Go".into(), duration: 3.0, looping: false, tracks: Vec::new(), events: Vec::new() })
    }

    fn at(d: &Document) -> Vec<(Point, f64)> {
        d.nodes[0].particles.as_ref().unwrap().iter().map(|(a, o)| (*a * Point::ZERO, *o)).collect()
    }

    #[test]
    fn an_emitter_throws_particles_that_fly_and_die() {
        let (d, a) = fountain(Emitter { rate: 30.0, life: 1.0, speed: 100.0, ..Emitter::default() });
        let mut cache = SimCache::default();
        // Half a second in: about 15 particles, all above the emitter, the oldest highest.
        let p = at(&a.pose_with(&d, 0.5, &mut cache));
        assert!((13..=16).contains(&p.len()), "{}", p.len());
        assert!(p.iter().all(|(q, _)| q.y <= 200.0 + 1e-9));
        assert!(p.first().unwrap().0.y < p.last().unwrap().0.y);
        // They fade as they age.
        assert!(p.first().unwrap().1 < p.last().unwrap().1);
        // Later the count stays near rate × life: old ones die as new ones come.
        let n = at(&a.pose_with(&d, 2.5, &mut cache)).len();
        assert!((22..=38).contains(&n), "{n}");
        // The same moment gives the same particles.
        assert_eq!(at(&a.pose_with(&d, 1.3, &mut SimCache::default())), at(&a.pose_with(&d, 1.3, &mut cache)));
        // Outside a simulation the shape is drawn as itself.
        let mut plain = d.clone();
        plain.rig();
        assert!(plain.nodes[0].particles.is_none());
    }

    #[test]
    fn a_burst_with_gravity_falls_back_down() {
        let (d, a) = fountain(Emitter { rate: 0.0, burst: 20.0, life: 3.0, speed: 200.0, spread: 60.0, gravity: 400.0, fade: 1.0, ..Emitter::default() });
        let mut cache = SimCache::default();
        assert_eq!(at(&a.pose_with(&d, 0.0, &mut cache)).len(), 20);
        let up = at(&a.pose_with(&d, 0.4, &mut cache));
        assert!(up.iter().all(|(q, _)| q.y < 200.0), "rising");
        let down = at(&a.pose_with(&d, 2.0, &mut cache));
        assert!(down.iter().all(|(q, _)| q.y > 200.0), "fallen back below the emitter");
        assert!(down.iter().all(|(_, o)| (*o - 1.0).abs() < 1e-9), "fade 1 keeps them opaque");
    }
}
