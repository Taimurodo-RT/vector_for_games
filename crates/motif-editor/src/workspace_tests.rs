use super::editing_tests::fixture;
use super::*;
use crate::workspace::{Panel, Workspace};

const SIMPLE: &str = "artboard \"Test\" 320x240\nrect box size 40 fill #ff0000\n";

#[test]
fn switching_workspaces_profiles_panels_and_focus_never_authors_a_document() {
    for source in [SIMPLE, include_str!("../../../examples/ui-kit.msc"), include_str!("../../../examples/character-portrait.msc")] {
        let mut app = fixture(source);
        let original = app.file_text();
        let doc = app.scene.doc.clone();
        for space in Workspace::ALL {
            app.set_workspace(space);
            for profile in [Purpose::Any, Purpose::Ui, Purpose::Game] {
                app.set_tool_profile(profile);
            }
            for panel in Panel::ALL {
                app.panels.toggle(panel);
            }
            app.panels.toggle_focus();
            app.panels.toggle(Panel::Script);
            app.panels.toggle_focus();
            assert!(app.file_text() == original);
            assert_eq!(app.scene.doc, doc);
            assert!(!app.dirty());
            assert!(app.undo.is_empty() && app.redo.is_empty());
        }
    }
}

#[test]
fn empty_animation_workspace_requires_explicit_creation_and_cannot_edit_artwork() {
    let mut app = fixture(SIMPLE);
    app.set_workspace(Workspace::Animate);
    let original = app.file_text();
    assert!(app.anim.is_none() && app.scene.doc.animations.is_empty());
    app.edit("box", Prop::X, Value::Num(90.0));
    assert!(app.file_text() == original && app.undo.is_empty());
    app.create_animation();
    assert_eq!(app.scene.doc.animations.len(), 1);
    assert_eq!(app.undo.len(), 1);
    assert!(app.anim.is_some());
    app.undo();
    assert!(app.file_text() == original);
}

#[test]
fn artwork_edit_returns_to_remembered_clip_and_time_without_resuming_playback() {
    let mut app = fixture(include_str!("../../../examples/character-portrait.msc"));
    app.select_anim("Look".into());
    app.time = 1.25;
    app.playing = true;
    app.set_workspace(Workspace::Animate);
    app.panels.layout_mut().timeline_height = 310.0;
    app.set_workspace(Workspace::Drawing);
    assert_eq!(app.anim.as_deref(), Some("Look"));
    assert!(!app.playing);
    assert_eq!(app.time, 1.25);
    app.edit("shirt", Prop::Fill, Value::Color(motif_core::Color::WHITE));
    app.set_workspace(Workspace::Setup);
    app.panels.toggle_focus();
    app.set_workspace(Workspace::Animate);
    app.panels.toggle_focus();
    assert_eq!(app.anim.as_deref(), Some("Look"));
    assert_eq!(app.time, 1.25);
    assert_eq!(app.panels.layout().timeline_height, 310.0);
    assert!(!app.playing);
    assert_eq!(app.undo.len(), 1, "only the artwork edit belongs to document history");
}

#[test]
fn autokey_off_previews_properties_and_diamonds_record_only_the_chosen_property() {
    let mut app = fixture(SIMPLE);
    app.create_animation();
    app.time = 0.4;
    app.auto_key = false;
    let original = app.file_text();
    let undo = app.undo.len();
    assert!(!app.auto_key);
    app.edit("box", Prop::X, Value::Num(70.0));
    app.edit("box", Prop::Fill, Value::Color(motif_core::Color::WHITE));
    assert!(app.file_text() == original && app.undo.len() == undo);
    assert_eq!(app.display_doc().node("box").unwrap().x, 70.0);
    app.toggle_key("box", Prop::X);
    assert_eq!(app.current_anim().unwrap().tracks.len(), 1);
    assert_eq!(app.current_anim().unwrap().tracks[0].prop, Prop::X);
    assert!(app.has_draft_pose(), "the other probed property remains available");
    app.record_draft_pose();
    assert_eq!(app.current_anim().unwrap().tracks.len(), 2);
    assert!(!app.has_draft_pose());
    app.undo();
    assert_eq!(app.current_anim().unwrap().tracks.len(), 1);
    app.undo();
    assert!(app.file_text() == original);
}

#[test]
fn recording_a_parameter_pose_is_atomic_and_recomputes_the_driven_artwork() {
    let mut app = fixture(include_str!("../../../examples/character-portrait.msc"));
    app.set_workspace(Workspace::Animate);
    app.select_anim("Look".into());
    app.time = 0.75;
    app.auto_key = false;
    let original = app.file_text();
    app.edit("turn", Prop::Parameter, Value::Num(0.5));
    app.edit("blink", Prop::Parameter, Value::Num(1.0));
    assert!(app.file_text() == original && app.undo.is_empty());
    assert!((app.display_doc().node("head").unwrap().yaw - 17.0).abs() < 1e-9);
    app.record_draft_pose();
    assert_eq!(app.undo.len(), 1);
    assert_eq!(app.display_doc().parameter("blink").unwrap().value, 1.0);
    app.undo();
    assert!(app.file_text() == original);
    app.redo();
    assert_eq!(app.display_doc().parameter("turn").unwrap().value, 0.5);
    app.edit("turn", Prop::Parameter, Value::Num(-0.5));
    app.set_workspace(Workspace::Drawing);
    assert!(!app.draft_pose.values.is_empty(), "the probe waits while the drawing is shown");
    app.set_workspace(Workspace::Animate);
    assert_eq!(app.display_doc().parameter("turn").unwrap().value, -0.5);
}

#[test]
fn ui_properties_and_states_survive_on_the_same_board_as_a_parameter_character() {
    let source =
        format!("{}\ngroup action ui button slice 4 {{\n rect plate size 80 24 fill #123456\n}}\n", include_str!("../../../examples/character-portrait.msc"));
    let mut app = fixture(&source);
    assert!(app.errors.is_empty(), "{:?}", app.errors);
    app.set_workspace(Workspace::Setup);
    app.edit("plate", Prop::Fill, Value::Color(motif_core::Color::WHITE));
    assert_eq!(app.undo.len(), 1, "UI styling is not mistaken for character artwork");
    let styled = app.file_text();
    app.edit("shirt", Prop::Fill, Value::Color(motif_core::Color::WHITE));
    assert!(app.file_text() != styled, "an explicit property edit records a key in the shared frame");
    app.add_ui_states();
    assert!(app.scene.doc.animation("Hover").is_some() && app.scene.doc.animation("Press").is_some());
    app.auto_key = true;
    app.time = 0.25;
    app.edit("plate", Prop::Fill, Value::Color(motif_core::Color::BLACK));
    assert!(app.current_anim().unwrap().tracks.iter().any(|t| t.node == "plate" && t.prop == Prop::Fill));
    let keyed = app.file_text();
    app.edit("head", Prop::Yaw, Value::Num(40.0));
    app.edit("shirt", Prop::Fill, Value::Color(motif_core::Color::BLACK));
    assert!(app.file_text() != keyed);
    assert_eq!(app.display_doc().node("head").unwrap().yaw, 40.0);
    let keyed = app.file_text();
    app.set_workspace(Workspace::Drawing);
    app.toggle_check();
    assert!(app.check.is_some());
    assert!(app.file_text() == keyed);
}

#[test]
fn open_and_board_changes_keep_layout_but_do_not_reuse_a_foreign_probe() {
    let mut app = fixture("artboard \"One\" 100x100\nrect box size 10\nartboard \"Two\" 100x100\nrect other size 10\n");
    app.create_animation();
    app.edit("box", Prop::X, Value::Num(50.0));
    app.panels.layout_mut().inspector_width = 312.0;
    app.switch_board(1);
    assert_eq!(app.workspace(), Workspace::Animate);
    assert_eq!(app.panels.layout().inspector_width, 312.0);
    assert!(app.anim.is_none() && !app.has_draft_pose());
    app.new_scene(SIMPLE);
    assert_eq!(app.workspace(), Workspace::Animate);
    assert_eq!(app.panels.layout().inspector_width, 312.0);
    assert!(app.scene.doc.animations.is_empty());
}

fn workspace_frame(app: &mut App, size: egui::Vec2, events: Vec<egui::Event>) -> (egui::FullOutput, Rect) {
    let ctx = app.ctx.clone();
    let mut canvas = Rect::NOTHING;
    let out = ctx.run_ui(egui::RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, size)), events, ..Default::default() }, |ui| {
        egui::Panel::top("top").exact_size(if size.x < 900.0 { 110.0 } else { 80.0 }).show(ui, |ui| app.top_bar(ui));
        app.workspace_panels(ui);
        canvas = ui.available_rect_before_wrap();
    });
    (out, canvas)
}

#[test]
fn real_panels_reserve_canvas_at_small_sizes_and_restore_the_focused_layout() {
    let mut app = fixture(SIMPLE);
    for space in Workspace::ALL {
        app.set_workspace(space);
        app.panels.set(Panel::Script, true);
        for size in [vec2(900.0, 560.0), vec2(1093.0, 614.0), vec2(1366.0, 768.0), vec2(1920.0, 1080.0)] {
            for _ in 0..2 {
                let (out, canvas) = workspace_frame(&mut app, size, vec![]);
                out.drop_without_applying_deltas();
                assert!(canvas.width() >= 315.0 && canvas.height() >= 215.0, "{space:?} {size:?}: {canvas:?}");
            }
        }
    }
    let before = app.panels.settings.clone();
    app.panels.toggle_focus();
    let (out, canvas) = workspace_frame(&mut app, vec2(900.0, 560.0), vec![]);
    out.drop_without_applying_deltas();
    assert!(canvas.width() > 820.0);
    app.panels.set(Panel::Inspector, true);
    app.panels.toggle_focus();
    assert_eq!(app.panels.settings, before);
}

#[test]
fn panel_handle_drag_persists_width_and_focus_does_not_overwrite_it() {
    let mut app = fixture(SIMPLE);
    let size = vec2(1366.0, 768.0);
    let (out, canvas) = workspace_frame(&mut app, size, vec![]);
    out.drop_without_applying_deltas();
    let start = pos2(canvas.right(), 350.0);
    let end = start - vec2(60.0, 0.0);
    let button = |pos, pressed| egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed, modifiers: Modifiers::NONE };
    for events in [vec![egui::Event::PointerMoved(start)], vec![button(start, true)], vec![egui::Event::PointerMoved(end)], vec![button(end, false)], vec![]] {
        workspace_frame(&mut app, size, events).0.drop_without_applying_deltas();
    }
    assert!(app.panels.layout().inspector_width > 300.0, "resize handle must update persistent layout: {:?}", app.panels.layout());
    let width = app.panels.layout().inspector_width;
    app.panels.toggle_focus();
    workspace_frame(&mut app, size, vec![]).0.drop_without_applying_deltas();
    app.panels.toggle_focus();
    workspace_frame(&mut app, size, vec![]).0.drop_without_applying_deltas();
    assert_eq!(app.panels.layout().inspector_width, width);
}

#[test]
fn top_workspace_buttons_and_focus_are_clickable_without_document_edits() {
    let mut app = fixture(SIMPLE);
    let original = app.file_text();
    let size = vec2(900.0, 560.0);
    for label in ["Анимация", "Настройка", "Рисование", "Фокус", "Вернуть панели"] {
        workspace_frame(&mut app, size, vec![]).0.drop_without_applying_deltas();
        let (out, _) = workspace_frame(&mut app, size, vec![]);
        let pos = out
            .shapes
            .iter()
            .find_map(|shape| match &shape.shape {
                egui::Shape::Text(text) if text.galley.job.text == label && text.pos.y < 110.0 => {
                    let center = text.pos + text.galley.size() * 0.5;
                    assert!(shape.clip_rect.contains(center), "{label} is clipped: {center:?}");
                    Some(center)
                }
                _ => None,
            })
            .unwrap_or_else(|| panic!("missing workspace control {label}"));
        out.drop_without_applying_deltas();
        for pressed in [true, false] {
            workspace_frame(
                &mut app,
                size,
                vec![
                    egui::Event::PointerMoved(pos),
                    egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed, modifiers: Modifiers::NONE },
                ],
            )
            .0
            .drop_without_applying_deltas();
        }
        match label {
            "Анимация" => assert_eq!(app.workspace(), Workspace::Animate),
            "Настройка" => assert_eq!(app.workspace(), Workspace::Setup),
            "Рисование" => assert_eq!(app.workspace(), Workspace::Drawing),
            "Фокус" => assert!(app.panels.focus.is_some()),
            _ => assert!(app.panels.focus.is_none()),
        }
        assert!(app.file_text() == original && app.undo.is_empty());
    }
}

#[test]
fn explicit_structural_deletion_is_available_and_undoable_in_animation() {
    let mut app = fixture(SIMPLE);
    app.create_animation();
    app.select_only(Some("box".into()));
    let before = app.file_text();
    assert_eq!(app.delete_selected_nodes(), 1);
    assert!(app.scene.doc.node("box").is_none());
    app.undo();
    assert!(app.file_text() == before);
}

#[test]
fn explicit_transform_keys_still_record_with_autokey_off_and_preserve_other_probes() {
    let mut app = fixture(SIMPLE);
    app.create_animation();
    app.select_only(Some("box".into()));
    app.time = 0.3;
    app.auto_key = false;
    let before = app.file_text();
    let history = app.undo.len();
    app.edit("box", Prop::X, Value::Num(80.0));
    app.edit("box", Prop::Fill, Value::Color(motif_core::Color::WHITE));
    app.record_transform();
    assert!(!app.auto_key);
    assert_eq!(app.current_anim().unwrap().tracks.len(), 5);
    assert_eq!(app.display_doc().node("box").unwrap().x, 80.0);
    assert!(app.has_draft_pose());
    assert_eq!(app.undo.len(), history + 1);
    app.undo();
    assert!(app.file_text() == before);
}

#[test]
fn document_purpose_remains_editable_separately_from_the_temporary_profile() {
    let mut app = fixture(SIMPLE);
    let original = app.file_text();
    app.set_tool_profile(Purpose::Game);
    assert!(app.file_text() == original && app.undo.is_empty());
    app.set_document_purpose(Purpose::Ui);
    assert_eq!(app.scene.doc.purpose, Purpose::Ui);
    assert_eq!(app.tool_profile(), Purpose::Ui);
    assert_eq!(app.undo.len(), 1);
    app.undo();
    assert!(app.file_text() == original);
}

#[test]
fn drawing_edits_the_drawing_for_every_clip_and_names_clips_that_key_the_property() {
    let mut app = fixture(include_str!("../../../examples/bounce.msc"));
    app.set_workspace(Workspace::Animate);
    app.select_anim("Idle".into());
    app.time = 0.4;
    app.set_workspace(Workspace::Drawing);
    assert_eq!(app.anim.as_deref(), Some("Idle"), "the clip waits for «Анимация»");
    assert_eq!(app.key_time(), None);
    let clips = app.scene.doc.animations.clone();
    let keyed: Vec<String> = clips.iter().filter(|a| a.tracks.iter().any(|t| t.node == "ball" && t.prop == Prop::Y)).map(|a| a.name.clone()).collect();
    assert!(!keyed.is_empty());
    app.edit("ball", Prop::Y, Value::Num(77.0));
    assert_eq!(app.scene.doc.animations, clips, "no key in «Рисование»");
    assert_eq!(app.scene.doc.node("ball").unwrap().y, 77.0);
    assert_eq!(app.display_doc().node("ball").unwrap().y, 77.0, "the canvas shows the drawing, not the pose");
    assert!(keyed.iter().all(|k| app.status.contains(k.as_str())), "{}", app.status);
    app.set_workspace(Workspace::Animate);
    assert_eq!(app.time, 0.4);
    assert_eq!(app.display_doc(), app.pose_anim(app.current_anim().unwrap(), 0.4));
}
