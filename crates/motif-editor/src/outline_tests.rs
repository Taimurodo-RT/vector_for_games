//! The outline commands of the «Контур» section, as the editor carries them out.

use super::canvas_menu::InspectorCategory as Category;
use super::editing_tests::fixture;
use super::snap_tests::bounds;
use super::*;
use crate::outline_ui::OutlineCommand;

const SCENE: &str = "artboard \"a\" 400x300\nrect back at 200 150 size 300 200 fill #222222\nrect box at 200 150 size 120 80 r 10 fill #ffd34d stroke #1b1d24 8 rot 15\npath wire at 90 60 d \"M -40 0 L 0 -20 L 40 0\" fill none stroke #c2410c 6 dash 12 10 cap round\ngroup set at 320 60 {\n  ellipse dot size 20 20 fill #ffffff\n}\nanim Idle 1s loop {\n  box.x 0: 200 | 1: 240\n}\n";

fn kind(app: &App, id: &str) -> ShapeKind {
    app.scene.doc.node(id).unwrap().kind
}

fn order(app: &App, id: &str) -> usize {
    app.scene.doc.index_of(id).unwrap()
}

#[test]
fn a_shape_becomes_a_path_in_one_undo_step_and_its_points_open_for_editing() {
    let mut app = fixture(SCENE);
    app.select_only(Some("box".into()));
    let was = bounds(&app, "box");
    app.outline_command("box", OutlineCommand::ShapeToPath);
    assert!(app.errors.is_empty(), "{:?}\n{}", app.errors, app.src);
    assert_eq!(kind(&app, "box"), ShapeKind::Path);
    assert_eq!(app.selected.as_deref(), Some("box"));
    let now = bounds(&app, "box");
    assert!((was.x0 - now.x0).abs() < 0.02 && (was.y1 - now.y1).abs() < 0.02, "{was:?} {now:?}");
    // The key of the clip still moves the same object, and its paint stayed on the line.
    assert!(app.src.contains("box.x 0: 200 | 1: 240"));
    assert!(app.src.contains("fill #ffd34d stroke #1b1d24 8 rot 15"), "{}", app.src);
    assert!(app.status.contains("теперь контур"), "{}", app.status);
    app.begin_point_edit();
    assert!(app.point_edit.is_some(), "the points of the new path can be edited");
    assert_eq!(app.undo.len(), 1, "one command is one undo step");
    app.undo();
    assert_eq!(app.src, SCENE);
    assert_eq!(kind(&app, "box"), ShapeKind::Rect);
}

#[test]
fn a_stroke_becomes_an_outline_and_undo_brings_the_stroke_back() {
    // A filled shape keeps its fill; the outline of the stroke is a new object right above it.
    let mut app = fixture(SCENE);
    app.select_only(Some("box".into()));
    let was = bounds(&app, "box");
    app.outline_command("box", OutlineCommand::StrokeToPath);
    assert!(app.errors.is_empty(), "{:?}\n{}", app.errors, app.src);
    assert_eq!(app.selected.as_deref(), Some("box_stroke"));
    assert_eq!(order(&app, "box_stroke"), order(&app, "box") + 1);
    let (shape, outline) = (app.scene.doc.node("box").unwrap(), app.scene.doc.node("box_stroke").unwrap());
    assert!(shape.stroke.is_none() && shape.fill.is_some());
    assert!(outline.stroke.is_none() && outline.fill == Some(motif_core::Color::rgb(0x1b, 0x1d, 0x24)));
    // The outline reaches half the width of the stroke beyond the turned shape.
    let now = bounds(&app, "box_stroke");
    assert!((3.9..5.7).contains(&(was.x0 - now.x0)) && (3.9..5.7).contains(&(now.y1 - was.y1)), "{was:?} {now:?}");
    // The shape moves in a clip and the new outline does not: the user is told.
    assert!(app.status.contains("Idle") && app.status.contains("box_stroke"), "{}", app.status);
    assert_eq!(app.undo.len(), 1);
    app.undo();
    assert_eq!(app.src, SCENE);

    // A line without a fill becomes the outline itself: same id, dashes and round ends in its form.
    app.select_only(Some("wire".into()));
    app.outline_command("wire", OutlineCommand::StrokeToPath);
    assert!(app.errors.is_empty(), "{:?}\n{}", app.errors, app.src);
    assert_eq!(app.selected.as_deref(), Some("wire"));
    let wire = app.scene.doc.node("wire").unwrap();
    assert!(wire.stroke.is_none() && wire.dash.is_empty() && wire.fill == Some(motif_core::Color::rgb(0xc2, 0x41, 0x0c)));
    let parts = wire.geometry.elements().iter().filter(|el| matches!(el, motif_core::kurbo::PathEl::MoveTo(_))).count();
    assert!(parts >= 5, "every dash is a part of the outline: {parts}");
    app.undo();
    assert_eq!(app.src, SCENE);

    // Nothing to turn: the file stays and the status says why.
    app.outline_command("back", OutlineCommand::StrokeToPath);
    assert_eq!(app.src, SCENE);
    assert!(app.status.contains("нет обводки"), "{}", app.status);
}

#[test]
fn an_offset_outline_appears_next_to_the_shape_and_the_shape_stays() {
    let mut app = fixture(SCENE);
    app.select_only(Some("back".into()));
    let was = bounds(&app, "back");
    app.outline_command("back", OutlineCommand::Offset(6.0));
    assert!(app.errors.is_empty(), "{:?}\n{}", app.errors, app.src);
    assert_eq!(app.selected.as_deref(), Some("back_offset"));
    let wide = bounds(&app, "back_offset");
    assert!((wide.x0 - (was.x0 - 6.0)).abs() < 0.02 && (wide.y1 - (was.y1 + 6.0)).abs() < 0.02, "{wide:?}");
    assert!(order(&app, "back_offset") < order(&app, "back"), "the wider outline lies under the shape");
    assert_eq!(bounds(&app, "back"), was);
    assert!(app.status.contains("наружу на 6"), "{}", app.status);

    app.offset_sharp = true;
    app.outline_command("back", OutlineCommand::Offset(-10.0));
    assert!(app.errors.is_empty(), "{:?}\n{}", app.errors, app.src);
    let made = app.selected.clone().unwrap();
    assert_eq!(made, "back_offset2", "the first name is taken");
    let narrow = bounds(&app, &made);
    assert!((narrow.x0 - (was.x0 + 10.0)).abs() < 0.02 && (narrow.y1 - (was.y1 - 10.0)).abs() < 0.02, "{narrow:?}");
    assert!(order(&app, &made) > order(&app, "back"), "the narrower outline lies on the shape");
    assert_eq!(app.undo.len(), 2);

    // Further inwards than half the shape: nothing is left, nothing is written.
    let src = app.src.clone();
    app.outline_command("back", OutlineCommand::Offset(-150.0));
    assert_eq!(app.src, src);
    assert!(app.status.contains("ничего не осталось"), "{}", app.status);
    app.undo();
    app.undo();
    assert_eq!(app.src, SCENE);
}

#[test]
fn the_contour_section_is_offered_for_every_drawn_object() {
    let app = fixture(SCENE);
    for id in ["box", "wire", "dot"] {
        assert!(app.inspector_categories(id).contains(&Category::Contour), "{id}");
    }
    assert!(!app.inspector_categories("set").contains(&Category::Contour));
}
