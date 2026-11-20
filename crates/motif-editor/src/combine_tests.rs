//! Live operations as the editor carries them out: made with the boolean buttons, changed and
//! taken apart in the group's section, and their shapes moved on the canvas.

use super::canvas_menu::InspectorCategory as Category;
use super::editing_tests::fixture;
use super::snap_tests::{click, drag, screen};
use super::*;
use motif_core::kurbo::Shape;
use motif_script::BoolOp;

const SCENE: &str = "artboard \"a\" 400x300\nrect base \"Плита\" at 150 150 size 120 100 fill #ffd34d stroke #1b1d24 4\nellipse hole \"Вырез\" at 210 150 size 60 60 fill #ff0000\nrect far at 340 40 size 30 30 fill #888888\n";

/// The scene with `base` and `hole` put into a live subtraction; returns the group's id.
fn combined(app: &mut App) -> String {
    app.select_only(Some("base".into()));
    app.toggle_selected("hole".into());
    app.combine_selection(BoolOp::Subtract);
    assert!(app.errors.is_empty(), "{:?}\n{}", app.errors, app.src);
    app.selected.clone().expect("the new group is selected")
}

fn area(app: &App, id: &str) -> f64 {
    let doc = app.display_doc();
    doc.node(id).unwrap().path().unwrap().area().abs()
}

fn frame_of(app: &App, id: &str) -> motif_core::kurbo::Rect {
    let doc = app.display_doc();
    crate::selection::world_bounds(&doc, doc.index_of(id).unwrap()).unwrap()
}

#[test]
fn the_boolean_buttons_make_a_live_operation_in_one_undo_step() {
    let mut app = fixture(SCENE);
    let group = combined(&mut app);
    assert!(app.src.contains(&format!("group {group} at 165 150 combine subtract {{")), "{}", app.src);
    let doc = app.display_doc();
    let (g, base, hole) = (doc.index_of(&group).unwrap(), doc.index_of("base").unwrap(), doc.index_of("hole").unwrap());
    // The shapes are still there; one draws the result, the other only shapes it.
    assert_eq!((doc.nodes[base].kind, doc.nodes[hole].kind), (ShapeKind::Rect, ShapeKind::Ellipse));
    assert!(doc.is_drawn(base) && !doc.is_drawn(hole) && doc.combined_in(hole) == Some(g));
    let cut = 12000.0 - std::f64::consts::PI * 900.0 / 2.0;
    assert!((area(&app, "base") - cut).abs() < 3.0, "{}", area(&app, "base"));
    assert!(app.status.contains("живая операция"), "{}", app.status);
    assert_eq!(app.undo.len(), 1, "one click is one undo step");
    app.undo();
    assert_eq!(app.src, SCENE);

    // With Alt the button cuts at once, as it always did.
    app.select_only(Some("base".into()));
    app.toggle_selected("hole".into());
    app.boolean(BoolOp::Subtract);
    assert!(!app.src.contains("combine") && !app.src.contains("hole") && app.src.contains("path "), "{}", app.src);
    // One shape is not enough, and the user is told.
    let mut app = fixture(SCENE);
    app.select_only(Some("far".into()));
    app.combine_selection(BoolOp::Union);
    assert!(app.status.contains("две фигуры") && app.src == SCENE, "{}", app.status);
}

#[test]
fn a_shape_taken_in_is_still_picked_and_moved_on_the_canvas() {
    let mut app = fixture(SCENE);
    combined(&mut app);
    let before = area(&app, "base");
    // Inside the cut nothing of the plate is painted: the click lands on the cutter.
    let at = screen(&mut app, 222.0, 150.0);
    click(&mut app, at, Modifiers::NONE);
    assert_eq!(app.selected.as_deref(), Some("hole"));
    // What is left of the plate is the plate.
    let at = screen(&mut app, 110.0, 150.0);
    click(&mut app, at, Modifiers::NONE);
    assert_eq!(app.selected.as_deref(), Some("base"));
    // The cutter dragged out of the plate: the plate is whole again, in one undo step.
    let (from, to) = (screen(&mut app, 222.0, 150.0), screen(&mut app, 322.0, 150.0));
    let steps = app.undo.len();
    drag(&mut app, from, to, Modifiers::CTRL);
    assert_eq!(app.selected.as_deref(), Some("hole"));
    assert!((area(&app, "base") - 12000.0).abs() < 1.0 && before < 11000.0, "{} {before}", area(&app, "base"));
    assert_eq!(app.undo.len(), steps + 1);
    app.undo();
    assert!((area(&app, "base") - before).abs() < 1e-6);
}

#[test]
fn frames_follow_what_is_picked() {
    let mut app = fixture(SCENE);
    let group = combined(&mut app);
    // The plate is framed by its own outline, though it draws less; the cutter by its own.
    let plate = frame_of(&app, "base");
    assert_eq!((plate.x0, plate.x1, plate.y0, plate.y1), (90.0, 210.0, 100.0, 200.0));
    let hole = frame_of(&app, "hole");
    assert_eq!((hole.x0, hole.x1), (180.0, 240.0));
    // The group is framed by what it draws: the cutter sticking out is not part of it.
    let whole = frame_of(&app, &group);
    assert!((whole.x0 - 90.0).abs() < 0.01 && (whole.x1 - 210.0).abs() < 0.01, "{whole:?}");
    // A union reaches as far as its shapes do.
    app.set_combine(&group, Some(BoolOp::Union));
    let whole = frame_of(&app, &group);
    assert!((whole.x1 - 240.0).abs() < 0.01, "{whole:?}");
}

#[test]
fn the_operation_is_changed_taken_off_and_taken_apart() {
    let mut app = fixture(SCENE);
    let group = combined(&mut app);
    let half = std::f64::consts::PI * 900.0 / 2.0;
    app.set_combine(&group, Some(BoolOp::Intersect));
    assert!(app.src.contains("combine intersect {") && (area(&app, "base") - half).abs() < 3.0, "{}", app.src);
    assert!(app.status.contains("пересечение"), "{}", app.status);
    // No operation: a plain group, each shape draws itself.
    app.set_combine(&group, None);
    let doc = app.display_doc();
    assert!(!app.src.contains("combine") && doc.is_drawn(doc.index_of("hole").unwrap()), "{}", app.src);
    app.set_combine(&group, Some(BoolOp::Subtract));

    // «Разобрать»: one outline under the group's id, selected, its points open for editing.
    let (steps, cut) = (app.undo.len(), area(&app, "base"));
    app.flatten_combine(&group);
    assert!(app.errors.is_empty(), "{:?}\n{}", app.errors, app.src);
    let path = app.scene.doc.node(&group).unwrap();
    assert_eq!((path.kind, app.selected.as_deref()), (ShapeKind::Path, Some(group.as_str())));
    assert!(app.scene.doc.node("hole").is_none() && app.src.contains("fill #ffd34d stroke #1b1d24 4"), "{}", app.src);
    assert!((area(&app, &group) - cut).abs() < 1.0);
    app.begin_point_edit();
    assert!(app.point_edit.is_some());
    assert_eq!(app.undo.len(), steps + 1);
    app.undo();
    assert!(app.src.contains("combine subtract {") && app.scene.doc.node("hole").is_some());

    // A cutter that slides in a clip would stand still after it: refused, in words.
    let moving = format!("{}anim Slide 1s {{\n  hole.x 0: 30 | 1: 0\n}}\n", app.src);
    let mut app = fixture(&moving);
    let was = app.src.clone();
    app.flatten_combine(&group);
    assert!(app.status.contains("Не разобрать") && app.status.contains("«hole»") && app.src == was, "{}", app.status);
    // The game panel says what such an operation costs.
    let warned = crate::game_panel::warnings(&[app.scene.doc.clone()], None);
    assert!(warned.iter().any(|(_, w)| w.contains("каждом кадре") && w.contains("«hole»")), "{warned:?}");
}

#[test]
fn the_section_is_offered_to_the_group_and_to_its_shapes() {
    let mut app = fixture(SCENE);
    assert!(!app.inspector_categories("base").contains(&Category::Combine), "a loose shape has no operation");
    let group = combined(&mut app);
    for id in [group.as_str(), "base", "hole"] {
        app.select_only(Some(id.to_string()));
        assert!(app.inspector_categories(id).contains(&Category::Combine), "{id}");
    }
    assert!(!app.inspector_categories("far").contains(&Category::Combine));
    // A plain group is offered the buttons too: that is where an operation is switched on.
    app.set_combine(&group, None);
    assert!(app.inspector_categories(&group).contains(&Category::Combine));
}
