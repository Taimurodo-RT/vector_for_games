//! The contract of a scene: what a game can bind to without reading the drawing. Its boards and
//! their sizes, animations, state machines with their states and inputs, pose parameters,
//! events, sides and pointer listeners. `motif contract` writes it as JSON, and a game (Forge)
//! checks its bindings against it when the `.msc` changes.

use motif_core::{Document, InputKind};
use serde::{Deserialize, Serialize};

/// The version of the contract's own layout: it grows when a field changes its meaning.
pub const CONTRACT_VERSION: u32 = 1;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Contract {
    pub contract: u32,
    pub boards: Vec<Board>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Board {
    pub name: String,
    pub width: f64,
    pub height: f64,
    pub animations: Vec<Animation>,
    pub machines: Vec<Machine>,
    pub parameters: Vec<Parameter>,
    /// Every event name the board's animations send, sorted, each once.
    pub events: Vec<String>,
    /// `directions`: the sides the game can face, from facing the viewer round clockwise.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sides: Option<Sides>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Animation {
    pub name: String,
    /// Seconds.
    pub duration: f64,
    #[serde(rename = "loop")]
    pub looping: bool,
    /// In time order.
    pub events: Vec<Event>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Event {
    pub name: String,
    /// Seconds into the animation.
    pub time: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Machine {
    pub name: String,
    pub inputs: Vec<Input>,
    /// The first is where the machine starts.
    pub states: Vec<State>,
    /// Objects whose pointer hover, press, click or position drive an input (`listen`).
    pub pointer: Vec<Listener>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Input {
    pub name: String,
    /// `number`, `bool` or `trigger`.
    pub kind: String,
    pub default: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct State {
    pub name: String,
    pub animation: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Listener {
    pub node: String,
    /// `hover`, `press`, `click`, `x` or `y`.
    pub event: String,
    pub input: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Parameter {
    pub id: String,
    pub name: String,
    pub min: f64,
    pub max: f64,
    pub default: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Sides {
    pub names: Vec<String>,
    /// The left-facing sides are the right-facing ones flipped by the game.
    pub mirror: bool,
}

/// The contract of every board of a scene, in the order they are written.
pub fn contract<'a>(boards: impl IntoIterator<Item = &'a Document>) -> Contract {
    Contract { contract: CONTRACT_VERSION, boards: boards.into_iter().map(board).collect() }
}

/// [`contract`] as JSON, two-space indented with a final newline, the way sample contracts are
/// stored.
pub fn contract_json<'a>(boards: impl IntoIterator<Item = &'a Document>) -> String {
    let mut s = serde_json::to_string_pretty(&contract(boards)).expect("a contract is plain data");
    s.push('\n');
    s
}

fn board(doc: &Document) -> Board {
    let mut events: Vec<String> = doc.animations.iter().flat_map(|a| a.events.iter().map(|e| e.name.clone())).collect();
    events.sort();
    events.dedup();
    Board {
        name: doc.artboard.name.clone(),
        width: doc.artboard.width,
        height: doc.artboard.height,
        animations: doc
            .animations
            .iter()
            .map(|a| {
                let mut events: Vec<Event> = a.events.iter().map(|e| Event { name: e.name.clone(), time: e.time }).collect();
                events.sort_by(|a, b| a.time.total_cmp(&b.time));
                Animation { name: a.name.clone(), duration: a.duration, looping: a.looping, events }
            })
            .collect(),
        machines: doc
            .machines
            .iter()
            .map(|m| Machine {
                name: m.name.clone(),
                inputs: m.inputs.iter().map(|i| Input { name: i.name.clone(), kind: kind(i.kind).into(), default: i.default }).collect(),
                states: m.states.iter().map(|s| State { name: s.name.clone(), animation: s.anim.clone() }).collect(),
                pointer: m.listeners.iter().map(|l| Listener { node: l.node.clone(), event: l.event.keyword().into(), input: l.input.clone() }).collect(),
            })
            .collect(),
        parameters: doc.parameters.iter().map(|p| Parameter { id: p.id.clone(), name: p.name.clone(), min: p.min, max: p.max, default: p.default }).collect(),
        events,
        sides: doc.directions.as_ref().map(|d| Sides { names: d.names().iter().map(|s| s.to_string()).collect(), mirror: d.mirror }),
    }
}

fn kind(k: InputKind) -> &'static str {
    match k {
        InputKind::Number => "number",
        InputKind::Bool => "bool",
        InputKind::Trigger => "trigger",
    }
}
