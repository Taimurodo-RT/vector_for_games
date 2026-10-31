//! The live link between Motif and a game (Forge) open on the same computer: short JSON
//! messages over a WebSocket on `127.0.0.1`. The game asks Motif to open a file where it wants
//! it and sends the values it plays the character with; Motif tells the game when a file is
//! saved, what in its contract changed, and which object is selected.
//!
//! Motif runs the [`Server`] (off until it is switched on); the game connects with a
//! [`Client`] or any WebSocket of its own. Every message is one JSON object with a `type`:
//! [`ToMotif`] from the game, [`FromMotif`] from Motif. Nothing is needed for either program to
//! work: the link only adds the live conveniences.

mod diff;
mod server;

pub use diff::{Change, changes};
pub use server::{Client, Server};

use motif_script::contract::Contract;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// The version of the messages: it grows when one changes its meaning.
pub const PROTOCOL: u32 = 1;

/// The port Motif listens on unless told otherwise.
pub const DEFAULT_PORT: u16 = 47615;

/// What the game sends to Motif.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ToMotif {
    /// «Открыть в Motif»: the file, and where in it (a board by name, an animation or a state of
    /// its machine).
    Open {
        file: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        board: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        animation: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        state: Option<String>,
    },
    /// The values the running game drives the machine's inputs with, by input name: Motif plays
    /// the character with them (and a recorded run takes them down).
    GameValues {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        machine: Option<String>,
        values: BTreeMap<String, f64>,
    },
    /// What the game has to bind to, as hints: its values, its input actions, and the event
    /// names its logic waits for.
    Profile {
        #[serde(default)]
        values: Vec<String>,
        #[serde(default)]
        actions: Vec<String>,
        #[serde(default)]
        events: Vec<String>,
    },
}

/// What Motif sends to the game.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum FromMotif {
    /// Sent first on every connection.
    Hello { app: String, version: String, protocol: u32 },
    /// A file was saved: the game reads it again, in the editor and in the running game.
    Saved { file: String, contract: Contract },
    /// What the save changed in the file's contract against the one saved before: renamed by id,
    /// removed, added. Sent after `saved`, only when something changed.
    ContractChanged { file: String, changes: Vec<Change> },
    /// The object selected in Motif (none: nothing selected).
    Focus {
        file: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        object: Option<String>,
    },
    /// A message from the game that Motif could not do, and why.
    Error { message: String },
}

impl ToMotif {
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).expect("a message is plain data")
    }
}

impl FromMotif {
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).expect("a message is plain data")
    }
}
