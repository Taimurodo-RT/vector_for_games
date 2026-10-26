//! Force fields set on objects: a point that pulls everything simulated towards it, or pushes
//! it away (`pull 2000 range 300`), and a fan that blows along the object's own direction
//! (`fan 600 range 300 cone 40`). They move and turn with their object and can be keyed.

use crate::Document;
use kurbo::{Point, Vec2};

/// A pull or a fan.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Field {
    /// Acceleration near the object, artboard units per second squared; below 0 a pull pushes
    /// away and a fan sucks in.
    pub strength: f64,
    /// How far it reaches; it fades to nothing there.
    pub range: f64,
    /// Width of a fan's stream, degrees.
    pub cone: f64,
}

impl Field {
    pub fn pull() -> Field {
        Field { strength: 2000.0, range: 300.0, cone: 360.0 }
    }

    pub fn fan() -> Field {
        Field { strength: 600.0, range: 300.0, cone: 40.0 }
    }
}

/// Inside this distance a pull weakens towards its centre, so things passing through it are not
/// flung off at a huge speed.
const CORE: f64 = 12.0;

/// A field where it is on this frame.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Source {
    node: usize,
    at: Point,
    /// The way a fan blows: its object's x axis.
    dir: Vec2,
    field: Field,
    fan: bool,
}

impl Source {
    /// Acceleration at `p` of an object that catches `sail` of the wind.
    fn at(&self, p: Point, sail: f64) -> Vec2 {
        let f = self.field;
        let rel = p - self.at;
        let dist = rel.hypot();
        if dist >= f.range || dist < 1e-9 || f.range <= 0.0 {
            return Vec2::ZERO;
        }
        let fade = 1.0 - dist / f.range;
        if !self.fan {
            return -rel / dist * f.strength * fade * (dist / CORE).min(1.0);
        }
        let along = rel.dot(self.dir) / dist;
        let half = (f.cone.clamp(1.0, 360.0) / 2.0).to_radians();
        let angle = along.clamp(-1.0, 1.0).acos();
        if angle >= half {
            return Vec2::ZERO;
        }
        // The stream blows along the fan and is weaker at its edges.
        let edge = 1.0 - (angle / half).powi(2);
        self.dir * f.strength * fade * edge * sail
    }
}

/// Every pull and fan of `doc`, where they are now.
pub(crate) fn sources(doc: &Document) -> Vec<Source> {
    let mut out = Vec::new();
    for (i, n) in doc.nodes.iter().enumerate() {
        for (field, fan) in [(n.pull, false), (n.fan, true)] {
            let Some(field) = field else { continue };
            let w = doc.world_transform(i);
            let x = w * Point::new(1.0, 0.0) - w * Point::ZERO;
            let dir = if x.hypot() > 1e-9 { x.normalize() } else { Vec2::new(1.0, 0.0) };
            out.push(Source { node: i, at: w * Point::ZERO, dir, field, fan });
        }
    }
    out
}

impl Document {
    /// What pushes node `i` at `p` besides gravity: the wind and the fields of other objects.
    pub(crate) fn push(&self, i: usize, p: Point) -> Vec2 {
        let n = &self.nodes[i];
        let sail = crate::sail(n);
        self.world.blow(n, p) + self.fields.iter().filter(|s| s.node != i).map(|s| s.at(p, sail)).sum::<Vec2>()
    }

    /// True when some object pulls or blows.
    pub(crate) fn has_fields(&self) -> bool {
        self.nodes.iter().any(|n| n.pull.is_some() || n.fan.is_some())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Animation, Artboard, Emitter, Node, ShapeKind, SimCache};

    /// A still burst of sparks at (100, 200) and a field object at (300, 200).
    fn scene(pull: Option<Field>, fan: Option<Field>, rot: f64) -> Document {
        scene_with(pull, fan, rot, 0.0)
    }

    fn scene_with(pull: Option<Field>, fan: Option<Field>, rot: f64, drag: f64) -> Document {
        let mut d = Document::new(Artboard::default());
        d.world.gravity = Vec2::ZERO;
        let mut s = Node::new("spark", ShapeKind::Ellipse);
        (s.x, s.y, s.width, s.height) = (100.0, 200.0, 4.0, 4.0);
        s.emit = Some(Emitter { rate: 0.0, burst: 1.0, life: 9.0, speed: 0.0, drag, ..Emitter::default() });
        let mut f = Node::new("field", ShapeKind::Ellipse);
        (f.x, f.y, f.rotation) = (300.0, 200.0, rot);
        (f.pull, f.fan) = (pull, fan);
        d.nodes = vec![s, f];
        d
    }

    fn spark(d: &Document, t: f64) -> Point {
        let a = Animation { name: "Go".into(), duration: 9.0, looping: false, tracks: Vec::new(), events: Vec::new() };
        let p = a.pose_with(d, t, &mut SimCache::default());
        p.nodes[0].particles.as_ref().unwrap()[0].0 * Point::ZERO
    }

    #[test]
    fn a_pull_draws_things_in_and_a_push_drives_them_off() {
        let d = scene(Some(Field::pull()), None, 0.0);
        let near = spark(&d, 0.5);
        assert!(near.x > 120.0 && (near.y - 200.0).abs() < 1e-6, "{near:?}");
        // Without drag it swings through the centre and back like a pendulum; with drag it
        // settles there.
        let swing: Vec<f64> = (0..60).map(|k| spark(&d, k as f64 * 0.1).x).collect();
        assert!(swing.iter().cloned().fold(0.0, f64::max) > 450.0, "{swing:?}");
        let held = scene_with(Some(Field::pull()), None, 0.0, 0.8);
        let late: Vec<f64> = (50..60).map(|k| spark(&held, k as f64 * 0.1).x).collect();
        assert!(late.iter().all(|x| (x - 300.0).abs() < 5.0), "{late:?}");
        let away = spark(&scene(Some(Field { strength: -2000.0, ..Field::pull() }), None, 0.0), 0.5);
        assert!(away.x < 80.0, "{away:?}");
        // Out of range, nothing happens.
        let far = spark(&scene(Some(Field { range: 150.0, ..Field::pull() }), None, 0.0), 1.0);
        assert_eq!(far, Point::new(100.0, 200.0));
    }

    #[test]
    fn a_fan_blows_only_along_its_stream() {
        // Pointing left, at the spark: it is blown away to the left.
        let blown = spark(&scene(None, Some(Field::fan()), 180.0), 0.5);
        assert!(blown.x < 80.0 && (blown.y - 200.0).abs() < 1e-6, "{blown:?}");
        // Pointing right, away from it: nothing.
        assert_eq!(spark(&scene(None, Some(Field::fan()), 0.0), 0.5), Point::new(100.0, 200.0));
        // Pointing up: the spark is out of the stream.
        assert_eq!(spark(&scene(None, Some(Field::fan()), -90.0), 0.5), Point::new(100.0, 200.0));
    }
}
