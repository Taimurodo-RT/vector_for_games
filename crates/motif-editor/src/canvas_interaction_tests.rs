use super::editing_tests::fixture;
use super::*;
use crate::workspace::{Panel, Workspace};

fn frame(app: &mut App, events: Vec<egui::Event>) -> egui::FullOutput {
    let ctx = app.ctx.clone();
    ctx.run_ui(egui::RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1100.0, 850.0))), events, ..Default::default() }, |ui| {
        app.canvas_ui(ui, None);
        app.floating_inspector(&ctx);
    })
}
fn button(p: Pos2, pressed: bool, button: egui::PointerButton) -> egui::Event {
    egui::Event::PointerButton { pos: p, button, pressed, modifiers: Modifiers::NONE }
}
fn click(app: &mut App, p: Pos2, which: egui::PointerButton) {
    for pressed in [true, false] {
        frame(app, vec![egui::Event::PointerMoved(p), button(p, pressed, which)]).drop_without_applying_deltas();
    }
}
fn label(app: &mut App, label: &str) -> Pos2 {
    frame(app, vec![]).drop_without_applying_deltas();
    let out = frame(app, vec![]);
    let pos = out.shapes.iter().find_map(|s| match &s.shape {
        egui::Shape::Text(t) if t.galley.job.text == label => Some(t.pos + t.galley.size() * 0.5),
        _ => None,
    });
    out.drop_without_applying_deltas();
    pos.unwrap_or_else(|| panic!("missing control {label}"))
}
fn ball(app: &mut App) -> Pos2 {
    frame(app, vec![]).drop_without_applying_deltas();
    let doc = app.display_doc();
    app.view.unwrap().to_screen(app.canvas_bounds, doc.world_transform(doc.index_of("ball").unwrap()) * Point::ZERO)
}

#[test]
fn drag_animated_ball_pauses_on_press_and_beats_its_motion_path() {
    for auto in [false, true] {
        let mut app = fixture(include_str!("../../../examples/bounce.msc"));
        app.select_anim("Idle".into());
        app.auto_key = auto;
        app.time = 0.3;
        app.motion_path = true;
        app.select_only(Some("ball".into()));
        app.snap = crate::snap::Snap::default();
        app.snap.free = true;
        app.snap.guides = false;
        let source = app.src.clone();
        let base = app.scene.doc.clone();
        let initial = app.display_doc().node("ball").unwrap().clone();
        let start = ball(&mut app);
        let end = start + vec2(76.0, -43.0);
        let zoom = app.view.unwrap().zoom as f64;
        app.playing = true;
        frame(&mut app, vec![egui::Event::PointerMoved(start), button(start, true, egui::PointerButton::Primary)]).drop_without_applying_deltas();
        assert!(!app.playing, "press must stop the clock before the drag threshold");
        frame(&mut app, vec![egui::Event::PointerMoved(end)]).drop_without_applying_deltas();
        assert!(matches!(app.drag, Some(DragOp::Move { .. })), "body must win over its trajectory");
        frame(&mut app, vec![button(end, false, egui::PointerButton::Primary)]).drop_without_applying_deltas();
        let shown = app.display_doc();
        let moved = shown.node("ball").unwrap();
        assert!((moved.x - initial.x - 76.0 / zoom).abs() < 0.1);
        assert!((moved.y - initial.y + 43.0 / zoom).abs() < 0.1);
        assert_eq!(app.time, 0.3);
        assert_eq!(app.scene.doc.nodes, base.nodes, "drawing must stay unchanged");
        if auto {
            assert_ne!(app.src, source);
            assert!(!app.has_draft_pose());
        } else {
            assert_eq!(app.src, source);
            app.toggle_playback();
            assert!(!app.playing && app.has_draft_pose() && app.play_needs_record, "Play must not lose a draft silently");
            app.record_draft_pose();
            assert!(!app.has_draft_pose());
            app.toggle_playback();
            assert!(app.playing);
        }
        let pose = app.current_anim().unwrap().pose(&app.scene.doc, 0.3);
        assert!((pose.node("ball").unwrap().x - moved.x).abs() < 0.1);
        assert!((pose.node("ball").unwrap().y - moved.y).abs() < 0.1);
        assert_eq!(app.undo.len(), 1, "one gesture / explicit pose record is one undo step");
        app.undo();
        assert_eq!(app.src, source);
    }
}

#[test]
fn context_inspector_stays_for_edits_and_closes_by_button_escape_or_outside() {
    for close in ["button", "escape", "outside"] {
        let mut app = fixture(include_str!("../../../examples/bounce.msc"));
        app.select_anim("Idle".into());
        let original = app.src.clone();
        let p = ball(&mut app);
        click(&mut app, p, egui::PointerButton::Secondary);
        let item = label(&mut app, "Свойства и ключи…");
        click(&mut app, item, egui::PointerButton::Primary);
        assert!(app.floating.is_some());
        let auto = label(&mut app, "Автоключ");
        click(&mut app, auto, egui::PointerButton::Primary);
        assert!(app.auto_key && app.floating.is_some(), "using controls must keep the inspector open");
        // A press in the inspector must not move the object or create drawing edits.
        assert_eq!(app.src, original);
        assert!(app.drag.is_none());
        match close {
            "button" => {
                let p = label(&mut app, "×");
                click(&mut app, p, egui::PointerButton::Primary);
            }
            "escape" => {
                frame(&mut app, vec![egui::Event::Key { key: Key::Escape, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE }])
                    .drop_without_applying_deltas();
            }
            _ => click(&mut app, pos2(15.0, 15.0), egui::PointerButton::Primary),
        }
        assert!(app.floating.is_none(), "close path: {close}");
        assert_eq!(app.src, original);
    }
}

#[test]
fn floating_key_command_records_without_closing_and_can_be_undone() {
    let mut app = fixture(include_str!("../../../examples/bounce.msc"));
    app.select_anim("Idle".into());
    app.time = 0.25;
    let source = app.src.clone();
    let p = ball(&mut app);
    click(&mut app, p, egui::PointerButton::Secondary);
    let item = label(&mut app, "Свойства и ключи…");
    click(&mut app, item, egui::PointerButton::Primary);
    let command = label(&mut app, "Записать ключи трансформации");
    click(&mut app, command, egui::PointerButton::Primary);
    assert!(app.floating.is_some());
    assert_ne!(app.src, source);
    let tracks = &app.current_anim().unwrap().tracks;
    for prop in [Prop::X, Prop::Y, Prop::Rotation, Prop::ScaleX, Prop::ScaleY] {
        assert!(tracks.iter().any(|t| t.node == "ball" && t.prop == prop && t.keys.iter().any(|k| (k.time - 0.25).abs() < 1e-6)));
    }
    assert_eq!(app.undo.len(), 1);
    app.undo();
    assert_eq!(app.src, source);
}

#[test]
fn motion_path_remains_pickable_over_an_unselected_background() {
    let mut app = fixture(
        "artboard \"Path\" 320x240\nrect bg at 160 120 size 320 240 fill #222222\nellipse ball at 60 100 size 20 fill #ff0000\nanim Move 1s {\nball.x 0: 60 | 1: 260\n}\n",
    );
    app.select_anim("Move".into());
    app.select_only(Some("ball".into()));
    app.motion_path = true;
    frame(&mut app, vec![]).drop_without_applying_deltas();
    let pos = app.view.unwrap().to_screen(app.canvas_bounds, Point::new(160.0, 100.0));
    click(&mut app, pos, egui::PointerButton::Primary);
    assert!((app.time - 0.5).abs() < 0.03, "trajectory on a background should still scrub: {}", app.time);
    assert!(app.undo.is_empty());
}

#[test]
fn focus_keeps_animation_timeline_and_preview_layout_preserves_canvas() {
    let mut app = fixture(include_str!("../../../examples/bounce.msc"));
    app.set_workspace(Workspace::Animate);
    let saved = app.panels.settings.clone();
    app.panels.toggle_focus();
    assert!(app.panels.layout().timeline);
    assert!(!app.panels.layout().inspector && !app.panels.layout().preview);
    app.panels.toggle_focus();
    assert_eq!(app.panels.settings, saved);
    app.panels.set(Panel::Preview, true);
    for size in [vec2(900.0, 560.0), vec2(1366.0, 768.0), vec2(1920.0, 1080.0)] {
        let ctx = app.ctx.clone();
        for _ in 0..2 {
            ctx.run_ui(egui::RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, size)), ..Default::default() }, |ui| {
                egui::Panel::top("top").exact_size(80.0).show(ui, |ui| app.top_bar(ui));
                app.workspace_panels(ui);
                let canvas = ui.available_rect_before_wrap();
                assert!(canvas.width() >= 315.0 && canvas.height() >= 215.0, "{size:?}: {canvas:?}");
            })
            .drop_without_applying_deltas();
        }
    }
    assert!(app.undo.is_empty());
}
