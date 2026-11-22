//! Safe edits of a symbol that has copies: before an object leaves the symbol, what refers to
//! it is found on every board of the file (animation tracks, edits of copies, variants), and the
//! removal takes all of it at once, so no board is left unreadable.

use crate::edit::{nodes_spans, remove_spans, selected_roots};
use crate::symbol::part_of;
use crate::{Reading, Scene, Span, parse_as};

/// What one board says about the objects of a symbol.
#[derive(Clone, Debug, PartialEq)]
pub struct BoardUse {
    /// The board, counted from 0, and its name.
    pub board: usize,
    pub name: String,
    /// Copies of the symbol on the board.
    pub copies: usize,
    /// Animation tracks of the objects, by clip, in the order of the names.
    pub tracks: Vec<(String, usize)>,
    /// Edits of copies in `override { }`.
    pub edits: usize,
    /// Other lines that name the objects: drivers, correctives, listeners of machines.
    pub links: usize,
}

/// What refers to objects of a symbol, in the whole file.
#[derive(Clone, Debug, PartialEq)]
pub struct PartUses {
    pub symbol: String,
    /// The objects asked about, by their ids inside the symbol.
    pub parts: Vec<String>,
    /// Variants of the symbol that hide or show one of them.
    pub variants: Vec<String>,
    /// Every board that has copies of the symbol.
    pub boards: Vec<BoardUse>,
}

impl PartUses {
    /// Something besides the symbol itself names the objects: removing them takes that too.
    pub fn referred(&self) -> bool {
        !self.variants.is_empty() || self.boards.iter().any(|b| !b.tracks.is_empty() || b.edits > 0 || b.links > 0)
    }

    pub fn copies(&self) -> usize {
        self.boards.iter().map(|b| b.copies).sum()
    }
}

/// The uses of objects `ids` (scene ids, `copy-object`) of the symbol that copy `copy` of board
/// `board` is a copy of, and every place of the file that goes when the objects are removed.
fn plan(file: &str, base: Option<&std::path::Path>, board: usize, copy: &str, ids: &[String]) -> Option<(PartUses, Vec<Span>)> {
    let views = crate::boards::board_views(file, base)?;
    let active = views.get(board)?;
    let symbol = active.scene.instances.get(copy)?.symbol.clone();
    let site = |scene: &Scene| scene.symbols.iter().find(|s| s.name == symbol).cloned();
    let sym = site(&active.scene)?;
    let which = active.in_file(sym.span);
    let through = |text: &str, copy: &str| parse_as(text, base, Reading { theme: None, symbol_of: Some(copy) }).ok();

    let edited = through(&active.text, copy)?;
    let roots = selected_roots(&edited, ids);
    let parts: Vec<String> = roots.iter().map(|id| part_of(&edited, copy, id).map(str::to_string)).collect::<Option<_>>()?;
    if parts.is_empty() {
        return None;
    }
    // The objects and everything inside them.
    let doc = &edited.doc;
    let inside: Vec<String> = doc
        .nodes
        .iter()
        .enumerate()
        .filter(|(i, n)| roots.iter().any(|r| *r == n.id || doc.index_of(r).is_some_and(|root| doc.is_inside(*i, root))))
        .filter_map(|(_, n)| part_of(&edited, copy, &n.id).map(str::to_string))
        .collect();

    let mut spans: Vec<Span> = Vec::new();
    let mut boards = Vec::new();
    for (k, v) in views.iter().enumerate() {
        // A board with a symbol of its own under the same name has another symbol.
        if site(&v.scene).map(|s| v.in_file(s.span)) != Some(which) {
            continue;
        }
        let mut copies: Vec<&String> = v.scene.instances.iter().filter(|(_, i)| i.symbol == symbol).map(|(id, _)| id).collect();
        copies.sort();
        let pick = if k == board { copy } else { copies.first()?.as_str() };
        let scene = if k == board { edited.clone() } else { through(&v.text, pick)? };
        let named: Vec<String> = parts.iter().map(|p| format!("{pick}-{p}")).collect();
        let named: Vec<&str> = named.iter().map(String::as_str).collect();
        spans.extend(nodes_spans(&v.text, &scene, &named)?.into_iter().map(|s| v.in_file(s)));

        let all: std::collections::HashSet<String> = copies.iter().flat_map(|c| inside.iter().map(move |p| format!("{c}-{p}"))).collect();
        let mut tracks: Vec<(String, usize)> =
            scene.anims.iter().map(|(name, a)| (name.clone(), a.tracks.iter().filter(|t| all.contains(&t.node)).count())).filter(|(_, n)| *n > 0).collect();
        tracks.sort();
        let edits = scene.override_entries.iter().filter(|(n, _)| all.contains(n)).count();
        let links = scene.driver_sites.values().flatten().filter(|t| all.contains(&t.node)).count()
            + scene.corrective_sites.values().flat_map(|c| &c.tracks).filter(|t| all.contains(&t.node)).count()
            + scene.doc.machines.iter().flat_map(|m| &m.listeners).filter(|l| all.contains(&l.node)).count();
        boards.push(BoardUse { board: k, name: v.scene.doc.artboard.name.clone(), copies: copies.len(), tracks, edits, links });
    }
    let variants = sym.variants.iter().filter(|v| v.hide.iter().chain(&v.show).any(|(id, _)| inside.contains(id))).map(|v| v.name.clone()).collect();
    Some((PartUses { symbol, parts, variants, boards }, spans))
}

/// What in the file refers to objects `ids` (scene ids, `copy-object`) of the symbol being edited
/// through copy `copy` of board `board`. `None` when a board has errors or the objects are not
/// objects of that copy.
pub fn symbol_part_uses(file: &str, base: Option<&std::path::Path>, board: usize, copy: &str, ids: &[String]) -> Option<PartUses> {
    plan(file, base, board, copy, ids).map(|(uses, _)| uses)
}

/// Returns the file without objects `ids` of the symbol being edited through copy `copy` of
/// board `board`: their lines leave the symbol, and with them, on every board, the animation
/// tracks, the edits of copies and the other lines that name them in any copy; variants stop
/// hiding them. Fails with the reason when the result cannot be read.
pub fn delete_symbol_parts(file: &str, base: Option<&std::path::Path>, board: usize, copy: &str, ids: &[String]) -> Result<String, String> {
    let (_, spans) = plan(file, base, board, copy, ids).ok_or("не удалось найти все строки выбранных объектов")?;
    let out = remove_spans(file, spans);
    match crate::parse_boards(&out, base) {
        Ok(_) => Ok(out),
        Err(errors) => Err(errors.first().map(ToString::to_string).unwrap_or_default()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FILE: &str = "\
symbol Coin size=24 {
  variant plain hide shine
  ellipse body size {size} fill #ffd23f
  group marks {
    rect shine at -4 -4 size 6 6 fill #ffffff
  }
}
artboard \"Первый\" 200x120
use Coin c1 at 40 60
use Coin c2 at 120 60 variant plain
rect floor at 100 110 size 200 10 fill #888888
anim Spin 1s loop {
  c1-shine.x 0: -4 | 1: 4
  c2-shine.x 0: -4 | 1: 4
  floor.x 0: 100 | 1: 110
}
override {
  c2-shine.fill = #ff0000
}
artboard \"Второй\" 200x120
use Coin c7 at 40 60
anim Blink 1s {
  c7-shine.opacity 0: 1 | 1: 0
  c7-body.opacity 0: 1 | 1: 0.5
}
machine M {
  input over false
  state Blink
  listen c7-marks hover -> over
}
";

    #[test]
    fn what_refers_to_an_object_of_a_symbol_is_found_on_every_board() {
        let uses = symbol_part_uses(FILE, None, 0, "c1", &["c1-marks".to_string()]).unwrap();
        assert_eq!((uses.symbol.as_str(), uses.parts.as_slice(), uses.variants.as_slice()), ("Coin", &["marks".to_string()][..], &["plain".to_string()][..]));
        assert_eq!(uses.copies(), 3);
        assert_eq!(
            uses.boards,
            [
                BoardUse { board: 0, name: "Первый".into(), copies: 2, tracks: vec![("Spin".into(), 2)], edits: 1, links: 0 },
                BoardUse { board: 1, name: "Второй".into(), copies: 1, tracks: vec![("Blink".into(), 1)], edits: 0, links: 1 },
            ]
        );
        assert!(uses.referred());
        // Nothing names the body except one track of the other board.
        let uses = symbol_part_uses(FILE, None, 0, "c1", &["c1-body".to_string()]).unwrap();
        assert_eq!(uses.boards[0].tracks, []);
        assert_eq!(uses.boards[1].tracks, [("Blink".to_string(), 1)]);
        assert!(uses.variants.is_empty() && uses.referred());
        // An object that is not of the copy is not asked about.
        assert_eq!(symbol_part_uses(FILE, None, 0, "c1", &["floor".to_string()]), None);
    }

    #[test]
    fn an_object_leaves_the_symbol_with_everything_that_names_it_on_every_board() {
        let out = delete_symbol_parts(FILE, None, 0, "c1", &["c1-marks".to_string()]).unwrap();
        let want = FILE
            .replace("  variant plain hide shine\n", "  variant plain\n")
            .replace("  group marks {\n    rect shine at -4 -4 size 6 6 fill #ffffff\n  }\n", "")
            .replace("  c1-shine.x 0: -4 | 1: 4\n  c2-shine.x 0: -4 | 1: 4\n", "")
            .replace("  c2-shine.fill = #ff0000\n", "")
            .replace("  c7-shine.opacity 0: 1 | 1: 0\n", "")
            .replace("  listen c7-marks hover -> over\n", "");
        assert_eq!(out, want);
        let boards = crate::parse_boards(&out, None).unwrap();
        assert!(boards[0].doc.node("c2-shine").is_none() && boards[1].doc.node("c7-body").is_some());
    }
}
