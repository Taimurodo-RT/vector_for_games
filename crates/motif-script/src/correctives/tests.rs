use super::*;
use motif_core::{MachinePlayer, Value};

const BASE: &str = "artboard \"Correctives\" 240x240\nparameter turn range -1 1 default 0 value 0\nparameter blink range 0 1 default 0 value 0\ngroup head at 120 120 deform 200 200 warp-x4 10 warp-y4 3 {\nrect face size 60 80 fill #ffeedd\n}\nrect ui_plate at 20 20 size 30 ui button fill #cccccc\ndriver turn {\nhead.warp-x4 -1: 10 | 0: 10 | 1: 10\n}\nanim Test 2s {\nturn.value 0: -1 linear | 2: 1\nblink.value 0: 0 linear | 2: 1\n}\n";
const CORRECTION: &str = "corrective lids parameters turn blink {\nhead.warp-x4 -1 0: 0 | -1 1: -20 | 0 0: 0 | 0 1: 0 | 1 0: 0 | 1 1: 40\nhead.warp-y4 -1 0: 0 | -1 1: 4 | 0 0: 0 | 0 1: 0 | 1 0: 0 | 1 1: 8\n}\n";

fn scene() -> Scene {
    crate::parse(&format!("{BASE}{CORRECTION}")).unwrap()
}

#[test]
fn bilinear_correctives_add_after_drivers_without_mutating_source_offsets_or_accumulating() {
    let s = scene();
    let before = s.doc.clone();
    let mut pose = s.doc.clone();
    pose.set_parameter("turn", 0.5);
    pose.set_parameter("blink", 0.5);
    pose.apply_parameters();
    assert_eq!(pose.node("head").unwrap().get(Prop::WarpX(4)), Value::Num(20.0));
    assert_eq!(pose.node("head").unwrap().get(Prop::WarpY(4)), Value::Num(5.0));
    assert_eq!(pose.node("head").unwrap().lattice.as_ref().unwrap().offsets[4].x, 10.0);
    pose.rig();
    let once = pose.node("face").unwrap().path().unwrap();
    for _ in 0..5 {
        pose.apply_parameters();
        pose.rig();
        assert_eq!(pose.node("face").unwrap().path().unwrap(), once);
    }
    assert_eq!(pose.node("ui_plate").unwrap(), s.doc.node("ui_plate").unwrap());
    assert_eq!(s.doc, before);
    pose.set_parameter("turn", 0.0);
    pose.apply_parameters();
    assert_eq!(pose.node("head").unwrap().get(Prop::WarpX(4)), Value::Num(10.0));
}

#[test]
fn explicit_keys_and_live_absolute_values_override_only_the_keyed_warp_coordinate() {
    let src = format!("{BASE}{CORRECTION}anim Direct 1s {{\nturn.value 0: 1\nblink.value 0: 1\nhead.warp-x4 0: 75\n}}\n");
    let s = crate::parse(&src).unwrap();
    let mut pose = s.doc.animation("Direct").unwrap().pose(&s.doc, 0.0);
    assert_eq!(pose.node("head").unwrap().get(Prop::WarpX(4)), Value::Num(75.0));
    assert_eq!(pose.node("head").unwrap().get(Prop::WarpY(4)), Value::Num(11.0));
    pose.nodes[0].set(Prop::WarpX(4), Value::Num(90.0));
    assert_eq!(pose.node("head").unwrap().get(Prop::WarpX(4)), Value::Num(90.0));
    assert_eq!(pose.node("head").unwrap().get(Prop::WarpY(4)), Value::Num(11.0));
}

#[test]
fn clips_use_two_parameter_coordinates_and_machine_blends_parameters_before_correcting() {
    let s = scene();
    let a = s.doc.animation("Test").unwrap();
    assert_eq!(a.pose(&s.doc, 1.5).node("head").unwrap().get(Prop::WarpX(4)), Value::Num(25.0));
    let src = format!(
        "{BASE}{CORRECTION}anim Left 1s loop {{\nturn.value 0: -1\nblink.value 0: 1\n}}\nanim Right 1s loop {{\nturn.value 0: 1\nblink.value 0: 1\n}}\nmachine M {{\ninput go false\nstate Left anim Left\nstate Right anim Right\nLeft -> Right when go blend 1s\n}}\n"
    );
    let s = crate::parse(&src).unwrap();
    let m = &s.doc.machines[0];
    let mut p = MachinePlayer::new(m);
    p.set(m, "go", 1.0);
    p.advance(m, &s.doc, 0.01);
    p.advance(m, &s.doc, 0.5);
    assert_eq!(
        p.pose(m, &s.doc).node("head").unwrap().get(Prop::WarpX(4)),
        Value::Num(10.0),
        "sample the center residual, rather than averaging asymmetric endpoint residuals"
    );
}

#[test]
fn source_authoring_creates_neutral_tables_and_edits_one_shared_anchor() {
    let s = crate::parse(BASE).unwrap();
    let out = add_lattice(BASE, &s, "new_lids", "head", ["turn", "blink"]).unwrap();
    let s = crate::parse(&out).unwrap();
    assert_eq!(s.doc.correctives[0].tracks.len(), 18);
    assert_eq!(s.doc.correctives[0].tracks[0].keys.len(), 6);
    let original_clips = s.doc.animations.clone();
    let ordinary = s.doc.drivers.clone();
    let changed = set_value(&out, &s, "new_lids", "head", Prop::WarpX(4), 1.0, 1.0, 20.0).unwrap();
    let s = crate::parse(&changed).unwrap();
    assert_eq!(s.doc.animations, original_clips);
    assert_eq!(s.doc.drivers, ordinary);
    assert_eq!(s.doc.correctives[0].tracks.iter().find(|t| t.prop == Prop::WarpX(4)).unwrap().sample(0.5, 0.5), 5.0);
    assert!(crate::parse(&remove(&changed, &s, "new_lids").unwrap()).unwrap().doc.correctives.is_empty());
}

#[test]
fn invalid_tables_are_rejected_with_context_instead_of_silent_partial_forms() {
    for invalid in [
        CORRECTION.replace("parameters turn blink", "parameters turn turn"),
        CORRECTION.replace("parameters turn blink", "parameters turn unknown"),
        CORRECTION.replace("head.warp-x4", "head.opacity"),
        CORRECTION.replace("head.warp-x4", "ui_plate.warp-x4"),
        CORRECTION.replace(" | 1 1: 40", ""),
        CORRECTION.replace("-1 1: -20", "-1 0: -20"),
        CORRECTION.replace("1 1: 40", "2 1: 40"),
        format!("{CORRECTION}{CORRECTION}"),
    ] {
        assert!(crate::parse(&format!("{BASE}{invalid}")).is_err(), "{invalid}");
    }
    assert!(add_lattice(BASE, &crate::parse(BASE).unwrap(), "invalid name", "head", ["turn", "blink"]).is_none());
}

#[test]
fn deleting_targets_cleans_corrective_tracks_and_copy_keeps_existing_references_valid() {
    let s = scene();
    let src = format!("{BASE}{CORRECTION}");
    let snips = crate::copy_nodes(&s, &src, &["head".into()]);
    let (copy, ids) = crate::paste_nodes(&src, &s, &snips, (0.0, 0.0));
    let c = crate::parse(&copy).unwrap();
    assert_eq!(c.doc.correctives, s.doc.correctives);
    assert!(c.doc.node(&ids[0]).is_some());
    let out = crate::delete_node(&src, &s, "head").unwrap();
    let after = crate::parse(&out).unwrap();
    assert!(after.doc.correctives[0].tracks.is_empty());
    assert!(after.doc.node("ui_plate").is_some());
    let named = crate::rename_node(&src, &s, "head", "Новое имя").unwrap();
    assert_eq!(crate::parse(&named).unwrap().doc.correctives, s.doc.correctives);
}

#[test]
fn multiple_residuals_add_commutatively_and_sampling_clamps_at_table_boundaries() {
    let first = CORRECTION;
    let second = CORRECTION.replace("lids parameters", "extra parameters");
    let mut a = crate::parse(&format!("{BASE}{first}{second}")).unwrap().doc;
    let mut b = crate::parse(&format!("{BASE}{second}{first}")).unwrap().doc;
    for d in [&mut a, &mut b] {
        d.set_parameter("turn", 1.0);
        d.set_parameter("blink", 1.0);
        d.apply_parameters();
    }
    assert_eq!(a.node("head").unwrap(), b.node("head").unwrap());
    assert_eq!(a.node("head").unwrap().get(Prop::WarpX(4)), Value::Num(90.0));
    assert_eq!(a.correctives[0].tracks[0].sample(200.0, 200.0), 40.0);
}

#[test]
fn runtime_nonfinite_control_uses_the_same_default_for_drivers_and_correctives() {
    let mut doc = scene().doc;
    doc.parameters.iter_mut().find(|p| p.id == "turn").unwrap().value = f64::NAN;
    doc.set_parameter("blink", 1.0);
    doc.apply_parameters();
    assert_eq!(doc.node("head").unwrap().get(Prop::WarpX(4)), Value::Num(10.0));
    assert_eq!(doc.node("head").unwrap().get(Prop::WarpY(4)), Value::Num(3.0));
}

#[test]
fn editing_a_missing_warp_coordinate_extends_a_partial_form_with_a_neutral_grid() {
    let partial = CORRECTION.lines().filter(|l| !l.starts_with("head.warp-x4")).collect::<Vec<_>>().join("\n");
    let src = format!("{BASE}{partial}\n");
    let s = crate::parse(&src).unwrap();
    let prior = s.doc.correctives[0].tracks[0].clone();
    let out = set_value(&src, &s, "lids", "head", Prop::WarpX(4), 1.0, 1.0, 13.5).unwrap();
    let reopened = crate::parse(&out).unwrap();
    let tracks = &reopened.doc.correctives[0].tracks;
    assert_eq!(tracks[0], prior);
    assert_eq!(tracks.len(), 2);
    assert_eq!(tracks[1].axes(), prior.axes());
    assert_eq!(tracks[1].sample(1.0, 1.0), 13.5);
    assert_eq!(tracks[1].sample(0.0, 1.0), 0.0);
    assert_eq!(tracks[1].sample(1.0, 0.0), 0.0);
    let refined = set_value(&src, &s, "lids", "head", Prop::WarpX(4), 0.5, 0.5, 2.0).unwrap();
    let refined = crate::parse(&refined).unwrap();
    assert_eq!(refined.doc.correctives[0].tracks[1].sample(0.5, 0.5), 2.0);
    assert!(set_value(&src, &s, "lids", "head", Prop::WarpX(4), 2.0, 0.5, 2.0).is_none());
    assert!(set_value(&src, &s, "lids", "ui_plate", Prop::WarpX(4), 1.0, 1.0, 2.0).is_none(), "target still requires a lattice");
    assert_eq!(reopened.doc.animations, s.doc.animations);
    assert_eq!(reopened.doc.drivers, s.doc.drivers);
}

#[test]
fn editing_different_existing_grids_refines_one_surface_without_changing_other_anchors() {
    let table = "corrective mixed parameters turn blink {\nhead.warp-x4 -1 0: 0 | -1 0.5: 2 | -1 1: 0 | 0 0: 0 | 0 0.5: 0 | 0 1: 0 | 1 0: 0 | 1 0.5: 2 | 1 1: 0\nhead.warp-y4 -1 0: 2 | -1 1: 6 | 1 0: 10 | 1 1: 18\n}\n";
    let src = format!("{BASE}{table}");
    let s = crate::parse(&src).unwrap();
    let original = &s.doc.correctives[0].tracks[1];
    let sample = original.sample(0.0, 0.5);
    let refined = set_value(&src, &s, "mixed", "head", Prop::WarpY(4), 0.0, 0.5, sample).unwrap();
    let r = crate::parse(&refined).unwrap();
    let expanded = &r.doc.correctives[0].tracks[1];
    assert_eq!(expanded.axes(), (vec![-1.0, 0.0, 1.0], vec![0.0, 0.5, 1.0]));
    for x in [-1.0, -0.7, 0.0, 0.3, 1.0] {
        for y in [0.0, 0.2, 0.5, 0.75, 1.0] {
            assert!((original.sample(x, y) - expanded.sample(x, y)).abs() < 1e-9);
        }
    }
    let changed = set_value(&src, &s, "mixed", "head", Prop::WarpY(4), 0.0, 0.5, 11.0).unwrap();
    let c = crate::parse(&changed).unwrap();
    let target = &c.doc.correctives[0].tracks[1];
    assert_eq!(target.sample(0.0, 0.5), 11.0);
    for key in &original.keys {
        assert_eq!(target.sample(key.x, key.y), key.value);
    }
    assert_eq!(c.doc.correctives[0].tracks[0], s.doc.correctives[0].tracks[0]);
    assert_eq!(c.doc.animations, s.doc.animations);
    assert_eq!(c.doc.drivers, s.doc.drivers);
    assert!(set_value(&src, &s, "mixed", "head", Prop::WarpY(4), 2.0, 0.5, 11.0).is_none());
    assert!(set_value(&src, &s, "mixed", "head", Prop::WarpY(4), 0.0, -0.5, 11.0).is_none());
}
