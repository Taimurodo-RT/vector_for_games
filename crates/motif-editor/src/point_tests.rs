//! Several points of an outline selected and edited together, driven through the real events.

use super::editing_tests::fixture;
use super::snap_tests::{click, drag, frame, screen};
use super::*;
use crate::compound::{Anchor, Compound};
use crate::selection::Align;

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

const HOLE: [(f64, f64); 4] = [(8.0, -20.0), (30.0, -20.0), (30.0, 4.0), (8.0, 4.0)];

/// Point editing of the ring with all four points of the right hole selected.
fn hole_selected(src: &str) -> App {
    let mut app = editing(src);
    for (k, (x, y)) in HOLE.into_iter().enumerate() {
        let p = at(&mut app, x, y);
        click(&mut app, p, if k == 0 { Modifiers::NONE } else { Modifiers::SHIFT });
    }
    assert_eq!(picked(&app), vec![a(2, 0), a(2, 1), a(2, 2), a(2, 3)]);
    app
}

/// Where a handle of the frame around the selected points is on the screen: the numbers are
/// the sides it pulls, `None` is the handle that turns.
fn handle(app: &mut App, which: Option<(i8, i8)>) -> Pos2 {
    frame(app, vec![], Modifiers::NONE);
    let doc = &app.scene.doc;
    let i = doc.index_of("ring").unwrap();
    let world = doc.world_transform(i);
    let shape = Compound::from_bez(&doc.nodes[i].editable_path()).unwrap();
    let b = crate::point_frame::bounds(&shape, &app.point_edit.as_ref().unwrap().anchors()).expect("the points have a frame");
    let (view, rect) = (app.view.unwrap(), app.canvas_bounds);
    let h = crate::point_frame::handles(b, |p| view.to_screen(rect, world * p));
    match which {
        None => h.rotate,
        Some((0, -1)) => h.sides[0].unwrap(),
        Some((1, 0)) => h.sides[1].unwrap(),
        Some((0, 1)) => h.sides[2].unwrap(),
        Some((-1, 0)) => h.sides[3].unwrap(),
        Some((-1, -1)) => h.corners[0],
        Some((1, -1)) => h.corners[1],
        Some((1, 1)) => h.corners[2],
        Some(_) => h.corners[3],
    }
}

/// Pulls a handle of the frame as far as the way from one point of the ring's space to another.
fn pull(app: &mut App, which: Option<(i8, i8)>, from: (f64, f64), to: (f64, f64), modifiers: Modifiers) {
    let grip = handle(app, which);
    let step = at(app, to.0, to.1) - at(app, from.0, from.1);
    drag(app, grip, grip + step, modifiers);
}

#[test]
fn a_side_of_the_frame_stretches_the_points_and_keeps_the_opposite_side() {
    let mut app = hole_selected(RING);
    pull(&mut app, Some((1, 0)), (30.0, 0.0), (41.0, 0.0), Modifiers::NONE);
    assert_eq!(points(&app, 2), vec![(8.0, -20.0), (41.0, -20.0), (41.0, 4.0), (8.0, 4.0)]);
    assert_eq!(points(&app, 1), vec![(-30.0, -8.0), (-6.0, -8.0), (-6.0, 16.0), (-30.0, 16.0)]);
    assert!(app.src.contains(&format!("d \"{OUTER} M -30 -8 L -6 -8 L -6 16 L -30 16 Z M ")), "the other parts are written as they were:\n{}", app.src);
    assert_eq!(picked(&app).len(), 4, "the points stay selected");
    assert_eq!(app.undo.len(), 1);
    app.undo();
    assert_eq!(app.src, RING);

    // Alt: around the middle (x = 19), both sides go apart.
    let mut app = hole_selected(RING);
    pull(&mut app, Some((1, 0)), (30.0, 0.0), (41.0, 0.0), Modifiers::ALT);
    assert_eq!(points(&app, 2), vec![(-3.0, -20.0), (41.0, -20.0), (41.0, 4.0), (-3.0, 4.0)]);

    // Shift on a corner: the larger of the two stretches for both sides.
    let mut app = hole_selected(RING);
    pull(&mut app, Some((1, 1)), (30.0, 4.0), (41.0, 7.0), Modifiers::SHIFT);
    assert_eq!(points(&app, 2), vec![(8.0, -20.0), (41.0, -20.0), (41.0, 16.0), (8.0, 16.0)]);
}

#[test]
fn the_frame_turns_the_points_around_its_middle() {
    // From the handle above the middle (19, -8) to the right of it: a quarter turn with Shift.
    let mut app = hole_selected(RING);
    let from = handle(&mut app, None);
    let to = at(&mut app, 60.0, -9.5);
    drag(&mut app, from, to, Modifiers::SHIFT);
    assert_eq!(points(&app, 2), vec![(31.0, -19.0), (31.0, 3.0), (7.0, 3.0), (7.0, -19.0)]);
    assert_eq!(points(&app, 1), vec![(-30.0, -8.0), (-6.0, -8.0), (-6.0, 16.0), (-30.0, 16.0)]);
    assert_eq!(app.undo.len(), 1);
}

#[test]
fn curve_handles_are_stretched_with_their_points() {
    let mut app = editing(RING);
    key(&mut app, Key::A, Modifiers::COMMAND);
    // The right bottom corner towards the middle of the ring, to a half, with Alt and Shift.
    pull(&mut app, Some((1, 1)), (50.0, 50.0), (25.0, 25.0), Modifiers::ALT | Modifiers::SHIFT);
    let half = "M -25 0 C -25 -14 -14 -25 0 -25 C 14 -25 25 -14 25 0 C 25 14 14 25 0 25 C -14 25 -25 14 -25 0 Z M -15 -4 L -3 -4 L -3 8 L -15 8 Z M 4 -10 L 15 -10 L 15 2 L 4 2 Z";
    assert!(app.src.contains(half), "{}", app.src);
}

#[test]
fn the_frame_follows_a_turned_and_scaled_outline() {
    // The frame lies along the outline's own axes: its "right" side is at the bottom of the screen here.
    let src = RING.replace("at 200 150", "at 200 150 rot 90 scale 2");
    let mut app = hole_selected(&src);
    let fixed = at(&mut app, 8.0, 4.0);
    pull(&mut app, Some((1, 0)), (30.0, 0.0), (41.0, 0.0), Modifiers::NONE);
    assert_eq!(points(&app, 2), vec![(8.0, -20.0), (41.0, -20.0), (41.0, 4.0), (8.0, 4.0)]);
    assert_eq!(at(&mut app, 8.0, 4.0), fixed);
    // A quarter turn as the eye sees it stays a quarter turn of the points.
    let from = handle(&mut app, None);
    let centre = at(&mut app, 24.5, -8.0);
    let to = centre + (from - centre).rot90();
    drag(&mut app, from, to, Modifiers::SHIFT);
    assert_eq!(points(&app, 2), vec![(12.5, 8.5), (12.5, -24.5), (36.5, -24.5), (36.5, 8.5)]);
}

#[test]
fn a_click_on_a_handle_of_the_frame_changes_nothing() {
    let mut app = hole_selected(RING);
    for which in [Some((1, 0)), Some((-1, -1)), None] {
        let p = handle(&mut app, which);
        click(&mut app, p, Modifiers::NONE);
        assert_eq!(app.src, RING);
        assert_eq!(picked(&app).len(), 4, "editing goes on with the same points");
    }
    assert_eq!(app.undo.len(), 0);
    // One point has no frame.
    let p = at(&mut app, 8.0, 4.0);
    click(&mut app, p, Modifiers::NONE);
    let shape = Compound::from_bez(&app.scene.doc.node("ring").unwrap().editable_path()).unwrap();
    assert!(crate::point_frame::bounds(&shape, &app.point_edit.as_ref().unwrap().anchors()).is_none());
}

#[test]
fn points_are_lined_up_and_spread_as_the_eye_sees_them() {
    let mut app = editing(RING);
    let from = at(&mut app, -18.0, 30.0);
    let to = at(&mut app, 19.0, -14.0);
    drag(&mut app, from, to, Modifiers::NONE);
    app.align_points(Align::Left);
    assert_eq!(points(&app, 1), vec![(-30.0, -8.0), (-6.0, -8.0), (-6.0, 16.0), (-30.0, 16.0)]);
    assert_eq!(points(&app, 2), vec![(8.0, -20.0), (30.0, -20.0), (30.0, 4.0), (-6.0, 4.0)]);
    assert!(app.src.contains(&format!("d \"{OUTER} M -30 -8 L -6 -8 L -6 16 L -30 16 Z M ")));
    assert_eq!(app.undo.len(), 1);
    let before = app.src.clone();
    app.align_points(Align::Left);
    assert_eq!((app.src.as_str(), app.undo.len()), (before.as_str(), 1), "nothing to do the second time");
    app.undo();
    assert_eq!(app.src, RING);

    // Turned a quarter: the left of the screen is the bottom of the outline's own space.
    let mut app = editing(&RING.replace("at 200 150", "at 200 150 rot 90"));
    let from = at(&mut app, -18.0, 30.0);
    let to = at(&mut app, 19.0, -14.0);
    drag(&mut app, from, to, Modifiers::NONE);
    assert_eq!(picked(&app), vec![a(1, 1), a(1, 2), a(2, 3)]);
    app.align_points(Align::Left);
    assert_eq!(points(&app, 1), vec![(-30.0, -8.0), (-6.0, 16.0), (-6.0, 16.0), (-30.0, 16.0)]);
    assert_eq!(points(&app, 2)[3], (8.0, 16.0));

    // Three points at equal steps between the outer two.
    let mut app = editing(RING);
    for (k, (x, y)) in [(-30.0, -8.0), (-6.0, -8.0), (30.0, -20.0)].into_iter().enumerate() {
        let p = at(&mut app, x, y);
        click(&mut app, p, if k == 0 { Modifiers::NONE } else { Modifiers::SHIFT });
    }
    app.align_points(Align::SpreadX);
    assert_eq!(points(&app, 1), vec![(-30.0, -8.0), (0.0, -8.0), (-6.0, 16.0), (-30.0, 16.0)]);
    assert_eq!(points(&app, 2), vec![(8.0, -20.0), (30.0, -20.0), (30.0, 4.0), (8.0, 4.0)]);
}
