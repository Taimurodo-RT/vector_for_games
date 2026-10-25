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

use motif_core::kurbo::{Affine, BezPath};
use motif_core::{Color, Document};

/// One shape ready to paint, in artboard space.
#[derive(Clone, Debug)]
pub struct Item {
    pub node: usize,
    pub path: BezPath,
    pub transform: Affine,
    pub fill: Option<Color>,
    pub stroke: Option<(Color, f64)>,
    /// Combined opacity of the node and its parent groups.
    pub opacity: f64,
}

/// Flattens the node tree into paint order, skipping hidden nodes and groups.
pub fn display_list(doc: &Document) -> Vec<Item> {
    (0..doc.nodes.len())
        .filter(|&i| doc.is_visible(i))
        .filter_map(|i| {
            let node = &doc.nodes[i];
            let path = node.path()?;
            let opacity = doc.world_opacity(i).clamp(0.0, 1.0);
            let stroke = node.stroke.filter(|_| node.stroke_width > 0.0).map(|c| (c, node.stroke_width));
            (opacity > 0.0 && (node.fill.is_some() || stroke.is_some())).then(|| Item {
                node: i,
                path,
                transform: doc.world_transform(i),
                fill: node.fill,
                stroke,
                opacity,
            })
        })
        .collect()
}

/// Applies opacity to a color's alpha.
pub(crate) fn faded(c: Color, opacity: f64) -> Color {
    Color { a: (c.a as f64 * opacity).round().clamp(0.0, 255.0) as u8, ..c }
}
