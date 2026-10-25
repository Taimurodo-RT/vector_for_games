use crate::{Op, Paint as Fill, display_ops, faded};
use motif_core::kurbo::{Affine, PathEl};
use motif_core::{Blend, Cap, Color, Document, Join};
use tiny_skia::{FillRule, GradientStop, LinearGradient, Mask, Paint, PathBuilder, Pixmap, RadialGradient, SpreadMode, Stroke, Transform};

#[derive(Clone, Copy, Debug)]
pub struct CpuOptions {
    /// Output pixels per artboard unit.
    pub scale: f64,
    /// Smooth edges. Turn off for hard pixel edges before pixelization.
    pub antialias: bool,
    /// Paint the artboard background; when false the image keeps transparency.
    pub background: bool,
}

impl Default for CpuOptions {
    fn default() -> Self {
        CpuOptions { scale: 1.0, antialias: true, background: true }
    }
}

/// Renders the artboard into an RGBA image of `width * scale` by `height * scale` pixels.
pub fn render_cpu(doc: &Document, opts: CpuOptions) -> Option<Pixmap> {
    let w = (doc.artboard.width * opts.scale).round().max(1.0) as u32;
    let h = (doc.artboard.height * opts.scale).round().max(1.0) as u32;
    let mut pixmap = Pixmap::new(w, h)?;
    if opts.background {
        let c = doc.artboard.background;
        pixmap.fill(tiny_skia::Color::from_rgba8(c.r, c.g, c.b, c.a));
    }
    let view = Affine::scale(opts.scale);
    // Nested clip layers: each mask already includes the ones around it.
    let mut masks: Vec<Mask> = Vec::new();
    for op in display_ops(doc) {
        let item = match op {
            Op::Draw(item) => item,
            Op::PushClip { path, transform, even_odd } => {
                let mut mask = match masks.last() {
                    Some(m) => m.clone(),
                    None => {
                        let mut m = Mask::new(w, h)?;
                        m.data_mut().fill(255);
                        m
                    }
                };
                if let Some(p) = to_skia(&path) {
                    let rule = if even_odd { FillRule::EvenOdd } else { FillRule::Winding };
                    mask.intersect_path(&p, rule, opts.antialias, to_transform(view * transform));
                } else {
                    mask.data_mut().fill(0);
                }
                masks.push(mask);
                continue;
            }
            Op::Pop => {
                masks.pop();
                continue;
            }
        };
        let Some(path) = to_skia(&item.path) else { continue };
        let ts = to_transform(view * item.transform);
        let mask = masks.last();
        let mode = blend_mode(item.blend);
        if let Some(fill) = &item.fill
            && let Some(mut p) = fill_paint(fill, item.opacity, opts.antialias)
        {
            p.blend_mode = mode;
            pixmap.fill_path(&path, &p, FillRule::Winding, ts, mask);
        }
        if let Some((color, width)) = item.stroke {
            let line_join = match item.line.join {
                Join::Miter => tiny_skia::LineJoin::Miter,
                Join::Round => tiny_skia::LineJoin::Round,
                Join::Bevel => tiny_skia::LineJoin::Bevel,
            };
            let line_cap = match item.line.cap {
                Cap::Butt => tiny_skia::LineCap::Butt,
                Cap::Round => tiny_skia::LineCap::Round,
                Cap::Square => tiny_skia::LineCap::Square,
            };
            // tiny-skia wants an even number of dash lengths, like SVG repeats an odd list.
            let mut dash: Vec<f32> = item.line.dash.iter().map(|v| *v as f32).collect();
            if dash.len() % 2 == 1 {
                dash.extend(dash.clone());
            }
            let dash = if dash.is_empty() { None } else { tiny_skia::StrokeDash::new(dash, 0.0) };
            let stroke = Stroke { width: width as f32, line_join, line_cap, dash, ..Stroke::default() };
            let mut p = paint(faded(color, item.opacity), opts.antialias);
            p.blend_mode = mode;
            pixmap.stroke_path(&path, &p, &stroke, ts, mask);
        }
    }
    Some(pixmap)
}

fn fill_paint(fill: &Fill, opacity: f64, antialias: bool) -> Option<Paint<'static>> {
    let (gradient, from, to) = match fill {
        Fill::Solid(c) => return Some(paint(faded(*c, opacity), antialias)),
        Fill::Gradient { gradient, from, to } => (gradient, *from, *to),
    };
    let n = gradient.colors.len().max(2) - 1;
    let stops: Vec<GradientStop> = gradient
        .colors
        .iter()
        .enumerate()
        .map(|(i, c)| {
            let c = faded(*c, opacity);
            GradientStop::new(i as f32 / n as f32, tiny_skia::Color::from_rgba8(c.r, c.g, c.b, c.a))
        })
        .collect();
    let (a, b) = (tiny_skia::Point::from_xy(from.x as f32, from.y as f32), tiny_skia::Point::from_xy(to.x as f32, to.y as f32));
    let shader = if gradient.radial {
        RadialGradient::new(a, 0.0, a, a.distance(b), stops, SpreadMode::Pad, Transform::identity())?
    } else {
        LinearGradient::new(a, b, stops, SpreadMode::Pad, Transform::identity())?
    };
    Some(Paint { shader, anti_alias: antialias, ..Paint::default() })
}

fn blend_mode(b: Blend) -> tiny_skia::BlendMode {
    use tiny_skia::BlendMode as M;
    match b {
        Blend::Normal => M::SourceOver,
        Blend::Multiply => M::Multiply,
        Blend::Screen => M::Screen,
        Blend::Overlay => M::Overlay,
        Blend::Darken => M::Darken,
        Blend::Lighten => M::Lighten,
        Blend::ColorDodge => M::ColorDodge,
        Blend::ColorBurn => M::ColorBurn,
        Blend::HardLight => M::HardLight,
        Blend::SoftLight => M::SoftLight,
        Blend::Difference => M::Difference,
        Blend::Exclusion => M::Exclusion,
        Blend::Hue => M::Hue,
        Blend::Saturation => M::Saturation,
        Blend::Color => M::Color,
        Blend::Luminosity => M::Luminosity,
    }
}

fn paint(c: Color, antialias: bool) -> Paint<'static> {
    let mut p = Paint::default();
    p.set_color_rgba8(c.r, c.g, c.b, c.a);
    p.anti_alias = antialias;
    p
}

fn to_transform(a: Affine) -> Transform {
    let [sx, ky, kx, sy, tx, ty] = a.as_coeffs().map(|v| v as f32);
    Transform::from_row(sx, ky, kx, sy, tx, ty)
}

fn to_skia(path: &motif_core::kurbo::BezPath) -> Option<tiny_skia::Path> {
    let mut pb = PathBuilder::new();
    for el in path.elements() {
        match *el {
            PathEl::MoveTo(p) => pb.move_to(p.x as f32, p.y as f32),
            PathEl::LineTo(p) => pb.line_to(p.x as f32, p.y as f32),
            PathEl::QuadTo(a, p) => pb.quad_to(a.x as f32, a.y as f32, p.x as f32, p.y as f32),
            PathEl::CurveTo(a, b, p) => pb.cubic_to(a.x as f32, a.y as f32, b.x as f32, b.y as f32, p.x as f32, p.y as f32),
            PathEl::ClosePath => pb.close(),
        }
    }
    pb.finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    use motif_core::{Artboard, Node, ShapeKind};

    #[test]
    fn draws_shapes_where_they_are() {
        let mut doc = Document::new(Artboard { name: "t".into(), width: 40.0, height: 20.0, background: Color::WHITE });
        let mut n = Node::new("r", ShapeKind::Rect);
        n.x = 10.0;
        n.y = 10.0;
        n.width = 10.0;
        n.height = 10.0;
        n.fill = Some(Color::rgb(255, 0, 0));
        doc.nodes.push(n);
        let px = render_cpu(&doc, CpuOptions { scale: 2.0, antialias: false, background: true }).unwrap();
        assert_eq!((px.width(), px.height()), (80, 40));
        let at = |x, y| px.pixel(x, y).unwrap().demultiply();
        assert_eq!((at(20, 20).red(), at(20, 20).green()), (255, 0));
        assert_eq!((at(60, 20).red(), at(60, 20).green()), (255, 255));
    }

    #[test]
    fn masks_and_gradients() {
        let mut doc = Document::new(Artboard { name: "t".into(), width: 40.0, height: 20.0, background: Color::WHITE });
        let mut g = Node::new("g", ShapeKind::Group);
        g.clip = true;
        doc.nodes.push(g);
        // Mask: the left half only.
        let mut m = Node::new("m", ShapeKind::Rect);
        (m.x, m.y, m.width, m.height, m.parent) = (10.0, 10.0, 20.0, 20.0, Some(0));
        m.fill = Some(Color::rgb(0, 255, 0));
        doc.nodes.push(m);
        // A black-to-red gradient over the whole artboard, left to right.
        let mut r = Node::new("r", ShapeKind::Rect);
        (r.x, r.y, r.width, r.height, r.parent) = (20.0, 10.0, 40.0, 20.0, Some(0));
        r.gradient = Some(motif_core::Gradient { radial: false, angle: 0.0, colors: vec![Color::rgb(0, 0, 0), Color::rgb(255, 0, 0)] });
        doc.nodes.push(r);
        let px = render_cpu(&doc, CpuOptions { scale: 1.0, antialias: false, background: true }).unwrap();
        let at = |x, y| px.pixel(x, y).unwrap().demultiply();
        // Inside the mask: the gradient, darker on the left; the mask itself is not drawn.
        assert!(at(2, 10).red() < at(18, 10).red());
        assert_eq!(at(10, 10).green(), 0);
        // Outside the mask: background.
        assert_eq!((at(30, 10).red(), at(30, 10).green()), (255, 255));
    }
}
