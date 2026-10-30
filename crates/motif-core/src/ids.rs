//! Hidden ids (`id=k7f2` at the end of a line) of what a game binds to: inputs and states of
//! machines, animations, events and pose parameters. A name can change; the id stays, so a
//! binding in the game survives a rename.

use std::collections::BTreeMap;

/// What an id is given to.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum IdKind {
    Input,
    State,
    Animation,
    /// An event name: every `event` line of that name, in any animation, shares it.
    Event,
    Parameter,
}

/// The ids of a scene, by kind, owner (the machine of an input or a state, else empty) and name.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Ids(BTreeMap<(IdKind, String, String), String>);

impl Ids {
    pub fn get(&self, kind: IdKind, owner: &str, name: &str) -> Option<&str> {
        self.0.get(&(kind, owner.to_string(), name.to_string())).map(String::as_str)
    }

    pub fn set(&mut self, kind: IdKind, owner: &str, name: &str, id: &str) {
        self.0.insert((kind, owner.to_string(), name.to_string()), id.to_string());
    }

    /// The name that has id `id` now.
    pub fn name_of(&self, kind: IdKind, owner: &str, id: &str) -> Option<&str> {
        self.0.iter().find(|((k, o, _), v)| *k == kind && o == owner && v.as_str() == id).map(|((_, _, name), _)| name.as_str())
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}
