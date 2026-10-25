//! Turns a [`Document`] into pixels.
//!
//! Both backends draw the same [`display_list`], so the editor preview (Vello on the GPU)
//! and exported frames (tiny-skia on the CPU) match.

mod cpu;
#[cfg(feature = "gpu")]
mod gpu;

pub use cpu::{CpuOptions, render_cpu, render_cpu_with};
#[cfg(feature = "gpu")]
pub use gpu::{build_scene, vello};

use motif_core::kurbo::{Affine, BezPath, Shape};
use motif_core::{Blend, Cap, Color, Document, Gradient, Join};

/// A reference picture drawn over the artboard background and under the shapes. Only the editor
/// passes one: exports never include it.
pub struct Backdrop {
    /// Premultiplied RGBA, as tiny-skia decodes it.
    pub pixmap: std::sync::Arc<tiny_skia::Pixmap>,
    #[cfg(feature = "gpu")]
    image: vello::peniko::ImageData,
}

impl Backdrop {
    pub fn new(pixmap: tiny_skia::Pixmap) -> Self {
        let pixmap = std::sync::Arc::new(pixmap);
        Backdrop {
            #[cfg(feature = "gpu")]
            image: vello::peniko::ImageData {
                data: vello::peniko::Blob::from(pixmap.data().to_vec()),
                format: vello::peniko::ImageFormat::Rgba8,
                alpha_type: vello::peniko::ImageAlphaType::AlphaPremultiplied,
                width: pixmap.width(),
                height: pixmap.height(),
            },
            pixmap,
        }
    }

    /// Decodes a PNG file's bytes.
    pub fn from_png(bytes: &[u8]) -> Result<Self, String> {
        tiny_skia::Pixmap::decode_png(bytes).map(Backdrop::new).map_err(|e| e.to_string())
    }

    pub fn size(&self) -> (u32, u32) {
        (self.pixmap.width(), self.pixmap.height())
    }
}

/// Where and how to draw a [`Backdrop`]: `transform` maps image pixels to artboard space.
#[derive(Clone, Copy)]
pub struct BackdropView<'a> {
    pub image: &'a Backdrop,
    pub transform: Affine,
    pub opacity: f64,
}

/// One shape ready to paint, in artboard space.
#[derive(Clone, Debug)]
pub struct Item {
    pub node: usize,
    pub path: BezPath,
    pub transform: Affine,
    pub fill: Option<Paint>,
    pub stroke: Option<(Color, f64)>,
    pub line: Line,
    /// Combined opacity of the node and its parent groups.
    pub opacity: f64,
    pub blend: Blend,
    pub even_odd: bool,
}

/// How a stroke is drawn: dashes, ends and corners.
#[derive(Clone, Debug, Default)]
pub struct Line {
    pub dash: Vec<f64>,
    pub cap: Cap,
    pub join: Join,
}

/// How a shape is filled.
#[derive(Clone, Debug)]
pub enum Paint {
    Solid(Color),
    /// Gradient with its start and end (or centre and radius point) in the shape's local space.
    Gradient {
        gradient: Gradient,
        from: motif_core::kurbo::Point,
        to: motif_core::kurbo::Point,
    },
}

/// A drawing step: shapes, and clip layers opened by masking groups.
#[derive(Clone, Debug)]
pub enum Op {
    Draw(Item),
    /// Following draws are clipped to `path` (artboard space after `transform`) until `Pop`.
    PushClip {
        path: BezPath,
        transform: Affine,
        /// Even-odd fill, so a frame with a hole clips to the outside of the hole.
        even_odd: bool,
    },
    Pop,
}

/// Flattens the node tree into paint order, skipping hidden nodes; masking groups become clip
/// layers around their children, and their first child (the mask) is not drawn.
pub fn display_ops(doc: &Document) -> Vec<Op> {
    let mut ops = Vec::new();
    // Clip layers still open: the group index they belong to.
    let mut open: Vec<usize> = Vec::new();
    let inside = |i: usize, group: usize| {
        let mut cur = doc.nodes[i].parent;
        while let Some(p) = cur {
            if p == group {
                return true;
            }
            cur = doc.nodes[p].parent;
        }
        false
    };
    for i in 0..doc.nodes.len() {
        while open.last().is_some_and(|&g| !inside(i, g)) {
            open.pop();
            ops.push(Op::Pop);
        }
        if !doc.is_visible(i) {
            continue;
        }
        let node = &doc.nodes[i];
        // The first child of a masking group is the mask itself.
        if let Some(p) = node.parent
            && doc.nodes[p].clip
            && doc.nodes.iter().position(|n| n.parent == Some(p)) == Some(i)
        {
            continue;
        }
        if node.clip
            && let Some(m) = doc.nodes.iter().position(|n| n.parent == Some(i))
            && let Some(path) = doc.nodes[m].path()
        {
            ops.push(Op::PushClip { path, transform: doc.world_transform(m), even_odd: false });
            open.push(i);
            continue;
        }
        let Some(path) = node.path() else { continue };
        let opacity = doc.world_opacity(i).clamp(0.0, 1.0);
        let stroke = node.stroke.filter(|_| node.stroke_width > 0.0).map(|c| (c, node.stroke_width));
        let fill = match (&node.gradient, node.fill) {
            (Some(g), _) if g.colors.len() >= 2 => {
                let (from, to) = g.line(path.bounding_box());
                Some(Paint::Gradient { gradient: g.clone(), from, to })
            }
            (_, Some(c)) => Some(Paint::Solid(c)),
            _ => None,
        };
        if opacity > 0.0 && (fill.is_some() || stroke.is_some()) {
            let transform = doc.world_transform(i);
            let line = Line { dash: node.dash.clone(), cap: node.cap, join: node.join };
            if let Some(sh) = node.shadow {
                shadow_items(&mut ops, i, &path, transform, sh, stroke.map_or(0.0, |s| s.1), opacity);
            }
            ops.push(Op::Draw(Item { node: i, path, transform, fill, stroke, line, opacity, blend: node.blend, even_odd: node.even_odd }));
        }
    }
    ops.extend(open.iter().map(|_| Op::Pop));
    ops
}

/// Most rings in a soft shadow: about one per artboard pixel of blur, so the edge has no bands.
const SHADOW_RINGS: usize = 32;

/// A shadow as plain shapes both renderers can draw: the shape moved by the offset and, for a
/// blur, rings of widening strokes outside it whose overlapping alpha fades out over the blur radius.
fn shadow_items(ops: &mut Vec<Op>, node: usize, path: &BezPath, transform: Affine, sh: motif_core::Shadow, stroke_w: f64, opacity: f64) {
    let moved = Affine::translate((sh.dx, sh.dy)) * transform;
    let line = Line { dash: Vec::new(), cap: Cap::Round, join: Join::Round };
    let item =
        |fill, stroke| Item { node, path: path.clone(), transform: moved, fill, stroke, line: line.clone(), opacity, blend: Blend::Normal, even_odd: false };
    let body = (stroke_w > 0.0).then_some((sh.color, stroke_w));
    if sh.blur > 0.0 {
        // The blur is in artboard pixels; strokes are in the shape's local space.
        let k = transform.as_coeffs();
        let local_scale = ((k[0] * k[3] - k[1] * k[2]).abs().sqrt()).max(1e-6);
        // Rings only count outside the shape: clip to a big frame with the shape cut out.
        let pad = (stroke_w + 2.0 * sh.blur) / local_scale + 2.0;
        let mut outside = path.bounding_box().inflate(pad, pad).to_path(0.1);
        outside.extend(path.iter());
        ops.push(Op::PushClip { path: outside, transform: moved, even_odd: true });
        // Where all rings overlap (at the edge) they add up to the shadow colour.
        let a = sh.color.a as f64 / 255.0;
        let rings = (sh.blur.ceil() as usize).clamp(4, SHADOW_RINGS);
        let ring = 1.0 - (1.0 - a).powf(1.0 / rings as f64);
        let faint = Color { a: (ring * 255.0).round().clamp(1.0, 255.0) as u8, ..sh.color };
        for r in 1..=rings {
            let width = (stroke_w + 2.0 * sh.blur * r as f64 / rings as f64) / local_scale;
            ops.push(Op::Draw(item(None, Some((faint, width)))));
        }
        ops.push(Op::Pop);
    }
    ops.push(Op::Draw(item(Some(Paint::Solid(sh.color)), body)));
}

/// The shapes in paint order, without clip layers.
pub fn display_list(doc: &Document) -> Vec<Item> {
    display_ops(doc)
        .into_iter()
        .filter_map(|op| match op {
            Op::Draw(item) => Some(item),
            _ => None,
        })
        .collect()
}

/// Applies opacity to a color's alpha.
pub(crate) fn faded(c: Color, opacity: f64) -> Color {
    Color { a: (c.a as f64 * opacity).round().clamp(0.0, 255.0) as u8, ..c }
}
