//! Document model shared by the editor, the exporters and the game runtimes.

mod color;
mod ease;
mod machine;
mod model;
mod style;
pub mod text;

pub use color::Color;
pub use ease::{Dir, Ease, Family};
pub use kurbo;
pub use machine::{Cond, CondOp, Input, InputKind, Machine, MachinePlayer, State, Transition};
pub use model::{Animation, Artboard, Document, Gradient, Keyframe, Node, Prop, ShapeKind, Track, Value};
pub use style::{Blend, Cap, Join, Shadow};
pub use text::{TextAlign, TextShape, TextStyle};
