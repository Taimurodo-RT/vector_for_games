//! Turns a [`Document`] into pixels.
//!
//! Both backends draw the same [`display_list`], so the editor preview (Vello on the GPU)
//! and exported frames (tiny-skia on the CPU) match.

mod cpu;
#[cfg(feature = "gpu")]
mod gpu;

pub use cpu::{CpuOptions, render_cpu};
#[cfg(feature = "gpu")]
pub use gpu::{build_scene, vello};

use motif_core::kurbo::{Affine, BezPath, Shape};
use motif_core::{Color, Document, Gradient};

/// One shape ready to paint, in artboard space.
#[derive(Clone, Debug)]
pub struct Item {
    pub node: usize,
    pub path: BezPath,
    pub transform: Affine,
    pub fill: Option<Paint>,
    pub stroke: Option<(Color, f64)>,
    /// Combined opacity of the node and its parent groups.
    pub opacity: f64,
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
            ops.push(Op::PushClip { path, transform: doc.world_transform(m) });
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
            ops.push(Op::Draw(Item { node: i, path, transform: doc.world_transform(i), fill, stroke, opacity }));
        }
    }
    ops.extend(open.iter().map(|_| Op::Pop));
    ops
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
