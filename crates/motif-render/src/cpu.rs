use crate::{BackdropView, Op, Paint as Fill, display_ops, faded};
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
    render_cpu_with(doc, opts, None)
}

/// [`render_cpu`] with a reference picture over the background, under the shapes.
pub fn render_cpu_with(doc: &Document, opts: CpuOptions, backdrop: Option<BackdropView>) -> Option<Pixmap> {
    let w = (doc.artboard.width * opts.scale).round().max(1.0) as u32;
    let h = (doc.artboard.height * opts.scale).round().max(1.0) as u32;
    let mut pixmap = Pixmap::new(w, h)?;
    if opts.background {
        let c = doc.artboard.background;
        pixmap.fill(tiny_skia::Color::from_rgba8(c.r, c.g, c.b, c.a));
    }
    let view = Affine::scale(opts.scale);
    if let Some(b) = backdrop {
        let paint = tiny_skia::PixmapPaint { opacity: b.opacity.clamp(0.0, 1.0) as f32, quality: tiny_skia::FilterQuality::Bilinear, ..Default::default() };
        pixmap.draw_pixmap(0, 0, b.image.pixmap.as_ref().as_ref(), &paint, to_transform(view * b.transform), None);
    }
    draw_ops(&mut pixmap, &display_ops(doc), view, opts.antialias, None);
    Some(pixmap)
}

/// Renders `region` of the scene (artboard units, which may reach past the artboard) into a
/// transparent image of `region * scale` pixels: one object with its shadow, for example.
pub fn render_region(doc: &Document, region: motif_core::kurbo::Rect, opts: CpuOptions) -> Option<Pixmap> {
    let w = (region.width() * opts.scale).round().max(1.0) as u32;
    let h = (region.height() * opts.scale).round().max(1.0) as u32;
    let mut pixmap = Pixmap::new(w, h)?;
    let view = Affine::scale(opts.scale) * Affine::translate(-region.origin().to_vec2());
    draw_ops(&mut pixmap, &display_ops(doc), view, opts.antialias, None);
    Some(pixmap)
}

/// Paints ops into `pixmap` under `view`, inside `outer` when given.
pub(crate) fn draw_ops(pixmap: &mut Pixmap, ops: &[Op], view: Affine, antialias: bool, outer: Option<&Mask>) {
    let (w, h) = (pixmap.width(), pixmap.height());
    let canvas = motif_core::kurbo::Rect::new(0.0, 0.0, w as f64, h as f64);
    // Nested clip layers: each mask already includes the ones around it.
    let mut masks: Vec<Mask> = Vec::new();
    for op in ops {
        let item = match op {
            Op::Draw(item) => item,
            Op::PushClip { path, transform, even_odd } => {
                let mut mask = match masks.last().or(outer) {
                    Some(m) => m.clone(),
                    None => {
                        let Some(mut m) = Mask::new(w, h) else { return };
                        m.data_mut().fill(255);
                        m
                    }
                };
                if let Some(p) = to_skia(path) {
                    let rule = if *even_odd { FillRule::EvenOdd } else { FillRule::Winding };
                    mask.intersect_path(&p, rule, antialias, to_transform(view * *transform));
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
            Op::Blur { ops, radius } => {
                if let Some(layer) = crate::blur::render(ops, *radius, view, antialias, Some(canvas)) {
                    let paint = tiny_skia::PixmapPaint { quality: tiny_skia::FilterQuality::Bilinear, ..Default::default() };
                    pixmap.draw_pixmap(0, 0, layer.pixmap.as_ref(), &paint, to_transform(layer.transform), masks.last().or(outer));
                }
                continue;
            }
        };
        let Some(path) = to_skia(&item.path) else { continue };
        let ts = to_transform(view * item.transform);
        let mask = masks.last().or(outer);
        let mode = blend_mode(item.blend);
        if let Some(fill) = &item.fill
            && let Some(mut p) = fill_paint(fill, item.opacity, antialias)
        {
            p.blend_mode = mode;
            pixmap.fill_path(&path, &p, if item.even_odd { FillRule::EvenOdd } else { FillRule::Winding }, ts, mask);
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
            let mut p = paint(faded(color, item.opacity), antialias);
            p.blend_mode = mode;
            pixmap.stroke_path(&path, &p, &stroke, ts, mask);
        }
    }
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
    fn animated_effect_colors_fade_pixels_without_changing_their_offsets() {
        use motif_core::{Animation, Ease, Keyframe, Prop, Shadow, Track, Value};
        let color = Color { a: 200, ..Color::BLACK };
        let animation = |prop| Animation {
            name: "Fade".into(),
            duration: 1.0,
            looping: false,
            events: vec![],
            tracks: vec![Track {
                node: "square".into(),
                prop,
                keys: vec![
                    Keyframe { time: 0.0, value: Value::None, ease: Ease::Linear },
                    Keyframe { time: 1.0, value: Value::Color(color), ease: Ease::Linear },
                ],
            }],
        };
        let mut doc = Document::new(Artboard { name: "effects".into(), width: 100.0, height: 100.0, background: Color::WHITE });
        let node = square(&mut doc, 30.0, 30.0, 20.0);
        node.id = "square".into();
        node.fill = Some(Color::WHITE);
        node.shadow = Some(Shadow { dx: 30.0, dy: 0.0, blur: 0.0, color });
        let anim = animation(Prop::ShadowColor);
        for (t, alpha) in [(0.0, 0), (0.25, 50), (0.5, 100), (1.0, 200)] {
            let px = render_cpu(&anim.pose(&doc, t), CpuOptions { background: false, antialias: false, ..Default::default() }).unwrap();
            assert_eq!(px.pixel(60, 30).unwrap().alpha(), alpha, "outer shadow t={t}");
        }
        let mut doc = Document::new(Artboard { name: "inset".into(), width: 100.0, height: 100.0, background: Color::WHITE });
        let node = square(&mut doc, 50.0, 50.0, 40.0);
        node.id = "square".into();
        node.fill = Some(Color::WHITE);
        node.inset = Some(Shadow { dx: 10.0, dy: 0.0, blur: 0.0, color });
        let anim = animation(Prop::InsetColor);
        for (t, red) in [(0.0, 255), (0.25, 205), (0.5, 155), (1.0, 55)] {
            let px = render_cpu(&anim.pose(&doc, t), CpuOptions { background: false, antialias: false, ..Default::default() }).unwrap();
            assert_eq!(px.pixel(33, 50).unwrap().red(), red, "inner shadow t={t}");
        }
    }

    #[test]
    fn a_previously_absent_outline_fades_visibly_before_its_final_key() {
        use motif_core::{Animation, Ease, Keyframe, Prop, Track, Value};
        let mut doc = Document::new(Artboard { name: "Outline".into(), width: 48.0, height: 40.0, background: Color::TRANSPARENT });
        let mut button = Node::new("button", ShapeKind::Rect);
        button.x = 24.0;
        button.y = 20.0;
        button.width = 20.0;
        button.height = 20.0;
        button.fill = None;
        doc.nodes.push(button);
        let ink = Color { r: 40, g: 120, b: 230, a: 200 };
        let mut anim = Animation {
            name: "Hover".into(),
            duration: 1.0,
            looping: false,
            events: vec![],
            tracks: vec![Track {
                node: "button".into(),
                prop: Prop::StrokeColor,
                keys: vec![
                    Keyframe { time: 0.0, value: Value::None, ease: Ease::Linear },
                    Keyframe { time: 1.0, value: Value::Color(ink), ease: Ease::Linear },
                ],
            }],
        };
        let alpha = |anim: &Animation, time| {
            let pose = anim.pose(&doc, time);
            let px = render_cpu(&pose, CpuOptions { background: false, antialias: false, ..Default::default() }).unwrap();
            px.pixel(13, 20).unwrap().alpha()
        };
        assert_eq!([alpha(&anim, 0.0), alpha(&anim, 0.25), alpha(&anim, 0.5), alpha(&anim, 1.0)], [0, 50, 100, 200]);
        anim.tracks[0].keys[0].value = Value::Color(ink);
        anim.tracks[0].keys[1].value = Value::None;
        assert_eq!([alpha(&anim, 0.0), alpha(&anim, 0.5), alpha(&anim, 1.0)], [200, 100, 0]);
        anim.tracks[0].keys[0].value = Value::None;
        anim.tracks[0].keys[1].value = Value::Color(ink);
        anim.tracks[0].keys[0].ease = Ease::Hold;
        assert_eq!([alpha(&anim, 0.5), alpha(&anim, 1.0)], [0, 200], "hold remains an intentional appearance jump");
        assert_eq!(doc.nodes[0].stroke, None);
        assert_eq!(doc.nodes[0].stroke_width, 0.0);
    }

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
    fn objects_behind_a_ball_are_hidden_by_it() {
        let mut doc = Document::new(Artboard { name: "t".into(), width: 60.0, height: 20.0, background: Color::WHITE });
        let mut ball = Node::new("ball", ShapeKind::Ellipse);
        (ball.x, ball.y, ball.width, ball.height, ball.yaw) = (30.0, 10.0, 20.0, 20.0, 180.0);
        ball.fill = Some(Color::rgb(0, 0, 255));
        // A wide bar sitting on the front of the ball, turned to the back by the ball's yaw.
        let mut bar = Node::new("bar", ShapeKind::Rect);
        (bar.width, bar.height) = (50.0, 6.0);
        bar.on = Some("ball".into());
        bar.fill = Some(Color::rgb(255, 0, 0));
        doc.nodes = vec![ball, bar];
        doc.rig();
        assert_eq!(doc.nodes[1].behind, Some((0, false)));
        let px = render_cpu(&doc, CpuOptions { scale: 1.0, antialias: false, background: true }).unwrap();
        let at = |x, y| px.pixel(x, y).unwrap().demultiply();
        // Over the ball the ball shows; beside it the bar does.
        assert_eq!((at(30, 10).red(), at(30, 10).blue()), (0, 255));
        assert_eq!((at(8, 10).red(), at(8, 10).blue()), (255, 0));
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

    #[test]
    fn backdrop_under_shapes() {
        let mut doc = Document::new(Artboard { name: "t".into(), width: 20.0, height: 10.0, background: Color::WHITE });
        let mut n = Node::new("r", ShapeKind::Rect);
        (n.x, n.y, n.width, n.height) = (15.0, 5.0, 10.0, 10.0);
        n.fill = Some(Color::rgb(0, 0, 255));
        doc.nodes.push(n);
        let mut pic = Pixmap::new(2, 1).unwrap();
        pic.fill(tiny_skia::Color::from_rgba8(255, 0, 0, 255));
        let bd = crate::Backdrop::new(pic);
        // Stretched over the whole artboard at half strength.
        let view = crate::BackdropView { image: &bd, transform: Affine::scale_non_uniform(10.0, 10.0), opacity: 0.5 };
        let px = render_cpu_with(&doc, CpuOptions { scale: 1.0, antialias: false, background: true }, Some(view)).unwrap();
        let at = |x, y| px.pixel(x, y).unwrap().demultiply();
        let c = at(4, 5);
        assert!(c.red() == 255 && (120..=135).contains(&c.green()), "{c:?}");
        assert_eq!((at(15, 5).red(), at(15, 5).blue()), (0, 255));
        // Exports never pass one.
        assert_eq!(render_cpu(&doc, CpuOptions::default()).unwrap().pixel(4, 5).unwrap().demultiply().green(), 255);
    }

    fn square(doc: &mut Document, x: f64, y: f64, size: f64) -> &mut Node {
        let mut n = Node::new(format!("n{}", doc.nodes.len()), ShapeKind::Rect);
        (n.x, n.y, n.width, n.height) = (x, y, size, size);
        n.fill = Some(Color::rgb(0, 0, 0));
        doc.nodes.push(n);
        doc.nodes.last_mut().unwrap()
    }

    #[test]
    fn blur_softens_edges_evenly() {
        let mut doc = Document::new(Artboard { name: "t".into(), width: 100.0, height: 100.0, background: Color::WHITE });
        square(&mut doc, 50.0, 50.0, 40.0).blur = 10.0;
        let px = render_cpu(&doc, CpuOptions::default()).unwrap();
        let red = |x, y| px.pixel(x, y).unwrap().demultiply().red();
        // Solid in the middle, half way at the edge, fading out over the radius, symmetric.
        assert!(red(50, 50) < 10);
        assert!((100..=155).contains(&red(30, 50)), "{}", red(30, 50));
        assert!(red(22, 50) > red(30, 50) && red(22, 50) < 250);
        assert!(red(12, 50) > 250);
        assert!((red(30, 50) as i32 - red(69, 50) as i32).abs() <= 2);
        // Scaling the output scales the blur with it.
        let big = render_cpu(&doc, CpuOptions { scale: 2.0, ..CpuOptions::default() }).unwrap();
        assert!((big.pixel(44, 100).unwrap().demultiply().red() as i32 - red(22, 50) as i32).abs() <= 6);
    }

    #[test]
    fn soft_shadow_and_elevation() {
        let mut doc = Document::new(Artboard { name: "t".into(), width: 100.0, height: 100.0, background: Color::WHITE });
        let n = square(&mut doc, 50.0, 50.0, 20.0);
        n.fill = Some(Color::rgb(255, 0, 0));
        n.shadow = Some(motif_core::Shadow { dx: 0.0, dy: 20.0, blur: 6.0, color: Color::rgb(0, 0, 255) });
        let px = render_cpu(&doc, CpuOptions::default()).unwrap();
        let at = |x, y| px.pixel(x, y).unwrap().demultiply();
        // The shadow is below the shape and soft; the shape stays on top.
        assert_eq!((at(50, 50).red(), at(50, 50).blue()), (255, 0));
        assert!(at(50, 70).red() < 30 && at(50, 70).blue() > 200);
        let edge = at(50, 80).red();
        assert!((60..=200).contains(&edge), "{edge}");
        // Height alone casts a shadow away from the light: light 0 sends it to the right.
        doc.nodes[0].shadow = None;
        doc.nodes[0].elevation = 15.0;
        doc.light = 0.0;
        let px = render_cpu(&doc, CpuOptions::default()).unwrap();
        let at = |x, y| px.pixel(x, y).unwrap().demultiply();
        assert!(at(68, 50).red() < 240, "{:?}", at(68, 50));
        assert_eq!(at(32, 50).red(), 255);
    }

    #[test]
    fn group_blurs_children_together() {
        let mut doc = Document::new(Artboard { name: "t".into(), width: 100.0, height: 40.0, background: Color::WHITE });
        let mut g = Node::new("g", ShapeKind::Group);
        g.blur = 8.0;
        doc.nodes.push(g);
        square(&mut doc, 30.0, 20.0, 20.0).parent = Some(0);
        square(&mut doc, 70.0, 20.0, 20.0).parent = Some(0);
        square(&mut doc, 50.0, 20.0, 4.0);
        let px = render_cpu(&doc, CpuOptions { antialias: false, ..CpuOptions::default() }).unwrap();
        let red = |x, y| px.pixel(x, y).unwrap().demultiply().red();
        assert!(red(19, 20) > 30 && red(19, 20) < 220);
        assert!(red(81, 20) > 30 && red(81, 20) < 220);
        // The square outside the group stays sharp.
        assert_eq!(red(50, 20), 0);
        assert!(red(5, 20) > 250);
    }

    #[test]
    fn interface_effects() {
        let mut doc = Document::new(Artboard { name: "t".into(), width: 200.0, height: 100.0, background: Color::BLACK });
        // A glow lights the dark around the shape, evenly on all sides, and fades out.
        let lamp = square(&mut doc, 50.0, 50.0, 40.0);
        lamp.fill = Some(Color::rgb(0, 0, 255));
        lamp.glow = Some(motif_core::Glow { size: 10.0, spread: 0.0, color: Color::rgb(255, 255, 0) });
        // An inner shadow darkens the inside along the top edge (offset down), not the middle.
        let well = square(&mut doc, 150.0, 50.0, 60.0);
        well.fill = Some(Color::WHITE);
        well.inset = Some(motif_core::Shadow { dx: 0.0, dy: 6.0, blur: 4.0, color: Color::BLACK });
        let px = render_cpu(&doc, CpuOptions::default()).unwrap();
        let at = |x, y| px.pixel(x, y).unwrap().demultiply();
        assert!(at(25, 50).red() > 30 && at(75, 50).red() > 30 && at(50, 25).red() > 30, "{:?}", at(25, 50));
        assert!((at(25, 50).red() as i32 - at(74, 50).red() as i32).abs() <= 3);
        assert!(at(5, 50).red() < 10);
        assert_eq!((at(50, 50).red(), at(50, 50).blue()), (0, 255));
        assert!(at(150, 22).red() < 120, "{:?}", at(150, 22));
        assert!(at(150, 60).red() > 250);
        assert!(at(150, 78).red() > 240, "the shadow falls from the top: {:?}", at(150, 78));

        // A gradient stroke runs over the shape: light at the top, dark at the bottom.
        let mut doc = Document::new(Artboard { name: "t".into(), width: 100.0, height: 100.0, background: Color::BLACK });
        let frame = square(&mut doc, 50.0, 50.0, 60.0);
        frame.stroke_width = 6.0;
        frame.stroke_gradient = Some(motif_core::Gradient { radial: false, angle: 90.0, colors: vec![Color::WHITE, Color::rgb(0, 0, 255)] });
        // Grain: specks lighter and darker than the fill, the same in every render.
        let grain = square(&mut doc, 50.0, 50.0, 30.0);
        grain.fill = Some(Color::rgb(128, 128, 128));
        grain.noise = Some(motif_core::Noise { amount: 0.5, size: 1.0 });
        let px = render_cpu(&doc, CpuOptions::default()).unwrap();
        let at = |x, y| px.pixel(x, y).unwrap().demultiply();
        assert!(at(50, 20).red() > 200 && at(50, 80).red() < 60, "{:?} {:?}", at(50, 20), at(50, 80));
        assert!(at(50, 80).blue() > 150);
        // Inside the frame there is no stroke.
        assert!(at(30, 30).red() < 5);
        let reds: Vec<u8> = (36..64).map(|x| at(x, 50).red()).collect();
        assert!(reds.iter().any(|&r| r > 150) && reds.iter().any(|&r| r < 100), "{reds:?}");
        let again = render_cpu(&doc, CpuOptions::default()).unwrap();
        assert_eq!(px.data(), again.data());
    }
}
