//! Writes visual edits back into the script text, touching as little of it as possible.

use crate::{Scene, Span, fmt_num};
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
        Some(close) => {
            let line_start = src[..close].rfind('\n').map_or(0, |i| i + 1);
            // Keep the closing brace on its own line even if it shares a line with an entry.
            let (at, line) = if src[line_start..close].trim().is_empty() { (line_start, line) } else { (close, format!("\n{line}")) };
            splice(src, Span::new(at, at), &line)
        }
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
