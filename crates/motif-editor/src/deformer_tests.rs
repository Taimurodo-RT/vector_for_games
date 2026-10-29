use super::editing_tests::fixture;
use super::*;
use crate::workspace::Workspace;

#[test]
fn draft_overrides_do_not_erase_explicit_keys_on_driven_properties() {
    let mut app = fixture(
        "artboard 200x200\nrect box size 30\nparameter p range -1 1 default 0 value 0\ndriver p {\nbox.x -1: -20 | 1: 20\n}\nanim A 2s {\np.value 0: 0 | 2: 1\nbox.x 0: 75 | 2: 95\n}\n",
    );
    app.time = 1.0;
    app.auto_key = false;
    app.edit("box", Prop::Y, Value::Num(30.0));
    assert_eq!(app.display_doc().node("box").unwrap().x, 85.0);
    app.edit("p", Prop::Parameter, Value::Num(-1.0));
    assert_eq!(app.display_doc().node("box").unwrap().x, 85.0);
    assert_eq!(app.display_doc().node("box").unwrap().y, 30.0);
}

const SCENE: &str = "artboard \"Face\" 400x400\ngroup head at 200 200 deform 220 240 {\npath face d \"M -50 -60 C -20 -80 20 -80 50 -60 L 40 60 L -40 60 Z\" fill #ffeecc\n}\nanim Idle 2s {\nhead.rot 0: 0 | 2: 4\n}\n";

fn frame(app: &mut App, events: Vec<egui::Event>) {
    let ctx = app.ctx.clone();
    ctx.run_ui(egui::RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(900.0, 700.0))), events, ..Default::default() }, |ui| {
        app.canvas_ui(ui, None)
    })
    .drop_without_applying_deltas();
}
fn drag_control(app: &mut App, delta: egui::Vec2) {
    frame(app, vec![]);
    let doc = app.display_doc();
    let i = doc.index_of("head").unwrap();
    let local = doc.nodes[i].lattice.as_ref().unwrap().control(4);
    let start = app.view.unwrap().to_screen(app.canvas_bounds, doc.world_transform(i) * doc.map_lattice_point(i, local));
    let end = start + delta;
    let button = |pos, pressed| egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed, modifiers: Modifiers::NONE };
    frame(app, vec![egui::Event::PointerMoved(start), button(start, true)]);
    frame(app, vec![egui::Event::PointerMoved(end)]);
    frame(app, vec![button(end, false)]);
}

#[test]
fn cage_drag_authors_two_keys_as_one_undo_step_and_modes_keep_the_result() {
    let mut app = fixture(SCENE);
    app.time = 0.5;
    app.select_only(Some("head".into()));
    app.deformer.target = Some("head".into());
    let before = app.src.clone();
    let original = app.scene.doc.node("face").unwrap().geometry.clone();
    drag_control(&mut app, vec2(35.0, -22.0));
    assert!(app.errors.is_empty());
    assert_eq!(app.undo.len(), 1);
    let pose = app.display_doc();
    assert_ne!(pose.node("head").unwrap().get(Prop::WarpX(4)), Value::Num(0.0));
    assert_eq!(app.scene.doc.node("face").unwrap().geometry, original);
    for space in Workspace::ALL {
        app.set_workspace(space);
        assert_eq!(app.time, 0.5);
        assert_eq!(app.display_doc(), pose);
    }
    app.undo();
    assert_eq!(app.src, before);
    app.redo();
    assert_eq!(app.display_doc(), pose);
}

#[test]
fn reference_form_drag_edits_the_driver_without_authoring_a_clip_or_moving_time() {
    let mut app = fixture(SCENE);
    app.time = 0.75;
    app.bind_lattice("head");
    let clips = app.scene.doc.animations.clone();
    let source = app.src.clone();
    let history = app.undo.len();
    drag_control(&mut app, vec2(30.0, 0.0));
    assert_ne!(app.src, source);
    assert_eq!(app.scene.doc.animations, clips);
    assert_eq!(app.time, 0.75);
    assert_eq!(app.undo.len(), history + 1);
    let p = &app.scene.doc.parameters[0];
    let track = &app.scene.doc.drivers[0].tracks.iter().find(|t| t.prop == Prop::WarpX(4)).unwrap();
    assert!(track.sample(p.max).unwrap().as_num().unwrap() > 0.0);
    assert_eq!(track.sample(p.default), Some(Value::Num(0.0)));
    app.undo();
    assert_eq!(app.src, source);
}

#[test]
fn drawing_a_contour_at_a_later_frame_records_a_form_and_preserves_the_original() {
    let mut app = fixture(SCENE);
    app.time = 0.6;
    let source = app.src.clone();
    let path = motif_core::kurbo::BezPath::from_svg("M -55 -60 C -20 -120 20 -120 55 -60 L 40 70 L -40 70 Z").unwrap();
    let edit = app.outline_edit_source("face", &path);
    assert!(app.apply_once("path-frame", edit));
    assert_eq!(app.display_doc().node("face").unwrap().editable_path(), path);
    let at = app.time;
    let current = app.display_doc();
    app.set_workspace(Workspace::Animate);
    assert_eq!(app.time, at);
    assert_eq!(app.display_doc(), current);
    app.time = 0.0;
    assert_eq!(app.display_doc().node("face").unwrap().editable_path(), parse(SCENE).unwrap().doc.node("face").unwrap().geometry);
    app.undo();
    assert_eq!(app.src, source);
}

#[test]
fn object_created_in_drawing_has_an_appearance_key_and_existing_objects_stay_unchanged() {
    let mut app = fixture(SCENE);
    app.time = 0.7;
    let original = app.scene.doc.node("face").unwrap().clone();
    let source = app.src.clone();
    let (out, id) = motif_script::add_node(&app.src, &app.scene, ShapeKind::Ellipse, (40.0, 40.0), (20.0, 20.0));
    assert!(app.apply_once("create", Some(out)));
    assert!(app.display_doc().node(&id).unwrap().visible);
    app.time = 0.2;
    assert!(!app.display_doc().node(&id).unwrap().visible);
    assert_eq!(app.scene.doc.node("face").unwrap(), &original);
    assert_eq!(app.undo.len(), 1);
    app.undo();
    assert_eq!(app.src, source);
}

#[test]
fn grouping_existing_art_at_a_later_frame_does_not_make_it_disappear() {
    let mut app = fixture(SCENE);
    app.time = 0.7;
    let (out, id) = motif_script::group_nodes(&app.src, &app.scene, &["face".into()], None).unwrap();
    assert!(app.apply_once("group", Some(out)));
    assert!(!app.current_anim().unwrap().tracks.iter().any(|t| t.node == id && t.prop == Prop::Visible));
    app.time = 0.0;
    let doc = app.display_doc();
    assert!(doc.is_visible(doc.index_of("face").unwrap()));
}

#[test]
fn ui_color_keys_in_drawing_share_the_frame_without_changing_other_clips_or_layout() {
    let mut app = fixture(
        "artboard \"UI\" 300x200 ui\ngroup button ui button slice 8 {\nrect plate size 120 40 r 8 fill #ffffff\n}\nanim Hover 1s {\nplate.opacity 0: 1 | 1: 0.8\n}\nanim Press 1s {\nplate.fill 0: #ffffff | 1: #000000\n}\n",
    );
    app.time = 0.4;
    let press = app.scene.doc.animation("Press").unwrap().clone();
    app.edit("plate", Prop::Fill, Value::Color(Color::rgb(20, 30, 40)));
    let posed = app.display_doc();
    for space in Workspace::ALL {
        app.set_workspace(space);
        assert_eq!(app.display_doc(), posed);
        assert_eq!(app.time, 0.4);
    }
    assert_eq!(app.scene.doc.animation("Press").unwrap(), &press);
    assert!(app.scene.doc.node("button").unwrap().ui);
    assert_eq!(app.scene.doc.node("plate").unwrap().fill, Some(Color::WHITE));
}
