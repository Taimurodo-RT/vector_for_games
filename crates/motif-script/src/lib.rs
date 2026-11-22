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
//! symbol Gem size=16 color=#3b5bff {
//!   star body size {size} points 4 fill color
//! }
//! use Gem gem1 at 400 60 size=24
//! anim Idle 1.2s loop {
//!   ball.y 0: 120 ease-in | 0.6: 278 ease-out | 1.2: 120
//! }
//! ```
//!
//! [`parse`] builds the document and remembers the source span of every value, so that
//! [`set_prop`] can write a visual edit back into the text without reformatting it.

mod boards;
mod bone;
mod boolean;
mod combine_edit;
mod content;
pub mod contract;
mod detach;
pub use content::quoted_content;
pub use contract::{Contract, contract, contract_json};
pub use reyvec::{MorphFile, reyvec_morph, reyvec_morph_json, reyvec_morph_warnings};
pub mod correctives;
mod edit;
mod eval;
mod expr;
pub mod fonts;
pub mod format;
pub mod forms;
pub mod ids;
mod include;
mod lexer;
mod machine;
pub mod marks;
mod master;
mod outline;
mod parser;
mod repeat;
pub mod reyvec;
mod styles;
mod svg;
mod symbol;
mod themes;
mod variants;

mod palette;
pub mod parameters;
pub use boards::{
    BOARD_GAP, BoardGrid, BoardSize, board_file_name, board_layout, boards, head_end, new_board, parse_boards, parse_boards_as, parse_boards_themed,
    rename_artboard, set_artboard_size, set_board_cell,
};
pub use bone::{add_bone, add_ik_target, add_weight_stroke, attach_to_bone, clear_weights, fmt_stroke, set_ik, set_skin};
pub mod character;
mod character_controls;
pub use boolean::{BoolOp, boolean};
pub use combine_edit::{combine_nodes, flatten_combine, set_combine};
pub use detach::detach_copy;
pub use edit::{
    KeyAt, KeyCopy, KeyText, Place, Snippet, add_anim, add_guide, add_node, add_path, add_states, backdrop_text, copy_keys, copy_nodes, delete_anim,
    delete_key, delete_keys, delete_node, directions_text, duplicate_anim, fmt_value, gradient_text, group_nodes, guide_text, move_key, move_node, paste_keys,
    paste_nodes, paste_nodes_into, path_to_d, remove_guides, rename_anim, rename_node, restack, retime_keys, reverse_anim, set_anim_duration, set_anim_loop,
    set_backdrop, set_clip, set_directions, set_gradient, set_guide, set_key, set_key_ease, set_keys_ease, set_light, set_own_text, set_path, set_prop,
    set_purpose, set_stroke_gradient, set_style, set_world, ungroup, world_text, write_event, write_event_data, write_tracks,
};
pub use edit::{delete_nodes, move_nodes, restack_nodes, selected_roots, style_nodes};
pub use include::{IncludeSite, add_include, file_includes, include_path, inline_includes, remove_include, touched as include_touched, with_included_names};
pub use machine::{
    MachineItem, add_machine, delete_machine, delete_machine_item, edit_machine, fmt_listener, fmt_machine, fmt_transition, free_name, make_entry,
};
pub use master::{BoardUse, PartUses, delete_symbol_parts, symbol_part_uses};
use motif_core::{Color, Document, Prop};
pub use outline::{offset_path, shape_to_path, stroke_to_path};
pub use palette::{
    Collected, ColorUse, ColorWord, Replaced, add_gradient_swatch, add_swatch, collect_palette, color_uses, free_gradient_name, free_swatch_name,
    gradient_to_palette, name_color, rename_swatch, replace_color, scene_color_uses, set_gradient_ref, set_gradient_swatch, set_ref, set_swatch,
    swatch_name_ok,
};
/// Native node property names and their value arity, useful to avoid ambiguous generated IDs.
pub use parser::prop_arity;
pub use repeat::{RepeatKind, RepeatSite, Vary, VaryFill, repeat_nodes, set_repeat, set_repeat_around, set_vary};
use std::collections::HashMap;
use std::fmt;
pub use styles::{
    assign_style, delete_style, delete_style_in_file, detach_style, free_style_name, make_style, own_look, rename_style, rename_style_in_file, replace_style,
    reset_to_style, style_name_ok, write_into_style,
};
pub use svg::import_svg;
pub use symbol::{CopyChange, add_use, copy_changes, free_symbol_name, make_symbol, part_of, reset_copy, set_param};
pub use themes::{add_theme, delete_theme, free_theme_name, rename_theme, reset_in_theme, theme_name_ok, theme_names};
pub use variants::{
    add_variant, delete_variant, free_variant_name, rename_variant, set_variant, set_variant_value, set_variant_visible, variant_from_copy, variant_name_ok,
};

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
    /// The whole declaration, including a group's children.
    pub span: Span,
    /// The id as written (a template such as `coin{i}` for nodes made by loops).
    pub id_span: Span,
    /// The quoted name, when the node has one.
    pub name_span: Option<Span>,
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
    pub parameter_values: HashMap<String, Span>,
    pub driver_sites: HashMap<String, Vec<TrackSite>>,
    pub corrective_sites: HashMap<String, correctives::CorrectiveSite>,
    /// Offset of the first top-level `anim` or `override`: new objects are inserted before it.
    pub blocks_start: Option<usize>,
    /// Every `override` entry: the node it changes and the span of the whole entry.
    pub override_entries: Vec<(String, Span)>,
    /// The texts whose words an entry of `override` gives (`title.content = "Купить"`), by id:
    /// where the quoted words are written.
    pub own_texts: HashMap<String, Span>,
    /// The quoted `d "…"` outline of each path node (not for nodes made by loops).
    pub path_sources: HashMap<String, Span>,
    /// Each property as written on a node line, key and values (`shadow 0 4 8 #0006`), by node id
    /// and key. Not for nodes made by loops.
    pub prop_spans: HashMap<(String, String), Span>,
    /// The reference picture under the artboard, when the script has a `backdrop` line.
    pub backdrop: Option<Backdrop>,
    /// The guide lines of the board (`guide x 120`), in the order written.
    pub guides: Vec<GuideLine>,
    /// Every `symbol` block, in the order written.
    pub symbols: Vec<SymbolSite>,
    /// Every copy of a symbol (`use` line), by the id of its group.
    pub instances: HashMap<String, Instance>,
    /// Where each `machine` block and its lines are written, by machine name.
    pub machines: HashMap<String, MachineSite>,
    /// Where the `world` line is written, when there is one.
    pub world: Option<Span>,
    /// Where the `directions` line is written, when there is one.
    pub directions: Option<Span>,
    /// The ids after `turn` in it, where each is written.
    pub directions_turn: Vec<(String, Span)>,
    /// Where each `weights` block is written, by the id of its shape.
    pub weights: HashMap<String, WeightsSite>,
    /// The grid cell (column, row) of this artboard among the others of its file: `cell 1 0`.
    pub board_cell: Option<(u32, u32)>,
    /// Colors named by top-level `let` lines, in the order written: the scene's palette.
    pub palette: Vec<Swatch>,
    /// Properties written as a name (`fill main`): the name, by node id and property.
    pub refs: HashMap<(String, Prop), String>,
    /// Original palette references at each gradient stop, for semantic SVG export.
    pub gradient_refs: HashMap<(String, bool), Vec<Option<String>>>,
    /// Every `style` line, in the order written.
    pub styles: Vec<StyleSite>,
    /// Every `style card` written on an object's line, in the order written. A line inside a
    /// loop or a symbol is here once, however many objects it makes.
    pub style_uses: Vec<StyleUse>,
    /// The style each object takes, by the object's id.
    pub styled: HashMap<String, String>,
    /// Gradients named by top-level `let` lines (`let sky = linear 90 #6fb8ff #ffffff`), in the
    /// order written: the gradients of the palette.
    pub gradients: Vec<GradientSwatch>,
    /// The palette gradient a fill (`false`) or a stroke (`true`) takes by name, by node id.
    pub gradient_names: HashMap<(String, bool), String>,
    /// Every place where a color is written and read, `#ff5a36` or the name of a palette color.
    /// A line inside a loop or a symbol is here once. A symbol that is never placed and a style
    /// that nothing takes are not read, so their colors are not here.
    pub colors: Vec<ColorSite>,
    /// Every gradient written after `gradient` or `stroke-gradient`, in the same way.
    pub gradient_sites: Vec<GradientSite>,
    /// The themes of the scene (`theme dark { … }`), block by block as written.
    pub themes: Vec<ThemeSite>,
    /// The theme the scene was read in; `None` is the scene as it is written.
    pub theme: Option<String>,
    /// The copy whose symbol the scene was read for editing ([`Reading::symbol_of`]): the
    /// objects of this copy are written by the lines of the symbol, and an edit of them changes
    /// every copy. `None` when no symbol is edited.
    pub editing: Option<String>,
    /// Every group that repeats what is inside it (`repeat grid 4 2 step 40 30`), by its id.
    pub repeats: HashMap<String, RepeatSite>,
    /// Every `include "kit.msc"` line, in the order written. What a file gives (colors,
    /// gradients, styles, themes, symbols) is in the lists above like the scene's own, and every
    /// place of it is the place of its `include` line: [`Scene::included`] tells them apart.
    /// The places of colors, gradients and styles used inside those lines are not kept: an edit
    /// of the whole scene leaves the other file alone.
    pub includes: Vec<IncludeSite>,
}

/// How a script is read. The script is the same; what differs is which values are shown and
/// where an edit goes.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Reading<'a> {
    /// The theme to show (`theme dark { … }`); `None` is the scene as it is written.
    pub theme: Option<&'a str>,
    /// The id of a copy (`use Coin c1`) whose symbol is being edited. The objects of that copy
    /// are read as written by hand: their places in the script are the lines of the symbol, so
    /// the functions that edit an object edit the symbol. What the copy changes for itself in
    /// `override { }` is not applied meanwhile: the copy shows the symbol as it is.
    pub symbol_of: Option<&'a str>,
    /// The seed a game gives to what varies (`repeat … vary-rot 15`): it is added to every
    /// `vary-seed` of the script, so 0 reads the scene as it is written and any other number
    /// reads another variant of it, the same one every time.
    pub vary_seed: u32,
}

/// A place of the script where a color is written.
#[derive(Clone, Debug, PartialEq)]
pub struct ColorSite {
    pub span: Span,
    pub color: Color,
    /// The palette color it is written by, when it is a name.
    pub name: Option<String>,
    /// The value of a `let`: here the color is made, not used.
    pub decl: bool,
}

/// A place of the script where the gradient of a fill or a stroke is written.
#[derive(Clone, Debug, PartialEq)]
pub struct GradientSite {
    /// What follows the property word: `linear 90 #a #b`, or the name of a palette gradient.
    pub span: Span,
    pub gradient: motif_core::Gradient,
    /// The palette gradient it is written by, when it is a name.
    pub name: Option<String>,
}

/// A named gradient of the palette: `let sky = linear 90 #6fb8ff #ffffff`.
#[derive(Clone, Debug, PartialEq)]
pub struct GradientSwatch {
    pub name: String,
    pub name_span: Span,
    pub gradient: motif_core::Gradient,
    /// The value after `=`, from the kind word to the last color.
    pub span: Span,
    /// The palette color each of its colors is written by, when it is a name.
    pub stops: Vec<Option<String>>,
    /// The gradient is the one the theme being shown gives the name: `span` is in the theme.
    pub themed: bool,
    /// The value after `=` on the `let` line, whatever theme is shown.
    pub decl: Span,
}

/// A theme: `theme dark { … }`. A theme written in several blocks is here once for each.
#[derive(Clone, Debug, PartialEq)]
pub struct ThemeSite {
    pub name: String,
    pub name_span: Span,
    /// From `theme` to the closing brace.
    pub span: Span,
    /// Byte offset of the closing brace: a new line is written before it.
    pub close: usize,
    pub entries: Vec<ThemeEntrySite>,
}

/// One line of a theme: `paper = #1b1d24` or `style card fill paper`.
#[derive(Clone, Debug, PartialEq)]
pub struct ThemeEntrySite {
    /// The name of the palette or the style the line changes.
    pub name: String,
    pub name_span: Span,
    /// From the first word of the line to its last value.
    pub span: Span,
    /// The line is `style NAME …`.
    pub style: bool,
}

/// A named look: `style card fill #ffffff stroke ink 2 r 10`.
#[derive(Clone, Debug, PartialEq)]
pub struct StyleSite {
    pub name: String,
    pub name_span: Span,
    /// From `style` to the last value of the line.
    pub span: Span,
    /// Byte offset where a new property can be appended on the line.
    pub insert_at: usize,
    /// Each property of the style: its key and where it is written with its values.
    pub props: Vec<(String, Span)>,
    /// The line is the one the theme being shown gives the style: it is written in the theme.
    pub themed: bool,
    /// The style's own line, from `style` to its last value, whatever theme is shown.
    pub decl: Span,
    /// The name on the style's own line.
    pub decl_name: Span,
}

/// `style card` on the line of an object.
#[derive(Clone, Debug, PartialEq)]
pub struct StyleUse {
    pub style: String,
    /// The whole property: `style card`.
    pub span: Span,
    pub name_span: Span,
    /// The kind of object the line makes.
    pub kind: motif_core::ShapeKind,
    /// The properties of a style that the line writes itself, each with where it is written.
    pub own: Vec<(String, Span)>,
}

/// A named color of the palette: `let main = #3b6cff`.
#[derive(Clone, Debug, PartialEq)]
pub struct Swatch {
    pub name: String,
    pub name_span: Span,
    pub color: Color,
    /// The value after `=`; rewritten when the color is edited, if it is a plain color.
    pub span: Span,
    pub literal: bool,
    /// The value is the one the theme being shown gives the name: `span` is in the theme.
    pub themed: bool,
    /// The value after `=` on the `let` line, whatever theme is shown.
    pub decl: Span,
}

/// A `weights` block: the strokes match the shape's `skin.paint` one to one.
#[derive(Clone, Debug, PartialEq)]
pub struct WeightsSite {
    /// From `weights` to the closing brace.
    pub span: Span,
    /// Offset of the closing brace.
    pub close: usize,
    pub strokes: Vec<Span>,
}

/// Where a `machine` block is written. The line lists match the machine's inputs, states and
/// transitions one to one.
#[derive(Clone, Debug, PartialEq)]
pub struct MachineSite {
    /// From `machine` to the closing brace.
    pub span: Span,
    /// Offset of the closing brace.
    pub close: usize,
    pub inputs: Vec<Span>,
    pub listeners: Vec<Span>,
    pub states: Vec<Span>,
    pub transitions: Vec<Span>,
    /// The `any at x y` line, when there is one.
    pub any: Option<Span>,
}

/// A `symbol` block: a reusable piece with parameters.
#[derive(Clone, Debug, PartialEq)]
pub struct SymbolSite {
    pub name: String,
    /// `clip` on the `symbol` line: every copy is masked by its first object.
    pub clip: bool,
    /// Parameters with their default values, in the order written.
    pub params: Vec<(String, motif_core::Value)>,
    /// From `symbol` to the closing brace.
    pub span: Span,
    /// Its variants, in the order written.
    pub variants: Vec<VariantSite>,
    /// The objects written `hidden` in the symbol, by their ids inside it: a copy does not show
    /// them unless its variant does (`show badge`).
    pub hidden: Vec<String>,
}

/// A `variant` line of a symbol: a named set of parameter values, of objects that are not
/// shown and of hidden objects that are. A copy takes it with `variant main` on its `use` line.
#[derive(Clone, Debug, PartialEq)]
pub struct VariantSite {
    pub name: String,
    pub name_span: Span,
    /// The whole line, from `variant` to its last word.
    pub span: Span,
    /// The parameters it sets, in the order written.
    pub values: Vec<VariantValue>,
    /// The objects it hides, by their ids inside the symbol, and where each is written.
    pub hide: Vec<(String, Span)>,
    /// The objects it shows though the symbol has them `hidden`, the same way.
    pub show: Vec<(String, Span)>,
}

/// A parameter a variant sets: `face=accent`.
#[derive(Clone, Debug, PartialEq)]
pub struct VariantValue {
    pub name: String,
    /// Where the value is written.
    pub span: Span,
    /// The value, once a copy of the symbol has read it (a symbol never placed reads none).
    pub value: Option<motif_core::Value>,
}

/// A `use` line: one copy of a symbol, drawn as a group with the symbol's objects inside.
/// Objects of the copy get ids `copy-object` (`coin1-body`).
#[derive(Clone, Debug, PartialEq)]
pub struct Instance {
    pub symbol: String,
    /// Every parameter of the symbol with the value this copy uses.
    pub params: Vec<ParamSite>,
    /// The variant of the symbol the copy is (`variant main` on its line), and where both
    /// words are written.
    pub variant: Option<(String, Span)>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ParamSite {
    pub name: String,
    pub value: motif_core::Value,
    /// The value on the `use` line, when it is written there (otherwise the default applies).
    pub span: Option<Span>,
    /// The written value is a plain literal of this copy only and can be rewritten in place.
    pub editable: bool,
    /// The value is the one the copy's variant gives, not the default of the symbol.
    pub variant: bool,
}

/// A guide line of the editor: objects and points stick to it. Never drawn in renders or exports
/// and never part of what a game plays.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GuideLine {
    /// A vertical line at `x = at`; otherwise a horizontal one at `y = at`.
    pub vertical: bool,
    pub at: f64,
    /// Stays in place: the editor does not let the pointer take it.
    pub locked: bool,
    /// The whole `guide …` line.
    pub span: Span,
}

/// A reference picture drawn under the shapes in the editor only: never exported.
#[derive(Clone, Debug, PartialEq)]
pub struct Backdrop {
    /// File name as written, relative to the script.
    pub file: String,
    /// Top-left corner on the artboard.
    pub x: f64,
    pub y: f64,
    pub scale: f64,
    pub opacity: f64,
    pub hidden: bool,
    /// The picture is a sprite sheet: one frame of it is shown at a time.
    pub sheet: Option<Sheet>,
    /// Layers of a layered picture (PSD) turned off and on against how the file has them, by
    /// name: `hide "Фон" show "Эскиз"`. A group's name covers the layers inside it.
    pub hide: Vec<String>,
    pub show: Vec<String>,
    /// The whole `backdrop …` line.
    pub span: Span,
}

/// A backdrop cut into a grid of frames (`sheet 8 2 frames 14 fps 12`), read left to right, top
/// to bottom. While an animation plays the frames follow its time; otherwise `frame` stays.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sheet {
    pub cols: u32,
    pub rows: u32,
    /// Frames in use: the last row may be short. Never more than `cols × rows`.
    pub frames: u32,
    pub fps: f64,
    /// The frame shown while nothing plays, from 0.
    pub frame: u32,
}

impl Sheet {
    pub fn new(cols: u32, rows: u32) -> Sheet {
        let (cols, rows) = (cols.max(1), rows.max(1));
        Sheet { cols, rows, frames: cols * rows, fps: 12.0, frame: 0 }
    }

    /// The frame shown at `time` seconds into an animation, or the still one with `None`.
    pub fn frame_at(&self, time: Option<f64>) -> u32 {
        let count = self.frames.max(1);
        match time {
            Some(t) if t.is_finite() && t >= 0.0 => ((t * self.fps).floor() as u64 % count as u64) as u32,
            _ => self.frame.min(count - 1),
        }
    }
}

/// Where an `anim` block is written in the script.
#[derive(Clone, Debug, PartialEq)]
pub struct AnimSite {
    /// Offset of the block's closing brace.
    pub close: usize,
    /// The duration after the name, `1.2s`.
    pub duration: Span,
    /// The `loop` word, when present.
    pub loop_word: Option<Span>,
    pub tracks: Vec<TrackSite>,
    pub events: Vec<EventSite>,
}

/// One `event name t1 t2 …` line of an `anim` block.
#[derive(Clone, Debug, PartialEq)]
pub struct EventSite {
    pub name: String,
    /// From `event` to the last word of the line.
    pub span: Span,
    /// Each time with the span of its number.
    pub times: Vec<(f64, Span)>,
    /// The source of its `key=value` data, kept when the line is written again.
    pub data: Option<Span>,
}

/// One `node.prop  t: v | …` line of an `anim` block.
#[derive(Clone, Debug, PartialEq)]
pub struct TrackSite {
    pub node: String,
    pub prop: Prop,
    /// From the path to the end of the last key.
    pub span: Span,
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
    /// Just the time, `0.6`.
    pub time_span: Span,
    /// The curve name, when written.
    pub ease: Option<Span>,
}

/// Parses and evaluates a scene script. Returns every error found, not only the first.
pub fn parse(src: &str) -> Result<Scene, Vec<ScriptError>> {
    parse_in(src, None)
}

/// [`parse`] for a script saved in `base`: font files are looked up there.
pub fn parse_in(src: &str, base: Option<&std::path::Path>) -> Result<Scene, Vec<ScriptError>> {
    parse_themed(src, base, None)
}

/// [`parse_in`] with the scene shown in `theme` (`theme dark { … }`): the values the theme gives
/// to names of the palette and to styles stand in for the written ones. A theme the script does
/// not have changes nothing, and [`Scene::theme`] says which theme was read.
///
/// A script with themes is read once more for every other theme and for the scene as written:
/// a mistake in any theme is an error of the script whichever theme is shown, and the places of
/// colors, gradients and styles that only another theme reads are known too.
pub fn parse_themed(src: &str, base: Option<&std::path::Path>, theme: Option<&str>) -> Result<Scene, Vec<ScriptError>> {
    parse_as(src, base, Reading { theme, ..Reading::default() })
}

/// [`parse_themed`] with the whole choice of how the script is read: the theme and the symbol
/// being edited. A copy the script does not have changes nothing ([`Scene::editing`] is `None`).
pub fn parse_as(src: &str, base: Option<&std::path::Path>, reading: Reading) -> Result<Scene, Vec<ScriptError>> {
    let theme = reading.theme;
    let toks = lexer::lex(src).map_err(|e| vec![e])?;
    let mut parser = parser::Parser::new(src, toks).in_folder(base);
    let stmts = parser.block(false);
    let (stmts, includes) = include::spread(stmts, &parser.included);
    let read = |theme: Option<&str>| {
        let mut ev = eval::Evaluator::new(src, base);
        ev.include(&includes);
        ev.theme = theme.map(str::to_string);
        ev.editing = reading.symbol_of.map(str::to_string);
        ev.vary_seed = reading.vary_seed;
        ev.run(&stmts);
        ev.read_unused();
        (ev.scene, ev.errors)
    };
    let (mut scene, found) = read(theme);
    let mut errors = parser.errors;
    errors.extend(found);
    if !scene.doc.themes.is_empty() {
        let others: Vec<Option<String>> = std::iter::once(None).chain(scene.doc.themes.iter().cloned().map(Some)).filter(|t| *t != scene.theme).collect();
        let mut colors: std::collections::HashSet<usize> = scene.colors.iter().map(|c| c.span.start).collect();
        let mut gradients: std::collections::HashSet<usize> = scene.gradient_sites.iter().map(|g| g.span.start).collect();
        let mut uses: std::collections::HashSet<usize> = scene.style_uses.iter().map(|u| u.span.start).collect();
        for other in others {
            let (seen, found) = read(other.as_deref());
            errors.extend(found);
            scene.colors.extend(seen.colors.into_iter().filter(|c| colors.insert(c.span.start)));
            scene.gradient_sites.extend(seen.gradient_sites.into_iter().filter(|g| gradients.insert(g.span.start)));
            scene.style_uses.extend(seen.style_uses.into_iter().filter(|u| uses.insert(u.span.start)));
        }
    }
    if errors.is_empty() {
        Ok(scene)
    } else {
        errors.sort_by_key(|e| e.offset);
        let mut told: Vec<ScriptError> = Vec::new();
        for e in errors {
            if !told.contains(&e) {
                told.push(e);
            }
        }
        Err(told)
    }
}

pub(crate) fn line_of(src: &str, offset: usize) -> usize {
    src[..offset.min(src.len())].bytes().filter(|&b| b == b'\n').count() + 1
}

/// Formats a number with at most `decimals` decimals and no trailing zeros.
pub fn fmt_num_with(v: f64, decimals: usize) -> String {
    let s = format!("{v:.decimals$}");
    let s = if s.contains('.') { s.trim_end_matches('0').trim_end_matches('.') } else { &s };
    if s == "-0" { "0".into() } else { s.to_string() }
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

#[cfg(test)]
mod deformer_tests;

#[cfg(test)]
mod face_combination_tests;
