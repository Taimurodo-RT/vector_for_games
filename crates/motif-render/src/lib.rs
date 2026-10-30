//! Turns a [`Document`] into pixels.
//!
//! Both backends draw the same [`display_list`], so the editor preview (Vello on the GPU)
//! and exported frames (tiny-skia on the CPU) match.

mod blur;
mod cpu;
#[cfg(feature = "gpu")]
mod gpu;
#[cfg(feature = "mesh")]
pub mod mesh;

pub use cpu::{CpuOptions, render_cpu, render_cpu_with, render_region};
#[cfg(feature = "gpu")]
pub use gpu::{build_scene, draw_pose, vello};

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
    /// Which authored paints this pass uses. Effect/derived colors have no reference.
    pub fill_source: Option<motif_core::Prop>,
    pub stroke_source: Option<motif_core::Prop>,
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
        target(&mut ops, &mut open).extend(node_ops(doc, i));
    }
    while !open.is_empty() {
        close(&mut ops, &mut open);
    }
    ops
}

/// Draw operations for one shape, in artboard coordinates, including its effects.
/// Ancestor clips and group blurs are handled by `display_ops`. Exporters can reuse
/// this painter without reimplementing shadows, brushes or three-dimensional faces.
pub fn node_ops(doc: &Document, i: usize) -> Vec<Op> {
    let Some(node) = doc.nodes.get(i) else { return Vec::new() };
    if node.kind.is_container() || !doc.is_visible(i) {
        return Vec::new();
    }
    let Some(path) = node.path() else { return Vec::new() };
    // A 3D body draws its faces; its shadow and outlines take the whole silhouette.
    let faces = node.body_faces(doc.light);
    let path = faces.as_deref().map_or(path, motif_core::silhouette);
    let brush = &node.brush;
    let seed = brush.seed_at(&node.id, doc.time) ^ (doc.seed as u32 ^ (doc.seed >> 32) as u32);
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
    // A gradient stroke paints the stroke's outline as a fill, with the gradient of the shape.
    let line_w = if node.stroke.is_some() || node.stroke_gradient.is_some() { node.stroke_width } else { 0.0 };
    let ring = node.stroke_gradient.as_ref().filter(|g| g.colors.len() >= 2 && line_w > 0.0);
    let glow = node.glow.filter(|g| g.size > 0.0 || g.spread > 0.0);
    let inset = node.inset;
    let noise = node.noise.filter(|n| n.amount > 0.0);
    if opacity <= 0.0 || (fill.is_none() && stroke.is_none() && ring.is_none() && node.outlines.is_empty() && !textured) {
        return Vec::new();
    }
    let mut own = Vec::new();
    let transform = doc.world_transform(i);
    let line = Line { dash: node.dash.clone(), cap: node.cap, join: node.join };
    let widest = node.outlines.first().map_or(0.0, |o| o.1);
    let plain = node.particles.is_none();
    if plain && let Some(g) = glow {
        glow_ops(&mut own, i, &path, transform, g, fill.is_some(), line_w.max(widest), opacity);
    }
    if plain && let Some(sh) = Shadow::cast(node.shadow, node.elevation, doc.light) {
        shadow_ops(&mut own, i, &path, transform, sh, line_w.max(widest), opacity);
    }
    // Layered outlines under the shape, widest first, with round corners like a sticker.
    for &(c, w) in node.outlines.iter().filter(|_| plain) {
        let line = Line { dash: Vec::new(), cap: Cap::Round, join: Join::Round };
        own.push(Op::Draw(Item {
            fill_source: None,
            stroke_source: None,
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
                fill_source: None,
                stroke_source: None,
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
            None if textured || inset.is_some() || noise.is_some() || ring.is_some() => {
                // Hatching, grain, an inner shadow and noise lie on the fill, under the outline.
                let base = node.fill.or_else(|| node.gradient.as_ref().and_then(|g| g.colors.first().copied()));
                let darker = base.map(|c| c.lerp(Color { a: c.a, ..Color::BLACK }, 0.45));
                let item = |path: BezPath, fill: Option<Paint>, stroke: Option<(Color, f64)>, line: Line, even_odd: bool, fill_source, stroke_source| {
                    Op::Draw(Item { fill_source, stroke_source, node: i, path, transform, fill, stroke, line, opacity, blend: node.blend, even_odd })
                };
                if fill.is_some() {
                    own.push(item(path.clone(), fill, None, line.clone(), node.even_odd, Some(motif_core::Prop::Fill), None));
                }
                own.push(Op::PushClip { path: path.clone(), transform, even_odd: node.even_odd });
                let bounds = path.bounding_box();
                if let (Some(h), Some(lines)) = (brush.hatch, brush.hatch_lines(bounds, seed))
                    && let Some(c) = h.color.or(node.stroke).or(darker)
                {
                    let round = Line { dash: Vec::new(), cap: Cap::Round, join: Join::Round };
                    own.push(item(lines, None, Some((c, h.width)), round, false, None, None));
                }
                if let (Some(g), Some(dots)) = (brush.grain, brush.grain_dots(bounds, seed ^ 0x5bd1_e995))
                    && let Some(c) = g.color.or(darker).or(node.stroke)
                {
                    own.push(item(dots, Some(Paint::Solid(c)), None, Line::default(), false, None, None));
                }
                if let Some(sh) = inset {
                    inset_ops(&mut own, i, &path, transform, sh, line_w, opacity);
                }
                if let Some(n) = noise {
                    own.extend(noise_ops(i, &path, transform, n, &node.id, opacity));
                }
                own.push(Op::Pop);
                match ring {
                    Some(g) => {
                        let outline = stroke_outline(&path, line_w, &line);
                        let (from, to) = g.line(path.bounding_box());
                        own.push(item(
                            outline,
                            Some(Paint::Gradient { gradient: brush.band_gradient(g).unwrap_or_else(|| g.clone()), from, to }),
                            None,
                            Line::default(),
                            false,
                            Some(motif_core::Prop::StrokeColor),
                            None,
                        ));
                    }
                    None if stroke.is_some() => own.push(item(path, None, stroke, line, node.even_odd, None, Some(motif_core::Prop::StrokeColor))),
                    None => {}
                }
            }
            None => own.push(Op::Draw(Item {
                fill_source: Some(motif_core::Prop::Fill),
                stroke_source: Some(motif_core::Prop::StrokeColor),
                node: i,
                path,
                transform,
                fill,
                stroke,
                line,
                opacity,
                blend: node.blend,
                even_odd: node.even_odd,
            })),
        },
    }
    if let Some(clip) = ball_clip(doc, i) {
        own.insert(0, clip);
        own.push(Op::Pop);
    }
    if node.blur > 0.0 { vec![Op::Blur { ops: own, radius: node.blur }] } else { own }
}

/// Bounds of vector draw operations, including strokes and nested blur extents.
pub fn ops_bounds(ops: &[Op]) -> Option<motif_core::kurbo::Rect> {
    blur::bounds(ops, Affine::IDENTITY)
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
            fill_source: None,
            stroke_source: None,
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
            ops.push(Op::Draw(Item {
                fill_source: None,
                stroke_source: None,
                node,
                path: own,
                transform,
                fill: None,
                stroke: Some(ink),
                line: line.clone(),
                opacity,
                blend,
                even_odd: false,
            }));
        }
    }
    if let Some(ink) = stroke.filter(|_| !top.is_empty()) {
        ops.push(Op::Draw(Item {
            fill_source: None,
            stroke_source: None,
            node,
            path: top,
            transform,
            fill: None,
            stroke: Some(ink),
            line,
            opacity,
            blend,
            even_odd: false,
        }));
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
        fill_source: None,
        stroke_source: None,
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

/// A glow: the shape (or only its stroke, when it has no fill) grown by the spread and softened
/// by the size, in the glow colour, behind the shape.
#[allow(clippy::too_many_arguments)]
fn glow_ops(ops: &mut Vec<Op>, node: usize, path: &BezPath, transform: Affine, g: motif_core::Glow, filled: bool, stroke_w: f64, opacity: f64) {
    let line = Line { dash: Vec::new(), cap: Cap::Round, join: Join::Round };
    let body = stroke_w + 2.0 * g.spread;
    let item = Item {
        fill_source: None,
        stroke_source: None,
        node,
        path: path.clone(),
        transform,
        fill: filled.then_some(Paint::Solid(g.color)),
        stroke: (body > 0.0).then_some((g.color, body)),
        line,
        opacity,
        blend: Blend::Normal,
        even_odd: false,
    };
    if g.size > 0.0 {
        ops.push(Op::Blur { ops: vec![Op::Draw(item)], radius: g.size });
    } else {
        ops.push(Op::Draw(item));
    }
}

/// An inner shadow, drawn inside the shape's clip: a frame around the shape with the shape cut
/// out, moved by the offset and softened, so its shade falls in from the edges.
fn inset_ops(ops: &mut Vec<Op>, node: usize, path: &BezPath, transform: Affine, sh: Shadow, stroke_w: f64, opacity: f64) {
    let reach = sh.blur * 1.5 + sh.dx.abs() + sh.dy.abs() + stroke_w + 1.0;
    let mut frame = path.bounding_box().inflate(reach, reach).to_path(0.1);
    frame.extend(path.iter());
    let item = Item {
        fill_source: None,
        stroke_source: None,
        node,
        path: frame,
        transform: Affine::translate((sh.dx, sh.dy)) * transform,
        fill: Some(Paint::Solid(sh.color)),
        stroke: None,
        line: Line::default(),
        opacity,
        blend: Blend::Normal,
        even_odd: true,
    };
    if sh.blur > 0.0 {
        ops.push(Op::Blur { ops: vec![Op::Draw(item)], radius: sh.blur });
    } else {
        ops.push(Op::Draw(item));
    }
}

/// Film grain: a square speck per cell over the shape's box, lighter or darker by a hash of the
/// cell, in three strengths each way. The same object keeps the same grain in every frame.
fn noise_ops(node: usize, path: &BezPath, transform: Affine, n: motif_core::Noise, id: &str, opacity: f64) -> Vec<Op> {
    let b = path.bounding_box();
    // At most this many specks: a big shape gets a coarser grain rather than a slow one.
    const MOST: f64 = 60_000.0;
    let size = n.size.max((b.area() / MOST).sqrt()).max(0.05);
    let (nx, ny) = ((b.width() / size).ceil() as i64, (b.height() / size).ceil() as i64);
    let seed = id.bytes().fold(0x811c_9dc5u32, |h, c| (h ^ c as u32).wrapping_mul(0x0100_0193));
    let mut levels: [BezPath; 6] = Default::default();
    for y in 0..ny {
        for x in 0..nx {
            let mut h = seed ^ (x as u32).wrapping_mul(0x9e37_79b1) ^ (y as u32).wrapping_mul(0x85eb_ca77);
            h ^= h >> 15;
            h = h.wrapping_mul(0x2c1b_3c6d);
            h ^= h >> 12;
            h = h.wrapping_mul(0x297a_2d39);
            h ^= h >> 15;
            // −1..1: the sign picks light or dark, the size one of three strengths.
            let v = (h % 2000) as f64 / 1000.0 - 1.0;
            let level = ((v.abs() * 3.0).ceil() as usize).clamp(1, 3);
            let k = if v >= 0.0 { level - 1 } else { 2 + level };
            let (x0, y0) = (b.x0 + x as f64 * size, b.y0 + y as f64 * size);
            let p = &mut levels[k];
            p.move_to((x0, y0));
            p.line_to((x0 + size, y0));
            p.line_to((x0 + size, y0 + size));
            p.line_to((x0, y0 + size));
            p.close_path();
        }
    }
    levels
        .into_iter()
        .enumerate()
        .filter(|(_, p)| !p.is_empty())
        .map(|(k, p)| {
            // Half the specks lighten, half darken.
            let strength = (k % 3 + 1) as f64 / 3.0 * n.amount.clamp(0.0, 1.0);
            let base = if k < 3 { Color::WHITE } else { Color::BLACK };
            let color = Color { a: (strength * 255.0).round() as u8, ..base };
            Op::Draw(Item {
                fill_source: None,
                stroke_source: None,
                node,
                path: p,
                transform,
                fill: Some(Paint::Solid(color)),
                stroke: None,
                line: Line::default(),
                opacity,
                blend: Blend::Normal,
                even_odd: false,
            })
        })
        .collect()
}

/// The outline of a stroke as a shape to fill: how a gradient stroke is painted.
pub(crate) fn stroke_outline(path: &BezPath, width: f64, line: &Line) -> BezPath {
    use motif_core::kurbo;
    let mut dash = line.dash.clone();
    if dash.len() % 2 == 1 {
        dash.extend(dash.clone());
    }
    let cap = match line.cap {
        Cap::Butt => kurbo::Cap::Butt,
        Cap::Round => kurbo::Cap::Round,
        Cap::Square => kurbo::Cap::Square,
    };
    let join = match line.join {
        Join::Miter => kurbo::Join::Miter,
        Join::Round => kurbo::Join::Round,
        Join::Bevel => kurbo::Join::Bevel,
    };
    let style = kurbo::Stroke { width, join, miter_limit: 4.0, start_cap: cap, end_cap: cap, ..kurbo::Stroke::default() }.with_dashes(0.0, dash);
    kurbo::stroke(path.iter(), &style, &kurbo::StrokeOpts::default(), 0.01)
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
