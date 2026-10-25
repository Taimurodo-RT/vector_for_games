//! Document model shared by the editor, the exporters and the game runtimes.

mod color;
mod model;

pub use color::Color;
pub use kurbo;
pub use model::{Animation, Artboard, Document, Ease, Gradient, Keyframe, Node, Prop, ShapeKind, Track, Value};
