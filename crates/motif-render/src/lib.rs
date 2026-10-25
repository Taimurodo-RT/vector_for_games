//! Turns a [`Document`] into pixels.
//!
//! Both backends draw the same [`display_list`], so the editor preview (Vello on the GPU)
//! and exported frames (tiny-skia on the CPU) match.

mod blur;
mod cpu;
#[cfg(feature = "gpu")]
mod gpu;

pub use cpu::{CpuOptions, render_cpu, render_cpu_with, render_region};
#[cfg(feature = "gpu")]
pub use gpu::{build_scene, vello};

use motif_core::kurbo::{Affine, BezPath, Shape};
use motif_core::{Blend, Cap, Color, Document, Gradient, Join, Shadow};

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

/// A drawing step: shapes, clip layers opened by masking groups, and blurred layers.
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
    /// `ops` drawn on their own layer, then softened by a Gaussian blur that reaches `radius`
    /// artboard units past the edges (σ = radius / 2, like CSS `box-shadow` blur).
    Blur {
        ops: Vec<Op>,
        radius: f64,
    },
}

/// A group whose children are still being collected: it may clip them, blur them, or both.
struct Open {
    group: usize,
    clip: bool,
    /// Blur radius and the ops gathered so far, when the group is blurred.
    blur: Option<(f64, Vec<Op>)>,
}

/// Where new ops go: the innermost blurred group, or the top level.
fn target<'a>(ops: &'a mut Vec<Op>, open: &'a mut [Open]) -> &'a mut Vec<Op> {
    match open.iter_mut().rev().find_map(|o| o.blur.as_mut()) {
        Some((_, inner)) => inner,
        None => ops,
    }
}

fn close(ops: &mut Vec<Op>, open: &mut Vec<Open>) {
    let Some(o) = open.pop() else { return };
    match o.blur {
        Some((radius, mut inner)) => {
            if o.clip {
                inner.push(Op::Pop);
            }
            target(ops, open).push(Op::Blur { ops: inner, radius });
        }
        None if o.clip => target(ops, open).push(Op::Pop),
        None => {}
    }
}

/// Flattens the node tree into paint order, skipping hidden nodes; masking groups become clip
/// layers around their children, and their first child (the mask) is not drawn. Blurred objects
/// and groups become blur layers.
pub fn display_ops(doc: &Document) -> Vec<Op> {
    let mut ops = Vec::new();
    let mut open: Vec<Open> = Vec::new();
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
        while open.last().is_some_and(|o| !inside(i, o.group)) {
            close(&mut ops, &mut open);
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
        // Bones are drawn by the editor only.
        if node.kind == motif_core::ShapeKind::Bone {
            continue;
        }
        if node.kind == motif_core::ShapeKind::Group {
            let mask = node.clip.then(|| doc.nodes.iter().position(|n| n.parent == Some(i))).flatten().and_then(|m| Some((m, doc.nodes[m].path()?)));
            let clip = match mask {
                Some((m, path)) => Some(Op::PushClip { path, transform: doc.world_transform(m), even_odd: false }),
                None => ball_clip(doc, i),
            };
            if clip.is_some() || node.blur > 0.0 {
                open.push(Open { group: i, clip: clip.is_some(), blur: (node.blur > 0.0).then(|| (node.blur, Vec::new())) });
                if let Some(clip) = clip {
                    target(&mut ops, &mut open).push(clip);
                }
            }
            continue;
        }
        let Some(path) = node.path() else { continue };
        // A 3D body draws its faces; its shadow and outlines take the whole silhouette.
        let faces = node.body_faces(doc.light);
        let path = faces.as_deref().map_or(path, motif_core::silhouette);
        let brush = &node.brush;
        let seed = brush.seed_at(&node.id, doc.time);
        // A body keeps its true outline: its faces are drawn from the shape itself.
        let path = if faces.is_none() { brush.roughen(&path, seed) } else { path };
        let opacity = doc.world_opacity(i).clamp(0.0, 1.0);
        let stroke = node.stroke.filter(|_| node.stroke_width > 0.0).map(|c| (c, node.stroke_width));
        let fill = match (&node.gradient, node.fill) {
            (Some(g), _) if g.colors.len() >= 2 => {
                let (from, to) = g.line(path.bounding_box());
                Some(Paint::Gradient { gradient: brush.band_gradient(g).unwrap_or_else(|| g.clone()), from, to })
            }
            (_, Some(c)) => Some(Paint::Solid(c)),
            _ => None,
        };
        let textured = brush.hatch.is_some() || brush.grain.is_some();
        if opacity <= 0.0 || (fill.is_none() && stroke.is_none() && node.outlines.is_empty() && !textured) {
            continue;
        }
        let mut own = Vec::new();
        let transform = doc.world_transform(i);
        let line = Line { dash: node.dash.clone(), cap: node.cap, join: node.join };
        let widest = node.outlines.first().map_or(0.0, |o| o.1);
        let plain = node.particles.is_none();
        if plain && let Some(sh) = Shadow::cast(node.shadow, node.elevation, doc.light) {
            shadow_ops(&mut own, i, &path, transform, sh, stroke.map_or(0.0, |s| s.1).max(widest), opacity);
        }
        // Layered outlines under the shape, widest first, with round corners like a sticker.
        for &(c, w) in node.outlines.iter().filter(|_| plain) {
            let line = Line { dash: Vec::new(), cap: Cap::Round, join: Join::Round };
            own.push(Op::Draw(Item {
                node: i,
                path: path.clone(),
                transform,
                fill: None,
                stroke: Some((c, w)),
                line,
                opacity,
                blend: node.blend,
                even_odd: false,
            }));
        }
        match &node.particles {
            // An emitter draws its particles instead of itself.
            Some(parts) => own.extend(parts.iter().filter(|(_, o)| *o > 0.0).map(|(at, o)| {
                Op::Draw(Item {
                    node: i,
                    path: path.clone(),
                    transform: *at,
                    fill: fill.clone(),
                    stroke,
                    line: line.clone(),
                    opacity: opacity * o.clamp(0.0, 1.0),
                    blend: node.blend,
                    even_odd: node.even_odd,
                })
            })),
            None => match faces {
                Some(faces) => body_ops(&mut own, i, faces, transform, stroke, opacity, node.blend, brush.bands > 0),
                None if textured => {
                    // Hatching and grain lie on the fill, under the outline.
                    let base = node.fill.or_else(|| node.gradient.as_ref().and_then(|g| g.colors.first().copied()));
                    let darker = base.map(|c| c.lerp(Color { a: c.a, ..Color::BLACK }, 0.45));
                    let item = |path: BezPath, fill: Option<Paint>, stroke: Option<(Color, f64)>, line: Line, even_odd: bool| {
                        Op::Draw(Item { node: i, path, transform, fill, stroke, line, opacity, blend: node.blend, even_odd })
                    };
                    if fill.is_some() {
                        own.push(item(path.clone(), fill, None, line.clone(), node.even_odd));
                    }
                    own.push(Op::PushClip { path: path.clone(), transform, even_odd: node.even_odd });
                    let bounds = path.bounding_box();
                    if let (Some(h), Some(lines)) = (brush.hatch, brush.hatch_lines(bounds, seed))
                        && let Some(c) = h.color.or(node.stroke).or(darker)
                    {
                        let round = Line { dash: Vec::new(), cap: Cap::Round, join: Join::Round };
                        own.push(item(lines, None, Some((c, h.width)), round, false));
                    }
                    if let (Some(g), Some(dots)) = (brush.grain, brush.grain_dots(bounds, seed ^ 0x5bd1_e995))
                        && let Some(c) = g.color.or(darker).or(node.stroke)
                    {
                        own.push(item(dots, Some(Paint::Solid(c)), None, Line::default(), false));
                    }
                    own.push(Op::Pop);
                    if stroke.is_some() {
                        own.push(item(path, None, stroke, line, node.even_odd));
                    }
                }
                None => own.push(Op::Draw(Item { node: i, path, transform, fill, stroke, line, opacity, blend: node.blend, even_odd: node.even_odd })),
            },
        }
        if let Some(clip) = ball_clip(doc, i) {
            own.insert(0, clip);
            own.push(Op::Pop);
        }
        let out = target(&mut ops, &mut open);
        if node.blur > 0.0 {
            out.push(Op::Blur { ops: own, radius: node.blur });
        } else {
            out.extend(own);
        }
    }
    while !open.is_empty() {
        close(&mut ops, &mut open);
    }
    ops
}

/// The faces of a body back to front, then its inked edges over them all, so that a thick edge
/// is not cut by the next face. A hairline of the face's own colour closes the seams between
/// neighbouring faces.
#[allow(clippy::too_many_arguments)]
fn body_ops(
    ops: &mut Vec<Op>,
    node: usize,
    faces: Vec<motif_core::Face>,
    transform: Affine,
    stroke: Option<(Color, f64)>,
    opacity: f64,
    blend: Blend,
    banded: bool,
) {
    let line = Line { dash: Vec::new(), cap: Cap::Round, join: Join::Round };
    let polylines = |lines: &[Vec<motif_core::kurbo::Point>], path: &mut BezPath| {
        for e in lines {
            path.move_to(e[0]);
            for p in &e[1..] {
                path.line_to(*p);
            }
        }
    };
    let mut top = BezPath::new();
    for f in faces {
        polylines(&f.top_edges, &mut top);
        // A curved stretch is shaded by a gradient across it.
        let (fill, seam) = match f.gradient {
            Some((from, to, colors)) => {
                // Banded light: neighbouring shades meet in a hard step, not a blend.
                let colors = if banded { motif_core::brush_hard(colors) } else { colors };
                (Paint::Gradient { gradient: Gradient { radial: false, angle: 0.0, colors }, from, to }, None)
            }
            None => (Paint::Solid(f.color), Some((f.color, 0.6))),
        };
        ops.push(Op::Draw(Item {
            node,
            path: f.path,
            transform,
            fill: Some(fill),
            stroke: seam,
            line: Line { join: Join::Round, ..Line::default() },
            opacity,
            blend,
            even_odd: f.even_odd,
        }));
        let mut own = BezPath::new();
        polylines(&f.edges, &mut own);
        if let Some(ink) = stroke.filter(|_| !own.is_empty()) {
            ops.push(Op::Draw(Item { node, path: own, transform, fill: None, stroke: Some(ink), line: line.clone(), opacity, blend, even_odd: false }));
        }
    }
    if let Some(ink) = stroke.filter(|_| !top.is_empty()) {
        ops.push(Op::Draw(Item { node, path: top, transform, fill: None, stroke: Some(ink), line, opacity, blend, even_odd: false }));
    }
}

/// The clip of an object on a ball (see `Document::place_orbits`): inside the ball for a shape
/// lying on it, outside it for one gone round its back.
fn ball_clip(doc: &Document, i: usize) -> Option<Op> {
    let (s, inside) = doc.nodes[i].behind?;
    let ball = doc.world_transform(s) * doc.nodes.get(s)?.path()?;
    if inside {
        return Some(Op::PushClip { path: ball, transform: Affine::IDENTITY, even_odd: false });
    }
    let mut path = motif_core::kurbo::Rect::new(-1e6, -1e6, 1e6, 1e6).to_path(0.1);
    path.extend(ball);
    Some(Op::PushClip { path, transform: Affine::IDENTITY, even_odd: true })
}

/// A shadow: the shape (and its stroke body) moved by the offset in the shadow colour, blurred
/// on its own layer when it has a blur.
fn shadow_ops(ops: &mut Vec<Op>, node: usize, path: &BezPath, transform: Affine, sh: Shadow, stroke_w: f64, opacity: f64) {
    let moved = Affine::translate((sh.dx, sh.dy)) * transform;
    let line = Line { dash: Vec::new(), cap: Cap::Round, join: Join::Round };
    let body = (stroke_w > 0.0).then_some((sh.color, stroke_w));
    let item = Item {
        node,
        path: path.clone(),
        transform: moved,
        fill: Some(Paint::Solid(sh.color)),
        stroke: body,
        line,
        opacity,
        blend: Blend::Normal,
        even_odd: false,
    };
    if sh.blur > 0.0 {
        ops.push(Op::Blur { ops: vec![Op::Draw(item)], radius: sh.blur });
    } else {
        ops.push(Op::Draw(item));
    }
}

/// The shapes in paint order, without clip or blur layers.
pub fn display_list(doc: &Document) -> Vec<Item> {
    fn walk(ops: Vec<Op>, out: &mut Vec<Item>) {
        for op in ops {
            match op {
                Op::Draw(item) => out.push(item),
                Op::Blur { ops, .. } => walk(ops, out),
                _ => {}
            }
        }
    }
    let mut out = Vec::new();
    walk(display_ops(doc), &mut out);
    out
}

/// Applies opacity to a color's alpha.
pub(crate) fn faded(c: Color, opacity: f64) -> Color {
    Color { a: (c.a as f64 * opacity).round().clamp(0.0, 255.0) as u8, ..c }
}
