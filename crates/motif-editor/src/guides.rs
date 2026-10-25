//! Smart guides while moving objects, the grid over the artboard, and the rulers along the canvas.

use crate::canvas::View;
use crate::theme;
use egui::{Align2, Color32, Rect, Stroke, pos2, vec2};
use motif_core::Document;
use motif_core::kurbo::{self, Point, Vec2};

/// Guide lines are drawn in this colour, like in most design tools.
pub const GUIDE: Color32 = Color32::from_rgb(255, 64, 160);
/// How close, in screen points, an edge must come to a guide to stick to it.
const REACH: f64 = 6.0;
pub const RULER: f32 = 18.0;

/// Edges and centres the moving objects can line up with, in artboard space.
#[derive(Clone, Debug, Default)]
pub struct Targets {
    /// Each value with the vertical extent of what it belongs to, so the guide can span both.
    xs: Vec<(f64, f64, f64)>,
    ys: Vec<(f64, f64, f64)>,
}

/// A guide to draw: vertical (`x` set) or horizontal, from `a` to `b` along it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Guide {
    pub vertical: bool,
    pub at: f64,
    pub a: f64,
    pub b: f64,
}

impl Targets {
    /// Every visible object that is not moving (nor inside or around a moving one), and the artboard.
    pub fn collect(doc: &Document, moving: &[usize]) -> Targets {
        let related = |i: usize| {
            moving.iter().any(|&m| {
                let up = |mut a: usize, b: usize| loop {
                    if a == b {
                        return true;
                    }
                    match doc.nodes[a].parent {
                        Some(p) => a = p,
                        None => return false,
                    }
                };
                up(i, m) || up(m, i)
            })
        };
        let mut t = Targets::default();
        let (w, h) = (doc.artboard.width, doc.artboard.height);
        t.add(kurbo::Rect::new(0.0, 0.0, w, h));
        for i in 0..doc.nodes.len() {
            if doc.is_visible(i)
                && !related(i)
                && let Some(b) = crate::selection::world_bounds(doc, i)
            {
                t.add(b);
            }
        }
        t
    }

    fn add(&mut self, b: kurbo::Rect) {
        for x in [b.x0, b.center().x, b.x1] {
            self.xs.push((x, b.y0, b.y1));
        }
        for y in [b.y0, b.center().y, b.y1] {
            self.ys.push((y, b.x0, b.x1));
        }
    }

    /// Nudges `delta` so an edge or the centre of `moving` (moved by `delta`) lands on a guide
    /// within reach on each axis. Returns the corrected delta, which axes snapped, and the guides.
    pub fn snap(&self, moving: kurbo::Rect, delta: Vec2, zoom: f32) -> (Vec2, [bool; 2], Vec<Guide>) {
        let reach = REACH / zoom.max(0.01) as f64;
        let b = moving + delta;
        let best = |own: [f64; 3], list: &[(f64, f64, f64)]| {
            let mut found: Option<f64> = None;
            for o in own {
                for (v, _, _) in list {
                    let d = v - o;
                    if d.abs() <= reach && found.is_none_or(|f| d.abs() < f.abs()) {
                        found = Some(d);
                    }
                }
            }
            found
        };
        let dx = best([b.x0, b.center().x, b.x1], &self.xs);
        let dy = best([b.y0, b.center().y, b.y1], &self.ys);
        let d = delta + Vec2::new(dx.unwrap_or(0.0), dy.unwrap_or(0.0));
        let b = moving + d;
        let mut guides = Vec::new();
        // Every line the snapped box now touches, spanning the box and what it lines up with.
        let near = |a: f64, b: f64| (a - b).abs() < 1e-6;
        if dx.is_some() {
            for o in [b.x0, b.center().x, b.x1] {
                let hits: Vec<_> = self.xs.iter().filter(|(v, _, _)| near(*v, o)).collect();
                if !hits.is_empty() {
                    let a = hits.iter().map(|h| h.1).fold(b.y0, f64::min);
                    let z = hits.iter().map(|h| h.2).fold(b.y1, f64::max);
                    guides.push(Guide { vertical: true, at: o, a, b: z });
                }
            }
        }
        if dy.is_some() {
            for o in [b.y0, b.center().y, b.y1] {
                let hits: Vec<_> = self.ys.iter().filter(|(v, _, _)| near(*v, o)).collect();
                if !hits.is_empty() {
                    let a = hits.iter().map(|h| h.1).fold(b.x0, f64::min);
                    let z = hits.iter().map(|h| h.2).fold(b.x1, f64::max);
                    guides.push(Guide { vertical: false, at: o, a, b: z });
                }
            }
        }
        (d, [dx.is_some(), dy.is_some()], guides)
    }
}

pub fn paint_guides(painter: &egui::Painter, rect: Rect, view: View, guides: &[Guide]) {
    for g in guides {
        let (p, q) = if g.vertical { (Point::new(g.at, g.a), Point::new(g.at, g.b)) } else { (Point::new(g.a, g.at), Point::new(g.b, g.at)) };
        painter.line_segment([view.to_screen(rect, p), view.to_screen(rect, q)], Stroke::new(1.0, GUIDE));
    }
}

/// Grid lines over the artboard every `step` units; skipped when they would be too dense.
pub fn paint_grid(painter: &egui::Painter, rect: Rect, view: View, doc: &Document, step: f64) {
    if step <= 0.0 || step as f32 * view.zoom < 5.0 {
        return;
    }
    let (w, h) = (doc.artboard.width, doc.artboard.height);
    let color = Color32::from_rgba_unmultiplied(120, 140, 255, 46);
    let strong = Color32::from_rgba_unmultiplied(120, 140, 255, 90);
    let stroke = |k: i64| Stroke::new(1.0, if k % 4 == 0 { strong } else { color });
    let mut k = 1;
    while k as f64 * step < w {
        let x = k as f64 * step;
        painter.line_segment([view.to_screen(rect, Point::new(x, 0.0)), view.to_screen(rect, Point::new(x, h))], stroke(k));
        k += 1;
    }
    let mut k = 1;
    while k as f64 * step < h {
        let y = k as f64 * step;
        painter.line_segment([view.to_screen(rect, Point::new(0.0, y)), view.to_screen(rect, Point::new(w, y))], stroke(k));
        k += 1;
    }
}

/// A tick spacing of 1, 2 or 5 times a power of ten that leaves at least `min_px` between labels.
fn tick_step(zoom: f32, min_px: f32) -> f64 {
    let raw = (min_px / zoom.max(1e-4)) as f64;
    let p = 10f64.powf(raw.log10().floor());
    [1.0, 2.0, 5.0, 10.0].iter().map(|m| m * p).find(|s| *s >= raw).unwrap_or(10.0 * p)
}

/// Rulers along the top and left of the canvas in artboard units, with the pointer and the
/// selection marked on them.
pub fn paint_rulers(painter: &egui::Painter, rect: Rect, view: View, pointer: Option<Point>, span: Option<kurbo::Rect>) {
    let top = Rect::from_min_max(rect.min, pos2(rect.right(), rect.top() + RULER));
    let left = Rect::from_min_max(pos2(rect.left(), rect.top() + RULER), pos2(rect.left() + RULER, rect.bottom()));
    for r in [top, left] {
        painter.rect_filled(r, 0.0, theme::PANEL);
    }
    painter.line_segment([top.left_bottom(), top.right_bottom()], Stroke::new(1.0, theme::LINE));
    painter.line_segment([left.right_top(), left.right_bottom()], Stroke::new(1.0, theme::LINE));
    // The selection's extent as a band.
    if let Some(s) = span {
        let a = view.to_screen(rect, Point::new(s.x0, s.y0));
        let b = view.to_screen(rect, Point::new(s.x1, s.y1));
        let band = theme::SEL.gamma_multiply(0.25);
        painter.rect_filled(Rect::from_x_y_ranges(a.x.max(top.left() + RULER)..=b.x.min(top.right()), top.y_range()).intersect(top), 0.0, band);
        painter.rect_filled(Rect::from_x_y_ranges(left.x_range(), a.y.max(left.top())..=b.y.min(left.bottom())).intersect(left), 0.0, band);
    }
    let step = tick_step(view.zoom, 56.0);
    let minor = step / 5.0;
    let font = theme::ui_font(10.0);
    let tick = Stroke::new(1.0, theme::FAINT);
    // Horizontal ruler.
    let from = view.to_doc(rect, pos2(top.left() + RULER, 0.0)).x;
    let to = view.to_doc(rect, pos2(top.right(), 0.0)).x;
    let mut v = (from / minor).floor() * minor;
    while v <= to {
        let x = view.to_screen(rect, Point::new(v, 0.0)).x;
        let major = ((v / step).round() * step - v).abs() < minor * 0.01;
        if x >= top.left() + RULER {
            let len = if major { 8.0 } else { 3.0 };
            painter.line_segment([pos2(x, top.bottom() - len), pos2(x, top.bottom())], tick);
            if major {
                painter.text(pos2(x + 3.0, top.top() + 1.0), Align2::LEFT_TOP, motif_script::fmt_num(v), font.clone(), theme::MUTED);
            }
        }
        v += minor;
    }
    // Vertical ruler, labels written sideways would need rotation: short numbers fit across instead.
    let from = view.to_doc(rect, pos2(0.0, left.top())).y;
    let to = view.to_doc(rect, pos2(0.0, left.bottom())).y;
    let mut v = (from / minor).floor() * minor;
    while v <= to {
        let y = view.to_screen(rect, Point::new(0.0, v)).y;
        let major = ((v / step).round() * step - v).abs() < minor * 0.01;
        if y >= left.top() {
            let len = if major { 8.0 } else { 3.0 };
            painter.line_segment([pos2(left.right() - len, y), pos2(left.right(), y)], tick);
            if major {
                let label = motif_script::fmt_num(v);
                // Digits stacked so the label fits the narrow strip.
                for (k, ch) in label.chars().enumerate() {
                    painter.text(pos2(left.center().x, y + 2.0 + k as f32 * 9.0), Align2::CENTER_TOP, ch, font.clone(), theme::MUTED);
                }
            }
        }
        v += minor;
    }
    if let Some(p) = pointer {
        let s = view.to_screen(rect, p);
        let mark = Stroke::new(1.0, GUIDE);
        if s.x >= top.left() + RULER {
            painter.line_segment([pos2(s.x, top.top()), pos2(s.x, top.bottom())], mark);
        }
        if s.y >= left.top() {
            painter.line_segment([pos2(left.left(), s.y), pos2(left.right(), s.y)], mark);
        }
    }
    // The corner square.
    painter.rect_filled(Rect::from_min_size(rect.min, vec2(RULER, RULER)), 0.0, theme::PANEL);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edges_stick_within_reach() {
        let mut t = Targets::default();
        t.add(kurbo::Rect::new(0.0, 0.0, 100.0, 100.0));
        let moving = kurbo::Rect::new(200.0, 200.0, 220.0, 220.0);
        // The left edge comes 3 units from x = 100 at zoom 1: it sticks; y stays free.
        let (d, stuck, guides) = t.snap(moving, Vec2::new(-97.0, -130.0), 1.0);
        assert_eq!((d.x, stuck), (-100.0, [true, false]));
        assert!(guides.iter().all(|g| g.vertical && g.at == 100.0));
        // Zoomed out the same screen reach covers more units; zoomed in, less.
        assert!(!t.snap(moving, Vec2::new(-97.0, -130.0), 4.0).1[0]);
        // Centres line up too: 210 -> 50 on y.
        let (d, stuck, _) = t.snap(moving, Vec2::new(-130.0, -158.0), 1.0);
        assert_eq!((d.y, stuck[1]), (-160.0, true));
    }
}
