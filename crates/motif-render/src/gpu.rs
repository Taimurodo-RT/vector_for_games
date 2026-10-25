use crate::{display_list, faded};
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
    for item in display_list(doc) {
        let t = view * item.transform;
        if let Some(fill) = item.fill {
            scene.fill(Fill::NonZero, t, color(faded(fill, item.opacity)), None, &item.path);
        }
        if let Some((c, width)) = item.stroke {
            let stroke = vello::kurbo::Stroke::new(width).with_join(vello::kurbo::Join::Round);
            scene.stroke(&stroke, t, color(faded(c, item.opacity)), None, &item.path);
        }
    }
    scene.pop_layer();
}
