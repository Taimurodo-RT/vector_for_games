use crate::{BackdropView, Op, Paint, display_ops, faded};
use motif_core::kurbo::{Affine, BezPath, PathEl, Rect};
use motif_core::{Blend, Cap, Color, Document, Join};
use std::cell::RefCell;
use std::collections::HashMap;
use std::hash::{DefaultHasher, Hash, Hasher};
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
    CACHE.with_borrow_mut(|c| c.frame += 1);
    draw_ops(scene, &display_ops(doc), view);
    CACHE.with_borrow_mut(|c| {
        let frame = c.frame;
        // Keep layers seen in the last few frames, so toggling between two states stays cheap.
        c.layers.retain(|_, (_, seen)| frame - *seen < 3);
    });
    scene.pop_layer();
}

/// Appends one pose for a game: the shapes clipped to the artboard, over its background when
/// `background` is set. `view` maps artboard space to target pixels, so many poses can share one
/// scene. Blurred layers are kept while they are drawn again within the next few hundred poses.
pub fn draw_pose(scene: &mut Scene, doc: &Document, view: Affine, background: bool) {
    let bounds = Rect::new(0.0, 0.0, doc.artboard.width, doc.artboard.height);
    if background {
        scene.fill(Fill::NonZero, view, color(doc.artboard.background), None, &bounds);
    }
    scene.push_clip_layer(Fill::NonZero, view, &bounds);
    CACHE.with_borrow_mut(|c| c.frame += 1);
    draw_ops(scene, &display_ops(doc), view);
    CACHE.with_borrow_mut(|c| {
        let frame = c.frame;
        if c.layers.len() > 64 {
            c.layers.retain(|_, (_, seen)| frame - *seen < 256);
        }
    });
    scene.pop_layer();
}

fn draw_ops(scene: &mut Scene, ops: &[Op], view: Affine) {
    for op in ops {
        let item = match op {
            Op::Draw(item) => item,
            Op::PushClip { path, transform, even_odd } => {
                scene.push_clip_layer(if *even_odd { Fill::EvenOdd } else { Fill::NonZero }, view * *transform, path);
                continue;
            }
            Op::Pop => {
                scene.pop_layer();
                continue;
            }
            Op::Blur { ops, radius } => {
                draw_blur(scene, ops, *radius, view);
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
}

/// Blurred layers drawn on the CPU and uploaded as images, kept between frames.
#[derive(Default)]
struct LayerCache {
    frame: u64,
    /// By content: the image, where it goes before panning, and the frame it was last drawn.
    layers: HashMap<u64, ((peniko::ImageData, Affine), u64)>,
}

thread_local! {
    static CACHE: RefCell<LayerCache> = RefCell::default();
}

/// A blurred layer. It is drawn without the view's translation, so panning reuses the same image
/// (and Vello does not upload it again).
fn draw_blur(scene: &mut Scene, ops: &[Op], radius: f64, view: Affine) {
    let [a, b, c, d, e, f] = view.as_coeffs();
    let linear = Affine::new([a, b, c, d, 0.0, 0.0]);
    let mut h = DefaultHasher::new();
    hash_ops(ops, &mut h);
    for v in [radius, a, b, c, d] {
        v.to_bits().hash(&mut h);
    }
    let key = h.finish();
    let image = CACHE.with_borrow_mut(|cache| {
        let frame = cache.frame;
        if let Some((img, seen)) = cache.layers.get_mut(&key) {
            *seen = frame;
            return Some(img.clone());
        }
        let layer = crate::blur::render(ops, radius, linear, true, None)?;
        let data = peniko::ImageData {
            data: peniko::Blob::from(layer.pixmap.data().to_vec()),
            format: peniko::ImageFormat::Rgba8,
            alpha_type: peniko::ImageAlphaType::AlphaPremultiplied,
            width: layer.pixmap.width(),
            height: layer.pixmap.height(),
        };
        cache.layers.insert(key, ((data.clone(), layer.transform), frame));
        Some((data, layer.transform))
    });
    if let Some((data, transform)) = image {
        let brush = peniko::ImageBrush::new(data).with_quality(peniko::ImageQuality::Medium);
        scene.draw_image(&brush, Affine::translate((e, f)) * transform);
    }
}

/// Hashes what a blurred layer draws, number by number: formatting the ops to text took longer
/// than drawing them once a glow sat on a line of text.
fn hash_ops(ops: &[Op], h: &mut DefaultHasher) {
    let path = |p: &BezPath, h: &mut DefaultHasher| {
        for el in p.elements() {
            match *el {
                PathEl::MoveTo(a) => (0u8, a.x.to_bits(), a.y.to_bits()).hash(h),
                PathEl::LineTo(a) => (1u8, a.x.to_bits(), a.y.to_bits()).hash(h),
                PathEl::QuadTo(a, b) => (2u8, a.x.to_bits(), a.y.to_bits(), b.x.to_bits(), b.y.to_bits()).hash(h),
                PathEl::CurveTo(a, b, c) => (3u8, [a.x, a.y, b.x, b.y, c.x, c.y].map(f64::to_bits)).hash(h),
                PathEl::ClosePath => 4u8.hash(h),
            }
        }
    };
    let affine = |t: &Affine, h: &mut DefaultHasher| t.as_coeffs().map(f64::to_bits).hash(h);
    for op in ops {
        match op {
            Op::Draw(item) => {
                0u8.hash(h);
                path(&item.path, h);
                affine(&item.transform, h);
                // Paint, line and the rest are small: their text is cheap.
                format!("{:?}{:?}{:?}{}{:?}{}", item.fill, item.stroke, item.line, item.opacity, item.blend, item.even_odd).hash(h);
            }
            Op::PushClip { path: p, transform, even_odd } => {
                (1u8, *even_odd).hash(h);
                path(p, h);
                affine(transform, h);
            }
            Op::Pop => 2u8.hash(h),
            Op::Blur { ops, radius } => {
                (3u8, radius.to_bits()).hash(h);
                hash_ops(ops, h);
            }
        }
    }
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
