use super::*;
use motif_core::kurbo::Shape;
use motif_core::{Document, Prop, Value};

const FACE: &str = include_str!("../../../examples/character-anime.msc");

fn pose(doc: &Document, turn: f64, blink: f64, mouth: f64, corrected: bool) -> Document {
    let mut out = doc.clone();
    if !corrected {
        out.correctives.clear();
    }
    for (id, value) in [("turn", turn), ("blink", blink), ("mouth", mouth)] {
        assert!(out.set_parameter(id, value));
    }
    out.apply_parameters();
    out.rig();
    out
}

#[test]
fn face_corrections_are_inactive_for_the_neutral_turn_and_preserve_ui() {
    let source = parse(FACE).unwrap();
    assert_eq!(source.doc.correctives.len(), 2);
    for blink in [0.0, 0.5, 1.0] {
        for mouth in [-1.0, 0.0, 1.0] {
            let a = pose(&source.doc, 0.0, blink, mouth, false);
            let b = pose(&source.doc, 0.0, blink, mouth, true);
            for (a, b) in a.nodes.iter().zip(&b.nodes) {
                assert_eq!(a.path(), b.path(), "{}", a.id);
            }
        }
    }
    for turn in [-1.0, -0.4, 0.0, 0.4, 1.0] {
        for blink in [0.0, 0.5, 1.0] {
            for mouth in [-1.0, 0.0, 1.0] {
                let mut displayed = pose(&source.doc, turn, blink, mouth, true);
                for id in ["ui_test", "ui_plate", "ui_label"] {
                    assert_eq!(displayed.node(id), source.doc.node(id), "UI {id}");
                }
                for n in &displayed.nodes {
                    if let Some(path) = n.path() {
                        assert!(path.elements().iter().all(|p| p.is_finite()), "{}", n.id);
                    }
                }
                let before = displayed.nodes.iter().map(|n| n.path()).collect::<Vec<_>>();
                displayed.apply_parameters();
                displayed.rig();
                assert_eq!(displayed.nodes.iter().map(|n| n.path()).collect::<Vec<_>>(), before, "correction evaluation must not accumulate");
            }
        }
    }
}

#[test]
fn turned_lid_correction_tightens_the_far_half_blink_without_reopening_closed_masks() {
    let doc = parse(FACE).unwrap().doc;
    for turn in [-1.0, 1.0] {
        let before = pose(&doc, turn, 0.5, 1.0, false);
        let corrected = pose(&doc, turn, 0.5, 1.0, true);
        let far = if turn > 0.0 { "white_r" } else { "white_l" };
        let before_height = before.node(far).unwrap().path().unwrap().bounding_box().height();
        let after_height = corrected.node(far).unwrap().path().unwrap().bounding_box().height();
        assert!(after_height < before_height - 0.2, "far half-blink must close faster: {before_height} -> {after_height}");
        for blink in [0.0, 0.5, 1.0] {
            let displayed = pose(&doc, turn, blink, 1.0, true);
            for side in ["l", "r"] {
                let white = displayed.node(&format!("white_{side}")).unwrap().path().unwrap();
                let mask = displayed.node(&format!("eye_clip_{side}")).unwrap().path().unwrap();
                assert_eq!(white, mask, "the eyeball's white and mask must deform together");
                if blink == 1.0 {
                    assert!(white.bounding_box().height() < 0.001);
                    assert!(white.area().abs() < 0.001, "a fully closed eye must have no visible fill");
                }
            }
        }
    }
}

#[test]
fn turned_mouth_correction_preserves_expression_and_composes_with_blink_and_explicit_keys() {
    let source = parse(FACE).unwrap();
    for turn in [-1.0, 1.0] {
        for mouth in [-1.0, 1.0] {
            let before = pose(&source.doc, turn, 1.0, mouth, false);
            let corrected = pose(&source.doc, turn, 1.0, mouth, true);
            let width = |d: &Document| d.node("mouth_line").unwrap().path().unwrap().bounding_box().width();
            assert!(width(&corrected) < width(&before) - 0.2, "turned lips should compress toward the far cheek");
            assert_eq!(corrected.node("mouth_line").unwrap().get(Prop::Contour), Value::Num(if mouth > 0.0 { 1.0 } else { 2.0 }));
            for side in ["l", "r"] {
                assert!(corrected.node(&format!("white_{side}")).unwrap().path().unwrap().bounding_box().height() < 0.001);
            }
        }
    }
    let test = format!("{FACE}\nanim OverrideCorrection 1s {{\nturn.value 0: 1\nblink.value 0: 0.5\nmouth.value 0: 1\neye_r.warp-y0 0: 123\n}}\n");
    let doc = parse(&test).unwrap().doc;
    assert_eq!(doc.animation("OverrideCorrection").unwrap().pose(&doc, 0.0).node("eye_r").unwrap().get(Prop::WarpY(0)), Value::Num(123.0));
    assert!(doc.animation("Combinations").is_some());
}
