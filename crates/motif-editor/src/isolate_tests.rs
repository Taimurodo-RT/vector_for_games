//! Isolation of a group, driven through the real pointer and key events: the way in and out,
//! what stays out of reach, what is dimmed, and where new objects go.

use super::editing_tests::fixture;
use super::snap_tests::{bounds, click, drag, frame, ids, new_id, screen};
use super::*;

/// A card with a nested badge between two objects that have nothing to do with it.
///
/// On the artboard: `back` 30..90 × 30..90, `plate` 120..280 × 100..200, `bar` 140..160 × 120..180,
/// `dot` 225..255 × 135..165, `far` 330..370 × 230..270.
const SCENE: &str = "artboard \"a\" 400x300
rect back at 60 60 size 60 60 fill #888888
group card \"Карточка\" at 200 150 {
  rect plate size 160 100 fill #ffffff
  rect bar at -50 0 size 20 60 fill #0000ff
  group badge \"Значок\" at 40 0 {
    ellipse dot at 0 0 size 30 30 fill #ff0000
  }
}
rect far at 350 250 size 40 40 fill #888888
";

/// Two clicks in one place. egui counts clicks by time alone, so the clicks made before must be
/// old enough not to make this a triple click.
fn double_click(app: &mut App, p: Pos2) {
    static LATER: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(1);
    let time = Some(10.0 * f64::from(LATER.fetch_add(1, std::sync::atomic::Ordering::Relaxed)));
    let ctx = app.ctx.clone();
    ctx.run_ui(egui::RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1100.0, 850.0))), time, ..Default::default() }, |ui| {
        app.canvas_ui(ui, None);
    })
    .drop_without_applying_deltas();
    for _ in 0..2 {
        click(app, p, Modifiers::NONE);
    }
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

fn picked(app: &App) -> Vec<String> {
    let mut all = app.selection();
    all.sort();
    all
}

fn parent(app: &App, id: &str) -> Option<String> {
    let doc = &app.scene.doc;
    doc.node(id).unwrap().parent.map(|p| doc.nodes[p].id.clone())
}

#[test]
fn a_double_click_goes_into_the_group_and_nothing_outside_can_be_touched() {
    let mut app = fixture(SCENE);
    let (dot, plate, back) = (screen(&mut app, 240.0, 150.0), screen(&mut app, 130.0, 110.0), screen(&mut app, 60.0, 60.0));
    // Without isolation a click picks whatever is under it.
    click(&mut app, back, Modifiers::NONE);
    assert_eq!(app.selected.as_deref(), Some("back"));

    // Into the group of the object that was clicked: the nearest one.
    double_click(&mut app, dot);
    assert_eq!((app.isolated.as_deref(), app.selected.as_deref()), (Some("badge"), Some("dot")));
    assert!(app.status.contains("Значок"), "{}", app.status);
    // The plate lies in the card, outside the badge: a click on it picks nothing.
    click(&mut app, plate, Modifiers::NONE);
    assert_eq!((app.isolated.as_deref(), app.selected.as_deref()), (Some("badge"), None));
    click(&mut app, back, Modifiers::SHIFT);
    assert_eq!(app.selected, None);
    // Nor can it be dragged: the press starts a marquee over the badge's own objects.
    let before = app.src.clone();
    let to = screen(&mut app, 250.0, 160.0);
    drag(&mut app, plate, to, Modifiers::NONE);
    assert_eq!(app.src, before, "nothing outside moved");
    assert_eq!(picked(&app), ["dot"]);

    // Esc: one level out, and the group that was left is selected.
    key(&mut app, Key::Escape, Modifiers::NONE);
    assert_eq!((app.isolated.as_deref(), app.selected.as_deref()), (Some("card"), Some("badge")));
    click(&mut app, plate, Modifiers::NONE);
    assert_eq!(app.selected.as_deref(), Some("plate"));
    click(&mut app, back, Modifiers::NONE);
    assert_eq!((app.isolated.as_deref(), app.selected.as_deref()), (Some("card"), None));
    // A marquee over the whole artboard and Ctrl+A take the objects of the card only.
    let (a, b) = (screen(&mut app, 5.0, 5.0), screen(&mut app, 395.0, 295.0));
    drag(&mut app, a, b, Modifiers::NONE);
    assert_eq!(picked(&app), ["badge", "bar", "plate"]);
    app.select_only(None);
    key(&mut app, Key::A, Modifiers::COMMAND);
    assert_eq!(picked(&app), ["badge", "bar", "plate"]);

    key(&mut app, Key::Escape, Modifiers::NONE);
    assert_eq!((app.isolated.as_deref(), app.selected.as_deref()), (None, Some("card")));
    // Out of isolation everything is as it was: Ctrl+A takes the objects of the artboard.
    key(&mut app, Key::A, Modifiers::COMMAND);
    assert_eq!(picked(&app), ["back", "card", "far"]);
    assert_eq!(app.src, SCENE, "going in and out writes nothing");
}

#[test]
fn a_path_opens_its_points_at_its_own_level_and_empty_canvas_leads_out() {
    let src = "artboard \"a\" 400x300
path loose at 60 60 d \"M -30 -30 L 30 -30 L 30 30 L -30 30 Z\" fill #888888
group pair at 250 150 {
  path leaf d \"M -40 -40 L 40 -40 L 40 40 L -40 40 Z\" fill #00aa00
}
";
    let mut app = fixture(src);
    let (loose, leaf, empty) = (screen(&mut app, 60.0, 60.0), screen(&mut app, 250.0, 150.0), screen(&mut app, 60.0, 250.0));
    // A path that lies in no group: a double click edits its points, as before.
    double_click(&mut app, loose);
    assert!(app.isolated.is_none() && app.point_edit.as_ref().is_some_and(|e| e.node == "loose"));
    key(&mut app, Key::Escape, Modifiers::NONE);
    assert!(app.point_edit.is_none());

    // A path in a group: the first double click goes into the group, the second to the points.
    double_click(&mut app, leaf);
    assert_eq!(app.isolated.as_deref(), Some("pair"));
    assert!(app.point_edit.is_none());
    double_click(&mut app, leaf);
    assert!(app.point_edit.as_ref().is_some_and(|e| e.node == "leaf"));
    // Esc leaves the points first, then the group.
    key(&mut app, Key::Escape, Modifiers::NONE);
    assert!(app.point_edit.is_none() && app.isolated.as_deref() == Some("pair"));

    // A double click on empty canvas, or on what is out of reach, leads out of every group.
    double_click(&mut app, empty);
    assert_eq!((app.isolated.as_deref(), app.selected.as_deref()), (None, None));
    double_click(&mut app, leaf);
    double_click(&mut app, loose);
    assert_eq!(app.isolated, None);
    // One empty double click with no isolation is nothing.
    double_click(&mut app, empty);
    assert_eq!((app.isolated.as_deref(), app.selected.as_deref()), (None, None));
    assert_eq!(app.src, src);
}

#[test]
fn the_rest_is_dimmed_on_the_canvas_only() {
    let mut app = fixture(SCENE);
    app.isolate(Some("badge".into()));
    let mut shown = app.display_doc();
    app.dim_outside(&mut shown);
    let opacity = |id: &str| shown.world_opacity(shown.index_of(id).unwrap());
    // The badge is drawn through the card around it, so the card itself is not dimmed.
    assert_eq!((opacity("dot"), opacity("badge"), opacity("card")), (1.0, 1.0, 1.0));
    for outside in ["back", "far", "plate", "bar"] {
        assert_eq!(opacity(outside), crate::isolate::DIM, "{outside}");
    }
    // The drawing is what it was: the file, the document and what is exported.
    assert_eq!(app.src, SCENE);
    assert!(app.display_doc().nodes.iter().all(|n| n.opacity == 1.0));

    // Something outside selected by other means (the layers, the script) ends the isolation.
    app.select_only(Some("far".into()));
    frame(&mut app, vec![], Modifiers::NONE);
    assert_eq!(app.isolated, None);
    // So does the end of the group itself.
    app.isolate(Some("badge".into()));
    app.select_only(Some("badge".into()));
    app.delete_selection();
    frame(&mut app, vec![], Modifiers::NONE);
    assert_eq!(app.isolated, None);
    // Only a group can be entered.
    app.isolate(Some("back".into()));
    assert_eq!(app.isolated, None);
}

#[test]
fn what_is_made_inside_a_group_belongs_to_it() {
    let mut app = fixture(SCENE);
    app.isolate(Some("card".into()));
    // A new shape is drawn where the pointer went and written into the group.
    let before = ids(&app);
    app.create_shape(ShapeKind::Rect, Point::new(300.0, 40.0), Point::new(340.0, 80.0), false);
    let made = new_id(&app, &before);
    assert_eq!(parent(&app, &made).as_deref(), Some("card"), "{}", app.src);
    let b = bounds(&app, &made);
    assert_eq!((b.x0, b.y0, b.x1, b.y1), (300.0, 40.0, 340.0, 80.0));
    frame(&mut app, vec![], Modifiers::NONE);
    assert_eq!((app.isolated.as_deref(), app.selected.as_deref()), (Some("card"), Some(made.as_str())));
    assert_eq!(app.undo.len(), 1, "one shape is one undo step");

    // A copy of an object from outside comes into the group and stands by its original.
    let ctx = app.ctx.clone();
    app.select_only(Some("far".into()));
    assert!(app.copy_selection(&ctx));
    app.isolate(Some("card".into()));
    let before = ids(&app);
    app.paste(None);
    let pasted = new_id(&app, &before);
    assert_eq!(parent(&app, &pasted).as_deref(), Some("card"), "{}", app.src);
    let (b, far) = (bounds(&app, &pasted), bounds(&app, "far"));
    assert_eq!((b.x0, b.y0), (far.x0 + 10.0, far.y0 + 10.0));
    frame(&mut app, vec![], Modifiers::NONE);
    assert_eq!(app.isolated.as_deref(), Some("card"));
}

#[test]
fn a_duplicate_stays_in_the_group_of_its_original() {
    // No isolation is needed for this: the numbers of a copy are counted from the same group.
    let mut app = fixture(SCENE);
    app.select_only(Some("dot".into()));
    let before = ids(&app);
    app.duplicate();
    let copy = new_id(&app, &before);
    assert_eq!(parent(&app, &copy).as_deref(), Some("badge"), "{}", app.src);
    let (b, dot) = (bounds(&app, &copy), bounds(&app, "dot"));
    assert_eq!((b.x0, b.y0, b.width()), (dot.x0 + 10.0, dot.y0 + 10.0, dot.width()));
    // At the level of the artboard a copy is written there, as before.
    app.select_only(Some("far".into()));
    let before = ids(&app);
    app.duplicate();
    assert_eq!(parent(&app, &new_id(&app, &before)), None);
}
