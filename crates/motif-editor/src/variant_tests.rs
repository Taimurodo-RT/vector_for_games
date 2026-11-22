//! Variants of a symbol in the panel of a copy: the copy takes one, its look becomes a new one,
//! and what a variant holds is changed for every copy that is this variant.

use super::editing_tests::fixture;
use super::snap_tests::frame;
use super::*;
use crate::variant_ui::VariantAct;

const BUTTONS: &str = "artboard \"a\" 400x300
symbol Button w=120 face=#2d6cdf {
  variant main face=#ff5a36
  rect bg size {w} 36 r 8 fill face
  ellipse glyph at -40 0 size 12 12 fill #ffffff
}
use Button b1 at 100 60
use Button b2 at 100 120 variant main
use Button b3 at 100 180 w=90
";

fn width(app: &App, id: &str) -> f64 {
    app.scene.doc.node(id).unwrap().width
}

fn fill(app: &App, id: &str) -> String {
    app.scene.doc.node(id).unwrap().fill.unwrap().to_hex()
}

#[test]
fn a_copy_takes_a_variant_and_goes_back_to_the_symbol() {
    let mut app = fixture(BUTTONS);
    app.select_only(Some("b1".into()));
    frame(&mut app, vec![], Modifiers::NONE);
    app.variant_act("b1", VariantAct::Take(Some("main".into())));
    assert!(app.src.contains("use Button b1 at 100 60 variant main\n"), "{}", app.src);
    assert_eq!(fill(&app, "b1-bg"), "#ff5a36");
    assert!(app.status.contains("вариант «main»"), "{}", app.status);
    // The panel of a copy with a variant is drawn.
    frame(&mut app, vec![], Modifiers::NONE);
    app.variant_act("b1", VariantAct::Take(None));
    assert_eq!(app.src, BUTTONS);
    // One step back each.
    app.undo();
    assert!(app.src.contains("b1 at 100 60 variant main"), "{}", app.src);
    app.undo();
    assert_eq!(app.src, BUTTONS);
}

#[test]
fn the_look_of_a_copy_becomes_a_variant_other_copies_can_take() {
    let mut app = fixture(BUTTONS);
    let before = app.scene.doc.clone();
    app.variant_act("b3", VariantAct::New);
    assert!(
        app.src.contains("  variant main face=#ff5a36\n  variant variant1 w=90\n") && app.src.contains("use Button b3 at 100 180 variant variant1\n"),
        "{}",
        app.src
    );
    assert_eq!(app.scene.doc, before, "nothing looks different");
    // A name of its own.
    app.variant_act("b3", VariantAct::Rename("small".into()));
    assert!(app.src.contains("  variant small w=90\n") && app.src.contains("b3 at 100 180 variant small\n"), "{}", app.src);
    app.variant_act("b3", VariantAct::Rename("main".into()));
    assert!(app.status.contains("не подходит") && app.src.contains("variant small w=90"), "{}", app.status);
    app.variant_act("b1", VariantAct::Take(Some("small".into())));
    assert_eq!(width(&app, "b1-bg"), 90.0);

    // What the variant holds is changed for both copies at once.
    app.variant_act("b1", VariantAct::Value("w".into(), Some(Value::Num(70.0))));
    assert_eq!((width(&app, "b1-bg"), width(&app, "b3-bg")), (70.0, 70.0));
    app.variant_act("b1", VariantAct::Visible("glyph".into(), false));
    assert!(app.src.contains("  variant small w=70 hide glyph\n"), "{}", app.src);
    let visible = |app: &App, id: &str| app.scene.doc.node(id).unwrap().visible;
    assert!(!visible(&app, "b1-glyph") && !visible(&app, "b3-glyph") && visible(&app, "b2-glyph"));
    app.variant_act("b1", VariantAct::Value("face".into(), Some(Value::Color(Color { r: 0, g: 0, b: 0, a: 255 }))));
    assert!(app.src.contains("  variant small w=70 face=#000000 hide glyph\n"), "{}", app.src);
    app.variant_act("b1", VariantAct::Value("w".into(), None));
    assert_eq!(width(&app, "b3-bg"), 120.0);
    // A parameter of the copy itself is stronger than its variant and stays the copy's own.
    let out = motif_script::set_param(&app.src, &app.scene, "b3", "face", Value::Color(Color { r: 255, g: 255, b: 255, a: 255 }));
    assert!(app.apply("b3.face", out));
    assert_eq!((fill(&app, "b3-bg").as_str(), fill(&app, "b1-bg").as_str()), ("#ffffff", "#000000"));

    // Without the variant its copies are the symbol again; the copy keeps what it wrote itself.
    app.variant_act("b1", VariantAct::Delete);
    assert!(!app.src.contains("variant small") && app.src.contains("use Button b3 at 100 180 face=#ffffff\n"), "{}", app.src);
    assert!(visible(&app, "b1-glyph") && fill(&app, "b1-bg") == "#2d6cdf");
}

#[test]
fn a_variant_shows_a_detail_the_symbol_keeps_hidden() {
    let src = BUTTONS.replace("fill #ffffff\n}", "fill #ffffff\n  ellipse badge at 54 -14 size 10 10 fill #ff0000 hidden\n}");
    let mut app = fixture(&src);
    let visible = |app: &App, id: &str| app.scene.doc.node(id).unwrap().visible;
    assert!(!visible(&app, "b1-badge") && !visible(&app, "b2-badge"));
    // Seen in the copies of the variant only: the variant writes what differs from the symbol.
    app.variant_act("b2", VariantAct::Visible("badge".into(), true));
    assert!(app.src.contains("  variant main face=#ff5a36 show badge\n"), "{}", app.src);
    assert!(visible(&app, "b2-badge") && !visible(&app, "b1-badge") && app.status.contains("показывает «badge»"), "{}", app.status);
    app.variant_act("b2", VariantAct::Visible("badge".into(), false));
    assert!(app.src.contains("  variant main face=#ff5a36\n") && !visible(&app, "b2-badge"), "{}", app.src);
    app.undo();
    assert!(visible(&app, "b2-badge"));
}

#[test]
fn inside_a_copy_the_symbol_shows_what_its_variant_hides() {
    let src = BUTTONS.replace("variant main face=#ff5a36", "variant main face=#ff5a36 hide glyph");
    let mut app = fixture(&src);
    assert!(!app.scene.doc.node("b2-glyph").unwrap().visible);
    app.edit_symbol("b2");
    assert_eq!(app.scene.editing.as_deref(), Some("b2"));
    assert!(app.scene.doc.node("b2-glyph").unwrap().visible, "the whole symbol is there to edit");
    // The object leaves the symbol and the list of what the variant hides.
    app.select_only(Some("b2-glyph".into()));
    app.delete_selection();
    // A variant hides it: the question names the variant.
    assert_eq!(app.symbol_delete.as_ref().map(|d| d.uses.variants.clone()), Some(vec!["main".to_string()]));
    app.answer_symbol_delete(true);
    assert!(app.src.contains("  variant main face=#ff5a36\n  rect bg") && !app.src.contains("glyph"), "{}", app.src);
}

#[test]
fn a_variant_of_a_shared_symbol_is_renamed_and_removed_on_every_board() {
    let file = "symbol Tag w=20 {
  variant wide w=60
  rect bg size {w} 10 fill #333333
}
artboard \"a\" 400x300
use Tag t1 at 100 150 variant wide
artboard \"b\" 400x300
use Tag t2 at 100 150 variant wide
";
    let mut app = fixture(file);
    assert_eq!(app.board_count(), 2);
    app.variant_act("t1", VariantAct::Rename("long".into()));
    assert_eq!(app.file_text(), file.replace("wide", "long"));
    app.variant_act("t1", VariantAct::Delete);
    assert_eq!(app.file_text(), file.replace("  variant wide w=60\n", "").replace(" variant wide", ""));
    app.undo();
    assert_eq!(app.file_text(), file.replace("wide", "long"));
}
