use crate::{BackdropView, Op, Paint, display_ops, faded};
use motif_core::kurbo::{Affine, Rect};
use motif_core::{Blend, Cap, Color, Document, Join};
pub use vello;
use vello::Scene;
use vello::peniko::{self, Fill};

fn color(c: Color) -> peniko::Color {
    peniko::Color::from_rgba8(c.r, c.g, c.b, c.a)
}

/// Appends the artboard (background, then shapes clipped to it) to a Vello scene.
/// `view` maps artboard space to target pixels (pan and zoom).
/// A `backdrop` is drawn over the background, under the shapes.
pub fn build_scene(scene: &mut Scene, doc: &Document, view: Affine, backdrop: Option<BackdropView>) {
    let bounds = Rect::new(0.0, 0.0, doc.artboard.width, doc.artboard.height);
    scene.fill(Fill::NonZero, view, color(doc.artboard.background), None, &bounds);
    scene.push_clip_layer(Fill::NonZero, view, &bounds);
    if let Some(b) = backdrop
        && b.opacity > 0.0
    {
        let brush = peniko::ImageBrush::new(b.image.image.clone()).with_alpha(b.opacity as f32);
        scene.draw_image(&brush, view * b.transform);
    }
    for op in display_ops(doc) {
        let item = match op {
            Op::Draw(item) => item,
            Op::PushClip { path, transform, even_odd } => {
                scene.push_clip_layer(if even_odd { Fill::EvenOdd } else { Fill::NonZero }, view * transform, &path);
                continue;
            }
            Op::Pop => {
                scene.pop_layer();
                continue;
            }
        };
        let t = view * item.transform;
        // A blend mode needs its own layer, bounded by the shape (with room for the stroke).
        let blended = item.blend != Blend::Normal;
        if blended {
            let pad = item.stroke.map_or(0.0, |s| s.1);
            let bounds = motif_core::kurbo::Shape::bounding_box(&item.path).inflate(pad, pad);
            scene.push_layer(Fill::NonZero, peniko::BlendMode::new(mix(item.blend), peniko::Compose::SrcOver), 1.0, t, &bounds);
        }
        let rule = if item.even_odd { Fill::EvenOdd } else { Fill::NonZero };
        match &item.fill {
            Some(Paint::Solid(c)) => scene.fill(rule, t, color(faded(*c, item.opacity)), None, &item.path),
            Some(Paint::Gradient { gradient, from, to }) => {
                let n = gradient.colors.len().max(2) - 1;
                let stops: Vec<peniko::ColorStop> =
                    gradient.colors.iter().enumerate().map(|(i, c)| peniko::ColorStop::from((i as f32 / n as f32, color(faded(*c, item.opacity))))).collect();
                let brush =
                    if gradient.radial { peniko::Gradient::new_radial(*from, from.distance(*to) as f32) } else { peniko::Gradient::new_linear(*from, *to) }
                        .with_stops(stops.as_slice());
                scene.fill(rule, t, &brush, None, &item.path);
            }
            None => {}
        }
        if let Some((c, width)) = item.stroke {
            let join = match item.line.join {
                Join::Miter => vello::kurbo::Join::Miter,
                Join::Round => vello::kurbo::Join::Round,
                Join::Bevel => vello::kurbo::Join::Bevel,
            };
            let cap = match item.line.cap {
                Cap::Butt => vello::kurbo::Cap::Butt,
                Cap::Round => vello::kurbo::Cap::Round,
                Cap::Square => vello::kurbo::Cap::Square,
            };
            let mut stroke = vello::kurbo::Stroke::new(width).with_join(join).with_caps(cap);
            if !item.line.dash.is_empty() {
                stroke = stroke.with_dashes(0.0, item.line.dash.iter().copied());
            }
            scene.stroke(&stroke, t, color(faded(c, item.opacity)), None, &item.path);
        }
        if blended {
            scene.pop_layer();
        }
    }
    scene.pop_layer();
}

fn mix(b: Blend) -> peniko::Mix {
    use peniko::Mix;
    match b {
        Blend::Normal => Mix::Normal,
        Blend::Multiply => Mix::Multiply,
        Blend::Screen => Mix::Screen,
        Blend::Overlay => Mix::Overlay,
        Blend::Darken => Mix::Darken,
        Blend::Lighten => Mix::Lighten,
        Blend::ColorDodge => Mix::ColorDodge,
        Blend::ColorBurn => Mix::ColorBurn,
        Blend::HardLight => Mix::HardLight,
        Blend::SoftLight => Mix::SoftLight,
        Blend::Difference => Mix::Difference,
        Blend::Exclusion => Mix::Exclusion,
        Blend::Hue => Mix::Hue,
        Blend::Saturation => Mix::Saturation,
        Blend::Color => Mix::Color,
        Blend::Luminosity => Mix::Luminosity,
    }
}
