//! Blurred layers: the ops of an [`Op::Blur`] are drawn into an offscreen image on the CPU and
//! softened with three box blurs (close to a Gaussian). Both renderers use the same image, so the
//! editor preview and exported frames match.

use crate::Op;
use motif_core::kurbo::{Affine, Rect, Shape};
use tiny_skia::Pixmap;

/// Largest side of a blurred layer in pixels; bigger layers are drawn at a lower resolution,
/// which the blur hides.
const MAX_SIDE: f64 = 2048.0;
/// A wide blur hides detail anyway, so its layer is drawn at a resolution where the blur is
/// this many pixels and stretched back: zoomed in, a soft shadow costs no more than at 100%.
const SIGMA_PX: f64 = 3.0;

/// A blurred layer ready to composite.
pub struct Layer {
    /// Premultiplied RGBA.
    pub pixmap: Pixmap,
    /// Maps layer pixels to target pixels.
    pub transform: Affine,
}

/// Uniform scale of an affine map (the square root of its area change).
pub(crate) fn scale_of(a: Affine) -> f64 {
    let [a, b, c, d, _, _] = a.as_coeffs();
    (a * d - b * c).abs().sqrt()
}

/// Bounds of the ops in target pixels under `view`, with room for strokes and inner blurs.
fn bounds(ops: &[Op], view: Affine) -> Option<Rect> {
    let mut out: Option<Rect> = None;
    for op in ops {
        let r = match op {
            Op::Draw(item) => {
                let t = view * item.transform;
                let pad = item.stroke.map_or(0.0, |s| s.1) * scale_of(t);
                t.transform_rect_bbox(item.path.bounding_box()).inflate(pad, pad)
            }
            Op::Blur { ops, radius } => {
                let pad = radius * 1.5 * scale_of(view);
                let Some(r) = bounds(ops, view) else { continue };
                r.inflate(pad, pad)
            }
            Op::PushClip { .. } | Op::Pop => continue,
        };
        out = Some(out.map_or(r, |o| o.union(r)));
    }
    out
}

/// Draws `ops` under `view` into a new image and blurs it by `radius` artboard units.
/// `clip` (target pixels) limits the layer to what can be seen.
pub fn render(ops: &[Op], radius: f64, view: Affine, antialias: bool, clip: Option<Rect>) -> Option<Layer> {
    let sigma = radius / 2.0 * scale_of(view);
    let reach = (sigma * 3.0).ceil();
    let mut area = bounds(ops, view)?.inflate(reach, reach);
    if let Some(c) = clip {
        area = area.intersect(c.inflate(reach, reach));
    }
    let area = area.expand();
    if area.width() < 1.0 || area.height() < 1.0 {
        return None;
    }
    let k = (MAX_SIDE / area.width().max(area.height())).min(SIGMA_PX / sigma.max(1e-9)).min(1.0);
    let mut pixmap = Pixmap::new((area.width() * k).ceil() as u32, (area.height() * k).ceil() as u32)?;
    let inner = Affine::scale(k) * Affine::translate(-area.origin().to_vec2()) * view;
    crate::cpu::draw_ops(&mut pixmap, ops, inner, antialias, None);
    gaussian(&mut pixmap, sigma * k);
    Some(Layer { pixmap, transform: Affine::translate(area.origin().to_vec2()) * Affine::scale(1.0 / k) })
}

/// Box radii whose three passes approximate a Gaussian of `sigma` (Kovesi, "Fast almost-Gaussian
/// filtering").
fn boxes(sigma: f64) -> [usize; 3] {
    let ideal = (12.0 * sigma * sigma / 3.0 + 1.0).sqrt();
    let mut lo = ideal.floor() as i64;
    if lo % 2 == 0 {
        lo -= 1;
    }
    let hi = lo + 2;
    let m = ((12.0 * sigma * sigma - (3 * lo * lo + 12 * lo + 9) as f64) / (-4.0 * lo as f64 - 4.0)).round() as i64;
    [0, 1, 2].map(|i| (((if i < m { lo } else { hi }) - 1) / 2).max(0) as usize)
}

/// Blurs premultiplied RGBA in place; outside the image counts as transparent.
pub(crate) fn gaussian(pixmap: &mut Pixmap, sigma: f64) {
    if sigma < 0.3 {
        return;
    }
    let (w, h) = (pixmap.width() as usize, pixmap.height() as usize);
    let data = pixmap.data_mut();
    let mut tmp = vec![0u8; data.len()];
    for r in boxes(sigma) {
        if r == 0 {
            continue;
        }
        // Rows into tmp, then columns back.
        for y in 0..h {
            box_line(data, &mut tmp, y * w * 4, 4, w, r);
        }
        for x in 0..w {
            box_line(&tmp, data, x * 4, w * 4, h, r);
        }
    }
}

/// One box pass over `len` pixels starting at byte `start`, `step` bytes apart.
fn box_line(src: &[u8], dst: &mut [u8], start: usize, step: usize, len: usize, r: usize) {
    let div = (2 * r + 1) as u32;
    let at = |i: usize| start + i * step;
    let mut sum = [0u32; 4];
    for i in 0..=r.min(len - 1) {
        for (c, s) in sum.iter_mut().enumerate() {
            *s += src[at(i) + c] as u32;
        }
    }
    for i in 0..len {
        for (c, s) in sum.iter().enumerate() {
            dst[at(i) + c] = ((s + div / 2) / div) as u8;
        }
        if i + r + 1 < len {
            for (c, s) in sum.iter_mut().enumerate() {
                *s += src[at(i + r + 1) + c] as u32;
            }
        }
        if i >= r {
            for (c, s) in sum.iter_mut().enumerate() {
                *s -= src[at(i - r) + c] as u32;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use motif_core::kurbo::Affine;
    use motif_core::{Artboard, Color, Document, Node, Shadow, ShapeKind};

    #[test]
    fn a_soft_shadow_costs_the_same_zoomed_in() {
        let mut doc = Document::new(Artboard::default());
        let mut card = Node::new("card", ShapeKind::Rect);
        (card.x, card.y, card.width, card.height) = (125.0, 100.0, 170.0, 110.0);
        card.shadow = Some(Shadow { dx: 0.0, dy: 10.0, blur: 16.0, color: Color::BLACK });
        doc.nodes.push(card);
        doc.rig();
        let ops = crate::display_ops(&doc);
        let Some(crate::Op::Blur { ops, radius }) = ops.iter().find(|o| matches!(o, crate::Op::Blur { .. })) else { panic!("no shadow layer") };
        let size = |zoom: f64| {
            let l = super::render(ops, *radius, Affine::scale(zoom), true, None).unwrap();
            // The layer still covers the shadow on the target.
            let covered = l.transform.transform_rect_bbox(motif_core::kurbo::Rect::new(0.0, 0.0, l.pixmap.width() as f64, l.pixmap.height() as f64));
            assert!(covered.width() > 170.0 * zoom, "{covered:?}");
            l.pixmap.width() * l.pixmap.height()
        };
        let (near, far) = (size(10.0), size(1.0));
        assert!(near <= far * 2, "{near} pixels at 1000% against {far} at 100%");
    }
}
