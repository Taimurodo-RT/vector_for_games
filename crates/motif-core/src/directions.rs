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
