//! Writes visual edits back into the script text, touching as little of it as possible.

use crate::{Scene, Span, TrackSite, fmt_num};
use motif_core::kurbo::{Affine, BezPath, PathEl, Point, Shape};
use motif_core::{Color, Prop, ShapeKind, Value};

pub fn fmt_value(v: Value) -> String {
    match v {
        Value::Num(n) => fmt_num(n),
        Value::Color(c) => c.to_hex(),
        Value::None => "none".into(),
    }
}

fn splice(src: &str, span: Span, text: &str) -> String {
    let mut out = String::with_capacity(src.len() + text.len());
    out.push_str(&src[..span.start]);
    out.push_str(text);
    out.push_str(&src[span.end..]);
    out
}

/// Returns the script with `node.prop` set to `value`, or `None` when the node does not exist.
///
/// * A literal that belongs to the node is replaced in place.
/// * A property missing from the node's line is appended to that line.
/// * A value computed from `let`, an expression or a loop is left alone; the change goes into
///   an `override { }` block instead, so the formula keeps working.
pub fn set_prop(src: &str, scene: &Scene, node: &str, prop: Prop, value: Value) -> Option<String> {
    let current = scene.doc.node(node)?;
    let text = fmt_value(value);
    let key = (node.to_string(), prop);

    if let Some(s) = scene.sources.get(&key) {
        if !s.editable {
            return Some(write_override(src, scene, node, prop, &text));
        }
        let repl = match s.paired {
            Some(other) => {
                let other_text = fmt_value(current.get(other));
                if matches!(prop, Prop::Width | Prop::ScaleX | Prop::SkewX) { format!("{text} {other_text}") } else { format!("{other_text} {text}") }
            }
            None => text,
        };
        return Some(splice(src, s.span, &repl));
    }

    let site = scene.sites.get(node)?;
    if site.generated {
        return Some(write_override(src, scene, node, prop, &text));
    }
    if prop == Prop::StrokeWidth
        && let Some(color) = scene.sources.get(&(node.to_string(), Prop::StrokeColor))
    {
        return Some(if color.editable && !color.from_override {
            splice(src, Span::new(color.span.end, color.span.end), &format!(" {text}"))
        } else {
            write_override(src, scene, node, prop, &text)
        });
    }

    let num = |p: Prop| fmt_value(current.get(p));
    let pick = |p: Prop| if p == prop { text.clone() } else { num(p) };
    let stroke_color = current.stroke.unwrap_or(Color::BLACK).to_hex();
    let stroke_width = if current.stroke_width > 0.0 { current.stroke_width } else { 2.0 };
    let addition = match prop {
        Prop::X | Prop::Y => format!("at {} {}", pick(Prop::X), pick(Prop::Y)),
        Prop::Width | Prop::Height => format!("size {} {}", pick(Prop::Width), pick(Prop::Height)),
        Prop::ScaleX | Prop::ScaleY => format!("scale {} {}", pick(Prop::ScaleX), pick(Prop::ScaleY)),
        Prop::Rotation => format!("rot {text}"),
        Prop::Opacity => format!("opacity {text}"),
        Prop::Radius => format!("r {text}"),
        Prop::Points => format!("points {text}"),
        Prop::Inner => format!("inner {text}"),
        Prop::Fill => format!("fill {text}"),
        Prop::StrokeColor => format!("stroke {text} {}", fmt_num(stroke_width)),
        Prop::StrokeWidth => format!("stroke {stroke_color} {text}"),
        Prop::SkewX | Prop::SkewY => format!("skew {} {}", pick(Prop::SkewX), pick(Prop::SkewY)),
        Prop::FontSize | Prop::LetterSpacing | Prop::Reveal | Prop::Wave | Prop::Phase => format!("{} {text}", prop.path_name()),
    };
    Some(splice(src, Span::new(site.insert_at, site.insert_at), &format!(" {addition}")))
}

fn write_override(src: &str, scene: &Scene, node: &str, prop: Prop, text: &str) -> String {
    let line = format!("  {node}.{} = {text}\n", prop.path_name());
    match scene.override_close {
        Some(close) => insert_before_close(src, close, &line),
        None => {
            let mut out = src.to_string();
            if !out.is_empty() && !out.ends_with('\n') {
                out.push('\n');
            }
            out.push_str("\n// Правки из редактора для объектов, заданных формулами или циклами\noverride {\n");
            out.push_str(&line);
            out.push_str("}\n");
            out
        }
    }
}

/// Inserts a whole `line` (ending in a newline) as the last entry of the block closed at `close`.
fn insert_before_close(src: &str, close: usize, line: &str) -> String {
    let line_start = src[..close].rfind('\n').map_or(0, |i| i + 1);
    // Keep the closing brace on its own line even if it shares a line with an entry.
    let (at, line) = if src[line_start..close].trim().is_empty() { (line_start, line.to_string()) } else { (close, format!("\n{line}")) };
    splice(src, Span::new(at, at), &line)
}

/// Keys closer in time than this are the same key.
const SAME_TIME: f64 = 0.005;

/// Key times are written with two decimals, so edits snap to them.
fn key_time(time: f64) -> f64 {
    (time.max(0.0) * 100.0).round() / 100.0
}

fn find_track<'a>(scene: &'a Scene, anim: &str, node: &str, prop: Prop) -> Option<&'a TrackSite> {
    scene.anims.get(anim)?.tracks.iter().find(|t| t.node == node && t.prop == prop)
}

/// Returns the script with a key `node.prop = value` at `time` in animation `anim`, or `None` when
/// the animation or node does not exist.
///
/// * A key already at that time gets its value replaced; its curve stays.
/// * Otherwise the key is inserted among the track's keys in time order.
/// * A property without a track gets a new track line at the end of the block. When the key is not
///   at 0, the track starts with the scene value at 0, so the change animates instead of replacing
///   the value for the whole animation.
pub fn set_key(src: &str, scene: &Scene, anim: &str, node: &str, prop: Prop, time: f64, value: Value) -> Option<String> {
    let site = scene.anims.get(anim)?;
    let base = scene.doc.node(node)?.get(prop);
    let time = key_time(time);
    let key = format!("{}: {}", fmt_num(time), fmt_value(value));
    let Some(track) = find_track(scene, anim, node, prop) else {
        let keys = if time < SAME_TIME { key } else { format!("0: {} | {key}", fmt_value(base)) };
        return Some(insert_before_close(src, site.close, &format!("  {node}.{}  {keys}\n", prop.path_name())));
    };
    if let Some(k) = track.keys.iter().find(|k| (k.time - time).abs() < SAME_TIME) {
        return Some(splice(src, k.value, &fmt_value(value)));
    }
    Some(match track.keys.iter().find(|k| k.time > time) {
        Some(next) => splice(src, Span::new(next.span.start, next.span.start), &format!("{key} | ")),
        None => {
            let end = track.keys.last()?.span.end;
            splice(src, Span::new(end, end), &format!(" | {key}"))
        }
    })
}

/// Returns the script without the key of `node.prop` at `time`, or `None` when there is no such key.
/// Removing the last key of a track removes the whole track line.
pub fn delete_key(src: &str, scene: &Scene, anim: &str, node: &str, prop: Prop, time: f64) -> Option<String> {
    let track = find_track(scene, anim, node, prop)?;
    let time = key_time(time);
    let i = track.keys.iter().position(|k| (k.time - time).abs() < SAME_TIME)?;
    let k = track.keys[i];
    let span = if track.keys.len() == 1 {
        let start = src[..k.span.start].rfind('\n').map_or(0, |n| n + 1);
        let end = src[k.span.end..].find('\n').map_or(src.len(), |n| k.span.end + n + 1);
        Span::new(start, end)
    } else if i == 0 {
        Span::new(k.span.start, track.keys[1].span.start)
    } else {
        Span::new(track.keys[i - 1].span.end, k.span.end)
    };
    Some(splice(src, span, ""))
}

/// Expands a span to the whole lines it touches, including the final newline.
pub(crate) fn whole_lines(src: &str, span: Span) -> Span {
    let start = src[..span.start].rfind('\n').map_or(0, |i| i + 1);
    let end = src[span.end..].find('\n').map_or(src.len(), |i| span.end + i + 1);
    Span::new(start, end)
}

/// Removes several spans at once; overlapping spans are merged.
pub(crate) fn remove_spans(src: &str, mut spans: Vec<Span>) -> String {
    spans.sort_by_key(|s| s.start);
    let mut out = String::with_capacity(src.len());
    let mut at = 0;
    for s in spans {
        if s.start > at {
            out.push_str(&src[at..s.start]);
        }
        at = at.max(s.end);
    }
    out.push_str(&src[at.min(src.len())..]);
    out
}

/// A fresh id such as `rect3` that no node uses yet.
pub(crate) fn free_id(scene: &Scene, base: &str) -> String {
    (1..).map(|i| format!("{base}{i}")).find(|id| scene.doc.node(id).is_none()).expect("ids are unbounded")
}

/// Returns the script with a new shape added on top of the others, and the new shape's id.
/// The line goes before the first `anim`/`override` block, so declarations stay together.
pub fn add_node(src: &str, scene: &Scene, kind: ShapeKind, center: (f64, f64), size: (f64, f64)) -> (String, String) {
    let id = free_id(scene, kind.keyword());
    let fill = match kind {
        ShapeKind::Rect => "#3b5bff",
        ShapeKind::Ellipse => "#ff5a36",
        ShapeKind::Star => "#ffc531",
        ShapeKind::Polygon => "#4fd38c",
        ShapeKind::Text => "#1b1d24",
        ShapeKind::Group | ShapeKind::Path => "none",
    };
    if kind == ShapeKind::Text {
        // A dragged box sets the letter height; a click gives the usual size.
        let fs = if size.1 >= 8.0 && size.1 != 80.0 { size.1.round() } else { 32.0 };
        let line = format!("text {id} at {} {} content \"Текст\" font-size {} fill {fill}\n", fmt_num(center.0), fmt_num(center.1), fmt_num(fs));
        return (insert_declaration(src, scene, &line), id);
    }
    let size = if (size.0 - size.1).abs() < 1e-9 { fmt_num(size.0) } else { format!("{} {}", fmt_num(size.0), fmt_num(size.1)) };
    let line = format!("{} {id} at {} {} size {size} fill {fill}\n", kind.keyword(), fmt_num(center.0), fmt_num(center.1));
    (insert_declaration(src, scene, &line), id)
}

/// Inserts a top-level declaration line before the first `anim`/`override` block.
pub(crate) fn insert_declaration(src: &str, scene: &Scene, line: &str) -> String {
    match scene.blocks_start {
        Some(at) => {
            let at = src[..at].rfind('\n').map_or(0, |i| i + 1);
            splice(src, Span::new(at, at), &format!("{line}\n"))
        }
        None => {
            let mut out = src.to_string();
            if !out.is_empty() && !out.ends_with('\n') {
                out.push('\n');
            }
            out.push_str(line);
            out
        }
    }
}

/// SVG path data with short numbers: `M 0 -40 C 20 -40 30 -20 30 0 L 0 40 Z`.
pub fn path_to_d(path: &BezPath) -> String {
    let pt = |p: Point| format!("{} {}", fmt_num(p.x), fmt_num(p.y));
    let parts: Vec<String> = path
        .elements()
        .iter()
        .map(|el| match *el {
            PathEl::MoveTo(p) => format!("M {}", pt(p)),
            PathEl::LineTo(p) => format!("L {}", pt(p)),
            PathEl::QuadTo(a, p) => format!("Q {} {}", pt(a), pt(p)),
            PathEl::CurveTo(a, b, p) => format!("C {} {} {}", pt(a), pt(b), pt(p)),
            PathEl::ClosePath => "Z".into(),
        })
        .collect();
    parts.join(" ")
}

/// Returns the script with the outline of path `node` replaced. `None` when the node is not a path
/// written in the script by hand (paths made by loops are edited in the text).
pub fn set_path(src: &str, scene: &Scene, node: &str, path: &BezPath) -> Option<String> {
    let n = scene.doc.node(node)?;
    let site = scene.sites.get(node)?;
    if n.kind != ShapeKind::Path || site.generated {
        return None;
    }
    let d = format!("\"{}\"", path_to_d(path));
    Some(match scene.path_sources.get(node) {
        Some(span) => splice(src, *span, &d),
        None => splice(src, Span::new(site.insert_at, site.insert_at), &format!(" d {d}")),
    })
}

/// The script text of a gradient property: `gradient linear 90 #a #b` or `gradient radial #a #b`.
pub fn gradient_text(g: &motif_core::Gradient) -> String {
    let mut out = String::from(if g.radial { "gradient radial" } else { "gradient linear " });
    if !g.radial {
        out.push_str(&fmt_num(g.angle));
    }
    for c in &g.colors {
        out.push(' ');
        out.push_str(&c.to_hex());
    }
    out
}

/// Returns the script with the gradient of `node` set, replaced or removed (`None`: back to the
/// plain fill). `None` when the node comes from a loop.
pub fn set_gradient(src: &str, scene: &Scene, node: &str, gradient: Option<&motif_core::Gradient>) -> Option<String> {
    set_style(src, scene, node, "gradient", gradient.map(gradient_text).as_deref())
}

/// Returns the script with the `clip` flag of group `node` turned on or off.
pub fn set_clip(src: &str, scene: &Scene, node: &str, on: bool) -> Option<String> {
    if scene.doc.node(node)?.kind != ShapeKind::Group {
        return None;
    }
    set_style(src, scene, node, "clip", on.then_some("clip"))
}

/// Writes a whole property of a node line: `text` is the key with its values (`dash 6 4`) and
/// replaces what is written for `key`, or is added at the end of the line; `None` removes it.
/// `None` when the node comes from a loop.
pub fn set_style(src: &str, scene: &Scene, node: &str, key: &str, text: Option<&str>) -> Option<String> {
    let site = scene.sites.get(node)?;
    if site.generated {
        return None;
    }
    Some(match (scene.prop_spans.get(&(node.to_string(), key.to_string())), text) {
        (Some(span), Some(t)) => splice(src, *span, t),
        (Some(span), None) => splice(src, with_space_before(src, *span), ""),
        (None, Some(t)) => splice(src, Span::new(site.insert_at, site.insert_at), &format!(" {t}")),
        (None, None) => src.to_string(),
    })
}

/// Widens `span` to take the spaces in front of it, so removing it leaves no double space.
fn with_space_before(src: &str, span: Span) -> Span {
    let start = src[..span.start].trim_end_matches([' ', '\t']).len();
    Span::new(start, span.end)
}

/// Returns the script with a new path whose outline is given in artboard space, and its id.
/// The node is placed at the outline's centre; closed outlines get a fill, open ones a stroke.
pub fn add_path(src: &str, scene: &Scene, outline: &BezPath) -> (String, String) {
    let id = free_id(scene, "path");
    let c = outline.bounding_box().center();
    let c = Point::new(c.x.round(), c.y.round());
    let local = Affine::translate(-c.to_vec2()) * outline.clone();
    let closed = outline.elements().last() == Some(&PathEl::ClosePath);
    let paint = if closed { "fill #4fd38c" } else { "fill none stroke #1b1d24 3" };
    let line = format!("path {id} at {} {} d \"{}\" {paint}\n", fmt_num(c.x), fmt_num(c.y), path_to_d(&local));
    (insert_declaration(src, scene, &line), id)
}

/// Returns the script without `node`, its children, and every key and override that refers to them.
/// `None` when the node does not exist or comes from a `for` loop (its line makes other nodes too).
pub fn delete_node(src: &str, scene: &Scene, node: &str) -> Option<String> {
    Some(remove_spans(src, node_spans(src, scene, node)?))
}

/// The whole lines declaring `node` and its children, with their animation tracks and overrides.
pub(crate) fn node_spans(src: &str, scene: &Scene, node: &str) -> Option<Vec<Span>> {
    let site = scene.sites.get(node)?;
    if site.generated {
        return None;
    }
    let doc = &scene.doc;
    let root = doc.index_of(node)?;
    let ids: Vec<&str> = (0..doc.nodes.len())
        .filter(|&i| {
            let mut cur = Some(i);
            while let Some(c) = cur {
                if c == root {
                    return true;
                }
                cur = doc.nodes[c].parent;
            }
            false
        })
        .map(|i| doc.nodes[i].id.as_str())
        .collect();
    let mut spans = vec![whole_lines(src, site.span)];
    for anim in scene.anims.values() {
        spans.extend(anim.tracks.iter().filter(|t| ids.contains(&t.node.as_str())).map(|t| whole_lines(src, t.span)));
    }
    spans.extend(scene.override_entries.iter().filter(|(n, _)| ids.contains(&n.as_str())).map(|(_, s)| whole_lines(src, *s)));
    Some(spans)
}

/// Returns the script with a new empty animation at the end, and its name.
pub fn add_anim(src: &str, scene: &Scene) -> (String, String) {
    let name = (1..).map(|i| format!("Anim{i}")).find(|n| scene.doc.animation(n).is_none()).expect("names are unbounded");
    let mut out = src.to_string();
    if !out.is_empty() && !out.ends_with('\n') {
        out.push('\n');
    }
    out.push_str(&format!("\nanim {name} 1s loop {{\n}}\n"));
    (out, name)
}

pub fn set_anim_duration(src: &str, scene: &Scene, anim: &str, seconds: f64) -> Option<String> {
    let site = scene.anims.get(anim)?;
    Some(splice(src, site.duration, &format!("{}s", fmt_num(seconds.max(0.01)))))
}

pub fn set_anim_loop(src: &str, scene: &Scene, anim: &str, looping: bool) -> Option<String> {
    let site = scene.anims.get(anim)?;
    match (looping, site.loop_word) {
        (true, None) => Some(splice(src, Span::new(site.duration.end, site.duration.end), " loop")),
        (false, Some(word)) => Some(splice(src, Span::new(site.duration.end, word.end), "")),
        _ => Some(src.to_string()),
    }
}

/// Returns the script with the key of `node.prop` at `from` moved to `to`. `None` when there is no
/// such key or another key of the track already sits at `to`.
pub fn move_key(src: &str, scene: &Scene, anim: &str, node: &str, prop: Prop, from: f64, to: f64) -> Option<String> {
    let track = find_track(scene, anim, node, prop)?;
    let (from, to) = (key_time(from), key_time(to));
    let key = track.keys.iter().find(|k| (k.time - from).abs() < SAME_TIME)?;
    if (from - to).abs() < SAME_TIME {
        return Some(src.to_string());
    }
    if track.keys.iter().any(|k| (k.time - to).abs() < SAME_TIME) {
        return None;
    }
    Some(splice(src, key.time_span, &fmt_num(to)))
}

/// Returns the script with the curve of the key of `node.prop` at `time` set to `ease`
/// as written in the script (`ease-out`, `cubic 0.4 0 0.2 1`, see `Ease::script`).
pub fn set_key_ease(src: &str, scene: &Scene, anim: &str, node: &str, prop: Prop, time: f64, ease: &str) -> Option<String> {
    let track = find_track(scene, anim, node, prop)?;
    let time = key_time(time);
    let key = track.keys.iter().find(|k| (k.time - time).abs() < SAME_TIME)?;
    Some(match key.ease {
        Some(span) => splice(src, span, ease),
        None => splice(src, Span::new(key.value.end, key.value.end), &format!(" {ease}")),
    })
}

/// Copied declarations: the text of each statement with the places to rewrite on paste.
#[derive(Clone, Debug, PartialEq)]
pub struct Snippet {
    pub text: String,
    /// Ids written in the text (relative spans), with whether they are loop templates.
    pub ids: Vec<(Span, String, bool)>,
    /// The `at x y` literals of the copied node, to offset the copy.
    pub at: Option<(Span, Span)>,
}

/// Copies the declarations of `ids` (a node's children come along with it). Nodes made by loops
/// are skipped: their line makes several nodes.
pub fn copy_nodes(scene: &Scene, src: &str, ids: &[String]) -> Vec<Snippet> {
    let doc = &scene.doc;
    let chosen: Vec<usize> = ids.iter().filter_map(|id| doc.index_of(id)).collect();
    let has_chosen_ancestor = |i: usize| {
        let mut cur = doc.nodes[i].parent;
        while let Some(p) = cur {
            if chosen.contains(&p) {
                return true;
            }
            cur = doc.nodes[p].parent;
        }
        false
    };
    let mut roots: Vec<usize> = chosen.iter().copied().filter(|&i| !has_chosen_ancestor(i)).collect();
    roots.sort_unstable();
    let mut out = Vec::new();
    for i in roots {
        let id = &doc.nodes[i].id;
        let Some(site) = scene.sites.get(id).filter(|s| !s.generated) else { continue };
        let span = site.span;
        let rel = |s: Span| Span::new(s.start - span.start, s.end - span.start);
        let inside = |s: Span| s.start >= span.start && s.end <= span.end;
        let mut names: Vec<(Span, String, bool)> = Vec::new();
        for (nid, ns) in &scene.sites {
            if inside(ns.span) && !names.iter().any(|(s, _, _)| *s == rel(ns.id_span)) {
                let written = src[ns.id_span.start..ns.id_span.end].to_string();
                names.push((rel(ns.id_span), if ns.generated { written } else { nid.clone() }, ns.generated));
            }
        }
        names.sort_by_key(|(s, _, _)| s.start);
        let at = match (scene.sources.get(&(id.clone(), Prop::X)), scene.sources.get(&(id.clone(), Prop::Y))) {
            (Some(x), Some(y)) if x.editable && y.editable && !x.from_override && inside(x.span) && inside(y.span) => Some((rel(x.span), rel(y.span))),
            _ => None,
        };
        out.push(Snippet { text: src[span.start..span.end].to_string(), ids: names, at });
    }
    out
}

/// Inserts copies of `snippets` with fresh ids, moved by `offset`. Returns the script and the ids
/// of the pasted top-level nodes.
pub fn paste_nodes(src: &str, scene: &Scene, snippets: &[Snippet], offset: (f64, f64)) -> (String, Vec<String>) {
    let mut taken: Vec<String> = scene.doc.nodes.iter().map(|n| n.id.clone()).collect();
    let mut block = String::new();
    let mut roots = Vec::new();
    for snip in snippets {
        let mut edits: Vec<(Span, String)> = Vec::new();
        // Plain ids get the next free number; loop templates get a suffix so every copy is new.
        let templates: Vec<&String> = snip.ids.iter().filter(|(_, _, g)| *g).map(|(_, t, _)| t).collect();
        let suffix = (2..)
            .map(|k| format!("_{k}"))
            .find(|sfx| !templates.iter().any(|t| taken.iter().any(|id| id.starts_with(t.split('{').next().unwrap_or("")) && id.ends_with(sfx.as_str()))))
            .expect("suffixes are unbounded");
        for (i, (span, id, generated)) in snip.ids.iter().enumerate() {
            let new = if *generated {
                format!("{id}{suffix}")
            } else {
                let base = id.trim_end_matches(|c: char| c.is_ascii_digit());
                let base = if base.is_empty() { "copy" } else { base };
                let new = (1..).map(|k| format!("{base}{k}")).find(|c| !taken.contains(c)).expect("ids are unbounded");
                taken.push(new.clone());
                new
            };
            if i == 0 {
                roots.push(new.clone());
            }
            edits.push((*span, new));
        }
        if let Some((x, y)) = snip.at {
            for (span, d) in [(x, offset.0), (y, offset.1)] {
                if let Ok(v) = snip.text[span.start..span.end].parse::<f64>() {
                    edits.push((span, fmt_num(v + d)));
                }
            }
        }
        edits.sort_by_key(|(s, _)| std::cmp::Reverse(s.start));
        let mut text = snip.text.clone();
        for (span, new) in edits {
            text = splice(&text, span, &new);
        }
        block.push_str(&text);
        block.push('\n');
    }
    (insert_declaration(src, scene, &block), roots)
}

/// The script line of a backdrop, leaving out values that are at their defaults.
pub fn backdrop_text(b: &crate::Backdrop) -> String {
    let mut out = format!("backdrop \"{}\"", b.file.replace('"', "'"));
    if b.x != 0.0 || b.y != 0.0 {
        out.push_str(&format!(" at {} {}", fmt_num(b.x), fmt_num(b.y)));
    }
    if b.scale != 1.0 {
        out.push_str(&format!(" scale {}", crate::fmt_num_with(b.scale, 4)));
    }
    out.push_str(&format!(" opacity {}", fmt_num(b.opacity)));
    if b.hidden {
        out.push_str(" hidden");
    }
    out
}

/// Returns the script with its `backdrop` line replaced, added under the `artboard` line, or
/// removed (`None`).
pub fn set_backdrop(src: &str, scene: &Scene, backdrop: Option<&crate::Backdrop>) -> String {
    match (&scene.backdrop, backdrop) {
        (Some(old), Some(b)) => splice(src, old.span, &backdrop_text(b)),
        (Some(old), None) => remove_spans(src, vec![whole_lines(src, old.span)]),
        (None, Some(b)) => {
            // Under the artboard line, or at the top when there is none.
            let at = src
                .match_indices("artboard")
                .find(|(i, _)| *i == 0 || src.as_bytes()[i - 1] == b'\n')
                .map_or(0, |(i, _)| src[i..].find('\n').map_or(src.len(), |n| i + n + 1));
            let mut line = backdrop_text(b);
            line.push('\n');
            if at == src.len() && !src.is_empty() && !src.ends_with('\n') {
                line.insert(0, '\n');
            }
            splice(src, Span::new(at, at), &line)
        }
        (None, None) => src.to_string(),
    }
}
