//! The palette: colors named by top-level `let` lines, and properties that use them by name.

use crate::boards::{BoardView, board_views};
use crate::edit::{splice, splice_all, write_override};
use crate::{Scene, Span, fmt_num};
use motif_core::{Color, Gradient, Prop};
use std::collections::HashSet;
use std::path::Path;

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
            let at = top_of_file(src);
            splice(src, Span::new(at, at), &format!("{line}\n"))
        }
    }
}

/// Where the first declaration of a file goes: after the format version and the comments that
/// open the file, in the head that every artboard shares.
pub(crate) fn top_of_file(src: &str) -> usize {
    let mut at = if crate::format::declared(src).is_some() { src.find('\n').map_or(src.len(), |i| i + 1) } else { 0 };
    for l in src[at..].split_inclusive('\n') {
        if !(l.trim().is_empty() || l.trim_start().starts_with("//")) {
            break;
        }
        at += l.len();
    }
    at.min(crate::head_end(src))
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

/// A color as it is written in a script: by its value or by the name of a palette color.
#[derive(Clone, Debug, PartialEq)]
pub enum ColorWord {
    Hex(Color),
    Name(String),
}

/// How often a color is used in a file.
#[derive(Clone, Debug, PartialEq)]
pub struct ColorUse {
    pub color: Color,
    /// The palette color, when the places are written by its name.
    pub name: Option<String>,
    /// In how many places of the file it is written. A line of a loop or a symbol is one place.
    pub places: usize,
}

/// What a replacement in the whole file gave.
#[derive(Clone, Debug, PartialEq)]
pub struct Replaced {
    pub file: String,
    pub places: usize,
    /// Places left as they were: they are written above the `let` of the new name.
    pub above: usize,
}

/// The palette colors every board of a file sees, as places of the file.
fn shared_swatches(views: &[BoardView]) -> Vec<(String, Color, Span)> {
    let Some(first) = views.first() else { return Vec::new() };
    let all = |name: &str, at: Span| views.iter().all(|v| v.scene.palette.iter().any(|s| s.name == name && v.in_file(s.name_span) == at));
    first.scene.palette.iter().map(|s| (s.name.clone(), s.color, first.in_file(s.name_span))).filter(|(name, _, at)| all(name, *at)).collect()
}

/// Where the line that holds `at` ends in `file`, its line break included.
fn line_end(file: &str, at: usize) -> usize {
    file[at..].find('\n').map_or(file.len(), |i| at + i + 1)
}

/// The `let` of the palette color `name` as board `v` sees it: where its name is in the file.
fn swatch_at(v: &BoardView, name: &str) -> Option<Span> {
    v.scene.palette.iter().find(|s| s.name == name).map(|s| v.in_file(s.name_span))
}

/// Every place of the file where a color is used, each once, with the board that reads it. The
/// values of `let` lines and of symbol parameters are where colors are made: they are left out.
fn color_places(views: &[BoardView]) -> Vec<(Span, &crate::ColorSite, &BoardView)> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for v in views {
        for site in v.scene.colors.iter().filter(|s| !s.decl) {
            let span = v.in_file(site.span);
            if seen.insert(span.start) {
                out.push((span, site, v));
            }
        }
    }
    out.sort_by_key(|(span, ..)| span.start);
    out
}

/// Counts the places of colors: the colors of `palette` by name, in its order, then the colors
/// written by value, the most used first. Each place says whether its name is this palette's.
fn tally<'a>(palette: &[crate::Swatch], places: impl Iterator<Item = (&'a crate::ColorSite, bool)>) -> Vec<ColorUse> {
    let mut out: Vec<ColorUse> = palette.iter().map(|s| ColorUse { color: s.color, name: Some(s.name.clone()), places: 0 }).collect();
    let named = out.len();
    for (site, ours) in places {
        match &site.name {
            Some(name) => {
                if let Some(u) = out[..named].iter_mut().find(|u| ours && u.name.as_deref() == Some(name.as_str())) {
                    u.places += 1;
                }
            }
            None => match out[named..].iter_mut().find(|u| u.color == site.color) {
                Some(u) => u.places += 1,
                None => out.push(ColorUse { color: site.color, name: None, places: 1 }),
            },
        }
    }
    out[named..].sort_by(|a, b| b.places.cmp(&a.places).then(a.color.to_hex().cmp(&b.color.to_hex())));
    out
}

/// The colors of a file and how often each is used: the palette colors of board `board` by
/// name, then the colors written by value, the most used first. `None` when a board has errors.
pub fn color_uses(file: &str, base: Option<&Path>, board: usize) -> Option<Vec<ColorUse>> {
    let views = board_views(file, base)?;
    let active = views.get(board)?;
    let ours = |site: &crate::ColorSite, v: &BoardView| site.name.as_deref().is_some_and(|name| swatch_at(v, name) == swatch_at(active, name));
    Some(tally(&active.scene.palette, color_places(&views).into_iter().map(|(_, site, v)| (site, ours(site, v)))))
}

/// [`color_uses`] for a file of one board, from its scene as already read.
pub fn scene_color_uses(scene: &Scene) -> Vec<ColorUse> {
    tally(&scene.palette, scene.colors.iter().filter(|s| !s.decl).map(|s| (s, true)))
}

/// Returns the file with the color `from` written as `to` in every place it is used, on every
/// board: a value by another value or by a palette name, a palette name by another name or by a
/// value. `board` is the board whose palette the names are of. The `let` lines themselves stay:
/// a palette color is changed in the palette. `None` when a board has errors, a name is not in
/// the palette, the new name is not seen on a board that uses the old color, or nothing uses
/// the color.
pub fn replace_color(file: &str, base: Option<&Path>, board: usize, from: &ColorWord, to: &ColorWord) -> Option<Replaced> {
    if from == to {
        return None;
    }
    let views = board_views(file, base)?;
    let active = views.get(board)?;
    let from_at = match from {
        ColorWord::Name(name) => Some(swatch_at(active, name)?),
        ColorWord::Hex(_) => None,
    };
    // A name can be written only below its `let`.
    let (text, to_at) = match to {
        ColorWord::Name(name) => (name.clone(), Some(swatch_at(active, name)?)),
        ColorWord::Hex(c) => (c.to_hex(), None),
    };
    let mut edits = Vec::new();
    let mut above = 0;
    for (span, site, v) in color_places(&views) {
        let wanted = match from {
            ColorWord::Hex(c) => site.name.is_none() && site.color == *c,
            ColorWord::Name(name) => site.name.as_deref() == Some(name.as_str()) && swatch_at(v, name) == from_at,
        };
        if !wanted {
            continue;
        }
        if let (ColorWord::Name(name), Some(at)) = (to, to_at) {
            if swatch_at(v, name) != Some(at) {
                return None;
            }
            if span.start < line_end(file, at.start) {
                above += 1;
                continue;
            }
        }
        edits.push((span, text.clone()));
    }
    if edits.is_empty() {
        return None;
    }
    let places = edits.len();
    let out = splice_all(file, edits);
    crate::parse_boards(&out, base).ok()?;
    Some(Replaced { file: out, places, above })
}

/// Whether `name` can name a new palette color or gradient in `scene`: a word that is not a
/// property, a number or a color name already, and not a gradient of the palette.
pub fn swatch_name_ok(scene: &Scene, name: &str) -> bool {
    let mut chars = name.chars();
    chars.next().is_some_and(|c| c.is_alphabetic() || c == '_')
        && chars.all(|c| c.is_alphanumeric() || c == '_')
        && crate::prop_arity(name).is_none()
        && !matches!(name, "none" | "linear" | "radial" | "let" | "for" | "in")
        && !scene.palette.iter().any(|s| s.name == name)
        && !scene.gradients.iter().any(|s| s.name == name)
}

/// Returns the file with the palette color or gradient `old` of board `board` called `new`: on
/// its `let` line and in every place that names it, on every board that sees it. `None` when
/// there is no such name, the new one cannot be used on one of those boards, or a board has
/// errors.
pub fn rename_swatch(file: &str, base: Option<&Path>, board: usize, old: &str, new: &str) -> Option<String> {
    let views = board_views(file, base)?;
    let active = views.get(board)?;
    let decl =
        |v: &BoardView| -> Option<Span> { swatch_at(v, old).or_else(|| v.scene.gradients.iter().find(|g| g.name == old).map(|g| v.in_file(g.name_span))) };
    let at = decl(active)?;
    let mut edits = vec![(at, new.to_string())];
    let mut seen = HashSet::from([at.start]);
    for v in views.iter().filter(|v| decl(v) == Some(at)) {
        if !swatch_name_ok(&v.scene, new) {
            return None;
        }
        // The value of another `let` may name the color too (`let paper = ink`): it follows.
        let colors = v.scene.colors.iter().filter(|s| s.name.as_deref() == Some(old)).map(|s| s.span);
        let gradients = v.scene.gradient_sites.iter().filter(|s| s.name.as_deref() == Some(old)).map(|s| s.span);
        for span in colors.chain(gradients).map(|s| v.in_file(s)) {
            if seen.insert(span.start) {
                edits.push((span, new.to_string()));
            }
        }
    }
    let out = splice_all(file, edits);
    crate::parse_boards(&out, base).ok()?;
    Some(out)
}

/// Returns the file with the color `color`, written by value so far, made a color of the shared
/// palette under `name`: its `let` joins the palette, and every place below it is written by
/// the name. `None` when a board has errors, the name cannot be used, or no place can take it.
pub fn name_color(file: &str, base: Option<&Path>, name: &str, color: Color) -> Option<Replaced> {
    let views = board_views(file, base)?;
    if !views.iter().all(|v| swatch_name_ok(&v.scene, name)) {
        return None;
    }
    let shared = shared_swatches(&views);
    let at = palette_end(file, shared.iter().map(|(_, _, span)| span.end).max());
    let places: Vec<Span> = color_places(&views).into_iter().filter(|(_, site, _)| site.name.is_none() && site.color == color).map(|(span, ..)| span).collect();
    let mut edits: Vec<(Span, String)> = places.iter().filter(|span| span.start >= at).map(|span| (*span, name.to_string())).collect();
    if edits.is_empty() {
        return None;
    }
    let (written, above) = (edits.len(), places.len() - edits.len());
    let line = format!("let {name} = {}\n", color.to_hex());
    edits.push((Span::new(at, at), if shared.is_empty() { format!("{line}\n") } else { line }));
    let out = splice_all(file, edits);
    crate::parse_boards(&out, base).ok()?;
    Some(Replaced { file: out, places: written, above })
}

/// What [`collect_palette`] did.
#[derive(Clone, Debug, PartialEq)]
pub struct Collected {
    pub file: String,
    /// The new names of the palette, colors and then gradients.
    pub names: Vec<String>,
    /// How many places are written by a name now.
    pub places: usize,
}

/// A palette name not used in `file` yet: `color1`, `color2`…; `taken` are names given in the
/// same edit.
fn free_name(file: &str, word: &str, taken: &[String]) -> String {
    (1..)
        .map(|n| format!("{word}{n}"))
        .find(|n| !taken.contains(n) && !file.contains(&format!("let {n} ")) && !file.contains(&format!("let {n}=")))
        .expect("some number is free")
}

/// Where new `let` lines of the shared palette go in a file: after `last`, the end of the last
/// such line, else at the top, in the head every board shares.
fn palette_end(file: &str, last: Option<usize>) -> usize {
    match last {
        Some(end) => line_end(file, end),
        None => top_of_file(file),
    }
}

/// Returns the file with its repeated colors moved into the palette, so that the scene follows
/// names: a color written by value in two places or more becomes a new palette color, and a
/// value equal to a color of the palette is written by that color's name. Gradients written
/// alike in two places or more become gradients of the palette the same way. Nothing changes
/// its look. Places above the palette keep their values. `None` when a board has errors or
/// there is nothing to collect.
pub fn collect_palette(file: &str, base: Option<&Path>) -> Option<Collected> {
    let (with_colors, mut names, mut places) = match collect_colors(file, base)? {
        Some((out, names, places)) => (out, names, places),
        None => (file.to_string(), Vec::new(), 0),
    };
    let out = match collect_gradients(&with_colors, base)? {
        Some((out, more, n)) => {
            names.extend(more);
            places += n;
            out
        }
        None => with_colors,
    };
    if places == 0 {
        return None;
    }
    crate::parse_boards(&out, base).ok()?;
    Some(Collected { file: out, names, places })
}

type Gathered = Option<(String, Vec<String>, usize)>;

fn collect_colors(file: &str, base: Option<&Path>) -> Option<Gathered> {
    let views = board_views(file, base)?;
    let shared = shared_swatches(&views);
    let at = palette_end(file, shared.iter().map(|(_, _, span)| span.end).max());
    let sites = color_places(&views);
    let literal: Vec<(Span, Color)> = sites.iter().filter(|(_, site, _)| site.name.is_none()).map(|(span, site, _)| (*span, site.color)).collect();
    let mut names: Vec<String> = Vec::new();
    let mut lines = String::new();
    let mut edits: Vec<(Span, String)> = Vec::new();
    let mut done: Vec<Color> = Vec::new();
    for &(_, color) in &literal {
        if done.contains(&color) {
            continue;
        }
        done.push(color);
        let (name, below) = match shared.iter().find(|(_, c, _)| *c == color) {
            // A name is written only below its own line.
            Some((name, _, span)) => (name.clone(), line_end(file, span.start)),
            None => {
                if literal.iter().filter(|(span, c)| *c == color && span.start >= at).count() < 2 {
                    continue;
                }
                let name = free_name(file, "color", &names);
                lines.push_str(&format!("let {name} = {}\n", color.to_hex()));
                names.push(name.clone());
                (name, at)
            }
        };
        edits.extend(literal.iter().filter(|(span, c)| *c == color && span.start >= below).map(|(span, _)| (*span, name.clone())));
    }
    if edits.is_empty() {
        return Some(None);
    }
    let places = edits.len();
    if !lines.is_empty() {
        // A palette that starts a file is set apart from what follows.
        if shared.is_empty() {
            lines.push('\n');
        }
        edits.push((Span::new(at, at), lines));
    }
    Some(Some((splice_all(file, edits), names, places)))
}

fn collect_gradients(file: &str, base: Option<&Path>) -> Option<Gathered> {
    let views = board_views(file, base)?;
    let first = views.first()?;
    // The gradients of the palette that every board sees, by the way they are written.
    let shared: Vec<(String, String, Span)> = first
        .scene
        .gradients
        .iter()
        .map(|g| (g.name.clone(), written(&first.text[g.span.start..g.span.end]), first.in_file(g.name_span)))
        .filter(|(name, _, at)| views.iter().all(|v| v.scene.gradients.iter().any(|g| g.name == *name && v.in_file(g.name_span) == *at)))
        .collect();
    let colors = shared_swatches(&views);
    let last = shared.iter().map(|(_, _, span)| span.end).chain(colors.iter().map(|(_, _, span)| span.end)).max();
    let at = palette_end(file, last);
    let mut seen = HashSet::new();
    let mut sites: Vec<(Span, String)> = Vec::new();
    for v in &views {
        for site in v.scene.gradient_sites.iter().filter(|s| s.name.is_none()) {
            let span = v.in_file(site.span);
            if seen.insert(span.start) {
                sites.push((span, written(&file[span.start..span.end])));
            }
        }
    }
    sites.sort_by_key(|(span, _)| span.start);
    let mut names: Vec<String> = Vec::new();
    let mut lines = String::new();
    let mut edits: Vec<(Span, String)> = Vec::new();
    let mut done: Vec<&str> = Vec::new();
    for (_, text) in &sites {
        if done.contains(&text.as_str()) {
            continue;
        }
        done.push(text);
        let (name, below) = match shared.iter().find(|(_, t, _)| t == text) {
            Some((name, _, span)) => (name.clone(), line_end(file, span.start)),
            None => {
                if sites.iter().filter(|(span, t)| t == text && span.start >= at).count() < 2 {
                    continue;
                }
                let name = free_name(file, "grad", &names);
                lines.push_str(&format!("let {name} = {text}\n"));
                names.push(name.clone());
                (name, at)
            }
        };
        edits.extend(sites.iter().filter(|(span, t)| t == text && span.start >= below).map(|(span, _)| (*span, name.clone())));
    }
    if edits.is_empty() {
        return Some(None);
    }
    let places = edits.len();
    if !lines.is_empty() {
        if last.is_none() {
            lines.push('\n');
        }
        edits.push((Span::new(at, at), lines));
    }
    Some(Some((splice_all(file, edits), names, places)))
}

/// A gradient as written, with single spaces: two places written alike mean the same.
fn written(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The text of a gradient after `gradient` or `let name =`: `linear 90 #a #b`.
pub(crate) fn gradient_text(g: &Gradient, stops: &[Option<String>]) -> String {
    let colors: Vec<String> = g.colors.iter().enumerate().map(|(i, c)| stops.get(i).cloned().flatten().unwrap_or_else(|| c.to_hex())).collect();
    if g.radial { format!("radial {}", colors.join(" ")) } else { format!("linear {} {}", fmt_num(g.angle), colors.join(" ")) }
}

/// Returns the script with the palette gradient `name` set to `gradient`. A color of it that
/// was written by a palette name keeps the name while its value is the same.
pub fn set_gradient_swatch(src: &str, scene: &Scene, name: &str, gradient: &Gradient) -> Option<String> {
    let s = scene.gradients.iter().find(|s| s.name == name)?;
    // A name stays where the color it stands for is still the one meant.
    let stops: Vec<Option<String>> =
        gradient.colors.iter().enumerate().map(|(i, c)| s.stops.get(i).cloned().flatten().filter(|_| s.gradient.colors.get(i) == Some(c))).collect();
    Some(splice(src, s.span, &gradient_text(gradient, &stops)))
}

/// A palette gradient name not used yet: `grad1`, `grad2`…
pub fn free_gradient_name(file: &str) -> String {
    free_name(file, "grad", &[])
}

/// Returns the script with a new gradient of the palette: a `let` line after the last line of
/// the palette, or at the top of the file.
pub fn add_gradient_swatch(src: &str, scene: &Scene, name: &str, gradient: &Gradient) -> String {
    let line = format!("let {name} = {}\n", gradient_text(gradient, &[]));
    let last = scene.gradients.iter().map(|s| s.span.end).chain(scene.palette.iter().map(|s| s.span.end)).max();
    match last {
        Some(end) if line_end(src, end) == src.len() && !src.ends_with('\n') => format!("{src}\n{line}"),
        Some(end) => {
            let at = line_end(src, end);
            splice(src, Span::new(at, at), &line)
        }
        None => {
            let at = top_of_file(src);
            splice(src, Span::new(at, at), &format!("{line}\n"))
        }
    }
}

/// Returns the script with the gradient written on the line of `node` moved into the palette
/// under `name`: a new `let` holds it, and the object takes it by the name. `None` when the
/// gradient is not written on the object's own line (a style gives it, a loop makes the
/// object, it is a palette gradient already), the object is written above the palette, or the
/// name cannot be used.
pub fn gradient_to_palette(src: &str, scene: &Scene, node: &str, stroke: bool, name: &str) -> Option<String> {
    if !swatch_name_ok(scene, name) || scene.sites.get(node)?.generated || scene.gradient_names.contains_key(&(node.to_string(), stroke)) {
        return None;
    }
    let key = if stroke { "stroke-gradient" } else { "gradient" };
    let span = *scene.prop_spans.get(&(node.to_string(), key.to_string()))?;
    let text = written(src[span.start..span.end].strip_prefix(key)?);
    let last = scene.gradients.iter().map(|s| s.span.end).chain(scene.palette.iter().map(|s| s.span.end)).max();
    let at = palette_end(src, last);
    if span.start < at {
        return None;
    }
    let line = format!("let {name} = {text}\n");
    let edits = vec![(span, format!("{key} {name}")), (Span::new(at, at), if last.is_none() { format!("{line}\n") } else { line })];
    Some(splice_all(src, edits))
}

/// Returns the script with the fill (or the stroke) of `node` taking the palette gradient
/// `name`: `gradient sky`. `None` for an object made by a loop or a symbol.
pub fn set_gradient_ref(src: &str, scene: &Scene, node: &str, stroke: bool, name: &str) -> Option<String> {
    scene.gradients.iter().find(|s| s.name == name)?;
    let key = if stroke { "stroke-gradient" } else { "gradient" };
    crate::edit::set_style(src, scene, node, key, Some(&format!("{key} {name}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{parse, parse_boards};

    fn scene(src: &str) -> Scene {
        parse(src).unwrap_or_else(|e| panic!("{}\n{src}", e.iter().map(|e| e.to_string()).collect::<Vec<_>>().join("\n")))
    }

    fn docs(file: &str) -> Vec<motif_core::Document> {
        parse_boards(file, None).unwrap_or_else(|e| panic!("{e:?}\n{file}")).into_iter().map(|s| s.doc).collect()
    }

    fn hex(c: &str) -> Color {
        Color::from_hex(c).unwrap()
    }

    fn first_error(src: &str) -> String {
        parse(src).unwrap_err()[0].message.clone()
    }

    #[test]
    fn a_gradient_of_the_palette_is_taken_by_name() {
        let src = "artboard \"a\" 200x100
let ink = #1b1d24
let sky = linear 90 #6fb8ff #ffffff
let ring = radial ink #ffffff #ffd23f
style plate gradient sky
rect a at 50 50 size 40 40 gradient sky
ellipse b at 120 50 size 40 40 stroke-gradient ring
rect c at 50 80 size 40 10 style plate
";
        let s = scene(src);
        let sky = Gradient { radial: false, angle: 90.0, colors: vec![hex("#6fb8ff"), hex("#ffffff")] };
        assert_eq!(s.doc.node("a").unwrap().gradient, Some(sky.clone()));
        assert_eq!(s.doc.node("c").unwrap().gradient, Some(sky.clone()));
        let b = s.doc.node("b").unwrap();
        assert_eq!(b.stroke_gradient.as_ref().unwrap().colors, vec![hex("#1b1d24"), hex("#ffffff"), hex("#ffd23f")]);
        assert!(b.stroke_gradient.as_ref().unwrap().radial && b.stroke_width == 2.0);
        // The palette lists them; an object knows the name it takes and the names of the colors.
        assert_eq!(s.gradients.iter().map(|g| g.name.as_str()).collect::<Vec<_>>(), ["sky", "ring"]);
        assert_eq!(s.gradients[0].gradient, sky);
        assert_eq!(&src[s.gradients[1].span.start..s.gradients[1].span.end], "radial ink #ffffff #ffd23f");
        assert_eq!(s.gradients[1].stops, vec![Some("ink".to_string()), None, None]);
        assert_eq!(s.gradient_names.get(&("a".to_string(), false)).map(String::as_str), Some("sky"));
        assert_eq!(s.gradient_names.get(&("c".to_string(), false)).map(String::as_str), Some("sky"));
        assert_eq!(s.gradient_names.get(&("b".to_string(), true)).map(String::as_str), Some("ring"));
        assert_eq!(s.gradient_refs.get(&("b".to_string(), true)), Some(&vec![Some("ink".to_string()), None, None]));
        // The palette changes: everything that takes the gradient follows.
        let s = scene(&src.replace("linear 90 #6fb8ff #ffffff", "linear 0 #000000 #ffffff #ff0000"));
        assert_eq!(s.doc.node("a").unwrap().gradient.as_ref().unwrap().colors.len(), 3);
        assert_eq!(s.doc.node("c").unwrap().gradient.as_ref().unwrap().angle, 0.0);
        // A file with one is of format 4; without, as before.
        assert_eq!(crate::format::needed(src), 4);
        assert_eq!(crate::format::needed("artboard \"a\" 9x9\nlet linear = 5\nrect a size 4 4 gradient linear 90 #000 #fff\n"), 1);

        assert!(first_error("artboard \"a\" 9x9\nrect a size 4 4 gradient sky\n").contains("нет градиента «sky» в палитре: градиент палитры пишется выше"));
        assert!(first_error("artboard \"a\" 9x9\nlet sea = radial #000 #fff\nrect a size 4 4 gradient sky\n").contains("есть: sea"));
        assert!(first_error("artboard \"a\" 9x9\nlet sky = linear 90 #000\n").contains("от двух цветов"));
        assert!(first_error("artboard \"a\" 9x9\nlet sky = #fff\nlet sky = linear 90 #000 #fff\n").contains("градиенту палитры нужно другое"));
        assert!(first_error("artboard \"a\" 9x9\nlet sky = linear 90 #000 #fff\nlet sky = #fff\n").contains("цвету нужно другое"));
        assert!(first_error("artboard \"a\" 9x9\ngroup g {\n  let sky = linear 90 #000 #fff\n}\n").contains("на верхнем уровне"));
    }

    const SET: &str = "artboard \"a\" 200x100 bg #ffffff
let ink = #1b1d24
let paper = ink
symbol Coin size=10 color=#ffd23f {
  ellipse body size {size} {size} fill color stroke #1b1d24 1
}
symbol Spare {
  rect body size 4 4 fill ink stroke #ffd23f 1 style idle
}
style card fill #ffffff stroke ink 2 shadow 0 2 4 #00000033
style idle fill #ffd23f stroke ink 1
rect a at 50 50 size 40 40 style card
rect b at 100 50 size 40 40 fill #ffd23f stroke #1b1d24 2 glow 6 #ffd23f
for i in 0..3 {
  ellipse d{i} at {i * 10} 90 size 6 6 fill #ffd23f
}
use Coin c1 at 20 20 color=#ff5a36
anim Blink 1s {
  b.fill 0: #ffd23f | 1: #ff5a36
}
override {
  d1.fill = #ff5a36
}
";

    #[test]
    fn every_place_of_a_color_is_known_once() {
        let s = scene(SET);
        let text = |site: &crate::ColorSite| &SET[site.span.start..site.span.end];
        let places = |word: &str| s.colors.iter().filter(|c| text(c) == word && !c.decl).count();
        // On the artboard line, in a style, on a line, in a loop (once), in a key, in a symbol
        // that is never placed and in a style that nothing takes.
        assert_eq!(places("#ffffff"), 2);
        assert_eq!(places("#ffd23f"), 6);
        assert_eq!(places("#ff5a36"), 3);
        assert_eq!(places("#1b1d24"), 2);
        // By name: in both styles and in the spare symbol. The value of `let paper` is where a
        // color is made, like the default of a symbol's parameter.
        assert_eq!(places("ink"), 3);
        assert_eq!(s.colors.iter().filter(|c| c.decl).map(text).collect::<Vec<_>>(), ["#ffd23f", "#1b1d24", "ink"]);
        assert!(s.colors.iter().all(|c| c.name.is_some() == (text(c) == "ink")));
        // A parameter of a symbol is not a color of the palette.
        assert!(!s.colors.iter().any(|c| text(c) == "color"));
        // Reading what is not used leaves nothing behind.
        assert!(s.doc.nodes.iter().all(|n| !n.id.starts_with("unused")));
        assert!(s.styled.keys().all(|id| !id.starts_with("unused")));
        assert_eq!(s.style_uses.iter().map(|u| u.style.as_str()).collect::<Vec<_>>(), ["card", "idle"]);

        let uses = color_uses(SET, None, 0).unwrap();
        assert_eq!(scene_color_uses(&s), uses);
        let line = |u: &ColorUse| format!("{} {}", u.name.clone().unwrap_or_else(|| u.color.to_hex()), u.places);
        assert_eq!(uses.iter().map(line).collect::<Vec<_>>(), ["ink 3", "paper 0", "#ffd23f 6", "#ff5a36 3", "#1b1d24 2", "#ffffff 2", "#00000033 1"]);
    }

    #[test]
    fn a_color_is_replaced_everywhere_by_a_value_or_a_name() {
        // A value by a value: every place, the palette and the defaults stay.
        let r = replace_color(SET, None, 0, &ColorWord::Hex(hex("#ffd23f")), &ColorWord::Hex(hex("#00c2a8"))).unwrap();
        assert_eq!((r.places, r.above), (6, 0));
        assert_eq!(r.file.matches("#00c2a8").count(), 6);
        assert!(r.file.contains("symbol Coin size=10 color=#ffd23f {"), "{}", r.file);
        assert!(r.file.contains("  b.fill 0: #00c2a8 | 1: #ff5a36\n"), "{}", r.file);
        assert!(r.file.contains("  rect body size 4 4 fill ink stroke #00c2a8 1 style idle\n"), "{}", r.file);
        let s = scene(&r.file);
        assert_eq!(s.doc.node("d2").unwrap().fill, Some(hex("#00c2a8")));
        assert_eq!(s.doc.node("b").unwrap().glow.unwrap().color, hex("#00c2a8"));

        // A value by a name: the place above the `let` cannot name it and is said so.
        let r = replace_color(SET, None, 0, &ColorWord::Hex(hex("#ffffff")), &ColorWord::Name("paper".into())).unwrap();
        assert_eq!((r.places, r.above), (1, 1));
        assert!(r.file.starts_with("artboard \"a\" 200x100 bg #ffffff\n"), "{}", r.file);
        assert!(r.file.contains("style card fill paper stroke ink 2 shadow"), "{}", r.file);
        let r = replace_color(SET, None, 0, &ColorWord::Hex(hex("#1b1d24")), &ColorWord::Name("ink".into())).unwrap();
        assert_eq!((r.places, r.above), (2, 0));
        assert!(r.file.contains("let ink = #1b1d24\n") && r.file.contains("fill color stroke ink 1\n") && r.file.contains("stroke ink 2 glow"), "{}", r.file);
        assert_eq!(docs(&r.file), docs(SET));

        // A name by a name and by a value; the `let` that is named after it stays.
        let r = replace_color(SET, None, 0, &ColorWord::Name("ink".into()), &ColorWord::Name("paper".into())).unwrap();
        assert_eq!(r.places, 3);
        assert!(
            r.file.contains("let paper = ink\n") && r.file.contains("stroke paper 2 shadow") && r.file.contains("fill paper stroke #ffd23f 1 style idle"),
            "{}",
            r.file
        );
        assert_eq!(docs(&r.file), docs(SET));
        let r = replace_color(SET, None, 0, &ColorWord::Name("ink".into()), &ColorWord::Hex(hex("#333333"))).unwrap();
        assert!(r.file.contains("style card fill #ffffff stroke #333333 2") && r.file.contains("let ink = #1b1d24\n"), "{}", r.file);

        // One color gets a name: its `let` joins the palette, the place above it stays.
        let r = name_color(SET, None, "sun", hex("#ffd23f")).unwrap();
        assert_eq!((r.places, r.above), (6, 0));
        assert!(r.file.contains("let paper = ink\nlet sun = #ffd23f\nsymbol Coin size=10 color=#ffd23f {"), "{}", r.file);
        assert!(r.file.contains("fill sun stroke #1b1d24 2 glow 6 sun\n") && r.file.contains("  b.fill 0: sun | 1: #ff5a36\n"), "{}", r.file);
        assert_eq!(docs(&r.file), docs(SET));
        let r = name_color(SET, None, "white", hex("#ffffff")).unwrap();
        assert_eq!((r.places, r.above), (1, 1));
        assert_eq!(name_color(SET, None, "ink", hex("#ffd23f")), None);
        assert_eq!(name_color(SET, None, "sun", hex("#123456")), None);

        // Nothing to do, and names that are not there.
        assert_eq!(replace_color(SET, None, 0, &ColorWord::Hex(hex("#123456")), &ColorWord::Hex(hex("#000000"))), None);
        assert_eq!(replace_color(SET, None, 0, &ColorWord::Name("paper".into()), &ColorWord::Hex(hex("#000000"))), None);
        assert_eq!(replace_color(SET, None, 0, &ColorWord::Name("ink".into()), &ColorWord::Name("ink".into())), None);
        assert_eq!(replace_color(SET, None, 0, &ColorWord::Name("none".into()), &ColorWord::Name("ink".into())), None);
        assert_eq!(replace_color(SET, None, 0, &ColorWord::Name("ink".into()), &ColorWord::Name("gone".into())), None);
    }

    const BOARDS: &str = "// set
let ink = #1b1d24
artboard \"one\" 100x100
rect a at 50 50 size 40 40 fill #ffd23f stroke ink 2
artboard \"two\" 100x100 cell 1 0
let own = #ff5a36
rect b at 50 50 size 40 40 fill #ffd23f stroke #1b1d24 2
rect c at 50 80 size 40 10 fill own stroke ink 1
";

    #[test]
    fn replacing_and_renaming_reach_every_board() {
        let before = docs(BOARDS);
        let r = replace_color(BOARDS, None, 0, &ColorWord::Hex(hex("#ffd23f")), &ColorWord::Hex(hex("#00c2a8"))).unwrap();
        assert_eq!(r.places, 2);
        assert!(r.file.contains("rect b at 50 50 size 40 40 fill #00c2a8 stroke #1b1d24 2\n"), "{}", r.file);
        let r = replace_color(BOARDS, None, 0, &ColorWord::Hex(hex("#1b1d24")), &ColorWord::Name("ink".into())).unwrap();
        assert!(r.file.contains("rect b at 50 50 size 40 40 fill #ffd23f stroke ink 2\n") && r.file.contains("let ink = #1b1d24\n"), "{}", r.file);
        assert_eq!(docs(&r.file), before);
        // A color one board keeps for itself cannot be written on another board.
        assert_eq!(replace_color(BOARDS, None, 1, &ColorWord::Hex(hex("#ffd23f")), &ColorWord::Name("own".into())), None);
        let r = replace_color(BOARDS, None, 1, &ColorWord::Name("own".into()), &ColorWord::Name("ink".into())).unwrap();
        assert!(r.file.contains("rect c at 50 80 size 40 10 fill ink stroke ink 1\n"), "{}", r.file);
        // The palette of the first board has no `own`.
        assert_eq!(replace_color(BOARDS, None, 0, &ColorWord::Name("own".into()), &ColorWord::Name("ink".into())), None);

        let out = rename_swatch(BOARDS, None, 1, "ink", "line").unwrap();
        assert!(out.contains("let line = #1b1d24\n") && out.contains("stroke line 2\n") && out.contains("fill own stroke line 1\n"), "{out}");
        assert!(!out.contains("ink"), "{out}");
        assert_eq!(docs(&out), before);
        assert_eq!(rename_swatch(BOARDS, None, 0, "ink", "own"), None);
        assert_eq!(rename_swatch(BOARDS, None, 0, "ink", "fill"), None);
        assert_eq!(rename_swatch(BOARDS, None, 0, "own", "mine"), None);
        let out = rename_swatch(BOARDS, None, 1, "own", "mine").unwrap();
        assert!(out.contains("let mine = #ff5a36\n") && out.contains("fill mine stroke ink 1\n"), "{out}");
    }

    #[test]
    fn a_new_name_follows_into_other_lets_gradients_and_what_is_not_used() {
        let src = "artboard \"a\" 100x100
let ink = #1b1d24
let paper = ink
let sky = linear 90 ink #ffffff
symbol Spare {
  rect body size 4 4 stroke ink 1 gradient sky
}
style idle fill ink
rect a at 50 50 size 40 40 gradient sky stroke ink 2
ellipse b at 50 80 size 10 10 stroke-gradient sky
anim A 1s {
  a.stroke 0: ink | 1: #ffffff
}
";
        let out = rename_swatch(src, None, 0, "ink", "dark").unwrap();
        assert!(!out.contains("ink"), "{out}");
        assert_eq!(out.matches("dark").count(), 7);
        assert_eq!(docs(&out), docs(src));
        // A gradient of the palette is renamed the same way.
        let out = rename_swatch(src, None, 0, "sky", "dawn").unwrap();
        assert!(out.contains("let dawn = linear 90 ink #ffffff\n"), "{out}");
        assert!(
            out.contains("stroke ink 1 gradient dawn\n") && out.contains("gradient dawn stroke ink 2\n") && out.contains("stroke-gradient dawn\n"),
            "{out}"
        );
        assert_eq!(docs(&out), docs(src));
        assert_eq!(rename_swatch(src, None, 0, "sky", "paper"), None);
        assert_eq!(rename_swatch(src, None, 0, "paper", "sky"), None);
    }

    #[test]
    fn repeated_colors_and_gradients_are_collected_into_the_palette() {
        let src = "motif 1
// Кнопки
artboard \"a\" 200x100 bg #fff4e4
rect a at 50 50 size 40 40 fill #ffd23f stroke #1b1d24 2 gradient linear 90 #ffd23f #ff5a36
rect b at 100 50 size 40 40 fill #ffd23f stroke #1b1d24 2 shadow 0 2 4 #00000033
ellipse c at 150 50 size 20 20 fill #7bd389 gradient linear  90 #ffd23f #ff5a36
for i in 0..2 {
  ellipse d{i} at {i * 10} 90 size 6 6 fill #1b1d24
}
anim Blink 1s {
  b.fill 0: #ffd23f | 1: #ff5a36
}
";
        let c = collect_palette(src, None).unwrap();
        // Three colors are written twice or more, one gradient twice; the rest stays as it is.
        assert_eq!(c.names, ["color1", "color2", "color3", "grad1"]);
        assert!(
            c.file.starts_with(
                "motif 1\nlet color1 = #ffd23f\nlet color2 = #1b1d24\nlet color3 = #ff5a36\nlet grad1 = linear 90 color1 color3\n\n// Кнопки\nartboard \"a\" 200x100 bg #fff4e4\n"
            ),
            "{}",
            c.file
        );
        assert!(c.file.contains("rect a at 50 50 size 40 40 fill color1 stroke color2 2 gradient grad1\n"), "{}", c.file);
        assert!(c.file.contains("rect b at 100 50 size 40 40 fill color1 stroke color2 2 shadow 0 2 4 #00000033\n"), "{}", c.file);
        assert!(c.file.contains("ellipse c at 150 50 size 20 20 fill #7bd389 gradient grad1\n"), "{}", c.file);
        assert!(c.file.contains("size 6 6 fill color2\n") && c.file.contains("  b.fill 0: color1 | 1: color3\n"), "{}", c.file);
        // Eleven places of colors and two of the gradient.
        assert_eq!(c.places, 13);
        assert_eq!(docs(&c.file), docs(src));
        // Done once, there is nothing more to collect.
        assert_eq!(collect_palette(&c.file, None), None);

        // A value equal to a color of the palette takes its name, even when written once; a
        // place above the palette keeps its value. A gradient written like one of the palette
        // takes its name; the colors of the palette's gradients are collected too.
        let src = "artboard \"a\" 100x100 bg #1b1d24
let ink = #1b1d24
let sky = linear 0 ink #ffffff
rect a at 50 50 size 40 40 fill #1b1d24 gradient linear 0 ink #ffffff
";
        let c = collect_palette(src, None).unwrap();
        assert_eq!(c.names, ["color1"]);
        assert_eq!(
            c.file,
            "artboard \"a\" 100x100 bg #1b1d24\nlet ink = #1b1d24\nlet color1 = #ffffff\nlet sky = linear 0 ink color1\nrect a at 50 50 size 40 40 fill ink gradient sky\n"
        );
        assert_eq!(docs(&c.file), docs(src));

        // In a file of several boards the new names go to the shared head.
        let c = collect_palette(BOARDS, None).unwrap();
        assert!(c.file.starts_with("// set\nlet ink = #1b1d24\nlet color1 = #ffd23f\nartboard \"one\""), "{}", c.file);
        assert!(c.file.contains("rect a at 50 50 size 40 40 fill color1 stroke ink 2\n"), "{}", c.file);
        assert!(c.file.contains("rect b at 50 50 size 40 40 fill color1 stroke ink 2\n"), "{}", c.file);
        assert_eq!(docs(&c.file), docs(BOARDS));
    }

    #[test]
    fn a_gradient_of_the_palette_is_added_set_and_assigned() {
        let src = "artboard \"a\" 100x100
let ink = #1b1d24
rect a at 50 50 size 40 40 fill #ffffff
rect b at 50 80 size 40 10 gradient linear 90 ink #ffffff
";
        let s = scene(src);
        let g = Gradient { radial: false, angle: 45.0, colors: vec![hex("#ff0000"), hex("#0000ff")] };
        assert_eq!(free_gradient_name(src), "grad1");
        let out = add_gradient_swatch(src, &s, "grad1", &g);
        assert!(out.contains("let ink = #1b1d24\nlet grad1 = linear 45 #ff0000 #0000ff\nrect a"), "{out}");
        let s2 = scene(&out);
        let out = set_gradient_ref(&out, &s2, "a", false, "grad1").unwrap();
        assert!(out.contains("rect a at 50 50 size 40 40 fill #ffffff gradient grad1\n"), "{out}");
        let s3 = scene(&out);
        assert_eq!(s3.doc.node("a").unwrap().gradient, Some(g.clone()));
        let out = set_gradient_ref(&out, &s3, "a", true, "grad1").unwrap();
        assert!(out.contains("gradient grad1 stroke-gradient grad1\n"), "{out}");
        assert_eq!(set_gradient_ref(&out, &s3, "a", false, "none"), None);
        // The gradient of the palette is edited in one place.
        let radial = Gradient { radial: true, ..g.clone() };
        let out = set_gradient_swatch(&out, &scene(&out), "grad1", &radial).unwrap();
        assert!(out.contains("let grad1 = radial #ff0000 #0000ff\n"), "{out}");
        assert!(scene(&out).doc.node("a").unwrap().gradient.as_ref().unwrap().radial);
        // An object edits its own gradient: it leaves the palette gradient, the palette stays.
        let own = crate::set_gradient(&out, &scene(&out), "a", Some(&g)).unwrap();
        assert!(own.contains("fill #ffffff gradient linear 45 #ff0000 #0000ff stroke-gradient grad1\n") && own.contains("let grad1 = radial"), "{own}");
        // A color written by a palette name keeps it while the color is the same.
        let mut wider = s.doc.node("b").unwrap().gradient.clone().unwrap();
        wider.angle = 30.0;
        wider.colors.push(hex("#ffd23f"));
        let out = crate::set_gradient(src, &s, "b", Some(&wider)).unwrap();
        assert!(out.contains("rect b at 50 80 size 40 10 gradient linear 30 #1b1d24 #ffffff #ffd23f\n"), "{out}");
        wider.colors.pop();
        let out = crate::set_gradient(src, &s, "b", Some(&wider)).unwrap();
        assert!(out.contains("rect b at 50 80 size 40 10 gradient linear 30 ink #ffffff\n"), "{out}");
        wider.colors[1] = hex("#000000");
        let out = crate::set_gradient(src, &s, "b", Some(&wider)).unwrap();
        assert!(out.contains("gradient linear 30 ink #000000\n"), "{out}");
        // The gradient of an object gets a name: the names of its colors go with it.
        let out = gradient_to_palette(src, &s, "b", false, "dusk").unwrap();
        assert!(out.contains("let ink = #1b1d24\nlet dusk = linear 90 ink #ffffff\nrect a"), "{out}");
        assert!(out.contains("rect b at 50 80 size 40 10 gradient dusk\n"), "{out}");
        assert_eq!(scene(&out).doc, s.doc);
        assert_eq!(gradient_to_palette(&out, &scene(&out), "b", false, "more"), None);
        assert_eq!(gradient_to_palette(src, &s, "b", false, "ink"), None);
        assert_eq!(gradient_to_palette(src, &s, "a", false, "dusk"), None);
        // In the palette too.
        let with = "artboard \"a\" 9x9\nlet ink = #1b1d24\nlet sky = linear 90 ink #ffffff\n";
        let out = set_gradient_swatch(with, &scene(with), "sky", &wider).unwrap();
        assert!(out.contains("let sky = linear 30 ink #000000\n"), "{out}");
    }
}
