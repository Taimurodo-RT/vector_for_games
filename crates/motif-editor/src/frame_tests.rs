//! The transform frame of the selection, driven through the real pointer events.

use super::editing_tests::fixture;
use super::snap_tests::{SCENE, bounds, drag, screen};
use super::*;

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() < 0.011
}

fn assert_box(app: &App, id: &str, want: (f64, f64, f64, f64)) {
    let b = bounds(app, id);
    assert!(near(b.x0, want.0) && near(b.y0, want.1) && near(b.x1, want.2) && near(b.y1, want.3), "{id}: {b:?}, wanted {want:?}\n{}", app.src);
}

fn started(src: &str, selected: &[&str]) -> App {
    let mut app = fixture(src);
    app.snap = crate::snap::Snap::default();
    app.select_only(Some(selected[0].into()));
    for id in &selected[1..] {
        app.toggle_selected((*id).to_string());
    }
    // One frame, so that the canvas has its view before the first press.
    screen(&mut app, 0.0, 0.0);
    app
}

/// Where a point of the object's own space is on the artboard.
fn world(app: &App, id: &str, x: f64, y: f64) -> Point {
    let doc = &app.scene.doc;
    doc.world_transform(doc.index_of(id).unwrap()) * Point::new(x, y)
}

#[test]
fn a_corner_resizes_from_the_opposite_corner_and_with_alt_from_the_middle() {
    // The left top corner of `other` (280, 180) is pulled to 2 points off the level of the
    // bottom of `box` (y = 120), where it sticks.
    let mut app = started(SCENE, &["other"]);
    let from = screen(&mut app, 280.0, 180.0);
    let to = screen(&mut app, 247.0, 120.0) + vec2(0.0, 2.0);
    drag(&mut app, from, to, Modifiers::NONE);
    assert_box(&app, "other", (247.0, 120.0, 320.0, 220.0));
    assert_eq!(app.undo.len(), 1);
    app.undo();
    assert_eq!(app.src, SCENE);

    // Alt on a handle is not the panning gesture: the frame grows around its middle (300, 200).
    let mut app = started(SCENE, &["other"]);
    let pan = app.view.unwrap().pan;
    drag(&mut app, from, to, Modifiers::ALT);
    assert_box(&app, "other", (247.0, 120.0, 353.0, 280.0));
    assert_eq!(app.view.unwrap().pan, pan, "the canvas stays where it was");
}

#[test]
fn a_side_handle_changes_one_size_and_with_shift_keeps_the_proportions() {
    let mut app = started(SCENE, &["box"]);
    let from = screen(&mut app, 130.0, 100.0);
    let to = screen(&mut app, 190.0, 100.0);
    drag(&mut app, from, to, Modifiers::NONE);
    assert_box(&app, "box", (70.0, 80.0, 190.0, 120.0));

    let mut app = started(SCENE, &["box"]);
    drag(&mut app, from, to, Modifiers::SHIFT);
    assert_box(&app, "box", (70.0, 60.0, 190.0, 140.0));

    // A corner with Shift takes the larger stretch for both sides.
    let mut app = started(SCENE, &["box"]);
    let from = screen(&mut app, 130.0, 120.0);
    let to = screen(&mut app, 190.0, 125.0);
    drag(&mut app, from, to, Modifiers::SHIFT);
    assert_box(&app, "box", (70.0, 80.0, 190.0, 160.0));
}

#[test]
fn a_turned_shape_keeps_its_far_side_and_its_angle() {
    let src = "artboard \"a\" 400x300\nrect r at 200 150 size 100 40 rot 30 fill #888888\n";
    let mut app = started(src, &["r"]);
    let far = world(&app, "r", -50.0, 0.0);
    let (held, pulled) = (world(&app, "r", 50.0, 0.0), world(&app, "r", 100.0, 0.0));
    let from = screen(&mut app, held.x, held.y);
    let to = screen(&mut app, pulled.x, pulled.y);
    drag(&mut app, from, to, Modifiers::COMMAND);
    let n = app.scene.doc.node("r").unwrap();
    assert!((n.width - 150.0).abs() < 0.2 && n.height == 40.0 && n.rotation == 30.0, "{}", app.src);
    let now = world(&app, "r", -n.width / 2.0, 0.0);
    assert!(now.distance(far) < 0.02, "the far side moved: {far:?} -> {now:?}");
}

#[test]
fn a_group_is_scaled_and_an_object_in_a_mirrored_group_follows_the_pointer() {
    let src = "artboard \"a\" 400x300\ngroup g at 200 150 {\n  rect a at -20 0 size 40 20 fill #888888\n  rect b at 30 10 size 20 40 fill #888888\n}\n";
    let mut app = started(src, &["g"]);
    assert_box(&app, "g", (160.0, 140.0, 240.0, 180.0));
    let from = screen(&mut app, 240.0, 180.0);
    let to = screen(&mut app, 280.0, 200.0);
    drag(&mut app, from, to, Modifiers::NONE);
    assert_box(&app, "g", (160.0, 140.0, 280.0, 200.0));
    assert!(app.src.contains("scale 1.5 1.5"), "{}", app.src);
    // The children are as they were written.
    assert!(app.src.contains("rect a at -20 0 size 40 20") && app.src.contains("rect b at 30 10 size 20 40"), "{}", app.src);

    // Inside a mirrored group the right side of `a` looks left (160 on the artboard).
    let src = "artboard \"a\" 400x300\ngroup g at 200 150 scale -1 1 {\n  rect a at 20 0 size 40 20 fill #888888\n}\n";
    let mut app = started(src, &["a"]);
    assert_box(&app, "a", (160.0, 140.0, 200.0, 160.0));
    let from = screen(&mut app, 160.0, 150.0);
    let to = screen(&mut app, 140.0, 150.0);
    drag(&mut app, from, to, Modifiers::COMMAND);
    assert_box(&app, "a", (140.0, 140.0, 200.0, 160.0));
    assert!(app.src.contains("rect a at 30 0 size 60 20"), "{}", app.src);
}

#[test]
fn several_objects_are_resized_together_as_one_step() {
    let mut app = started(SCENE, &["box", "other"]);
    // The frame is 70..320 × 80..220; its right bottom corner goes half as far again.
    let from = screen(&mut app, 320.0, 220.0);
    let to = screen(&mut app, 445.0, 290.0);
    drag(&mut app, from, to, Modifiers::COMMAND);
    assert_box(&app, "box", (70.0, 80.0, 160.0, 140.0));
    assert_box(&app, "other", (385.0, 230.0, 445.0, 290.0));
    // Shapes with a size change the size: no scale appears.
    assert!(!app.src.contains("scale"), "{}", app.src);
    assert_eq!(app.undo.len(), 1);
    app.undo();
    assert_eq!(app.src, SCENE);
}

#[test]
fn several_objects_turn_around_the_middle_of_their_frame() {
    let mut app = started(SCENE, &["box", "other"]);
    // The handle above the top side, pulled to the right of the centre (195, 150): a quarter turn.
    let from = screen(&mut app, 195.0, 80.0) + vec2(0.0, -22.0);
    let to = screen(&mut app, 330.0, 152.0);
    drag(&mut app, from, to, Modifiers::SHIFT);
    assert_box(&app, "box", (225.0, 25.0, 265.0, 85.0));
    assert_box(&app, "other", (125.0, 235.0, 165.0, 275.0));
    assert_eq!(app.src.matches("rot 90").count(), 2, "{}", app.src);
    assert_eq!(app.undo.len(), 1);
}

#[test]
fn a_turned_object_in_a_frame_stretched_to_one_side_is_skewed_exactly() {
    let src = "artboard \"a\" 400x300\nrect a at 100 100 size 60 40 fill #888888\nrect t at 300 200 size 60 40 rot 30 fill #888888\n";
    let mut app = started(src, &["a", "t"]);
    let corners = [(-30.0, -20.0), (30.0, -20.0), (30.0, 20.0), (-30.0, 20.0)];
    let before = corners.map(|(x, y)| world(&app, "t", x, y));
    let frame = bounds(&app, "a").union(bounds(&app, "t"));
    let from = screen(&mut app, frame.x1, frame.center().y);
    let to = screen(&mut app, frame.x0 + frame.width() * 1.25, frame.center().y);
    drag(&mut app, from, to, Modifiers::COMMAND);
    // The straight one only got wider; how much tells the stretch.
    let a = bounds(&app, "a");
    let f = (a.x1 - 70.0) / 60.0;
    assert!((f - 1.25).abs() < 0.01 && near(a.x0, 70.0) && a.height() == 40.0, "{a:?}");
    assert!(!app.src.lines().any(|l| l.starts_with("rect a") && (l.contains("skew") || l.contains("scale"))), "{}", app.src);
    // The turned one is where the stretched frame takes each of its corners.
    for ((x, y), was) in corners.into_iter().zip(before) {
        let want = Point::new(70.0 + (was.x - 70.0) * f, was.y);
        let now = world(&app, "t", x, y);
        assert!(now.distance(want) < 0.08, "corner {x} {y}: {now:?}, wanted {want:?}\n{}", app.src);
    }
    assert!(app.src.lines().any(|l| l.starts_with("rect t") && l.contains("skew")), "{}", app.src);
}

#[test]
fn several_objects_resized_in_animation_get_keys_and_the_drawing_stays() {
    let src = format!("{SCENE}anim Grow 1s {{\n  box.opacity 0: 1 | 1: 1\n}}\n");
    let mut app = started(&src, &["box", "other"]);
    app.select_anim("Grow".into());
    app.set_workspace(crate::workspace::Workspace::Animate);
    app.time = 0.5;
    screen(&mut app, 0.0, 0.0);
    let from = screen(&mut app, 320.0, 220.0);
    let to = screen(&mut app, 445.0, 290.0);
    drag(&mut app, from, to, Modifiers::COMMAND);
    // The drawing lines are as they were; the clip got the keys.
    assert!(app.src.starts_with(SCENE), "{}", app.src);
    for track in ["box.w", "box.h", "other.x", "other.y", "other.w", "other.h"] {
        assert!(app.src.lines().any(|l| l.trim_start().starts_with(track) && l.contains("0.5:")), "no key on {track}\n{}", app.src);
    }
    assert_eq!(app.undo.len(), 1, "one step for the whole pull");
}
