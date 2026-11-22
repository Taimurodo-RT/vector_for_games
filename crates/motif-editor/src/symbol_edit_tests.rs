//! A symbol edited on the canvas through one of its copies: what is changed, drawn or removed
//! inside the copy changes the symbol and every copy; «Только эта копия» keeps the old way.

use super::editing_tests::fixture;
use super::isolate_tests::{double_click, key, parent};
use super::snap_tests::{bounds, drag, frame, ids, new_id, screen};
use super::*;
use crate::workspace::Workspace;

/// Two coins and an object that has nothing to do with them.
///
/// On the artboard: `c1-body` 80..120 × 130..170, `c1-shine` 87..97 × 137..147, `c2` around
/// 300 150 with a body of 60 and a shine it paints red for itself.
const COINS: &str = "artboard \"a\" 400x300
symbol Coin size=40 color=#ffd23f {
  ellipse body size {size} {size} fill color
  rect shine at -8 -8 size 10 10 fill #ffffff
}
use Coin c1 at 100 150
use Coin c2 at 300 150 size=60
rect far at 350 250 size 40 40 fill #888888
override {
  c2-shine.fill = #ff0000
}
";

fn x(app: &App, id: &str) -> f64 {
    app.scene.doc.node(id).unwrap().x
}

#[test]
fn a_double_click_on_a_copy_opens_its_symbol_and_a_drag_moves_the_object_in_every_copy() {
    let mut app = fixture(COINS);
    assert_eq!(app.workspace(), Workspace::Drawing);
    let shine = screen(&mut app, 92.0, 142.0);
    double_click(&mut app, shine);
    assert_eq!((app.isolated.as_deref(), app.selected.as_deref()), (Some("c1"), Some("c1-shine")));
    assert_eq!(app.scene.editing.as_deref(), Some("c1"));
    assert!(app.status.contains("символ Coin") && app.status.contains("(2)"), "{}", app.status);

    // Ctrl: no snapping, the object goes where the pointer does.
    let to = screen(&mut app, 102.0, 142.0);
    drag(&mut app, shine, to, Modifiers::CTRL);
    let moved = x(&app, "c1-shine");
    assert!(moved > -8.0 && x(&app, "c2-shine") == moved, "{}", app.src);
    // The line of the symbol has the new place; nothing is written for the copy.
    assert!(
        !app.src.contains("rect shine at -8 -8") && !app.src.contains("c1-shine.x") && app.src.ends_with("override {\n  c2-shine.fill = #ff0000\n}\n"),
        "{}",
        app.src
    );

    // One undo step brings the symbol back, and the symbol stays open.
    app.undo();
    assert_eq!((app.src.as_str(), app.scene.editing.as_deref()), (COINS, Some("c1")));

    // Out of the copy the script is read as usual again.
    key(&mut app, Key::Escape, Modifiers::NONE);
    frame(&mut app, vec![], Modifiers::NONE);
    assert_eq!((app.isolated.as_deref(), app.scene.editing.as_deref()), (None, None));
}

#[test]
fn only_this_copy_keeps_an_edit_with_the_copy() {
    let mut app = fixture(COINS);
    app.isolate(Some("c1".into()));
    assert_eq!(app.scene.editing.as_deref(), Some("c1"));
    app.set_copy_only(true);
    assert_eq!(app.scene.editing, None);
    app.edit("c1-shine", Prop::X, Value::Num(4.0));
    assert!(app.src.contains("  c1-shine.x = 4\n") && app.src.contains("rect shine at -8 -8"), "{}", app.src);
    assert_eq!((x(&app, "c1-shine"), x(&app, "c2-shine")), (4.0, -8.0));

    // Back to the symbol: the copy shows the symbol itself, its own change waits in the script.
    app.set_copy_only(false);
    assert_eq!((app.scene.editing.as_deref(), x(&app, "c1-shine")), (Some("c1"), -8.0));
    app.edit("c1-shine", Prop::X, Value::Num(-6.0));
    assert!(app.src.contains("rect shine at -6 -8") && app.src.contains("  c1-shine.x = 4\n"), "{}", app.src);

    // The choice holds for one stay inside a copy.
    app.set_copy_only(true);
    app.isolate(None);
    frame(&mut app, vec![], Modifiers::NONE);
    assert!(!app.copy_only);
    assert_eq!((x(&app, "c1-shine"), x(&app, "c2-shine")), (4.0, -6.0));
}

#[test]
fn a_value_that_a_parameter_sets_is_not_written_for_the_copy_behind_the_back() {
    let mut app = fixture(COINS);
    app.isolate(Some("c1".into()));
    for (prop, value) in [(Prop::Width, Value::Num(50.0)), (Prop::Fill, Value::Color(motif_core::Color::from_hex("#3b5bff").unwrap()))] {
        app.edit("c1-body", prop, value);
        assert_eq!(app.src, COINS);
        assert!(app.status.contains("«только эту копию»"), "{}", app.status);
    }
    assert!(app.undo.is_empty());
    // For the copy alone the same edit is written, as before.
    app.set_copy_only(true);
    app.edit("c1-body", Prop::Width, Value::Num(50.0));
    assert!(app.src.contains("  c1-body.w = 50\n"), "{}", app.src);
}

#[test]
fn what_is_drawn_inside_a_copy_becomes_an_object_of_the_symbol() {
    let mut app = fixture(COINS);
    app.isolate(Some("c1".into()));
    let before = ids(&app);
    app.create_shape(ShapeKind::Rect, Point::new(130.0, 100.0), Point::new(150.0, 120.0), false);
    let made = new_id(&app, &before);
    assert_eq!((made.as_str(), parent(&app, &made).as_deref(), app.selected.as_deref()), ("c1-rect1", Some("c1"), Some("c1-rect1")), "{}", app.src);
    assert!(app.src.contains("  rect shine at -8 -8 size 10 10 fill #ffffff\n  rect rect1 at 40 -40 size 20 "), "{}", app.src);
    let (b, other) = (bounds(&app, &made), bounds(&app, "c2-rect1"));
    assert_eq!((b.x0, b.y0, b.x1, b.y1), (130.0, 100.0, 150.0, 120.0));
    assert_eq!((other.x0, other.y0), (330.0, 100.0));
    assert_eq!(app.undo.len(), 1, "one shape is one undo step");
    // Nothing is left where the object was first written: not an empty line either.
    assert!(app.src.contains("use Coin c2 at 300 150 size=60\nrect far at 350 250 size 40 40 fill #888888\noverride {\n"), "{}", app.src);
    frame(&mut app, vec![], Modifiers::NONE);
    assert_eq!((app.isolated.as_deref(), app.scene.editing.as_deref()), (Some("c1"), Some("c1")));

    // A duplicate is written beside its original, in the symbol.
    app.select_only(Some("c1-shine".into()));
    app.duplicate();
    assert_eq!(app.selected.as_deref(), Some("c1-shine1"), "{}", app.src);
    assert!(app.src.contains("  rect shine1 at 2 2 size 10 10 fill #ffffff\n}"), "{}", app.src);
    assert!(app.scene.doc.node("c2-shine1").is_some());

    // An object copied outside comes into the symbol where its original stands.
    let ctx = app.ctx.clone();
    app.isolate(None);
    app.select_only(Some("far".into()));
    assert!(app.copy_selection(&ctx));
    app.isolate(Some("c1".into()));
    app.paste(None);
    assert_eq!(app.selected.as_deref(), Some("c1-far1"), "{}", app.src);
    let (b, far) = (bounds(&app, "c1-far1"), bounds(&app, "far"));
    assert_eq!((b.x0, b.y0), (far.x0 + 10.0, far.y0 + 10.0));
}

#[test]
fn objects_of_a_symbol_are_removed_grouped_and_restacked_for_every_copy() {
    let mut app = fixture(COINS);
    app.isolate(Some("c1".into()));
    app.select_list(vec!["c1-body".into(), "c1-shine".into()]);
    app.group_selection();
    let group = app.selected.clone().unwrap();
    assert_eq!((group.as_str(), parent(&app, "c1-shine").as_deref(), parent(&app, "c2-shine").as_deref()), ("c1-group1", Some("c1-group1"), Some("c2-group1")));
    app.undo();

    app.select_only(Some("c1-shine".into()));
    app.restack_selection(false);
    let doc = &app.scene.doc;
    assert!(doc.index_of("c2-shine").unwrap() < doc.index_of("c2-body").unwrap(), "{}", app.src);
    app.undo();

    // A copy changed the object for itself, so the removal first shows what goes with it.
    app.select_only(Some("c1-shine".into()));
    app.delete_selection();
    assert_eq!(app.src, COINS, "nothing is removed before the answer");
    let asked = app.symbol_delete.as_ref().expect("the removal waits for an answer");
    assert_eq!((asked.uses.copies(), asked.uses.boards[0].edits), (2, 1));
    frame(&mut app, vec![], Modifiers::NONE);
    // «Отмена» leaves the symbol as it was.
    app.answer_symbol_delete(false);
    assert!(app.symbol_delete.is_none() && app.src == COINS && app.status.contains("отменено"), "{}", app.status);
    // «Удалить во всех копиях»: with the object goes what the copy changed in it.
    app.delete_selection();
    app.answer_symbol_delete(true);
    assert!(!app.src.contains("shine"), "{}", app.src);
    assert!(app.scene.doc.node("c2-shine").is_none() && app.scene.doc.node("c2-body").is_some());
    assert_eq!(app.scene.editing.as_deref(), Some("c1"), "the symbol stays open");
    // One step back returns the object and the edit of the copy.
    app.undo();
    assert_eq!(app.src, COINS);

    // An object nothing refers to is removed at once, as before.
    app.select_only(Some("c1-body".into()));
    app.delete_selection();
    assert!(app.symbol_delete.is_none() && !app.src.contains("body"), "{}", app.src);
}

#[test]
fn a_symbol_is_edited_in_drawing_only_and_the_panel_button_leads_into_it() {
    let mut app = fixture(COINS);
    app.set_workspace(Workspace::Setup);
    app.edit_symbol("c2");
    assert_eq!((app.workspace(), app.isolated.as_deref(), app.scene.editing.as_deref()), (Workspace::Drawing, Some("c2"), Some("c2")));
    // The copy shows the symbol: its red shine is white while the symbol is edited.
    let fill = |app: &App| app.scene.doc.node("c2-shine").unwrap().fill.unwrap().to_hex();
    assert_eq!(fill(&app), "#ffffff");
    // In the other workspaces keys are written, and they belong to the copy.
    app.set_workspace(Workspace::Animate);
    assert_eq!((app.scene.editing.as_deref(), fill(&app).as_str()), (None, "#ff0000"));
    app.set_workspace(Workspace::Drawing);
    frame(&mut app, vec![], Modifiers::NONE);
    assert_eq!(app.isolated.as_deref().and(app.scene.editing.as_deref()), app.isolated.as_deref());
}

#[test]
fn an_object_of_a_shared_symbol_is_removed_on_every_board_after_the_question() {
    // The symbol is written above the first artboard: both boards place copies of it, and the
    // second one animates the shine of its copy.
    let file = "symbol Coin {
  ellipse body size 40 40 fill #ffd23f
  rect shine at -8 -8 size 10 10 fill #ffffff
}
artboard \"a\" 400x300
use Coin c1 at 100 150
artboard \"b\" 400x300
use Coin c7 at 100 150
anim Blink 1s {
  c7-shine.opacity 0: 1 | 1: 0
}
";
    let mut app = fixture(file);
    assert_eq!(app.board_count(), 2);
    app.isolate(Some("c1".into()));
    assert_eq!(app.scene.editing.as_deref(), Some("c1"));
    // A change of the shared symbol reaches the other board.
    app.edit("c1-shine", Prop::X, Value::Num(-6.0));
    assert!(app.file_text().starts_with("symbol Coin {\n  ellipse body size 40 40 fill #ffd23f\n  rect shine at -6 -8"), "{}", app.file_text());
    // The other board animates the object: the removal shows that board too and, once
    // answered, takes the track with the object, so no board is left unreadable.
    let before = app.file_text();
    app.select_only(Some("c1-shine".into()));
    app.delete_selection();
    assert_eq!(app.file_text(), before);
    let asked = app.symbol_delete.as_ref().expect("the removal waits for an answer");
    assert_eq!(asked.uses.boards.iter().map(|b| (b.name.as_str(), b.copies, b.tracks.len())).collect::<Vec<_>>(), [("a", 1, 0), ("b", 1, 1)]);
    app.answer_symbol_delete(true);
    assert!(!app.file_text().contains("shine"), "{}", app.file_text());
    assert!(app.file_text().contains("anim Blink 1s {\n}\n"), "{}", app.file_text());
    assert!(motif_script::parse_boards(&app.file_text(), None).is_ok());
    app.undo();
    assert_eq!(app.file_text(), before);
}

#[test]
fn a_detached_copy_is_a_group_of_its_own_with_the_same_picture() {
    let mut app = fixture(COINS);
    let before = app.scene.doc.clone();
    app.select_only(Some("c2".into()));
    frame(&mut app, vec![], Modifiers::NONE);
    app.detach_copy("c2");
    assert!(
        app.src.contains("group c2 at 300 150 {\n  ellipse c2-body size 60 60 fill #ffd23f\n  rect c2-shine at -8 -8 size 10 10 fill #ffffff\n}\n"),
        "{}",
        app.src
    );
    assert_eq!(app.scene.doc, before, "the picture and the edit of the copy are the same");
    assert!(!app.scene.instances.contains_key("c2") && app.scene.instances.contains_key("c1"));
    assert!(app.status.contains("отцеплена") && app.selected.as_deref() == Some("c2"), "{}", app.status);
    // The symbol no longer changes the group.
    app.isolate(Some("c1".into()));
    app.edit("c1-shine", Prop::X, Value::Num(0.0));
    assert_eq!((x(&app, "c1-shine"), x(&app, "c2-shine")), (0.0, -8.0));
    app.undo();
    app.undo();
    assert_eq!(app.src, COINS);

    // From inside the symbol: the symbol is left first, then the copy is detached.
    app.edit_symbol("c1");
    app.detach_copy("c1");
    assert_eq!((app.scene.editing.as_deref(), app.isolated.as_deref()), (None, None));
    assert!(app.src.contains("group c1 at 100 150 {\n  ellipse c1-body size 40 40 fill #ffd23f\n"), "{}", app.src);
}

#[test]
fn the_words_typed_for_a_text_of_a_copy_stay_with_that_copy() {
    let mut app = fixture(
        "artboard \"a\" 400x300
symbol Button {
  rect bg size 120 36 r 8 fill #2d6cdf
  text label content \"Играть\" font-size 16 fill #ffffff
}
use Button play at 100 60
use Button buy at 100 140
",
    );
    let words = |app: &App, id: &str| app.scene.doc.node(id).unwrap().text.as_ref().unwrap().content.clone();
    // Typed letter by letter: one entry, one step back.
    for typed in ["К", "Ку", "Купить"] {
        app.set_own_text("buy-label", typed);
    }
    assert!(app.src.ends_with("override {\n  buy-label.content = \"Купить\"\n}\n"), "{}", app.src);
    assert_eq!((words(&app, "buy-label").as_str(), words(&app, "play-label").as_str()), ("Купить", "Играть"));
    assert!(app.status.contains("теперь свой"), "{}", app.status);
    // The list of the copy's changes takes the words back.
    assert_eq!(motif_script::copy_changes(&app.src, &app.scene, "buy").len(), 1);
    app.undo();
    assert_eq!(words(&app, "buy-label"), "Играть");
    assert!(!app.src.contains("buy-label.content"), "{}", app.src);
}
