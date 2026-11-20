//! What a drag shows in numbers: the distances from the dragged objects to their neighbours and
//! to the edges of the artboard, and a label at the pointer with the shift, size or angle.

use crate::canvas::View;
use egui::{Align2, Color32, FontId, Pos2, Rect, Stroke, vec2};
use motif_core::kurbo::{Point, Rect as KRect};
use motif_script::fmt_num;

/// Distances are drawn in this colour, apart from the pink of the alignment guides.
pub const MEASURE: Color32 = Color32::from_rgb(255, 122, 26);
/// Closer than this the objects touch, and the alignment guide says so already.
const TOUCH: f64 = 0.005;

/// A distance from a side of the dragged box to the nearest edge that faces it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Gap {
    pub from: Point,
    pub to: Point,
}

impl Gap {
    pub fn length(&self) -> f64 {
        self.from.distance(self.to)
    }
}

/// For each side of `b`: the nearest edge that faces it, among the boxes of `rects` that overlap
/// the side across. A box beside `b` gives the gap between them, a box that holds `b` the
/// distance to its inner edge (the padding). A box that only overlaps `b` faces no side of it.
pub fn gaps(rects: &[KRect], b: KRect) -> Vec<Gap> {
    let mut out = Vec::new();
    // (side of b, whether the side looks along x, direction)
    for (side, along_x, dir) in [(b.x0, true, -1.0), (b.x1, true, 1.0), (b.y0, false, -1.0), (b.y1, false, 1.0)] {
        let mut best: Option<(f64, f64, f64)> = None;
        for r in rects {
            let (lo, hi) = if along_x { (b.y0.max(r.y0), b.y1.min(r.y1)) } else { (b.x0.max(r.x0), b.x1.min(r.x1)) };
            if hi < lo {
                continue;
            }
            // Along the axis of the side: the extents of both boxes.
            let ((b0, b1), (r0, r1)) = if along_x { ((b.x0, b.x1), (r.x0, r.x1)) } else { ((b.y0, b.y1), (r.y0, r.y1)) };
            let holds = r0 <= b0 + TOUCH && r1 >= b1 - TOUCH;
            let (near, far) = if dir > 0.0 { (r0, r1) } else { (r1, r0) };
            let beside = (near - side) * dir >= -TOUCH;
            let edge = if beside {
                near
            } else if holds {
                far
            } else {
                continue;
            };
            let gap = (edge - side) * dir;
            if gap > TOUCH && best.is_none_or(|(g, _, _)| gap < g) {
                best = Some((gap, edge, (lo + hi) / 2.0));
            }
        }
        if let Some((_, edge, across)) = best {
            out.push(if along_x {
                Gap { from: Point::new(side, across), to: Point::new(edge, across) }
            } else {
                Gap { from: Point::new(across, side), to: Point::new(across, edge) }
            });
        }
    }
    out
}

fn pill(painter: &egui::Painter, at: Pos2, text: &str, fill: Color32) {
    let font = FontId::proportional(10.5);
    let galley = painter.layout_no_wrap(text.to_string(), font, Color32::WHITE);
    let r = Rect::from_center_size(at, galley.size() + vec2(8.0, 3.0));
    painter.rect_filled(r, 3.0, fill);
    painter.galley(r.center() - galley.size() / 2.0, galley, Color32::WHITE);
}

pub fn paint_gaps(painter: &egui::Painter, rect: Rect, view: View, gaps: &[Gap]) {
    for g in gaps {
        let (a, b) = (view.to_screen(rect, g.from), view.to_screen(rect, g.to));
        let stroke = Stroke::new(1.0, MEASURE);
        painter.line_segment([a, b], stroke);
        let across = if (a.x - b.x).abs() > (a.y - b.y).abs() { vec2(0.0, 3.0) } else { vec2(3.0, 0.0) };
        painter.line_segment([a - across, a + across], stroke);
        painter.line_segment([b - across, b + across], stroke);
        // A short line has no room for its number: the number goes beside it.
        let beside = if a.distance(b) < 34.0 { across * 4.0 } else { egui::Vec2::ZERO };
        pill(painter, a.lerp(b, 0.5) + beside, &fmt_num(g.length()), MEASURE);
    }
}

/// The note that follows the pointer during a drag.
pub fn paint_pointer_label(painter: &egui::Painter, pointer: Pos2, text: &str) {
    let font = FontId::proportional(11.0);
    let galley = painter.layout_no_wrap(text.to_string(), font, Color32::WHITE);
    let r = Align2::LEFT_TOP.anchor_size(pointer + vec2(14.0, 18.0), galley.size() + vec2(10.0, 6.0));
    painter.rect_filled(r, 4.0, Color32::from_black_alpha(210));
    painter.galley(r.min + vec2(5.0, 3.0), galley, Color32::WHITE);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gaps_go_to_the_neighbour_beside_and_to_the_inner_edges_of_the_box_around() {
        let board = KRect::new(0.0, 0.0, 400.0, 300.0);
        let panel = KRect::new(40.0, 40.0, 260.0, 180.0);
        let other = KRect::new(200.0, 60.0, 240.0, 100.0);
        // A button inside the panel, level with `other`.
        let b = KRect::new(60.0, 70.0, 120.0, 90.0);
        let found = gaps(&[board, panel, other], b);
        let lengths: Vec<f64> = found.iter().map(Gap::length).collect();
        // Left and top: the padding of the panel. Right: the neighbour. Bottom: the panel again.
        assert_eq!(lengths, vec![20.0, 80.0, 30.0, 90.0]);
        assert_eq!(found[1], Gap { from: Point::new(120.0, 80.0), to: Point::new(200.0, 80.0) });
        // Touching boxes and boxes that do not face the side give nothing.
        assert!(gaps(&[KRect::new(120.0, 70.0, 150.0, 90.0)], b).is_empty());
        assert!(gaps(&[KRect::new(200.0, 200.0, 240.0, 240.0)], b).is_empty());
        // A box that only overlaps faces no side: nothing to measure to.
        assert!(gaps(&[KRect::new(100.0, 80.0, 180.0, 130.0)], b).is_empty());
    }
}
