//! The actual App canvas must start UI checks from authored Normal, independently
//! of the clip/frame selected in the shared drawing/animation workspace.
use super::editing_tests::fixture;
use super::*;
use motif_core::kurbo::Point;

fn play_source() -> String {
    motif_html::import(include_str!("../../../examples/import/uiverse-play.html"), include_str!("../../../examples/import/uiverse-play.css")).unwrap().source
}

fn same_drawing(actual: &Document, expected: &Document, context: &str) {
    for original in &expected.nodes {
        let node = actual.node(&original.id).unwrap_or_else(|| panic!("{context}: missing {}", original.id));
        for prop in Prop::ALL {
            assert_eq!(node.get(prop), original.get(prop), "{context}: {}.{prop:?}", original.id);
        }
    }
    let opts = motif_render::CpuOptions { background: false, ..Default::default() };
    let actual = motif_render::render_cpu(actual, opts).unwrap();
    let expected = motif_render::render_cpu(expected, opts).unwrap();
    assert!(actual.data() == expected.data(), "{context}: actual canvas rendering must also match");
}

#[test]
fn ui_check_of_imported_play_ignores_the_selected_authoring_clip_and_restores_it_afterwards() {
    let source = play_source();
    for (clip, time) in [("Hover", 0.0), ("Hover", 0.31), ("HoverLoop", 0.0), ("HoverLoop", 0.33)] {
        let mut app = fixture(&source);
        app.set_workspace(crate::workspace::Workspace::Animate);
        app.select_anim(clip.into());
        app.time = time;
        app.playing = true;
        let selected_pose = app.display_doc();
        assert_ne!(selected_pose.node("leftTop_bg").unwrap().fill, app.scene.doc.node("leftTop_bg").unwrap().fill);
        let authored = app.scene.doc.clone();
        let before = app.file_text();
        app.toggle_check();
        assert!(app.check.is_some() && app.run.is_none());
        assert_eq!(app.anim.as_deref(), Some(clip));
        assert_eq!(app.time, time);
        assert!(!app.playing);
        same_drawing(&app.display_doc(), &authored, &format!("before pointer, selected={clip}@{time}"));

        let button = authored.nodes.iter().find(|n| n.ui_role == motif_core::UiRole::Button).unwrap();
        let center = authored.world_transform(authored.index_of(&button.id).unwrap()) * Point::ZERO;
        let normal = app.display_doc();
        let check = app.check.as_mut().unwrap();
        check.pointer(&normal, Some(center), false);
        assert!(check.control(&button.id).unwrap().hover);
        assert!(check.advance(&authored, 0.85));
        let hovered = app.display_doc();
        assert_eq!(hovered.node("leftTop_bg").unwrap().fill, motif_core::Color::from_hex("#27c39f"));
        assert!(app.check.as_mut().unwrap().advance(&authored, 0.25));
        let continued = app.display_doc();
        let left = |doc: &Document| (doc.world_transform(doc.index_of("leftArrow").unwrap()) * Point::ZERO).x;
        assert!((left(&continued) - left(&hovered)).abs() > 1.0, "the actual App check must continue the hover loop");
        let check = app.check.as_mut().unwrap();
        check.pointer(&continued, None, false);
        check.advance(&authored, 1.0);
        same_drawing(&app.display_doc(), &authored, &format!("after leaving, selected={clip}@{time}"));
        app.toggle_check();
        same_drawing(&app.display_doc(), &selected_pose, &format!("after check stopped, selected={clip}@{time}"));
        assert_eq!(app.time, time);
        assert_eq!(app.file_text(), before);
        assert!(app.undo.is_empty());
    }
}

#[test]
fn importing_play_opens_authored_normal_even_when_the_previous_board_selected_hover() {
    let source = play_source();
    let reopened = fixture(&source);
    assert_eq!(reopened.anim.as_deref(), Some("Normal"));
    same_drawing(&reopened.display_doc(), &reopened.scene.doc, "opening the imported MSC must default to authored normal");
    let mut app =
        fixture("artboard \"Existing\" 200x100 ui\nrect old at 100 50 size 80 30 fill #ff0000 ui\nanim Hover 1s {\n old.fill 0: #ff0000 | 1: #00ff00\n}\n");
    app.select_anim("Hover".into());
    app.time = 0.7;
    let previous = app.file_text();
    assert!(app.add_imported_ui_board(&source));
    assert_eq!(app.time, 0.0);
    assert_eq!(app.anim.as_deref(), Some("Normal"));
    same_drawing(&app.display_doc(), &app.scene.doc, "a fresh imported board must show its authored normal drawing");
    assert_eq!(app.undo.len(), 1, "the whole import must remain one undo step");
    for space in crate::workspace::Workspace::ALL {
        app.set_workspace(space);
        assert_eq!(app.time, 0.0);
        assert_eq!(app.anim.as_deref(), Some("Normal"));
        same_drawing(&app.display_doc(), &app.scene.doc, "workspaces keep the same imported normal frame");
    }
    let imported = app.file_text();
    app.undo();
    assert_eq!(app.file_text(), previous);
    assert_eq!(app.boards.active, 0);
    assert!(app.scene.doc.animation("Hover").is_some(), "the previous board's animation must stay intact");
    app.redo();
    assert_eq!(app.file_text(), imported);
    assert_eq!(app.boards.active, 1);
    assert_eq!(app.anim.as_deref(), Some("Normal"));
    same_drawing(&app.display_doc(), &app.scene.doc, "redo restores the imported board's normal selection");
}

#[test]
fn main_ui_check_and_independent_ui_preview_share_normal_hover_loop_and_leave_poses() {
    let source = play_source();
    let mut app = fixture(&source);
    app.select_anim("HoverLoop".into());
    app.time = 0.33;
    app.preview.sync(&app.scene.doc, true);
    app.preview.set_mode(crate::preview::Mode::Ui);
    app.toggle_check();
    let authored = app.scene.doc.clone();
    let normal = app.display_doc();
    same_drawing(&normal, &app.preview.pose().unwrap(), "main check and UI preview must begin at the same normal drawing");
    let button = authored.nodes.iter().find(|n| n.ui_role == motif_core::UiRole::Button).unwrap();
    let center = authored.world_transform(authored.index_of(&button.id).unwrap()) * Point::ZERO;
    app.check.as_mut().unwrap().pointer(&normal, Some(center), false);
    app.preview.pointer(&normal, Some(center), false);
    for _ in 0..22 {
        assert!(app.check.as_mut().unwrap().advance(&authored, 0.05));
        assert!(app.preview.advance(0.05, None));
    }
    let hovered = app.display_doc();
    same_drawing(&hovered, &app.preview.pose().unwrap(), "the live hover continuation must match both UI surfaces");
    app.check.as_mut().unwrap().pointer(&hovered, None, false);
    app.preview.pointer(&hovered, None, false);
    for _ in 0..20 {
        app.check.as_mut().unwrap().advance(&authored, 0.05);
        app.preview.advance(0.05, None);
    }
    same_drawing(&app.display_doc(), &authored, "the main canvas must return to authored normal");
    same_drawing(&app.preview.pose().unwrap(), &authored, "the independent preview must return to authored normal");
    assert_eq!(app.anim.as_deref(), Some("HoverLoop"));
    assert_eq!(app.time, 0.33);
}

#[test]
fn ui_check_keeps_the_live_pointer_machine_as_its_base() {
    let mut app = fixture(concat!(
        "artboard \"Machine UI\" 240x100 ui\nrect button at 60 50 size 80 30 fill #ff0000 ui\n",
        "anim Move 1s {\n button.x 0: 80 | 1: 160\n}\nanim Hover 1s {\n button.fill 0: #ff0000 | 1: #00ff00\n}\n",
        "machine Pointer {\n input hover false\n listen button hover -> hover\n state Move\n}\n"
    ));
    app.select_anim("Hover".into());
    app.time = 0.6;
    app.toggle_check();
    assert!(app.check.is_some() && app.run.is_some());
    let machine = app.scene.doc.machines[0].clone();
    app.run.as_mut().unwrap().player.advance(&machine, &app.scene.doc, 0.5);
    let shown = app.display_doc();
    assert!((shown.node("button").unwrap().x - 120.0).abs() < 1e-9, "checking UI must preserve a simultaneously running listener machine");
    assert_eq!(shown.node("button").unwrap().fill, motif_core::Color::from_hex("#ff0000"));
}
