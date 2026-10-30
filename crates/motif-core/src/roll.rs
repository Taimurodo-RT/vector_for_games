//! Physics on a ball (`roll`): an object sitting on a ball (`on`) is held to its surface and
//! rolls over it in 3D towards the ball's bottom, turning as it rolls; tipping the ball
//! (`pitch`) sends it rolling. A turning ball carries what grips it
//! (`friction`), and objects rolling on the same ball bump into each other. The result is
//! written back as latitude and longitude, so it is drawn, hidden and wrapped like any orbit.

use crate::orbit::{Ball, Turn, V3, frame};
use crate::{Document, SIM_FPS, ShapeKind};

/// Settings of an object rolling on its ball (`roll 0.3 0.05`).
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Roll {
    /// How much a turning ball carries the object along, 0..1: 1 turns it with the ball.
    pub friction: f64,
    /// How fast rolling dies out, 0..1 per frame.
    pub damp: f64,
    /// Acceleration towards the ball's bottom pole, artboard units per second squared, when the
    /// object sets no `weight`.
    pub gravity: f64,
}

impl Default for Roll {
    fn default() -> Self {
        Roll { friction: 0.3, damp: 0.05, gravity: 900.0 }
    }
}

/// How much of their closing speed two objects keep when they bump, bounced back.
const BOUNCE: f64 = 0.3;

/// An object rolling on a ball. Directions are on the unit sphere as the viewer sees it: x
/// right, y down, z towards the viewer.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Roller {
    node: usize,
    ball: usize,
    roll: Roll,
    /// Where it is, and how fast it moves (unit-sphere lengths per second).
    q: V3,
    v: V3,
    /// Turn from rolling, degrees.
    spin: f64,
    /// The ball's turn a step ago, to tell how its surface moves.
    turn: Turn,
}

/// The ball's radius at the height of node `i`, and the object's own radius.
fn sizes(doc: &Document, i: usize, s: usize) -> (f64, f64) {
    let b = Ball::of(doc, s);
    (((b.rx + b.ry) / 2.0 + doc.nodes[i].alt).max(1e-6), radius(doc, i))
}

/// How far node `i` reaches from its origin: half its size, or for a group, as far as its
/// children reach.
fn radius(doc: &Document, i: usize) -> f64 {
    let n = &doc.nodes[i];
    if !n.kind.is_container() {
        return n.width.abs().max(n.height.abs()) / 2.0 * n.scale_x.abs().max(n.scale_y.abs());
    }
    let scale = n.scale_x.abs().max(n.scale_y.abs());
    (0..doc.nodes.len())
        .filter(|&c| doc.nodes[c].parent == Some(i))
        .map(|c| (doc.nodes[c].x.hypot(doc.nodes[c].y) + radius(doc, c)) * scale)
        .fold(0.0, f64::max)
}

fn ball_of(doc: &Document, i: usize) -> Option<usize> {
    doc.nodes[i].on.as_deref().and_then(|id| doc.index_of(id)).filter(|&s| s != i && doc.nodes[s].kind != ShapeKind::Group)
}

/// Objects of `keys` that roll, resting where their keys put them.
pub(crate) fn rollers(keys: &Document) -> Vec<Roller> {
    (0..keys.nodes.len())
        .filter_map(|i| {
            let roll = keys.nodes[i].roll?;
            let s = ball_of(keys, i)?;
            let turn = Ball::of(keys, s).turn;
            let (at, _, _) = frame(keys.nodes[i].lat, keys.nodes[i].lon);
            Some(Roller { node: i, ball: s, roll, q: turn.apply(at), v: V3::ZERO, spin: 0.0, turn })
        })
        .collect()
}

/// Moves rollers on by `dt` seconds on the balls of `pose`, in `steps` equal steps.
pub(crate) fn step_rollers(rollers: &mut [Roller], pose: &Document, dt: f64, steps: usize) {
    if rollers.is_empty() {
        return;
    }
    let h = dt / steps as f64;
    let frames = h * SIM_FPS;
    let turns: Vec<Turn> = rollers.iter().map(|r| Ball::of(pose, r.ball).turn).collect();
    // Gravity pulls the way the scene's gravity points as the ball sits untipped, so it turns
    // with the ball: tip the ball and things roll the other way. Wind blows as seen.
    let down: Vec<V3> = rollers
        .iter()
        .zip(&turns)
        .map(|(r, t)| {
            let n = &pose.nodes[r.node];
            let g = pose.world.pull(n, r.roll.gravity);
            let w = pose.push(r.node, pose.world_transform(r.node) * kurbo::Point::ZERO);
            t.apply(V3::new(g.x, g.y, 0.0)).add(V3::new(w.x, w.y, 0.0))
        })
        .collect();
    let size: Vec<(f64, f64)> = rollers.iter().map(|r| sizes(pose, r.node, r.ball)).collect();
    // How the surface under each one moves this frame, as the ball turns.
    let surface: Vec<V3> = rollers.iter().zip(&turns).map(|(r, t)| t.apply(r.turn.undo(r.q)).sub(r.q).mul(1.0 / dt)).collect();
    for _ in 0..steps {
        for (k, r) in rollers.iter_mut().enumerate() {
            let (big, own) = size[k];
            // Its own speed is rolling over the surface; a turning ball also carries it along
            // as much as it grips.
            let carried = surface[k].mul(r.roll.friction.clamp(0.0, 1.0));
            r.v = r.v.add(down[k].mul(h / big));
            r.v = r.v.sub(r.q.mul(r.v.dot(r.q))).mul((1.0 - r.roll.damp.clamp(0.0, 1.0)).powf(frames));
            // Rolling over the surface turns it; seen from the front only the turn about the
            // view axis shows.
            if own > 1e-9 {
                r.spin += r.q.cross(r.v).z * big / own * h * 180.0 / std::f64::consts::PI;
            }
            r.q = r.q.add(r.v.add(carried).mul(h)).unit();
            r.v = r.v.sub(r.q.mul(r.v.dot(r.q)));
        }
        // Objects on the same ball keep apart by their sizes, measured along the surface.
        for a in 0..rollers.len() {
            for b in a + 1..rollers.len() {
                if rollers[a].ball != rollers[b].ball {
                    continue;
                }
                let (qa, qb) = (rollers[a].q, rollers[b].q);
                let gap = qa.dot(qb).clamp(-1.0, 1.0).acos();
                let need = size[a].1 / size[a].0 + size[b].1 / size[b].0;
                if gap >= need {
                    continue;
                }
                let away_a = qa.sub(qb.mul(qa.dot(qb))).unit();
                let away_b = qb.sub(qa.mul(qa.dot(qb))).unit();
                let push = (need - gap) / 2.0;
                rollers[a].q = qa.add(away_a.mul(push)).unit();
                rollers[b].q = qb.add(away_b.mul(push)).unit();
                let closing = rollers[a].v.dot(away_a) + rollers[b].v.dot(away_b);
                if closing < 0.0 {
                    let j = -(1.0 + BOUNCE) * closing / 2.0;
                    rollers[a].v = rollers[a].v.add(away_a.mul(j));
                    rollers[b].v = rollers[b].v.add(away_b.mul(j));
                }
            }
        }
    }
    for (r, t) in rollers.iter_mut().zip(turns) {
        r.turn = t;
    }
}

/// Writes where each roller is as its latitude and longitude on its ball as `doc` turns it,
/// and its turn from rolling.
pub(crate) fn apply_rollers(rollers: &[Roller], doc: &mut Document) {
    for r in rollers {
        if r.node >= doc.nodes.len() || r.ball >= doc.nodes.len() {
            continue;
        }
        let u = Ball::of(doc, r.ball).turn.undo(r.q);
        let n = &mut doc.nodes[r.node];
        n.lat = (-u.y).clamp(-1.0, 1.0).asin().to_degrees();
        n.lon = u.x.atan2(u.z).to_degrees();
        n.swing = r.spin;
    }
}

impl crate::sim::Carry for Roller {
    fn node(&self) -> usize {
        self.node
    }
    fn carry(&mut self, old: &Self) {
        (self.q, self.v, self.spin, self.turn) = (old.q, old.v, old.spin, old.turn);
    }
}

#[cfg(test)]
mod tests {
    use crate::{Animation, Artboard, Document, Ease, Keyframe, Node, Prop, Roll, ShapeKind, SimCache, Track, Value};
    use kurbo::Point;

    /// A ball of radius 100 at (200, 200) and a marble of radius 10 on it.
    fn globe(lat: f64, lon: f64, roll: Roll) -> Document {
        let mut d = Document::new(Artboard::default());
        let mut ball = Node::new("ball", ShapeKind::Ellipse);
        (ball.x, ball.y, ball.width, ball.height) = (200.0, 200.0, 200.0, 200.0);
        let mut m = Node::new("marble", ShapeKind::Ellipse);
        (m.width, m.height, m.lat, m.lon) = (20.0, 20.0, lat, lon);
        m.on = Some("ball".into());
        m.roll = Some(roll);
        d.nodes = vec![ball, m];
        d
    }

    fn at(d: &Document, a: &Animation, t: f64, i: usize) -> (Point, f64, bool) {
        let p = a.pose_with(d, t, &mut SimCache::default());
        (p.world_transform(i) * Point::ZERO, p.nodes[i].swing, p.nodes[i].behind.is_some())
    }

    fn still() -> Animation {
        Animation { name: "Go".into(), duration: 10.0, looping: false, tracks: Vec::new(), events: Vec::new() }
    }

    #[test]
    fn a_marble_rolls_down_the_ball_and_turns() {
        let d = globe(60.0, 20.0, Roll { damp: 0.1, ..Roll::default() });
        let (start, _, _) = at(&d, &still(), 0.0, 1);
        assert!(start.y < 130.0, "{start:?}");
        let (end, spin, behind) = at(&d, &still(), 4.0, 1);
        // It settles at the bottom of the ball as seen, rolling clockwise on the right side.
        assert!((end.y - 300.0).abs() < 3.0 && (end.x - 200.0).abs() < 8.0, "{end:?}");
        assert!(!behind);
        assert!(spin > 90.0, "{spin}");
    }

    #[test]
    fn a_turning_ball_carries_what_grips_it() {
        let key = |time, v| Keyframe { time, value: Value::Num(v), ease: Ease::Linear };
        let a = Animation {
            name: "Spin".into(),
            duration: 2.0,
            looping: false,
            tracks: vec![Track { node: "ball".into(), prop: Prop::Yaw, keys: vec![key(0.0, 0.0), key(1.0, 90.0), key(2.0, 90.0)] }],
            events: Vec::new(),
        };
        let grip = globe(0.0, 0.0, Roll { friction: 1.0, gravity: 0.0, ..Roll::default() });
        let (p, _, _) = at(&grip, &a, 2.0, 1);
        assert!((p.x - 300.0).abs() < 4.0, "carried to the right edge: {p:?}");
        let slick = globe(0.0, 0.0, Roll { friction: 0.0, gravity: 0.0, ..Roll::default() });
        let (p, _, _) = at(&slick, &a, 2.0, 1);
        assert!((p.x - 200.0).abs() < 1.0, "stays in front as the ball turns under it: {p:?}");
    }

    #[test]
    fn marbles_on_one_ball_bump_apart() {
        let mut d = globe(0.0, 0.0, Roll { gravity: 0.0, ..Roll::default() });
        let mut other = d.nodes[1].clone();
        other.id = "other".into();
        other.lon = 3.0;
        d.nodes.push(other);
        let p = still().pose_with(&d, 1.0, &mut SimCache::default());
        let (a, b) = (p.world_transform(1) * Point::ZERO, p.world_transform(2) * Point::ZERO);
        assert!((a - b).hypot() > 19.0, "{a:?} {b:?}");
    }
}
