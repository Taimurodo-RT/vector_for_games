use super::*;
use motif_core::kurbo::{BezPath, Point, Shape, Vec2};
use motif_core::{Prop, Value};

#[test]
fn anime_turn_has_a_convex_centerline_and_compresses_the_far_eye() {
    let src = include_str!("../../../examples/character-anime.msc");
    let scene = parse(src).unwrap();
    for turn in [-1.0, 1.0] {
        let mut pose = scene.doc.clone();
        pose.set_parameter("turn", turn);
        pose.apply_parameters();
        pose.rig();
        let head = pose.node("head").unwrap().lattice.as_ref().unwrap();
        let left = head.map(Point::new(-150.0, 0.0));
        let right = head.map(Point::new(150.0, 0.0));
        assert!(((left.x + right.x) / 2.0).abs() < 1e-9, "silhouette must not slide against the turn");
        assert!(left.x > -150.0 && right.x < 150.0, "both edges compress toward the pivot");
        assert!(head.map(Point::ZERO).x * turn > 15.0, "the surface center protrudes with the turn");

        let point = |id: &str, p: Point| {
            let i = pose.index_of(id).unwrap();
            pose.world_transform(i) * pose.map_lattice_point(i, p)
        };
        let nose_tip = match pose.node("nose").unwrap().editable_path().elements()[1] {
            motif_core::kurbo::PathEl::LineTo(p) => p,
            _ => panic!("nose tip landmark changed"),
        };
        let nose = point("nose", nose_tip);
        let mouth = point("mouth_line", Point::ZERO);
        assert!((nose.x - mouth.x) * turn > 10.0, "nose projects beyond the mouth's centerline");
        let width = |id: &str| pose.node(id).unwrap().path().unwrap().bounding_box().width();
        let (near, far) = if turn > 0.0 { ("white_l", "white_r") } else { ("white_r", "white_l") };
        assert!(width(near) > width(far) + 5.0, "the eye toward the far silhouette must narrow");
    }
}

#[test]
fn anime_head_turn_stays_mirrored_and_unfolded_across_its_parameter_range() {
    let src = include_str!("../../../examples/character-anime.msc");
    let scene = parse(src).unwrap();
    for amount in [0.0, 0.25, 0.5, 0.75, 1.0] {
        let mut poses = [scene.doc.clone(), scene.doc.clone()];
        for (pose, sign) in poses.iter_mut().zip([-1.0, 1.0]) {
            pose.set_parameter("turn", amount * sign);
            pose.apply_parameters();
        }
        let negative = poses[0].node("head").unwrap().lattice.as_ref().unwrap();
        let positive = poses[1].node("head").unwrap().lattice.as_ref().unwrap();
        for y in (-180..=180).step_by(30) {
            for x in (-150..=150).step_by(25) {
                let p = Point::new(x as f64, y as f64);
                let a = positive.map(p);
                let b = negative.map(Point::new(-p.x, p.y));
                assert!((a.x + b.x).abs() < 1e-9 && (a.y - b.y).abs() < 1e-9);
                // Keep a positive local Jacobian: a visually hollow face must not be
                // 'fixed' by turning its cage inside out or folding the far cheek.
                let dx = (positive.map(p + Vec2::new(0.01, 0.0)) - a) / 0.01;
                let dy = (positive.map(p + Vec2::new(0.0, 0.01)) - a) / 0.01;
                assert!(dx.cross(dy) > 0.5);
            }
        }
    }
}

#[test]
fn anime_parameter_combinations_keep_masks_paths_and_ui_finite() {
    let src = include_str!("../../../examples/character-anime.msc");
    let scene = parse(src).unwrap();
    crate::character::read(src).unwrap().unwrap().validate(&scene).unwrap();
    let before = scene.doc.clone();
    for turn in [-1.0, 0.0, 1.0] {
        for blink in [0.0, 0.5, 1.0] {
            for mouth in [-1.0, 0.0, 1.0] {
                let mut pose = scene.doc.clone();
                for (id, value) in [("turn", turn), ("blink", blink), ("mouth", mouth), ("look", turn)] {
                    pose.set_parameter(id, value);
                }
                pose.apply_parameters();
                pose.rig();
                for n in &pose.nodes {
                    if let Some(p) = n.path() {
                        assert!(p.elements().iter().all(|e| e.is_finite()), "{}", n.id);
                    }
                }
                assert_eq!(pose.node("ui_plate").unwrap(), scene.doc.node("ui_plate").unwrap());
                if blink == 1.0 {
                    assert!(pose.node("white_l").unwrap().path().unwrap().bounding_box().height() < 0.01);
                }
            }
        }
    }
    assert_eq!(scene.doc, before);
}

#[test]
fn transitions_blend_explicit_keys_after_drivers_and_contours_by_actual_paths() {
    let src = "artboard 200x200\npath box d \"M 0 0 L 20 0 L 20 20 Z\" forms \"M 100 0 L 120 0 L 120 20 Z\" \"M -100 0 L -80 0 L -80 20 Z\"\nparameter p range -1 1 default 0 value 0\ndriver p {\nbox.x -1: -20 | 1: 20\n}\nanim Left 1s loop {\np.value 0: -1\nbox.x 0: 50\nbox.contour 0: 2\n}\nanim Right 1s loop {\np.value 0: 1\nbox.x 0: 70\nbox.contour 0: 0\n}\nmachine M {\ninput go false\nstate Left anim Left\nstate Right anim Right\nLeft -> Right when go blend 1s\n}\n";
    let scene = parse(src).unwrap();
    let m = &scene.doc.machines[0];
    let mut player = motif_core::MachinePlayer::new(m);
    player.set(m, "go", 1.0);
    player.advance(m, &scene.doc, 0.01);
    player.advance(m, &scene.doc, 0.5);
    let pose = player.pose(m, &scene.doc);
    assert!((pose.node("box").unwrap().x - 60.0).abs() < 1e-9);
    assert!((pose.node("box").unwrap().path().unwrap().bounding_box().x0 + 50.0).abs() < 1e-9);
}

const CAGE: &str = "artboard \"Rig\" 400x400\ngroup face at 200 200 deform 200 200 {\npath shape d \"M -50 -40 C -20 -90 30 -90 50 -40 L 50 60 L -50 60 Z M -10 -10 L 10 -10 L 10 10 L -10 10 Z\" fill #ffaaaa evenodd\n}\nanim Idle 2s {\nface.warp-x4 0: 0 | 1: 35 | 2: 0\n}\n";

#[test]
fn neutral_lattice_preserves_exact_beziers_and_repeated_evaluation_is_stable() {
    let scene = parse(CAGE).unwrap();
    let mut doc = scene.doc.clone();
    let base = doc.node("shape").unwrap().geometry.clone();
    doc.rig();
    assert_eq!(doc.node("shape").unwrap().path().unwrap(), base);
    let mut pose = scene.doc.animations[0].pose(&scene.doc, 1.0);
    let once = pose.node("shape").unwrap().path().unwrap();
    assert_ne!(once, base);
    assert_eq!(once.elements().iter().filter(|e| matches!(e, motif_core::kurbo::PathEl::ClosePath)).count(), 2);
    pose.rig();
    assert_eq!(pose.node("shape").unwrap().path().unwrap(), once);
    assert_eq!(scene.doc.node("shape").unwrap().geometry, base);
}

#[test]
fn nested_reflected_cages_compose_and_inverse_handles_return_to_the_source() {
    let src = "artboard \"Nested\" 400x400\ngroup outer at 200 200 scale -1 1 rot 15 deform 300 300 {\ngroup inner at 20 5 rot 20 deform 120 100 {\nrect shape size 60 50 fill #ffffff\n}\n}\n";
    let mut doc = parse(src).unwrap().doc;
    doc.nodes[0].lattice.as_mut().unwrap().offsets = [Vec2::new(15.0, 0.0); 9];
    doc.nodes[1].lattice.as_mut().unwrap().offsets = [Vec2::new(0.0, 12.0); 9];
    let index = doc.index_of("shape").unwrap();
    let p = Point::new(12.0, 8.0);
    let mapped = doc.map_lattice_point(index, p);
    assert!((mapped - p).hypot() > 10.0);
    assert!((doc.unmap_lattice_point(index, mapped) - p).hypot() < 1e-5);
    doc.rig();
    let once = doc.nodes[index].path().unwrap();
    assert!(once.elements().iter().all(|e| e.is_finite()));
    doc.rig();
    assert_eq!(doc.nodes[index].path().unwrap(), once);
}

#[test]
fn skin_then_lattice_keeps_the_bone_pose_and_adds_group_deformation() {
    let src = "artboard \"Skin\" 300x300\ngroup rig at 150 150 deform 200 200 {\nbone b len 100\npath shape d \"M 0 -10 L 90 -10 L 90 10 L 0 10 Z\" skin b fill #ffffff\n}\nanim Move 1s {\nb.rot 0: 0 | 1: 35\n}\n";
    let doc = parse(src).unwrap().doc;
    let mut bent = doc.animations[0].pose(&doc, 0.8);
    let before = bent.node("shape").unwrap().path().unwrap().bounding_box();
    bent.nodes[0].lattice.as_mut().unwrap().offsets = [Vec2::new(17.0, 5.0); 9];
    bent.rig();
    let after = bent.node("shape").unwrap().path().unwrap().bounding_box();
    assert!((after.x0 - before.x0 - 17.0).abs() < 0.2);
    assert!((after.y0 - before.y0 - 5.0).abs() < 0.2);
}

#[test]
fn contour_keys_interpolate_authored_paths_without_visiting_another_clips_forms() {
    let src = "artboard \"Forms\" 200x200\npath shape d \"M 0 0 L 20 0 L 20 20 Z\" fill #ffffff\nanim A 2s {\n}\nanim B 2s {\n}\n";
    let a = BezPath::from_svg("M 40 0 L 60 0 L 60 20 Z").unwrap();
    let b = BezPath::from_svg("M -80 0 L -60 0 L -60 20 Z").unwrap();
    let c = BezPath::from_svg("M 80 0 L 100 0 L 100 20 Z").unwrap();
    let mut out = src.to_string();
    for (clip, time, path) in [("A", 1.0, &a), ("B", 1.0, &b), ("A", 2.0, &c)] {
        out = forms::set_contour_key(&out, &parse(&out).unwrap(), clip, "shape", time, path, None).unwrap();
    }
    let doc = parse(&out).unwrap().doc;
    let pose = doc.animation("A").unwrap().pose(&doc, 1.5);
    assert!((pose.node("shape").unwrap().path().unwrap().bounding_box().x0 - 60.0).abs() < 1e-9);
    assert_eq!(doc.animation("B").unwrap().pose(&doc, 1.0).node("shape").unwrap().path().unwrap(), b);
    let revised = BezPath::from_svg("M 42 0 L 62 0 L 62 20 Z").unwrap();
    out = forms::set_contour_key(&out, &parse(&out).unwrap(), "A", "shape", 1.0, &revised, None).unwrap();
    let reopened = parse(&out).unwrap();
    assert_eq!(reopened.doc.node("shape").unwrap().forms.as_ref().unwrap().shapes.len(), 3, "editing one frame reuses its unshared form");
    assert_eq!(reopened.doc.animation("A").unwrap().pose(&reopened.doc, 1.0).node("shape").unwrap().path().unwrap(), revised);
    assert_eq!(reopened.doc.node("shape").unwrap().geometry, parse(src).unwrap().doc.node("shape").unwrap().geometry);
}

#[test]
fn appearance_is_discrete_and_can_move_to_the_start() {
    let src = "artboard \"Birth\" 100x100\nrect new size 20\nanim A 1s {\n}\n";
    let out = forms::key_appearance(src, &parse(src).unwrap(), "A", "new", 0.5, None).unwrap();
    let scene = parse(&out).unwrap();
    let a = &scene.doc.animations[0];
    assert!(!a.pose(&scene.doc, 0.49).node("new").unwrap().visible);
    assert!(a.pose(&scene.doc, 0.5).node("new").unwrap().visible);
    let out = move_key(&out, &scene, "A", "new", Prop::Visible, 0.5, 0.0).unwrap();
    let doc = parse(&out).unwrap().doc;
    assert!(doc.animations[0].pose(&doc, 0.0).node("new").unwrap().visible);
}

#[test]
fn direct_animation_keys_can_override_a_driver_without_destroying_its_response() {
    let src = "artboard \"Override\" 100x100\nrect box size 20\nparameter turn range -1 1 default 0 value 0\ndriver turn {\nbox.x -1: -20 | 1: 20\n}\nanim A 1s {\nturn.value 0: 1 | 1: -1\nbox.x 0: 50 | 1: 60\n}\n";
    let doc = parse(src).unwrap().doc;
    assert_eq!(doc.animations[0].pose(&doc, 0.5).node("box").unwrap().x, 55.0);
    assert_eq!(doc.drivers[0].tracks[0].keys[0].value, Value::Num(-20.0));
}

#[test]
fn malformed_deformers_and_contour_indices_are_rejected() {
    for src in [
        CAGE.replace("deform 200 200", "deform 0 200"),
        CAGE.replace("group face", "rect face"),
        CAGE.replace("deform 200 200", ""),
        "artboard \"Bad\" 100x100\npath p d \"M 0 0 L 10 10\" forms \"not a path\"\n".into(),
        "artboard \"Bad\" 100x100\npath p d \"M 0 0 L 10 10\" forms \"M 0 0 L 20 20\"\nanim A 1s {\np.contour 0: 4\n}\n".into(),
    ] {
        assert!(parse(&src).is_err());
    }
}
