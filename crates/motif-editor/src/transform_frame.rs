//! The transform frame of the selection: corner and side handles that resize it, the handle
//! that turns it, and the arithmetic that turns a pull on the frame into values of each object.
//!
//! One object gets a frame along its own axes; several get one along the artboard's. A pull is
//! always described the same way: a map `G` of the artboard with one point that stays put (the
//! opposite side or corner, the centre with Alt, the centre of a turn). Every object then takes
//! the values that make its world transform `G · W₀`:
//!
//! * when `G` only stretches the object along its own axes, a shape with a size (`rect`,
//!   `ellipse`, `star`, `polygon`) changes `size` and keeps its stroke and corner radius, and a
//!   group, contour or text changes `scale`;
//! * otherwise (a turned object in a frame stretched to one side, or a turn) the rotation, scale
//!   and skew are found from the wanted matrix;
//! * the position is solved last, from the values as they are written (rounded), so the fixed
//!   point stays exactly where it was.

use crate::app::{App, node_bounds};
use crate::canvas::View;
use crate::canvas_drag::DragOp;
use crate::snap::Snap;
use egui::{Pos2, Rect, vec2};
use motif_core::kurbo::{Affine, Point, Rect as KRect, Vec2};
use motif_core::{Document, Prop, ShapeKind};
use motif_script::fmt_num;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Handle {
    /// A size handle. The numbers tell which sides of the frame it pulls: -1, 0 or 1 per axis.
    Size(i8, i8),
    Rotate,
}

const CORNERS: [(i8, i8); 4] = [(-1, -1), (1, -1), (1, 1), (-1, 1)];
/// Top, right, bottom, left: side `k` runs from corner `k` to corner `k + 1`.
const SIDES: [(i8, i8); 4] = [(0, -1), (1, 0), (0, 1), (-1, 0)];
/// A side shorter than this on the screen has no handle of its own: the corners are enough.
const SIDE_MIN: f32 = 28.0;
/// The smallest stretch of a pull, so that nothing collapses into a line.
const MIN_FACTOR: f64 = 1e-3;

/// The frame around the selection.
pub(crate) struct Frame {
    /// Frame space to artboard space: the object's own space for one object, the artboard's for several.
    pub(crate) to_world: Affine,
    pub(crate) bounds: KRect,
    pub(crate) items: Vec<usize>,
}

/// The handles of a frame on the screen.
pub(crate) struct Handles {
    pub(crate) corners: [Pos2; 4],
    pub(crate) sides: [Option<Pos2>; 4],
    pub(crate) rotate: Pos2,
    center: Pos2,
}

/// The point of `b` a handle sits on.
pub(crate) fn grip(b: KRect, hx: i8, hy: i8) -> Point {
    let c = b.center();
    let side = |h: i8, lo: f64, mid: f64, hi: f64| match h {
        -1 => lo,
        1 => hi,
        _ => mid,
    };
    Point::new(side(hx, b.x0, c.x, b.x1), side(hy, b.y0, c.y, b.y1))
}

fn sign(v: f64) -> f64 {
    if v < 0.0 { -1.0 } else { 1.0 }
}

/// Values as the script writes them: two decimals, four for scale.
fn exact(v: f64) -> f64 {
    (v * 100.0).round() / 100.0
}

fn exact_scale(v: f64) -> f64 {
    let r = (v * 10_000.0).round() / 10_000.0;
    if r == 0.0 { 0.0001 * sign(v) } else { r }
}

fn linear(a: Affine) -> Affine {
    let c = a.as_coeffs();
    Affine::new([c[0], c[1], c[2], c[3], 0.0, 0.0])
}

impl Frame {
    /// A point of the frame on the artboard: a corner, the middle of a side or the centre.
    pub(crate) fn point(&self, hx: i8, hy: i8) -> Point {
        self.to_world * grip(self.bounds, hx, hy)
    }

    pub(crate) fn handles(&self, rect: Rect, view: View) -> Handles {
        Handles::around(|(hx, hy)| view.to_screen(rect, self.to_world * grip(self.bounds, hx, hy)))
    }
}

impl Handles {
    /// The handles of a frame whose corners, side middles and centre are at `at` on the screen.
    pub(crate) fn around(at: impl Fn((i8, i8)) -> Pos2) -> Handles {
        let corners = CORNERS.map(&at);
        let center = at((0, 0));
        let mut sides = [None; 4];
        for (k, side) in SIDES.into_iter().enumerate() {
            if corners[k].distance(corners[(k + 1) % 4]) >= SIDE_MIN {
                sides[k] = Some(at(side));
            }
        }
        let top_mid = at(SIDES[0]);
        let up = (top_mid - center).normalized();
        let up = if up.is_finite() && up != egui::Vec2::ZERO { up } else { vec2(0.0, -1.0) };
        Handles { corners, sides, rotate: top_mid + up * 22.0, center }
    }

    pub(crate) fn hit(&self, p: Pos2) -> Option<Handle> {
        if self.rotate.distance(p) < 8.0 {
            return Some(Handle::Rotate);
        }
        let corners = self.corners.iter().zip(CORNERS).map(|(c, h)| (Some(*c), h));
        let sides = self.sides.iter().zip(SIDES).map(|(s, h)| (*s, h));
        corners.chain(sides).find(|(at, _)| at.is_some_and(|at| at.distance(p) < 7.0)).map(|(_, (hx, hy))| Handle::Size(hx, hy))
    }

    /// The resize cursor that points the way a size handle pulls on the screen.
    pub(crate) fn cursor(&self, hx: i8, hy: i8) -> egui::CursorIcon {
        let at = CORNERS
            .iter()
            .position(|&h| h == (hx, hy))
            .map(|k| self.corners[k])
            .or_else(|| SIDES.iter().position(|&h| h == (hx, hy)).and_then(|k| self.sides[k]))
            .unwrap_or(self.center);
        let d = at - self.center;
        match d.y.atan2(d.x).to_degrees().rem_euclid(180.0) {
            a if !(22.5..157.5).contains(&a) => egui::CursorIcon::ResizeHorizontal,
            a if a < 67.5 => egui::CursorIcon::ResizeNwSe,
            a if a < 112.5 => egui::CursorIcon::ResizeVertical,
            _ => egui::CursorIcon::ResizeNeSw,
        }
    }
}

/// Rotation has no system cursor: a circular arrow is drawn at the pointer instead.
pub(crate) fn paint_rotate_cursor(ctx: &egui::Context, p: Pos2) {
    ctx.set_cursor_icon(egui::CursorIcon::None);
    let fg = ctx.layer_painter(egui::LayerId::new(egui::Order::Tooltip, egui::Id::unique("rotate-cursor")));
    fg.circle_filled(p, 11.0, egui::Color32::from_black_alpha(140));
    crate::theme::paint_icon(&fg, Rect::from_center_size(p, vec2(22.0, 22.0)), crate::theme::Icon::Rotate, egui::Color32::WHITE);
}

/// How far a size handle pulled to `p` (frame space) stretches the frame, and around which point.
/// `centre` (Alt) stretches around the middle instead of the opposite side; `keep` (Shift) keeps
/// the proportions.
pub(crate) fn factors(b: KRect, hx: i8, hy: i8, p: Point, centre: bool, keep: bool) -> (Point, f64, f64) {
    let held = grip(b, hx, hy);
    let anchor = if centre { b.center() } else { grip(b, -hx, -hy) };
    let factor = |h: i8, p: f64, anchor: f64, held: f64| if h == 0 || (held - anchor).abs() < 1e-9 { 1.0 } else { (p - anchor) / (held - anchor) };
    let (mut fx, mut fy) = (factor(hx, p.x, anchor.x, held.x), factor(hy, p.y, anchor.y, held.y));
    if keep {
        match (hx != 0, hy != 0) {
            (true, true) => {
                let m = fx.abs().max(fy.abs());
                (fx, fy) = (m * sign(fx), m * sign(fy));
            }
            // A side with Shift: the other axis grows with it, around its middle.
            (true, false) => fy = fx.abs(),
            (false, true) => fx = fy.abs(),
            (false, false) => {}
        }
    }
    (anchor, fx, fy)
}

/// What a transform keeps of an object from the moment the pull started.
#[derive(Clone, Copy)]
struct Start {
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    rot: f64,
    sx: f64,
    sy: f64,
    kx: f64,
    ky: f64,
    pivot: Vec2,
    offset: Vec2,
    warp: Affine,
    swing: f64,
    /// A shape with a size of its own; the others are stretched by `scale`.
    sized: bool,
    /// A stack places it: its position is not ours to write.
    stacked: bool,
}

struct PullItem {
    node: String,
    /// Its world transform and the way from the artboard to its parent, at the start.
    world: Affine,
    to_parent: Affine,
    start: Start,
}

/// One value of one object during a pull. `changed` is false when it is back at the value the
/// pull started from, so nothing needs to be written for it.
pub(crate) struct Edit {
    pub(crate) node: String,
    pub(crate) prop: Prop,
    pub(crate) value: f64,
    pub(crate) changed: bool,
}

/// A handle of the frame being pulled.
pub struct Pull {
    handle: Handle,
    to_world: Affine,
    bounds: KRect,
    items: Vec<PullItem>,
    /// The pointer minus the handle when the button went down: the handle does not jump to the pointer.
    pub(crate) grab: Vec2,
    /// A turn: the direction from the centre to the pointer at the start, degrees.
    start_angle: f64,
}

impl PullItem {
    /// The values that put the object where the map `g` of the artboard takes it; `fixed` is the
    /// point `g` leaves in place.
    fn edits(&self, g: Affine, fixed: Point, out: &mut Vec<Edit>) {
        let s = &self.start;
        let w0 = linear(self.world);
        if w0.determinant().abs() < 1e-12 {
            return;
        }
        // The pull as the object sees it in its own space.
        let f = (w0.inverse() * linear(g) * w0).as_coeffs();
        let size = f[0].abs().max(f[3].abs()).max(1e-9);
        let along = f[1].abs() < 1e-6 * size && f[2].abs() < 1e-6 * size;
        let (mut w, mut h, mut rot, mut sx, mut sy, mut kx, mut ky) = (s.w, s.h, s.rot, s.sx, s.sy, s.kx, s.ky);
        // The fixed point in the object's own space; a shape that changes its size carries it along.
        let mut q = self.world.inverse() * fixed;
        if along && s.sized {
            w = exact(s.w * f[0].abs()).max(0.01);
            h = exact(s.h * f[3].abs()).max(0.01);
            if s.w > 0.0 {
                q.x *= sign(f[0]) * w / s.w;
            }
            if s.h > 0.0 {
                q.y *= sign(f[3]) * h / s.h;
            }
        } else if along {
            sx = exact_scale(s.sx * f[0]);
            sy = exact_scale(s.sy * f[3]);
        } else {
            // Rotation · skew · scale that make the wanted matrix: the first column gives the
            // angle and the x scale, the second the y scale and the skew.
            let n = (linear(s.warp).inverse() * linear(self.to_parent * g * self.world)).as_coeffs();
            let mut angle = n[1].atan2(n[0]);
            let (sin, cos) = angle.sin_cos();
            let (mut x_scale, mut y_scale) = (n[0].hypot(n[1]), -sin * n[2] + cos * n[3]);
            let slant = if y_scale.abs() > 1e-12 { (cos * n[2] + sin * n[3]) / y_scale } else { 0.0 };
            if s.sx < 0.0 {
                // The same matrix with both scales negated, half a turn away: keeps a mirrored object mirrored.
                (angle, x_scale, y_scale) = (angle + std::f64::consts::PI, -x_scale, -y_scale);
            }
            let turned = angle.to_degrees() - s.swing;
            rot = exact(turned + 360.0 * ((s.rot - turned) / 360.0).round());
            (sx, sy) = (exact_scale(x_scale), exact_scale(y_scale));
            (kx, ky) = (exact(slant.atan().to_degrees().clamp(-89.0, 89.0)), 0.0);
        }
        // Nothing to write for a value that is where it started (to the precision it is written with).
        let same = |a: f64, b: f64, step: f64| (a - b).abs() < step * 0.5;
        let (w_same, h_same, rot_same) = (same(w, s.w, 0.01), same(h, s.h, 0.01), same(rot, s.rot, 0.01));
        let (sx_same, sy_same) = (same(sx, s.sx, 0.0001), same(sy, s.sy, 0.0001));
        let (kx_same, ky_same) = (same(kx, s.kx, 0.01), same(ky, s.ky, 0.01));
        let keep = |same: bool, new: f64, old: f64| if same { old } else { new };
        (w, h, rot) = (keep(w_same, w, s.w), keep(h_same, h, s.h), keep(rot_same, rot, s.rot));
        (sx, sy, kx, ky) = (keep(sx_same, sx, s.sx), keep(sy_same, sy, s.sy), keep(kx_same, kx, s.kx), keep(ky_same, ky, s.ky));
        // The same matrix `Node::local_transform` builds, from the values as written.
        let tan = |deg: f64| deg.clamp(-89.0, 89.0).to_radians().tan();
        let n = s.warp * Affine::rotate((rot + s.swing).to_radians()) * Affine::skew(tan(kx), tan(ky)) * Affine::scale_non_uniform(sx, sy);
        let origin = (self.to_parent * fixed).to_vec2() - s.offset - s.pivot - (n * (q - s.pivot)).to_vec2();
        let (x, y) = (exact(origin.x), exact(origin.y));
        let mut push = |prop: Prop, value: f64, changed: bool| out.push(Edit { node: self.node.clone(), prop, value, changed });
        if !s.stacked {
            push(Prop::X, x, !same(x, s.x, 0.01));
            push(Prop::Y, y, !same(y, s.y, 0.01));
        }
        if s.sized {
            push(Prop::Width, w, !w_same);
            push(Prop::Height, h, !h_same);
        }
        push(Prop::Rotation, rot, !rot_same);
        push(Prop::ScaleX, sx, !sx_same);
        push(Prop::ScaleY, sy, !sy_same);
        push(Prop::SkewX, kx, !kx_same);
        push(Prop::SkewY, ky, !ky_same);
    }
}

impl Pull {
    /// A size handle is held (and not the one that turns the frame).
    pub(crate) fn sizing(&self) -> bool {
        matches!(self.handle, Handle::Size(..))
    }

    /// A side handle of a frame that stands straight on the artboard: it moves along one axis
    /// only, `Some(true)` for x. Lines across that axis mean nothing to it.
    pub(crate) fn straight_axis(&self) -> Option<bool> {
        let c = self.to_world.as_coeffs();
        let straight = c[1].abs() < 1e-9 && c[2].abs() < 1e-9;
        match self.handle {
            Handle::Size(_, 0) if straight => Some(true),
            Handle::Size(0, _) if straight => Some(false),
            _ => None,
        }
    }

    /// The values of every object for the handle pulled to `to` (artboard space), and a line
    /// for the status bar. `stuck` tells that the point sits on a guide or an anchor; `keep` is
    /// Shift (proportions, or turns by 15°), `centre` is Alt (around the middle of the frame).
    pub(crate) fn edits(&self, snap: Snap, to: Point, stuck: bool, keep: bool, centre: bool) -> (Vec<Edit>, String) {
        let about = |p: Point, m: Affine| Affine::translate(p.to_vec2()) * m * Affine::translate(-p.to_vec2());
        let (g, fixed, note) = match self.handle {
            Handle::Size(hx, hy) => {
                let (anchor, mut fx, mut fy) = factors(self.bounds, hx, hy, self.to_world.inverse() * to, centre, keep);
                let own_size = match self.items.as_slice() {
                    [it] if it.start.sized && it.world == self.to_world => Some(it.start),
                    _ => None,
                };
                if let Some(s) = own_size {
                    // One shape with a size: it goes by the size step, or exactly to what the handle stuck to.
                    let size = |v: f64| if stuck { exact(v) } else { snap.size(v) }.max(1.0);
                    if s.w > 0.0 && (hx != 0 || keep) {
                        fx = sign(fx) * size(s.w * fx.abs()) / s.w;
                    }
                    if s.h > 0.0 && (hy != 0 || keep) {
                        fy = sign(fy) * size(s.h * fy.abs()) / s.h;
                    }
                }
                let floor = |f: f64| if f.abs() < MIN_FACTOR { MIN_FACTOR * sign(f) } else { f };
                (fx, fy) = (floor(fx), floor(fy));
                let note = match own_size {
                    Some(s) => format!("Размер {} × {}", fmt_num(s.w * fx.abs()), fmt_num(s.h * fy.abs())),
                    None => format!("Масштаб {}% × {}%", fmt_num(fx.abs() * 100.0), fmt_num(fy.abs() * 100.0)),
                };
                let stretch = about(anchor, Affine::scale_non_uniform(fx, fy));
                (self.to_world * stretch * self.to_world.inverse(), self.to_world * anchor, note)
            }
            Handle::Rotate => {
                let center = self.to_world * self.bounds.center();
                let mut turn = (to - center).atan2().to_degrees() - self.start_angle;
                turn = (turn + 180.0).rem_euclid(360.0) - 180.0;
                turn = if keep { (turn / 15.0).round() * 15.0 } else { snap.rot(turn) };
                (about(center, Affine::rotate(turn.to_radians())), center, format!("Поворот {}°", fmt_num(turn)))
            }
        };
        let mut out = Vec::new();
        for it in &self.items {
            it.edits(g, fixed, &mut out);
        }
        (out, note)
    }
}

impl App {
    /// The frame of the selection: one object's own box, or the box around several.
    pub(crate) fn transform_frame(&self, doc: &Document) -> Option<Frame> {
        if self.point_edit.is_some() {
            return None;
        }
        let picked: Vec<usize> = self.selected_in(doc).into_iter().filter(|&i| doc.nodes[i].kind != ShapeKind::Bone).collect();
        if let ([i], true) = (picked.as_slice(), self.also.is_empty()) {
            // A repeat along an outline stands where the outline puts it: no frame to pull.
            if self.placed_by_outline(&doc.nodes[*i].id, motif_core::Prop::X).is_some() {
                return None;
            }
            return Some(Frame { to_world: doc.world_transform(*i), bounds: node_bounds(doc, *i)?, items: picked });
        }
        // An object inside a selected group moves with the group.
        let mut is_picked = vec![false; doc.nodes.len()];
        picked.iter().for_each(|&i| is_picked[i] = true);
        let inside = |i: usize| std::iter::successors(doc.nodes[i].parent, |&p| doc.nodes[p].parent).any(|p| is_picked[p]);
        let items: Vec<usize> = picked.iter().copied().filter(|&i| !inside(i) && !doc.is_locked(i)).collect();
        if items.len() < 2 {
            return None;
        }
        let bounds = items.iter().filter_map(|&i| crate::selection::world_bounds(doc, i)).reduce(|a, b| a.union(b))?;
        Some(Frame { to_world: Affine::IDENTITY, bounds, items })
    }

    /// A handle of the frame is grabbed at `dp` (artboard space).
    pub(crate) fn begin_pull(&mut self, doc: &Document, frame: Frame, handle: Handle, dp: Point) -> DragOp {
        let items = frame
            .items
            .iter()
            .map(|&i| {
                let n = &doc.nodes[i];
                PullItem {
                    node: n.id.clone(),
                    world: doc.world_transform(i),
                    to_parent: n.parent.map_or(Affine::IDENTITY, |p| doc.world_transform(p).inverse()),
                    start: Start {
                        x: n.x,
                        y: n.y,
                        w: n.width,
                        h: n.height,
                        rot: n.rotation,
                        sx: n.scale_x,
                        sy: n.scale_y,
                        kx: n.skew_x,
                        ky: n.skew_y,
                        pivot: n.pivot,
                        offset: n.offset,
                        warp: n.warp.unwrap_or(Affine::IDENTITY),
                        swing: n.swing,
                        sized: matches!(n.kind, ShapeKind::Rect | ShapeKind::Ellipse | ShapeKind::Star | ShapeKind::Polygon),
                        stacked: n.parent.is_some_and(|p| doc.nodes[p].stack.is_some()),
                    },
                }
            })
            .collect();
        let (grab, start_angle) = match handle {
            Handle::Size(hx, hy) => {
                self.drag_targets = self.snap_targets(doc, &frame.items, true);
                if hx == 0 || hy == 0 {
                    // A side meets lines, not single points.
                    self.drag_targets.clear_points();
                }
                (dp - frame.to_world * grip(frame.bounds, hx, hy), 0.0)
            }
            Handle::Rotate => (Vec2::ZERO, (dp - frame.to_world * frame.bounds.center()).atan2().to_degrees()),
        };
        DragOp::Transform(Pull { handle, to_world: frame.to_world, bounds: frame.bounds, items, grab, start_angle })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_corner_stretches_from_the_opposite_corner_and_alt_from_the_middle() {
        let b = KRect::new(0.0, 0.0, 100.0, 50.0);
        let (anchor, fx, fy) = factors(b, 1, 1, Point::new(150.0, 100.0), false, false);
        assert_eq!((anchor, fx, fy), (Point::new(0.0, 0.0), 1.5, 2.0));
        let (anchor, fx, fy) = factors(b, 1, 1, Point::new(150.0, 100.0), true, false);
        assert_eq!((anchor, fx, fy), (Point::new(50.0, 25.0), 2.0, 3.0));
        // Shift takes the larger stretch for both axes; a side with Shift grows the other axis too.
        assert_eq!(factors(b, 1, 1, Point::new(150.0, 100.0), false, true), (Point::new(0.0, 0.0), 2.0, 2.0));
        assert_eq!(factors(b, -1, 0, Point::new(-100.0, 7.0), false, true), (Point::new(100.0, 25.0), 2.0, 2.0));
        // Past the fixed side the frame turns inside out.
        assert_eq!(factors(b, 1, 0, Point::new(-50.0, 0.0), false, false).1, -0.5);
    }
}
