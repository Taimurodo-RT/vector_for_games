//! Live repeats in the editor: the commands of the section «Повтор» and the grips of a grid,
//! driven through the real pointer events.

use super::editing_tests::fixture;
use super::snap_tests::{drag, screen};
use super::*;
use motif_script::RepeatKind;

const PANEL: &str = "artboard \"a\" 400x300\nrect panel at 200 150 size 200 100 fill #39404d\nellipse rivet at 120 110 size 8 8 fill #c9ced8\n";

const DOTS: &str = "artboard \"a\" 400x300\ngroup dots at 100 100 repeat grid 3 2 step 30 20 {\n  ellipse dot size 10 10 fill #ffd23f\n}\nanim Blink 1s {\n  dots-1-2-dot.opacity 0: 1 | 1: 0\n  dots-2-1-dot.opacity 0: 1 | 1: 0\n}\n";

fn started(src: &str, selected: &str) -> App {
    let mut app = fixture(src);
    app.snap = crate::snap::Snap::default();
    app.select_only(Some(selected.into()));
    // One frame, so that the canvas has its view before the first press.
    screen(&mut app, 0.0, 0.0);
    app
}

/// Where an object stands on the artboard.
fn at(app: &App, id: &str) -> (f64, f64) {
    let doc = &app.scene.doc;
    let p = doc.world_transform(doc.index_of(id).unwrap_or_else(|| panic!("no {id}\n{}", app.src))).translation();
    ((p.x * 10.0).round() / 10.0, (p.y * 10.0).round() / 10.0)
}

#[test]
fn an_object_goes_into_a_group_that_repeats_it() {
    // A row: the rivet stays where it was drawn, the group is selected, one undo step.
    let mut app = started(PANEL, "rivet");
    app.repeat_selection(1);
    assert!(app.src.contains("group group1 at 120 110 repeat grid 3 1 step 10 10 {"), "{}", app.src);
    assert_eq!(app.selected.as_deref(), Some("group1"));
    assert_eq!((at(&app, "rivet"), at(&app, "group1-3-1-rivet")), ((120.0, 110.0), (140.0, 110.0)));
    app.undo();
    assert_eq!(app.src, PANEL);

    // A ring: its centre is the pivot of the new group, below the rivet.
    let mut app = started(PANEL, "rivet");
    app.repeat_selection(2);
    assert!(app.src.contains("group group1 at 120 110 pivot 0 12 repeat circle 6 {"), "{}", app.src);
    assert_eq!((at(&app, "rivet"), at(&app, "group1-4-rivet")), ((120.0, 110.0), (120.0, 134.0)));

    // Along an outline: the panel the rivet lies on is taken, and gives the group its place.
    let mut app = started(PANEL, "rivet");
    app.repeat_selection(3);
    assert!(app.src.contains("group group1 repeat along panel 8 {"), "{}", app.src);
    assert_eq!((at(&app, "rivet"), at(&app, "group1-2-rivet")), ((100.0, 100.0), (175.0, 100.0)));
    // The outline decides where the group stands: a drag of the group says so and changes nothing.
    let before = app.src.clone();
    app.edit("group1", Prop::X, Value::Num(50.0));
    assert_eq!(app.src, before);
    assert!(app.status.contains("стоит вдоль контура «panel»"), "{}", app.status);
    // The panel grows, the rivets stand along its new edge.
    app.edit("panel", Prop::Width, Value::Num(300.0));
    assert_eq!((at(&app, "rivet"), at(&app, "group1-2-rivet")), ((50.0, 100.0), (150.0, 100.0)));

    // With no shape to go along there is nothing to do, and the editor says what is missing.
    let mut app = started("artboard \"a\" 400x300\nellipse rivet at 120 110 size 8 8 fill #c9ced8\n", "rivet");
    app.repeat_selection(3);
    assert!(app.status.contains("нет фигуры, вдоль которой идти"), "{}", app.status);
}

#[test]
fn the_grips_of_a_grid_change_its_step_and_how_many_there_are() {
    let mut app = started(DOTS, "dots");
    // The grips run above the first dot (its top is y = 95), clear of the knob that turns the
    // frame, and 16 points to the left of it.
    let above = |app: &mut App, x: f64| screen(app, x, 95.0) + vec2(0.0, -40.0);
    let beside = |app: &mut App, y: f64| screen(app, 95.0, y) + vec2(-16.0, 0.0);

    // The diamond over the second column is the step: 30 becomes 44.
    let (from, to) = (above(&mut app, 130.0), above(&mut app, 144.0));
    drag(&mut app, from, to, Modifiers::NONE);
    assert!(app.src.contains("repeat grid 3 2 step 44 20"), "{}", app.src);
    assert_eq!(at(&app, "dots-3-1-dot"), (188.0, 100.0));
    assert_eq!(app.undo.len(), 1, "one drag is one step back");

    // The ring past the last column is how many columns: pulled two steps on, there are five.
    let (from, to) = (above(&mut app, 100.0 + 44.0 * 3.0), above(&mut app, 100.0 + 44.0 * 5.0 + 6.0));
    drag(&mut app, from, to, Modifiers::NONE);
    assert!(app.src.contains("repeat grid 5 2 step 44 20"), "{}", app.src);
    assert_eq!(app.scene.repeats["dots"].clones.len(), 9);
    assert_eq!(app.undo.len(), 2);

    // The ring below the last row, pulled up to the first row: one row. What the animation said
    // about the second row goes with it; the first row keeps its track.
    let (from, to) = (beside(&mut app, 140.0), beside(&mut app, 118.0));
    drag(&mut app, from, to, Modifiers::NONE);
    assert!(app.src.contains("repeat grid 5 1 step 44 20"), "{}", app.src);
    assert!(!app.src.contains("dots-1-2-dot") && app.src.contains("dots-2-1-dot.opacity"), "{}", app.src);
    assert!(app.status.contains("повторов 5") || app.status.contains("Повторов стало 5"), "{}", app.status);
    app.undo();
    assert!(app.src.contains("repeat grid 5 2 step 44 20") && app.src.contains("dots-1-2-dot.opacity"), "{}", app.src);

    // A press anywhere else on the canvas is not a grip: the group is moved as usual.
    let mut app = started(DOTS, "dots");
    let (from, to) = (screen(&mut app, 100.0, 100.0), screen(&mut app, 120.0, 100.0));
    drag(&mut app, from, to, Modifiers::COMMAND);
    assert!(app.src.contains("repeat grid 3 2 step 30 20"), "{}", app.src);
}

#[test]
fn the_panel_changes_the_kind_and_the_numbers_of_a_repeat() {
    let mut app = started(DOTS, "dots");
    // A number scrubbed in the panel: fewer columns.
    assert!(app.set_repeat("dots", Some(RepeatKind::Grid { cols: 2, rows: 2, step: (30.0, 20.0) }), true));
    assert!(app.src.contains("repeat grid 2 2 step 30 20"), "{}", app.src);
    // Another kind: the repeats of the grid go, with what was said about them.
    assert!(app.set_repeat("dots", Some(RepeatKind::Circle { count: 4, arc: 360.0, upright: true, mirror: false }), false));
    assert!(app.src.contains("group dots at 100 100 repeat circle 4 upright {") && !app.src.contains("dots-2-1-dot"), "{}", app.src);
    // No repeat: an ordinary group with the first dot in it.
    assert!(app.set_repeat("dots", None, false));
    assert!(app.src.contains("group dots at 100 100 {") && app.scene.repeats.is_empty(), "{}", app.src);
    assert_eq!(app.scene.doc.nodes.len(), 2);
    // The switch of the panel gives numbers to start with. A circle for a group whose objects
    // sit on its own point brings a centre below them, so the repeats do not turn in place.
    app.switch_repeat("dots", 2);
    assert!(app.src.contains("group dots at 100 100 pivot 0 15 repeat circle 6 {"), "{}", app.src);
    assert_eq!((at(&app, "dot"), at(&app, "dots-4-dot")), ((100.0, 100.0), (100.0, 130.0)));
    // A grid after it takes its step from one dot, not from the whole ring.
    app.switch_repeat("dots", 1);
    assert!(app.src.contains("group dots at 100 100 pivot 0 15 repeat grid 3 1 step 13 13 {"), "{}", app.src);
    app.switch_repeat("dots", 0);
    assert!(app.src.contains("group dots at 100 100 pivot 0 15 {"), "{}", app.src);
    // A made repeat is not a line of the script: it has nothing to set.
    let mut app = started(DOTS, "dots-2-1");
    assert!(!app.set_repeat("dots-2-1", Some(RepeatKind::Circle { count: 4, arc: 360.0, upright: false, mirror: false }), false));
    assert_eq!(app.src, DOTS);
}

#[test]
fn a_mirror_reflects_an_object_in_its_side_and_a_group_beside_its_content() {
    // The button: the half goes into a new group, the line stands on its right side.
    const HALF: &str = "artboard \"a\" 400x300\nrect half at 100 100 size 40 60 fill #c9ced8\n";
    let mut app = started(HALF, "half");
    app.repeat_selection(4);
    assert!(app.src.contains("group group1 at 100 100 pivot 20 30 repeat mirror x {"), "{}", app.src);
    assert_eq!(app.selected.as_deref(), Some("group1"));
    assert_eq!((at(&app, "half"), at(&app, "group1-x-half")), ((100.0, 100.0), (140.0, 100.0)));
    // The second line of the panel: all four quarters around the lower right corner of the half.
    assert!(app.set_repeat("group1", Some(RepeatKind::Mirror { x: true, y: true }), true));
    assert!(app.src.contains("repeat mirror x y {"), "{}", app.src);
    assert_eq!((at(&app, "group1-y-half"), at(&app, "group1-xy-half")), ((100.0, 160.0), (140.0, 160.0)));
    // One step back for the line, one for the mirror.
    app.undo();
    app.undo();
    assert_eq!(app.src, HALF);

    // The switch for a group: its objects lie over its own point, so the line goes to their side.
    const GROUP: &str = "artboard \"a\" 400x300\ngroup wing at 100 100 {\n  rect feather size 40 10 fill #c9ced8\n}\n";
    let mut app = started(GROUP, "wing");
    app.switch_repeat("wing", 4);
    assert!(app.src.contains("group wing at 100 100 pivot 20 5 repeat mirror x {"), "{}", app.src);
    assert_eq!(at(&app, "wing-x-feather"), (140.0, 100.0));
    // Objects drawn beside the point of the group are reflected in the line through it.
    let beside = GROUP.replace("rect feather size", "rect feather at -30 0 size");
    let mut app = started(&beside, "wing");
    app.switch_repeat("wing", 4);
    assert!(app.src.contains("group wing at 100 100 repeat mirror x {"), "{}", app.src);
    assert_eq!((at(&app, "feather"), at(&app, "wing-x-feather")), ((70.0, 100.0), (130.0, 100.0)));
    // A circle whose repeats are reflected: twice as many, the numbered ones keep their ids.
    app.switch_repeat("wing", 2);
    assert!(app.set_repeat("wing", Some(RepeatKind::Circle { count: 3, arc: 360.0, upright: false, mirror: true }), false));
    assert!(app.src.contains("repeat circle 3 mirror {"), "{}", app.src);
    assert_eq!(app.scene.repeats["wing"].clones, ["wing-1m", "wing-2", "wing-2m", "wing-3", "wing-3m"]);
}
