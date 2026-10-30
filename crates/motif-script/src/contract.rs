//! The contract of a scene: what a game can bind to without reading the drawing. Its boards and
//! their sizes, animations, state machines with their states and inputs, pose parameters,
//! events, sides and pointer listeners. `motif contract` writes it as JSON, and a game (Forge)
//! checks its bindings against it when the `.msc` changes. Inputs, states, animations, events
//! and parameters carry their hidden ids (`id=…`), so a binding survives a rename.

use motif_core::{Document, EventValue, IdKind, InputKind};
use serde::{Deserialize, Serialize};

/// The version of the contract's own layout: it grows when a field changes its meaning.
pub const CONTRACT_VERSION: u32 = 3;

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
    /// Every event the board's animations send, by name, sorted, each once.
    pub events: Vec<EventName>,
    /// `directions`: the sides the game can face, from facing the viewer round clockwise.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sides: Option<Sides>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Animation {
    /// The hidden id (`id=…`) the game binds to: it stays when the name changes. Missing until
    /// Motif saves the file.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
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
    /// The hidden id (`id=…`) the game binds to: it stays when the name changes. Missing until
    /// Motif saves the file.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    pub name: String,
    /// Seconds into the animation.
    pub time: f64,
    /// What the game is told with the event (`damage=2`): numbers, true/false and text.
    #[serde(default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub data: serde_json::Map<String, serde_json::Value>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EventName {
    /// The hidden id (`id=…`) the game binds to: it stays when the name changes. Missing until
    /// Motif saves the file.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    pub name: String,
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
    /// The hidden id (`id=…`) the game binds to: it stays when the name changes. Missing until
    /// Motif saves the file.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    pub name: String,
    /// `number`, `bool` or `trigger`.
    pub kind: String,
    pub default: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct State {
    /// The hidden id (`id=…`) the game binds to: it stays when the name changes. Missing until
    /// Motif saves the file.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
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
    /// The hidden id (`id=…`) the game binds to: it stays when the name changes. Missing until
    /// Motif saves the file.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// The name the script and the animation tracks use (`turn`).
    pub name: String,
    /// The name shown in Motif (`Поворот головы`).
    pub title: String,
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
    let id = |kind, owner: &str, name: &str| doc.ids.get(kind, owner, name).map(str::to_string);
    let mut names: Vec<&str> = doc.animations.iter().flat_map(|a| a.events.iter().map(|e| e.name.as_str())).collect();
    names.sort();
    names.dedup();
    let events = names.into_iter().map(|name| EventName { id: id(IdKind::Event, "", name), name: name.to_string() }).collect();
    Board {
        name: doc.artboard.name.clone(),
        width: doc.artboard.width,
        height: doc.artboard.height,
        animations: doc
            .animations
            .iter()
            .map(|a| {
                let mut events: Vec<Event> =
                    a.events.iter().map(|e| Event { id: id(IdKind::Event, "", &e.name), name: e.name.clone(), time: e.time, data: data(&e.data) }).collect();
                events.sort_by(|a, b| a.time.total_cmp(&b.time));
                Animation { id: id(IdKind::Animation, "", &a.name), name: a.name.clone(), duration: a.duration, looping: a.looping, events }
            })
            .collect(),
        machines: doc
            .machines
            .iter()
            .map(|m| Machine {
                name: m.name.clone(),
                inputs: m
                    .inputs
                    .iter()
                    .map(|i| Input { id: id(IdKind::Input, &m.name, &i.name), name: i.name.clone(), kind: kind(i.kind).into(), default: i.default })
                    .collect(),
                states: m.states.iter().map(|s| State { id: id(IdKind::State, &m.name, &s.name), name: s.name.clone(), animation: s.anim.clone() }).collect(),
                pointer: m.listeners.iter().map(|l| Listener { node: l.node.clone(), event: l.event.keyword().into(), input: l.input.clone() }).collect(),
            })
            .collect(),
        parameters: doc
            .parameters
            .iter()
            .map(|p| Parameter { id: id(IdKind::Parameter, "", &p.id), name: p.id.clone(), title: p.name.clone(), min: p.min, max: p.max, default: p.default })
            .collect(),
        events,
        sides: doc.directions.as_ref().map(|d| Sides { names: d.names().iter().map(|s| s.to_string()).collect(), mirror: d.mirror }),
    }
}

fn data(pairs: &[(String, EventValue)]) -> serde_json::Map<String, serde_json::Value> {
    pairs
        .iter()
        .map(|(k, v)| {
            let v = match v {
                EventValue::Number(n) => serde_json::json!(n),
                EventValue::Bool(b) => serde_json::json!(b),
                EventValue::Text(t) => serde_json::json!(t),
            };
            (k.clone(), v)
        })
        .collect()
}

fn kind(k: InputKind) -> &'static str {
    match k {
        InputKind::Number => "number",
        InputKind::Bool => "bool",
        InputKind::Trigger => "trigger",
    }
}
