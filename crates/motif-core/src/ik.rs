//! Inverse kinematics: a bone with `ik` turns itself and the bones above it so that its tip
//! reaches a target object. One bone aims, two bones bend like an arm or a leg (solved exactly),
//! longer chains are solved step by step.

use crate::{Document, ShapeKind};
use kurbo::{Affine, Point};
use std::f64::consts::PI;

/// Degrees in -180..180.
fn degrees(rad: f64) -> f64 {
    let d = rad.to_degrees();
    let d = (d + 180.0).rem_euclid(360.0) - 180.0;
    (d * 100.0).round() / 100.0
}

impl Document {
    /// Bends every `ik` chain towards its target. Runs after an animation has posed the
    /// document, so keys on a target move the whole limb.
    pub fn solve_ik(&mut self) {
        for i in 0..self.nodes.len() {
            let Some(ik) = self.nodes[i].ik.clone() else { continue };
            if self.nodes[i].kind != ShapeKind::Bone {
                continue;
            }
            let Some(t) = self.index_of(&ik.target) else { continue };
            let target = self.world_transform(t) * Point::ZERO;
            // This bone and the bones above it, root first.
            let mut chain = vec![i];
            while chain.len() < ik.chain.max(1) {
                match self.nodes[chain[chain.len() - 1]].parent {
                    Some(p) if self.nodes[p].kind == ShapeKind::Bone => chain.push(p),
                    _ => break,
                }
            }
            chain.reverse();
            match chain[..] {
                [b] => self.aim(b, target),
                [a, b] => self.two_bones(a, b, target, ik.flip),
                _ => self.ccd(&chain, target),
            }
        }
    }

    /// Solve in the parent's coordinates so a mirrored, scaled or skewed character
    /// uses the same joint angles as its untransformed rig. A collapsed parent has
    /// no inverse; leave its pose alone instead of introducing non-finite angles.
    fn parent_space(&self, i: usize) -> Option<Affine> {
        let parent = self.nodes[i].parent.map_or(Affine::IDENTITY, |p| self.world_transform(p));
        let det = parent.determinant();
        (det.is_finite() && det.abs() > 1e-12).then(|| parent.inverse())
    }

    fn tip(&self, i: usize) -> Point {
        self.world_transform(i) * self.nodes[i].bone_tip()
    }

    /// Turns bone `i` to point at `target`.
    fn aim(&mut self, i: usize, target: Point) {
        let Some(space) = self.parent_space(i) else { return };
        let target = space * target;
        let o = space * self.world_transform(i) * Point::ZERO;
        if (target - o).hypot() < 1e-9 {
            return;
        }
        self.nodes[i].rotation = degrees((target - o).atan2());
    }

    /// Bends bone `a` and its child `b` so that the tip of `b` reaches `target`, or points
    /// straight at it when it is out of reach.
    fn two_bones(&mut self, a: usize, b: usize, target: Point, flip: bool) {
        let Some(space) = self.parent_space(a) else { return };
        let local_target = space * target;
        let wa = space * self.world_transform(a);
        let o = wa * Point::ZERO;
        let (bx, by) = (self.nodes[b].x, self.nodes[b].y);
        let l1 = (wa * Point::new(bx, by) - o).hypot();
        let l2 = (space * self.tip(b) - space * self.world_transform(b) * Point::ZERO).hypot();
        if l1 < 1e-9 || l2 < 1e-9 {
            self.aim(a, target);
            self.aim(b, target);
            return;
        }
        let d = (local_target - o).hypot().clamp((l1 - l2).abs() + 1e-6, l1 + l2 - 1e-6);
        let at_root = ((l1 * l1 + d * d - l2 * l2) / (2.0 * l1 * d)).clamp(-1.0, 1.0).acos();
        let at_joint = ((l1 * l1 + l2 * l2 - d * d) / (2.0 * l1 * l2)).clamp(-1.0, 1.0).acos();
        let s = if flip { -1.0 } else { 1.0 };
        // The joint sits `at_root` off the line to the target; `off` is where the child starts on its parent.
        let off = by.atan2(bx);
        let to_joint = (local_target - o).atan2() - s * at_root;
        self.nodes[a].rotation = degrees(to_joint - off);
        self.nodes[b].rotation = degrees(s * (PI - at_joint) + off);
    }

    /// Cyclic coordinate descent: each bone in turn, from the tip up, turns its part of the
    /// chain towards the target.
    fn ccd(&mut self, chain: &[usize], target: Point) {
        let last = chain[chain.len() - 1];
        for _ in 0..16 {
            for &bone in chain.iter().rev() {
                let Some(space) = self.parent_space(bone) else { continue };
                let o = space * self.world_transform(bone) * Point::ZERO;
                let tip = space * self.tip(last);
                let turn = (space * target - o).atan2() - (tip - o).atan2();
                self.nodes[bone].rotation = degrees(self.nodes[bone].rotation.to_radians() + turn);
            }
            if (self.tip(last) - target).hypot() < 0.05 {
                break;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{Artboard, Document, Ik, Node, ShapeKind};
    use kurbo::Point;

    /// A chain of `n` bones of length 50 from (100, 100), the last with ik to a target at `t`.
    fn arm(n: usize, t: (f64, f64), flip: bool) -> Document {
        let mut d = Document::new(Artboard::default());
        for k in 0..n {
            let mut b = Node::new(format!("b{k}"), ShapeKind::Bone);
            b.width = 50.0;
            if k == 0 {
                (b.x, b.y) = (100.0, 100.0);
            } else {
                b.x = 50.0;
                b.parent = Some(k - 1);
            }
            d.nodes.push(b);
        }
        let mut target = Node::new("t", ShapeKind::Group);
        (target.x, target.y) = t;
        d.nodes.push(target);
        d.nodes[n - 1].ik = Some(Ik { target: "t".into(), chain: n, flip });
        d.solve_ik();
        d
    }

    fn tip(d: &Document, i: usize) -> Point {
        d.world_transform(i) * d.nodes[i].bone_tip()
    }

    #[test]
    fn two_bones_reach_and_bend_either_way() {
        let d = arm(2, (160.0, 140.0), false);
        assert!((tip(&d, 1) - Point::new(160.0, 140.0)).hypot() < 0.05, "{:?}", tip(&d, 1));
        let flipped = arm(2, (160.0, 140.0), true);
        assert!((tip(&flipped, 1) - Point::new(160.0, 140.0)).hypot() < 0.05);
        // The elbow goes to the other side of the line to the target.
        let elbow = |d: &Document| d.world_transform(1) * Point::ZERO;
        let side = |p: Point| (160.0 - 100.0) * (p.y - 100.0) - (140.0 - 100.0) * (p.x - 100.0);
        assert!(side(elbow(&d)) * side(elbow(&flipped)) < 0.0);
        // Out of reach: the arm points straight at the target.
        let far = arm(2, (400.0, 100.0), false);
        assert!((tip(&far, 1) - Point::new(200.0, 100.0)).hypot() < 0.05);
    }

    #[test]
    fn one_bone_aims_and_long_chains_reach() {
        let d = arm(1, (100.0, 300.0), false);
        assert!((d.nodes[0].rotation - 90.0).abs() < 1e-6);
        let d = arm(4, (180.0, 60.0), false);
        assert!((tip(&d, 3) - Point::new(180.0, 60.0)).hypot() < 0.1, "{:?}", tip(&d, 3));
    }

    #[test]
    fn transformed_parent_preserves_one_two_and_long_chain_contacts() {
        for n in [1, 2, 4] {
            for sx in [-1.0, -1.3, 1.3] {
                for flip in [false, true] {
                    let mut d = arm(n, (130.0, 140.0), flip);
                    let mut parent = Node::new("character", ShapeKind::Group);
                    parent.x = 240.0;
                    parent.y = -80.0;
                    parent.rotation = 31.0;
                    parent.scale_x = sx;
                    parent.scale_y = 0.8;
                    parent.skew_x = 12.0;
                    d.nodes[0].parent = Some(d.nodes.len());
                    d.nodes[n].parent = Some(d.nodes.len());
                    d.nodes.push(parent);
                    // Start from a different pose; do not just re-solve an already correct arm.
                    for bone in &mut d.nodes[..n] {
                        bone.rotation = 0.0;
                    }
                    d.solve_ik();
                    let target = d.world_transform(n) * Point::ZERO;
                    assert!(tip(&d, n - 1).distance(target) < 0.1, "{n} bones / scale {sx} / flip {flip}");
                }
            }
        }
    }

    #[test]
    fn collapsed_parent_leaves_finite_joint_angles() {
        for n in [1, 2, 4] {
            let mut d = arm(n, (130.0, 140.0), false);
            let mut parent = Node::new("collapsed", ShapeKind::Group);
            parent.scale_x = 0.0;
            d.nodes[0].parent = Some(d.nodes.len());
            d.nodes[n].parent = Some(d.nodes.len());
            d.nodes.push(parent);
            let before: Vec<_> = d.nodes.iter().map(|n| n.rotation).collect();
            d.solve_ik();
            assert_eq!(before, d.nodes.iter().map(|n| n.rotation).collect::<Vec<_>>());
        }
    }
}
