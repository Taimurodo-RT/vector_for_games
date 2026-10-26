//! The simulation behind physical animation: springs, jiggling bones, squash, ropes and cloth,
//! fluids, particles and soft bodies, stepped together in fixed steps. An animation simulates from its
//! start, so the same time always gives the same pose and export bakes it into frames; a state
//! machine steps it as it plays.

use crate::fluid::{self, Pool};
use crate::particles::{self, Spray};
use crate::roll::{self, Roller};
use crate::rope::{self, Strand};
use crate::soft::{self, Body};
use crate::spring::{self, Sprung, Tip};
use crate::squash::{self, Stretch};
use crate::{Animation, Document};

/// Simulation frames per second; each frame is split into `SUBSTEPS` steps.
pub const SIM_FPS: f64 = 60.0;
const SUBSTEPS: usize = 10;

/// Everything simulated at one moment.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SimState {
    values: Vec<Sprung>,
    rollers: Vec<Roller>,
    tips: Vec<Tip>,
    stretches: Vec<Stretch>,
    strands: Vec<Strand>,
    pools: Vec<Pool>,
    sprays: Vec<Spray>,
    bodies: Vec<Body>,
    /// Seconds simulated, which drives the wind's gusts.
    time: f64,
    /// Nodes in the document it was started from: a different count means the scene changed.
    nodes: usize,
}

impl SimState {
    /// A simulation at rest in `keys`, the document as the keys put it (before `rig`).
    pub fn new(keys: &Document) -> SimState {
        let values = spring::sprung(keys);
        let mut pose = keys.clone();
        pose.place_orbits();
        pose.solve_ik();
        let tips = spring::tips(&pose);
        let stretches = squash::stretches(&pose);
        pose.skin_all();
        let strands = rope::strands(&pose);
        rope::apply_strands(&strands, &mut pose);
        SimState {
            values,
            rollers: roll::rollers(keys),
            tips,
            stretches,
            strands,
            pools: fluid::pools(&pose),
            sprays: particles::sprays(&pose),
            bodies: soft::bodies(&pose),
            time: 0.0,
            nodes: keys.nodes.len(),
        }
    }

    /// True when the simulation was started from a document shaped like `doc`.
    pub fn fits(&self, doc: &Document) -> bool {
        self.nodes == doc.nodes.len()
    }

    /// Moves on by `dt` seconds (at most one frame) towards `keys`.
    pub fn step(&mut self, keys: &Document, dt: f64) {
        let mut pose = keys.clone();
        pose.world.time = self.time;
        if keys.has_fields() {
            let mut rigged = keys.clone();
            rigged.rig();
            pose.fields = crate::field::sources(&rigged);
        }
        self.time += dt;
        spring::step_values(&mut self.values, keys, dt, SUBSTEPS);
        spring::apply_values(&self.values, &mut pose);
        roll::step_rollers(&mut self.rollers, &pose, dt, SUBSTEPS);
        roll::apply_rollers(&self.rollers, &mut pose);
        pose.place_orbits();
        pose.solve_ik();
        spring::step_tips(&mut self.tips, &mut pose, dt, SUBSTEPS);
        squash::step_stretches(&mut self.stretches, &mut pose, dt, SUBSTEPS);
        pose.skin_all();
        rope::step_strands(&mut self.strands, &pose, dt, SUBSTEPS);
        rope::apply_strands(&self.strands, &mut pose);
        fluid::step_pools(&mut self.pools, &pose, dt);
        particles::step_sprays(&mut self.sprays, &pose, dt);
        soft::step_bodies(&mut self.bodies, &pose, dt, SUBSTEPS);
    }

    /// Moves on by any `dt`, one frame at a time.
    pub fn advance(&mut self, keys: &Document, mut dt: f64) {
        while dt > 1e-9 {
            let h = dt.min(1.0 / SIM_FPS);
            self.step(keys, h);
            dt -= h;
        }
    }

    /// `keys` posed as the simulation has it: sprung values, bones (IK, then jiggle), squash,
    /// skins and soft bodies.
    pub fn apply(&self, keys: &Document) -> Document {
        let mut out = keys.clone();
        spring::apply_values(&self.values, &mut out);
        roll::apply_rollers(&self.rollers, &mut out);
        out.place_orbits();
        out.solve_ik();
        spring::apply_tips(&self.tips, &mut out);
        squash::apply_stretches(&self.stretches, &mut out);
        out.skin_all();
        rope::apply_strands(&self.strands, &mut out);
        fluid::apply_pools(&self.pools, &mut out);
        particles::apply_sprays(&self.sprays, &mut out);
        soft::apply_bodies(&self.bodies, &mut out);
        out
    }
}

/// Simulated states of one animation, one per frame, so scrubbing does not start over.
#[derive(Clone, Debug, Default)]
pub struct SimCache {
    states: Vec<SimState>,
}

impl SimCache {
    pub fn clear(&mut self) {
        self.states.clear();
    }
}

impl Document {
    /// True when something is simulated (springs, jiggling bones, squash, ropes and cloth,
    /// particles, soft bodies), so posing
    /// needs a simulation.
    pub fn has_sim(&self) -> bool {
        self.nodes.iter().any(|n| {
            n.soft.is_some()
                || n.spring.is_some()
                || n.jiggle.is_some()
                || n.squash.is_some()
                || n.emit.is_some()
                || n.rope.is_some()
                || n.fluid.is_some()
                || n.roll.is_some()
        })
    }
}

impl Animation {
    /// The document at `t` seconds, simulated from the start of the animation. `cache` keeps
    /// the states of frames already simulated; clear it when the scene changes.
    pub fn pose_with(&self, doc: &Document, t: f64, cache: &mut SimCache) -> Document {
        let keys = self.keys_at(doc, t);
        if !doc.has_sim() {
            let mut out = keys;
            out.rig();
            return out;
        }
        let t = self.local_time(t);
        let frame = (t * SIM_FPS).floor() as usize;
        if cache.states.is_empty() {
            cache.states.push(SimState::new(&self.keys_at(doc, 0.0)));
        }
        while cache.states.len() <= frame {
            let done = cache.states.len();
            let mut s = cache.states[done - 1].clone();
            // Each frame is stepped towards the keys of the time it ends at.
            s.step(&self.keys_at(doc, done as f64 / SIM_FPS), 1.0 / SIM_FPS);
            cache.states.push(s);
        }
        let mut s = cache.states[frame].clone();
        let rest = t - frame as f64 / SIM_FPS;
        if rest > 1e-9 {
            s.step(&keys, rest);
        }
        s.apply(&keys)
    }
}

#[cfg(test)]
mod tests {
    use crate::{Animation, Artboard, Document, Ease, Keyframe, Node, Prop, ShapeKind, SimCache, Spring, Track, Value};
    use kurbo::Point;

    fn key(time: f64, v: f64) -> Keyframe {
        Keyframe { time, value: Value::Num(v), ease: Ease::Hold }
    }

    /// A box that jumps from x 0 to x 100 at t 0.1, with a spring on it.
    fn jump(spring: Option<Spring>) -> (Document, Animation) {
        let mut d = Document::new(Artboard::default());
        let mut b = Node::new("b", ShapeKind::Rect);
        b.spring = spring;
        d.nodes.push(b);
        let a = Animation {
            name: "Go".into(),
            duration: 3.0,
            looping: false,
            tracks: vec![Track { node: "b".into(), prop: Prop::X, keys: vec![key(0.0, 0.0), key(0.1, 100.0)] }],
            events: Vec::new(),
        };
        (d, a)
    }

    #[test]
    fn a_spring_overshoots_and_settles() {
        let (d, a) = jump(Some(Spring { freq: 2.0, damp: 0.2 }));
        let mut cache = SimCache::default();
        let xs: Vec<f64> = (0..=180).map(|f| a.pose_with(&d, f as f64 / 60.0, &mut cache).nodes[0].x).collect();
        assert_eq!(xs[0], 0.0);
        assert!(xs[8] < 20.0, "it does not jump at once: {}", xs[8]);
        let peak = xs.iter().cloned().fold(f64::MIN, f64::max);
        assert!(peak > 130.0, "overshoots: {peak}");
        assert!((xs[180] - 100.0).abs() < 3.0, "settles: {}", xs[180]);
        // The cache gives the same answer as starting over.
        assert_eq!(a.pose_with(&d, 1.25, &mut SimCache::default()).nodes[0].x, a.pose_with(&d, 1.25, &mut cache).nodes[0].x);
        // Without the spring the box follows the keys exactly.
        let (d, a) = jump(None);
        assert_eq!(a.pose(&d, 0.5).nodes[0].x, 100.0);
    }

    #[test]
    fn a_damped_spring_does_not_overshoot() {
        let (d, a) = jump(Some(Spring { freq: 3.0, damp: 1.0 }));
        let mut cache = SimCache::default();
        let peak = (0..=180).map(|f| a.pose_with(&d, f as f64 / 60.0, &mut cache).nodes[0].x).fold(f64::MIN, f64::max);
        assert!(peak <= 100.0 + 1e-6, "{peak}");
    }

    /// A tail: a root bone keyed to swing, with a jiggling bone hanging from it.
    fn tail(jiggle: bool) -> (Document, Animation) {
        let mut d = Document::new(Artboard::default());
        let mut root = Node::new("root", ShapeKind::Bone);
        (root.x, root.y, root.width) = (100.0, 100.0, 50.0);
        let mut end = Node::new("end", ShapeKind::Bone);
        (end.x, end.width, end.parent) = (50.0, 50.0, Some(0));
        end.jiggle = jiggle.then(Spring::default);
        d.nodes = vec![root, end];
        let k = |time, v| Keyframe { time, value: Value::Num(v), ease: Ease::Linear };
        let a = Animation {
            name: "Wag".into(),
            duration: 2.0,
            looping: false,
            tracks: vec![Track { node: "root".into(), prop: Prop::Rotation, keys: vec![k(0.0, 0.0), k(0.5, 60.0)] }],
            events: Vec::new(),
        };
        (d, a)
    }

    fn tip(d: &Document) -> Point {
        d.world_transform(1) * d.nodes[1].bone_tip()
    }

    #[test]
    fn a_jiggling_bone_lags_and_swings_back() {
        let (d, a) = tail(true);
        let mut cache = SimCache::default();
        let stiff = tail(false);
        // While the root turns down, the jiggling end trails behind (less turned).
        let mid = a.pose_with(&d, 0.4, &mut cache);
        assert!(mid.nodes[1].rotation < -3.0, "trails: {}", mid.nodes[1].rotation);
        // It keeps its length.
        let len = (tip(&mid) - mid.world_transform(1) * Point::ZERO).hypot();
        assert!((len - 50.0).abs() < 1e-6, "{len}");
        // After the root stops, the end swings past and comes back to rest.
        let swing = (31..80).map(|f| a.pose_with(&d, f as f64 / 60.0, &mut cache).nodes[1].rotation).fold(f64::MIN, f64::max);
        assert!(swing > 3.0, "swings past: {swing}");
        let end = a.pose_with(&d, 2.0, &mut cache);
        assert!(end.nodes[1].rotation.abs() < 1.0, "comes to rest: {}", end.nodes[1].rotation);
        assert!((tip(&end) - tip(&stiff.1.pose(&stiff.0, 2.0))).hypot() < 1.0);
    }

    /// A ball keyed to fly right fast and stop dead at t 0.5, with squash on.
    fn throw() -> (Document, Animation) {
        let mut d = Document::new(Artboard::default());
        let mut b = Node::new("b", ShapeKind::Ellipse);
        (b.width, b.height) = (40.0, 40.0);
        b.squash = Some(crate::Squash::default());
        d.nodes.push(b);
        let k = |time, v| Keyframe { time, value: Value::Num(v), ease: Ease::Linear };
        let a = Animation {
            name: "Throw".into(),
            duration: 2.0,
            looping: false,
            tracks: vec![Track { node: "b".into(), prop: Prop::X, keys: vec![k(0.0, 0.0), k(0.5, 400.0)] }],
            events: Vec::new(),
        };
        (d, a)
    }

    /// Width and height of the ball as drawn on the artboard.
    fn size(d: &Document) -> (f64, f64) {
        use kurbo::Shape;
        let b = (d.world_transform(0) * d.nodes[0].path().unwrap()).bounding_box();
        (b.width(), b.height())
    }

    #[test]
    fn squash_stretches_in_flight_and_squashes_on_a_stop() {
        let (d, a) = throw();
        let mut cache = SimCache::default();
        // Flying at 800 units per second: longer along the motion, thinner across, same area.
        let (w, h) = size(&a.pose_with(&d, 0.45, &mut cache));
        assert!(w > 50.0 && h < 32.0, "{w} {h}");
        assert!((w * h - 1600.0).abs() < 60.0, "keeps the area: {}", w * h);
        // Right after the stop it swings into a squash: short along the motion, wide across.
        let squashed = (31..60).map(|f| size(&a.pose_with(&d, f as f64 / 60.0, &mut cache))).any(|(w, h)| w < 36.0 && h > 44.0);
        assert!(squashed);
        // And settles back to round, staying where the keys put it.
        let end = a.pose_with(&d, 2.0, &mut cache);
        let (w, h) = size(&end);
        assert!((w - 40.0).abs() < 1.0 && (h - 40.0).abs() < 1.0, "{w} {h}");
        assert_eq!(end.nodes[0].x, 400.0);
    }
}
