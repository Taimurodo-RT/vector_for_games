//! Snapping of points placed on the canvas, driven through the real pointer events.

use super::editing_tests::fixture;
use super::*;

pub(super) const SCENE: &str = "artboard \"a\" 400x300\nrect box at 100 100 size 60 40 fill #888888\nrect other at 300 200 size 40 40 fill #888888\n";

pub(super) fn frame(app: &mut App, mut events: Vec<egui::Event>, modifiers: Modifiers) {
    let ctx = app.ctx.clone();
    events.insert(0, egui::Event::ModifiersChanged(modifiers));
    ctx.run_ui(egui::RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1100.0, 850.0))), events, ..Default::default() }, |ui| {
        app.canvas_ui(ui, None);
    })
    .drop_without_applying_deltas();
}

pub(super) fn screen(app: &mut App, x: f64, y: f64) -> Pos2 {
    frame(app, vec![], Modifiers::NONE);
    app.view.unwrap().to_screen(app.canvas_bounds, Point::new(x, y))
}

pub(super) fn button(p: Pos2, pressed: bool, modifiers: Modifiers) -> egui::Event {
    egui::Event::PointerButton { pos: p, button: egui::PointerButton::Primary, pressed, modifiers }
}

pub(super) fn drag(app: &mut App, from: Pos2, to: Pos2, modifiers: Modifiers) {
    frame(app, vec![egui::Event::PointerMoved(from), button(from, true, modifiers)], modifiers);
    frame(app, vec![egui::Event::PointerMoved(from.lerp(to, 0.5))], modifiers);
    frame(app, vec![egui::Event::PointerMoved(to)], modifiers);
    frame(app, vec![button(to, false, modifiers)], modifiers);
}

pub(super) fn click(app: &mut App, p: Pos2, modifiers: Modifiers) {
    frame(app, vec![egui::Event::PointerMoved(p), button(p, true, modifiers)], modifiers);
    frame(app, vec![button(p, false, modifiers)], modifiers);
}

/// Bounds of the object in artboard space.
pub(super) fn bounds(app: &App, id: &str) -> motif_core::kurbo::Rect {
    let doc = &app.scene.doc;
    crate::selection::world_bounds(doc, doc.index_of(id).unwrap()).unwrap()
}

pub(super) fn new_id(app: &App, before: &[String]) -> String {
    app.scene.doc.nodes.iter().map(|n| n.id.clone()).find(|id| !before.contains(id)).expect("a new object")
}

pub(super) fn ids(app: &App) -> Vec<String> {
    app.scene.doc.nodes.iter().map(|n| n.id.clone()).collect()
}

#[test]
fn a_new_shape_lands_edge_to_edge_on_its_neighbour_and_ctrl_places_it_freely() {
    for free in [false, true] {
        let mut app = fixture(SCENE);
        app.snap = crate::snap::Snap::default();
        app.tool = Tool::Create(ShapeKind::Rect);
        let before = ids(&app);
        // The drag starts 2 screen points off the right top corner of `box` (130, 80) and ends
        // 2 points short of the level of its bottom edge.
        let from = screen(&mut app, 130.0, 80.0) + vec2(2.0, 2.0);
        let to = screen(&mut app, 190.0, 120.0) + vec2(0.0, -2.0);
        let modifiers = if free { Modifiers::COMMAND } else { Modifiers::NONE };
        drag(&mut app, from, to, modifiers);
        let b = bounds(&app, &new_id(&app, &before));
        if free {
            assert!((b.x0 - 130.0).abs() > 0.05 && (b.y0 - 80.0).abs() > 0.05, "Ctrl must leave the corner under the pointer: {b:?}");
        } else {
            assert_eq!((b.x0, b.y0, b.x1, b.y1), (130.0, 80.0, 190.0, 120.0));
        }
        assert_eq!(app.undo.len(), 1, "one shape is one undo step");
        assert!(app.guides.is_empty() && app.snap_mark.is_none(), "guides go away with the drag");
    }
}

#[test]
fn a_new_square_grows_from_the_corner_where_the_drag_started() {
    let mut app = fixture(SCENE);
    app.snap = crate::snap::Snap::default();
    app.tool = Tool::Create(ShapeKind::Rect);
    let before = ids(&app);
    let from = screen(&mut app, 130.0, 80.0);
    let to = screen(&mut app, 180.0, 60.0);
    drag(&mut app, from, to, Modifiers::SHIFT);
    let b = bounds(&app, &new_id(&app, &before));
    assert_eq!((b.x0, b.y0, b.x1, b.y1), (130.0, 30.0, 180.0, 80.0));
}

#[test]
fn pen_points_sit_exactly_on_anchors_and_shift_keeps_an_axis() {
    let mut app = fixture(SCENE);
    app.snap = crate::snap::Snap::default();
    app.tool = Tool::Pen;
    let before = ids(&app);
    // Near the corner of `box` (130, 120), near the corner of `other` (280, 180), then with
    // Shift roughly below it: straight down.
    let a = screen(&mut app, 130.0, 120.0) + vec2(3.0, -2.0);
    let b = screen(&mut app, 280.0, 180.0) + vec2(-2.0, 3.0);
    let c = screen(&mut app, 287.0, 260.0);
    click(&mut app, a, Modifiers::NONE);
    click(&mut app, b, Modifiers::NONE);
    click(&mut app, c, Modifiers::SHIFT);
    app.finish_pen(false);
    let id = new_id(&app, &before);
    let doc = &app.scene.doc;
    let i = doc.index_of(&id).unwrap();
    let world = doc.world_transform(i) * doc.nodes[i].path().unwrap();
    let pts: Vec<Point> = world.elements().iter().filter_map(|el| el.end_point()).collect();
    assert_eq!(pts[0], Point::new(130.0, 120.0));
    assert_eq!(pts[1], Point::new(280.0, 180.0));
    assert_eq!(pts[2].x, 280.0, "Shift keeps the third point right under the second");
    assert!((pts[2].y - 260.0).abs() < 0.11);
    // The script holds short numbers, not the noise of a subtraction.
    let line = app.src.lines().find(|l| l.starts_with(&format!("path {id}"))).unwrap();
    assert!(line.split_whitespace().all(|w| w.len() < 12), "{line}");
}

#[test]
fn guides_come_out_of_the_rulers_hold_objects_and_go_back() {
    let mut app = fixture(SCENE);
    app.snap = crate::snap::Snap::default();
    let top = screen(&mut app, 215.0, 0.0);
    let ruler = app.canvas_bounds.top() + 9.0;
    // From the top ruler to 2 points off the top edge of `box` (y = 80): a horizontal guide
    // that sticks to the edge.
    let to = screen(&mut app, 215.0, 80.0) + vec2(0.0, 2.0);
    drag(&mut app, egui::pos2(top.x, ruler), to, Modifiers::NONE);
    assert!(app.src.contains("\nguide y 80\n"), "{}", app.src);
    // From the left ruler: a vertical one on the step.
    let left = app.canvas_bounds.left() + 9.0;
    let to = screen(&mut app, 215.0, 250.0);
    drag(&mut app, egui::pos2(left, to.y), to, Modifiers::NONE);
    assert!(app.src.contains("guide y 80\nguide x 215\n"), "{}", app.src);
    assert_eq!(app.undo.len(), 2, "each guide is one undo step");
    assert_eq!(app.scene.doc, motif_script::parse(SCENE).unwrap().doc, "guides are not part of the drawing");

    // `other` (280..320) is dragged left until its edge is 3 points from the guide: it sticks.
    app.select_only(Some("other".into()));
    let from = screen(&mut app, 300.0, 200.0);
    let to = screen(&mut app, 235.0, 200.0) + vec2(3.0, 0.0);
    drag(&mut app, from, to, Modifiers::NONE);
    assert_eq!(bounds(&app, "other").x0, 215.0);

    // A locked guide stays where it is; an unlocked one dropped on its ruler is gone.
    let with_guides = app.src.clone();
    let locked = motif_script::set_guide(&app.src, &app.scene, 0, 80.0, true).unwrap();
    app.apply_once("guide", Some(locked));
    app.select_only(None);
    let on_y = screen(&mut app, 360.0, 80.0);
    let away = screen(&mut app, 360.0, 30.0);
    let locked_src = app.src.clone();
    drag(&mut app, on_y, away, Modifiers::NONE);
    assert_eq!(app.src, locked_src);
    let on_x = screen(&mut app, 215.0, 270.0);
    drag(&mut app, on_x, egui::pos2(left, on_x.y), Modifiers::NONE);
    assert!(!app.src.contains("guide x"), "{}", app.src);
    assert!(app.src.contains("guide y 80 locked"));
    assert_ne!(app.src, with_guides);
    assert!(app.guide_drag.is_none());
}

#[test]
fn a_file_with_guides_is_saved_as_version_two_and_without_them_as_one() {
    let dir = std::env::temp_dir().join(format!("motif-guides-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("g.msc");
    let two_boards = "artboard \"a\" 40x40\nrect r size 10\nartboard \"b\" 40x40\nguide x 5\nrect q size 10\n";
    std::fs::write(&path, two_boards).unwrap();
    // The first board is open; the guide is on the second: the file still needs version 2.
    let mut app = fixture(two_boards);
    app.path = Some(path.clone());
    app.saved_src = app.file_text();
    app.save(false);
    let saved = std::fs::read_to_string(&path).unwrap();
    assert_eq!(saved, format!("motif 2\n{two_boards}"));
    assert_eq!(motif_script::parse_boards(&saved, None).unwrap()[1].guides.len(), 1);
    let _ = std::fs::remove_dir_all(dir);
}
