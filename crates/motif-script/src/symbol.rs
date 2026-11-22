//! Edits for symbols: placing a copy, changing a copy's parameter, turning a group into a symbol.

use crate::edit::{fmt_value, free_id, insert_declaration, splice, splice_all, whole_lines};
use crate::{Scene, Span, fmt_num};
use motif_core::{ShapeKind, Value};

/// The id that object `node` of the scene has inside the symbol of copy `copy`: `coin1-body` is
/// `body`. `None` when `node` is not an object of that copy; an object beside the copy whose id
/// only begins the same way (`coin1-shadow` on the artboard) is not one.
pub fn part_of<'a>(scene: &Scene, copy: &str, node: &'a str) -> Option<&'a str> {
    let doc = &scene.doc;
    let (c, i) = (doc.index_of(copy)?, doc.index_of(node)?);
    node.strip_prefix(copy)?.strip_prefix('-').filter(|rest| !rest.is_empty() && doc.is_inside(i, c))
}

/// [`part_of`] for the copy through which a symbol is being edited ([`Scene::editing`]).
pub(crate) fn edited_part<'a>(scene: &Scene, node: &'a str) -> Option<&'a str> {
    part_of(scene, scene.editing.as_deref()?, node)
}

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

/// One thing a copy has of its own, apart from its symbol: a parameter written on its `use` line
/// or a property of one of its objects written in `override { }`.
#[derive(Clone, Debug, PartialEq)]
pub struct CopyChange {
    /// The object of the copy by its id inside the symbol (`body`); `None` for a parameter.
    pub part: Option<String>,
    /// The parameter, or the property as the script names it (`x`, `fill`).
    pub name: String,
    /// The value as written.
    pub value: String,
    /// What leaves the script when the change is taken back.
    pub(crate) span: Span,
}

/// What copy `copy` has of its own, in the order written: its parameters first. A copy made by a
/// loop shares its line with the other copies of the loop, so its parameters are not listed.
pub fn copy_changes(src: &str, scene: &Scene, copy: &str) -> Vec<CopyChange> {
    let Some(inst) = scene.instances.get(copy) else { return Vec::new() };
    let mut out = Vec::new();
    if scene.sites.get(copy).is_some_and(|s| !s.generated) {
        for p in &inst.params {
            let Some(value) = p.span else { continue };
            // ` size=24`: the name, the sign and the space before them go with the value.
            let before = src[..value.start].trim_end_matches(|c: char| c.is_whitespace() && c != '\n');
            let Some(name) = before.strip_suffix('=').map(|b| b.trim_end_matches(' ')).filter(|b| b.ends_with(p.name.as_str())) else { continue };
            let start = src[..name.len() - p.name.len()].trim_end_matches(' ').len();
            out.push(CopyChange { part: None, name: p.name.clone(), value: src[value.start..value.end].to_string(), span: Span::new(start, value.end) });
        }
    }
    for (node, span) in &scene.override_entries {
        let Some(part) = part_of(scene, copy, node) else { continue };
        // `coin1-body.x = 12`
        let Some((prop, value)) = src[span.start..span.end].get(node.len() + 1..).and_then(|rest| rest.split_once('=')) else { continue };
        out.push(CopyChange { part: Some(part.to_string()), name: prop.trim().to_string(), value: value.trim().to_string(), span: whole_lines(src, *span) });
    }
    out
}

/// Returns the script with one change of copy `copy` taken back (`which` counts
/// [`copy_changes`]), or with all of them: the copy is drawn as its symbol says again. `None`
/// when there is nothing to take back.
pub fn reset_copy(src: &str, scene: &Scene, copy: &str, which: Option<usize>) -> Option<String> {
    let changes = copy_changes(src, scene, copy);
    let edits: Vec<(Span, String)> = match which {
        Some(k) => vec![(changes.get(k)?.span, String::new())],
        None => changes.iter().map(|c| (c.span, String::new())).collect(),
    };
    (!edits.is_empty()).then(|| splice_all(src, edits))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Place, Reading, copy_nodes, delete_nodes, group_nodes, move_nodes, parse, parse_as, paste_nodes_into, restack, set_prop};
    use motif_core::{Color, Prop};

    const COINS: &str = "\
artboard \"Монеты\" 200x120 bg #ffffff
symbol Coin size=24 color=#ffd23f {
  ellipse body size {size} {size} fill color stroke #1b1d24 2
  rect shine at -4 -4 size 6 6 fill #ffffff
}
use Coin c1 at 40 60
use Coin c2 at 120 60 size=30
rect floor at 100 110 size 200 10 fill #888888
anim Spin 1s loop {
  c1-shine.x 0: -4 | 1: 4
  c2-shine.x 0: -4 | 1: 4
}
override {
  c2-shine.fill = #ff0000
}
";

    fn through(src: &str, copy: &str) -> Scene {
        parse_as(src, None, Reading { theme: None, symbol_of: Some(copy), vary_seed: 0 }).unwrap()
    }

    fn hex(scene: &Scene, id: &str) -> String {
        scene.doc.node(id).unwrap().fill.unwrap().to_hex()
    }

    #[test]
    fn an_edit_through_a_copy_changes_the_symbol_and_every_copy() {
        // Read as usual, an object of a copy is changed for that copy alone.
        let plain = parse(COINS).unwrap();
        assert_eq!(plain.editing, None);
        let own = set_prop(COINS, &plain, "c1-shine", Prop::Y, Value::Num(-6.0)).unwrap();
        assert!(own.contains("  c1-shine.y = -6\n") && own.contains("rect shine at -4 -4"), "{own}");

        // Read for the edit of the symbol, the same call writes into the line of the symbol.
        let scene = through(COINS, "c1");
        assert_eq!(scene.editing.as_deref(), Some("c1"));
        assert_eq!(scene.doc, plain.doc, "the scene looks the same: only the place of an edit differs");
        let out = set_prop(COINS, &scene, "c1-shine", Prop::Y, Value::Num(-6.0)).unwrap();
        assert!(out.contains("  rect shine at -4 -6 size 6 6 fill #ffffff\n") && !out.contains("c1-shine.y"), "{out}");
        let after = parse(&out).unwrap();
        assert_eq!((after.doc.node("c1-shine").unwrap().y, after.doc.node("c2-shine").unwrap().y), (-6.0, -6.0));

        // A property the line does not have yet is added to it.
        let out = set_prop(COINS, &scene, "c1-shine", Prop::Rotation, Value::Num(45.0)).unwrap();
        assert!(out.contains("  rect shine at -4 -4 size 6 6 fill #ffffff rot 45\n"), "{out}");
        // A copy the script does not have changes nothing.
        assert_eq!(through(COINS, "c9").editing, None);

        // An object beside the copy is not its object, whatever its id begins with.
        let beside = format!("{COINS}rect c1-shadow at 40 90 size 30 6 fill #000000\n");
        let scene = through(&beside, "c1");
        assert_eq!((part_of(&scene, "c1", "c1-shine"), part_of(&scene, "c1", "c1-shadow")), (Some("shine"), None));
        let (out, _, _) = delete_nodes(&beside, &scene, &["c1-shadow".to_string()], None).unwrap();
        assert!(!out.contains("shadow") && parse(&out).unwrap().doc == parse(COINS).unwrap().doc, "{out}");
    }

    #[test]
    fn the_copy_shows_the_symbol_while_it_is_edited() {
        // `c2` paints its shine red for itself. While the symbol is edited through it, the shine
        // is drawn as the symbol says, and a new fill goes into the symbol.
        assert_eq!(hex(&parse(COINS).unwrap(), "c2-shine"), "#ff0000");
        let scene = through(COINS, "c2");
        assert_eq!(hex(&scene, "c2-shine"), "#ffffff");
        assert_eq!(hex(&scene, "c1-shine"), "#ffffff");
        let out = set_prop(COINS, &scene, "c2-shine", Prop::Fill, Value::Color(Color::from_hex("#00ff00").unwrap())).unwrap();
        assert!(out.contains("fill #00ff00\n") && out.contains("  c2-shine.fill = #ff0000\n"), "{out}");
        // Out of the symbol the copy has its own color again, the other copy has the new one.
        let after = parse(&out).unwrap();
        assert_eq!((hex(&after, "c1-shine"), hex(&after, "c2-shine")), ("#00ff00".to_string(), "#ff0000".to_string()));
    }

    #[test]
    fn a_value_from_a_parameter_is_not_replaced_in_the_symbol() {
        // The size and the fill of the body come from the parameters. An edit of them cannot go
        // into the line of the symbol; it is written for the copy, which the editor refuses
        // while the symbol is edited.
        let scene = through(COINS, "c1");
        let own = |s: &Scene| s.override_entries.iter().filter(|(node, _)| part_of(s, "c1", node).is_some()).count();
        for (prop, value, entry) in [
            (Prop::Width, Value::Num(40.0), "  c1-body.w = 40\n"),
            (Prop::Fill, Value::Color(Color::from_hex("#3b5bff").unwrap()), "  c1-body.fill = #3b5bff\n"),
        ] {
            let out = set_prop(COINS, &scene, "c1-body", prop, value).unwrap();
            assert!(out.contains(entry) && out.contains("ellipse body size {size} {size} fill color stroke"), "{out}");
            assert_eq!((own(&scene), own(&through(&out, "c1"))), (0, 1));
        }
    }

    #[test]
    fn what_a_copy_has_of_its_own_is_listed_and_taken_back() {
        let scene = parse(COINS).unwrap();
        assert!(copy_changes(COINS, &scene, "c1").is_empty());
        let list = copy_changes(COINS, &scene, "c2");
        let shown: Vec<(Option<&str>, &str, &str)> = list.iter().map(|c| (c.part.as_deref(), c.name.as_str(), c.value.as_str())).collect();
        assert_eq!(shown, [(None, "size", "30"), (Some("shine"), "fill", "#ff0000")]);
        // The list is the same while the symbol is edited through the copy.
        assert_eq!(copy_changes(COINS, &through(COINS, "c2"), "c2"), list);

        let out = reset_copy(COINS, &scene, "c2", Some(0)).unwrap();
        assert!(out.contains("use Coin c2 at 120 60\n") && out.contains("c2-shine.fill"), "{out}");
        assert_eq!(parse(&out).unwrap().doc.node("c2-body").unwrap().width, 24.0);
        let out = reset_copy(COINS, &scene, "c2", Some(1)).unwrap();
        assert!(out.contains("size=30") && !out.contains("c2-shine.fill"), "{out}");
        let out = reset_copy(COINS, &scene, "c2", None).unwrap();
        let after = parse(&out).unwrap();
        assert!(copy_changes(&out, &after, "c2").is_empty());
        assert_eq!(after.doc.node("c2").unwrap().x, 120.0);
        assert_eq!(reset_copy(COINS, &scene, "c1", None), None);
        assert_eq!(reset_copy(COINS, &scene, "c2", Some(5)), None);
    }

    #[test]
    fn a_new_object_goes_into_the_symbol_and_stays_where_it_was_drawn() {
        let scene = through(COINS, "c1");
        let out = move_nodes(COINS, &scene, &["floor".to_string()], "c1", Place::Into, None).unwrap();
        assert!(out.contains("  rect shine at -4 -4 size 6 6 fill #ffffff\n  rect floor at 60 50 size 200 10 fill #888888\n}\n"), "{out}");
        let after = parse(&out).unwrap();
        let world = |id: &str| {
            let doc = &after.doc;
            doc.world_transform(doc.index_of(id).unwrap()).translation()
        };
        assert_eq!((world("c1-floor").x, world("c1-floor").y), (100.0, 110.0));
        assert_eq!((world("c2-floor").x, world("c2-floor").y), (180.0, 110.0));
        // Read as usual, a copy takes nothing in: it has no block of its own.
        assert!(move_nodes(COINS, &parse(COINS).unwrap(), &["floor".to_string()], "c1", Place::Into, None).is_err());

        // Through a copy drawn twice as large the object is written half as large, so it stays as
        // drawn; what does not change (the rotation, the skew) is not written.
        let big = COINS.replace("use Coin c1 at 40 60\n", "use Coin c1 at 40 60 scale 2\n");
        let out = move_nodes(&big, &through(&big, "c1"), &["floor".to_string()], "c1", Place::Into, None).unwrap();
        assert!(out.contains("  rect floor at 30 25 size 200 10 fill #888888 scale 0.5 0.5\n}\n"), "{out}");
    }

    #[test]
    fn the_points_of_a_contour_and_the_objects_of_a_group_are_edited_in_the_symbol() {
        let src = "\
artboard \"a\" 200x120
symbol Leaf {
  path blade d \"M 0 0 L 20 0 L 10 20 Z\" fill #4fd38c
  group stem at 0 30 {
    rect stick size 4 20 fill #6b4a2b
  }
}
use Leaf l1 at 50 40
use Leaf l2 at 150 40
";
        let scene = through(src, "l1");
        let outline = motif_core::kurbo::BezPath::from_svg("M 0 0 L 30 0 L 10 20 Z").unwrap();
        let out = crate::set_path(src, &scene, "l1-blade", &outline).unwrap();
        assert!(out.contains("  path blade d \"M 0 0 L 30 0 L 10 20 Z\" fill #4fd38c\n"), "{out}");
        let after = parse(&out).unwrap();
        let drawn = |id: &str| crate::path_to_d(&after.doc.node(id).unwrap().path().unwrap());
        assert_eq!((drawn("l1-blade"), drawn("l2-blade")), ("M 0 0 L 30 0 L 10 20 Z".to_string(), "M 0 0 L 30 0 L 10 20 Z".to_string()));
        // Read as usual, the points of a copy's contour are not rewritten.
        assert_eq!(crate::set_path(src, &parse(src).unwrap(), "l1-blade", &outline), None);

        // Deeper, in a group of the symbol: its objects are lines of the symbol too.
        let out = set_prop(src, &scene, "l1-stick", Prop::Height, Value::Num(30.0)).unwrap();
        assert!(out.contains("    rect stick size 4 30 fill #6b4a2b\n"), "{out}");
        let out = move_nodes(src, &scene, &["l1-blade".to_string()], "l1-stem", Place::Into, None).unwrap();
        assert!(out.contains("  group stem at 0 30 {\n    rect stick size 4 20 fill #6b4a2b\n    path blade d"), "{out}");
        let after = parse(&out).unwrap();
        let doc = &after.doc;
        assert!(doc.is_inside(doc.index_of("l2-blade").unwrap(), doc.index_of("l2-stem").unwrap()));
        // The blade stands where it stood: the group is 30 below, so it is written 30 above.
        assert_eq!(doc.node("l2-blade").unwrap().y, -30.0);
    }

    #[test]
    fn an_object_removed_from_the_symbol_leaves_every_copy() {
        let scene = through(COINS, "c1");
        let (out, removed, skipped) = delete_nodes(COINS, &scene, &["c1-shine".to_string()], None).unwrap();
        assert_eq!((removed, skipped.len()), (1, 0));
        // Its line, the keys of both copies and what a copy changed in it are gone.
        assert!(!out.contains("shine"), "{out}");
        let after = parse(&out).unwrap();
        assert!(after.doc.node("c2-shine").is_none() && after.doc.node("c2-body").is_some());
    }

    #[test]
    fn objects_of_a_symbol_are_grouped_copied_and_restacked_in_it() {
        let scene = through(COINS, "c1");
        let (out, group) = group_nodes(COINS, &scene, &["c1-body".to_string(), "c1-shine".to_string()], None).unwrap();
        assert_eq!(group, "c1-group1");
        let after = parse(&out).unwrap();
        for copy in ["c1", "c2"] {
            let doc = &after.doc;
            let g = doc.index_of(&format!("{copy}-group1")).unwrap();
            assert!(doc.is_inside(doc.index_of(&format!("{copy}-shine")).unwrap(), g) && doc.is_inside(doc.index_of(&format!("{copy}-body")).unwrap(), g));
        }
        // The keys of both copies moved with the objects: they count from the new group.
        let shift = after.doc.node("c1-shine").unwrap().x + 4.0;
        for copy in ["c1", "c2"] {
            let track = after.doc.animation("Spin").unwrap().tracks.iter().find(|t| t.node == format!("{copy}-shine")).unwrap();
            assert_eq!(track.keys[0].value.as_num().unwrap(), -4.0 + shift, "{out}");
        }

        // A copy of an object is written beside it under a free id of the symbol.
        let snippets = copy_nodes(&scene, COINS, &["c1-shine".to_string()]);
        let (out, ids) = paste_nodes_into(COINS, &scene, &snippets, (10.0, 10.0), Some("c1"));
        assert_eq!(ids, ["c1-shine1"]);
        assert!(out.contains("  rect shine at -4 -4 size 6 6 fill #ffffff\n  rect shine1 at 6 6 size 6 6 fill #ffffff\n}\n"), "{out}");
        let after = parse(&out).unwrap();
        assert!(after.doc.node("c1-shine1").is_some() && after.doc.node("c2-shine1").is_some());

        // One step under: the lines of the symbol swap.
        let out = restack(COINS, &scene, "c1-shine", false).unwrap();
        assert!(out.contains("{\n  rect shine at -4 -4 size 6 6 fill #ffffff\n  ellipse body"), "{out}");
        let after = parse(&out).unwrap();
        assert!(after.doc.index_of("c2-shine").unwrap() < after.doc.index_of("c2-body").unwrap());
    }

    const LABELS: &str = "\
artboard \"Кнопки\" 300x200 bg #ffffff
symbol Button {
  group row stack row center gap 6 pad 10 {
    rect bg fit r 8 fill #2d6cdf
    text label content \"Играть\" font-size 16 fill #ffffff
  }
}
use Button play at 80 60
use Button buy at 80 140
override {
  buy-label.content = \"Купить всё\"
}
";

    #[test]
    fn a_copy_says_words_of_its_own() {
        let words = |s: &Scene, id: &str| s.doc.node(id).unwrap().text.as_ref().unwrap().content.clone();
        let scene = parse(LABELS).unwrap();
        assert_eq!((words(&scene, "play-label").as_str(), words(&scene, "buy-label").as_str()), ("Играть", "Купить всё"));
        assert!(scene.doc.node("buy-label").unwrap().width > scene.doc.node("play-label").unwrap().width + 20.0, "the longer words are wider");
        // The symbol itself, while it is edited through the copy.
        let edited = parse_as(LABELS, None, Reading { theme: None, symbol_of: Some("buy"), vary_seed: 0 }).unwrap();
        assert_eq!(words(&edited, "buy-label"), "Играть");

        // The words are one of the changes of the copy and are taken back like the others.
        let changes = copy_changes(LABELS, &scene, "buy");
        assert_eq!(changes.len(), 1);
        assert_eq!((changes[0].part.as_deref(), changes[0].name.as_str(), changes[0].value.as_str()), (Some("label"), "content", "\"Купить всё\""));
        let back = reset_copy(LABELS, &scene, "buy", Some(0)).unwrap();
        assert_eq!(words(&parse(&back).unwrap(), "buy-label"), "Играть");

        // New words are written over the old ones; another copy gets an entry of its own.
        let said = "Взять \"всё\"\nсразу";
        let out = crate::set_own_text(LABELS, &scene, "buy-label", said).unwrap();
        assert_eq!(out.matches("buy-label.content").count(), 1, "{out}");
        assert_eq!(words(&parse(&out).unwrap(), "buy-label"), said);
        let out = crate::set_own_text(LABELS, &scene, "play-label", "Старт").unwrap();
        assert!(out.contains("  buy-label.content = \"Купить всё\"\n  play-label.content = \"Старт\"\n}"), "{out}");
        assert_eq!(crate::set_own_text(LABELS, &scene, "buy-bg", "нет"), None);
        // A scene without the block gets one.
        let plain = LABELS.replace("override {\n  buy-label.content = \"Купить всё\"\n}\n", "");
        let out = crate::set_own_text(&plain, &parse(&plain).unwrap(), "buy-label", "Купить").unwrap();
        assert!(out.ends_with("override {\n  buy-label.content = \"Купить\"\n}\n"), "{out}");

        let told = |src: &str| parse(src).unwrap_err().iter().map(ToString::to_string).collect::<Vec<_>>().join("\n");
        assert!(told(&LABELS.replace("buy-label.content", "buy-bg.content")).contains("«buy-bg» не текст"));
        assert!(told(&LABELS.replace("= \"Купить всё\"", "= 5")).contains("нужен текст в кавычках"));
        // Older Motif does not read such an entry.
        assert_eq!((crate::format::needed(LABELS), crate::format::needed(&plain)), (5, 1));
    }
}
