//! Facing directions for top-down and isometric games (`directions 8 turn head body mirror`):
//! one character is shot from every side by turning the listed objects (balls and 3D bodies, by
//! their `yaw`), and an animation written as `Walk_up` replaces `Walk` for that side alone.
//!
//! Direction 0 faces the viewer (down the screen); the rest go round clockwise as seen from
//! above: down, down_right, right, up_right, up, up_left, left, down_left. A turn of `yaw` 90
//! makes a ball's front face right.

use crate::Document;

const EIGHT: [&str; 8] = ["down", "down_right", "right", "up_right", "up", "up_left", "left", "down_left"];
const FOUR: [&str; 4] = ["down", "right", "up", "left"];

#[derive(Clone, Debug, PartialEq)]
pub struct Directions {
    /// 4 or 8.
    pub count: usize,
    /// Objects turned to face each direction (by `yaw`), with what is on them.
    pub turn: Vec<String>,
    /// The left-facing sides are the right-facing ones flipped: not drawn, flipped in the game.
    pub mirror: bool,
}

impl Directions {
    /// The name of every direction, from facing the viewer round clockwise.
    pub fn names(&self) -> &'static [&'static str] {
        if self.count == 4 { &FOUR } else { &EIGHT }
    }

    /// The turn (`yaw`, degrees) direction `k` adds to the turned objects.
    pub fn yaw(&self, k: usize) -> f64 {
        let a = k as f64 * 360.0 / self.count as f64;
        if a > 180.0 { a - 360.0 } else { a }
    }

    /// The direction `k` is drawn as when it faces left and the sides are mirrored.
    pub fn mirror_of(&self, k: usize) -> Option<usize> {
        let a = self.yaw(k);
        (self.mirror && a < 0.0).then(|| (self.count - k) % self.count)
    }

    /// The directions that are drawn: all of them, or without the mirrored left ones.
    pub fn drawn(&self) -> Vec<usize> {
        (0..self.count).filter(|&k| self.mirror_of(k).is_none()).collect()
    }

    /// The side direction `k` is drawn from, and whether it is shown flipped: a left side of
    /// mirrored directions is its right side turned over.
    pub fn shot(&self, k: usize) -> (usize, bool) {
        let k = k % self.count;
        self.mirror_of(k).map_or((k, false), |d| (d, true))
    }

    /// The direction nearest to looking along `(x, y)` on the screen, `y` down: `(0, 1)` is down,
    /// `(1, 0)` right. None for a zero vector.
    pub fn nearest(&self, x: f64, y: f64) -> Option<usize> {
        if !(x.is_finite() && y.is_finite()) || (x == 0.0 && y == 0.0) {
            return None;
        }
        let turns = x.atan2(y) / std::f64::consts::TAU;
        Some(((turns * self.count as f64).round() as i64).rem_euclid(self.count as i64) as usize)
    }

    /// The direction named `name`.
    pub fn find(&self, name: &str) -> Option<usize> {
        self.names().iter().position(|n| *n == name)
    }

    /// The animation that plays `anim` facing direction `k`: `anim_side` when the scene has it.
    pub fn anim_for<'a>(&self, doc: &'a Document, anim: &str, k: usize) -> Option<&'a crate::Animation> {
        doc.animation(&format!("{anim}_{}", self.names()[k])).or_else(|| doc.animation(anim))
    }

    /// Whether `anim` is one side of another animation (`Walk_up` beside `Walk`), not one of its own.
    pub fn is_side(&self, doc: &Document, anim: &str) -> bool {
        self.names().iter().any(|n| anim.strip_suffix(n).and_then(|b| b.strip_suffix('_')).is_some_and(|base| doc.animation(base).is_some()))
    }
}

impl Document {
    /// Turns the pose to face direction `k` of its `directions`: the turned objects get the
    /// direction's yaw on top of their own. Call [`Document::rig`] after it so what sits on
    /// them follows.
    pub fn face(&mut self, k: usize) {
        let Some(d) = self.directions.clone() else { return };
        let yaw = d.yaw(k % d.count);
        for id in &d.turn {
            if let Some(i) = self.index_of(id) {
                self.nodes[i].yaw += yaw;
            }
        }
    }
}

impl crate::Machine {
    /// The machine as it plays facing direction `k` of the scene's `directions`: every state
    /// plays its animation's side (`Walk_up` for `Walk`) where the scene has one. A mirrored left
    /// side plays the right one's. The states and transitions stay, so a player keeps its state
    /// and time when it turns.
    pub fn facing(&self, doc: &Document, k: usize) -> crate::Machine {
        let mut out = self.clone();
        if let Some(d) = &doc.directions {
            let (drawn, _) = d.shot(k);
            for s in &mut out.states {
                if let Some(a) = d.anim_for(doc, &s.anim, drawn) {
                    s.anim = a.name.clone();
                }
            }
        }
        out
    }
}

impl crate::Animation {
    /// [`crate::Animation::pose_with`] turned to face direction `face` of the scene's `directions`.
    pub fn pose_facing(&self, doc: &Document, t: f64, cache: &mut crate::SimCache, face: Option<usize>) -> Document {
        let mut out = self.pose_with(doc, t, cache);
        if let Some(k) = face.filter(|_| doc.directions.is_some()) {
            out.face(k);
            out.rig();
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Animation, Artboard, Node, ShapeKind};

    fn dirs(count: usize, mirror: bool) -> Directions {
        Directions { count, turn: vec!["head".into()], mirror }
    }

    #[test]
    fn eight_sides_go_round_and_left_ones_mirror_right_ones() {
        let d = dirs(8, true);
        assert_eq!(d.yaw(2), 90.0, "right");
        assert_eq!(d.yaw(4), 180.0, "up: the back");
        assert_eq!(d.yaw(6), -90.0, "left");
        assert_eq!(d.mirror_of(6), Some(2));
        assert_eq!(d.mirror_of(5), Some(3), "up_left is up_right flipped");
        assert_eq!(d.mirror_of(7), Some(1));
        assert_eq!(d.drawn(), vec![0, 1, 2, 3, 4]);
        assert_eq!(dirs(4, true).drawn(), vec![0, 1, 2]);
        assert_eq!(dirs(4, false).drawn(), vec![0, 1, 2, 3]);
        assert_eq!(dirs(4, false).names()[3], "left");
    }

    #[test]
    fn a_side_animation_replaces_its_base() {
        let mut doc = Document::new(Artboard::default());
        let anim = |name: &str| Animation { name: name.into(), duration: 1.0, looping: true, tracks: Vec::new(), events: Vec::new() };
        doc.animations = vec![anim("Walk"), anim("Walk_up"), anim("Jump_up_high")];
        let d = dirs(8, false);
        assert_eq!(d.anim_for(&doc, "Walk", 4).unwrap().name, "Walk_up");
        assert_eq!(d.anim_for(&doc, "Walk", 2).unwrap().name, "Walk");
        assert!(d.is_side(&doc, "Walk_up"));
        assert!(!d.is_side(&doc, "Walk") && !d.is_side(&doc, "Jump_up_high"));
    }

    #[test]
    fn a_look_picks_the_nearest_side_and_left_ones_are_shot_flipped() {
        let d = dirs(8, true);
        assert_eq!(d.nearest(0.0, 1.0), Some(0), "down");
        assert_eq!(d.nearest(1.0, 0.1), Some(2), "right");
        assert_eq!(d.nearest(0.0, -3.0), Some(4), "up");
        assert_eq!(d.nearest(-1.0, -1.0), Some(5), "up_left");
        assert_eq!(d.nearest(-1.0, 0.0), Some(6), "left");
        assert_eq!(d.nearest(0.0, 0.0), None);
        assert_eq!(dirs(4, true).nearest(0.9, -1.0), Some(2), "up wins over right when nearer");
        assert_eq!(d.shot(6), (2, true));
        assert_eq!(d.shot(5), (3, true));
        assert_eq!(d.shot(4), (4, false));
        assert_eq!(dirs(8, false).shot(6), (6, false));
    }

    #[test]
    fn a_machine_facing_a_side_plays_its_side_animations() {
        use crate::{Machine, State};
        let mut doc = Document::new(Artboard::default());
        let anim = |name: &str| Animation { name: name.into(), duration: 1.0, looping: true, tracks: Vec::new(), events: Vec::new() };
        doc.animations = vec![anim("Idle"), anim("Walk"), anim("Walk_up"), anim("Walk_right")];
        doc.directions = Some(dirs(8, true));
        let st = |n: &str| State { name: n.into(), anim: n.into(), pos: None };
        let m = Machine {
            name: "M".into(),
            inputs: Vec::new(),
            listeners: Vec::new(),
            states: vec![st("Idle"), st("Walk")],
            transitions: Vec::new(),
            any_pos: None,
        };
        let anims = |k: usize| m.facing(&doc, k).states.iter().map(|s| s.anim.clone()).collect::<Vec<_>>();
        assert_eq!(anims(4), ["Idle", "Walk_up"]);
        assert_eq!(anims(0), ["Idle", "Walk"]);
        assert_eq!(anims(6), ["Idle", "Walk_right"], "left is the right side flipped");
        assert_eq!(m.facing(&doc, 4).states[1].name, "Walk", "the state keeps its name");
    }

    #[test]
    fn facing_adds_yaw_to_the_turned_objects_only() {
        let mut doc = Document::new(Artboard::default());
        let mut head = Node::new("head", ShapeKind::Ellipse);
        head.yaw = 10.0;
        doc.nodes = vec![head, Node::new("floor", ShapeKind::Rect)];
        doc.directions = Some(dirs(8, false));
        doc.face(2);
        assert_eq!(doc.nodes[0].yaw, 100.0);
        assert_eq!(doc.nodes[1].yaw, 0.0);
    }
}
