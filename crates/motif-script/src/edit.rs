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

pub(crate) fn splice(src: &str, span: Span, text: &str) -> String {
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
    if prop.is_world() {
        return None;
    }
    let current = scene.doc.node(node)?;
    let text = fmt_value(value);
    let key = (node.to_string(), prop);

    if let Some(s) = scene.sources.get(&key) {
        // A palette color named on the node's own line (`fill main`) gives way to the picked color
        // there: the object leaves the palette. Numbers from `let` stay and get an override.
        let named = matches!(prop, Prop::Fill | Prop::StrokeColor)
            && scene.refs.contains_key(&key)
            && !s.from_override
            && scene.sites.get(node).is_some_and(|site| !site.generated);
        if !s.editable && !named {
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

    // A shadow written without a blur gets one after its offset.
    let after = match prop {
        Prop::ShadowBlur => Some(Prop::ShadowY),
        Prop::InsetBlur => Some(Prop::InsetY),
        _ => None,
    };
    if let Some(y) = after.and_then(|after| scene.sources.get(&(node.to_string(), after))) {
        return Some(if y.editable && !y.from_override {
            splice(src, Span::new(y.span.end, y.span.end), &format!(" {text}"))
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
        Prop::Width | Prop::Height if current.kind == ShapeKind::Bone => format!("len {}", pick(Prop::Width)),
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
        Prop::FontSize
        | Prop::LetterSpacing
        | Prop::Reveal
        | Prop::Wave
        | Prop::Phase
        | Prop::Blur
        | Prop::Elevation
        | Prop::Lat
        | Prop::Lon
        | Prop::Alt
        | Prop::Yaw
        | Prop::Pitch
        | Prop::Pull
        | Prop::Fan
        | Prop::Rough => {
            format!("{} {text}", prop.path_name())
        }
        Prop::GravityX | Prop::GravityY | Prop::WindX | Prop::WindY | Prop::Gust => return None,
        // How far goes after the targets: without them there is nothing to flow into.
        Prop::Depth => format!("extrude {text}"),
        // How much the back narrows goes after the depth.
        Prop::Taper => {
            let span = scene.prop_spans.get(&(node.to_string(), "extrude".to_string()))?;
            return Some(splice(src, Span::new(span.end, span.end), &format!(" {text}")));
        }
        Prop::Morph => {
            let span = scene.prop_spans.get(&(node.to_string(), "morph".to_string()))?;
            return Some(splice(src, Span::new(span.end, span.end), &format!(" {text}")));
        }
        Prop::ShadowX | Prop::ShadowY | Prop::ShadowBlur => {
            let color = current.shadow.unwrap_or(motif_core::Shadow::DEFAULT).color.to_hex();
            format!("shadow {} {} {} {color}", pick(Prop::ShadowX), pick(Prop::ShadowY), pick(Prop::ShadowBlur))
        }
        Prop::InsetX | Prop::InsetY | Prop::InsetBlur => {
            let color = current.inset.unwrap_or(motif_core::Shadow::DEFAULT).color.to_hex();
            format!("inset {} {} {} {color}", pick(Prop::InsetX), pick(Prop::InsetY), pick(Prop::InsetBlur))
        }
        Prop::Glow => {
            let g = current.glow.unwrap_or(motif_core::Glow::DEFAULT);
            let spread = if g.spread > 0.0 { format!(" {}", fmt_num(g.spread)) } else { String::new() };
            format!("glow {text}{spread} {}", g.color.to_hex())
        }
        Prop::Noise => {
            let size = current.noise.unwrap_or(motif_core::Noise::DEFAULT).size;
            if size == motif_core::Noise::DEFAULT.size { format!("noise {text}") } else { format!("noise {text} {}", fmt_num(size)) }
        }
    };
    Some(splice(src, Span::new(site.insert_at, site.insert_at), &format!(" {addition}")))
}

pub(crate) fn write_override(src: &str, scene: &Scene, node: &str, prop: Prop, text: &str) -> String {
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
    let base = if prop.is_world() { scene.doc.world.get(prop) } else { scene.doc.node(node)?.get(prop) };
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
        ShapeKind::Group | ShapeKind::Path | ShapeKind::Bone => "none",
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

/// Returns the script with the stroke gradient of `node` set, or removed when `None`.
pub fn set_stroke_gradient(src: &str, scene: &Scene, node: &str, gradient: Option<&motif_core::Gradient>) -> Option<String> {
    set_style(src, scene, node, "stroke-gradient", gradient.map(|g| format!("stroke-{}", gradient_text(g))).as_deref())
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

/// The whole lines declaring `node` and its children, with their animation tracks, overrides and
/// pointer listeners.
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
    for m in &doc.machines {
        let Some(site) = scene.machines.get(&m.name) else { continue };
        spans.extend(m.listeners.iter().zip(&site.listeners).filter(|(l, _)| ids.contains(&l.node.as_str())).map(|(_, s)| whole_lines(src, *s)));
    }
    // Turning parts of `directions` leave its line; with the last of them goes the `turn` word.
    let gone: Vec<Span> = scene.directions_turn.iter().filter(|(id, _)| ids.contains(&id.as_str())).map(|(_, s)| *s).collect();
    if !gone.is_empty() {
        let with_space = |s: Span| Span::new(src[..s.start].trim_end_matches([' ', '\t']).len(), s.end);
        if gone.len() == scene.directions_turn.len()
            && let Some(first) = gone.first()
            && src[..first.start].trim_end().ends_with("turn")
        {
            let word = src[..first.start].trim_end().len() - "turn".len();
            spans.push(with_space(Span::new(word, gone.last().map_or(first.end, |s| s.end))));
        } else {
            spans.extend(gone.into_iter().map(with_space));
        }
    }
    // Weights go with their shape, and strokes with their bone.
    for (shape, w) in &scene.weights {
        if ids.contains(&shape.as_str()) {
            spans.push(whole_lines(src, w.span));
        } else if let Some(skin) = doc.node(shape).and_then(|n| n.skin.as_ref()) {
            spans.extend(skin.paint.iter().zip(&w.strokes).filter(|(st, _)| ids.contains(&st.bone.as_str())).map(|(_, s)| whole_lines(src, *s)));
        }
    }
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

/// Returns the script with an empty, non-looping animation for each of `names` that is missing,
/// and the names it added.
pub fn add_states(src: &str, scene: &Scene, names: &[&str], seconds: f64) -> (String, Vec<String>) {
    let mut out = src.to_string();
    let mut added = Vec::new();
    for name in names.iter().filter(|n| scene.doc.animation(n).is_none()) {
        if !out.is_empty() && !out.ends_with('\n') {
            out.push('\n');
        }
        out.push_str(&format!("\nanim {name} {}s {{\n}}\n", fmt_num(seconds)));
        added.push(name.to_string());
    }
    (out, added)
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

/// A key as written in a track: its time, the value text and the curve text.
#[derive(Clone, Debug, PartialEq)]
pub struct KeyText {
    pub time: f64,
    pub value: String,
    pub ease: Option<String>,
}

/// A key of one track of an animation: `node.prop` at `time`.
#[derive(Clone, Debug, PartialEq)]
pub struct KeyAt {
    pub node: String,
    pub prop: Prop,
    pub time: f64,
}

/// A copied key: where it came from and how it is written.
#[derive(Clone, Debug, PartialEq)]
pub struct KeyCopy {
    pub node: String,
    pub prop: Prop,
    pub key: KeyText,
}

fn read_keys(src: &str, track: &TrackSite) -> Vec<KeyText> {
    track
        .keys
        .iter()
        .map(|k| KeyText { time: k.time, value: src[k.value.start..k.value.end].to_string(), ease: k.ease.map(|e| src[e.start..e.end].to_string()) })
        .collect()
}

fn keys_line(keys: &[KeyText]) -> String {
    keys.iter()
        .map(|k| match &k.ease {
            Some(e) => format!("{}: {} {e}", fmt_num(key_time(k.time)), k.value),
            None => format!("{}: {}", fmt_num(key_time(k.time)), k.value),
        })
        .collect::<Vec<_>>()
        .join(" | ")
}

/// Applies non-overlapping replacements at once.
fn splice_all(src: &str, mut edits: Vec<(Span, String)>) -> String {
    edits.sort_by_key(|(s, _)| std::cmp::Reverse(s.start));
    let mut out = src.to_string();
    for (span, text) in edits {
        out.replace_range(span.start..span.end, &text);
    }
    out
}

/// Returns the script with the keys of whole tracks replaced: each entry is the new key list of
/// `node.prop`, sorted on writing. An empty list removes the track line; a track the animation
/// does not have yet gets a new line at the end of the block. `None` when the animation does not
/// exist or a list has two keys at the same time.
pub fn write_tracks(src: &str, scene: &Scene, anim: &str, tracks: &[(String, Prop, Vec<KeyText>)]) -> Option<String> {
    let site = scene.anims.get(anim)?;
    let mut edits = Vec::new();
    let mut added = String::new();
    for (node, prop, keys) in tracks {
        let mut keys = keys.clone();
        keys.sort_by(|a, b| key_time(a.time).total_cmp(&key_time(b.time)));
        if keys.windows(2).any(|w| (key_time(w[0].time) - key_time(w[1].time)).abs() < SAME_TIME) {
            return None;
        }
        match find_track(scene, anim, node, *prop) {
            Some(track) if keys.is_empty() => edits.push((whole_lines(src, track.span), String::new())),
            Some(track) => {
                let (first, last) = (track.keys.first()?, track.keys.last()?);
                edits.push((Span::new(first.span.start, last.span.end), keys_line(&keys)));
            }
            None if keys.is_empty() => {}
            None => added.push_str(&format!("  {node}.{}  {}\n", prop.path_name(), keys_line(&keys))),
        }
    }
    let out = splice_all(src, edits);
    if added.is_empty() {
        return Some(out);
    }
    // Every edited track line is inside the block, so the closing brace moved by the length change.
    let close = (site.close + out.len()).checked_sub(src.len())?;
    Some(insert_before_close(&out, close, &added))
}

/// The tracks that hold `keys`, read with their keys as written.
fn tracks_of(src: &str, scene: &Scene, anim: &str, keys: &[KeyAt]) -> Option<Vec<(String, Prop, Vec<KeyText>)>> {
    let mut out: Vec<(String, Prop, Vec<KeyText>)> = Vec::new();
    for k in keys {
        if out.iter().any(|(n, p, _)| n == &k.node && *p == k.prop) {
            continue;
        }
        let track = find_track(scene, anim, &k.node, k.prop)?;
        out.push((k.node.clone(), k.prop, read_keys(src, track)));
    }
    Some(out)
}

fn is_picked(keys: &[KeyAt], node: &str, prop: Prop, time: f64) -> bool {
    keys.iter().any(|k| k.node == node && k.prop == prop && (k.time - time).abs() < SAME_TIME)
}

/// Returns the script with `keys` moved to `map(time)` (snapped to 0.01 s and kept at or after 0),
/// for moving and stretching several keys at once. `None` when a key does not exist or two keys
/// of a track would land on the same time.
pub fn retime_keys(src: &str, scene: &Scene, anim: &str, keys: &[KeyAt], map: impl Fn(f64) -> f64) -> Option<String> {
    let mut tracks = tracks_of(src, scene, anim, keys)?;
    for k in keys {
        let (_, _, list) = tracks.iter().find(|(n, p, _)| n == &k.node && *p == k.prop)?;
        list.iter().find(|x| (x.time - k.time).abs() < SAME_TIME)?;
    }
    for (node, prop, list) in &mut tracks {
        for key in list.iter_mut() {
            if is_picked(keys, node, *prop, key.time) {
                key.time = key_time(map(key.time));
            }
        }
    }
    write_tracks(src, scene, anim, &tracks)
}

/// Returns the script without `keys`; tracks left without keys are removed. `None` when one of
/// them does not exist.
pub fn delete_keys(src: &str, scene: &Scene, anim: &str, keys: &[KeyAt]) -> Option<String> {
    let mut tracks = tracks_of(src, scene, anim, keys)?;
    let before: usize = tracks.iter().map(|t| t.2.len()).sum();
    for (node, prop, list) in &mut tracks {
        list.retain(|key| !is_picked(keys, node, *prop, key.time));
    }
    let after: usize = tracks.iter().map(|t| t.2.len()).sum();
    if before - after != keys.len() {
        return None;
    }
    write_tracks(src, scene, anim, &tracks)
}

/// Returns the script with the curve of every key in `keys` set to `ease`.
pub fn set_keys_ease(src: &str, scene: &Scene, anim: &str, keys: &[KeyAt], ease: &str) -> Option<String> {
    let mut edits = Vec::new();
    for k in keys {
        let track = find_track(scene, anim, &k.node, k.prop)?;
        let key = track.keys.iter().find(|x| (x.time - k.time).abs() < SAME_TIME)?;
        edits.push(match key.ease {
            Some(span) => (span, ease.to_string()),
            None => (Span::new(key.value.end, key.value.end), format!(" {ease}")),
        });
    }
    Some(splice_all(src, edits))
}

/// Copies `keys` as written, in time order.
pub fn copy_keys(src: &str, scene: &Scene, anim: &str, keys: &[KeyAt]) -> Vec<KeyCopy> {
    let mut out: Vec<KeyCopy> = keys
        .iter()
        .filter_map(|k| {
            let track = find_track(scene, anim, &k.node, k.prop)?;
            let i = track.keys.iter().position(|x| (x.time - k.time).abs() < SAME_TIME)?;
            Some(KeyCopy { node: k.node.clone(), prop: k.prop, key: read_keys(src, track).swap_remove(i) })
        })
        .collect();
    out.sort_by(|a, b| a.key.time.total_cmp(&b.key.time));
    out
}

/// Returns the script with copied keys pasted so that the earliest lands at `at`, and the times
/// they now have. With `onto`, keys copied from a single object go to that object instead. A key
/// already at a pasted time is replaced. `None` when the animation or an object does not exist.
pub fn paste_keys(src: &str, scene: &Scene, anim: &str, clip: &[KeyCopy], at: f64, onto: Option<&str>) -> Option<(String, Vec<KeyAt>)> {
    let start = clip.iter().map(|c| c.key.time).fold(f64::INFINITY, f64::min);
    if !start.is_finite() {
        return None;
    }
    let mut tracks: Vec<(String, Prop, Vec<KeyText>)> = Vec::new();
    let mut pasted = Vec::new();
    for c in clip {
        let node = onto.unwrap_or(&c.node);
        if node != "world" && scene.doc.node(node).is_none() {
            return None;
        }
        if !tracks.iter().any(|(n, p, _)| n == node && *p == c.prop) {
            let list = find_track(scene, anim, node, c.prop).map(|t| read_keys(src, t)).unwrap_or_default();
            tracks.push((node.to_string(), c.prop, list));
        }
        let (_, _, list) = tracks.iter_mut().find(|(n, p, _)| n == node && *p == c.prop)?;
        let time = key_time(at + c.key.time - start);
        list.retain(|k| (k.time - time).abs() >= SAME_TIME);
        list.push(KeyText { time, ..c.key.clone() });
        pasted.push(KeyAt { node: node.to_string(), prop: c.prop, time });
    }
    Some((write_tracks(src, scene, anim, &tracks)?, pasted))
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
    if let Some(s) = &b.sheet {
        out.push_str(&format!(" sheet {} {}", s.cols, s.rows));
        if s.frames != s.cols * s.rows {
            out.push_str(&format!(" frames {}", s.frames));
        }
        if s.fps != 12.0 {
            out.push_str(&format!(" fps {}", fmt_num(s.fps)));
        }
        if s.frame != 0 {
            out.push_str(&format!(" frame {}", s.frame));
        }
    }
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
        (None, Some(b)) => under_artboard(src, &backdrop_text(b)),
        (None, None) => src.to_string(),
    }
}

/// Returns the script with `line` added under the `artboard` line, or at the top when there is none.
fn under_artboard(src: &str, line: &str) -> String {
    let at = src
        .match_indices("artboard")
        .find(|(i, _)| *i == 0 || src.as_bytes()[i - 1] == b'\n')
        .map_or(0, |(i, _)| src[i..].find('\n').map_or(src.len(), |n| i + n + 1));
    let mut line = format!("{line}\n");
    if at == src.len() && !src.is_empty() && !src.ends_with('\n') {
        line.insert(0, '\n');
    }
    splice(src, Span::new(at, at), &line)
}

/// The script line of the scene's gravity and wind, leaving out values that are at their defaults.
pub fn world_text(w: &motif_core::World) -> String {
    let d = motif_core::World::default();
    let mut out = "world".to_string();
    if w.gravity != d.gravity {
        out.push_str(&format!(" gravity {} {}", fmt_num(w.gravity.x), fmt_num(w.gravity.y)));
    }
    if w.wind != d.wind {
        out.push_str(&format!(" wind {} {}", fmt_num(w.wind.x), fmt_num(w.wind.y)));
    }
    if w.gust != d.gust {
        out.push_str(&format!(" gust {}", fmt_num(w.gust)));
    }
    out
}

/// Returns the script with its `world` line set to `world`: replaced, added under the
/// `artboard` line, or removed when everything is at its default.
pub fn set_world(src: &str, scene: &Scene, world: &motif_core::World) -> String {
    match (scene.world, world.is_default()) {
        (Some(span), false) => splice(src, span, &world_text(world)),
        (Some(span), true) => remove_spans(src, vec![whole_lines(src, span)]),
        (None, false) => under_artboard(src, &world_text(world)),
        (None, true) => src.to_string(),
    }
}

/// The `directions` line for `d`.
pub fn directions_text(d: &motif_core::Directions) -> String {
    let mut out = format!("directions {}", d.count);
    if !d.turn.is_empty() {
        out.push_str(" turn ");
        out.push_str(&d.turn.join(" "));
    }
    if d.mirror {
        out.push_str(" mirror");
    }
    out
}

/// Returns the script with its `directions` line set to `d`: replaced, added under the
/// `artboard` line, or removed with `None`.
pub fn set_directions(src: &str, scene: &Scene, d: Option<&motif_core::Directions>) -> String {
    match (scene.directions, d) {
        (Some(span), Some(d)) => splice(src, span, &directions_text(d)),
        (Some(span), None) => remove_spans(src, vec![whole_lines(src, span)]),
        (None, Some(d)) => under_artboard(src, &directions_text(d)),
        (None, None) => src.to_string(),
    }
}

/// Returns the script with the scene light (`light` on the `artboard` line) set to `angle`
/// degrees; the default 90 removes it. `None` when there is no artboard line.
pub fn set_light(src: &str, angle: f64) -> Option<String> {
    let light = fmt_num(angle);
    edit_artboard_line(src, |words| {
        if let Some(k) = words.iter().position(|w| w == "light") {
            words.drain(k..(k + 2).min(words.len()));
        }
        if angle != 90.0 {
            words.extend(["light".to_string(), light.clone()]);
        }
    })
}

/// Returns the script with the artboard marked for interface (`ui`), game art (`game`) or
/// neither. `None` when the script has no artboard line.
pub fn set_purpose(src: &str, purpose: motif_core::Purpose) -> Option<String> {
    edit_artboard_line(src, |words| {
        words.retain(|w| w != "ui" && w != "game");
        match purpose {
            motif_core::Purpose::Ui => words.push("ui".into()),
            motif_core::Purpose::Game => words.push("game".into()),
            motif_core::Purpose::Any => {}
        }
    })
}

/// Rewrites the words of the `artboard` line before its comment. The name comes first and may
/// hold spaces, so it is kept apart from the words.
pub(crate) fn edit_artboard_line(src: &str, f: impl FnOnce(&mut Vec<String>)) -> Option<String> {
    let start = src.match_indices("artboard").find(|(i, _)| *i == 0 || src.as_bytes()[i - 1] == b'\n')?.0;
    let end = src[start..].find('\n').map_or(src.len(), |n| start + n);
    let line = &src[start..end];
    let code_end = line.find("//").unwrap_or(line.len());
    let (code, comment) = line.split_at(code_end);
    // `artboard "Имя с пробелами"` stays as written; the rest are single words.
    let head_end = match code.find('"') {
        Some(open) => code[open + 1..].find('"').map_or(code.len(), |close| open + close + 2),
        None => "artboard".len(),
    };
    let (head, rest) = code.split_at(head_end);
    let mut words: Vec<String> = rest.split_whitespace().map(str::to_string).collect();
    f(&mut words);
    let mut out = head.trim_end().to_string();
    for w in &words {
        out.push(' ');
        out.push_str(w);
    }
    if !comment.is_empty() {
        out.push(' ');
        out.push_str(comment);
    }
    Some(splice(src, Span::new(start, end), &out))
}

/// Returns the script with the name shown in the layers set to `name` (an empty name removes it).
/// `None` when the node comes from a loop.
pub fn rename_node(src: &str, scene: &Scene, node: &str, name: &str) -> Option<String> {
    let site = scene.sites.get(node).filter(|s| !s.generated)?;
    // Names have no escapes: a quote or a line break would end the string.
    let name: String = name.chars().filter(|c| *c != '"' && *c != '\n' && *c != '\r').collect();
    let name = name.trim();
    Some(match (site.name_span, name.is_empty()) {
        (Some(span), false) => splice(src, span, &format!("\"{name}\"")),
        (Some(span), true) => splice(src, with_space_before(src, span), ""),
        (None, false) => splice(src, Span::new(site.id_span.end, site.id_span.end), &format!(" \"{name}\"")),
        (None, true) => src.to_string(),
    })
}

/// Where a dragged layer lands, next to `target` in the script or inside it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Place {
    /// Written before the target: drawn under it.
    Before,
    /// Written after the target: drawn over it.
    After,
    /// The last child of the target group: drawn over its other children.
    Into,
}

/// Returns the script with the declaration of `node` (with its children) moved next to `target`
/// or into the group `target`. Animation tracks stay where they are, they name the node by id.
/// When the parent changes, `at` is rewritten so the node stays where it was on the artboard.
/// `None` when either node comes from a loop, `node` would land inside itself, or `Into` names
/// something other than a group.
pub fn move_node(src: &str, scene: &Scene, node: &str, target: &str, place: Place) -> Option<String> {
    let doc = &scene.doc;
    let (i, t) = (doc.index_of(node)?, doc.index_of(target)?);
    let site = scene.sites.get(node).filter(|s| !s.generated)?;
    let tsite = scene.sites.get(target).filter(|s| !s.generated)?;
    if i == t || doc.is_inside(t, i) || (place == Place::Into && doc.nodes[t].kind != ShapeKind::Group) {
        return None;
    }
    // Both declarations must have their lines to themselves (a comment after them is fine).
    let alone = |span: Span| {
        let l = whole_lines(src, span);
        let after = src[span.end..l.end].trim();
        src[l.start..span.start].trim().is_empty() && (after.is_empty() || after.starts_with("//"))
    };
    if !alone(site.span) || !alone(tsite.span) {
        return None;
    }
    // The comment lines right above a declaration describe it, so they move with it.
    let with_comments = |start: usize| {
        let mut start = start;
        while start > 0 {
            let prev = src[..start - 1].rfind('\n').map_or(0, |k| k + 1);
            if !src[prev..start].trim_start().starts_with("//") {
                break;
            }
            start = prev;
        }
        start
    };
    let lines = whole_lines(src, site.span);
    let lines = Span::new(with_comments(lines.start), lines.end);
    let new_parent = if place == Place::Into { Some(t) } else { doc.nodes[t].parent };

    // The declaration's lines, with `at` adjusted to the new parent.
    let mut text = src[lines.start..lines.end].to_string();
    if new_parent != doc.nodes[i].parent {
        let n = &doc.nodes[i];
        let world = |p: Option<usize>| p.map_or(Affine::IDENTITY, |p| doc.world_transform(p));
        let at = world(new_parent).inverse() * (world(n.parent) * Point::new(n.x, n.y));
        let (x, y) = (fmt_num((at.x * 100.0).round() / 100.0), fmt_num((at.y * 100.0).round() / 100.0));
        let rel = |s: Span| Span::new(s.start - lines.start, s.end - lines.start);
        let inside = |s: &crate::Source| !s.from_override && s.span.start >= site.span.start && s.span.end <= site.span.end;
        let sx = scene.sources.get(&(node.to_string(), Prop::X));
        let sy = scene.sources.get(&(node.to_string(), Prop::Y));
        match (sx, sy) {
            (Some(a), _) if a.editable && inside(a) && a.paired.is_some() => text = splice(&text, rel(a.span), &format!("{x} {y}")),
            (Some(a), Some(b)) if a.editable && b.editable && inside(a) && inside(b) => {
                // The later literal first, so the earlier span still points at its number.
                let (first, second) = if a.span.start < b.span.start { ((a, &x), (b, &y)) } else { ((b, &y), (a, &x)) };
                text = splice(&text, rel(second.0.span), second.1);
                text = splice(&text, rel(first.0.span), first.1);
            }
            (None, None) => {
                let end = site.insert_at - lines.start;
                text = splice(&text, Span::new(end, end), &format!(" at {x} {y}"));
            }
            // Written by a formula: the formula stays.
            _ => {}
        }
    }

    // Indent it like its new neighbours.
    let indent_of = |at: usize| {
        let start = src[..at].rfind('\n').map_or(0, |k| k + 1);
        src[start..].chars().take_while(|c| *c == ' ' || *c == '\t').collect::<String>()
    };
    let old_indent = indent_of(site.span.start);
    let new_indent = if place == Place::Into { format!("{}  ", indent_of(tsite.span.start)) } else { indent_of(tsite.span.start) };
    let mut block = String::new();
    for line in text.lines() {
        let line = line.strip_prefix(old_indent.as_str()).unwrap_or(line.trim_start());
        if !line.is_empty() {
            block.push_str(&new_indent);
        }
        block.push_str(line);
        block.push('\n');
    }

    let (at, block) = match place {
        Place::Before => (with_comments(whole_lines(src, tsite.span).start), block),
        Place::After => (whole_lines(src, tsite.span).end, block),
        Place::Into => {
            let close = tsite.span.end - 1;
            let line_start = src[..close].rfind('\n').map_or(0, |k| k + 1);
            if src[line_start..close].trim().is_empty() { (line_start, block) } else { (close, format!("\n{block}")) }
        }
    };
    let block = if at == src.len() && !src.ends_with('\n') { format!("\n{block}") } else { block };
    Some(if at <= lines.start {
        format!("{}{block}{}{}", &src[..at], &src[at..lines.start], &src[lines.end..])
    } else if at >= lines.end {
        format!("{}{}{block}{}", &src[..lines.start], &src[lines.end..at], &src[at..])
    } else {
        return None;
    })
}

/// Returns the script with the event `name` of animation `anim` written as `new_name` at
/// `times` (sorted, a time given twice kept once): its line rewritten, a new line at the end of
/// the block, or the line removed when `times` is empty. `None` when the animation does not
/// exist, or `new_name` is not a plain word or already names another event there.
pub fn write_event(src: &str, scene: &Scene, anim: &str, name: &str, new_name: &str, times: &[f64]) -> Option<String> {
    let site = scene.anims.get(anim)?;
    let word =
        !new_name.is_empty() && new_name.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '-') && !new_name.starts_with(|c: char| c.is_ascii_digit());
    if !word || (new_name != name && site.events.iter().any(|e| e.name == new_name)) {
        return None;
    }
    let mut times: Vec<f64> = times.iter().map(|t| key_time(*t)).collect();
    times.sort_by(f64::total_cmp);
    times.dedup_by(|a, b| (*a - *b).abs() < SAME_TIME);
    let line = format!("event {new_name} {}", times.iter().map(|t| fmt_num(*t)).collect::<Vec<_>>().join(" "));
    match site.events.iter().find(|e| e.name == name) {
        Some(e) if times.is_empty() => Some(splice(src, whole_lines(src, e.span), "")),
        Some(e) => Some(splice(src, e.span, &line)),
        None if times.is_empty() => Some(src.to_string()),
        None => Some(insert_before_close(src, site.close, &format!("  {line}\n"))),
    }
}

/// Fails when the position of `node` is written by a formula: moving it to another parent would
/// leave the formula as it is and the node somewhere else.
fn movable(scene: &Scene, node: &str) -> Result<(), String> {
    let fixed = [Prop::X, Prop::Y].iter().any(|p| scene.sources.get(&(node.to_string(), *p)).is_some_and(|s| !s.editable || s.from_override));
    if fixed { Err(format!("место «{node}» задано формулой")) } else { Ok(()) }
}

/// Re-reads the script after one step of an edit that takes several.
fn reparse(src: &str, base: Option<&std::path::Path>) -> Result<Scene, String> {
    crate::parse_in(src, base).map_err(|e| e.first().map_or_else(|| "скрипт не читается".to_string(), |e| e.message.clone()))
}

/// The whole lines of an `anim` block, from its header to its closing brace, and the span of
/// its name in the header.
fn anim_block(src: &str, scene: &Scene, anim: &str) -> Option<(Span, Span)> {
    let site = scene.anims.get(anim)?;
    let start = src[..site.duration.start].rfind('\n').map_or(0, |i| i + 1);
    let header = &src[start..site.duration.start];
    let at = start + header.find(anim)?;
    Some((whole_lines(src, Span::new(start, site.close + 1)), Span::new(at, at + anim.len())))
}

/// Whether `name` can be an animation's name: one word, not taken by another animation.
fn anim_name_ok(scene: &Scene, name: &str) -> Result<(), String> {
    let word = name.chars().next().is_some_and(|c| c.is_alphabetic() || c == '_') && name.chars().all(|c| c.is_alphanumeric() || c == '_');
    if !word {
        return Err(format!("«{name}» — имя анимации пишется одним словом из букв, цифр и _"));
    }
    if scene.doc.animation(name).is_some() {
        return Err(format!("анимация «{name}» уже есть"));
    }
    Ok(())
}

/// A name like `Run2` (or `RunBack2` for `RunBack`) that no animation has yet.
fn free_anim_name(scene: &Scene, base: &str) -> String {
    if scene.doc.animation(base).is_none() {
        return base.to_string();
    }
    (2..).map(|i| format!("{base}{i}")).find(|n| scene.doc.animation(n).is_none()).expect("names are unbounded")
}

/// Returns the script with animation `anim` called `new_name`, the machine states that play it
/// following along.
pub fn rename_anim(src: &str, scene: &Scene, anim: &str, new_name: &str, base: Option<&std::path::Path>) -> Result<String, String> {
    anim_name_ok(scene, new_name)?;
    let (_, name) = anim_block(src, scene, anim).ok_or("нет такой анимации")?;
    // Every edit stays inside its own block, so done from the bottom up each one still finds
    // its place in the original script; no step in between has to parse.
    let mut steps: Vec<(usize, Option<String>)> = vec![(name.start, None)];
    for m in scene.doc.machines.iter().filter(|m| m.states.iter().any(|s| s.anim == anim)) {
        steps.push((scene.machines.get(&m.name).ok_or("машина без места в скрипте")?.span.start, Some(m.name.clone())));
    }
    steps.sort_by_key(|s| std::cmp::Reverse(s.0));
    let mut out = src.to_string();
    for (_, machine) in steps {
        out = match machine {
            None => splice(&out, name, new_name),
            Some(m) => crate::edit_machine(&out, scene, &m, |m| {
                for s in &mut m.states {
                    if s.anim == anim {
                        s.anim = new_name.to_string();
                    }
                }
            })
            .ok_or_else(|| format!("машина «{m}» не обновилась"))?,
        };
    }
    reparse(&out, base)?;
    Ok(out)
}

/// Returns the script with a copy of animation `anim` right after it, and the copy's name.
pub fn duplicate_anim(src: &str, scene: &Scene, anim: &str) -> Option<(String, String)> {
    let (block, name) = anim_block(src, scene, anim)?;
    let copy = free_anim_name(scene, anim);
    let text = format!("{}{copy}{}", &src[block.start..name.start], &src[name.end..block.end]);
    let text = if text.ends_with('\n') { text } else { format!("\n{text}\n") };
    Some((splice(src, Span::new(block.end, block.end), &format!("\n{text}")), copy))
}

/// Returns the script with a copy of animation `anim` that plays it backwards, right after it,
/// and the copy's name: every key at `duration - t`, every curve mirrored so each segment looks
/// the same in reverse, events moved likewise. A `hold` becomes a jump at the start of its
/// segment, written as a key 0.01 s in.
pub fn reverse_anim(src: &str, scene: &Scene, anim: &str) -> Option<(String, String)> {
    let a = scene.doc.animation(anim)?;
    let site = scene.anims.get(anim)?;
    let (block, _) = anim_block(src, scene, anim)?;
    let d = a.duration;
    let copy = free_anim_name(scene, &format!("{anim}Back"));
    let mut body = String::new();
    for ts in &site.tracks {
        let track = a.tracks.iter().find(|t| t.node == ts.node && t.prop == ts.prop)?;
        // Keys in time order: the text of the value and the curve to the next key.
        let mut keys: Vec<(f64, String, motif_core::Ease)> = track
            .keys
            .iter()
            .filter_map(|k| {
                let site = ts.keys.iter().find(|s| (s.time - k.time).abs() < 1e-9)?;
                Some((k.time.min(d), src[site.value.start..site.value.end].to_string(), k.ease))
            })
            .collect();
        keys.sort_by(|x, y| x.0.total_cmp(&y.0));
        let mut out: Vec<KeyText> = Vec::new();
        let curve = |e: motif_core::Ease| (e != motif_core::Ease::EaseInOut).then(|| e.script());
        for i in (0..keys.len()).rev() {
            let (t, value, _) = &keys[i];
            let Some(prev) = i.checked_sub(1).map(|p| &keys[p]) else {
                out.push(KeyText { time: d - t, value: value.clone(), ease: None });
                break;
            };
            if prev.2 == motif_core::Ease::Hold {
                // The original holds `prev` and jumps at `t`: backwards it jumps right away.
                out.push(KeyText { time: d - t, value: value.clone(), ease: Some("linear".into()) });
                if t - prev.0 > 0.015 {
                    out.push(KeyText { time: d - t + 0.01, value: prev.1.clone(), ease: Some("hold".into()) });
                }
            } else {
                out.push(KeyText { time: d - t, value: value.clone(), ease: curve(prev.2.reversed()) });
            }
        }
        body.push_str(&format!("  {}.{}  {}\n", ts.node, ts.prop.path_name(), keys_line(&out)));
    }
    for e in &site.events {
        let times: Vec<String> = e.times.iter().rev().map(|(t, _)| fmt_num(key_time(d - t))).collect();
        body.push_str(&format!("  event {} {}\n", e.name, times.join(" ")));
    }
    let header = format!("anim {copy} {}s{} {{\n", fmt_num(d), if a.looping { " loop" } else { "" });
    let lead = if src[..block.end].ends_with('\n') { "\n" } else { "\n\n" };
    Some((splice(src, Span::new(block.end, block.end), &format!("{lead}{header}{body}}}\n")), copy))
}

/// Returns the script without animation `anim`. Refused while a machine state plays it.
pub fn delete_anim(src: &str, scene: &Scene, anim: &str) -> Result<String, String> {
    let users: Vec<String> =
        scene.doc.machines.iter().flat_map(|m| m.states.iter().filter(|s| s.anim == anim).map(move |s| format!("{}.{}", m.name, s.name))).collect();
    if !users.is_empty() {
        return Err(format!("её играют состояния {}: сначала дайте им другую анимацию", users.join(", ")));
    }
    let (block, _) = anim_block(src, scene, anim).ok_or("нет такой анимации")?;
    // The blank line that set it apart goes with it.
    let end = if src[block.end..].starts_with('\n') { block.end + 1 } else { block.end };
    Ok(splice(src, Span::new(block.start, end), ""))
}

/// The edits that add `d` to every key of the `x` and `y` tracks of `nodes`, in every animation:
/// what keeps their motion in place when their parent moves by `-d`. Fails when a key is
/// written as a formula.
fn shift_position_keys(src: &str, scene: &Scene, nodes: &[&str], d: (f64, f64)) -> Result<Vec<(Span, String)>, String> {
    let mut edits = Vec::new();
    for anim in scene.anims.values() {
        for track in anim.tracks.iter().filter(|t| nodes.contains(&t.node.as_str())) {
            let by = match track.prop {
                Prop::X => d.0,
                Prop::Y => d.1,
                _ => continue,
            };
            if by.abs() < 1e-9 {
                continue;
            }
            for k in &track.keys {
                let text = src[k.value.start..k.value.end].trim();
                let v: f64 = text.parse().map_err(|_| format!("ключ {}.{} = {text} — формула, его не сдвинуть", track.node, track.prop.path_name()))?;
                edits.push((k.value, fmt_num(v + by)));
            }
        }
    }
    Ok(edits)
}

/// Returns the script with `ids` wrapped in a new group, and the group's id. The group sits
/// where the topmost of them was, its origin in the middle of their box, and everything stays
/// where it was on the artboard, animations included. All of them must be in the same group
/// (or all at the top level); one inside another is taken with it.
pub fn group_nodes(src: &str, scene: &Scene, ids: &[String], base: Option<&std::path::Path>) -> Result<(String, String), String> {
    let doc = &scene.doc;
    let mut picked: Vec<usize> = ids.iter().filter_map(|id| doc.index_of(id)).collect();
    picked.sort_unstable();
    picked.dedup();
    let outer: Vec<usize> = picked.iter().copied().filter(|&i| !picked.iter().any(|&o| o != i && doc.is_inside(i, o))).collect();
    let &last = outer.last().ok_or("нечего группировать")?;
    let parent = doc.nodes[last].parent;
    if outer.iter().any(|&i| doc.nodes[i].parent != parent) {
        return Err("объекты лежат в разных группах: сначала перенесите их в одну".into());
    }
    if outer.iter().any(|&i| scene.sites.get(&doc.nodes[i].id).is_none_or(|s| s.generated)) {
        return Err("объекты из цикла группируются в самом цикле".into());
    }
    let world = doc.world_bounds(last).ok_or("у объектов нет размера")?;
    let world = outer.iter().filter_map(|&i| doc.world_bounds(i)).fold(world, |a, b| a.union(b));
    let to_parent = parent.map_or(Affine::IDENTITY, |p| doc.world_transform(p).inverse());
    let c = to_parent * world.center();
    let c = ((c.x * 10.0).round() / 10.0, (c.y * 10.0).round() / 10.0);

    // Keys first: their spans are still those of this script.
    let names: Vec<&str> = outer.iter().map(|&i| doc.nodes[i].id.as_str()).collect();
    names.iter().try_for_each(|id| movable(scene, id))?;
    let src = splice_all(src, shift_position_keys(src, scene, &names, (-c.0, -c.1))?);
    let scene = reparse(&src, base)?;
    let group = free_id(&scene, "group");
    let site = scene.sites.get(&doc.nodes[last].id).ok_or("объект пропал")?;
    let line = whole_lines(&src, site.span);
    let indent: String = src[line.start..].chars().take_while(|c| *c == ' ' || *c == '\t').collect();
    let decl = format!("{indent}group {group} at {} {} {{\n{indent}}}\n", fmt_num(c.0), fmt_num(c.1));
    let at = line.end;
    let mut src = if at == src.len() && !src.ends_with('\n') { format!("{src}\n{decl}") } else { format!("{}{decl}{}", &src[..at], &src[at..]) };
    for id in names {
        let scene = reparse(&src, base)?;
        src =
            move_node(&src, &scene, id, &group, Place::Into).ok_or_else(|| format!("«{id}» не переносится в группу: его строка делится с другим объектом"))?;
    }
    reparse(&src, base)?;
    Ok((src, group))
}

/// Returns the script with group `id` taken apart: its children go where it was, in the same
/// order and in the same place on the artboard, animations included, and the group is removed.
/// Also returns the children's ids. Only a group that does nothing but move its children can
/// be taken apart: no rotation, scale, skew, opacity, effects or animation of its own.
pub fn ungroup(src: &str, scene: &Scene, id: &str, base: Option<&std::path::Path>) -> Result<(String, Vec<String>), String> {
    let doc = &scene.doc;
    let g = doc.index_of(id).ok_or("нет такого объекта")?;
    let n = &doc.nodes[g];
    if n.kind != ShapeKind::Group {
        return Err("это не группа".into());
    }
    if scene.sites.get(id).is_none_or(|s| s.generated) {
        return Err("группа из цикла разбирается в самом цикле".into());
    }
    let plain = n.rotation.abs() < 1e-9
        && (n.scale_x - 1.0).abs() < 1e-9
        && (n.scale_y - 1.0).abs() < 1e-9
        && n.skew_x.abs() < 1e-9
        && n.skew_y.abs() < 1e-9
        && (n.opacity - 1.0).abs() < 1e-9
        && n.blur.abs() < 1e-9
        && n.shadow.is_none()
        && n.elevation.abs() < 1e-9
        && n.stack.is_none();
    if !plain {
        return Err("у группы свой поворот, масштаб, прозрачность, эффекты или раскладка: они пропали бы".into());
    }
    if scene.anims.values().any(|a| a.tracks.iter().any(|t| t.node == id)) {
        return Err("у группы своя анимация: она пропала бы".into());
    }
    let children: Vec<String> = (0..doc.nodes.len()).filter(|&k| doc.nodes[k].parent == Some(g)).map(|k| doc.nodes[k].id.clone()).collect();
    if children.is_empty() {
        return Ok((delete_node(src, scene, id).ok_or("группа не удаляется")?, children));
    }
    let names: Vec<&str> = children.iter().map(String::as_str).collect();
    names.iter().try_for_each(|id| movable(scene, id))?;
    let mut src = splice_all(src, shift_position_keys(src, scene, &names, (n.x, n.y))?);
    // The last child first: each lands right after the group, so the order stays.
    for child in children.iter().rev() {
        let scene = reparse(&src, base)?;
        src = move_node(&src, &scene, child, id, Place::After)
            .ok_or_else(|| format!("«{child}» не выносится из группы: его строка делится с другим объектом"))?;
    }
    let scene = reparse(&src, base)?;
    let src = delete_node(&src, &scene, id).ok_or("группа не удаляется")?;
    reparse(&src, base)?;
    Ok((src, children))
}

/// Returns the script with `id` drawn one step over (`up`) or under its neighbour in the same
/// group: its declaration swaps places with that neighbour's. `None` at the top or bottom, or
/// when either comes from a loop.
pub fn restack(src: &str, scene: &Scene, id: &str, up: bool) -> Option<String> {
    let doc = &scene.doc;
    let i = doc.index_of(id)?;
    let parent = doc.nodes[i].parent;
    let sibling = |k: &usize| doc.nodes[*k].parent == parent;
    let other = if up { (i + 1..doc.nodes.len()).find(sibling) } else { (0..i).rev().find(sibling) }?;
    move_node(src, scene, id, &doc.nodes[other].id, if up { Place::After } else { Place::Before })
}
