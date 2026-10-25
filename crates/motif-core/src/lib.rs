//! Document model shared by the editor, the exporters and the game runtimes.

mod color;
mod ease;
mod ik;
mod machine;
mod model;
mod sim;
mod skin;
mod soft;
mod spring;
mod style;
pub mod text;

pub use color::Color;
pub use ease::{Dir, Ease, Family};
pub use kurbo;
pub use machine::{Cond, CondOp, Input, InputKind, Machine, MachinePlayer, State, Transition};
pub use model::{Animation, Artboard, Document, Gradient, Ik, Keyframe, Node, Prop, ShapeKind, Skin, SkinBind, Track, Value};
pub use sim::{SIM_FPS, SimCache, SimState};
pub use soft::Soft;
pub use spring::{SPRUNG, Spring};
pub use style::{Blend, Cap, Join, Shadow};
pub use text::{TextAlign, TextShape, TextStyle};
