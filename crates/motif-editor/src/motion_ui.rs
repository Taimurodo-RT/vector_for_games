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
        let n = ((anim.duration * FPS).ceil() as usize).clamp(1, MAX_SAMPLES);
        // A looping animation wraps its last moment back to the start: pose just before it.
        let end = if anim.looping { (anim.duration - 1e-6).max(0.0) } else { anim.duration };
        let poses: Vec<(f64, motif_core::Document)> =
            (0..=n).map(|k| anim.duration * k as f64 / n as f64).map(|t| (t, self.pose_anim(anim, t.min(end)))).collect();
        let mut out = Vec::new();
        for id in selection.iter().take(8) {
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
