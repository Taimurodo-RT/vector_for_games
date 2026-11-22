//! Edits for the variants of a symbol: `variant main face=#ff5a36 hide icon` inside the symbol
//! is a named set of parameter values and of objects that are not shown, and `variant main` on
//! a `use` line makes the copy that variant. A value the copy writes itself is stronger.

use crate::edit::{fmt_value, splice, splice_all, whole_lines, with_space_before};
use crate::{Scene, Span, SymbolSite, VariantSite};
use motif_core::Value;

fn symbol<'a>(scene: &'a Scene, name: &str) -> Option<&'a SymbolSite> {
    scene.symbols.iter().find(|s| s.name == name)
}

fn variant<'a>(scene: &'a Scene, symbol_name: &str, name: &str) -> Option<&'a VariantSite> {
    symbol(scene, symbol_name)?.variants.iter().find(|v| v.name == name)
}

/// A name a variant can have: one word of letters, digits, `_` and `-` that starts with a letter.
pub fn variant_name_ok(name: &str) -> bool {
    let mut chars = name.chars();
    chars.next().is_some_and(|c| c.is_alphabetic() || c == '_') && chars.all(|c| c.is_alphanumeric() || c == '_' || c == '-') && name != "hide"
}

/// A variant name the symbol does not have yet: `variant1`, `variant2`…
pub fn free_variant_name(scene: &Scene, symbol_name: &str) -> String {
    let taken = |n: &str| variant(scene, symbol_name, n).is_some();
    (1..).map(|k| format!("variant{k}")).find(|n| !taken(n)).expect("names are unbounded")
}

/// What a variant line says, as written: the values by parameter and the hidden objects.
struct Written {
    values: Vec<(String, String)>,
    hide: Vec<String>,
}

impl Written {
    fn of(src: &str, v: &VariantSite) -> Self {
        Written {
            values: v.values.iter().map(|p| (p.name.clone(), src[p.span.start..p.span.end].to_string())).collect(),
            hide: v.hide.iter().map(|(id, _)| id.clone()).collect(),
        }
    }

    /// The line: values first, then what is hidden.
    fn line(&self, name: &str) -> String {
        let mut out = format!("variant {name}");
        for (param, value) in &self.values {
            out.push_str(&format!(" {param}={value}"));
        }
        if !self.hide.is_empty() {
            out.push_str(&format!(" hide {}", self.hide.join(" ")));
        }
        out
    }
}

/// Where a new `variant` line of the symbol goes (after its last variant, or first in the
/// block) and the text put before the line: the indent of the block.
fn new_line_place(src: &str, sym: &SymbolSite) -> Option<(usize, String)> {
    let after = match sym.variants.last() {
        Some(last) => whole_lines(src, last.span).end,
        None => sym.span.start + src[sym.span.start..sym.span.end].find('\n')? + 1,
    };
    // Indented as the line that follows is, when it is a line of the block.
    let next = src[after..sym.span.end].lines().next().unwrap_or("");
    let indent: String = next.chars().take_while(|c| *c == ' ' || *c == '\t').collect();
    Some((after, if next.trim_start().starts_with('}') || indent.is_empty() { "  ".to_string() } else { indent }))
}

/// Returns the script with a new, empty variant `name` of the symbol: a copy that takes it is
/// drawn as the symbol until the variant is given values. `None` when there is no such symbol
/// or the name is taken or is not one word.
pub fn add_variant(src: &str, scene: &Scene, symbol_name: &str, name: &str) -> Option<String> {
    let sym = symbol(scene, symbol_name)?;
    if !variant_name_ok(name) || sym.variants.iter().any(|v| v.name == name) {
        return None;
    }
    let (at, indent) = new_line_place(src, sym)?;
    Some(splice(src, Span::new(at, at), &format!("{indent}variant {name}\n")))
}

/// Returns the script with copy `copy` being variant `name` of its symbol, or with the copy
/// drawn as the symbol itself again (`None`). `None` when `copy` is not a copy, its line is
/// made by a loop, the symbol has no such variant, or nothing changes.
pub fn set_variant(src: &str, scene: &Scene, copy: &str, name: Option<&str>) -> Option<String> {
    let inst = scene.instances.get(copy)?;
    let site = scene.sites.get(copy).filter(|s| !s.generated)?;
    if let Some(name) = name {
        variant(scene, &inst.symbol, name)?;
    }
    match (&inst.variant, name) {
        (Some((old, _)), Some(new)) if old == new => None,
        (Some((_, span)), Some(new)) => Some(splice(src, *span, &format!("variant {new}"))),
        (Some((_, span)), None) => Some(splice(src, with_space_before(src, *span), "")),
        (None, Some(new)) => Some(splice(src, Span::new(site.insert_at, site.insert_at), &format!(" variant {new}"))),
        (None, None) => None,
    }
}

/// Returns the script with the look of copy `copy` kept as a new variant `name` of its symbol:
/// the parameters the copy writes on its line move into the variant (over the values of the
/// variant the copy was, which the new one keeps, with what that one hides), and the copy
/// becomes the new variant. It looks the same; other copies can now take the same look.
/// `None` when `copy` is not a copy written by hand or the name is taken or is not one word.
pub fn variant_from_copy(src: &str, scene: &Scene, copy: &str, name: &str) -> Option<String> {
    let inst = scene.instances.get(copy)?;
    let site = scene.sites.get(copy).filter(|s| !s.generated)?;
    let sym = symbol(scene, &inst.symbol)?;
    if !variant_name_ok(name) || sym.variants.iter().any(|v| v.name == name) {
        return None;
    }
    let mut written = match inst.variant.as_ref().and_then(|(old, _)| variant(scene, &inst.symbol, old)) {
        Some(old) => Written::of(src, old),
        None => Written { values: Vec::new(), hide: Vec::new() },
    };
    let own: Vec<crate::CopyChange> = crate::copy_changes(src, scene, copy).into_iter().filter(|c| c.part.is_none()).collect();
    let mut edits: Vec<(Span, String)> = Vec::new();
    for c in &own {
        match written.values.iter_mut().find(|(param, _)| *param == c.name) {
            Some((_, value)) => value.clone_from(&c.value),
            None => written.values.push((c.name.clone(), c.value.clone())),
        }
        edits.push((c.span, String::new()));
    }
    let (at, indent) = new_line_place(src, sym)?;
    edits.push((Span::new(at, at), format!("{indent}{}\n", written.line(name))));
    match &inst.variant {
        Some((_, span)) => edits.push((*span, format!("variant {name}"))),
        // After the parameters that leave the line: an insertion at their end comes first.
        None => edits.push((Span::new(site.insert_at, site.insert_at), format!(" variant {name}"))),
    }
    // A parameter that ends where the new word goes: the word is written in its place.
    if inst.variant.is_none()
        && let Some(k) = edits.iter().position(|(span, text)| text.is_empty() && span.end == site.insert_at)
    {
        edits.pop();
        edits[k].1 = format!(" variant {name}");
    }
    Some(splice_all(src, edits))
}

/// Returns the script with variant `name` of the symbol setting parameter `param` to `value`,
/// or not setting it any more (`None`): copies of the variant then get the default again.
/// `None` when there is no such variant or parameter, or nothing changes.
pub fn set_variant_value(src: &str, scene: &Scene, symbol_name: &str, name: &str, param: &str, value: Option<Value>) -> Option<String> {
    let sym = symbol(scene, symbol_name)?;
    let v = variant(scene, symbol_name, name)?;
    sym.params.iter().find(|(p, _)| p == param)?;
    let mut written = Written::of(src, v);
    let at = written.values.iter().position(|(p, _)| p == param);
    match (at, value) {
        (Some(k), Some(value)) => written.values[k].1 = fmt_value(value),
        (None, Some(value)) => written.values.push((param.to_string(), fmt_value(value))),
        (Some(k), None) => {
            written.values.remove(k);
        }
        (None, None) => return None,
    }
    let out = splice(src, v.span, &written.line(name));
    (out != src).then_some(out)
}

/// Returns the script with variant `name` of the symbol hiding object `part` (its id inside
/// the symbol, `icon`), or showing it again. `None` when there is no such variant, or nothing
/// changes.
pub fn set_variant_hidden(src: &str, scene: &Scene, symbol_name: &str, name: &str, part: &str, hidden: bool) -> Option<String> {
    let v = variant(scene, symbol_name, name)?;
    let mut written = Written::of(src, v);
    let at = written.hide.iter().position(|id| id == part);
    match (at, hidden) {
        (None, true) => written.hide.push(part.to_string()),
        (Some(k), false) => {
            written.hide.remove(k);
        }
        _ => return None,
    }
    Some(splice(src, v.span, &written.line(name)))
}

/// The lines of the copies that are variant `name` of the symbol: where `variant name` is
/// written. A line of a loop or of another symbol is one line, however many copies it makes.
fn takers(scene: &Scene, symbol_name: &str, name: &str) -> Vec<Span> {
    let mut spans: Vec<Span> = scene
        .instances
        .values()
        .filter(|i| i.symbol == symbol_name)
        .filter_map(|i| i.variant.as_ref())
        .filter(|(v, _)| v == name)
        .map(|(_, span)| *span)
        .collect();
    spans.sort_by_key(|s| s.start);
    spans.dedup();
    spans
}

/// An edit of variant `name` of symbol `symbol_name` of board `board` made in the whole file:
/// a symbol of the shared head has copies on every board. `edits` gives the changes of one
/// board's script. `None` when a board has errors or the board has no such variant.
fn in_file(
    file: &str,
    base: Option<&std::path::Path>,
    board: usize,
    symbol_name: &str,
    name: &str,
    edits: impl Fn(&str, &Scene, &VariantSite) -> Vec<(Span, String)>,
) -> Option<String> {
    let views = crate::boards::board_views(file, base)?;
    let line = |v: &crate::boards::BoardView| variant(&v.scene, symbol_name, name).map(|s| v.in_file(s.span));
    let which = line(views.get(board)?)?;
    let mut all: Vec<(Span, String)> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    // A board with a symbol of its own under the same name has another variant: it stays.
    for v in views.iter().filter(|v| line(v) == Some(which)) {
        for (span, text) in edits(&v.text, &v.scene, variant(&v.scene, symbol_name, name)?) {
            let span = v.in_file(span);
            if seen.insert((span.start, span.end)) {
                all.push((span, text));
            }
        }
    }
    let out = splice_all(file, all);
    crate::parse_boards(&out, base).ok()?;
    Some(out)
}

/// Returns the file with variant `old` of the symbol called `new`, at the symbol and at every
/// copy that takes it, on every board. `None` when a board has errors, there is no such
/// variant, or `new` is taken or is not one word.
pub fn rename_variant(file: &str, base: Option<&std::path::Path>, board: usize, symbol_name: &str, old: &str, new: &str) -> Option<String> {
    if !variant_name_ok(new) || old == new {
        return None;
    }
    in_file(file, base, board, symbol_name, old, |_, scene, v| {
        let mut edits = vec![(v.name_span, new.to_string())];
        edits.extend(takers(scene, symbol_name, old).into_iter().map(|span| (span, format!("variant {new}"))));
        edits
    })
}

/// Returns the file without variant `name` of the symbol: its line goes, and every copy that
/// took it, on every board, is drawn as the symbol itself again (what the copy writes on its
/// own line stays). `None` when a board has errors or there is no such variant.
pub fn delete_variant(file: &str, base: Option<&std::path::Path>, board: usize, symbol_name: &str, name: &str) -> Option<String> {
    in_file(file, base, board, symbol_name, name, |src, scene, v| {
        let mut edits = vec![(whole_lines(src, v.span), String::new())];
        edits.extend(takers(scene, symbol_name, name).into_iter().map(|span| (with_space_before(src, span), String::new())));
        edits
    })
}

/// What leaves the `hide` lists of the symbol's variants when objects of the symbol are
/// deleted: `gone` tells by the id inside the symbol. A list that loses every object loses
/// its `hide` word too.
pub(crate) fn hide_spans(src: &str, sym: &SymbolSite, gone: impl Fn(&str) -> bool) -> Vec<Span> {
    let mut out = Vec::new();
    for v in &sym.variants {
        // One list is the ids that follow one `hide` word.
        let mut k = 0;
        while k < v.hide.len() {
            let mut end = k + 1;
            while end < v.hide.len() && src[v.hide[end - 1].1.end..v.hide[end].1.start].trim().is_empty() {
                end += 1;
            }
            let list = &v.hide[k..end];
            if list.iter().all(|(id, _)| gone(id)) {
                let word = src[..list[0].1.start].trim_end().len().saturating_sub("hide".len());
                out.push(with_space_before(src, Span::new(word, list[list.len() - 1].1.end)));
            } else {
                let mut kept_before = false;
                for (j, (id, span)) in list.iter().enumerate() {
                    if !gone(id) {
                        kept_before = true;
                    } else if kept_before {
                        out.push(with_space_before(src, *span));
                    } else {
                        out.push(Span::new(span.start, list[j + 1].1.start));
                    }
                }
            }
            k = end;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Reading, parse, parse_as, parse_boards};
    use motif_core::Color;

    const BUTTONS: &str = "\
artboard \"Кнопки\" 320x200 bg #ffffff
let accent = #ff5a36
symbol Button w=120 face=#2d6cdf ink=#ffffff {
  variant main face=accent
  variant icon w=44 hide label
  rect bg size {w} 36 r 8 fill face
  text label content \"OK\" font-size 16 fill ink
  ellipse glyph size 12 12 fill ink
}
use Button b1 at 80 40
use Button b2 at 80 90 variant main
use Button b3 at 80 140 variant icon
use Button b4 at 220 140 variant icon w=60 face=#000000
";

    fn fill(scene: &Scene, id: &str) -> String {
        scene.doc.node(id).unwrap().fill.unwrap().to_hex()
    }

    fn width(scene: &Scene, id: &str) -> f64 {
        scene.doc.node(id).unwrap().width
    }

    #[test]
    fn a_copy_takes_the_values_and_the_hidden_objects_of_its_variant() {
        let scene = parse(BUTTONS).unwrap();
        // As the symbol is drawn.
        assert_eq!((fill(&scene, "b1-bg").as_str(), width(&scene, "b1-bg")), ("#2d6cdf", 120.0));
        assert!(scene.doc.node("b1-label").unwrap().visible);
        assert_eq!(scene.instances["b1"].variant, None);
        // The value of a variant can be a name of the palette.
        assert_eq!((fill(&scene, "b2-bg").as_str(), width(&scene, "b2-bg")), ("#ff5a36", 120.0));
        assert_eq!(scene.instances["b2"].variant.as_ref().map(|v| v.0.as_str()), Some("main"));
        let face = scene.instances["b2"].params.iter().find(|p| p.name == "face").unwrap();
        assert!(face.variant && face.span.is_none());
        // A variant hides objects of the symbol.
        assert_eq!(width(&scene, "b3-bg"), 44.0);
        assert!(!scene.doc.node("b3-label").unwrap().visible && scene.doc.node("b3-glyph").unwrap().visible);
        // What the copy writes itself is stronger than its variant.
        assert_eq!((fill(&scene, "b4-bg").as_str(), width(&scene, "b4-bg")), ("#000000", 60.0));
        assert!(!scene.doc.node("b4-label").unwrap().visible);
        // The symbol knows its variants.
        let sym = &scene.symbols[0];
        assert_eq!(sym.variants.iter().map(|v| v.name.as_str()).collect::<Vec<_>>(), ["main", "icon"]);
        assert_eq!(&BUTTONS[sym.variants[1].span.start..sym.variants[1].span.end], "variant icon w=44 hide label");
        assert_eq!(sym.variants[0].values[0].value, Some(Value::Color(Color { r: 0xff, g: 0x5a, b: 0x36, a: 255 })), "read where the palette is known");
        assert_eq!(sym.variants[1].values[0].value, Some(Value::Num(44.0)));
    }

    #[test]
    fn a_theme_changes_the_value_a_variant_takes_from_the_palette() {
        let src = format!("{BUTTONS}theme night {{\n  accent = #112233\n}}\n");
        let night = parse_as(&src, None, Reading { theme: Some("night"), symbol_of: None }).unwrap();
        assert_eq!(fill(&night, "b2-bg"), "#112233");
        assert_eq!(fill(&parse(&src).unwrap(), "b2-bg"), "#ff5a36");
    }

    #[test]
    fn mistakes_in_variants_are_told_once_with_the_names_there_are() {
        let told = |src: &str| parse(src).unwrap_err().iter().map(|e| e.message.clone()).collect::<Vec<_>>();
        let e = told(&BUTTONS.replace("variant main\n", "variant primary\n"));
        assert_eq!(e, ["у символа «Button» нет варианта «primary»: есть: main, icon"]);
        let e = told(&BUTTONS.replace("variant icon w=44", "variant icon width=44"));
        assert_eq!(e, ["у символа «Button» нет параметра «width»: есть: w, face, ink"]);
        // A wrong value is told once, though four copies read it.
        let e = told(&BUTTONS.replace("face=accent", "face=acent"));
        assert_eq!(e, ["неизвестное имя «acent»"]);
        // An object that is not there is told when a copy takes the variant, once.
        let e = told(&BUTTONS.replace("hide label", "hide lable"));
        assert_eq!(e, ["в символе «Button» нет объекта «lable»: вариант не может его скрыть"]);
        let e = told(&BUTTONS.replace("variant icon w=44", "variant main w=44"));
        assert!(e[0].contains("уже есть вариант «main»"), "{e:?}");
        // The line belongs to a symbol, the word to a `use` line.
        let e = told(&format!("{BUTTONS}variant big w=200\n"));
        assert!(e[0].contains("внутри symbol"), "{e:?}");
        let e = told(&format!("{BUTTONS}rect r size 4 variant main\n"));
        assert!(e[0].contains("в строке use"), "{e:?}");
        let e = told(&BUTTONS.replace("hide label", "hide"));
        assert!(e[0].contains("после hide"), "{e:?}");
    }

    #[test]
    fn the_copy_a_symbol_is_edited_through_shows_all_of_the_symbol() {
        let through = parse_as(BUTTONS, None, Reading { theme: None, symbol_of: Some("b3") }).unwrap();
        assert!(through.doc.node("b3-label").unwrap().visible, "the hidden object can be reached and edited");
        assert_eq!(width(&through, "b3-bg"), 44.0, "the values of the variant stay");
        assert!(!through.doc.node("b4-label").unwrap().visible);
    }

    #[test]
    fn a_copy_is_made_a_variant_and_the_symbol_itself_again() {
        let scene = parse(BUTTONS).unwrap();
        let out = set_variant(BUTTONS, &scene, "b1", Some("icon")).unwrap();
        assert!(out.contains("use Button b1 at 80 40 variant icon\n"), "{out}");
        let out = set_variant(BUTTONS, &scene, "b2", Some("icon")).unwrap();
        assert!(out.contains("use Button b2 at 80 90 variant icon\n"), "{out}");
        let out = set_variant(BUTTONS, &scene, "b4", None).unwrap();
        assert!(out.contains("use Button b4 at 220 140 w=60 face=#000000\n"), "{out}");
        assert_eq!(set_variant(BUTTONS, &scene, "b2", Some("main")), None);
        assert_eq!(set_variant(BUTTONS, &scene, "b1", None), None);
        assert_eq!(set_variant(BUTTONS, &scene, "b1", Some("nope")), None);
        // A new parameter of the copy is written after the word and stays the copy's own.
        let out = crate::set_param(BUTTONS, &scene, "b3", "w", Value::Num(50.0)).unwrap();
        assert!(out.contains("use Button b3 at 80 140 variant icon w=50\n"), "{out}");
        assert_eq!(width(&parse(&out).unwrap(), "b3-bg"), 50.0);
    }

    #[test]
    fn a_variant_is_added_changed_and_emptied() {
        let scene = parse(BUTTONS).unwrap();
        let out = add_variant(BUTTONS, &scene, "Button", "ghost").unwrap();
        assert!(out.contains("  variant icon w=44 hide label\n  variant ghost\n  rect bg"), "{out}");
        assert_eq!(add_variant(BUTTONS, &scene, "Button", "main"), None);
        assert_eq!(add_variant(BUTTONS, &scene, "Button", "two words"), None);
        assert_eq!(free_variant_name(&scene, "Button"), "variant1");
        // The first variant of a symbol goes first in its block.
        let plain = "artboard \"a\" 10x10\nsymbol Dot {\n  ellipse d size 4 4\n}\nuse Dot d1\n";
        let out = add_variant(plain, &parse(plain).unwrap(), "Dot", "big").unwrap();
        assert!(out.contains("symbol Dot {\n  variant big\n  ellipse d"), "{out}");

        let out = set_variant_value(BUTTONS, &scene, "Button", "main", "w", Some(Value::Num(160.0))).unwrap();
        assert!(out.contains("  variant main face=accent w=160\n"), "{out}");
        let out = set_variant_value(BUTTONS, &scene, "Button", "main", "face", Some(Value::Color(Color { r: 1, g: 2, b: 3, a: 255 }))).unwrap();
        assert!(out.contains("  variant main face=#010203\n"), "{out}");
        let out = set_variant_value(BUTTONS, &scene, "Button", "icon", "w", None).unwrap();
        assert!(out.contains("  variant icon hide label\n"), "{out}");
        assert_eq!(width(&parse(&out).unwrap(), "b3-bg"), 120.0);
        assert_eq!(set_variant_value(BUTTONS, &scene, "Button", "main", "nope", Some(Value::Num(1.0))), None);
        assert_eq!(set_variant_value(BUTTONS, &scene, "Button", "main", "w", None), None);

        let out = set_variant_hidden(BUTTONS, &scene, "Button", "icon", "glyph", true).unwrap();
        assert!(out.contains("  variant icon w=44 hide label glyph\n"), "{out}");
        let out = set_variant_hidden(BUTTONS, &scene, "Button", "main", "glyph", true).unwrap();
        assert!(out.contains("  variant main face=accent hide glyph\n"), "{out}");
        assert!(!parse(&out).unwrap().doc.node("b2-glyph").unwrap().visible);
        let out = set_variant_hidden(BUTTONS, &scene, "Button", "icon", "label", false).unwrap();
        assert!(out.contains("  variant icon w=44\n"), "{out}");
        assert_eq!(set_variant_hidden(BUTTONS, &scene, "Button", "icon", "label", true), None);
    }

    #[test]
    fn the_look_of_a_copy_is_kept_as_a_new_variant() {
        let scene = parse(BUTTONS).unwrap();
        // The copy was a variant: the new one keeps its values and what it hides.
        let out = variant_from_copy(BUTTONS, &scene, "b4", "dark").unwrap();
        assert!(out.contains("  variant icon w=44 hide label\n  variant dark w=60 face=#000000 hide label\n"), "{out}");
        assert!(out.contains("use Button b4 at 220 140 variant dark\n"), "{out}");
        let after = parse(&out).unwrap();
        assert_eq!(after.doc, scene.doc, "the copy looks the same");
        // A copy with nothing of its own gives an empty variant.
        let out = variant_from_copy(BUTTONS, &scene, "b1", "plain").unwrap();
        assert!(out.contains("  variant plain\n") && out.contains("use Button b1 at 80 40 variant plain\n"), "{out}");
        assert_eq!(parse(&out).unwrap().doc, scene.doc);
        // A copy that only writes parameters.
        let src = BUTTONS.replace("use Button b1 at 80 40\n", "use Button b1 at 80 40 w=90 opacity 0.5 ink=#111111\n");
        let before = parse(&src).unwrap();
        let out = variant_from_copy(&src, &before, "b1", "small").unwrap();
        assert!(out.contains("  variant small w=90 ink=#111111\n"), "{out}");
        assert!(out.contains("use Button b1 at 80 40 opacity 0.5 variant small\n"), "{out}");
        assert_eq!(parse(&out).unwrap().doc, before.doc);
        assert_eq!(variant_from_copy(BUTTONS, &scene, "b1", "main"), None);
    }

    #[test]
    fn an_object_deleted_from_the_symbol_leaves_the_lists_of_hidden_ones() {
        let src = BUTTONS
            .replace("variant main face=accent", "variant main face=accent hide label glyph")
            .replace("hide label\n", "hide glyph ink=#111111 hide label\n");
        let through = |src: &str| parse_as(src, None, Reading { theme: None, symbol_of: Some("b1") }).unwrap();
        let (out, ..) = crate::delete_nodes(&src, &through(&src), &["b1-label".to_string()], None).unwrap();
        assert!(out.contains("  variant main face=accent hide glyph\n  variant icon w=44 hide glyph ink=#111111\n  rect bg"), "{out}");
        let (out, ..) = crate::delete_nodes(&src, &through(&src), &["b1-glyph".to_string()], None).unwrap();
        assert!(out.contains("  variant main face=accent hide label\n  variant icon w=44 ink=#111111 hide label\n"), "{out}");
        let (out, ..) = crate::delete_nodes(&src, &through(&src), &["b1-label".to_string(), "b1-glyph".to_string()], None).unwrap();
        assert!(out.contains("  variant main face=accent\n  variant icon w=44 ink=#111111\n  rect bg"), "{out}");
    }

    #[test]
    fn a_variant_is_renamed_and_removed_on_every_board() {
        let file = "\
symbol Tag w=20 {
  variant wide w=60
  rect bg size {w} 10 fill #333333
}
artboard \"Первый\" 100x100
use Tag t1 at 10 10 variant wide
for i in 0..2 {
  use Tag r{i} at {i*30} 50 variant wide
}
artboard \"Второй\" 100x100
use Tag t2 at 10 10 variant wide w=70
use Tag t3 at 10 30
";
        let out = rename_variant(file, None, 0, "Tag", "wide", "long").unwrap();
        assert_eq!(out, file.replace("wide", "long"));
        assert_eq!(rename_variant(file, None, 0, "Tag", "wide", "not a word"), None);
        assert_eq!(rename_variant(file, None, 0, "Tag", "nope", "long"), None);

        let out = delete_variant(file, None, 1, "Tag", "wide").unwrap();
        assert_eq!(out, file.replace("  variant wide w=60\n", "").replace(" variant wide", ""));
        let boards = parse_boards(&out, None).unwrap();
        assert_eq!(boards[0].doc.node("t1-bg").unwrap().width, 20.0);
        assert_eq!(boards[1].doc.node("t2-bg").unwrap().width, 70.0, "what the copy writes itself stays");
    }
}
