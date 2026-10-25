use crate::{Op, Paint, display_ops, faded};
use motif_core::kurbo::{Affine, Rect};
use motif_core::{Color, Document};
pub use vello;
use vello::Scene;
use vello::peniko::{self, Fill};

fn color(c: Color) -> peniko::Color {
    peniko::Color::from_rgba8(c.r, c.g, c.b, c.a)
}

/// Appends the artboard (background, then shapes clipped to it) to a Vello scene.
/// `view` maps artboard space to target pixels (pan and zoom).
pub fn build_scene(scene: &mut Scene, doc: &Document, view: Affine) {
    let bounds = Rect::new(0.0, 0.0, doc.artboard.width, doc.artboard.height);
    scene.fill(Fill::NonZero, view, color(doc.artboard.background), None, &bounds);
    scene.push_clip_layer(Fill::NonZero, view, &bounds);
    for op in display_ops(doc) {
        let item = match op {
            Op::Draw(item) => item,
            Op::PushClip { path, transform } => {
                scene.push_clip_layer(Fill::NonZero, view * transform, &path);
                continue;
            }
            Op::Pop => {
                scene.pop_layer();
                continue;
            }
        };
        let t = view * item.transform;
        match &item.fill {
            Some(Paint::Solid(c)) => scene.fill(Fill::NonZero, t, color(faded(*c, item.opacity)), None, &item.path),
            Some(Paint::Gradient { gradient, from, to }) => {
                let n = gradient.colors.len().max(2) - 1;
                let stops: Vec<peniko::ColorStop> =
                    gradient.colors.iter().enumerate().map(|(i, c)| peniko::ColorStop::from((i as f32 / n as f32, color(faded(*c, item.opacity))))).collect();
                let brush =
                    if gradient.radial { peniko::Gradient::new_radial(*from, from.distance(*to) as f32) } else { peniko::Gradient::new_linear(*from, *to) }
                        .with_stops(stops.as_slice());
                scene.fill(Fill::NonZero, t, &brush, None, &item.path);
            }
            None => {}
        }
        if let Some((c, width)) = item.stroke {
            let stroke = vello::kurbo::Stroke::new(width).with_join(vello::kurbo::Join::Round);
            scene.stroke(&stroke, t, color(faded(c, item.opacity)), None, &item.path);
        }
    }
    scene.pop_layer();
}
