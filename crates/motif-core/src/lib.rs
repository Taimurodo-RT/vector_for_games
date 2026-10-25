//! Document model shared by the editor, the exporters and the game runtimes.

mod color;
mod ease;
mod model;
mod style;
pub mod text;

pub use color::Color;
pub use ease::{Dir, Ease, Family};
pub use kurbo;
pub use model::{Animation, Artboard, Document, Gradient, Keyframe, Node, Prop, ShapeKind, Track, Value};
pub use style::{Blend, Cap, Join, Shadow};
pub use text::{TextAlign, TextShape, TextStyle};
