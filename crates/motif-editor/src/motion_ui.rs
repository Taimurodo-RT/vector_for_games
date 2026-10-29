//! The motion path: where the selected objects travel during the animation, drawn on the canvas
//! with a dot per frame (close dots — slow, far apart — fast) and a handle at every position key.
//! A handle drags its key; a click on the line moves the playhead to that moment.

use crate::app::{App, FPS, MoveItem, Tool};
use crate::canvas::View;
use crate::theme::{self, ACCENT};
use egui::{Align2, Color32, Pos2, Rect, Stroke};
use motif_core::kurbo::{Affine, Point};
use motif_core::{Animation, Prop};

/// Most samples along one path; long animations are sampled more sparsely.
const MAX_SAMPLES: usize = 360;
/// Pointer distance (screen pixels) that picks a key handle, and the line.
const KEY_PICK: f32 = 7.0;
const LINE_PICK: f32 = 5.0;

/// One object's trip through the animation.
pub(crate) struct MotionPath {
    node: String,
    /// Time, origin in artboard space, artboard space to the parent's space.
    samples: Vec<(f64, Point, Affine)>,
    /// Times of the object's own position keys.
    keys: Vec<f64>,
}

/// What the pointer is over on a motion path.
pub(crate) struct MotionHit {
    node: String,
    time: f64,
    at: Point,
    to_parent: Affine,
    pub(crate) key: bool,
}

impl MotionPath {
    fn sample_at(&self, t: f64) -> Option<(Point, Affine)> {
        let i = self.samples.partition_point(|s| s.0 < t);
        let after = self.samples.get(i).or(self.samples.last())?;
        let before = &self.samples[i.saturating_sub(1)];
        if (after.0 - before.0).abs() < 1e-9 {
            return Some((after.1, after.2));
        }
        let k = ((t - before.0) / (after.0 - before.0)).clamp(0.0, 1.0);
        Some((before.1.lerp(after.1, k), if k < 0.5 { before.2 } else { after.2 }))
    }
}

/// Times of the position keys `node` has in `anim`.
fn key_times(anim: &Animation, node: &str) -> Vec<f64> {
    let mut out: Vec<f64> =
        anim.tracks.iter().filter(|tr| tr.node == node && matches!(tr.prop, Prop::X | Prop::Y)).flat_map(|tr| tr.keys.iter().map(|k| k.time)).collect();
    out.sort_by(f64::total_cmp);
    out.dedup_by(|a, b| (*a - *b).abs() < 0.005);
    out
}

fn motion_is_affine(doc: &motif_core::Document) -> bool {
    !doc.has_sim() && doc.nodes.iter().all(|n| n.on.is_none() && n.ik.is_none() && n.hang.is_none() && n.stack.is_none() && n.anchor.is_none() && !n.fit)
}

fn affine_motion_candidate(doc: &motif_core::Document, anim: &Animation, id: &str) -> bool {
    let Some(mut i) = doc.index_of(id) else { return false };
    let mut ancestors = Vec::new();
    loop {
        ancestors.push(doc.nodes[i].id.as_str());
        match doc.nodes[i].parent {
            Some(parent) => i = parent,
            None => break,
        }
    }
    let transform = |prop| matches!(prop, Prop::X | Prop::Y | Prop::Rotation | Prop::ScaleX | Prop::ScaleY | Prop::SkewX | Prop::SkewY);
    let changes = |track: &motif_core::Track| track.keys.first().is_some_and(|first| track.keys.iter().any(|key| key.value != first.value));
    anim.tracks.iter().any(|t| {
        if !changes(t) {
            return false;
        }
        if t.prop == Prop::Parameter {
            doc.drivers
                .iter()
                .filter(|d| d.parameter == t.node)
                .flat_map(|d| &d.tracks)
                .any(|driven| ancestors.contains(&driven.node.as_str()) && transform(driven.prop) && changes(driven))
        } else {
            ancestors.contains(&t.node.as_str()) && transform(t.prop)
        }
    })
}

impl App {
    /// Whether motion paths are shown and can be picked now.
    fn motion_on(&self) -> bool {
        self.motion_path
            && self.anim.is_some()
            && self.running().is_none()
            && self.tool == Tool::Select
            && self.point_edit.is_none()
            && self.brush_w.on.is_none()
    }

    /// The paths of the selected objects that move in the current animation, computed once per
    /// script, animation and selection.
    pub(crate) fn motion_paths(&self) -> std::cell::Ref<'_, Vec<MotionPath>> {
        use std::hash::{Hash, Hasher};
        let selection = self.selection();
        let mut h = std::collections::hash_map::DefaultHasher::new();
        (&self.src, &self.anim, &selection, self.motion_on()).hash(&mut h);
        let key = h.finish();
        if self.motion.borrow().0 != key {
            let paths = match self.current_anim().filter(|_| self.motion_on()) {
                Some(anim) => self.compute_paths(anim, &selection),
                None => Vec::new(),
            };
            *self.motion.borrow_mut() = (key, paths);
        }
        std::cell::Ref::map(self.motion.borrow(), |m| &m.1)
    }

    fn compute_paths(&self, anim: &Animation, selection: &[String]) -> Vec<MotionPath> {
        if selection.is_empty() {
            return Vec::new();
        }
        // Warp/forms change the artwork, but not the affine origin used by this overlay.
        // Full rig/simulation is retained whenever layout, orbit, IK or physics can move it.
        let affine_only = motion_is_affine(&self.scene.doc);
        let selection: Vec<_> = selection.iter().take(8).filter(|id| !affine_only || affine_motion_candidate(&self.scene.doc, anim, id)).collect();
        if selection.is_empty() {
            return Vec::new();
        }
        let n = ((anim.duration * FPS).ceil() as usize).clamp(1, MAX_SAMPLES);
        // A looping animation wraps its last moment back to the start: pose just before it.
        let end = if anim.looping { (anim.duration - 1e-6).max(0.0) } else { anim.duration };
        let poses: Vec<(f64, motif_core::Document)> = (0..=n)
            .map(|k| anim.duration * k as f64 / n as f64)
            .map(|t| (t, if affine_only { anim.keys_at(&self.scene.doc, t.min(end)) } else { self.pose_anim(anim, t.min(end)) }))
            .collect();
        let mut out = Vec::new();
        for id in selection {
            let samples: Vec<(f64, Point, Affine)> = poses
                .iter()
                .filter_map(|(t, d)| {
                    let i = d.index_of(id)?;
                    let to_parent = d.nodes[i].parent.map_or(Affine::IDENTITY, |p| d.world_transform(p).inverse());
                    Some((*t, d.world_transform(i) * Point::ZERO, to_parent))
                })
                .collect();
            let Some(first) = samples.first() else { continue };
            // An object that stays put has no path to show.
            if samples.iter().all(|s| (s.1 - first.1).hypot() < 0.5) {
                continue;
            }
            out.push(MotionPath { node: id.clone(), samples, keys: key_times(anim, id) });
        }
        out
    }

    /// The key handle or the stretch of path under `p`; handles win over the line.
    pub(crate) fn motion_hit(&self, rect: Rect, view: View, p: Pos2) -> Option<MotionHit> {
        if !self.motion_on() {
            return None;
        }
        let paths = self.motion_paths();
        let screen = |q: Point| view.to_screen(rect, q);
        let mut best: Option<(f32, MotionHit)> = None;
        for path in paths.iter() {
            for &t in &path.keys {
                let Some((at, to_parent)) = path.sample_at(t) else { continue };
                let d = screen(at).distance(p);
                if d < KEY_PICK && best.as_ref().is_none_or(|b| !b.1.key || d < b.0) {
                    best = Some((d, MotionHit { node: path.node.clone(), time: t, at, to_parent, key: true }));
                }
            }
        }
        if best.is_some() {
            return best.map(|b| b.1);
        }
        for path in paths.iter() {
            for w in path.samples.windows(2) {
                let (a, b) = (screen(w[0].1), screen(w[1].1));
                let ab = b - a;
                let k = if ab.length_sq() > 0.0 { ((p - a).dot(ab) / ab.length_sq()).clamp(0.0, 1.0) } else { 0.0 };
                let d = (a + ab * k).distance(p);
                if d < LINE_PICK && best.as_ref().is_none_or(|b| d < b.0) {
                    let t = ((w[0].0 + (w[1].0 - w[0].0) * k as f64) * 100.0).round() / 100.0;
                    let (at, to_parent) = path.sample_at(t).unwrap_or((w[0].1, w[0].2));
                    best = Some((d, MotionHit { node: path.node.clone(), time: t, at, to_parent, key: false }));
                }
            }
        }
        best.map(|b| b.1)
    }

    /// Moves the playhead to the picked moment, so edits key it there.
    pub(crate) fn motion_seek(&mut self, hit: &MotionHit) {
        self.time = hit.time;
        self.playing = false;
        self.status = if hit.key {
            format!("Ключ «{}» на {} с: тяните, чтобы сдвинуть", hit.node, motif_script::fmt_num(hit.time))
        } else {
            format!("{} с: тяните, чтобы поставить здесь ключ положения", motif_script::fmt_num(hit.time))
        };
    }

    /// Starts dragging the object from where it is at the picked moment.
    pub(crate) fn motion_drag(&mut self, hit: MotionHit, from: Point) -> crate::app::DragOp {
        self.motion_seek(&hit);
        if !self.is_selected(&hit.node) {
            self.select_only(Some(hit.node.clone()));
        }
        crate::app::DragOp::Move {
            from,
            items: vec![MoveItem { node: hit.node, origin: hit.at, to_parent: hit.to_parent }],
            start_box: None,
            targets: Default::default(),
        }
    }

    /// The paths, their frame dots and key handles.
    pub(crate) fn paint_motion(&self, painter: &egui::Painter, rect: Rect, view: View, pointer: Option<Pos2>) {
        let paths = self.motion_paths();
        if paths.is_empty() {
            return;
        }
        let hot = pointer.and_then(|p| self.motion_hit(rect, view, p));
        let now = self.current_anim().map(|a| a.local_time(self.time));
        let line = ACCENT.gamma_multiply(0.8);
        for path in paths.iter() {
            let pts: Vec<Pos2> = path.samples.iter().map(|s| view.to_screen(rect, s.1)).collect();
            painter.add(egui::Shape::line(pts.clone(), Stroke::new(3.0, Color32::from_black_alpha(90))));
            painter.add(egui::Shape::line(pts.clone(), Stroke::new(1.25, line)));
            // A dot per frame: their spacing shows the speed.
            let every = ((pts.len() as f64 / (path.samples.last().map_or(1.0, |s| s.0) * FPS).max(1.0)).round() as usize).max(1);
            for p in pts.iter().step_by(every) {
                painter.circle_filled(*p, 2.2, Color32::from_black_alpha(110));
                painter.circle_filled(*p, 1.5, ACCENT);
            }
            for &t in &path.keys {
                let Some((at, _)) = path.sample_at(t) else { continue };
                let c = view.to_screen(rect, at);
                let picked = hot.as_ref().is_some_and(|h| h.key && h.node == path.node && (h.time - t).abs() < 1e-6);
                let current = now.is_some_and(|n| (n - t).abs() < 0.005);
                let r = if picked { 6.0 } else { 4.5 };
                let fill = if current { ACCENT } else { Color32::WHITE };
                painter.circle(c, r, fill, Stroke::new(1.5, ACCENT));
                if picked {
                    time_label(painter, c, t);
                }
            }
        }
        if let Some(h) = hot.filter(|h| !h.key) {
            let c = view.to_screen(rect, h.at);
            painter.circle_stroke(c, 4.0, Stroke::new(1.5, ACCENT));
            time_label(painter, c, h.time);
        }
    }
}

/// «0.6 с» on a dark plate next to a point of the path, readable over any artboard.
fn time_label(painter: &egui::Painter, at: Pos2, t: f64) {
    let galley = painter.layout_no_wrap(format!("{} с", motif_script::fmt_num(t)), theme::ui_font(11.5), Color32::WHITE);
    let r = Align2::LEFT_BOTTOM.anchor_size(at + egui::vec2(10.0, -8.0), galley.size()).expand2(egui::vec2(5.0, 2.0));
    painter.rect_filled(r, 4.0, Color32::from_black_alpha(200));
    painter.galley(r.min + egui::vec2(5.0, 2.0), galley, Color32::WHITE);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn face_warp_controls_and_empty_selection_need_no_origin_trajectory() {
        let mut app = crate::app::editing_tests::fixture(include_str!("../../../examples/character-anime.msc"));
        app.select_anim("Combinations".into());
        let anim = app.current_anim().unwrap();
        assert!(motion_is_affine(&app.scene.doc));
        for id in ["portrait", "head", "face", "mouth_rig", "eye_l"] {
            assert!(!affine_motion_candidate(&app.scene.doc, anim, id));
            assert!(app.compute_paths(anim, &[id.into()]).is_empty());
        }
        assert!(app.compute_paths(anim, &[]).is_empty());
    }

    #[test]
    fn affine_samples_keep_parent_rotation_and_driven_position_without_deforming_art() {
        let mut app = crate::app::editing_tests::fixture(
            "artboard 300x300\nparameter p range -1 1 default 0 value 0\ngroup root at 150 150 deform 180 180 {\nellipse ball at 35 0 size 30\n}\ndriver p {\nball.x -1: 20 | 1: 70\nroot.warp-x4 -1: -25 | 1: 25\n}\nanim A 2s {\nroot.rot 0: 0 linear | 2: 90\np.value 0: -1 linear | 2: 1\n}\n",
        );
        app.select_anim("A".into());
        let anim = app.current_anim().unwrap();
        assert!(motion_is_affine(&app.scene.doc));
        let paths = app.compute_paths(anim, &["ball".into()]);
        assert_eq!(paths.len(), 1);
        for (t, point, parent) in &paths[0].samples {
            let pose = app.pose_anim(anim, *t);
            let i = pose.index_of("ball").unwrap();
            assert!((*point - pose.world_transform(i) * Point::ZERO).hypot() < 1e-9);
            assert_eq!(*parent, pose.world_transform(pose.nodes[i].parent.unwrap()).inverse());
        }
    }

    #[test]
    fn layout_or_simulation_keeps_the_full_pose_path() {
        let scene =
            motif_script::parse("artboard 300x300\ngroup row stack row {\nrect a size 40\nrect b size 40\n}\nanim A 1s {\na.w 0: 40 | 1: 80\n}\n").unwrap();
        assert!(!motion_is_affine(&scene.doc));
        let sim = motif_script::parse(include_str!("../../../examples/bounce.msc")).unwrap();
        let mut doc = sim.doc;
        doc.nodes[0].spring = Some(motif_core::Spring::default());
        assert!(!motion_is_affine(&doc));
    }

    #[test]
    fn recorded_constant_transform_does_not_require_trajectory_samples() {
        let scene =
            motif_script::parse("artboard 200x200\nrect box at 50 50 size 30\nanim A 2s {\nbox.x 0: 50 | 1: 50 | 2: 50\nbox.y 0: 50\nbox.rot 0: 0 | 2: 0\n}\n")
                .unwrap();
        assert!(!affine_motion_candidate(&scene.doc, &scene.doc.animations[0], "box"));
    }

    #[test]
    fn key_times_merge_x_and_y() {
        let scene = motif_script::parse(
            "artboard \"a\" 100x100\nellipse b at 0 0 size 4\nanim A 1s {\n  b.x 0: 0 | 0.5: 10 | 1: 20\n  b.y 0: 0 | 0.7: 5\n  b.w 0.2: 4 | 0.3: 8\n}\n",
        )
        .unwrap();
        let anim = scene.doc.animation("A").unwrap();
        assert_eq!(key_times(anim, "b"), vec![0.0, 0.5, 0.7, 1.0]);
    }

    #[test]
    fn sample_at_interpolates_between_samples() {
        let path = MotionPath {
            node: "b".into(),
            samples: vec![(0.0, Point::new(0.0, 0.0), Affine::IDENTITY), (1.0, Point::new(10.0, 20.0), Affine::IDENTITY)],
            keys: vec![0.0, 1.0],
        };
        assert_eq!(path.sample_at(0.25).unwrap().0, Point::new(2.5, 5.0));
        assert_eq!(path.sample_at(2.0).unwrap().0, Point::new(10.0, 20.0));
    }
}
