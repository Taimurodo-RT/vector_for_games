use super::editing_tests::fixture;
use super::*;
use crate::preview::{Mode, Preview};

#[test]
fn preview_owns_time_and_linked_pose_excludes_unrecorded_changes() {
    let mut app = fixture(include_str!("../../../examples/bounce.msc"));
    app.select_anim("Idle".into());
    app.time = 0.4;
    app.select_only(Some("ball".into()));
    app.auto_key = false;
    app.edit("ball", Prop::X, Value::Num(310.0));
    let source = app.src.clone();
    let history = app.undo.clone();
    app.preview.sync(&app.scene.doc, true);
    app.preview.set_mode(Mode::Clip);
    app.preview.playing = true;
    for _ in 0..12 {
        assert!(app.preview.advance(0.05, None));
    }
    assert!((app.preview.time - 0.6).abs() < 1e-6);
    assert_eq!(app.time, 0.4);
    app.preview.follow = true;
    app.preview.advance(0.05, Some(("Idle", app.time)));
    assert_eq!(app.preview.time, app.time);
    let actual = app.preview.pose().unwrap();
    let expected = app.current_anim().unwrap().pose(&app.scene.doc, app.time);
    assert_eq!(actual.nodes, expected.nodes);
    assert_ne!(actual.node("ball").unwrap().x, app.display_doc().node("ball").unwrap().x);
    app.preview.hidden();
    let time = app.preview.time;
    app.preview.follow = false;
    app.preview.advance(99.0, None);
    assert_eq!(app.preview.time, time);
    assert!(!app.preview.playing && app.has_draft_pose());
    assert_eq!(app.selected.as_deref(), Some("ball"));
    assert_eq!(app.src, source);
    assert_eq!(app.undo, history);
}

#[test]
fn invalid_script_keeps_last_valid_preview_and_valid_update_resets_simulation() {
    let doc = parse(include_str!("../../../examples/bounce.msc")).unwrap().doc;
    let mut p = Preview::default();
    p.sync(&doc, true);
    p.set_mode(Mode::Clip);
    p.playing = true;
    p.advance(0.1, None);
    let shown = p.pose().unwrap();
    let mut changed = doc.clone();
    changed.nodes[0].x += 100.0;
    p.sync(&changed, false);
    assert!(p.stale && !p.playing);
    assert!(!p.advance(0.1, None));
    assert_eq!(p.pose().unwrap(), shown);
    p.sync(&changed, true);
    assert!(!p.stale && !p.playing);
    assert_eq!(p.time, 0.0);
    assert_eq!(p.doc.as_ref(), Some(&changed));
}

#[test]
fn preview_checkbox_mouse_capture_and_resize_never_write_authoring_state() {
    let src = "artboard \"UI\" 320x240 ui\ngroup check at 80 80 ui check {\nrect off size 50 fill #ff0000 part unchecked\nrect on size 50 fill #00ff00 part checked\n}\n";
    let mut app = fixture(src);
    let original = app.scene.doc.clone();
    app.preview.sync(&original, true);
    app.preview.set_mode(Mode::Ui);
    let checked = app.preview.check.control("check").unwrap().checked;
    let shown = app.preview.pose().unwrap();
    let at = Some(Point::new(80.0, 80.0));
    app.preview.pointer(&shown, at, true);
    app.preview.pointer(&shown, at, false);
    assert_ne!(app.preview.check.control("check").unwrap().checked, checked);
    assert_eq!(app.preview.check.control("check").unwrap().clicks, 1);
    app.preview.size = Some((640.0, 480.0));
    assert_eq!(app.preview.pose().unwrap().artboard.width, 640.0);
    app.preview.pointer(&shown, at, true);
    app.preview.hidden();
    app.preview.pointer(&shown, at, false);
    assert_eq!(app.preview.check.control("check").unwrap().clicks, 1, "closing the preview must cancel pointer capture");
    assert_eq!(app.scene.doc, original);
    assert_eq!(app.src, src);
    assert!(app.check.is_none() && app.selected.is_none() && app.undo.is_empty());
}

#[test]
fn portrait_preview_parameters_and_machine_inputs_are_isolated_and_reset_between_files() {
    let mut app = fixture(include_str!("../../../examples/character-portrait.msc"));
    let source = app.src.clone();
    app.preview.sync(&app.scene.doc, true);
    let neutral = app.preview.pose().unwrap();
    app.preview.parameters.insert("turn".into(), 0.8);
    app.preview.parameters.insert("blink".into(), 1.0);
    let posed = app.preview.pose().unwrap();
    assert_ne!(neutral.nodes, posed.nodes);
    assert_eq!(app.src, source);
    assert!(app.pose_preview.is_empty() && !app.has_draft_pose());
    app.new_scene(include_str!("../../../examples/machine.msc"));
    assert!(app.preview.doc.is_none() && app.preview.parameters.is_empty());
    let undo = app.undo.clone();
    let source = app.src.clone();
    app.preview.sync(&app.scene.doc, true);
    app.preview.set_mode(Mode::Machine);
    let machine = &app.scene.doc.machines[0];
    app.preview.player.as_mut().unwrap().set(machine, "speed", 1.0);
    app.preview.playing = true;
    for _ in 0..5 {
        app.preview.advance(0.05, None);
    }
    let state = app.preview.player.as_ref().unwrap().state;
    assert_eq!(machine.states[state].name, "Run");
    assert!(app.run.is_none());
    assert_eq!(app.undo, undo);
    assert_eq!(app.src, source);
    app.preview.hidden();
    let time = app.preview.player.as_ref().unwrap().time;
    app.preview.advance(0.1, None);
    assert_eq!(app.preview.player.as_ref().unwrap().time, time);
}
