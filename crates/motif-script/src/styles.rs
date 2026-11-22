//! Named styles: a `style card fill #ffffff stroke ink 2 r 10` line keeps a look under a name,
//! and an object takes it with `style card` on its line. What the object writes itself wins over
//! the style. The style is applied when the script is read: the document, the picture and the
//! game see ordinary objects.

use crate::edit::{splice_all, with_space_before};
use crate::parser::STYLE_KEYS;
use crate::{Scene, Span, StyleSite, StyleUse};
use motif_core::ShapeKind;

/// Whether an object of `kind` takes property `key` from its style, when its own line writes
/// the properties `own`.
pub(crate) fn taken(key: &str, kind: ShapeKind, own: &[&str]) -> bool {
    // A plain fill of the object's own replaces the gradient of the style.
    let pair = match key {
        "gradient" => "fill",
        "stroke-gradient" => "stroke",
        _ => "",
    };
    !own.contains(&key) && !own.contains(&pair) && fits(key, kind)
}

/// A style may hold more than one kind of object can show: letters have a font, shapes a brush.
/// An object takes what its kind has and leaves the rest.
fn fits(key: &str, kind: ShapeKind) -> bool {
    match key {
        "font" | "font-size" | "letter-spacing" | "line-height" | "align" => kind == ShapeKind::Text,
        "rough" | "boil" | "bands" | "hatch" | "grain" | "seed" => !kind.is_container(),
        _ => true,
    }
}

/// The other way to paint the same thing: a fill and a gradient, a stroke and its gradient.
fn pair(key: &str) -> &'static str {
    match key {
        "fill" => "gradient",
        "gradient" => "fill",
        "stroke" => "stroke-gradient",
        "stroke-gradient" => "stroke",
        _ => "",
    }
}

fn site<'a>(scene: &'a Scene, name: &str) -> Option<&'a StyleSite> {
    scene.styles.iter().find(|s| s.name == name)
}

/// `style card` on the line of `node`, when the node has a line of its own.
fn use_of<'a>(scene: &'a Scene, node: &str) -> Option<&'a StyleUse> {
    if scene.sites.get(node)?.generated {
        return None;
    }
    let span = scene.prop_spans.get(&(node.to_string(), "style".to_string()))?;
    scene.style_uses.iter().find(|u| u.span.start == span.start)
}

/// What `no shadow` refuses: `shadow`.
fn refused(own: &str) -> Option<&str> {
    own.strip_prefix("no ")
}

/// Whether the style of `node` gives it property `key` when the object writes none of its own.
pub(crate) fn gives(scene: &Scene, node: &str, key: &str) -> bool {
    let Some(style) = scene.styled.get(node).and_then(|name| site(scene, name)) else { return false };
    let kind = scene.doc.node(node).map(|n| n.kind);
    style.props.iter().any(|(k, _)| k == key) && kind.is_some_and(|kind| fits(key, kind))
}

/// The look properties written on the line of `node`, in the order written, each with where.
/// `no shadow`, a property of the style the object refuses, is one of them.
fn own_spans(scene: &Scene, node: &str) -> Vec<(String, Span)> {
    let at = |key: String| scene.prop_spans.get(&(node.to_string(), key.clone())).map(|s| (key, *s));
    let mut own: Vec<_> = STYLE_KEYS.iter().flat_map(|k| [at(k.to_string()), at(format!("no {k}"))]).flatten().collect();
    own.sort_by_key(|(_, s)| s.start);
    own
}

/// The look properties `node` writes on its own line, in the order written. For an object with
/// a style these are the ones that differ from it; `no shadow` says it has none.
pub fn own_look(scene: &Scene, node: &str) -> Vec<String> {
    own_spans(scene, node).into_iter().map(|(k, _)| k).collect()
}

/// Whether `name` can name a new style: a word of the script that is not a style yet and not a
/// property.
pub fn style_name_ok(scene: &Scene, name: &str) -> bool {
    let mut chars = name.chars();
    chars.next().is_some_and(|c| c.is_alphabetic() || c == '_')
        && chars.all(|c| c.is_alphanumeric() || c == '_' || c == '-')
        && !name.ends_with('-')
        && crate::prop_arity(name).is_none()
        && name != "none"
        && site(scene, name).is_none()
}

/// A style name not used yet: `style1`, `style2`… `file` is the whole file: a new style of the
/// shared head must not meet a style that another board keeps for itself.
pub fn free_style_name(file: &str) -> String {
    let taken: Vec<&str> = file.lines().filter_map(|l| l.trim_start().strip_prefix("style")?.split_whitespace().next()).collect();
    (1..).map(|n| format!("style{n}")).find(|n| !taken.contains(&n.as_str())).expect("some number is free")
}

/// What the style gives to the line of `u`, written out as properties.
fn written_out(src: &str, scene: &Scene, u: &StyleUse) -> Vec<String> {
    let own: Vec<&str> = u.own.iter().map(|(k, _)| refused(k).unwrap_or(k)).collect();
    let Some(style) = site(scene, &u.style) else { return Vec::new() };
    style.props.iter().filter(|(k, _)| taken(k, u.kind, &own)).map(|(_, s)| src[s.start..s.end].to_string()).collect()
}

/// `style card` on a line replaced by what the style gives to it: the line looks the same and
/// no longer follows the style. What the line refused (`no shadow`) has nothing left to refuse.
fn bake(src: &str, scene: &Scene, u: &StyleUse) -> Vec<(Span, String)> {
    let props = written_out(src, scene, u);
    let mut edits: Vec<(Span, String)> = u.own.iter().filter(|(k, _)| refused(k).is_some()).map(|(_, s)| (with_space_before(src, *s), String::new())).collect();
    edits.push(if props.is_empty() { (with_space_before(src, u.span), String::new()) } else { (u.span, props.join(" ")) });
    edits
}

/// Where a new `style` line goes and what is put there: after the last style, else after the
/// palette, else at the top of the file.
fn new_line(src: &str, scene: &Scene, line: &str) -> (Span, String) {
    let last = scene.styles.iter().map(|s| s.decl.end).max().or(scene.palette.iter().map(|s| s.decl.end).max());
    match last {
        Some(end) => match src[end..].find('\n') {
            Some(i) => (Span::new(end + i + 1, end + i + 1), format!("{line}\n")),
            None => (Span::new(src.len(), src.len()), format!("\n{line}\n")),
        },
        None => {
            let at = crate::palette::top_of_file(src);
            (Span::new(at, at), format!("{line}\n\n"))
        }
    }
}

/// Writes `style name` on the line of an object: in place of the style it had, or at the end.
fn name_on_line(scene: &Scene, node: &str, name: &str) -> Option<(Span, String)> {
    let at = scene.sites.get(node)?.insert_at;
    Some(match scene.prop_spans.get(&(node.to_string(), "style".to_string())) {
        Some(span) => (*span, format!("style {name}")),
        None => (Span::new(at, at), format!(" style {name}")),
    })
}

/// Returns the script with the look of `node` kept as a new style `name`, and the object taking
/// it: the look properties leave its line. `None` when the object is made by a loop or a symbol,
/// has no look written, or the name cannot be used.
pub fn make_style(src: &str, scene: &Scene, node: &str, name: &str) -> Option<String> {
    if scene.sites.get(node)?.generated || !style_name_ok(scene, name) {
        return None;
    }
    let own = own_spans(scene, node);
    // What the object takes from the style it already has is part of its look too.
    let mut props = use_of(scene, node).map(|u| written_out(src, scene, u)).unwrap_or_default();
    props.extend(own.iter().filter(|(k, _)| refused(k).is_none()).map(|(_, s)| src[s.start..s.end].to_string()));
    if props.is_empty() {
        return None;
    }
    let mut edits: Vec<(Span, String)> = own.iter().map(|(_, s)| (with_space_before(src, *s), String::new())).collect();
    edits.push(name_on_line(scene, node, name)?);
    edits.push(new_line(src, scene, &format!("style {name} {}", props.join(" "))));
    Some(splice_all(src, edits))
}

/// Returns the script with every one of `nodes` taking the style `name`. An object's own
/// properties that the style sets leave its line, so it looks as the style says; the others
/// stay. Objects made by a loop or a symbol are passed by. `None` when there is no such style or
/// no object could take it.
pub fn assign_style(src: &str, scene: &Scene, nodes: &[String], name: &str) -> Option<String> {
    let style = site(scene, name)?;
    let mut edits: Vec<(Span, String)> = Vec::new();
    for node in nodes {
        let Some(kind) = scene.doc.node(node).map(|n| n.kind) else { continue };
        if scene.sites.get(node).is_none_or(|s| s.generated) {
            continue;
        }
        let set: Vec<&str> = style.props.iter().map(|(k, _)| k.as_str()).filter(|k| fits(k, kind)).collect();
        // What the object refused was about the style it had.
        edits.extend(
            own_spans(scene, node)
                .into_iter()
                .filter(|(k, _)| refused(k).is_some() || set.contains(&k.as_str()) || set.contains(&pair(k)))
                .map(|(_, s)| (with_space_before(src, s), String::new())),
        );
        edits.extend(name_on_line(scene, node, name));
    }
    (!edits.is_empty()).then(|| splice_all(src, edits))
}

/// Returns the script with `node` free of its style: what the style gave is written on the
/// object's line, so it looks the same. `None` when the object has no style on a line of its own.
pub fn detach_style(src: &str, scene: &Scene, node: &str) -> Option<String> {
    Some(splice_all(src, bake(src, scene, use_of(scene, node)?)))
}

/// Returns the script with the object's own look properties removed (one, or all with `None`):
/// the object looks as its style says again. `None` when it has no style or nothing to remove.
pub fn reset_to_style(src: &str, scene: &Scene, node: &str, key: Option<&str>) -> Option<String> {
    use_of(scene, node)?;
    let edits: Vec<(Span, String)> =
        own_spans(scene, node).into_iter().filter(|(k, _)| key.is_none_or(|key| key == k)).map(|(_, s)| (with_space_before(src, s), String::new())).collect();
    (!edits.is_empty()).then(|| splice_all(src, edits))
}

/// Returns the script with the own look properties of `node` moved into its style: every object
/// that takes the style changes with it. `None` when the object has no style or nothing of its
/// own to give.
///
/// While a theme is shown the look goes into the style's line of that theme and the object keeps
/// its own properties: the scene as written and the other themes stay as they were.
pub fn write_into_style(src: &str, scene: &Scene, node: &str) -> Option<String> {
    let style = site(scene, &use_of(scene, node)?.style)?;
    let own = own_spans(scene, node);
    if own.is_empty() {
        return None;
    }
    let mut edits = Vec::new();
    let mut added = String::new();
    // The line of an object is one for every theme. While a theme is shown the object keeps
    // what it has of its own: without it the object would change in the scene as written and
    // in the other themes, which nobody is looking at.
    let keeps_own = scene.theme.is_some();
    for (key, span) in &own {
        if !keeps_own {
            edits.push((with_space_before(src, *span), String::new()));
        }
        let at = |key: &str| style.props.iter().find(|(k, _)| k == key).map(|(_, s)| *s);
        // What the object refuses leaves the style.
        if let Some(gone) = refused(key) {
            edits.extend(at(gone).map(|s| (with_space_before(src, s), String::new())));
            continue;
        }
        let text = &src[span.start..span.end];
        match at(key) {
            Some(s) => edits.push((s, text.to_string())),
            None => added.push_str(&format!(" {text}")),
        }
        // A plain fill takes the place of the gradient the style had.
        if matches!(key.as_str(), "fill" | "stroke")
            && let Some(s) = at(pair(key))
        {
            edits.push((with_space_before(src, s), String::new()));
        }
    }
    if !added.is_empty() {
        edits.push((Span::new(style.insert_at, style.insert_at), added));
    }
    // A gradient may leave the style for two reasons at once: a plain fill and `no gradient`.
    edits.sort_by_key(|(s, _)| (s.start, s.end));
    edits.dedup_by(|a, b| a.0 == b.0);
    if scene.theme.is_some() && !style.themed {
        // The theme being shown gets the line of the style with the object's look written
        // into it; the style's own line stays as it is for the other themes.
        let line = Span::new(style.span.start, style.span.end.max(style.insert_at));
        let (mut changes, mut edits): (Vec<_>, Vec<_>) = edits.into_iter().partition(|(s, _)| s.start >= line.start && s.end <= line.end);
        for (s, _) in &mut changes {
            *s = Span::new(s.start - line.start, s.end - line.start);
        }
        let text = splice_all(&src[line.start..line.end], changes);
        edits.push(crate::themes::new_entry(src, scene, style.decl, text.trim_end())?);
        return Some(splice_all(src, edits));
    }
    // Under a theme the style may already say all the object has.
    Some(splice_all(src, edits)).filter(|out| out != src)
}

/// What removing the style `name` changes in the script: every line that took it gets what the
/// style gave written out, and the line of the style goes.
fn delete_edits(src: &str, scene: &Scene, name: &str) -> Option<Vec<(Span, String)>> {
    let style = site(scene, name)?;
    let mut edits: Vec<(Span, String)> = scene.style_uses.iter().filter(|u| u.style == name).flat_map(|u| bake(src, scene, u)).collect();
    // The whole line of the style, with its line break, and its lines in the themes.
    let lines = scene.themes.iter().flat_map(|t| &t.entries).filter(|e| e.style && e.name == name).map(|e| e.span);
    for line in lines.chain([style.decl]) {
        edits.push((crate::themes::whole_line(src, line), String::new()));
    }
    Some(edits)
}

/// What calling the style `old` by the name `new` changes: its line and every line that takes it.
fn rename_edits(scene: &Scene, old: &str, new: &str) -> Option<Vec<(Span, String)>> {
    let style = site(scene, old)?;
    if !style_name_ok(scene, new) {
        return None;
    }
    let mut edits = vec![(style.decl_name, new.to_string())];
    edits.extend(scene.style_uses.iter().filter(|u| u.style == old).map(|u| (u.name_span, new.to_string())));
    // The lines the themes give the style follow its name.
    edits.extend(scene.themes.iter().flat_map(|t| &t.entries).filter(|e| e.style && e.name == old).map(|e| (e.name_span, new.to_string())));
    Some(edits)
}

/// Returns the script without the style `name`: every line that took it gets what the style
/// gave written out, so nothing changes its look. `None` when there is no such style.
pub fn delete_style(src: &str, scene: &Scene, name: &str) -> Option<String> {
    Some(splice_all(src, delete_edits(src, scene, name)?))
}

/// Returns the script with the style `old` called `new`, on its line and on every line that
/// takes it. `None` when there is no such style or the new name cannot be used.
pub fn rename_style(src: &str, scene: &Scene, old: &str, new: &str) -> Option<String> {
    Some(splice_all(src, rename_edits(scene, old, new)?))
}

/// An edit of the style `name` of board `board` made in the whole file: a style of the shared
/// head is taken by objects of every board, and each of them is read to find its lines. `edits`
/// gives the changes of one board's script. `None` when a board has errors, the board has no
/// such style, or the edit cannot be made on one of the boards.
fn in_file(
    file: &str,
    base: Option<&std::path::Path>,
    board: usize,
    name: &str,
    edits: impl Fn(&str, &Scene) -> Option<Vec<(Span, String)>>,
) -> Option<String> {
    let views = crate::boards::board_views(file, base)?;
    let line = |v: &crate::boards::BoardView| site(&v.scene, name).map(|s| v.in_file(s.span));
    let which = line(views.get(board)?)?;
    let mut all: Vec<(Span, String)> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    // A board with a style of its own under the same name is another style: it stays.
    for v in views.iter().filter(|v| line(v) == Some(which)) {
        for (span, text) in edits(&v.text, &v.scene)? {
            let span = v.in_file(span);
            // What is written in the head is found on every board.
            if seen.insert((span.start, span.end)) {
                all.push((span, text));
            }
        }
    }
    Some(splice_all(file, all))
}

/// [`delete_style`] in a file of several boards: the objects of every board keep their look.
pub fn delete_style_in_file(file: &str, base: Option<&std::path::Path>, board: usize, name: &str) -> Option<String> {
    in_file(file, base, board, name, |src, scene| delete_edits(src, scene, name))
}

/// [`rename_style`] in a file of several boards: every board that takes the style follows.
pub fn rename_style_in_file(file: &str, base: Option<&std::path::Path>, board: usize, old: &str, new: &str) -> Option<String> {
    in_file(file, base, board, old, |_, scene| rename_edits(scene, old, new))
}

/// Returns the file with every object that takes the style `from` taking `to` instead, on
/// every board, and how many lines changed. What a line writes over its style stays; what it
/// refused (`no shadow`) was about the old style and goes. `board` is the board whose styles
/// the names are of. `None` when a board has errors, one of the styles is missing, `to` is not
/// seen on a board that uses `from`, or nothing takes `from`.
pub fn replace_style(file: &str, base: Option<&std::path::Path>, board: usize, from: &str, to: &str) -> Option<(String, usize)> {
    if from == to {
        return None;
    }
    let views = crate::boards::board_views(file, base)?;
    let line = |v: &crate::boards::BoardView, name: &str| site(&v.scene, name).map(|s| v.in_file(s.span));
    let active = views.get(board)?;
    let (from_at, to_at) = (line(active, from)?, line(active, to)?);
    let mut edits: Vec<(Span, String)> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    let mut lines = 0;
    for v in views.iter().filter(|v| line(v, from) == Some(from_at)) {
        for u in v.scene.style_uses.iter().filter(|u| u.style == from) {
            if line(v, to) != Some(to_at) {
                return None;
            }
            if !seen.insert(v.in_file(u.name_span).start) {
                continue;
            }
            lines += 1;
            edits.push((v.in_file(u.name_span), to.to_string()));
            edits.extend(u.own.iter().filter(|(k, _)| refused(k).is_some()).map(|(_, s)| (v.in_file(with_space_before(&v.text, *s)), String::new())));
        }
    }
    if edits.is_empty() {
        return None;
    }
    let out = splice_all(file, edits);
    crate::parse_boards(&out, base).ok()?;
    Some((out, lines))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Scene, parse, set_prop};
    use motif_core::{Color, Prop, Value};

    fn scene(src: &str) -> Scene {
        parse(src).unwrap_or_else(|e| panic!("{}", e.iter().map(|e| e.to_string()).collect::<Vec<_>>().join("\n")))
    }

    fn hex(c: &str) -> Color {
        Color::from_hex(c).unwrap()
    }

    const CARDS: &str = "artboard \"a\" 200x100
let ink = #1b1d24
style card fill #ffffff stroke ink 2 r 10 shadow 0 4 8 #00000033
rect a at 50 50 size 80 40 style card
rect b at 150 50 size 80 40 style card fill #ffd23f
";

    #[test]
    fn an_object_takes_the_look_of_its_style_and_its_own_word_wins() {
        let s = scene(CARDS);
        let (a, b) = (s.doc.node("a").unwrap(), s.doc.node("b").unwrap());
        assert_eq!(a.fill, Some(hex("#ffffff")));
        assert_eq!((a.stroke, a.stroke_width, a.radius), (Some(hex("#1b1d24")), 2.0, 10.0));
        assert_eq!(a.shadow.unwrap().blur, 8.0);
        // `b` writes its own fill: it wins, the rest still comes from the style.
        assert_eq!(b.fill, Some(hex("#ffd23f")));
        assert_eq!((b.stroke_width, b.radius), (2.0, 10.0));
        assert_eq!(s.styled.get("a").map(String::as_str), Some("card"));
        assert_eq!(own_look(&s, "a"), Vec::<String>::new());
        assert_eq!(own_look(&s, "b"), vec!["fill"]);
        // The style changes: every object that takes it changes, `b` keeps its own fill.
        let s = scene(&CARDS.replace("fill #ffffff stroke ink 2", "fill #e8f0ff stroke ink 5"));
        assert_eq!(s.doc.node("a").unwrap().fill, Some(hex("#e8f0ff")));
        assert_eq!(s.doc.node("b").unwrap().fill, Some(hex("#ffd23f")));
        assert_eq!(s.doc.node("b").unwrap().stroke_width, 5.0);
    }

    #[test]
    fn a_style_may_be_written_below_and_works_in_loops_groups_and_symbols() {
        let s = scene(
            "artboard \"a\" 200x100
symbol Chip {
  rect body size 20 10 style chip
}
group g at 10 10 {
  for i in 0..3 {
    ellipse dot{i} at {i * 10} 0 size 6 6 style chip
  }
}
use Chip c1 at 100 50
style chip fill #2a6fdb stroke #1b1d24 1
",
        );
        for id in ["dot0", "dot2", "c1-body"] {
            assert_eq!(s.doc.node(id).unwrap().fill, Some(hex("#2a6fdb")), "{id}");
            assert_eq!(s.styled.get(id).map(String::as_str), Some("chip"), "{id}");
        }
        // The line in the loop is one place of the script, whatever it makes.
        assert_eq!(s.style_uses.len(), 2);
    }

    #[test]
    fn a_style_gives_each_kind_what_that_kind_has() {
        let s = scene(
            "artboard \"a\" 200x100
style title fill #1b1d24 font bold font-size 30 align left rough 2 opacity 0.5
text t at 10 10 content \"Заголовок\" style title
rect r at 50 50 size 20 20 style title
group g at 0 0 style title {
  rect inner size 4 4
}
",
        );
        let (t, r, g) = (s.doc.node("t").unwrap(), s.doc.node("r").unwrap(), s.doc.node("g").unwrap());
        assert_eq!((t.get(Prop::FontSize), t.opacity), (Value::Num(30.0), 0.5));
        // A rectangle has no font, a group no brush: they take the rest.
        assert_eq!((r.fill, r.opacity, r.brush.rough), (Some(hex("#1b1d24")), 0.5, 2.0));
        assert_eq!((g.opacity, g.brush.rough), (0.5, 0.0));
    }

    #[test]
    fn a_plain_fill_of_the_object_replaces_the_gradient_of_the_style() {
        let s = scene(
            "artboard \"a\" 200x100
style sky gradient linear 90 #2a6fdb #c58cff stroke-gradient linear 0 #ffffff #000000
rect a size 20 20 style sky
rect b size 20 20 style sky fill #ff5a36 stroke #1b1d24 3
",
        );
        assert!(s.doc.node("a").unwrap().gradient.is_some());
        let b = s.doc.node("b").unwrap();
        assert_eq!((b.gradient.is_some(), b.stroke_gradient.is_some()), (false, false));
        assert_eq!((b.fill, b.stroke_width), (Some(hex("#ff5a36")), 3.0));
    }

    #[test]
    fn mistakes_in_styles_are_told_where_they_are_written() {
        let err = |src: &str| parse(src).unwrap_err().iter().map(|e| format!("{}: {}", e.line, e.message)).collect::<Vec<_>>();
        let e = err("artboard \"a\" 10x10\nrect a size 4 4 style card\n");
        assert!(e.len() == 1 && e[0].starts_with("2: нет стиля «card»"), "{e:?}");
        let e = err("artboard \"a\" 10x10\nstyle card fill #fff\nstyle soft r 4\nrect a size 4 4 style crad\n");
        assert!(e[0].contains("есть: card, soft"), "{e:?}");
        let e = err("artboard \"a\" 10x10\nstyle card fill #fff\nstyle card r 4\n");
        assert!(e.len() == 1 && e[0].starts_with("3: стиль «card» уже есть"), "{e:?}");
        // Only the look: where an object is and what it is made of stay on its line.
        let e = err("artboard \"a\" 10x10\nstyle card fill #fff at 10 10\n");
        assert!(e.len() == 1 && e[0].contains("«at» остаётся на строке объекта"), "{e:?}");
        let e = err("artboard \"a\" 10x10\nstyle card fill #fff\nstyle big style card r 4\n");
        assert!(e.len() == 1 && e[0].contains("стиль не берёт другой стиль"), "{e:?}");
        for (bad, says) in [
            ("style\n", "после style нужно имя"),
            ("style fill #fff\n", "после style нужно имя"),
            ("style card\n", "после style нужно имя"),
            ("style card \"Карточка\" fill #fff\n", "без кавычек"),
            ("style card fill #fff {\n}\n", "одной строкой"),
            ("group g {\n  style card fill #fff\n}\n", "на верхнем уровне"),
            ("rect a size 4 4 style\n", "после style нужно имя стиля"),
        ] {
            let e = err(&format!("artboard \"a\" 10x10\n{bad}"));
            assert!(e.iter().any(|m| m.contains(says)), "{bad:?}: {e:?}");
        }
        // A wrong value in a style is one mistake on its line, however many objects take it.
        let one = err("artboard \"a\" 10x10\nstyle card fill nothing r 4\nrect a size 4 4 style card\n");
        let three =
            err("artboard \"a\" 10x10\nstyle card fill nothing r 4\nrect a size 4 4 style card\nrect b size 4 4 style card\nrect c size 4 4 style card\n");
        assert!(!one.is_empty() && one.iter().all(|m| m.starts_with("2: ")), "{one:?}");
        assert_eq!(three, one);
    }

    #[test]
    fn an_edit_of_a_styled_property_becomes_the_objects_own() {
        let s = scene(CARDS);
        // The fill of `a` is written in the style: the edit does not touch the style, it adds
        // the object's own property.
        let out = set_prop(CARDS, &s, "a", Prop::Fill, Value::Color(hex("#ff5a36"))).unwrap();
        assert!(out.contains("rect a at 50 50 size 80 40 style card fill #ff5a36\n"), "{out}");
        assert!(out.contains("style card fill #ffffff stroke ink 2 r 10"), "{out}");
        let out = set_prop(CARDS, &s, "a", Prop::StrokeWidth, Value::Num(6.0)).unwrap();
        assert!(out.contains("style card stroke #1b1d24 6\n"), "{out}");
        let s2 = scene(&out);
        assert_eq!(s2.doc.node("a").unwrap().stroke_width, 6.0);
        assert_eq!(s2.doc.node("b").unwrap().stroke_width, 2.0);
        // And back: the own property goes, the style shows again.
        let back = reset_to_style(&out, &s2, "a", Some("stroke")).unwrap();
        assert_eq!(back, CARDS);
        assert_eq!(reset_to_style(&back, &scene(&back), "a", None), None);
        let all = reset_to_style(CARDS, &s, "b", None).unwrap();
        assert!(all.contains("rect b at 150 50 size 80 40 style card\n"), "{all}");
    }

    #[test]
    fn the_look_of_an_object_becomes_a_style() {
        let src = "motif 1
// Экран
artboard \"a\" 200x100
rect a at 50 50 size 80 40 r 8 fill #ffffff stroke #1b1d24 2 rot 5
rect b at 150 50 size 80 40
anim Idle 1s {
  a.y 0: 50 | 1: 60
}
";
        let s = scene(src);
        assert_eq!(free_style_name(src), "style1");
        // A name kept by another board of the file is not free either.
        assert_eq!(free_style_name("style style1 r 2\nartboard \"a\" 9x9\nartboard \"b\" 9x9\n  style style2 r 4\n"), "style3");
        let out = make_style(src, &s, "a", "card").unwrap();
        // The look leaves the line, the place and the turn stay. The style goes to the top.
        assert!(out.starts_with("motif 1\nstyle card r 8 fill #ffffff stroke #1b1d24 2\n\n// Экран\nartboard"), "{out}");
        assert!(out.contains("rect a at 50 50 size 80 40 rot 5 style card\n"), "{out}");
        let s2 = scene(&out);
        assert_eq!(s2.doc.node("a"), s.doc.node("a"));
        // The next style goes after the first; another object takes a style by name.
        let out = assign_style(&out, &s2, &["b".to_string()], "card").unwrap();
        assert!(out.contains("rect b at 150 50 size 80 40 style card\n"), "{out}");
        let s3 = scene(&out);
        assert_eq!(s3.doc.node("b").unwrap().radius, 8.0);
        let out = set_prop(&out, &s3, "b", Prop::Fill, Value::Color(hex("#ffd23f"))).unwrap();
        let s4 = scene(&out);
        let out = make_style(&out, &s4, "b", "gold").unwrap();
        // The new style holds the whole look of `b`: what it took from `card` and its own fill.
        assert!(out.contains("style card r 8 fill #ffffff stroke #1b1d24 2\nstyle gold r 8 stroke #1b1d24 2 fill #ffd23f\n\n// Экран"), "{out}");
        assert!(out.contains("rect b at 150 50 size 80 40 style gold\n"), "{out}");
        assert_eq!(scene(&out).doc.node("b"), s4.doc.node("b"));
        // Names that cannot be used, and objects with nothing to keep.
        let s5 = scene(&out);
        for bad in ["card", "fill", "2x", "", "my style", "none"] {
            assert_eq!(make_style(&out, &s5, "a", bad), None, "{bad}");
            assert!(!style_name_ok(&s5, bad), "{bad}");
        }
        assert!(style_name_ok(&s5, "card-big") && style_name_ok(&s5, "карточка"));
        let bare = "artboard \"a\" 10x10\nrect a size 4 4\n";
        assert_eq!(make_style(bare, &scene(bare), "a", "card"), None);
    }

    #[test]
    fn assigning_a_style_drops_what_the_style_sets() {
        let src = "artboard \"a\" 200x100
let ink = #1b1d24
style card fill #ffffff stroke ink 2
style sky gradient linear 90 #2a6fdb #c58cff
rect a at 50 50 size 80 40 fill #ff5a36 stroke-gradient linear 0 #fff #000 r 6 style sky
";
        let s = scene(src);
        // `card` sets the fill and the stroke: the object's own fill and stroke gradient go, its
        // radius stays.
        let out = assign_style(src, &s, &["a".to_string()], "card").unwrap();
        assert!(out.contains("rect a at 50 50 size 80 40 r 6 style card\n"), "{out}");
        let a = scene(&out);
        let a = a.doc.node("a").unwrap();
        assert_eq!((a.fill, a.stroke_gradient.is_some(), a.radius), (Some(hex("#ffffff")), false, 6.0));
        // `sky` sets a gradient: a plain fill of the object would hide it, so it goes too.
        let src2 = src.replace(" style sky\n", "\n");
        let out = assign_style(&src2, &scene(&src2), &["a".to_string()], "sky").unwrap();
        assert!(out.contains("rect a at 50 50 size 80 40 stroke-gradient linear 0 #fff #000 r 6 style sky\n"), "{out}");
        assert!(scene(&out).doc.node("a").unwrap().gradient.is_some());
        assert_eq!(assign_style(src, &s, &["a".to_string()], "nothing"), None);
        // Several objects at once.
        let many = "artboard \"a\" 10x10\nstyle s fill #fff\nrect a size 4 4 fill #000\ntext t content \"x\"\nrect c size 4 4 r 2\n";
        let out = assign_style(many, &scene(many), &["a".to_string(), "t".to_string(), "c".to_string()], "s").unwrap();
        assert!(out.ends_with("rect a size 4 4 style s\ntext t content \"x\" style s\nrect c size 4 4 r 2 style s\n"), "{out}");
    }

    #[test]
    fn own_properties_are_written_into_the_style() {
        let src = "artboard \"a\" 200x100
style card fill #ffffff gradient linear 90 #fff #eee stroke #1b1d24 2
rect a at 50 50 size 80 40 style card fill #ffd23f r 12 stroke #1b1d24 4
rect b at 150 50 size 80 40 style card
";
        let s = scene(src);
        let out = write_into_style(src, &s, "a").unwrap();
        // The fill and the stroke are replaced where they stand, the gradient gives way to the
        // plain fill, the radius is added. The object keeps nothing of its own.
        assert!(out.contains("style card fill #ffd23f stroke #1b1d24 4 r 12\n"), "{out}");
        assert!(out.contains("rect a at 50 50 size 80 40 style card\n"), "{out}");
        let s2 = scene(&out);
        assert_eq!(s2.doc.node("a"), s.doc.node("a"));
        // The other object follows.
        let b = s2.doc.node("b").unwrap();
        assert_eq!((b.fill, b.gradient.is_some(), b.radius, b.stroke_width), (Some(hex("#ffd23f")), false, 12.0, 4.0));
        assert_eq!(write_into_style(&out, &s2, "a"), None);
    }

    #[test]
    fn a_style_is_detached_deleted_and_renamed_without_changing_the_picture() {
        let src = "artboard \"a\" 200x100
style card fill #ffffff stroke #1b1d24 2 font-size 20 // карточки
style empty font-size 14
rect a at 50 50 size 80 40 style card
rect b at 150 50 size 80 40 fill #ffd23f style card r 4
rect c at 10 10 size 4 4 style empty
group g at 0 0 {
  for i in 0..2 {
    ellipse dot{i} at {i * 10} 0 size 6 6 style card stroke none
  }
}
";
        let s = scene(src);
        // Detached: the line gets what the style gave, in the place of `style card`.
        let out = detach_style(src, &s, "a").unwrap();
        assert!(out.contains("rect a at 50 50 size 80 40 fill #ffffff stroke #1b1d24 2\n"), "{out}");
        let out = detach_style(src, &s, "b").unwrap();
        assert!(out.contains("rect b at 150 50 size 80 40 fill #ffd23f stroke #1b1d24 2 r 4\n"), "{out}");
        // A style that gives this kind nothing leaves no trace.
        let out = detach_style(src, &s, "c").unwrap();
        assert!(out.contains("rect c at 10 10 size 4 4\n"), "{out}");
        assert_eq!(scene(&out).doc, s.doc);
        // An object made by a loop has no line of its own.
        assert_eq!(detach_style(src, &s, "dot0"), None);
        assert_eq!(assign_style(src, &s, &["dot0".to_string()], "empty"), None);
        assert_eq!(make_style(src, &s, "dot0", "x"), None);

        // Deleted: every line that took the style, the one in the loop too, keeps its look.
        let out = delete_style(src, &s, "card").unwrap();
        assert!(!out.contains("card"), "{out}");
        assert!(out.contains("    ellipse dot{i} at {i * 10} 0 size 6 6 fill #ffffff stroke none\n"), "{out}");
        assert!(out.starts_with("artboard \"a\" 200x100\nstyle empty font-size 14\nrect a "), "{out}");
        let s2 = scene(&out);
        assert_eq!(s2.doc, s.doc);
        assert_eq!(s2.styles.len(), 1);
        assert_eq!(delete_style(&out, &s2, "card"), None);

        // Renamed: on its line and on every line that takes it.
        let out = rename_style(src, &s, "card", "плитка").unwrap();
        assert_eq!(out.matches("style плитка").count(), 4);
        assert!(!out.contains("style card"), "{out}");
        assert!(out.contains("// карточки"), "{out}");
        assert_eq!(scene(&out).doc, s.doc);
        assert_eq!(rename_style(src, &s, "card", "empty"), None);
        assert_eq!(rename_style(src, &s, "nothing", "x"), None);
    }

    #[test]
    fn an_object_refuses_a_property_of_its_style() {
        let src = "artboard \"a\" 200x100
style card fill #ffffff gradient linear 90 #fff #eee glow 12 #ffd34d dash 6 4 shadow 0 4 8 #00000033
rect a at 50 50 size 80 40 style card no glow no gradient
rect b at 150 50 size 80 40 style card
";
        let s = scene(src);
        let (a, b) = (s.doc.node("a").unwrap(), s.doc.node("b").unwrap());
        assert_eq!((a.glow.is_some(), a.gradient.is_some(), a.shadow.is_some(), a.dash.len()), (false, false, true, 2));
        assert_eq!((b.glow.is_some(), b.gradient.is_some()), (true, true));
        assert_eq!(own_look(&s, "a"), vec!["no glow", "no gradient"]);
        // The panel takes a property away: on an object with a style that is a refusal.
        let out = crate::set_style(src, &s, "b", "glow", None).unwrap();
        assert!(out.contains("rect b at 150 50 size 80 40 style card no glow\n"), "{out}");
        assert!(scene(&out).doc.node("b").unwrap().glow.is_none());
        // Its own property taken away shows the style again, so it turns into a refusal too.
        let own = src.replace("style card\n", "style card dash 2 2\n");
        let out = crate::set_style(&own, &scene(&own), "b", "dash", None).unwrap();
        assert!(out.contains("rect b at 150 50 size 80 40 style card no dash\n"), "{out}");
        // Written again, the property takes the place of the refusal.
        let out = crate::set_style(src, &s, "a", "glow", Some("glow 4 #ff0000")).unwrap();
        assert!(out.contains("style card glow 4 #ff0000 no gradient\n"), "{out}");
        let refused = src.replace("no glow no gradient", "no shadow");
        let out = set_prop(&refused, &scene(&refused), "a", Prop::ShadowBlur, Value::Num(2.0)).unwrap();
        assert!(out.contains("rect a at 50 50 size 80 40 style card shadow 0 0 2 #00000055\n"), "{out}");
        // An object without a style, or a property its style does not give: nothing to refuse.
        let plain = "artboard \"a\" 10x10\nstyle s fill #fff\nrect a size 4 4 glow 4 #f00\nrect b size 4 4 style s glow 4 #f00\n";
        let out = crate::set_style(plain, &scene(plain), "a", "glow", None).unwrap();
        assert!(out.contains("rect a size 4 4\n"), "{out}");
        let out = crate::set_style(plain, &scene(plain), "b", "glow", None).unwrap();
        assert!(out.contains("rect b size 4 4 style s\n"), "{out}");

        // Back to the style, one refusal or all.
        let out = reset_to_style(src, &s, "a", Some("no glow")).unwrap();
        assert!(out.contains("rect a at 50 50 size 80 40 style card no gradient\n"), "{out}");
        // Detached and deleted: what was refused is not written, and the refusals go.
        let out = detach_style(src, &s, "a").unwrap();
        assert!(out.contains("rect a at 50 50 size 80 40 fill #ffffff dash 6 4 shadow 0 4 8 #00000033\n"), "{out}");
        assert_eq!(scene(&out).doc, s.doc);
        let out = delete_style(src, &s, "card").unwrap();
        assert!(!out.contains("no ") && !out.contains("style"), "{out}");
        assert_eq!(scene(&out).doc, s.doc);
        // A new style from the object holds what the object shows.
        let out = make_style(src, &s, "a", "flat").unwrap();
        assert!(out.contains("style flat fill #ffffff dash 6 4 shadow 0 4 8 #00000033\n"), "{out}");
        assert!(out.contains("rect a at 50 50 size 80 40 style flat\n"), "{out}");
        // Written into the style, a refusal takes the property out of it for everyone.
        let both = src.replace("no glow no gradient", "fill #ff5a36 no glow no gradient");
        let out = write_into_style(&both, &scene(&both), "a").unwrap();
        assert!(out.contains("style card fill #ff5a36 dash 6 4 shadow 0 4 8 #00000033\n"), "{out}");
        assert!(out.contains("rect a at 50 50 size 80 40 style card\n"), "{out}");
        // Another style assigned: the refusals were about the old one.
        let two = format!("{src}style other fill #000000\n");
        let out = assign_style(&two, &scene(&two), &["a".to_string()], "other").unwrap();
        assert!(out.contains("rect a at 50 50 size 80 40 style other\n"), "{out}");

        let err = |src: &str| parse(src).unwrap_err().iter().map(|e| e.message.clone()).collect::<Vec<_>>();
        assert!(err("artboard \"a\" 10x10\nrect a size 4 4 no at\n")[0].contains("после no"));
        assert!(err("artboard \"a\" 10x10\nstyle s fill #fff no glow\n")[0].contains("no пишется на строке объекта"));
    }

    #[test]
    fn a_style_of_the_shared_head_is_renamed_and_deleted_on_every_board() {
        // `card` is shared. The second and the third board each have a style `note` of their
        // own: two styles that happen to share a name.
        let file = "// set
let ink = #1b1d24
style card fill #ffffff stroke ink 2
symbol Chip {
  rect body size 20 10 style card no stroke
}
artboard \"one\" 100x100
rect a at 50 50 size 40 40 style card
artboard \"two\" 100x100 cell 1 0
style note fill #fff4c2
rect b at 50 50 size 40 40 style card r 4
rect n at 50 80 size 40 10 style note
use Chip c at 20 20
artboard \"three\" 100x100 cell 2 0
style note fill #000000
rect d at 50 50 size 40 40 style note
";
        let docs = |text: &str| -> Vec<motif_core::Document> {
            crate::parse_boards(text, None).unwrap_or_else(|e| panic!("{e:?}\n{text}")).into_iter().map(|s| s.doc).collect()
        };
        let before = docs(file);
        // Renamed from the first board: the second follows, in the symbol of the head too.
        let out = rename_style_in_file(file, None, 0, "card", "plate").unwrap();
        assert!(out.contains("style plate fill #ffffff stroke ink 2\n"), "{out}");
        assert!(out.contains("rect body size 20 10 style plate no stroke\n"), "{out}");
        assert!(out.contains("rect a at 50 50 size 40 40 style plate\n"), "{out}");
        assert!(out.contains("rect b at 50 50 size 40 40 style plate r 4\n"), "{out}");
        assert!(!out.contains("card"), "{out}");
        assert_eq!(docs(&out), before);
        // The same from the second board; a name taken on one of the boards is refused.
        assert_eq!(rename_style_in_file(file, None, 1, "card", "plate").unwrap(), out);
        assert_eq!(rename_style_in_file(file, None, 0, "card", "note"), None);
        // A board's own style is renamed on that board only.
        let own = rename_style_in_file(file, None, 2, "note", "dark").unwrap();
        assert!(own.contains("style dark fill #000000\nrect d at 50 50 size 40 40 style dark\n"), "{own}");
        assert!(own.contains("style note fill #fff4c2\n") && own.contains("rect n at 50 80 size 40 10 style note\n"), "{own}");
        assert_eq!(rename_style_in_file(file, None, 0, "note", "dark"), None);

        // Deleted: every board keeps its look.
        let out = delete_style_in_file(file, None, 1, "card").unwrap();
        assert!(!out.contains("card"), "{out}");
        assert!(out.contains("rect body size 20 10 fill #ffffff\n"), "{out}");
        assert!(out.contains("rect a at 50 50 size 40 40 fill #ffffff stroke ink 2\n"), "{out}");
        assert!(out.contains("rect b at 50 50 size 40 40 fill #ffffff stroke ink 2 r 4\n"), "{out}");
        assert_eq!(docs(&out), before);
        let own = delete_style_in_file(file, None, 2, "note").unwrap();
        assert!(own.contains("rect d at 50 50 size 40 40 fill #000000\n") && own.contains("style note fill #fff4c2\n"), "{own}");
        assert_eq!(docs(&own), before);
        // A file of one board is its script.
        let one = "artboard \"a\" 100x100\nstyle card fill #ffffff\nrect a size 4 4 style card\n";
        assert_eq!(rename_style_in_file(one, None, 0, "card", "plate"), rename_style(one, &scene(one), "card", "plate"));
        assert_eq!(delete_style_in_file(one, None, 0, "card"), delete_style(one, &scene(one), "card"));
        assert_eq!(delete_style_in_file(one, None, 0, "none"), None);
    }

    #[test]
    fn a_style_is_replaced_by_another_everywhere() {
        let file = "let ink = #1b1d24
style card fill #ffffff stroke ink 2 shadow 0 2 4 #00000033
style flat fill #f0f0f0
symbol Spare {
  rect body size 4 4 style card
}
artboard \"one\" 100x100
rect a at 50 50 size 40 40 style card no shadow r 4
artboard \"two\" 100x100 cell 1 0
style own fill #000000
for i in 0..2 {
  rect b{i} at {i * 20} 50 size 10 10 style card fill #ffd23f
}
rect c at 50 80 size 40 10 style own
";
        let (out, lines) = replace_style(file, None, 0, "card", "flat").unwrap();
        // The line of the first board, the loop of the second and the symbol nobody placed.
        assert_eq!(lines, 3);
        // What was refused belonged to the old style; what the line writes itself stays.
        assert!(out.contains("rect a at 50 50 size 40 40 style flat r 4\n"), "{out}");
        assert!(out.contains("size 10 10 style flat fill #ffd23f\n") && out.contains("  rect body size 4 4 style flat\n"), "{out}");
        assert!(out.contains("style card fill #ffffff stroke ink 2 shadow 0 2 4 #00000033\n"), "{out}");
        let boards = crate::parse_boards(&out, None).unwrap();
        assert_eq!(boards[0].doc.node("a").unwrap().fill, Some(hex("#f0f0f0")));
        assert_eq!(boards[1].doc.node("b1").unwrap().fill, Some(hex("#ffd23f")));
        assert_eq!(boards[1].doc.node("b1").unwrap().stroke_width, 0.0);
        // A style one board keeps for itself cannot be taken on another.
        assert_eq!(replace_style(file, None, 1, "card", "own"), None);
        let (out, lines) = replace_style(file, None, 1, "own", "card").unwrap();
        assert!(lines == 1 && out.contains("rect c at 50 80 size 40 10 style card\n"), "{out}");
        assert_eq!(replace_style(file, None, 0, "flat", "card"), None);
        assert_eq!(replace_style(file, None, 0, "card", "card"), None);
        assert_eq!(replace_style(file, None, 0, "card", "gone"), None);
    }

    #[test]
    fn styles_follow_the_palette_and_stay_under_animation_and_overrides() {
        let src = "artboard \"a\" 200x100
let main = #2a6fdb
style chip fill main opacity 0.5
rect a at 50 50 size 20 20 style chip
anim Fade 1s {
  a.opacity 0: 1 | 1: 0
}
override {
  a.fill = #ff5a36
}
";
        let s = scene(src);
        let a = s.doc.node("a").unwrap();
        assert_eq!((a.fill, a.opacity), (Some(hex("#ff5a36")), 0.5));
        let anim = s.doc.animation("Fade").unwrap();
        assert_eq!(anim.pose(&s.doc, 0.0).node("a").unwrap().opacity, 1.0);
        let s = scene(&src.replace("  a.fill = #ff5a36\n", "").replace("#2a6fdb", "#4fd38c"));
        assert_eq!(s.doc.node("a").unwrap().fill, Some(hex("#4fd38c")));
        // The palette name stays known for the object (the web export writes it as a variable),
        // but the value has no place on the object's line.
        assert_eq!(s.refs.get(&("a".to_string(), Prop::Fill)).map(String::as_str), Some("main"));
        assert!(!s.sources.contains_key(&("a".to_string(), Prop::Fill)));
        let g = scene("artboard \"a\" 10x10\nlet main = #2a6fdb\nstyle sky gradient linear 90 main #ffffff\nrect a size 4 4 style sky\n");
        assert_eq!(g.gradient_refs.get(&("a".to_string(), false)), Some(&vec![Some("main".to_string()), None]));
        // Picking the same palette colour for a styled object writes its own property by name.
        let plain = src.replace("override {\n  a.fill = #ff5a36\n}\n", "");
        let out = crate::set_ref(&plain, &scene(&plain), "a", Prop::Fill, "main").unwrap();
        assert!(out.contains("rect a at 50 50 size 20 20 style chip fill main\n"), "{out}");
    }
}
