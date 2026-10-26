//! The scene's world (`world gravity 0 900 wind 200 0 gust 0.4`): which way things fall and how
//! the wind blows, shared by everything simulated. Each object takes its share by its weight
//! (`weight`) and how much wind it catches (`sail`).

use crate::{Node, Prop, Value};
use kurbo::{Point, Vec2};

/// Gravity of a scene that sets none, artboard units per second squared, straight down.
pub const GRAVITY: f64 = 900.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct World {
    /// Acceleration of an object of weight 1, artboard units per second squared.
    pub gravity: Vec2,
    /// Acceleration the wind gives an object with sail 1.
    pub wind: Vec2,
    /// How much the wind swells and dies down, 0..1: 0 blows evenly.
    pub gust: f64,
    /// Seconds since the simulation started, which drives the gusts. Not part of the script.
    pub time: f64,
}

impl Default for World {
    fn default() -> Self {
        World { gravity: Vec2::new(0.0, GRAVITY), wind: Vec2::ZERO, gust: 0.0, time: 0.0 }
    }
}

impl World {
    /// The keyable properties of the world, as `world.gravity-x` and so on in an animation.
    pub const PROPS: [Prop; 5] = [Prop::GravityX, Prop::GravityY, Prop::WindX, Prop::WindY, Prop::Gust];

    pub fn get(&self, prop: Prop) -> Value {
        Value::Num(match prop {
            Prop::GravityX => self.gravity.x,
            Prop::GravityY => self.gravity.y,
            Prop::WindX => self.wind.x,
            Prop::WindY => self.wind.y,
            Prop::Gust => self.gust,
            _ => return Value::None,
        })
    }

    pub fn set(&mut self, prop: Prop, value: Value) {
        let Value::Num(v) = value else { return };
        match prop {
            Prop::GravityX => self.gravity.x = v,
            Prop::GravityY => self.gravity.y = v,
            Prop::WindX => self.wind.x = v,
            Prop::WindY => self.wind.y = v,
            Prop::Gust => self.gust = v,
            _ => {}
        }
    }

    /// True when it is the world of a scene that sets none.
    pub fn is_default(&self) -> bool {
        World { time: self.time, ..World::default() } == *self
    }

    /// The pull of gravity on node `n`, whose own simulation falls by `own` (its old
    /// `gravity`) when it sets no `weight`.
    pub(crate) fn pull(&self, n: &Node, own: f64) -> Vec2 {
        self.gravity * weight(n, own)
    }

    /// The wind's push at `p` on node `n`: it swells and dies down, and runs over the scene in
    /// waves, so neighbours sway a little out of step.
    pub(crate) fn blow(&self, n: &Node, p: Point) -> Vec2 {
        if self.wind == Vec2::ZERO {
            return Vec2::ZERO;
        }
        let t = self.time;
        let along = self.wind.normalize();
        // Waves run downwind: the phase is how far along the wind the point is.
        let ph = p.to_vec2().dot(along) * 0.01;
        let swell = 0.5 + 0.5 * (t * 1.3 - ph).sin() * (t * 0.47 + 1.0 - ph * 0.6).sin();
        let ripple = (t * 5.0 - ph * 3.0).sin() * 0.25;
        let gust = self.gust.clamp(0.0, 1.0);
        let strength = 1.0 - gust + gust * (swell * 2.0 + ripple).max(0.0);
        // Gusts also turn the wind a little.
        let turn = gust * 0.3 * (t * 0.9 - ph * 1.7).sin();
        let (s, c) = turn.sin_cos();
        let w = Vec2::new(self.wind.x * c - self.wind.y * s, self.wind.x * s + self.wind.y * c);
        w * strength * sail(n)
    }
}

/// How heavy node `n` is: its `weight`, or its simulation's own old `gravity` as a share of the
/// usual one.
pub fn weight(n: &Node, own: f64) -> f64 {
    n.weight.unwrap_or(own / GRAVITY)
}

/// The old `gravity` of the simulation node `n` has that falls (particles, rope, fluid,
/// rolling, soft body, in the order `gravity` goes to them), or `None` when nothing on it falls.
pub fn own_gravity(n: &Node) -> Option<f64> {
    n.emit.map(|e| e.gravity).or(n.rope.as_ref().map(|r| r.gravity)).or(n.fluid.map(|f| f.gravity)).or(n.roll.map(|r| r.gravity)).or(n.soft.map(|s| s.gravity))
}

/// How much wind node `n` catches: its `sail`, or 1.
pub fn sail(n: &Node) -> f64 {
    n.sail.unwrap_or(1.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ShapeKind;

    #[test]
    fn an_even_wind_blows_steadily_and_a_gusty_one_swells() {
        let n = Node::new("leaf", ShapeKind::Rect);
        let mut w = World { wind: Vec2::new(200.0, 0.0), ..World::default() };
        let even: Vec<Vec2> = (0..100).map(|k| World { time: k as f64 * 0.1, ..w }.blow(&n, Point::ZERO)).collect();
        assert!(even.iter().all(|v| (v.x - 200.0).abs() < 1e-9 && v.y.abs() < 1e-9));
        w.gust = 1.0;
        let xs: Vec<f64> = (0..100).map(|k| World { time: k as f64 * 0.1, ..w }.blow(&n, Point::ZERO).x).collect();
        let (lo, hi) = (xs.iter().cloned().fold(f64::MAX, f64::min), xs.iter().cloned().fold(f64::MIN, f64::max));
        assert!(lo < 60.0 && hi > 300.0, "{lo} {hi}");
        let mean = xs.iter().sum::<f64>() / xs.len() as f64;
        assert!((mean - 200.0).abs() < 80.0, "{mean}");
    }

    /// Where the particles of a still burst are after a second in `world`, for a spark of `weight`.
    fn burst(world: World, weight: Option<f64>, sail: Option<f64>) -> Vec<Point> {
        use crate::{Animation, Artboard, Document, Emitter, SimCache};
        let mut d = Document::new(Artboard::default());
        let mut n = Node::new("spark", ShapeKind::Ellipse);
        (n.x, n.y, n.width, n.height) = (200.0, 200.0, 4.0, 4.0);
        n.emit = Some(Emitter { rate: 0.0, burst: 10.0, life: 5.0, speed: 0.0, ..Emitter::default() });
        (n.weight, n.sail) = (weight, sail);
        d.nodes.push(n);
        d.world = world;
        let a = Animation { name: "Go".into(), duration: 3.0, looping: false, tracks: Vec::new(), events: Vec::new() };
        let p = a.pose_with(&d, 1.0, &mut SimCache::default());
        p.nodes[0].particles.as_ref().unwrap().iter().map(|(t, _)| *t * Point::ZERO).collect()
    }

    #[test]
    fn particles_fall_the_way_the_world_pulls_and_blow_downwind() {
        // Particles kept no gravity of their own: without a weight they hang still.
        assert!(burst(World::default(), None, None).iter().all(|q| (q.x - 200.0).abs() < 1e-6 && (q.y - 200.0).abs() < 1e-6));
        // Sideways gravity: after a second they have fallen half of 400 to the right.
        let side = World { gravity: Vec2::new(400.0, 0.0), ..World::default() };
        for q in burst(side, Some(1.0), None) {
            assert!((q.x - 400.0).abs() < 5.0 && (q.y - 200.0).abs() < 1e-6, "{q:?}");
        }
        // Wind blows up-left, twice as hard on a big sail; no sail, no push.
        let wind = World { wind: Vec2::new(-100.0, -100.0), ..World::default() };
        let q = burst(wind, Some(0.0), Some(2.0))[0];
        assert!((q.x - 100.0).abs() < 3.0 && (q.y - 100.0).abs() < 3.0, "{q:?}");
        let q = burst(wind, Some(0.0), Some(0.0))[0];
        assert_eq!(q, Point::new(200.0, 200.0));
    }

    #[test]
    fn weight_and_sail_scale_the_world() {
        let mut n = Node::new("rock", ShapeKind::Ellipse);
        let w = World { gravity: Vec2::new(300.0, 0.0), wind: Vec2::new(0.0, -100.0), ..World::default() };
        assert_eq!(w.pull(&n, GRAVITY), Vec2::new(300.0, 0.0));
        assert_eq!(w.pull(&n, 0.0), Vec2::ZERO, "a soft body without weight floats as before");
        n.weight = Some(2.0);
        n.sail = Some(0.5);
        assert_eq!(w.pull(&n, 0.0), Vec2::new(600.0, 0.0));
        assert_eq!(w.blow(&n, Point::ZERO), Vec2::new(0.0, -50.0));
    }
}
