//! Edits for symbols: placing a copy, changing a copy's parameter, turning a group into a symbol.

use crate::edit::{fmt_value, free_id, insert_declaration, splice};
use crate::{Scene, Span, fmt_num};
use motif_core::{ShapeKind, Value};

/// Returns the script with a new copy of `symbol` centred at `center`, and the copy's id.
pub fn add_use(src: &str, scene: &Scene, symbol: &str, center: (f64, f64)) -> (String, String) {
    let base: String = symbol.chars().flat_map(char::to_lowercase).filter(|c| c.is_alphanumeric() || *c == '_').collect();
    let id = free_id(scene, if base.is_empty() { "copy" } else { &base });
    let line = format!("use {symbol} {id} at {} {}\n", fmt_num(center.0), fmt_num(center.1));
    (insert_declaration(src, scene, &line), id)
}

/// Returns the script with parameter `name` of copy `id` set to `value`: rewritten where it is
/// written on the `use` line, or added to the line. `None` when `id` is not a copy, has no such
/// parameter, or its line is made by a loop (and so shared by several copies).
pub fn set_param(src: &str, scene: &Scene, id: &str, name: &str, value: Value) -> Option<String> {
    let inst = scene.instances.get(id)?;
    let param = inst.params.iter().find(|p| p.name == name)?;
    let site = scene.sites.get(id)?;
    if site.generated {
        return None;
    }
    let text = fmt_value(value);
    Some(match param.span {
        Some(span) => splice(src, span, &text),
        None => splice(src, Span::new(site.insert_at, site.insert_at), &format!(" {name}={text}")),
    })
}

/// A symbol name that is not taken yet, made from `id`: `ball` → `Ball`.
pub fn free_symbol_name(scene: &Scene, id: &str) -> String {
    let word: String = id.chars().filter(|c| c.is_alphanumeric() || *c == '_').collect();
    let mut chars = word.chars();
    let base = match chars.next() {
        Some(c) if c.is_alphabetic() => c.to_uppercase().chain(chars).collect(),
        _ => "Symbol".to_string(),
    };
    let taken = |n: &str| scene.symbols.iter().any(|s| s.name == n);
    if !taken(&base) {
        return base;
    }
    (2..).map(|k| format!("{base}{k}")).find(|n| !taken(n)).expect("names are unbounded")
}

/// Turns group `id` into a symbol named `name` and puts one copy of it in the group's place, with
/// the group's own properties. The objects of the group get ids `id-object`, so animation tracks
/// and overrides of them are renamed to match. `None` when `id` is not a group written by hand,
/// is already a copy, or `name` is taken.
pub fn make_symbol(src: &str, scene: &Scene, id: &str, name: &str) -> Option<String> {
    let doc = &scene.doc;
    let index = doc.index_of(id)?;
    let site = scene.sites.get(id)?;
    if doc.nodes[index].kind != ShapeKind::Group || site.generated || scene.instances.contains_key(id) || scene.symbols.iter().any(|s| s.name == name) {
        return None;
    }
    // `group id "name" props {` … `}`: the header without its brace, and the objects inside.
    let header = src[site.span.start..site.insert_at].strip_prefix("group")?;
    let body_start = src[site.insert_at..site.span.end].find('\n')? + site.insert_at + 1;
    let close = site.span.end.checked_sub(1).filter(|&c| src.as_bytes().get(c) == Some(&b'}'))?;
    let body_end = src[..close].rfind('\n').map_or(body_start, |n| n + 1).max(body_start);
    let body = &src[body_start..body_end];

    // The symbol goes on the line of the group's top-level ancestor, so it stays at the top level.
    let mut top = index;
    while let Some(p) = doc.nodes[top].parent {
        top = p;
    }
    let top_site = scene.sites.get(&doc.nodes[top].id)?;
    let line_start = src[..top_site.span.start].rfind('\n').map_or(0, |n| n + 1);
    let mut edits: Vec<(Span, String)> =
        vec![(Span::new(line_start, line_start), format!("symbol {name} {{\n{body}}}\n\n")), (site.span, format!("use {name}{}", header.trim_end()))];
    // Objects inside the group, renamed `id-object`.
    let inside = |i: usize| {
        let mut cur = doc.nodes[i].parent;
        while let Some(p) = cur {
            if p == index {
                return true;
            }
            cur = doc.nodes[p].parent;
        }
        false
    };
    let renamed: Vec<&str> = (0..doc.nodes.len()).filter(|&i| inside(i)).map(|i| doc.nodes[i].id.as_str()).collect();
    let mut rename = |start: usize, node: &str| {
        if renamed.contains(&node) && src[start..].starts_with(node) {
            edits.push((Span::new(start, start + node.len()), format!("{id}-{node}")));
        }
    };
    for anim in scene.anims.values() {
        for t in &anim.tracks {
            rename(t.span.start, &t.node);
        }
    }
    for (node, span) in &scene.override_entries {
        rename(span.start, node);
    }
    // Apply from the end, a replacement before an insertion at the same place.
    edits.sort_by(|a, b| b.0.start.cmp(&a.0.start).then(b.0.end.cmp(&a.0.end)));
    let mut out = src.to_string();
    for (span, text) in edits {
        out = splice(&out, span, &text);
    }
    Some(out)
}
