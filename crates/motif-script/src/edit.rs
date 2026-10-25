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
                if matches!(prop, Prop::Width | Prop::ScaleX) { format!("{text} {other_text}") } else { format!("{other_text} {text}") }
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
fn whole_lines(src: &str, span: Span) -> Span {
    let start = src[..span.start].rfind('\n').map_or(0, |i| i + 1);
    let end = src[span.end..].find('\n').map_or(src.len(), |i| span.end + i + 1);
    Span::new(start, end)
}

/// Removes several spans at once; overlapping spans are merged.
fn remove_spans(src: &str, mut spans: Vec<Span>) -> String {
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
fn free_id(scene: &Scene, base: &str) -> String {
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
        ShapeKind::Group | ShapeKind::Path => "none",
    };
    let size = if (size.0 - size.1).abs() < 1e-9 { fmt_num(size.0) } else { format!("{} {}", fmt_num(size.0), fmt_num(size.1)) };
    let line = format!("{} {id} at {} {} size {size} fill {fill}\n", kind.keyword(), fmt_num(center.0), fmt_num(center.1));
    (insert_declaration(src, scene, &line), id)
}

/// Inserts a top-level declaration line before the first `anim`/`override` block.
fn insert_declaration(src: &str, scene: &Scene, line: &str) -> String {
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
    Some(remove_spans(src, spans))
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
/// (`linear`, `ease-in`, `ease-out`, `ease-in-out`, `hold` or `spring`).
pub fn set_key_ease(src: &str, scene: &Scene, anim: &str, node: &str, prop: Prop, time: f64, ease: &str) -> Option<String> {
    let track = find_track(scene, anim, node, prop)?;
    let time = key_time(time);
    let key = track.keys.iter().find(|k| (k.time - time).abs() < SAME_TIME)?;
    Some(match key.ease {
        Some(span) => splice(src, span, ease),
        None => splice(src, Span::new(key.value.end, key.value.end), &format!(" {ease}")),
    })
}
