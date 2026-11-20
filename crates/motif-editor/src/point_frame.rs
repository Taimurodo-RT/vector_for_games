//! The frame around several selected points of an outline. Its handles stretch and turn the
//! points together, with their curve handles; the buttons of the panel line the points up.
//!
//! The frame lies along the outline's own axes, so it turns and leans with the object. A size
//! handle keeps the opposite side where it is (Alt: the middle; Shift: the proportions). The
//! turn is made the way the eye sees it on the artboard, also inside a stretched object.

use crate::app::App;
use crate::compound::{Anchor, Compound};
use crate::selection::Align;
use crate::transform_frame::{Handle, Handles, factors, grip};
use egui::{Color32, Pos2, Rect, Stroke, StrokeKind, vec2};
use motif_core::kurbo::{Affine, Point, Rect as KRect, Vec2};

/// How far the frame stands off the points on the screen, so that its handles do not cover them.
const PAD: f32 = 10.0;

/// The bounds of the selected points in the outline's own space. Fewer than two points, or
/// points that all sit in one place, have no frame.
pub(crate) fn bounds(o: &Compound, picked: &[Anchor]) -> Option<KRect> {
    let mut pts = picked.iter().filter(|a| o.contains(**a)).map(|a| o.parts[a.part].outline.verts[a.vertex].p);
    let first = pts.next()?;
    let mut b = KRect::from_points(first, first);
    let mut count = 1;
    for p in pts {
        b = b.union_pt(p);
        count += 1;
    }
    (count >= 2 && (b.width() > 1e-9 || b.height() > 1e-9)).then_some(b)
}

/// The handles of the frame on the screen; `screen` maps the outline's own space.
pub(crate) fn handles(b: KRect, screen: impl Fn(Point) -> Pos2) -> Handles {
    let c = b.center();
    let origin = screen(c);
    let axis = |d: Vec2, fallback: egui::Vec2| {
        let v = (screen(c + d) - origin).normalized();
        if v.is_finite() && v != egui::Vec2::ZERO { v } else { fallback }
    };
    let (ux, uy) = (axis(Vec2::new(1.0, 0.0), vec2(1.0, 0.0)), axis(Vec2::new(0.0, 1.0), vec2(0.0, 1.0)));
    Handles::around(|(hx, hy)| screen(grip(b, hx, hy)) + ux * (hx as f32 * PAD) + uy * (hy as f32 * PAD))
}

/// A pull on a handle of the frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FramePull {
    pub(crate) handle: Handle,
    /// The bounds of the points when the pull started.
    pub(crate) bounds: KRect,
    /// The pointer minus the pulled point of the bounds at the press (own space).
    pub(crate) grab: Vec2,
    /// The direction from the middle of the frame to the pointer at the press, on the artboard.
    pub(crate) start_angle: f64,
}

impl FramePull {
    /// `p` is the pointer in the outline's own space, `world` takes that space to the artboard.
    pub(crate) fn new(handle: Handle, bounds: KRect, p: Point, world: Affine) -> Self {
        let grab = match handle {
            Handle::Size(hx, hy) => p - grip(bounds, hx, hy),
            Handle::Rotate => Vec2::ZERO,
        };
        let centre = world * bounds.center();
        FramePull { handle, bounds, grab, start_angle: (world * p - centre).atan2() }
    }

    /// The map of the outline's own space for the pointer at `p`, and a note for the user.
    pub(crate) fn map(&self, p: Point, world: Affine, keep: bool, centre: bool) -> (Affine, String) {
        match self.handle {
            Handle::Size(hx, hy) => {
                let (anchor, fx, fy) = factors(self.bounds, hx, hy, p - self.grab, centre, keep);
                let map = Affine::translate(anchor.to_vec2()) * Affine::scale_non_uniform(fx, fy) * Affine::translate(-anchor.to_vec2());
                let pct = |f: f64| format!("{:.0}%", f * 100.0);
                let note = match (hx != 0 || keep, hy != 0 || keep) {
                    (true, false) => format!("Точки: ширина {}", pct(fx)),
                    (false, true) => format!("Точки: высота {}", pct(fy)),
                    _ => format!("Точки: {} × {}", pct(fx), pct(fy)),
                };
                (map, note)
            }
            Handle::Rotate => {
                let pivot = world * self.bounds.center();
                let mut turn = (world * p - pivot).atan2() - self.start_angle;
                if keep {
                    let step = 15f64.to_radians();
                    turn = (turn / step).round() * step;
                }
                let map = world.inverse() * Affine::rotate_about(turn, pivot) * world;
                let mut deg = turn.to_degrees();
                if deg > 180.0 {
                    deg -= 360.0;
                } else if deg < -180.0 {
                    deg += 360.0;
                }
                (map, format!("Точки: поворот {deg:.1}°"))
            }
        }
    }
}

/// Where `map` puts `p`, as the outline keeps it: two decimals. A coordinate the map does not
/// change stays as it was written.
fn exact(p: Point, map: Affine) -> Point {
    let q = map * p;
    let keep = |was: f64, now: f64| if (now - was).abs() < 1e-9 { was } else { (now * 100.0).round() / 100.0 };
    Point::new(keep(p.x, q.x), keep(p.y, q.y))
}

/// Applies `map` to the selected points and their curve handles.
pub(crate) fn apply(o: &mut Compound, picked: &[Anchor], map: Affine) {
    for a in picked {
        if o.contains(*a) {
            let v = &mut o.parts[a.part].outline.verts[a.vertex];
            v.p = exact(v.p, map);
            v.cin = v.cin.map(|c| exact(c, map));
            v.cout = v.cout.map(|c| exact(c, map));
        }
    }
}

/// Moves the selected points so that they line up or stand at equal steps, as seen on the
/// artboard (`world` takes the outline's own space there). Returns false when nothing moved.
pub(crate) fn align(o: &mut Compound, picked: &[Anchor], world: Affine, how: Align) -> bool {
    let mut items: Vec<(Anchor, Point)> = picked.iter().filter(|a| o.contains(**a)).map(|a| (*a, world * o.parts[a.part].outline.verts[a.vertex].p)).collect();
    let spread = matches!(how, Align::SpreadX | Align::SpreadY);
    if items.len() < 2 || (spread && items.len() < 3) || world.determinant().abs() < 1e-12 {
        return false;
    }
    let all = items.iter().fold(KRect::from_points(items[0].1, items[0].1), |b, it| b.union_pt(it.1));
    let along_x = matches!(how, Align::Left | Align::CenterX | Align::Right | Align::SpreadX);
    if spread {
        items.sort_by(|a, b| if along_x { a.1.x.total_cmp(&b.1.x) } else { a.1.y.total_cmp(&b.1.y) });
    }
    let last = (items.len() - 1) as f64;
    let c = world.inverse().as_coeffs();
    let mut moved = false;
    for (k, (a, at)) in items.into_iter().enumerate() {
        let t = k as f64 / last;
        let to = match how {
            Align::Left => all.x0,
            Align::CenterX => all.center().x,
            Align::Right => all.x1,
            Align::Top => all.y0,
            Align::CenterY => all.center().y,
            Align::Bottom => all.y1,
            Align::SpreadX => all.x0 + all.width() * t,
            Align::SpreadY => all.y0 + all.height() * t,
        };
        let d = if along_x { Vec2::new(to - at.x, 0.0) } else { Vec2::new(0.0, to - at.y) };
        if d.hypot() < 1e-9 {
            continue;
        }
        // The step on the artboard, in the outline's own space. Only the coordinates that
        // change are rounded, so a point keeps the other one as it was written.
        let own = Vec2::new(c[0] * d.x + c[2] * d.y, c[1] * d.x + c[3] * d.y);
        let v = &mut o.parts[a.part].outline.verts[a.vertex];
        let step = |was: f64, by: f64| if by.abs() < 1e-9 { was } else { ((was + by) * 100.0).round() / 100.0 };
        let to = Point::new(step(v.p.x, own.x), step(v.p.y, own.y));
        if to != v.p {
            v.translate(to - v.p);
            moved = true;
        }
    }
    moved
}

/// The frame and its handles over the canvas.
pub(crate) fn paint(painter: &egui::Painter, h: &Handles, line: Color32) {
    let dark = Color32::from_rgb(27, 29, 36);
    painter.add(egui::Shape::closed_line(h.corners.to_vec(), Stroke::new(1.0, line.gamma_multiply(0.7))));
    let top_mid = h.corners[0].lerp(h.corners[1], 0.5);
    painter.line_segment([top_mid, h.rotate], Stroke::new(1.0, line.gamma_multiply(0.7)));
    painter.circle(h.rotate, 4.0, dark, Stroke::new(1.5, Color32::WHITE));
    for s in h.sides.iter().flatten() {
        painter.rect(Rect::from_center_size(*s, vec2(6.0, 6.0)), 3.0, dark, Stroke::new(1.5, Color32::WHITE), StrokeKind::Middle);
    }
    for c in h.corners {
        painter.rect(Rect::from_center_size(c, vec2(7.0, 7.0)), 1.0, dark, Stroke::new(1.5, Color32::WHITE), StrokeKind::Middle);
    }
}

impl App {
    /// The align and spread buttons of the panel, for the selected points.
    pub(crate) fn align_points(&mut self, how: Align) {
        let Some(edit) = &self.point_edit else { return };
        let doc = self.display_doc();
        let Some(i) = doc.index_of(&edit.node) else { return };
        let Some(mut o) = Compound::from_bez(&doc.nodes[i].editable_path()) else { return };
        let picked = edit.anchors();
        if align(&mut o, &picked, doc.world_transform(i), how) {
            self.write_outline(&o);
        } else {
            self.status = "Точки уже стоят так".into();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pathedit::{Outline, Vertex};

    fn square() -> Compound {
        let verts = [(0.0, 0.0), (10.0, 0.0), (10.0, 20.0), (0.0, 20.0)].map(|(x, y)| Vertex { p: Point::new(x, y), cin: None, cout: None }).to_vec();
        Compound::from_bez(&Outline { verts, closed: true }.to_bez()).unwrap()
    }

    #[test]
    fn one_point_or_points_in_one_place_have_no_frame() {
        let o = square();
        let a = |vertex| Anchor { part: 0, vertex };
        assert!(bounds(&o, &[a(0)]).is_none());
        assert!(bounds(&o, &[a(0), a(0)]).is_none());
        assert_eq!(bounds(&o, &[a(0), a(1)]), Some(KRect::new(0.0, 0.0, 10.0, 0.0)));
        assert_eq!(bounds(&o, &[a(1), a(3)]), Some(KRect::new(0.0, 0.0, 10.0, 20.0)));
    }

    #[test]
    fn a_turn_is_made_as_the_eye_sees_it_in_a_stretched_object() {
        // The object is twice as wide as its own space. A quarter turn on the artboard of the
        // points (0,0) and (10,0), seen at (0,0) and (20,0), puts them at (10,-10) and (10,10)
        // there: (5,-10) and (5,10) in own space.
        let world = Affine::scale_non_uniform(2.0, 1.0);
        let b = KRect::new(0.0, 0.0, 10.0, 0.0);
        let pull = FramePull::new(Handle::Rotate, b, Point::new(5.0, -30.0), world);
        let (map, note) = pull.map(Point::new(20.0, 0.0), world, true, false);
        let mut o = square();
        apply(&mut o, &[Anchor { part: 0, vertex: 0 }, Anchor { part: 0, vertex: 1 }], map);
        let v = &o.parts[0].outline.verts;
        assert_eq!((v[0].p, v[1].p), (Point::new(5.0, -10.0), Point::new(5.0, 10.0)));
        assert_eq!(v[2].p, Point::new(10.0, 20.0), "points outside the selection stay");
        assert_eq!(note, "Точки: поворот 90.0°");
    }

    #[test]
    fn spread_puts_points_at_equal_steps_between_the_outer_ones() {
        let mut o = Compound::from_bez(&motif_core::kurbo::BezPath::from_svg("M 0 0 L 3 5 L 4 9 L 20 1").unwrap()).unwrap();
        let all: Vec<Anchor> = (0..4).map(|vertex| Anchor { part: 0, vertex }).collect();
        assert!(align(&mut o, &all, Affine::IDENTITY, Align::SpreadX));
        let xs: Vec<f64> = o.parts[0].outline.verts.iter().map(|v| v.p.x).collect();
        assert_eq!(xs, vec![0.0, 6.67, 13.33, 20.0]);
        assert!(!align(&mut o, &all, Affine::IDENTITY, Align::SpreadX), "already spread");
        assert!(align(&mut o, &all, Affine::IDENTITY, Align::Bottom));
        assert!(o.parts[0].outline.verts.iter().all(|v| v.p.y == 9.0));
    }
}
