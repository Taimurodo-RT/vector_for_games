//! Writes visual edits back into the script text, touching as little of it as possible.

use crate::{Scene, Span, TrackSite, fmt_num};
use motif_core::{Color, Prop, Value};

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
