//! Scene script (`.msc`): a compact text format for building a Motif scene.
//!
//! ```text
//! artboard "Main" 480x360 bg #fff4e4
//! let red = #ff5a36
//! ellipse ball at 150 150 size 72 fill red stroke #1b1d24 4
//! group coins at 60 40 {
//!   for i in 0..5 {
//!     ellipse coin{i} at {i * 24} 0 size 16 fill #ffd23f
//!   }
//! }
//! anim Idle 1.2s loop {
//!   ball.y 0: 120 ease-in | 0.6: 278 ease-out | 1.2: 120
//! }
//! ```
//!
//! [`parse`] builds the document and remembers the source span of every value, so that
//! [`set_prop`] can write a visual edit back into the text without reformatting it.

mod edit;
mod eval;
mod expr;
mod lexer;
mod parser;

pub use edit::{delete_key, fmt_value, set_key, set_prop};
use motif_core::{Document, Prop};
use std::collections::HashMap;
use std::fmt;

/// Byte range in the script text.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Span {
    pub start: usize,
    pub end: usize,
}

impl Span {
    pub fn new(start: usize, end: usize) -> Self {
        Span { start, end }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScriptError {
    /// 1-based line.
    pub line: usize,
    /// 1-based column, in characters.
    pub column: usize,
    pub offset: usize,
    pub message: String,
}

impl ScriptError {
    pub fn at(src: &str, offset: usize, message: impl Into<String>) -> Self {
        let offset = offset.min(src.len());
        let line = line_of(src, offset);
        let line_start = src[..offset].rfind('\n').map_or(0, |i| i + 1);
        ScriptError { line, column: src[line_start..offset].chars().count() + 1, offset, message: message.into() }
    }
}

impl fmt::Display for ScriptError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "строка {}:{}: {}", self.line, self.column, self.message)
    }
}

impl std::error::Error for ScriptError {}

/// Where a property value of a node is written in the script.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Source {
    pub span: Span,
    /// True when the span is a plain literal that belongs to this node only and can be rewritten in place.
    pub editable: bool,
    /// Set for one-value shorthands (`size 72`): the other property read from the same literal.
    pub paired: Option<Prop>,
    /// The value comes from an `override { }` entry.
    pub from_override: bool,
}

/// Where a node is declared in the script.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NodeSite {
    /// Byte offset at which a missing property can be appended to the node's line.
    pub insert_at: usize,
    /// The node was produced by a `for` loop, so its line is shared with other nodes.
    pub generated: bool,
    /// 1-based line of the declaration.
    pub line: usize,
}

/// A parsed scene: the document plus everything needed to edit the script it came from.
#[derive(Clone, Debug, PartialEq)]
pub struct Scene {
    pub doc: Document,
    pub sources: HashMap<(String, Prop), Source>,
    pub sites: HashMap<String, NodeSite>,
    /// Offset of the closing brace of the `override { }` block, when there is one.
    pub override_close: Option<usize>,
    /// Where each animation and its tracks are written, by animation name.
    pub anims: HashMap<String, AnimSite>,
}

/// Where an `anim` block is written in the script.
#[derive(Clone, Debug, PartialEq)]
pub struct AnimSite {
    /// Offset of the block's closing brace.
    pub close: usize,
    pub tracks: Vec<TrackSite>,
}

/// One `node.prop  t: v | …` line of an `anim` block.
#[derive(Clone, Debug, PartialEq)]
pub struct TrackSite {
    pub node: String,
    pub prop: Prop,
    pub keys: Vec<KeySite>,
}

/// One `t: v [curve]` key of a track, in the order it is written.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct KeySite {
    pub time: f64,
    /// The whole key, from the time to the curve.
    pub span: Span,
    /// Just the value.
    pub value: Span,
}

/// Parses and evaluates a scene script. Returns every error found, not only the first.
pub fn parse(src: &str) -> Result<Scene, Vec<ScriptError>> {
    let toks = lexer::lex(src).map_err(|e| vec![e])?;
    let mut parser = parser::Parser::new(src, toks);
    let stmts = parser.block(false);
    let mut ev = eval::Evaluator::new(src);
    ev.run(&stmts);
    let mut errors = parser.errors;
    errors.extend(ev.errors);
    if errors.is_empty() {
        Ok(ev.scene)
    } else {
        errors.sort_by_key(|e| e.offset);
        errors.dedup();
        Err(errors)
    }
}

pub(crate) fn line_of(src: &str, offset: usize) -> usize {
    src[..offset.min(src.len())].bytes().filter(|&b| b == b'\n').count() + 1
}

/// Formats a number the way a person would type it: no trailing zeros, at most two decimals.
pub fn fmt_num(v: f64) -> String {
    let r = (v * 100.0).round() / 100.0;
    if r == r.trunc() {
        format!("{}", r as i64)
    } else {
        let s = format!("{r:.2}");
        s.trim_end_matches('0').to_string()
    }
}

#[cfg(test)]
mod tests;
