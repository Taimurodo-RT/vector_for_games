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

/// The corners of a 2w × 2h box around the object's own origin, on the artboard.
fn corners(app: &App, id: &str, w: f64, h: f64) -> [Point; 4] {
    [(-w, -h), (w, -h), (w, h), (-w, h)].map(|(x, y)| world(app, id, x, y))
}

fn double_click(app: &mut App, p: Pos2) {
    for _ in 0..2 {
        super::snap_tests::frame(app, vec![egui::Event::PointerMoved(p), super::snap_tests::button(p, true, Modifiers::NONE)], Modifiers::NONE);
        super::snap_tests::frame(app, vec![super::snap_tests::button(p, false, Modifiers::NONE)], Modifiers::NONE);
    }
}

#[test]
fn the_pivot_moves_to_a_corner_of_the_frame_and_the_object_stays() {
    let src = "artboard \"a\" 400x300\nrect r at 200 150 size 100 40 rot 30 fill #888888\n";
    let mut app = started(src, &["r"]);
    app.pivot_mode = true;
    let before = corners(&app, "r", 50.0, 20.0);
    // From the mark in the middle to 3 points off the right bottom corner: it sticks to the corner.
    let from = screen(&mut app, 200.0, 150.0);
    let to = screen(&mut app, before[2].x, before[2].y) + vec2(2.0, -2.0);
    drag(&mut app, from, to, Modifiers::NONE);
    assert!(app.src.contains("pivot 50 20"), "{}", app.src);
    for (was, now) in before.into_iter().zip(corners(&app, "r", 50.0, 20.0)) {
        assert!(was.distance(now) < 0.02, "the object moved: {was:?} -> {now:?}\n{}", app.src);
    }
    assert_eq!(app.undo.len(), 1, "the pivot and the position are one step");

    // The object now turns around that corner: the corner stays, the rest goes around it.
    let n = app.scene.doc.node("r").unwrap().clone();
    app.edit("r", Prop::Rotation, Value::Num(n.rotation + 90.0));
    let turned = corners(&app, "r", 50.0, 20.0);
    assert!(turned[2].distance(before[2]) < 0.02 && turned[0].distance(before[0]) > 50.0);
    app.undo();

    // A double click on the mark puts it back; the object stays again.
    let mark = screen(&mut app, before[2].x, before[2].y);
    double_click(&mut app, mark);
    assert!(!app.src.contains("pivot"), "{}", app.src);
    for (was, now) in before.into_iter().zip(corners(&app, "r", 50.0, 20.0)) {
        assert!(was.distance(now) < 0.02, "the object moved: {was:?} -> {now:?}\n{}", app.src);
    }
    app.undo();
    app.undo();
    assert_eq!(app.src, src);
}

#[test]
fn the_pivot_is_right_in_a_turned_mirrored_group_with_a_negative_scale() {
    let src = "artboard \"a\" 400x300\ngroup g at 200 150 rot 15 scale -1 1 {\n  rect a at 20 0 size 40 20 rot 10 scale 1 -2 fill #888888\n}\n";
    let mut app = started(src, &["a"]);
    app.pivot_mode = true;
    let before = corners(&app, "a", 20.0, 10.0);
    let centre = world(&app, "a", 0.0, 0.0);
    let from = screen(&mut app, centre.x, centre.y);
    let to = screen(&mut app, before[0].x, before[0].y);
    drag(&mut app, from, to, Modifiers::NONE);
    assert!(app.src.contains("pivot -20 -10"), "{}", app.src);
    for (was, now) in before.into_iter().zip(corners(&app, "a", 20.0, 10.0)) {
        assert!(was.distance(now) < 0.03, "the object moved: {was:?} -> {now:?}\n{}", app.src);
    }
}

#[test]
fn the_pivot_is_part_of_the_drawing_and_is_not_moved_while_a_clip_is_keyed() {
    let src = format!("{SCENE}anim Grow 1s {{\n  box.opacity 0: 1 | 1: 1\n}}\n");
    let mut app = started(&src, &["box"]);
    app.pivot_mode = true;
    app.select_anim("Grow".into());
    app.set_workspace(crate::workspace::Workspace::Animate);
    screen(&mut app, 0.0, 0.0);
    let from = screen(&mut app, 100.0, 100.0);
    let to = screen(&mut app, 130.0, 120.0);
    drag(&mut app, from, to, Modifiers::NONE);
    assert_eq!(app.src, src);
    assert!(app.status.contains("Опорная точка"), "{}", app.status);
}

#[test]
fn a_frame_pull_keeps_the_far_side_of_an_object_with_a_moved_pivot() {
    let src = "artboard \"a\" 400x300\nrect r at 200 150 size 100 40 rot 30 pivot 50 20 fill #888888\n";
    let mut app = started(src, &["r"]);
    let far = world(&app, "r", -50.0, 0.0);
    let (held, pulled) = (world(&app, "r", 50.0, 0.0), world(&app, "r", 90.0, 0.0));
    let from = screen(&mut app, held.x, held.y);
    let to = screen(&mut app, pulled.x, pulled.y);
    drag(&mut app, from, to, Modifiers::COMMAND);
    let n = app.scene.doc.node("r").unwrap();
    assert!((n.width - 140.0).abs() < 0.2, "{}", app.src);
    let now = world(&app, "r", -n.width / 2.0, 0.0);
    assert!(now.distance(far) < 0.02, "the far side moved: {far:?} -> {now:?}\n{}", app.src);
}

#[test]
fn a_drag_tells_its_shift_at_the_pointer_and_forgets_it_on_release() {
    use super::snap_tests::{button, frame};
    let mut app = started(SCENE, &["box"]);
    // Off the centre: the pivot mark is in the middle.
    let from = screen(&mut app, 85.0, 90.0);
    let to = screen(&mut app, 105.0, 96.0);
    let free = Modifiers::COMMAND;
    frame(&mut app, vec![egui::Event::PointerMoved(from), button(from, true, free)], free);
    frame(&mut app, vec![egui::Event::PointerMoved(from.lerp(to, 0.5))], free);
    frame(&mut app, vec![egui::Event::PointerMoved(to)], free);
    assert_eq!(app.drag_label.as_deref(), Some("Δ 20 · 6"));
    frame(&mut app, vec![button(to, false, free)], free);
    assert_eq!(app.drag_label, None);
    assert_box(&app, "box", (90.0, 86.0, 150.0, 126.0));
}

#[test]
fn a_selected_object_is_moved_by_its_middle_until_the_pivot_mode_is_on() {
    // The mark sits in the middle, where the object is grabbed: without the mode it does not take the mouse.
    let mut app = started(SCENE, &["box"]);
    let from = screen(&mut app, 100.0, 100.0);
    let to = screen(&mut app, 120.0, 106.0);
    drag(&mut app, from, to, Modifiers::COMMAND);
    assert_box(&app, "box", (90.0, 86.0, 150.0, 126.0));
    assert!(!app.src.contains("pivot"), "{}", app.src);
}

#[test]
fn a_group_is_framed_with_the_groups_inside_it_and_without_what_is_hidden() {
    // The objects of the copy `tag` sit in a group of the symbol, as they do under a `stack`
    // layout: the copy itself has no shape of its own. `extra` is shown by no variant here.
    const TAGS: &str = "artboard \"a\" 400x300\nsymbol Tag {\n  group row {\n    rect a at -20 0 size 20 10 fill #888888\n    rect b at 30 0 size 40 20 fill #888888\n    group extra at 0 -40 hidden {\n      rect c size 10 10 fill #888888\n    }\n  }\n}\nuse Tag tag at 100 100\ngroup outer at 300 200 {\n  rect own size 10 10 fill #888888\n  group inner at 40 0 {\n    rect deep size 20 30 fill #888888\n  }\n}\n";
    let app = started(TAGS, &["tag"]);
    assert_box(&app, "tag", (70.0, 90.0, 150.0, 110.0));
    assert_box(&app, "tag-row", (70.0, 90.0, 150.0, 110.0));
    assert!(app.transform_frame(&app.scene.doc).is_some(), "the copy has its frame");
    // A group with a shape of its own and a group: the frame is around both.
    assert_box(&app, "outer", (295.0, 185.0, 350.0, 215.0));
    // Only hidden things inside: the frame is still there, around them.
    assert_box(&app, "tag-extra", (95.0, 55.0, 105.0, 65.0));

    // The frame is a working one: its corner scales the copy and the group, the far corner stays.
    let mut app = started(TAGS, &["tag"]);
    let from = screen(&mut app, 150.0, 110.0);
    let to = screen(&mut app, 190.0, 120.0);
    drag(&mut app, from, to, Modifiers::NONE);
    assert_box(&app, "tag", (70.0, 90.0, 190.0, 120.0));
    assert!(app.src.contains("scale 1.5 1.5"), "{}", app.src);
    let mut app = started(TAGS, &["outer"]);
    let from = screen(&mut app, 350.0, 215.0);
    let to = screen(&mut app, 361.0, 221.0);
    drag(&mut app, from, to, Modifiers::NONE);
    assert_box(&app, "outer", (295.0, 185.0, 361.0, 221.0));
    assert!(app.src.contains("scale 1.2 1.2"), "{}", app.src);
}
