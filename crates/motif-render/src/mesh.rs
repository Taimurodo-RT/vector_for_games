//! The scene as triangles, for a GPU that paints them itself: MotifSprite hands them to Godot.
//!
//! Plain fills and strokes become coloured triangles with a thin fading fringe along their edges
//! (the antialiasing). What triangles cannot show yet (blurs and shadows, masks, gradients,
//! blend modes) is drawn on the CPU as before, but only as a small picture under that part:
//! a [`Patch`]. Pieces come in paint order.
//!
//! A shape is cut once in its own coordinates and kept: while it only moves, turns or scales,
//! the next frames just move its triangles. A shape in cages (`deform`) is cut as it lies before
//! them, and each frame the cages bend its points; a stroke bends along its line and keeps its
//! width. Only a shape whose own outline changes (bones, morphs) is cut again.

use crate::{Op, Paint, blur, cpu};
use lyon_tessellation::math::point;
use lyon_tessellation::path::Path as LyonPath;
use lyon_tessellation::{
    BuffersBuilder, FillOptions, FillRule, FillTessellator, FillVertex, LineCap, LineJoin, StrokeOptions, StrokeTessellator, StrokeVertex, VertexBuffers,
};
use motif_core::kurbo::{Affine, BezPath, CubicBez, ParamCurve, PathEl, PathSeg, Point, QuadBez, Rect, Vec2};
use motif_core::{Blend, Color, Document};
use std::collections::HashMap;
use std::hash::{DefaultHasher, Hash, Hasher};
use tiny_skia::Pixmap;

/// Triangles in artboard units: `colors` premultiplied, three `indices` a triangle.
#[derive(Clone, Debug, Default)]
pub struct Mesh {
    pub points: Vec<[f32; 2]>,
    pub colors: Vec<[f32; 4]>,
    pub indices: Vec<i32>,
}

/// A part drawn on the CPU: premultiplied pixels whose top-left corner is at `at` (artboard
/// units), `scale` pixels to a unit.
#[derive(Clone, Debug)]
pub struct Patch {
    pub pixmap: Pixmap,
    pub at: Point,
    pub scale: f64,
}

#[derive(Clone, Debug)]
pub enum Piece {
    Mesh(Mesh),
    Patch(Patch),
}

/// The pose as pieces for a picture of `scale` pixels a unit, cut afresh. A [`Cutter`] kept
/// between frames does the same faster.
pub fn pieces(doc: &Document, scale: f64) -> Vec<Piece> {
    Cutter::default().pieces(doc, scale)
}

/// Cuts poses into pieces and remembers the shapes it cut. Keep one per playing scene.
#[derive(Default)]
pub struct Cutter {
    fill: FillTessellator,
    stroke: StrokeTessellator,
    cuts: HashMap<u64, (Cut, u64)>,
    frame: u64,
}

/// A shape cut in its own coordinates.
#[derive(Default)]
struct Cut {
    points: Vec<[f32; 2]>,
    indices: Vec<u32>,
    /// The edge band: the point on the outline, its offset for a band one unit wide, and
    /// whether it is on the filled side (opaque) or the open one (clear).
    band: Vec<([f32; 2], [f32; 2], bool)>,
    band_indices: Vec<u32>,
    /// A stroke cut along its unbent line: the point on the line and the offset to the stroke's
    /// side, so that a cage bends the line and the stroke keeps its width.
    line: Vec<([f32; 2], [f32; 2])>,
    line_indices: Vec<u32>,
    /// The points of `line` on the stroke's outer sides, along which its edge band runs.
    line_band: Vec<u32>,
    /// The outer sides, as pairs of places in `line_band`.
    line_edges: Vec<[u32; 2]>,
}

impl Cutter {
    /// The pose as pieces for a picture of `scale` pixels a unit: that sets how finely curves
    /// are cut and how wide the edge band is. The artboard background is not included.
    pub fn pieces(&mut self, doc: &Document, scale: f64) -> Vec<Piece> {
        let scale = if scale.is_finite() && scale > 0.0 { scale } else { 1.0 };
        self.frame += 1;
        let canvas = Rect::new(0.0, 0.0, doc.artboard.width * scale, doc.artboard.height * scale);
        let mut out: Vec<Piece> = Vec::new();
        let ops = crate::display_ops(doc);
        let mut i = 0;
        while i < ops.len() {
            // A run of ops drawn together on the CPU: a blur, or a mask up to its `Pop`.
            let run = match &ops[i] {
                Op::Draw(item) if meshable(item) => {
                    if !matches!(out.last(), Some(Piece::Mesh(_))) {
                        out.push(Piece::Mesh(Mesh::default()));
                    }
                    if let Some(Piece::Mesh(mesh)) = out.last_mut() {
                        self.item(doc, item, scale, mesh);
                    }
                    i += 1;
                    continue;
                }
                Op::Draw(_) | Op::Blur { .. } | Op::Pop => 1,
                Op::PushClip { .. } => {
                    let mut depth = 0;
                    let mut n = 0;
                    for op in &ops[i..] {
                        n += 1;
                        match op {
                            Op::PushClip { .. } => depth += 1,
                            Op::Pop => depth -= 1,
                            _ => {}
                        }
                        if depth == 0 {
                            break;
                        }
                    }
                    n
                }
            };
            if let Some(p) = patch(&ops[i..i + run], scale, canvas) {
                out.push(Piece::Patch(p));
            }
            i += run;
        }
        out.retain(|p| !matches!(p, Piece::Mesh(m) if m.indices.is_empty()));
        // Shapes not seen for a while are forgotten: a clip that ended, a morph passed.
        let frame = self.frame;
        self.cuts.retain(|_, (_, seen)| frame - *seen < 3);
        out
    }

    fn item(&mut self, doc: &Document, item: &crate::Item, scale: f64, mesh: &mut Mesh) {
        // How finely to cut, in the shape's own units: its curves are seen `zoom` times larger.
        let zoom = blur::scale_of(item.transform).max(1e-9) * scale;
        let tolerance = (0.2 / zoom) as f32;
        // A shape bent by cages is cut as it is before them, once, and its points are bent each
        // frame: animating a cage costs no cutting. Its sides are split so that they bend too.
        let cage = doc.nodes.get(item.node).and_then(|n| n.cage_bend()).filter(|(_, bent, _)| **bent == item.path);
        let cage = cage.map(|(unbent, _, map)| (split(&unbent, 12.0 / zoom), map));
        let bend = cage.as_ref().map(|(_, map)| move |p: Point| map.map(p));
        let bend = bend.as_ref().map(|b| b as &dyn Fn(Point) -> Point);
        if let Some(Paint::Solid(c)) = &item.fill {
            let color = premultiplied(*c, item.opacity);
            let even_odd = item.even_odd;
            match &cage {
                Some((unbent, _)) => {
                    let key = key(unbent, even_odd, None, zoom) ^ BENT;
                    self.place(key, item, scale, color, bend, mesh, |fill, stroke| cut(fill, stroke, unbent, even_odd, tolerance));
                }
                None => {
                    let key = key(&item.path, even_odd, None, zoom);
                    self.place(key, item, scale, color, None, mesh, |fill, stroke| cut(fill, stroke, &item.path, even_odd, tolerance));
                }
            }
        }
        if let Some((c, width)) = item.stroke
            && width > 0.0
        {
            let color = premultiplied(c, item.opacity);
            let line = &item.line;
            match &cage {
                // Dashes are laid along the line as drawn: such a stroke is cut as drawn.
                Some((unbent, _)) if line.dash.is_empty() => {
                    let key = key(unbent, false, Some((width, line)), zoom) ^ BENT;
                    self.place(key, item, scale, color, bend, mesh, |_, stroke| cut_line(stroke, unbent, width, line, tolerance));
                }
                _ => {
                    let key = key(&item.path, false, Some((width, line)), zoom);
                    let outline = || crate::stroke_outline(&item.path, width, line);
                    self.place(key, item, scale, color, None, mesh, |fill, stroke| cut(fill, stroke, &outline(), false, tolerance));
                }
            }
        }
    }

    /// Adds the cut shape `key` to `mesh` where `item` puts it, cutting it first if it is new;
    /// `bend` moves its points first, in the shape's own coordinates.
    #[allow(clippy::too_many_arguments)]
    fn place(
        &mut self,
        key: u64,
        item: &crate::Item,
        scale: f64,
        color: [f32; 4],
        bend: Option<&dyn Fn(Point) -> Point>,
        mesh: &mut Mesh,
        shape: impl FnOnce(&mut FillTessellator, &mut StrokeTessellator) -> Cut,
    ) {
        if color[3] <= 0.0 {
            return;
        }
        let frame = self.frame;
        if let Some((_, seen)) = self.cuts.get_mut(&key) {
            *seen = frame;
        } else {
            let cut = shape(&mut self.fill, &mut self.stroke);
            self.cuts.insert(key, (cut, frame));
        }
        let Some((cut, _)) = self.cuts.get(&key) else { return };
        let t = item.transform;
        // The band is one pixel wide on screen, whatever the shape's own scale.
        let pixel = 1.0 / (blur::scale_of(t).max(1e-9) * scale);
        let linear = Affine::new([t.as_coeffs()[0], t.as_coeffs()[1], t.as_coeffs()[2], t.as_coeffs()[3], 0.0, 0.0]);
        let local = |p: [f32; 2]| Point::new(p[0] as f64, p[1] as f64);
        let mut bender = Bender { bend, step: pixel * 0.25, last: None };
        let screen = |at: Point, offset: Vec2| to_f32(t * at + (linear * offset.to_point()).to_vec2());
        let base = mesh.points.len() as i32;
        mesh.points.extend(cut.points.iter().map(|p| to_f32(t * bend.map_or(local(*p), |b| b(local(*p))))));
        mesh.colors.extend(std::iter::repeat_n(color, cut.points.len()));
        mesh.indices.extend(cut.indices.iter().map(|i| base + *i as i32));
        let base = mesh.points.len() as i32;
        for (on, offset, _) in &cut.band {
            let (at, offset) = bender.place(*on, local(*offset).to_vec2() * pixel);
            mesh.points.push(screen(at, offset));
        }
        mesh.colors.extend(cut.band.iter().map(|(_, _, filled)| if *filled { color } else { [0.0; 4] }));
        mesh.indices.extend(cut.band_indices.iter().map(|i| base + *i as i32));
        if cut.line.is_empty() {
            return;
        }
        // A stroke along a bent line: its sides, then a band a pixel wide straddling each
        // outer side, opaque inside and clear outside.
        let sides: Vec<(Point, Vec2)> = cut.line.iter().map(|(on, offset)| bender.place(*on, local(*offset).to_vec2())).collect();
        let base = mesh.points.len() as i32;
        mesh.points.extend(sides.iter().map(|(at, offset)| screen(*at, *offset)));
        mesh.colors.extend(std::iter::repeat_n(color, sides.len()));
        mesh.indices.extend(cut.line_indices.iter().map(|i| base + *i as i32));
        let half = |offset: Vec2| {
            let l = offset.hypot();
            if l > 1e-12 { offset * (pixel * 0.5 / l) } else { Vec2::ZERO }
        };
        // Each outer point of the stroke gets its two band points once: `band[i]` is where they
        // start, for the `i`-th point of `line_band`.
        let base = mesh.points.len() as i32;
        for i in &cut.line_band {
            let (at, offset) = sides[*i as usize];
            mesh.points.push(screen(at, offset - half(offset)));
            mesh.points.push(screen(at, offset + half(offset)));
            mesh.colors.extend([color, [0.0; 4]]);
        }
        for [a, b] in &cut.line_edges {
            let (a, b) = (base + 2 * *a as i32, base + 2 * *b as i32);
            mesh.indices.extend([a, b, b + 1, a, b + 1, a + 1]);
        }
    }
}

/// Bends the points of a shape cut before its cages. Band and stroke points come in pairs on
/// one point of the outline, so the last point bent is remembered.
struct Bender<'a> {
    bend: Option<&'a dyn Fn(Point) -> Point>,
    /// How far apart the points are that tell which way the cage turns a direction.
    step: f64,
    last: Option<([f32; 2], Point, Vec2, Vec2)>,
}

impl Bender<'_> {
    /// Where point `on` of the outline goes, and where `offset` from it: the offset turns with
    /// the bent outline and keeps its length, so bands stay a pixel wide and strokes keep their
    /// width however the cage stretches the shape.
    fn place(&mut self, on: [f32; 2], offset: Vec2) -> (Point, Vec2) {
        let p = Point::new(on[0] as f64, on[1] as f64);
        let Some(bend) = self.bend else { return (p, offset) };
        let (at, dx, dy) = match self.last {
            Some((last, at, dx, dy)) if last == on => (at, dx, dy),
            _ => {
                let at = bend(p);
                let dx = bend(p + Vec2::new(self.step, 0.0)) - at;
                let dy = bend(p + Vec2::new(0.0, self.step)) - at;
                self.last = Some((on, at, dx, dy));
                (at, dx, dy)
            }
        };
        let turned = dx * offset.x + dy * offset.y;
        let (len, l) = (offset.hypot(), turned.hypot());
        (at, if l > 1e-12 { turned * (len / l) } else { offset })
    }
}

/// Cuts the stroke of `path`, `width` wide, along the line: see [`Cut::line`].
fn cut_line(stroke: &mut StrokeTessellator, path: &BezPath, width: f64, line: &crate::Line, tolerance: f32) -> Cut {
    let mut out = Cut::default();
    let cap = match line.cap {
        crate::Cap::Butt => LineCap::Butt,
        crate::Cap::Round => LineCap::Round,
        crate::Cap::Square => LineCap::Square,
    };
    let join = match line.join {
        crate::Join::Miter => LineJoin::Miter,
        crate::Join::Round => LineJoin::Round,
        crate::Join::Bevel => LineJoin::Bevel,
    };
    let options = StrokeOptions::tolerance(tolerance).with_line_width(width as f32).with_line_cap(cap).with_line_join(join).with_miter_limit(4.0);
    let mut buffers: VertexBuffers<([f32; 2], [f32; 2]), u32> = VertexBuffers::new();
    let ok = stroke.tessellate_path(
        &to_lyon(path),
        &options,
        &mut BuffersBuilder::new(&mut buffers, |v: StrokeVertex| {
            let on = v.position_on_path().to_array();
            let p = v.position().to_array();
            (on, [p[0] - on[0], p[1] - on[1]])
        }),
    );
    if ok.is_err() {
        return out;
    }
    // The outer sides are the edges of one triangle only; the band runs along them.
    let mut edges: HashMap<(u32, u32), u32> = HashMap::new();
    for tri in buffers.indices.as_chunks::<3>().0 {
        for (a, b) in [(tri[0], tri[1]), (tri[1], tri[2]), (tri[2], tri[0])] {
            *edges.entry((a.min(b), a.max(b))).or_default() += 1;
        }
    }
    let mut outer: Vec<(u32, u32)> = edges.into_iter().filter(|(_, n)| *n == 1).map(|(e, _)| e).collect();
    outer.sort_unstable();
    let mut place: HashMap<u32, u32> = HashMap::new();
    for (a, b) in outer {
        let mut at = |i: u32| {
            *place.entry(i).or_insert_with(|| {
                out.line_band.push(i);
                out.line_band.len() as u32 - 1
            })
        };
        let edge = [at(a), at(b)];
        out.line_edges.push(edge);
    }
    (out.line, out.line_indices) = (buffers.vertices, buffers.indices);
    out
}

/// Marks the key of a shape cut before its cages bend it: not the same cut as that outline drawn
/// straight, which is not split as finely.
const BENT: u64 = 0x9e37_79b9_7f4a_7c15;

/// `path` with its lines and curves split into pieces at most `step` long, so that bending its
/// points bends its sides too: a straight side crossing a cage curves with it.
fn split(path: &BezPath, step: f64) -> BezPath {
    let mut out = BezPath::new();
    let mut last = Point::ZERO;
    let mut start = Point::ZERO;
    let line = |out: &mut BezPath, from: Point, to: Point| {
        let n = ((to - from).hypot() / step).ceil().clamp(1.0, 4096.0) as usize;
        for k in 1..=n {
            out.line_to(from.lerp(to, k as f64 / n as f64));
        }
    };
    for el in path.elements() {
        match *el {
            PathEl::MoveTo(p) => {
                out.move_to(p);
                (last, start) = (p, p);
            }
            PathEl::LineTo(p) => {
                line(&mut out, last, p);
                last = p;
            }
            PathEl::ClosePath => {
                if last != start {
                    line(&mut out, last, start);
                }
                out.close_path();
                last = start;
            }
            PathEl::QuadTo(..) | PathEl::CurveTo(..) => {
                let seg = match *el {
                    PathEl::QuadTo(a, b) => PathSeg::Quad(QuadBez::new(last, a, b)),
                    PathEl::CurveTo(a, b, c) => PathSeg::Cubic(CubicBez::new(last, a, b, c)),
                    _ => unreachable!(),
                };
                // Curves stay curves, cut where the pieces are about `step` long.
                // The control polygon is never shorter than the curve.
                let hull = match seg {
                    PathSeg::Quad(q) => (q.p1 - q.p0).hypot() + (q.p2 - q.p1).hypot(),
                    PathSeg::Cubic(c) => (c.p1 - c.p0).hypot() + (c.p2 - c.p1).hypot() + (c.p3 - c.p2).hypot(),
                    PathSeg::Line(l) => (l.p1 - l.p0).hypot(),
                };
                let n = (hull / step).ceil().clamp(1.0, 4096.0) as usize;
                for k in 0..n {
                    let piece = seg.subsegment((k as f64 / n as f64)..((k + 1) as f64 / n as f64));
                    match piece {
                        PathSeg::Quad(q) => out.quad_to(q.p1, q.p2),
                        PathSeg::Cubic(c) => out.curve_to(c.p1, c.p2, c.p3),
                        PathSeg::Line(l) => out.line_to(l.p1),
                    }
                }
                last = seg.end();
            }
        }
    }
    out
}

/// Whether triangles show the item as the CPU would draw it.
fn meshable(item: &crate::Item) -> bool {
    item.blend == Blend::Normal && !matches!(item.fill, Some(Paint::Gradient { .. }))
}

/// Identifies a shape to cut: its outline, fill rule, stroke and how finely it is cut (in
/// steps of 2×, so zooming does not cut it again every frame).
fn key(path: &BezPath, even_odd: bool, stroke: Option<(f64, &crate::Line)>, zoom: f64) -> u64 {
    let mut h = DefaultHasher::new();
    for el in path.elements() {
        let pts: &[Point] = match el {
            PathEl::MoveTo(p) | PathEl::LineTo(p) => &[*p],
            PathEl::QuadTo(a, b) => &[*a, *b],
            PathEl::CurveTo(a, b, c) => &[*a, *b, *c],
            PathEl::ClosePath => &[],
        };
        std::mem::discriminant(el).hash(&mut h);
        for p in pts {
            p.x.to_bits().hash(&mut h);
            p.y.to_bits().hash(&mut h);
        }
    }
    even_odd.hash(&mut h);
    if let Some((width, line)) = stroke {
        width.to_bits().hash(&mut h);
        (line.cap as u8, line.join as u8).hash(&mut h);
        for d in &line.dash {
            d.to_bits().hash(&mut h);
        }
    }
    (zoom.log2().round() as i64).hash(&mut h);
    h.finish()
}

/// Cuts `path` into triangles and an edge band, `tolerance` the largest error in its units.
fn cut(fill: &mut FillTessellator, stroke: &mut StrokeTessellator, path: &BezPath, even_odd: bool, tolerance: f32) -> Cut {
    let mut out = Cut::default();
    let rule = if even_odd { FillRule::EvenOdd } else { FillRule::NonZero };
    let lyon = to_lyon(path);
    let mut core: VertexBuffers<[f32; 2], u32> = VertexBuffers::new();
    let options = FillOptions::tolerance(tolerance).with_fill_rule(rule);
    if fill.tessellate_path(&lyon, &options, &mut BuffersBuilder::new(&mut core, |v: FillVertex| v.position().to_array())).is_err() {
        return out;
    }
    (out.points, out.indices) = (core.vertices, core.indices);
    // Which side of each outline is filled is asked once per outline, of the outline cut into
    // lines, so holes and crossed outlines fade the right way under either fill rule.
    let polygon = flat(path, tolerance as f64);
    let filled = |p: Point| {
        let w = winding(&polygon, p);
        if even_odd { w % 2 != 0 } else { w != 0 }
    };
    for sub in subpaths(path) {
        let mut band: VertexBuffers<([f32; 2], [f32; 2], bool), u32> = VertexBuffers::new();
        // A band one unit wide: its offsets are scaled to a pixel when placed.
        let options = StrokeOptions::tolerance(tolerance).with_line_width(1.0).with_line_join(LineJoin::Miter).with_miter_limit(4.0);
        let ok = stroke.tessellate_path(
            &to_lyon(&sub),
            &options,
            &mut BuffersBuilder::new(&mut band, |v: StrokeVertex| {
                let on = v.position_on_path().to_array();
                let p = v.position().to_array();
                (on, [p[0] - on[0], p[1] - on[1]], v.side().is_positive())
            }),
        );
        let Some(&(on, offset, side)) = band.vertices.first() else { continue };
        if ok.is_err() {
            continue;
        }
        // A probe a little way along the first vertex's offset, at a scale where the outline
        // is about a pixel: tolerance is a fifth of one.
        let reach = tolerance as f64;
        let probe = Point::new(on[0] as f64 + offset[0] as f64 * reach, on[1] as f64 + offset[1] as f64 * reach);
        let inside = side == filled(probe);
        let base = out.band.len() as u32;
        out.band.extend(band.vertices.iter().map(|(on, offset, side)| (*on, *offset, *side == inside)));
        out.band_indices.extend(band.indices.iter().map(|i| base + i));
    }
    out
}

fn to_f32(p: Point) -> [f32; 2] {
    [p.x as f32, p.y as f32]
}

/// `path` as closed polygons.
fn flat(path: &BezPath, tolerance: f64) -> Vec<Vec<Point>> {
    let mut out: Vec<Vec<Point>> = Vec::new();
    motif_core::kurbo::flatten(path.iter(), tolerance, |el| match el {
        PathEl::MoveTo(p) => out.push(vec![p]),
        PathEl::LineTo(p) => {
            if let Some(last) = out.last_mut() {
                last.push(p);
            }
        }
        _ => {}
    });
    out
}

/// How many times the polygons wind round `p`, counting direction.
fn winding(polygons: &[Vec<Point>], p: Point) -> i32 {
    let mut w = 0;
    for poly in polygons {
        for (k, a) in poly.iter().enumerate() {
            let b = poly[(k + 1) % poly.len()];
            let cross = |a: Point, b: Point| (b - a).cross(p - a);
            if a.y <= p.y {
                if b.y > p.y && cross(*a, b) > 0.0 {
                    w += 1;
                }
            } else if b.y <= p.y && cross(*a, b) < 0.0 {
                w -= 1;
            }
        }
    }
    w
}

/// `ops` drawn on the CPU over the part of the canvas they cover.
fn patch(ops: &[Op], scale: f64, canvas: Rect) -> Option<Patch> {
    let view = Affine::scale(scale);
    let area = blur::bounds(ops, view)?.inflate(1.0, 1.0).intersect(canvas).expand();
    if area.width() < 1.0 || area.height() < 1.0 {
        return None;
    }
    let mut pixmap = Pixmap::new(area.width() as u32, area.height() as u32)?;
    cpu::draw_ops(&mut pixmap, ops, Affine::translate(-area.origin().to_vec2()) * view, true, None);
    Some(Patch { pixmap, at: Point::new(area.x0 / scale, area.y0 / scale), scale })
}

fn premultiplied(c: Color, opacity: f64) -> [f32; 4] {
    let a = c.a as f32 / 255.0 * opacity.clamp(0.0, 1.0) as f32;
    [c.r as f32 / 255.0 * a, c.g as f32 / 255.0 * a, c.b as f32 / 255.0 * a, a]
}

/// Each outline of `path` on its own.
fn subpaths(path: &BezPath) -> Vec<BezPath> {
    let mut out: Vec<BezPath> = Vec::new();
    for el in path.elements() {
        if matches!(el, PathEl::MoveTo(_)) || out.is_empty() {
            out.push(BezPath::new());
        }
        if let Some(last) = out.last_mut() {
            last.push(*el);
        }
    }
    out
}

fn to_lyon(path: &BezPath) -> LyonPath {
    let p = |p: Point| point(p.x as f32, p.y as f32);
    let mut b = LyonPath::builder();
    let mut open = false;
    for el in path.elements() {
        match *el {
            PathEl::MoveTo(a) => {
                if open {
                    b.end(false);
                }
                b.begin(p(a));
                open = true;
            }
            PathEl::LineTo(a) if open => {
                b.line_to(p(a));
            }
            PathEl::QuadTo(c, a) if open => {
                b.quadratic_bezier_to(p(c), p(a));
            }
            PathEl::CurveTo(c1, c2, a) if open => {
                b.cubic_bezier_to(p(c1), p(c2), p(a));
            }
            PathEl::ClosePath if open => {
                b.end(true);
                open = false;
            }
            _ => {}
        }
    }
    if open {
        b.end(false);
    }
    b.build()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Paints the pieces with tiny-skia's own triangle-free path: each triangle as a path. Close
    /// enough to the GPU to compare coverage.
    pub(super) fn paint(pieces: &[Piece], w: u32, h: u32, scale: f64) -> Pixmap {
        let mut px = Pixmap::new(w, h).unwrap();
        for piece in pieces {
            match piece {
                Piece::Patch(p) => {
                    let paint = tiny_skia::PixmapPaint::default();
                    let t = tiny_skia::Transform::from_translate((p.at.x * scale) as f32, (p.at.y * scale) as f32);
                    px.draw_pixmap(0, 0, p.pixmap.as_ref(), &paint, t, None);
                }
                Piece::Mesh(m) => {
                    for t in m.indices.chunks(3) {
                        let c: Vec<[f32; 4]> = t.iter().map(|i| m.colors[*i as usize]).collect();
                        let mean = |k: usize| (c[0][k] + c[1][k] + c[2][k]) / 3.0;
                        let a = mean(3);
                        if a <= 0.0 {
                            continue;
                        }
                        let mut pb = tiny_skia::PathBuilder::new();
                        for (n, i) in t.iter().enumerate() {
                            let [x, y] = m.points[*i as usize];
                            let (x, y) = (x * scale as f32, y * scale as f32);
                            if n == 0 { pb.move_to(x, y) } else { pb.line_to(x, y) }
                        }
                        pb.close();
                        let mut paint = tiny_skia::Paint::default();
                        paint.set_color(
                            tiny_skia::Color::from_rgba((mean(0) / a).min(1.0), (mean(1) / a).min(1.0), (mean(2) / a).min(1.0), a.min(1.0)).unwrap(),
                        );
                        paint.anti_alias = false;
                        px.fill_path(&pb.finish().unwrap(), &paint, tiny_skia::FillRule::Winding, tiny_skia::Transform::identity(), None);
                    }
                }
            }
        }
        px
    }

    fn doc(src: &str) -> Document {
        motif_script::parse(src).unwrap().doc
    }

    #[test]
    fn plain_shapes_are_triangles_and_effects_are_patches() {
        let d = doc("artboard \"t\" 100x60\nrect a at 30 30 size 30 30 fill #ff0000 stroke #000000 2\nellipse b at 70 30 size 20 20 fill #0000ff blur 4\n");
        let pieces = pieces(&d, 2.0);
        assert!(matches!(pieces[0], Piece::Mesh(_)));
        assert!(matches!(pieces[1], Piece::Patch(_)));
        assert_eq!(pieces.len(), 2);
        let Piece::Mesh(m) = &pieces[0] else { unreachable!() };
        assert_eq!(m.points.len(), m.colors.len());
        assert!(m.indices.len() % 3 == 0 && m.indices.iter().all(|i| (*i as usize) < m.points.len()));
    }

    #[test]
    fn every_example_is_cut_into_well_formed_pieces() {
        let dir = std::path::Path::new("../../examples");
        let mut files: Vec<_> =
            std::fs::read_dir(dir).unwrap().filter_map(|e| e.ok().map(|e| e.path())).filter(|p| p.extension().is_some_and(|x| x == "msc")).collect();
        files.sort();
        assert!(files.len() > 20);
        for f in files {
            let src = std::fs::read_to_string(&f).unwrap();
            for board in motif_script::parse_boards(&src, Some(dir)).unwrap() {
                let mut cutter = Cutter::default();
                for _ in 0..2 {
                    for piece in cutter.pieces(&board.doc, 1.5) {
                        if let Piece::Mesh(m) = piece {
                            assert_eq!(m.points.len(), m.colors.len(), "{f:?}");
                            assert!(m.indices.len() % 3 == 0 && m.indices.iter().all(|i| (*i as usize) < m.points.len()), "{f:?}");
                            assert!(m.points.iter().flatten().all(|v| v.is_finite()), "{f:?}");
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn triangles_cover_what_the_cpu_draws() {
        let src = "artboard \"t\" 120x80\nrect a at 40 40 size 50 40 r 8 fill #ff5a36 stroke #1b1d24 3\npath p at 90 40 d \"M -20 -20 L 20 -20 L 20 20 L -20 20 Z M -8 -8 L -8 8 L 8 8 L 8 -8 Z\" fill #3a7bd5\n";
        let d = doc(src);
        let cpu = crate::render_cpu(&d, crate::CpuOptions { scale: 2.0, antialias: true, background: false }).unwrap();
        let gpu = paint(&pieces(&d, 2.0), cpu.width(), cpu.height(), 2.0);
        let mut off = 0usize;
        for (a, b) in cpu.pixels().iter().zip(gpu.pixels()) {
            if (a.alpha() as i32 - b.alpha() as i32).abs() > 140 || (a.red() as i32 - b.red() as i32).abs() > 140 {
                off += 1;
            }
        }
        // Only edge pixels may differ, and not by much: no missing fills, no filled holes.
        assert!(off < 60, "{off} pixels differ");
        let at = |p: &Pixmap, x, y| p.pixel(x, y).unwrap().alpha();
        assert_eq!(at(&gpu, 180, 80), 0, "the hole stays open");
        assert_eq!(at(&gpu, 80, 80), 255);
        assert_eq!(at(&gpu, 0, 0), 0);
    }

    #[test]
    fn shapes_in_a_bending_cage_are_cut_once_and_bent_like_on_the_cpu() {
        let src = "artboard \"t\" 200x200\ngroup face at 100 100 deform 140 140 {\npath shape d \"M -50 -40 C -20 -70 30 -70 50 -40 L 50 50 L -50 50 Z M -10 -10 L 10 -10 L 10 10 L -10 10 Z\" fill #ff5a36 evenodd\npath line d \"M -60 60 Q 0 30 60 60\" fill none stroke #1b1d24 4 cap round\n}\nanim Idle 2s {\nface.warp-x4 0: 0 | 1: 35 | 2: 0\nface.warp-y1 0: 0 | 1: -20 | 2: 0\n}\n";
        let scene = motif_script::parse(src).unwrap();
        let mut cutter = Cutter::default();
        for (n, t) in [0.6, 1.0, 1.3].into_iter().enumerate() {
            let pose = scene.doc.animations[0].pose(&scene.doc, t);
            assert!(pose.nodes[pose.index_of("shape").unwrap()].cage_bend().is_some());
            assert!(pose.nodes[pose.index_of("line").unwrap()].cage_bend().is_some());
            let pieces = cutter.pieces(&pose, 2.0);
            // Every pose of the cage reuses the cuts: the shape and the line.
            assert_eq!(cutter.cuts.len(), 2, "pose {n}");
            let cpu = crate::render_cpu(&pose, crate::CpuOptions { scale: 2.0, antialias: true, background: false }).unwrap();
            let gpu = paint(&pieces, cpu.width(), cpu.height(), 2.0);
            let off = cpu.pixels().iter().zip(gpu.pixels()).filter(|(a, b)| (a.alpha() as i32 - b.alpha() as i32).abs() > 140).count();
            // Only a few edge pixels may differ: the bent sides follow the CPU's curves.
            assert!(off < 80, "pose {n}: {off} pixels differ");
            let hole = pose.world_transform(pose.index_of("shape").unwrap()) * pose.map_lattice_point(pose.index_of("shape").unwrap(), Point::ZERO);
            assert_eq!(gpu.pixel((hole.x * 2.0) as u32, (hole.y * 2.0) as u32).unwrap().alpha(), 0, "pose {n}: the hole stays open");
        }
    }
}
#[cfg(test)]
mod look {
    use super::*;
    #[test]
    #[ignore]
    fn look() {
        for f in ["directions", "character-platformer", "character-anime"] {
            let src = std::fs::read_to_string(format!("../../examples/{f}.msc")).unwrap();
            let d = motif_script::parse_boards(&src, Some(std::path::Path::new("../../examples"))).unwrap().remove(0).doc;
            let t = std::time::Instant::now();
            let n = 50;
            let mut p = Vec::new();
            for _ in 0..n {
                p = pieces(&d, 1.0);
            }
            let ms = t.elapsed().as_secs_f64() * 1000.0 / n as f64;
            let tri: usize = p.iter().map(|x| if let Piece::Mesh(m) = x { m.indices.len() / 3 } else { 0 }).sum();
            let patches: Vec<(u32, u32)> =
                p.iter().filter_map(|x| if let Piece::Patch(q) = x { Some((q.pixmap.width(), q.pixmap.height())) } else { None }).collect();
            let mut cutter = Cutter::default();
            let t = std::time::Instant::now();
            for _ in 0..n {
                p = cutter.pieces(&d, 1.0);
            }
            let kept = t.elapsed().as_secs_f64() * 1000.0 / n as f64;
            println!("{f}: pieces {ms:.3} ms, kept {kept:.3} ms, {tri} triangles, patches {patches:?}");
            let cpu = crate::render_cpu(&d, crate::CpuOptions { scale: 1.0, antialias: true, background: false }).unwrap();
            let gpu = super::tests::paint(&p, cpu.width(), cpu.height(), 1.0);
            cpu.save_png(format!("/tmp/claude-0/m-{f}-cpu.png")).unwrap();
            gpu.save_png(format!("/tmp/claude-0/m-{f}-gpu.png")).unwrap();
        }
    }
}
