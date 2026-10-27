//! The palette: colors named by top-level `let` lines, and properties that use them by name.

use crate::edit::{splice, write_override};
use crate::{Scene, Span, fmt_num};
use motif_core::{Color, Prop};

/// The script with the palette color `name` set to `color`, or `None` when there is no such
/// color or it is not written as a plain color.
pub fn set_swatch(src: &str, scene: &Scene, name: &str, color: Color) -> Option<String> {
    let s = scene.palette.iter().find(|s| s.name == name && s.literal)?;
    Some(splice(src, s.span, &color.to_hex()))
}

/// The script with a new palette color: a `let` line after the last one, or after the comments
/// at the top of the file (in the head that every artboard shares).
pub fn add_swatch(src: &str, scene: &Scene, name: &str, color: Color) -> String {
    let line = format!("let {name} = {}\n", color.to_hex());
    match scene.palette.iter().map(|s| s.span.end).max() {
        Some(end) => {
            let at = src[end..].find('\n').map_or(src.len(), |i| end + i + 1);
            if at == src.len() && !src.ends_with('\n') {
                return format!("{src}\n{line}");
            }
            splice(src, Span::new(at, at), &line)
        }
        None => {
            // After the comments that open the file.
            let mut at = 0;
            for l in src.split_inclusive('\n') {
                if !(l.trim().is_empty() || l.trim_start().starts_with("//")) {
                    break;
                }
                at += l.len();
            }
            let at = at.min(crate::head_end(src));
            splice(src, Span::new(at, at), &format!("{line}\n"))
        }
    }
}

/// A palette name not used yet: `color1`, `color2`…
pub fn free_swatch_name(src: &str) -> String {
    (1..).map(|n| format!("color{n}")).find(|n| !src.contains(&format!("let {n} ")) && !src.contains(&format!("let {n}="))).expect("some number is free")
}

/// The script with the color property `prop` (fill or stroke) of `node` written as the palette
/// name `name`, so it changes with the palette.
pub fn set_ref(src: &str, scene: &Scene, node: &str, prop: Prop, name: &str) -> Option<String> {
    if !matches!(prop, Prop::Fill | Prop::StrokeColor) {
        return None;
    }
    let current = scene.doc.node(node)?;
    let site = scene.sites.get(node)?;
    let key = (node.to_string(), prop);
    match scene.sources.get(&key) {
        Some(s) if !s.from_override && !site.generated => Some(splice(src, s.span, name)),
        _ if site.generated || scene.sources.get(&key).is_some_and(|s| s.from_override) => Some(write_override(src, scene, node, prop, name)),
        _ => {
            let addition = match prop {
                Prop::Fill => format!("fill {name}"),
                _ => format!("stroke {name} {}", fmt_num(if current.stroke_width > 0.0 { current.stroke_width } else { 2.0 })),
            };
            Some(splice(src, Span::new(site.insert_at, site.insert_at), &format!(" {addition}")))
        }
    }
}
