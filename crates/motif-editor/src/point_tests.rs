//! Several points of an outline selected and edited together, driven through the real events.

use super::editing_tests::fixture;
use super::snap_tests::{click, drag, screen};
use super::*;
use crate::compound::{Anchor, Compound};

const OUTER: &str = "M -50 0 C -50 -28 -28 -50 0 -50 C 28 -50 50 -28 50 0 C 50 28 28 50 0 50 C -28 50 -50 28 -50 0 Z";
const RING: &str = "artboard \"a\" 400x300\npath ring at 200 150 d \"M -50 0 C -50 -28 -28 -50 0 -50 C 28 -50 50 -28 50 0 C 50 28 28 50 0 50 C -28 50 -50 28 -50 0 Z M -30 -8 L -6 -8 L -6 16 L -30 16 Z M 8 -20 L 30 -20 L 30 4 L 8 4 Z\" evenodd fill #4fd38c\nrect other at 60 60 size 40 40 fill #888888\n";

fn a(part: usize, vertex: usize) -> Anchor {
    Anchor { part, vertex }
}

fn editing(src: &str) -> App {
    let mut app = fixture(src);
    app.snap = crate::snap::Snap::default();
    app.select_only(Some("ring".into()));
    app.begin_point_edit();
    // One frame, so that the canvas has its view before the first press.
    screen(&mut app, 0.0, 0.0);
    app
}

/// Where a point of the ring's own space is on the screen.
fn at(app: &mut App, x: f64, y: f64) -> Pos2 {
    let doc = &app.scene.doc;
    let p = doc.world_transform(doc.index_of("ring").unwrap()) * Point::new(x, y);
    screen(app, p.x, p.y)
}

fn picked(app: &App) -> Vec<Anchor> {
    let mut all = app.point_edit.as_ref().expect("points are still edited").anchors();
    all.sort_by_key(|a| (a.part, a.vertex));
    all
}

fn points(app: &App, part: usize) -> Vec<(f64, f64)> {
    let shape = Compound::from_bez(&app.scene.doc.node("ring").unwrap().editable_path()).unwrap();
    shape.parts[part].outline.verts.iter().map(|v| (v.p.x, v.p.y)).collect()
}

fn parts(app: &App) -> usize {
    Compound::from_bez(&app.scene.doc.node("ring").unwrap().editable_path()).unwrap().parts.len()
}

fn key(app: &mut App, key: Key, modifiers: Modifiers) {
    let ctx = app.ctx.clone();
    let events = vec![egui::Event::ModifiersChanged(modifiers), egui::Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers }];
    ctx.run_ui(egui::RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1100.0, 850.0))), events, ..Default::default() }, |ui| {
        app.shortcuts(&ctx);
        app.canvas_ui(ui, None);
    })
    .drop_without_applying_deltas();
}

#[test]
fn a_frame_takes_points_of_two_holes_and_they_move_together() {
    let mut app = editing(RING);
    // From the body of the ring under the left hole to the inside of the right hole.
    let from = at(&mut app, -18.0, 30.0);
    let to = at(&mut app, 19.0, -14.0);
    drag(&mut app, from, to, Modifiers::NONE);
    assert_eq!(picked(&app), vec![a(1, 1), a(1, 2), a(2, 3)]);
    assert_eq!(app.undo.len(), 0, "a selection is not a change of the file");
    assert_eq!(app.src, RING);

    // Any selected point pulls all of them; the handles and the other points stay.
    let from = at(&mut app, 8.0, 4.0);
    let to = at(&mut app, 18.0, 10.0);
    drag(&mut app, from, to, Modifiers::NONE);
    assert_eq!(points(&app, 1), vec![(-30.0, -8.0), (4.0, -2.0), (4.0, 22.0), (-30.0, 16.0)]);
    assert_eq!(points(&app, 2), vec![(8.0, -20.0), (30.0, -20.0), (30.0, 4.0), (18.0, 10.0)]);
    assert!(app.src.contains(&format!("d \"{OUTER} M ")), "the outer contour is written as it was:\n{}", app.src);
    assert!(app.src.ends_with("rect other at 60 60 size 40 40 fill #888888\n"));
    assert_eq!(picked(&app), vec![a(1, 1), a(1, 2), a(2, 3)], "the points stay selected after the move");
    assert_eq!(app.undo.len(), 1, "one move is one undo step");
    app.undo();
    assert_eq!(app.src, RING);
}

#[test]
fn shift_adds_and_removes_points_and_a_plain_click_takes_one() {
    let mut app = editing(RING);
    let first = at(&mut app, -30.0, -8.0);
    let second = at(&mut app, 30.0, 4.0);
    let third = at(&mut app, 0.0, -50.0);
    click(&mut app, first, Modifiers::NONE);
    click(&mut app, second, Modifiers::SHIFT);
    click(&mut app, third, Modifiers::SHIFT);
    assert_eq!(picked(&app), vec![a(0, 1), a(1, 0), a(2, 2)]);
    assert_eq!(app.point_edit.as_ref().unwrap().selected, Some(a(0, 1)), "the last one is the active point");
    click(&mut app, second, Modifiers::SHIFT);
    assert_eq!(picked(&app), vec![a(0, 1), a(1, 0)]);

    // A frame with Shift adds to what is selected: the whole right hole.
    let from = at(&mut app, 3.0, -25.0);
    let to = at(&mut app, 36.0, 9.0);
    drag(&mut app, from, to, Modifiers::SHIFT);
    assert_eq!(picked(&app), vec![a(0, 1), a(1, 0), a(2, 0), a(2, 1), a(2, 2), a(2, 3)]);

    // A point outside the selection is taken alone, and a drag moves only it.
    let from = at(&mut app, -6.0, 16.0);
    let to = at(&mut app, -2.0, 20.0);
    drag(&mut app, from, to, Modifiers::NONE);
    assert_eq!(picked(&app), vec![a(1, 2)]);
    assert_eq!(points(&app, 1), vec![(-30.0, -8.0), (-6.0, -8.0), (-2.0, 20.0), (-30.0, 16.0)]);
    assert_eq!(points(&app, 2), vec![(8.0, -20.0), (30.0, -20.0), (30.0, 4.0), (8.0, 4.0)]);
    click(&mut app, first, Modifiers::NONE);
    assert_eq!(picked(&app), vec![a(1, 0)]);
    assert_eq!(app.src.matches(OUTER).count(), 1);
}

#[test]
fn ctrl_a_selects_every_point_and_the_arrows_move_the_selected_ones() {
    let mut app = editing(RING);
    key(&mut app, Key::A, Modifiers::COMMAND);
    assert_eq!(picked(&app).len(), 12);
    assert_eq!(app.selection(), vec!["ring".to_string()], "Ctrl+A stays inside the outline");

    let from = at(&mut app, -18.0, 30.0);
    let to = at(&mut app, 19.0, -14.0);
    drag(&mut app, from, to, Modifiers::NONE);
    assert_eq!(picked(&app), vec![a(1, 1), a(1, 2), a(2, 3)]);
    let x = app.scene.doc.node("ring").unwrap().x;
    key(&mut app, Key::ArrowRight, Modifiers::NONE);
    key(&mut app, Key::ArrowRight, Modifiers::NONE);
    key(&mut app, Key::ArrowUp, Modifiers::SHIFT);
    assert_eq!(points(&app, 1), vec![(-30.0, -8.0), (-4.0, -18.0), (-4.0, 6.0), (-30.0, 16.0)]);
    assert_eq!(points(&app, 2), vec![(8.0, -20.0), (30.0, -20.0), (30.0, 4.0), (10.0, -6.0)]);
    assert_eq!(app.scene.doc.node("ring").unwrap().x, x, "the object itself stays");
    assert!(app.src.contains(&format!("d \"{OUTER} M ")));
    assert_eq!(app.undo.len(), 1, "a run of arrow presses is one undo step");
    app.undo();
    assert_eq!(app.src, RING);
}

#[test]
fn arrows_move_points_by_what_the_eye_sees_in_a_turned_and_scaled_outline() {
    // Turned a quarter and twice as large: one unit to the right on the artboard is half a
    // unit up in the outline's own space.
    let src = RING.replace("at 200 150", "at 200 150 rot 90 scale 2");
    let mut app = editing(&src);
    let p = at(&mut app, 30.0, 4.0);
    click(&mut app, p, Modifiers::NONE);
    assert_eq!(picked(&app), vec![a(2, 2)]);
    key(&mut app, Key::ArrowRight, Modifiers::SHIFT);
    assert_eq!(points(&app, 2)[2], (30.0, -1.0));
}

#[test]
fn delete_takes_all_selected_points_and_a_whole_hole_with_them() {
    let mut app = editing(RING);
    // The whole left hole and one point of the right one.
    let from = at(&mut app, -36.0, -13.0);
    let to = at(&mut app, 12.0, 20.0);
    drag(&mut app, from, to, Modifiers::NONE);
    assert_eq!(picked(&app), vec![a(1, 0), a(1, 1), a(1, 2), a(1, 3), a(2, 3)]);
    key(&mut app, Key::Delete, Modifiers::NONE);
    assert_eq!(parts(&app), 2, "the hole with every point selected is gone");
    assert_eq!(points(&app, 1), vec![(8.0, -20.0), (30.0, -20.0), (30.0, 4.0)]);
    assert!(app.src.contains(&format!("d \"{OUTER} M ")));
    assert!(picked(&app).is_empty());
    assert_eq!(app.undo.len(), 1);

    // Two of three points: the part would be left with one, so nothing is deleted.
    let first = at(&mut app, 8.0, -20.0);
    let second = at(&mut app, 30.0, -20.0);
    click(&mut app, first, Modifiers::NONE);
    click(&mut app, second, Modifiers::SHIFT);
    let before = app.src.clone();
    key(&mut app, Key::Delete, Modifiers::NONE);
    assert_eq!(app.src, before);
    assert!(app.status.contains("хотя бы две точки"), "{}", app.status);
    assert!(app.scene.doc.node("ring").is_some(), "Delete never removes the object while its points are edited");

    // Every point of the outline: refused as well.
    key(&mut app, Key::A, Modifiers::COMMAND);
    key(&mut app, Key::Delete, Modifiers::NONE);
    assert_eq!(app.src, before);
    app.undo();
    assert_eq!(app.src, RING);
}

#[test]
fn points_moved_together_in_an_animation_write_one_contour_key_and_keep_the_drawing() {
    let src = format!("{RING}anim Idle 1s loop {{\n  ring.rot 0: 0 | 1: 0\n}}\n");
    let mut app = editing(&src);
    app.set_workspace(crate::workspace::Workspace::Animate);
    app.anim = Some("Idle".into());
    app.time = 0.5;
    app.playing = false;
    app.select_only(Some("ring".into()));
    app.begin_point_edit();
    screen(&mut app, 0.0, 0.0);
    let from = at(&mut app, -18.0, 30.0);
    let to = at(&mut app, 19.0, -14.0);
    drag(&mut app, from, to, Modifiers::NONE);
    assert_eq!(picked(&app).len(), 3);
    let from = at(&mut app, 8.0, 4.0);
    let to = at(&mut app, 18.0, 10.0);
    drag(&mut app, from, to, Modifiers::NONE);
    let base = motif_script::parse(&app.src).unwrap();
    let shape = Compound::from_bez(&base.doc.node("ring").unwrap().geometry).unwrap();
    assert_eq!(shape.parts[2].outline.verts[3].p, Point::new(8.0, 4.0), "the drawing itself stays:\n{}", app.src);
    assert!(app.src.contains("ring.contour"), "the move is a key of the clip:\n{}", app.src);
    assert_eq!(app.undo.len(), 1);
}
