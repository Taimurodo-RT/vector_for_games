//! What changed between two contracts of one file, as the game needs to hear it: a name the game
//! binds to renamed (the hidden id stayed), removed, or added.

use motif_script::contract::{Board, Contract};
use serde::{Deserialize, Serialize};

/// One change of what a game binds to.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Change {
    /// `board`, `animation`, `event`, `machine`, `input`, `state` or `parameter`.
    pub kind: String,
    /// The board it is on (empty for a board itself).
    pub board: String,
    /// The machine of an input or a state (empty otherwise).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub machine: String,
    /// Its hidden id, when it has one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// The name before; none when it was added.
    pub was: Option<String>,
    /// The name now; none when it was removed.
    pub now: Option<String>,
}

/// Everything renamed, removed or added from `before` to `after`, board by board in the order of
/// `after` (removed boards last). Things with an id are matched by it, the rest by name.
pub fn changes(before: &Contract, after: &Contract) -> Vec<Change> {
    let mut out = Vec::new();
    let mut groups: Vec<Group> = Vec::new();
    let boards = |c: &Contract| c.boards.iter().map(|b| Item { id: None, name: b.name.clone() }).collect::<Vec<_>>();
    groups.push(("board".into(), String::new(), String::new(), boards(before), boards(after)));
    for b in &after.boards {
        let Some(old) = before.boards.iter().find(|o| o.name == b.name) else { continue };
        for ((kind, board, was), (_, _, now)) in per_board(old).into_iter().zip(per_board(b)) {
            groups.push((kind, board, String::new(), was, now));
        }
        for m in &b.machines {
            let Some(om) = old.machines.iter().find(|o| o.name == m.name) else { continue };
            let inputs = |m: &motif_script::contract::Machine| m.inputs.iter().map(|i| Item { id: i.id.clone(), name: i.name.clone() }).collect();
            let states = |m: &motif_script::contract::Machine| m.states.iter().map(|s| Item { id: s.id.clone(), name: s.name.clone() }).collect();
            groups.push(("input".into(), b.name.clone(), m.name.clone(), inputs(om), inputs(m)));
            groups.push(("state".into(), b.name.clone(), m.name.clone(), states(om), states(m)));
        }
    }
    for (kind, board, machine, was, now) in groups {
        let same = |a: &Item, b: &Item| match (&a.id, &b.id) {
            (Some(x), Some(y)) => x == y,
            _ => a.name == b.name,
        };
        let change = |id: Option<String>, was: Option<String>, now: Option<String>| Change {
            kind: kind.clone(),
            board: board.clone(),
            machine: machine.clone(),
            id,
            was,
            now,
        };
        for n in &now {
            match was.iter().find(|w| same(w, n)) {
                Some(w) if w.name != n.name => out.push(change(n.id.clone(), Some(w.name.clone()), Some(n.name.clone()))),
                Some(_) => {}
                None => out.push(change(n.id.clone(), None, Some(n.name.clone()))),
            }
        }
        for w in was.iter().filter(|w| !now.iter().any(|n| same(w, n))) {
            out.push(change(w.id.clone(), Some(w.name.clone()), None));
        }
    }
    out
}

/// One kind of thing on a board (or in a machine): kind, board, machine, before, after.
type Group = (String, String, String, Vec<Item>, Vec<Item>);

struct Item {
    id: Option<String>,
    name: String,
}

/// The kinds of one board matched by id or name: animations, events, machines, parameters.
fn per_board(b: &Board) -> Vec<(String, String, Vec<Item>)> {
    let group = |kind: &str, items: Vec<Item>| (kind.to_string(), b.name.clone(), items);
    vec![
        group("animation", b.animations.iter().map(|a| Item { id: a.id.clone(), name: a.name.clone() }).collect()),
        group("event", b.events.iter().map(|e| Item { id: e.id.clone(), name: e.name.clone() }).collect()),
        group("machine", b.machines.iter().map(|m| Item { id: None, name: m.name.clone() }).collect()),
        group("parameter", b.parameters.iter().map(|p| Item { id: p.id.clone(), name: p.name.clone() }).collect()),
    ]
}
