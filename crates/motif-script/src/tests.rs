use super::*;
use motif_core::{Color, Prop, ShapeKind, Value};

const EXAMPLE: &str = include_str!("../../../examples/bounce.msc");

fn scene(src: &str) -> Scene {
    match parse(src) {
        Ok(s) => s,
        Err(e) => panic!("{}", e.iter().map(|e| e.to_string()).collect::<Vec<_>>().join("\n")),
    }
}

#[test]
fn example_parses() {
    let s = scene(EXAMPLE);
    let doc = &s.doc;
    assert_eq!(doc.artboard.name, "Главный");
    assert_eq!((doc.artboard.width, doc.artboard.height), (480.0, 360.0));
    assert_eq!(doc.nodes.len(), 11);
    let ball = doc.node("ball").unwrap();
    assert_eq!((ball.width, ball.height), (72.0, 72.0));
    assert_eq!(ball.fill, Color::from_hex("#ff5a36"));
    assert_eq!(ball.stroke_width, 4.0);
    let coin = doc.node("coin3").unwrap();
    assert_eq!(coin.x, 84.0);
    assert_eq!(doc.nodes[coin.parent.unwrap()].id, "coins");
    assert_eq!(doc.animations.len(), 2);
    assert_eq!(doc.animation("Idle").unwrap().tracks.len(), 6);
}

#[test]
fn reports_errors_with_lines() {
    let errs = parse("rect a at 1 2\nrect b sizee 3\nellipse a\n").unwrap_err();
    let lines: Vec<_> = errs.iter().map(|e| e.line).collect();
    assert_eq!(lines, vec![2, 3], "{errs:?}");
    assert!(errs[0].message.contains("sizee"));
}

#[test]
fn edits_literal_in_place() {
    let src = "rect a at 10 20 size 30  // keep me\n";
    let s = scene(src);
    let out = set_prop(src, &s, "a", Prop::X, Value::Num(12.5)).unwrap();
    assert_eq!(out, "rect a at 12.5 20 size 30  // keep me\n");
    let out = set_prop(src, &s, "a", Prop::Height, Value::Num(8.0)).unwrap();
    assert_eq!(out, "rect a at 10 20 size 30 8  // keep me\n");
}

#[test]
fn appends_missing_property() {
    let src = "group g \"Группа\" {\n  star s fill #fff\n}\n";
    let s = scene(src);
    let out = set_prop(src, &s, "s", Prop::Rotation, Value::Num(45.0)).unwrap();
    assert_eq!(out, "group g \"Группа\" {\n  star s fill #fff rot 45\n}\n");
    let out = set_prop(src, &s, "g", Prop::X, Value::Num(5.0)).unwrap();
    assert_eq!(out, "group g \"Группа\" at 5 0 {\n  star s fill #fff\n}\n");
    let out = set_prop(src, &s, "s", Prop::StrokeWidth, Value::Num(3.0)).unwrap();
    assert_eq!(scene(&out).doc.node("s").unwrap().stroke_width, 3.0);
}

#[test]
fn derived_values_go_to_override() {
    let src = "let w = 30\nrect a size w 10\nfor i in 0..2 {\n  rect b{i} at {i * 10} 0\n}\n";
    let s = scene(src);
    let out = set_prop(src, &s, "a", Prop::Width, Value::Num(40.0)).unwrap();
    assert!(out.ends_with("override {\n  a.w = 40\n}\n"), "{out}");
    let s2 = scene(&out);
    assert_eq!(s2.doc.node("a").unwrap().width, 40.0);
    // Second edit to a generated node goes into the same block; editing it again rewrites the entry.
    let out2 = set_prop(&out, &s2, "b1", Prop::Y, Value::Num(7.0)).unwrap();
    let s3 = scene(&out2);
    assert_eq!(s3.doc.node("b1").unwrap().y, 7.0);
    assert_eq!(s3.doc.node("b0").unwrap().y, 0.0);
    let out3 = set_prop(&out2, &s3, "b1", Prop::Y, Value::Num(9.0)).unwrap();
    assert_eq!(out3.matches("b1.y").count(), 1);
    assert_eq!(scene(&out3).doc.node("b1").unwrap().y, 9.0);
}

#[test]
fn every_prop_round_trips_through_edit() {
    let src = "polygon p points 6\n";
    for prop in Prop::ALL {
        let s = scene(src);
        // Grain strength is 0..1.
        let value = if prop.is_color() {
            Value::Color(Color::rgb(1, 2, 3))
        } else if prop == Prop::Noise {
            Value::Num(0.5)
        } else {
            Value::Num(7.0)
        };
        // How far a morph goes means nothing without shapes to flow into, a taper without depth.
        if matches!(prop, Prop::Morph | Prop::Taper) {
            assert_eq!(set_prop(src, &s, "p", prop, value), None);
            continue;
        }
        let out = set_prop(src, &s, "p", prop, value).unwrap();
        let got = scene(&out).doc.node("p").unwrap().get(prop);
        assert_eq!(got, value, "{prop:?}: {out}");
        assert_eq!(scene(&out).doc.nodes[0].kind, ShapeKind::Polygon);
    }
}

#[test]
fn animation_keys_and_eases() {
    let s = scene("ellipse b\nanim Go 2s {\n  b.x 0: 0 linear | 1: 100 hold | 2: 50\n}\n");
    let anim = s.doc.animation("Go").unwrap();
    assert!(!anim.looping);
    let posed = anim.pose(&s.doc, 0.5);
    assert_eq!(posed.node("b").unwrap().x, 50.0);
    let posed = anim.pose(&s.doc, 1.5);
    assert_eq!(posed.node("b").unwrap().x, 100.0);
}

#[test]
fn number_format() {
    assert_eq!(fmt_num(3.0), "3");
    assert_eq!(fmt_num(2.5), "2.5");
    assert_eq!(fmt_num(1.0 / 3.0), "0.33");
    assert_eq!(fmt_num(-0.004), "0");
}

#[test]
fn override_entries_stay_on_adjacent_lines() {
    let mut src = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../examples/bounce.msc")).unwrap();
    for (prop, v) in [(Prop::Y, 10.0), (Prop::Y, 35.0), (Prop::X, 1.0), (Prop::X, 2.0)] {
        let s = scene(&src);
        src = set_prop(&src, &s, "coin0", prop, Value::Num(v)).unwrap();
    }
    assert!(src.ends_with("override {\n  coin0.y = 35\n  coin0.x = 2\n}\n"), "{src}");
}

#[test]
fn set_key_replaces_inserts_and_appends() {
    let src = "rect a at 10 20\nanim Go 1s {\n  a.x  0: 10 linear | 1: 50\n}\n";
    let s = scene(src);
    // Same time: only the value changes, the curve stays.
    let out = set_key(src, &s, "Go", "a", Prop::X, 0.0, Value::Num(12.0)).unwrap();
    assert!(out.contains("a.x  0: 12 linear | 1: 50\n"), "{out}");
    // Between keys: inserted in time order.
    let out = set_key(src, &s, "Go", "a", Prop::X, 0.5, Value::Num(80.0)).unwrap();
    assert!(out.contains("a.x  0: 10 linear | 0.5: 80 | 1: 50\n"), "{out}");
    assert_eq!(scene(&out).doc.animations[0].tracks[0].keys.len(), 3);
    // After the last key: appended.
    let out = set_key(src, &s, "Go", "a", Prop::X, 1.25, Value::Num(0.0)).unwrap();
    assert!(out.contains("| 1: 50 | 1.25: 0\n"), "{out}");
    // Times snap to hundredths, so a nearby time hits the same key.
    let out = set_key(src, &s, "Go", "a", Prop::X, 0.9999, Value::Num(7.0)).unwrap();
    assert!(out.contains("| 1: 7\n"), "{out}");
}

#[test]
fn set_key_creates_track_from_scene_value() {
    let src = "rect a at 10 20\nanim Go 1s {\n  a.x  0: 10 | 1: 50\n}\n";
    let s = scene(src);
    let out = set_key(src, &s, "Go", "a", Prop::Y, 0.4, Value::Num(60.0)).unwrap();
    assert!(out.ends_with("  a.x  0: 10 | 1: 50\n  a.y  0: 20 | 0.4: 60\n}\n"), "{out}");
    let s2 = scene(&out);
    let posed = s2.doc.animations[0].pose(&s2.doc, 0.4);
    assert_eq!(posed.node("a").unwrap().y, 60.0);
    // A colour key at time 0 needs no extra base key.
    let out = set_key(src, &s, "Go", "a", Prop::Fill, 0.0, Value::Color(Color::WHITE)).unwrap();
    assert!(out.contains("  a.fill  0: #ffffff\n}"), "{out}");
    // Empty block, closing brace on the anim line's own block.
    let src = "rect a\nanim Go 1s {\n}\n";
    let out = set_key(src, &scene(src), "Go", "a", Prop::X, 0.0, Value::Num(5.0)).unwrap();
    assert_eq!(out, "rect a\nanim Go 1s {\n  a.x  0: 5\n}\n");
    assert!(set_key(src, &scene(src), "Nope", "a", Prop::X, 0.0, Value::Num(5.0)).is_none());
}

#[test]
fn delete_key_keeps_the_line_tidy() {
    let src = "rect a\nanim Go 1s {\n  a.x  0: 10 linear | 0.5: 3 | 1: 50\n  a.y  0: 1\n}\n";
    let s = scene(src);
    let first = delete_key(src, &s, "Go", "a", Prop::X, 0.0).unwrap();
    assert!(first.contains("  a.x  0.5: 3 | 1: 50\n"), "{first}");
    let mid = delete_key(src, &s, "Go", "a", Prop::X, 0.5).unwrap();
    assert!(mid.contains("  a.x  0: 10 linear | 1: 50\n"), "{mid}");
    let last = delete_key(src, &s, "Go", "a", Prop::X, 1.0).unwrap();
    assert!(last.contains("  a.x  0: 10 linear | 0.5: 3\n"), "{last}");
    let whole = delete_key(src, &s, "Go", "a", Prop::Y, 0.0).unwrap();
    assert_eq!(whole, "rect a\nanim Go 1s {\n  a.x  0: 10 linear | 0.5: 3 | 1: 50\n}\n");
    assert!(delete_key(src, &s, "Go", "a", Prop::Y, 0.3).is_none());
    for out in [first, mid, last, whole] {
        scene(&out);
    }
}

#[test]
fn add_node_goes_before_blocks_with_a_fresh_id() {
    let src = "rect rect1 at 0 0\n\nanim Go 1s {\n  rect1.x  0: 0\n}\n";
    let (out, id) = add_node(src, &scene(src), ShapeKind::Rect, (40.0, 30.0), (20.0, 10.0));
    assert_eq!(id, "rect2");
    assert_eq!(out, "rect rect1 at 0 0\n\nrect rect2 at 40 30 size 20 10 fill #3b5bff\n\nanim Go 1s {\n  rect1.x  0: 0\n}\n");
    let (out, id) = add_node("artboard 10x10", &scene("artboard 10x10"), ShapeKind::Star, (5.0, 5.0), (8.0, 8.0));
    assert_eq!((out.as_str(), id.as_str()), ("artboard 10x10\nstar star1 at 5 5 size 8 fill #ffc531\n", "star1"));
}

#[test]
fn delete_node_takes_children_keys_and_overrides() {
    let src = "rect keep\ngroup g at 1 1 {\n  rect a\n  rect b\n}\nanim Go 1s {\n  keep.x  0: 0\n  a.y  0: 1 | 1: 2\n  g.rot  0: 0\n}\noverride {\n  b.x = 3\n  keep.y = 4\n}\n";
    let out = delete_node(src, &scene(src), "g").unwrap();
    assert_eq!(out, "rect keep\nanim Go 1s {\n  keep.x  0: 0\n}\noverride {\n  keep.y = 4\n}\n");
    scene(&out);
    let out = delete_node(src, &scene(src), "a").unwrap();
    assert!(!out.contains("rect a") && !out.contains("a.y"), "{out}");
    scene(&out);
    // Nodes made by a loop share their line with siblings.
    assert!(delete_node(EXAMPLE, &scene(EXAMPLE), "coin2").is_none());
}

#[test]
fn layers_rename_lock_and_reorder() {
    let src = "rect a \"Фон\" at 10 10\ngroup g at 100 50 {\n  rect b at 5 5 // кнопка\n}\nrect c\nanim Go 1s {\n  a.x  0: 0\n}\n";
    let s = scene(src);
    // Names: replaced, added, removed.
    let out = rename_node(src, &s, "a", "Небо \"синее\"").unwrap();
    assert!(out.starts_with("rect a \"Небо синее\" at 10 10\n"), "{out}");
    assert!(rename_node(src, &s, "c", "Пол").unwrap().contains("rect c \"Пол\"\n"));
    assert!(rename_node(src, &s, "a", " ").unwrap().starts_with("rect a at 10 10\n"));
    // Locked shapes are drawn but not picked on the canvas.
    let out = set_style(src, &s, "c", "locked", Some("locked")).unwrap();
    assert!(out.contains("rect c locked\n"), "{out}");
    let locked = scene(&out);
    assert!(locked.doc.node("c").unwrap().locked);
    let p = motif_core::kurbo::Point::new(10.0, 10.0);
    assert_eq!((s.doc.hit_test(p), locked.doc.hit_test(p)), (Some(3), Some(0)));
    // Order: over c, then under a.
    let out = move_node(src, &s, "a", "c", Place::After).unwrap();
    assert_eq!(scene(&out).doc.nodes.iter().map(|n| n.id.as_str()).collect::<Vec<_>>(), ["g", "b", "c", "a"]);
    assert!(out.contains("a.x  0: 0"));
    let out = move_node(src, &s, "c", "a", Place::Before).unwrap();
    assert!(out.starts_with("rect c\nrect a"), "{out}");
    // Into a group: indented, and still at the same place on the artboard.
    let out = move_node(src, &s, "a", "g", Place::Into).unwrap();
    assert!(out.contains("  rect b at 5 5 // кнопка\n  rect a \"Фон\" at -90 -40\n}"), "{out}");
    let moved = scene(&out);
    let i = moved.doc.index_of("a").unwrap();
    assert_eq!(moved.doc.nodes[i].parent, moved.doc.index_of("g"));
    assert_eq!(moved.doc.world_transform(i) * motif_core::kurbo::Point::ZERO, motif_core::kurbo::Point::new(10.0, 10.0));
    // Out of it, with its comment, and a missing `at` written.
    let out = move_node(src, &s, "b", "c", Place::After).unwrap();
    assert!(out.contains("group g at 100 50 {\n}\nrect c\nrect b at 105 55 // кнопка\n"), "{out}");
    let out = move_node(src, &s, "c", "g", Place::Into).unwrap();
    assert!(out.contains("  rect c at -100 -50\n}"), "{out}");
    // Comments above a declaration go along with it.
    let src2 = "// небо\nrect a\n// земля\n// и трава\nrect b\n";
    let out = move_node(src2, &scene(src2), "b", "a", Place::Before).unwrap();
    assert_eq!(out, "// земля\n// и трава\nrect b\n// небо\nrect a\n");
    // Not into itself, not into a shape.
    assert!(move_node(src, &s, "g", "b", Place::After).is_none());
    assert!(move_node(src, &s, "a", "c", Place::Into).is_none());
}

#[test]
fn anim_header_edits() {
    let src = "rect a\nanim Go 1.2s loop {\n  a.x  0: 0 | 1: 5 linear\n}\n";
    let s = scene(src);
    assert!(set_anim_duration(src, &s, "Go", 2.5).unwrap().contains("anim Go 2.5s loop {"));
    let once = set_anim_loop(src, &s, "Go", false).unwrap();
    assert!(once.contains("anim Go 1.2s {"), "{once}");
    assert!(!scene(&once).doc.animations[0].looping);
    assert!(set_anim_loop(&once, &scene(&once), "Go", true).unwrap().contains("anim Go 1.2s loop {"));
    let (out, name) = add_anim(src, &s);
    assert_eq!(name, "Anim1");
    assert!(scene(&out).doc.animation("Anim1").is_some());
    // Keys: retime and curve.
    let moved = move_key(src, &s, "Go", "a", Prop::X, 1.0, 0.75).unwrap();
    assert!(moved.contains("a.x  0: 0 | 0.75: 5 linear"), "{moved}");
    assert!(move_key(src, &s, "Go", "a", Prop::X, 1.0, 0.0).is_none());
    let eased = set_key_ease(src, &s, "Go", "a", Prop::X, 0.0, "hold").unwrap();
    assert!(eased.contains("a.x  0: 0 hold | 1: 5 linear"), "{eased}");
    let eased = set_key_ease(src, &s, "Go", "a", Prop::X, 1.0, "spring").unwrap();
    assert!(eased.contains("| 1: 5 spring"), "{eased}");
}

#[test]
fn paths_parse_edit_and_add() {
    use motif_core::kurbo::{BezPath, Point, Shape};
    let src = "path leaf \"Лист\" at 100 80 d \"M 0 -20 C 15 -20 20 0 0 20 L -10 0 Z\" fill #4fd38c\nanim Go 1s {\n}\n";
    let s = scene(src);
    let leaf = s.doc.node("leaf").unwrap();
    assert_eq!(leaf.kind, ShapeKind::Path);
    assert_eq!(leaf.geometry.elements().len(), 4);
    assert_eq!(leaf.height, 40.0);
    assert!(s.doc.hit_test(Point::new(100.0, 80.0)).is_some());

    let mut tri = BezPath::new();
    tri.move_to((0.0, 0.0));
    tri.line_to((10.5, 0.0));
    tri.line_to((5.0, 8.25));
    tri.close_path();
    let out = set_path(src, &s, "leaf", &tri).unwrap();
    assert!(out.contains("d \"M 0 0 L 10.5 0 L 5 8.25 Z\" fill"), "{out}");
    assert_eq!(scene(&out).doc.node("leaf").unwrap().geometry, tri);

    // New outlines are centred on their node and go before the blocks.
    let mut open = BezPath::new();
    open.move_to((10.0, 10.0));
    open.line_to((30.0, 50.0));
    let (out, id) = add_path(src, &s, &open);
    assert_eq!(id, "path1");
    assert!(out.contains("path path1 at 20 30 d \"M -10 -20 L 10 20\" fill none stroke #1b1d24 3\n\nanim Go"), "{out}");
    let p = scene(&out).doc.node("path1").unwrap().clone();
    assert_eq!(p.geometry.bounding_box().center(), Point::ORIGIN);

    // Mistakes are reported, not ignored.
    assert!(parse("rect a d \"M 0 0\"").is_err());
    assert!(parse("path a d \"M 0 0 Q\"").is_err());
    assert!(parse("path a d 5").is_err());
}

#[test]
fn copy_and_paste_nodes() {
    let s = scene(EXAMPLE);
    // The ball alone: new id, moved by the offset, inserted before the blocks.
    let snips = copy_nodes(&s, EXAMPLE, &["ball".to_string()]);
    assert_eq!(snips.len(), 1);
    let (out, ids) = paste_nodes(EXAMPLE, &s, &snips, (10.0, 10.0));
    assert_eq!(ids, vec!["ball1".to_string()]);
    let pasted = scene(&out);
    let copy = pasted.doc.node("ball1").unwrap();
    assert_eq!((copy.x, copy.y), (160.0, 160.0));
    assert_eq!(copy.fill, s.doc.node("ball").unwrap().fill);
    // A group with a loop inside: children get a suffix so the copy parses.
    let snips = copy_nodes(&s, EXAMPLE, &["coins".to_string(), "coin2".to_string()]);
    assert_eq!(snips.len(), 1, "a child of a copied group is not copied twice");
    let (out, ids) = paste_nodes(EXAMPLE, &s, &snips, (0.0, 0.0));
    let pasted = scene(&out);
    assert_eq!(ids, vec!["coins1".to_string()]);
    assert!(pasted.doc.node("coin3_2").is_some());
    assert_eq!(pasted.doc.nodes.len(), s.doc.nodes.len() + 6);
    // Pasting twice still gives fresh ids.
    let again = paste_nodes(&out, &pasted, &snips, (0.0, 0.0)).0;
    scene(&again);
    // Nodes made by a loop are not copied on their own.
    assert!(copy_nodes(&s, EXAMPLE, &["coin1".to_string()]).is_empty());
}

#[test]
fn gradients_and_masks() {
    let src = "artboard \"M\" 200x200\nrect a at 50 50 size 40 gradient linear 90 #ff0000 #0000ff\ngroup g clip {\n  ellipse m at 0 0 size 40\n  rect b at 0 0 size 60\n}\n";
    let s = scene(src);
    let g = s.doc.node("a").unwrap().gradient.clone().unwrap();
    assert!(!g.radial);
    assert_eq!(g.angle, 90.0);
    assert_eq!(g.colors.len(), 2);
    assert!(s.doc.node("g").unwrap().clip);
    // Replace with radial, then remove; the line stays tidy.
    let radial = motif_core::Gradient { radial: true, angle: 0.0, colors: g.colors.clone() };
    let out = set_gradient(src, &s, "a", Some(&radial)).unwrap();
    assert!(out.contains("rect a at 50 50 size 40 gradient radial #ff0000 #0000ff\n"), "{out}");
    let out = set_gradient(&out, &scene(&out), "a", None).unwrap();
    assert!(out.contains("rect a at 50 50 size 40\n"), "{out}");
    let out = set_gradient(&out, &scene(&out), "a", Some(&g)).unwrap();
    assert!(out.contains("size 40 gradient linear 90 #ff0000 #0000ff\n"), "{out}");
    // Clip on and off.
    let out = set_clip(src, &s, "g", false).unwrap();
    assert!(out.contains("group g {"), "{out}");
    let out = set_clip(&out, &scene(&out), "g", true).unwrap();
    assert!(out.contains("group g clip {"), "{out}");
    assert!(set_clip(src, &s, "a", true).is_none());
    // Errors.
    assert!(parse("rect a gradient linear #ff0000").is_err());
    assert!(parse("rect a gradient spiral #ff0000 #00ff00").is_err());
    assert!(parse("rect a clip").is_err());
}

#[test]
fn cubic_ease() {
    let src = "artboard \"M\" 100x100\nrect a at 0 0 size 10\nanim A 1s {\n  a.x 0: 0 cubic 0.2 0 0.2 1.4 | 1: 100\n}\n";
    let s = scene(src);
    let k = &s.doc.animations[0].tracks[0].keys[0];
    assert_eq!(k.ease, motif_core::Ease::Cubic([0.2, 0.0, 0.2, 1.4]));
    // Overshoots past the end value before settling.
    assert!(k.ease.apply(0.8) > 1.0);
    assert!((k.ease.apply(1.0) - 1.0).abs() < 1e-9);
    // Writing a curve back keeps the script parseable.
    let e = motif_core::Ease::Cubic([0.25, 0.1, 0.25, 1.0]);
    let out = set_key_ease(src, &s, "A", "a", Prop::X, 0.0, &e.script()).unwrap();
    assert!(out.contains("a.x 0: 0 cubic 0.25 0.1 0.25 1 | 1: 100"), "{out}");
    assert_eq!(scene(&out).doc.animations[0].tracks[0].keys[0].ease, e);
    let out = set_key_ease(&out, &scene(&out), "A", "a", Prop::X, 0.0, "linear").unwrap();
    assert!(out.contains("a.x 0: 0 linear | 1: 100"), "{out}");
    assert!(parse("artboard \"M\" 9x9\nrect a\nanim A 1s {\n  a.x 0: 0 cubic 0.2 0 | 1: 5\n}").is_err());
    assert!(parse("artboard \"M\" 9x9\nrect a\nanim A 1s {\n  a.x 0: 0 cubic 2 0 0.2 1 | 1: 5\n}").is_err());
}

#[test]
fn named_and_stepped_eases() {
    let src = "artboard \"M\" 100x100\nrect a at 0 0 size 10\nanim A 1s {\n  a.x 0: 0 ease-out-bounce | 0.5: 50 steps 4 | 0.8: 80 ease | 1: 100\n}\n";
    let s = scene(src);
    let keys = &s.doc.animations[0].tracks[0].keys;
    assert_eq!(keys[0].ease, motif_core::Ease::Named(motif_core::Family::Bounce, motif_core::Dir::Out));
    assert_eq!(keys[1].ease, motif_core::Ease::Steps(4));
    assert_eq!(keys[2].ease, motif_core::Ease::Css);
    let out = set_key_ease(src, &s, "A", "a", Prop::X, 0.5, &motif_core::Ease::Steps(6).script()).unwrap();
    assert!(out.contains("0.5: 50 steps 6 |"), "{out}");
    assert!(parse("artboard \"M\" 9x9\nrect a\nanim A 1s {\n  a.x 0: 0 steps 0 | 1: 5\n}").is_err());
    assert!(parse("artboard \"M\" 9x9\nrect a\nanim A 1s {\n  a.x 0: 0 ease-sideways | 1: 5\n}").is_err());
}

#[test]
fn css_like_properties() {
    let src = "artboard \"M\" 200x200\nrect a at 50 50 size 40 r 8 4 skew 10 pivot 0 20 stroke #000 2 dash 6 4 cap butt join bevel shadow 0 4 8 #00000055 blend multiply\nrect b at 0 0 size 10 shadow 2 2 #000000\n";
    let s = scene(src);
    let a = s.doc.node("a").unwrap();
    assert_eq!(a.radii, Some([8.0, 4.0, 8.0, 4.0]));
    assert_eq!((a.skew_x, a.skew_y), (10.0, 0.0));
    assert_eq!((a.pivot.x, a.pivot.y), (0.0, 20.0));
    assert_eq!(a.dash, vec![6.0, 4.0]);
    assert_eq!((a.cap, a.join, a.blend), (motif_core::Cap::Butt, motif_core::Join::Bevel, motif_core::Blend::Multiply));
    let sh = a.shadow.unwrap();
    assert_eq!((sh.dx, sh.dy, sh.blur, sh.color.a), (0.0, 4.0, 8.0, 0x55));
    assert_eq!(s.doc.node("b").unwrap().shadow.unwrap().blur, 0.0);
    // Whole properties are replaced, added and removed in place.
    let out = set_style(src, &s, "a", "dash", Some("dash 2")).unwrap();
    assert!(out.contains(" dash 2 cap butt"), "{out}");
    let out = set_style(&out, &scene(&out), "a", "blend", None).unwrap();
    assert!(out.contains("#00000055\n"), "{out}");
    let out = set_style(&out, &scene(&out), "b", "blend", Some("blend screen")).unwrap();
    assert!(out.contains("shadow 2 2 #000000 blend screen\n"), "{out}");
    // A single skew value is skewX; editing skewY writes both.
    let out = set_prop(src, &s, "a", Prop::SkewY, Value::Num(5.0)).unwrap();
    assert!(out.contains("skew 10 5"), "{out}");
    // Skew animates by its path names.
    let anim = format!("{src}anim A 1s {{\n  a.kx 0: 0 | 1: 20\n}}\n");
    assert_eq!(scene(&anim).doc.animations[0].tracks[0].prop, Prop::SkewX);
    for bad in ["rect a cap wavy", "rect a blend glow", "rect a shadow 1 2", "rect a dash -1", "rect a pivot 3"] {
        assert!(parse(&format!("artboard \"M\" 9x9\n{bad}")).is_err(), "{bad}");
    }
}

#[test]
fn backdrop_line() {
    let src = "artboard \"M\" 200x200\nrect a at 50 50 size 40\n";
    let s = scene(src);
    assert!(s.backdrop.is_none());
    let mut b = crate::Backdrop {
        file: "ref.png".into(),
        x: 0.0,
        y: 0.0,
        scale: 1.0,
        opacity: 0.4,
        hidden: false,
        sheet: None,
        hide: Vec::new(),
        show: Vec::new(),
        span: Span::new(0, 0),
    };
    let out = set_backdrop(src, &s, Some(&b));
    assert_eq!(out, "artboard \"M\" 200x200\nbackdrop \"ref.png\" opacity 0.4\nrect a at 50 50 size 40\n");
    let s2 = scene(&out);
    let got = s2.backdrop.clone().unwrap();
    assert_eq!((got.file.as_str(), got.opacity, got.scale, got.hidden), ("ref.png", 0.4, 1.0, false));
    (b.x, b.y, b.scale, b.hidden) = (10.0, -5.0, 0.5, true);
    let out = set_backdrop(&out, &s2, Some(&b));
    assert!(out.contains("backdrop \"ref.png\" at 10 -5 scale 0.5 opacity 0.4 hidden\nrect"), "{out}");
    let s3 = scene(&out);
    assert!(s3.backdrop.as_ref().unwrap().hidden);
    assert_eq!(set_backdrop(&out, &s3, None), src);
    // A sprite sheet: 4 × 2 with 7 frames at 8 fps, still on frame 2.
    let mut sheet = crate::Sheet::new(4, 2);
    (sheet.frames, sheet.fps, sheet.frame) = (7, 8.0, 2);
    b.sheet = Some(sheet);
    let out = set_backdrop(&out, &s3, Some(&b));
    assert!(out.contains("backdrop \"ref.png\" at 10 -5 scale 0.5 opacity 0.4 sheet 4 2 frames 7 fps 8 frame 2 hidden"), "{out}");
    let got = scene(&out).backdrop.unwrap().sheet.unwrap();
    assert_eq!(got, sheet);
    assert_eq!((got.frame_at(None), got.frame_at(Some(0.0)), got.frame_at(Some(0.26)), got.frame_at(Some(1.0))), (2, 0, 2, 1));
    let plain = crate::Sheet::new(8, 1);
    let line = backdrop_text(&crate::Backdrop { sheet: Some(plain), ..b.clone() });
    assert!(line.contains("sheet 8 1 hidden"), "{line}");
    // Layers of a PSD turned off and on by name.
    let psd = crate::Backdrop {
        file: "hero.psd".into(), sheet: None, hide: vec!["Фон".into(), "Тень 2".into()], show: vec!["Эскиз".into()], ..b.clone()
    };
    let line = backdrop_text(&psd);
    assert!(line.ends_with("hide \"Фон\" \"Тень 2\" show \"Эскиз\" hidden"), "{line}");
    let got = scene(&format!("artboard \"M\" 9x9\n{line}\n")).backdrop.unwrap();
    assert_eq!((got.hide, got.show), (psd.hide.clone(), psd.show.clone()));
    // Layer names keep their quotes and backslashes, so hiding finds the very layer.
    let odd = crate::Backdrop { file: "C:\\art\\hero.psd".into(), hide: vec!["Red \"front\"".into(), "a\\b".into()], show: vec!["\"".into()], ..psd.clone() };
    let line = backdrop_text(&odd);
    assert!(line.starts_with("backdrop \"C:\\art\\hero.psd\"") && line.contains("hide \"Red \\\"front\\\"\" \"a\\b\" show \"\\\"\""), "{line}");
    let got = scene(&format!("artboard \"M\" 9x9\n{line}\n")).backdrop.unwrap();
    assert_eq!((got.file, got.hide, got.show), (odd.file.clone(), odd.hide.clone(), odd.show.clone()));
    for bad in [
        "backdrop \"a.png\" sheet 0 1",
        "backdrop \"a.png\" sheet 2.5 1",
        "backdrop \"a.png\" sheet 4 1 frames 5",
        "backdrop \"a.png\" sheet 4 1 frame 4",
        "backdrop \"a.png\" sheet 4 1 fps 0",
        "backdrop \"a.png\" frames 3",
        "backdrop \"a.psd\" hide",
        "backdrop \"a.psd\" show 3",
    ] {
        assert!(parse(&format!("artboard \"M\" 9x9\n{bad}")).is_err(), "{bad}");
    }
    for bad in ["backdrop ref.png", "backdrop \"a.png\" glow 1", "backdrop \"a.png\"\nbackdrop \"b.png\"", "group g {\n  backdrop \"a.png\"\n}"] {
        assert!(parse(&format!("artboard \"M\" 9x9\n{bad}")).is_err(), "{bad}");
    }
}

#[test]
fn svg_import() {
    let src = "artboard \"M\" 200x100\n";
    let svg = br##"<svg xmlns="http://www.w3.org/2000/svg" width="400" height="100" viewBox="0 0 400 100">
      <defs><linearGradient id="g" x1="0" y1="0" x2="1" y2="0"><stop offset="0" stop-color="#ff0000"/><stop offset="1" stop-color="#0000ff"/></linearGradient></defs>
      <rect id="bar" x="0" y="0" width="200" height="100" fill="url(#g)"/>
      <g opacity="0.5" transform="translate(200 0)">
        <path id="rect" d="M0 0 L200 0 L200 100 Z M50 20 L150 20 L150 80 Z" fill="#00ff00" fill-rule="evenodd" stroke="#000" stroke-width="4" stroke-linecap="butt"/>
      </g>
    </svg>"##;
    let (out, id) = crate::import_svg(src, &scene(src), svg, "logo").unwrap();
    assert_eq!(id, "logo");
    let s = scene(&out);
    let g = s.doc.node("logo").unwrap();
    assert_eq!((g.x, g.y), (100.0, 50.0));
    // Scaled to fit 90% of the artboard width: 400 -> 180.
    let bar = s.doc.node("bar").unwrap();
    assert!((bar.x + 45.0).abs() < 0.2 && bar.gradient.is_some(), "{out}");
    // `rect` is a command, so the second shape gets a fresh id.
    let shape = s.doc.node("shape1").unwrap();
    assert!(shape.even_odd && (shape.opacity - 0.5).abs() < 1e-9 && shape.cap == motif_core::Cap::Butt, "{out}");
    assert!((shape.stroke_width - 1.8).abs() < 1e-9, "{out}");
    assert!(crate::import_svg(src, &s, b"<svg xmlns=\"http://www.w3.org/2000/svg\"/>", "x").is_err());
    assert!(crate::import_svg(src, &s, b"not svg", "x").is_err());
}

#[test]
fn boolean_ops() {
    use motif_core::kurbo::Shape;
    let src = "artboard \"M\" 200x200\nrect a \"Низ\" at 50 50 size 40 fill #ff0000 stroke #000 2\ngroup g at 10 0 {\n  ellipse b at 60 50 size 40 fill #00ff00\n}\nanim A 1s {\n  b.x 0: 60 | 1: 80\n}\n";
    let s = scene(src);
    let ids = vec!["a".to_string(), "b".to_string()];
    // Union: one path in place of the bottom rect, with its name and style; b and its keys are gone.
    let (out, id) = crate::boolean(src, &s, &ids, crate::BoolOp::Union).unwrap();
    let u = scene(&out);
    assert!(u.doc.node("a").is_none() && u.doc.node("b").is_none() && !out.contains("b.x"), "{out}");
    let p = u.doc.node(&id).unwrap();
    assert!(out.contains(&format!("\npath {id} \"Низ\" at ")) && p.even_odd && p.stroke_width == 2.0, "{out}");
    let bb = p.path().unwrap().bounding_box();
    assert!((bb.width() - 60.0).abs() < 0.5 && (bb.height() - 40.0).abs() < 0.5, "{bb:?}");
    // Subtract: the rect minus the circle reaching into it from the right (circle centre x = 70, rect 30..70).
    let (out, id) = crate::boolean(src, &s, &ids, crate::BoolOp::Subtract).unwrap();
    let d = scene(&out).doc.node(&id).unwrap().path().unwrap().bounding_box();
    assert!((d.width() - 40.0).abs() < 0.5, "{d:?}");
    // Intersect: only the overlap, x from 50 to 70 (the circle spans 50..90).
    let (out, id) = crate::boolean(src, &s, &ids, crate::BoolOp::Intersect).unwrap();
    let i = scene(&out).doc.node(&id).unwrap().path().unwrap().bounding_box();
    assert!((i.width() - 20.0).abs() < 0.5, "{i:?}");
    // Exclude keeps the outer extent.
    let (out, id) = crate::boolean(src, &s, &ids, crate::BoolOp::Exclude).unwrap();
    assert!((scene(&out).doc.node(&id).unwrap().path().unwrap().bounding_box().width() - 60.0).abs() < 0.5);
    assert!(crate::boolean(src, &s, &["a".into()], crate::BoolOp::Union).is_err());
    assert!(crate::boolean(src, &s, &["a".into(), "g".into()], crate::BoolOp::Union).is_err());
}

#[test]
fn text_and_outlines() {
    use motif_core::kurbo::Shape;
    let src = "artboard \"M\" 400x200\ntext t at 200 100 content \"Hi\\nyou\" font bold font-size 40 letter-spacing 4 line-height 1.5 align left outline #000 10 #fff 16 fill #f00\n";
    let s = scene(src);
    let t = s.doc.node("t").unwrap();
    let shape = t.text.as_ref().unwrap();
    assert_eq!(shape.lines.len(), 2);
    assert_eq!((t.text_style.size, t.text_style.spacing, t.text_style.line_height, t.text_style.align), (40.0, 4.0, 1.5, motif_core::TextAlign::Left));
    // Widest outline first.
    assert_eq!(t.outlines.iter().map(|o| o.1).collect::<Vec<_>>(), vec![16.0, 10.0]);
    // Left aligned: letters start at the origin; two lines of 60 units.
    let bb = t.path().unwrap().bounding_box();
    assert!(bb.x0 >= 0.0 && bb.x0 < 6.0 && bb.height() > 60.0 && bb.height() < 120.0, "{bb:?}");
    // Font size animates; reveal 0 hides every letter.
    let anim = format!("{src}anim A 1s {{\n  t.font-size 0: 40 | 1: 80\n  t.reveal 0: 0 | 1: 1\n}}\n");
    let s2 = scene(&anim);
    let a = &s2.doc.animations[0];
    let start = a.pose(&s2.doc, 0.0);
    assert!(start.node("t").unwrap().path().unwrap().elements().is_empty());
    let end = a.pose(&s2.doc, 1.0);
    assert!(end.node("t").unwrap().path().unwrap().bounding_box().width() > bb.width() * 1.8);
    // Editing writes the CSS-like names.
    let out = set_prop(src, &s, "t", Prop::FontSize, Value::Num(50.0)).unwrap();
    assert!(out.contains("font-size 50"), "{out}");
    let out = set_style(src, &s, "t", "content", Some("content \"Yo\"")).unwrap();
    assert_eq!(scene(&out).doc.node("t").unwrap().text.as_ref().unwrap().content, "Yo");
    for bad in ["rect r content \"x\"", "text t font \"nope.ttf\"", "text t align middle", "rect r outline #000"] {
        assert!(parse(&format!("artboard \"M\" 9x9\n{bad}")).is_err(), "{bad}");
    }
    // A text alone turns into outlines.
    let (out, id) = crate::boolean(src, &s, &["t".into()], crate::BoolOp::Union).unwrap();
    let p = scene(&out).doc.node(&id).unwrap().path().unwrap();
    assert!(!out.contains("content") && (p.bounding_box().width() - bb.width()).abs() < 1.0, "{out}");
    // The title example parses.
    let title = include_str!("../../../examples/title.msc");
    assert!(scene(title).doc.node("title").unwrap().text.is_some());
}

#[test]
fn blur_elevation_and_light() {
    let src = "artboard \"M\" 200x200 bg #fff light 120 // свет\nrect a at 50 50 blur 4 elevation 10\nrect b at 0 0 shadow 2 3 #000000\nrect c at 9 9\n";
    let s = scene(src);
    assert_eq!(s.doc.light, 120.0);
    let a = s.doc.node("a").unwrap();
    assert_eq!((a.blur, a.elevation), (4.0, 10.0));
    // Shadow parts edit their own numbers; a missing blur goes after the offset.
    let out = set_prop(src, &s, "b", Prop::ShadowY, Value::Num(7.0)).unwrap();
    assert!(out.contains("shadow 2 7 #000000"), "{out}");
    let out = set_prop(src, &s, "b", Prop::ShadowBlur, Value::Num(5.0)).unwrap();
    assert!(out.contains("shadow 2 3 5 #000000"), "{out}");
    let out = set_prop(src, &s, "c", Prop::ShadowX, Value::Num(4.0)).unwrap();
    assert!(out.contains("rect c at 9 9 shadow 4 0 0 #00000055"), "{out}");
    let out = set_prop(src, &s, "a", Prop::Blur, Value::Num(2.5)).unwrap();
    assert!(out.contains("blur 2.5 elevation 10"), "{out}");
    // Effects animate by their path names.
    let anim = format!("{src}anim A 1s {{\n  b.shadow-blur 0: 0 | 1: 20\n  a.elevation 0: 0 | 1: 30\n}}\n");
    let s2 = scene(&anim);
    let posed = s2.doc.animations[0].pose(&s2.doc, 0.5);
    assert_eq!(posed.node("b").unwrap().shadow.unwrap().blur, 10.0);
    assert_eq!(posed.node("a").unwrap().elevation, 15.0);
    // The light is written on the artboard line, before its comment; 90 removes it.
    let out = set_light(src, 45.0).unwrap();
    assert!(out.starts_with("artboard \"M\" 200x200 bg #fff light 45 // свет\n"), "{out}");
    assert_eq!(scene(&out).doc.light, 45.0);
    let out = set_light(&out, 90.0).unwrap();
    assert!(out.starts_with("artboard \"M\" 200x200 bg #fff // свет\n"), "{out}");
    assert!(parse("artboard \"M\" 9x9 light #fff").is_err());
}

#[test]
fn artboard_purpose() {
    use motif_core::Purpose;
    let src = "artboard \"Меню игры\" 200x100 bg #fff light 45 // экран\nrect a\n";
    assert_eq!(scene(src).doc.purpose, Purpose::Any);
    let out = set_purpose(src, Purpose::Ui).unwrap();
    assert!(out.starts_with("artboard \"Меню игры\" 200x100 bg #fff light 45 ui // экран\n"), "{out}");
    assert_eq!(scene(&out).doc.purpose, Purpose::Ui);
    let out = set_purpose(&out, Purpose::Game).unwrap();
    assert!(out.starts_with("artboard \"Меню игры\" 200x100 bg #fff light 45 game // экран\n"), "{out}");
    assert_eq!(scene(&out).doc.purpose, Purpose::Game);
    assert_eq!(set_purpose(&out, Purpose::Any).unwrap(), src);
    // The light still finds its place next to the flag.
    assert!(set_light(&out, 10.0).unwrap().starts_with("artboard \"Меню игры\" 200x100 bg #fff game light 10 // экран\n"));
    assert!(set_purpose("artboard 9x9\n", Purpose::Ui).unwrap().starts_with("artboard 9x9 ui\n"));
    assert!(parse("artboard 9x9 ui 3").is_err());
    // States for a kit: only the missing ones.
    let src = "artboard 9x9 ui\nrect b ui\nanim Hover 0.2s {\n}\n";
    let (out, added) = add_states(src, &scene(src), &["Hover", "Press"], 0.25);
    assert_eq!(added, ["Press"]);
    assert!(out.ends_with("anim Hover 0.2s {\n}\n\nanim Press 0.25s {\n}\n"), "{out}");
    assert!(!scene(&out).doc.animation("Press").unwrap().looping);
}

const SYMBOLS: &str = "artboard 320x160
let ink = #1b1d24
symbol Coin size=24 color=#ffd23f {
  ellipse body size {size} fill color stroke ink 2
  ellipse shine at {-size * 0.2} 0 size 4 fill #fff
}
use Coin c1 at 60 80
use Coin c2 \"Большая\" at 140 80 size=48 color=#ff5a36
group row at 200 80 {
  for i in 0..2 {
    use Coin r{i} at {i * 36} 0 size=20
  }
}
anim Spin 1s loop {
  c1-body.sx 0: 1 | 1: 0.5
}
";

#[test]
fn symbols_make_copies_with_parameters() {
    let s = scene(SYMBOLS);
    let doc = &s.doc;
    let c1 = doc.node("c1").unwrap();
    assert_eq!(c1.kind, ShapeKind::Group);
    let body = doc.node("c1-body").unwrap();
    assert_eq!(doc.nodes[body.parent.unwrap()].id, "c1");
    assert_eq!((body.width, body.fill), (24.0, Color::from_hex("#ffd23f")));
    let big = doc.node("c2-body").unwrap();
    assert_eq!((big.width, big.fill), (48.0, Color::from_hex("#ff5a36")));
    assert!((doc.node("c2-shine").unwrap().x + 9.6).abs() < 1e-9);
    assert_eq!(doc.node("r1-body").unwrap().width, 20.0);
    assert_eq!(doc.nodes[doc.node("r1").unwrap().parent.unwrap()].id, "row");
    assert_eq!(doc.animation("Spin").unwrap().tracks[0].node, "c1-body");
    // The objects of a copy are not written on their own line: edits go to `override`.
    assert!(!s.sources[&("c1-body".to_string(), Prop::Width)].editable);
    assert!(s.sources[&("c1".to_string(), Prop::X)].editable);
    assert_eq!(s.symbols[0].name, "Coin");
    let inst = &s.instances["c2"];
    assert_eq!(inst.params[0].value, Value::Num(48.0));
    assert!(inst.params[0].editable);
    assert!(s.instances["c1"].params[0].span.is_none());
}

#[test]
fn symbol_errors() {
    let errs = parse("use Gem g at 0 0\nsymbol A x=1 {\n  rect a size {x}\n}\nuse A a1 y=2\nsymbol B {\n  use B b\n}\nuse B b1\n").unwrap_err();
    let text = errs.iter().map(|e| e.to_string()).collect::<Vec<_>>().join("\n");
    assert!(text.contains("нет символа «Gem»"), "{text}");
    assert!(text.contains("нет параметра «y»"), "{text}");
    assert!(text.contains("сам в себя"), "{text}");
}

#[test]
fn edits_symbol_parameters() {
    let s = scene(SYMBOLS);
    let out = set_param(SYMBOLS, &s, "c2", "size", Value::Num(30.0)).unwrap();
    assert!(out.contains("use Coin c2 \"Большая\" at 140 80 size=30 color=#ff5a36"), "{out}");
    let out = set_param(SYMBOLS, &s, "c1", "color", Value::Color(Color::from_hex("#3b5bff").unwrap())).unwrap();
    assert!(out.contains("use Coin c1 at 60 80 color=#3b5bff\n"), "{out}");
    assert_eq!(scene(&out).doc.node("c1-body").unwrap().fill, Color::from_hex("#3b5bff"));
    // A copy made by a loop shares its line with the others.
    assert!(set_param(SYMBOLS, &s, "r0", "size", Value::Num(5.0)).is_none());
    let (out, id) = add_use(SYMBOLS, &s, "Coin", (10.0, 20.0));
    assert_eq!(id, "coin1");
    assert!(scene(&out).doc.node("coin1-body").is_some());
}

#[test]
fn turns_a_group_into_a_symbol() {
    let src = "artboard 200x200\ngroup ball \"Мяч\" at 100 100 clip {\n  ellipse shape size 72\n  ellipse body size 72 fill #ff5a36\n}\nanim Hit 1s {\n  body.sx 0: 1 | 1: 2\n  ball.y 0: 100 | 1: 50\n}\n";
    let s = scene(src);
    assert_eq!(free_symbol_name(&s, "ball"), "Ball");
    let out = make_symbol(src, &s, "ball", "Ball").unwrap();
    assert_eq!(
        out,
        "artboard 200x200\nsymbol Ball {\n  ellipse shape size 72\n  ellipse body size 72 fill #ff5a36\n}\n\nuse Ball ball \"Мяч\" at 100 100 clip\nanim Hit 1s {\n  ball-body.sx 0: 1 | 1: 2\n  ball.y 0: 100 | 1: 50\n}\n"
    );
    let after = scene(&out);
    assert_eq!(after.doc.node("ball-body").unwrap().fill, Color::from_hex("#ff5a36"));
    assert!(after.doc.node("ball").unwrap().clip);
    // The same pictures before and after.
    let a = motif_core::Document::clone(&s.doc);
    assert_eq!(a.nodes.len(), after.doc.nodes.len());
    assert!(make_symbol(&out, &after, "ball", "Ball2").is_none());
}

#[test]
fn symbol_clip_and_example() {
    let s = scene("symbol Ball clip size=10 {\n  ellipse mask size {size}\n  rect band size 20 4\n}\nuse Ball b at 0 0\n");
    assert!(s.doc.node("b").unwrap().clip);
    let ex = scene(include_str!("../../../examples/symbols.msc"));
    assert_eq!(ex.symbols.len(), 3);
    assert!(ex.doc.node("coin5-face").is_some());
}

const MACHINE: &str = "rect b at 0 0 size 10
anim Idle 1s loop {
  b.x 0: 0 | 1: 0
}
anim Run 0.5s loop {
  b.x 0: 100 | 0.5: 100
}
machine Player {
  input speed 0
  input hover false
  input jump trigger
  state Idle at 40 60
  state Move anim Run
  Idle->Run when speed > 0.5 and not hover blend 0.2s
  Move -> Idle when speed <= 0.5
  any -> Idle when jump at end
}
";

#[test]
fn machines_parse() {
    use motif_core::{CondOp, InputKind};
    let src = MACHINE.replace("Idle->Run", "Idle->Move");
    let s = scene(&src);
    let m = s.doc.machine("Player").unwrap();
    assert_eq!(m.inputs.iter().map(|i| i.kind).collect::<Vec<_>>(), [InputKind::Number, InputKind::Bool, InputKind::Trigger]);
    assert_eq!(m.states[1].anim, "Run");
    assert_eq!(m.states[0].pos, Some((40.0, 60.0)));
    let t = &m.transitions[0];
    assert_eq!((t.from, t.to, t.blend, t.conds.len(), t.conds[1].op), (Some(0), 1, 0.2, 2, CondOp::Not));
    assert!(m.transitions[2].from.is_none() && m.transitions[2].at_end);
    assert_eq!(fmt_transition(m, t), "Idle -> Move when speed > 0.5 and not hover blend 0.2s");
    assert_eq!(s.machines["Player"].transitions.len(), 3);
}

#[test]
fn machine_errors() {
    let msg = |src: &str| parse(src).unwrap_err().iter().map(|e| e.message.clone()).collect::<Vec<_>>().join(" | ");
    assert!(msg(MACHINE).contains("нет состояния «Run»"), "{}", msg(MACHINE));
    let bad = MACHINE.replace("state Move anim Run", "state Move").replace("Idle->Run", "Idle->Move");
    assert!(msg(&bad).contains("нет анимации «Move»"));
    let bad = MACHINE.replace("Idle->Run when speed > 0.5", "Idle->Move when speed");
    assert!(msg(&bad).contains("число: сравните"));
    let bad = MACHINE.replace("Idle->Run when speed > 0.5", "Idle->Move when fly > 0.5");
    assert!(msg(&bad).contains("нет входа «fly»"));
    let bad = MACHINE.replace("Idle->Run", "Idle->Move").replace("input jump trigger", "input jump maybe");
    assert!(msg(&bad).contains("trigger"));
}

#[test]
fn edits_machines() {
    use motif_core::{Cond, CondOp, Input, InputKind, State, Transition};
    let src = MACHINE.replace("Idle->Run", "Idle->Move");
    let s = scene(&src);
    // Moving a state rewrites only its line; the other lines keep their spelling.
    let out = edit_machine(&src, &s, "Player", |m| m.states[1].pos = Some((220.0, 60.0))).unwrap();
    assert!(out.contains("  state Move anim Run at 220 60\n") && out.contains("Idle->Move when"));
    // Renaming an input rewrites the conditions that read it.
    let out = edit_machine(&src, &s, "Player", |m| {
        m.inputs[0].name = "vel".into();
        for t in &mut m.transitions {
            for c in &mut t.conds {
                if c.input == "speed" {
                    c.input = "vel".into();
                }
            }
        }
    })
    .unwrap();
    assert!(out.contains("input vel 0") && out.contains("Idle -> Move when vel > 0.5") && out.contains("Move -> Idle when vel <= 0.5"));
    scene(&out);
    // New lines go before the closing brace.
    let out = edit_machine(&src, &s, "Player", |m| {
        m.inputs.push(Input { name: "dead".into(), kind: InputKind::Bool, default: 0.0 });
        m.states.push(State { name: "Rest".into(), anim: "Idle".into(), pos: None });
        m.transitions.push(Transition {
            from: Some(2),
            to: 0,
            conds: vec![Cond { input: "dead".into(), op: CondOp::Not, value: 0.0 }],
            at_end: false,
            blend: 0.0,
        });
    })
    .unwrap();
    assert!(out.ends_with("  input dead false\n  state Rest anim Idle\n  Rest -> Idle when not dead\n}\n"), "{out}");
    assert_eq!(scene(&out).doc.machine("Player").unwrap().transitions.len(), 4);
    // A state goes with its transitions; an input with its conditions.
    let out = delete_machine_item(&src, &s, "Player", MachineItem::State(1)).unwrap();
    let m = scene(&out).doc.machines[0].clone();
    assert_eq!((m.states.len(), m.transitions.len()), (1, 1));
    let out = delete_machine_item(&src, &s, "Player", MachineItem::Input(1)).unwrap();
    assert!(out.contains("Idle -> Move when speed > 0.5 blend 0.2s\n"));
    let out = make_entry(&src, &s, "Player", 1).unwrap();
    assert_eq!(scene(&out).doc.machines[0].states[0].name, "Move");
    let out = edit_machine(&src, &s, "Player", |m| m.any_pos = Some((10.0, 20.0))).unwrap();
    assert!(out.contains("  any at 10 20\n"));
    let s2 = scene(&out);
    assert_eq!(s2.doc.machines[0].any_pos, Some((10.0, 20.0)));
    let out = edit_machine(&out, &s2, "Player", |m| m.any_pos = Some((30.0, 20.0))).unwrap();
    assert!(out.contains("  any at 30 20\n") && !out.contains("any at 10"));
    let (out, name) = add_machine(&src, &s);
    assert_eq!(name, "Machine");
    assert!(out.ends_with("machine Machine {\n  state Idle at 40 80\n  state Run at 220 80\n}\n"));
    let s2 = scene(&out);
    assert_eq!(delete_machine(&out, &s2, "Machine").unwrap().trim_end(), src.trim_end());
}

const ARM: &str = "artboard 400x300
rect sleeve at 150 100 size 100 24 skin upper lower
group hand at 180 140
bone upper at 100 100 len 50 {
  bone lower at 50 0 len 50 ik hand flip
}
anim Wave 1s {
  hand.x 0: 180 | 1: 120
}
";

#[test]
fn bones_parse_with_ik_and_skin() {
    let s = scene(ARM);
    let lower = s.doc.node("lower").unwrap();
    assert_eq!(lower.kind, ShapeKind::Bone);
    assert_eq!(lower.width, 50.0);
    assert_eq!(lower.ik, Some(motif_core::Ik { target: "hand".into(), chain: 2, flip: true }));
    assert_eq!(s.doc.nodes[lower.parent.unwrap()].id, "upper");
    let skin = s.doc.node("sleeve").unwrap().skin.as_ref().unwrap();
    assert_eq!((skin.bones.len(), skin.bind.len()), (2, 2));
    // The setup pose already reaches for the target.
    let tip = s.doc.world_transform(s.doc.index_of("lower").unwrap()) * lower.bone_tip();
    assert!((tip - motif_core::kurbo::Point::new(180.0, 140.0)).hypot() < 0.1, "{tip:?}");
    // Posing moves the target, the arm follows and the sleeve bends with it.
    let a = s.doc.animation("Wave").unwrap();
    let end = a.pose(&s.doc, 1.0);
    let i = end.index_of("lower").unwrap();
    let tip = end.world_transform(i) * end.nodes[i].bone_tip();
    assert!((tip - motif_core::kurbo::Point::new(120.0, 140.0)).hypot() < 0.1, "{tip:?}");
    assert_ne!(end.node("sleeve").unwrap().path(), s.doc.node("sleeve").unwrap().path());
    // Bones draw nothing in the game.
    assert!(s.doc.node("upper").unwrap().fill.is_none());
}

#[test]
fn bone_errors_point_at_the_line() {
    let msg = |src: &str| parse(src).unwrap_err().iter().map(|e| e.message.clone()).collect::<Vec<_>>().join("\n");
    assert!(msg("rect a len 5\n").contains("только у bone"));
    assert!(msg("bone b len 50 ik nobody\n").contains("nobody"));
    assert!(msg("rect r skin r\n").contains("не кость"));
    assert!(msg("group g skin b\nbone b\n").contains("не у group"));
    assert!(msg("bone a len 50 {\n  bone b at 50 0 len 50 ik t {\n    group t\n  }\n}\n").contains("внутри цепочки"));
}

#[test]
fn bone_edits_round_trip() {
    let src = "artboard 400x300\nrect body at 200 150 size 40 80\nanim Idle 1s {\n}\n";
    let s = scene(src);
    // A root bone, then a child inside its block.
    let (out, root) = add_bone(src, &s, None, (200.0, 100.0), 50.0, 90.0).unwrap();
    assert!(out.contains("bone bone1 at 200 100 len 50 rot 90\n"), "{out}");
    let s = scene(&out);
    let (out, child) = add_bone(&out, &s, Some(&root), (50.0, 0.0), 40.0, 0.0).unwrap();
    assert!(out.contains("bone bone1 at 200 100 len 50 rot 90 {\n  bone bone2 at 50 0 len 40\n}"), "{out}");
    let s = scene(&out);
    assert_eq!(s.doc.nodes[s.doc.node(&child).unwrap().parent.unwrap()].id, root);
    // A third bone goes after the second, in the same block.
    let (out, _) = add_bone(&out, &s, Some(&root), (50.0, 0.0), 10.0, 0.0).unwrap();
    assert!(out.contains("  bone bone2 at 50 0 len 40\n  bone bone3 at 50 0 len 10\n}"), "{out}");
    let s = scene(&out);
    // Length edits write `len`.
    let out = set_prop(&out, &s, &child, Prop::Width, Value::Num(45.0)).unwrap();
    assert!(out.contains("bone bone2 at 50 0 len 45"));
    let s = scene(&out);

    // IK with a fresh target at the tip.
    let (out, target) = add_ik_target(&out, &s, &child).unwrap();
    let s = scene(&out);
    let t = s.doc.node(&target).unwrap();
    assert!((t.x - 200.0).abs() < 0.2 && (t.y - 195.0).abs() < 0.2, "{} {}", t.x, t.y);
    let ik = motif_core::Ik { target: target.clone(), chain: 2, flip: true };
    let out = set_ik(&out, &s, &child, Some(&ik)).unwrap();
    assert!(out.contains(&format!("len 45 ik {target} flip")), "{out}");
    let s = scene(&out);
    assert_eq!(s.doc.node(&child).unwrap().ik.as_ref(), Some(&ik));
    let out = set_ik(&out, &s, &child, Some(&motif_core::Ik { chain: 1, flip: false, ..ik })).unwrap();
    assert!(out.contains(&format!("len 45 ik {target} chain 1\n")), "{out}");
    let s = scene(&out);
    let out = set_ik(&out, &s, &child, None).unwrap();
    assert!(out.contains("bone bone2 at 50 0 len 45\n"), "{out}");
    let s = scene(&out);

    // Skin and back.
    let skin = motif_core::Skin { sharp: 6.0, ..motif_core::Skin::new(vec![root.clone(), child.clone()]) };
    let out = set_skin(&out, &s, "body", Some(&skin)).unwrap();
    assert!(out.contains("rect body at 200 150 size 40 80 skin bone1 bone2 skin-sharp 6\n"), "{out}");
    let s = scene(&out);
    assert_eq!(s.doc.node("body").unwrap().skin.as_ref().unwrap().bind.len(), 2);
    let out2 = set_skin(&out, &s, "body", None).unwrap();
    assert!(out2.contains("rect body at 200 150 size 40 80\n"), "{out2}");

    // Attaching keeps the shape where it is on the artboard.
    let before = s.doc.world_transform(s.doc.index_of("body").unwrap());
    let out = attach_to_bone(&out, &s, "body", Some(&child)).unwrap();
    let s = scene(&out);
    let i = s.doc.index_of("body").unwrap();
    assert_eq!(s.doc.nodes[s.doc.nodes[i].parent.unwrap()].id, child);
    let after = s.doc.world_transform(i);
    for p in [motif_core::kurbo::Point::ZERO, motif_core::kurbo::Point::new(10.0, 20.0)] {
        assert!(((before * p) - (after * p)).hypot() < 0.2, "{out}");
    }
    assert!(out.contains("    rect body at "), "{out}");
    // And out again.
    let out = attach_to_bone(&out, &s, "body", None).unwrap();
    let s = scene(&out);
    let i = s.doc.index_of("body").unwrap();
    assert!(s.doc.nodes[i].parent.is_none());
    assert!(((before * motif_core::kurbo::Point::ZERO) - (s.doc.world_transform(i) * motif_core::kurbo::Point::ZERO)).hypot() < 0.2, "{out}");
    assert!(out.contains("\nrect body at 200 150"), "{out}");
}

#[test]
fn variables_named_like_bone_props_stay_values() {
    let s = scene("let skin = #5ee0a0\nlet len = 30\nrect a size len fill skin\n");
    let a = s.doc.node("a").unwrap();
    assert_eq!(a.fill, Color::from_hex("#5ee0a0"));
    assert_eq!(a.width, 30.0);
}

#[test]
fn soft_bodies_and_solids_parse() {
    let s = scene("ellipse b size 40 soft 0.2 pressure 1.1 gravity 900 drive 0\nrect w size 100 20 solid\nrect plain\n");
    let soft = s.doc.node("b").unwrap().soft.unwrap();
    assert_eq!((soft.stiff, soft.pressure, soft.gravity, soft.drive), (0.2, 1.1, 900.0, 0.0));
    assert_eq!(soft.friction, motif_core::Soft::default().friction);
    assert!(s.doc.node("w").unwrap().solid);
    assert!(s.doc.node("plain").unwrap().soft.is_none());
    // `soft` alone uses the defaults; the panel's edits write single keys.
    let src = "ellipse b soft\n";
    let s = scene(src);
    assert_eq!(s.doc.node("b").unwrap().soft, Some(motif_core::Soft::default()));
    let out = set_style(src, &s, "b", "friction", Some("friction 0.6")).unwrap();
    assert_eq!(out, "ellipse b soft friction 0.6\n");
    let errs = parse("ellipse b soft 3\ngroup g soft\n").unwrap_err();
    assert_eq!(errs.len(), 2, "{errs:?}");
}

#[test]
fn springs_and_jiggling_bones_parse() {
    let s = scene("group g spring {\n  rect r spring 5 0.8\n}\nbone a len 40 {\n  bone b len 30 jiggle 4\n}\n");
    let d = motif_core::Spring::default();
    assert_eq!(s.doc.node("g").unwrap().spring, Some(d));
    assert_eq!(s.doc.node("r").unwrap().spring, Some(motif_core::Spring { freq: 5.0, damp: 0.8 }));
    assert_eq!(s.doc.node("b").unwrap().jiggle, Some(motif_core::Spring { freq: 4.0, damp: d.damp }));
    assert!(s.doc.node("a").unwrap().jiggle.is_none());
    let src = "rect r spring\n";
    let s = scene(src);
    let out = set_style(src, &s, "r", "spring", Some("spring 2 0.5")).unwrap();
    assert_eq!(out, "rect r spring 2 0.5\n");
    let errs = parse("rect r jiggle\nrect q spring 0\nrect p spring 3 -1\n").unwrap_err();
    assert_eq!(errs.len(), 3, "{errs:?}");
}

#[test]
fn squash_parses() {
    let s = scene("ellipse a squash\nellipse b squash 1 6 0.2\n");
    assert_eq!(s.doc.node("a").unwrap().squash, Some(motif_core::Squash::default()));
    assert_eq!(s.doc.node("b").unwrap().squash, Some(motif_core::Squash { amount: 1.0, spring: motif_core::Spring { freq: 6.0, damp: 0.2 } }));
    let errs = parse("rect r squash -1\nrect q squash 1 0\n").unwrap_err();
    assert_eq!(errs.len(), 2, "{errs:?}");
}

#[test]
fn orbits_parse_and_place() {
    let src = "ellipse planet at 200 200 size 200 200 yaw 90\nellipse moon size 10 10 on planet lat 0 lon 0 alt 20\nellipse spot size 30 30 on planet lon -90 wrap\nanim Spin 2s {\n  moon.lon 0: 0 | 2: 360\n}\n";
    let s = scene(src);
    let moon = s.doc.node("moon").unwrap();
    assert_eq!((moon.on.as_deref(), moon.alt), (Some("planet"), 20.0));
    assert!(s.doc.node("spot").unwrap().wrap);
    // Turned by yaw 90, the front of the planet faces right: the moon sits at its right edge.
    assert!((moon.x - 320.0).abs() < 1e-6, "{}", moon.x);
    let half = s.doc.animations[0].pose(&s.doc, 1.0);
    let moon = half.node("moon").unwrap();
    assert!((moon.x - 80.0).abs() < 1e-6, "half way round it is on the left: {}", moon.x);
    let out = set_prop(src, &s, "planet", motif_core::Prop::Pitch, motif_core::Value::Num(15.0)).unwrap();
    assert!(out.starts_with("ellipse planet at 200 200 size 200 200 yaw 90 pitch 15\n"), "{out}");
    let errs = parse("ellipse a on nowhere\ngroup g\nellipse b on g\nellipse c on c\nellipse d wrap\n").unwrap_err();
    assert_eq!(errs.len(), 4, "{errs:?}");
}

#[test]
fn emitters_parse() {
    let s = scene(
        "ellipse spark size 6 6 emit 40 life 1.5 speed 200 angle -60 spread 90 gravity 500 drag 0.2 fade 0 grow 0.3 spin 180 burst 10\nellipse blob soft gravity 900\n",
    );
    let e = s.doc.node("spark").unwrap().emit.unwrap();
    assert_eq!((e.rate, e.life, e.speed, e.angle, e.spread, e.gravity), (40.0, 1.5, 200.0, -60.0, 90.0, 500.0));
    assert_eq!((e.drag, e.fade, e.grow, e.spin, e.burst), (0.2, 0.0, 0.3, 180.0, 10.0));
    assert!(s.doc.node("spark").unwrap().soft.is_none(), "gravity goes to the particles");
    assert_eq!(s.doc.node("blob").unwrap().soft.unwrap().gravity, 900.0);
    assert_eq!(scene("rect r emit\n").doc.node("r").unwrap().emit, Some(motif_core::Emitter::default()));
    let errs = parse("rect a life 2\nrect b emit -1\ngroup g emit 5\nrect c emit fade 3\n").unwrap_err();
    assert_eq!(errs.len(), 4, "{errs:?}");
}

#[test]
fn ropes_and_cloth_parse() {
    use motif_core::{Pin, Rope};
    let s = scene(
        "group hand at 300 40\npath rope at 100 50 d \"M 0 0 L 100 0\" stroke #000 2 rope 0.8 0.05 pin start gravity 600 wind 120 tie hand\n\
         ellipse lamp size 10 10 hang rope 0.5\nellipse link size 16 10 links rope\nrect flag size 80 60 cloth pin top-left top-right\n",
    );
    let r = s.doc.node("rope").unwrap();
    assert_eq!(r.rope, Some(Rope { stiff: 0.8, damp: 0.05, gravity: 600.0, wind: 120.0, ..Rope::default() }));
    assert_eq!(r.tie, vec!["hand".to_string()]);
    assert_eq!(s.doc.node("lamp").unwrap().hang, Some(("rope".into(), 0.5)));
    assert_eq!(s.doc.node("link").unwrap().links.as_deref(), Some("rope"));
    let flag = s.doc.node("flag").unwrap().rope.clone().unwrap();
    assert!(flag.cloth);
    assert_eq!(flag.pins, vec![Pin::TopLeft, Pin::TopRight]);
    assert_eq!(scene("rect r hang x\nrect x rope\n").doc.node("r").unwrap().hang, Some(("x".into(), 1.0)));
    let errs = parse(
        "rect a pin start\nrect b rope pin top\nrect c cloth tie a\nrect d hang nothing\nrect e links f\nrect f cloth\nrect g rope 2\ngroup h rope\nrect i rope pin sideways\n",
    )
    .unwrap_err();
    assert_eq!(errs.len(), 8, "{errs:?}");
}

#[test]
fn morphs_parse_key_and_edit() {
    let src = "star s hidden\npath h d \"M 0 0 L 10 0 L 5 8 Z\" hidden\nellipse b morph s h\nanim A 2s {\n  b.morph 0: 0 | 2: 2\n}\n";
    let s = scene(src);
    let m = s.doc.node("b").unwrap().morph.clone().unwrap();
    assert_eq!((m.targets, m.amount), (vec!["s".to_string(), "h".to_string()], 0.0));
    assert_eq!(s.doc.animation("A").unwrap().keys_at(&s.doc, 1.0).node("b").unwrap().get(Prop::Morph), Value::Num(1.0));
    // A written amount is edited in place; a missing one goes after the targets.
    let out = set_prop(src, &s, "b", Prop::Morph, Value::Num(0.5)).unwrap();
    assert!(out.contains("ellipse b morph s h 0.5\n"), "{out}");
    let out = set_prop(&out, &scene(&out), "b", Prop::Morph, Value::Num(1.5)).unwrap();
    assert!(out.contains("ellipse b morph s h 1.5\n"), "{out}");
    assert_eq!(scene(&out).doc.node("b").unwrap().get(Prop::Morph), Value::Num(1.5));
    let errs =
        parse("ellipse a morph nothing\nellipse b morph b\ngroup g\nellipse c morph g\ngroup h morph a\nellipse d morph a -1\nellipse e morph\n").unwrap_err();
    assert_eq!(errs.len(), 6, "{errs:?}");
}

#[test]
fn bodies_parse_key_and_edit() {
    let src = "rect cube size 80 80 extrude 80 yaw 30 pitch 20\nellipse cone extrude 120 0\nanim A 1s {\n  cube.depth 0: 80 | 1: 20\n}\n";
    let s = scene(src);
    assert_eq!(s.doc.node("cube").unwrap().body, Some(motif_core::Body { depth: 80.0, taper: 1.0 }));
    assert_eq!(s.doc.node("cone").unwrap().body, Some(motif_core::Body { depth: 120.0, taper: 0.0 }));
    assert_eq!(s.doc.animation("A").unwrap().keys_at(&s.doc, 0.5).node("cube").unwrap().get(Prop::Depth), Value::Num(50.0));
    let out = set_prop(src, &s, "cube", Prop::Taper, Value::Num(0.5)).unwrap();
    assert!(out.contains("extrude 80 0.5 yaw"), "{out}");
    let out = set_prop(&out, &scene(&out), "cube", Prop::Depth, Value::Num(40.0)).unwrap();
    assert!(out.contains("extrude 40 0.5 yaw"), "{out}");
    let errs = parse("rect a extrude 0\ngroup g extrude 10\nrect b extrude 10 -1\n").unwrap_err();
    assert_eq!(errs.len(), 3, "{errs:?}");
}

#[test]
fn ui_pieces_parse_and_edit() {
    let src = "group btn ui slice 12 {\n  rect base size 80 30\n}\nrect bar size 80 10 slice 4 2\nrect tip size 40 20 slice 1 2 3 4\nrect icon size 10 10 ui\n";
    let s = scene(src);
    let n = |id: &str| s.doc.node(id).unwrap().clone();
    assert!(n("btn").ui && n("icon").ui && !n("bar").ui);
    assert_eq!(n("btn").slice, Some([12.0; 4]));
    assert_eq!(n("bar").slice, Some([4.0, 2.0, 4.0, 2.0]));
    assert_eq!(n("tip").slice, Some([1.0, 2.0, 3.0, 4.0]));
    assert_eq!(n("icon").slice, None);
    let out = set_style(src, &s, "bar", "slice", Some("slice 6")).unwrap();
    assert!(out.contains("size 80 10 slice 6\n"), "{out}");
    let out = set_style(src, &s, "icon", "ui", None).unwrap();
    assert!(out.contains("rect icon size 10 10\n"), "{out}");
    let errs = parse("rect a slice -1\nrect b slice 1 2 3\nrect c slice\n").unwrap_err();
    assert_eq!(errs.len(), 3, "{errs:?}");
}

#[test]
fn ui_roles_and_parts() {
    let src = "group hp ui progress {\n  rect frame size 80 10 part background\n  rect juice size 40 6 part fill\n}\ngroup ok ui {\n  rect body size 80 30\n  text label content \"OK\" font bold part text\n}\n";
    let s = scene(src);
    let n = |id: &str| s.doc.node(id).unwrap().clone();
    assert_eq!((n("hp").ui, n("hp").ui_role), (true, motif_core::UiRole::Progress));
    assert_eq!(n("juice").part, Some(motif_core::UiPart::Fill), "fill is a property too, but here the word of part");
    assert_eq!(n("ok").ui_role, motif_core::UiRole::Auto);
    let font = n("label").text.unwrap().font.clone().unwrap();
    assert_eq!(font.name, "IBMPlexSans-SemiBold.ttf");
    assert_eq!(font.data.as_slice(), motif_core::text::BOLD_FONT);
    let out = set_style(src, &s, "hp", "ui", Some("ui slider")).unwrap();
    assert!(out.contains("group hp ui slider {"), "{out}");
    let errors = |src: &str| parse(src).unwrap_err().into_iter().map(|e| e.message).collect::<Vec<_>>();
    assert_eq!(errors("group a ui gauge {\n  rect b\n}\n").len(), 1);
    assert_eq!(errors("rect a part fill\n"), ["part — деталь составного элемента: положите её в группу с ui, например ui progress"]);
    assert_eq!(
        errors("group a ui progress {\n  rect b part background\n  rect c part grabber\n}\n"),
        ["для ui progress внутри не хватает: part fill", "у «ui progress» детали: background, fill"]
    );
    assert_eq!(errors("group a ui {\n  rect b part text\n}\n"), ["part text — подпись: это должен быть text"]);
    assert_eq!(errors("group a ui check {\n  rect b part wheel\n  rect c part checked\n  rect d part unchecked\n}\n").len(), 1);
}

#[test]
fn a_slider_drawn_by_its_value_animation() {
    let src = "group knob ui slider dial turn 300 value 0.4 {\n  ellipse face size 60 60\n}\ngroup tube ui slider dial up {\n  rect glass size 20 80\n}\nanim Value 1s {\n  face.opacity 0: 0 | 1: 1\n  glass.opacity 0: 0 | 1: 1\n}\n";
    let s = scene(src);
    let knob = s.doc.node("knob").unwrap();
    assert_eq!(knob.dial, motif_core::Dial { drag: motif_core::DialDrag::Turn, arc: 300.0, value: 0.4 });
    assert_eq!(s.doc.node("tube").unwrap().dial.drag, motif_core::DialDrag::Up);
    // Drawn by Value, it needs no track or grabber; the panel writes the way back.
    assert!(motif_core::is_dial(&s.doc, s.doc.index_of("knob").unwrap()));
    let out = set_style(src, &s, "knob", "dial", Some("dial across")).unwrap();
    assert!(out.contains("group knob ui slider dial across value 0.4 {"), "{out}");
    let errors = |src: &str| parse(src).unwrap_err().into_iter().map(|e| e.message).collect::<Vec<_>>();
    assert_eq!(errors("group a ui slider {\n  rect b\n}\n"), ["для ui slider внутри не хватает: part track, part grabber"]);
    assert_eq!(errors("rect a size 10 10 dial turn\n"), ["dial и value — у слайдера, который рисует анимация Value: ui slider dial turn"]);
    assert_eq!(errors("rect a size 10 10 ui button radio difficulty\n"), ["radio — у флажка: ui check radio difficulty"]);
    assert_eq!(errors("group a ui slider dial sideways {\n  rect b\n}\nanim Value 1s {\n  b.x 0: 0 | 1: 10\n}\n").len(), 1);
    assert_eq!(errors("group a ui slider value 2 {\n  rect b\n}\nanim Value 1s {\n  b.x 0: 0 | 1: 10\n}\n").len(), 1);
}

#[test]
fn the_palette_is_the_top_level_colors() {
    let src = "// kit\nlet main = #3b6cff\nlet r = 14\nlet ink = main\nrect a size 10 10 fill main stroke ink 2\nrect b size 10 10 fill #ffffff\ngroup g {\n  rect c size 4 4\n}\n";
    let s = scene(src);
    let names: Vec<_> = s.palette.iter().map(|p| (p.name.as_str(), p.literal)).collect();
    assert_eq!(names, [("main", true), ("ink", false)], "numbers are not colors");
    assert_eq!(s.refs.get(&("a".to_string(), Prop::Fill)).map(String::as_str), Some("main"));
    assert_eq!(s.refs.get(&("a".to_string(), Prop::StrokeColor)).map(String::as_str), Some("ink"));
    // Recolor the palette: every user follows.
    let out = set_swatch(src, &s, "main", Color::rgb(255, 0, 0)).unwrap();
    assert!(out.contains("let main = #ff0000\n"), "{out}");
    assert_eq!(scene(&out).doc.node("a").unwrap().stroke, Some(Color::rgb(255, 0, 0)));
    assert!(set_swatch(src, &s, "ink", Color::BLACK).is_none(), "ink is written as a name");
    // Assign by name: replacing a color, a name, or adding the property.
    let out = set_ref(src, &s, "b", Prop::Fill, "main").unwrap();
    assert!(out.contains("rect b size 10 10 fill main\n"), "{out}");
    let out = set_ref(src, &s, "a", Prop::Fill, "ink").unwrap();
    assert!(out.contains("fill ink stroke ink 2"), "{out}");
    let out = set_ref(src, &s, "c", Prop::StrokeColor, "main").unwrap();
    assert!(out.contains("rect c size 4 4 stroke main 2\n"), "{out}");
    // A picked color replaces the name on the node's own line, not through override.
    let out = set_prop(src, &s, "a", Prop::Fill, Value::Color(Color::rgb(1, 2, 3))).unwrap();
    assert!(out.contains("rect a size 10 10 fill #010203 stroke ink 2") && !out.contains("override"), "{out}");
    // New colors go after the last one, or after the opening comments.
    let name = free_swatch_name(src);
    assert_eq!(name, "color1");
    let out = add_swatch(src, &s, &name, Color::WHITE);
    assert!(out.contains("let ink = main\nlet color1 = #ffffff\nrect a"), "{out}");
    let bare = "// title\n\nartboard 10x10\nrect a size 2 2\n";
    let out = add_swatch(bare, &scene(bare), "paper", Color::WHITE);
    assert!(out.starts_with("// title\n\nlet paper = #ffffff\n\nartboard"), "{out}");
    assert_eq!(scene(&out).palette.len(), 1);
}

#[test]
fn brushes_parse_key_and_edit() {
    let src = "rect rock size 80 60 fill #888888 rough 3 40 boil 8 seed 4 hatch 45 6 1.5 #222222 cross grain 0.2 3 bands 3\nellipse sun rough 2\nanim A 1s {\n  sun.rough 0: 2 | 1: 6\n}\n";
    let s = scene(src);
    let b = &s.doc.node("rock").unwrap().brush;
    assert_eq!((b.rough, b.wave, b.boil, b.seed, b.bands), (3.0, 40.0, 8.0, Some(4), 3));
    assert_eq!(b.hatch, Some(motif_core::Hatch { angle: 45.0, gap: 6.0, width: 1.5, color: Some(Color::rgb(0x22, 0x22, 0x22)), cross: true }));
    assert_eq!(b.grain, Some(motif_core::Grain { density: 0.2, size: 3.0, color: None }));
    let posed = s.doc.animation("A").unwrap().keys_at(&s.doc, 0.5);
    assert_eq!(posed.node("sun").unwrap().get(Prop::Rough), Value::Num(4.0));
    assert_eq!(posed.time, 0.5);
    let out = set_prop(src, &s, "rock", Prop::Rough, Value::Num(5.0)).unwrap();
    assert!(out.contains("rough 5 40 boil"), "{out}");
    let src2 = "rect plain size 10 10\n";
    let out = set_prop(src2, &scene(src2), "plain", Prop::Rough, Value::Num(2.0)).unwrap();
    assert!(out.contains("rough 2"), "{out}");
    let errs =
        parse("rect a rough -1\nrect b hatch 45 0\nrect c grain 2\nrect d bands 0\ngroup g rough 2\nrect e seed 1.5\nrect f hatch 45 6 nope\n").unwrap_err();
    assert_eq!(errs.len(), 8, "{errs:?}");
}

#[test]
fn fluids_parse() {
    let s = scene("rect pool size 60 120 fluid\nrect tap size 10 10 fluid 0.4 flow 80 gravity 600\n");
    assert_eq!(s.doc.node("pool").unwrap().fluid, Some(motif_core::Fluid::default()));
    assert_eq!(s.doc.node("tap").unwrap().fluid, Some(motif_core::Fluid { visc: 0.4, flow: 80.0, gravity: 600.0 }));
    let errs = parse("rect a flow 10\nrect b fluid 2\ngroup g fluid\nrect c fluid flow 0\n").unwrap_err();
    assert_eq!(errs.len(), 4, "{errs:?}");
}

#[test]
fn rolling_parses() {
    let s = scene("ellipse ball size 200 200\nellipse m size 20 20 on ball lat 40 roll 0.8 0.1 gravity 500\nellipse n size 20 20 on ball roll\n");
    assert_eq!(s.doc.node("m").unwrap().roll, Some(motif_core::Roll { friction: 0.8, damp: 0.1, gravity: 500.0 }));
    assert_eq!(s.doc.node("n").unwrap().roll, Some(motif_core::Roll::default()));
    let errs = parse("ellipse ball size 200 200\nellipse a size 20 20 roll\nellipse b size 20 20 on ball roll 2\n").unwrap_err();
    assert_eq!(errs.len(), 2, "{errs:?}");
}

#[test]
fn the_world_and_weights_parse() {
    use motif_core::kurbo::Vec2;
    let s = scene("artboard 400x300\nworld gravity 100 -450 wind 200 0 gust 0.4\nellipse leaf weight 0.2 sail 3\n");
    let w = s.doc.world;
    assert_eq!((w.gravity, w.wind, w.gust), (Vec2::new(100.0, -450.0), Vec2::new(200.0, 0.0), 0.4));
    let leaf = s.doc.node("leaf").unwrap();
    assert_eq!((leaf.weight, leaf.sail), (Some(0.2), Some(3.0)));
    assert!(scene("ellipse e\n").doc.world.is_default());
    let errs = parse("world wind 1 2 gust 3\nworld gust 0.1\nellipse e sail -1\n").unwrap_err();
    assert_eq!(errs.len(), 3, "{errs:?}");
}

#[test]
fn the_world_is_keyed_and_edited() {
    let src = "artboard 400x300\nworld wind 100 0\nellipse e\nanim Blow 2s {\n  world.wind-x 0: 0 | 2: 300\n}\n";
    let s = scene(src);
    let a = s.doc.animation("Blow").unwrap();
    assert_eq!(a.keys_at(&s.doc, 1.0).world.wind.x, 150.0);
    assert!(parse("ellipse e\nanim A 1s {\n  e.wind-x 0: 0\n}\n").is_err());
    assert!(parse("ellipse e\nanim A 1s {\n  world.x 0: 0\n}\n").is_err());
    // Editing rewrites the line, adds one under the artboard, or removes it at the defaults.
    let mut w = s.doc.world;
    w.gust = 0.5;
    let out = set_world(src, &s, &w);
    assert!(out.contains("world wind 100 0 gust 0.5\n"), "{out}");
    let out = set_world(src, &s, &motif_core::World::default());
    assert!(!out.contains("world wind"), "{out}");
    let bare = "artboard 400x300\nellipse e\n";
    let out = set_world(bare, &scene(bare), &w);
    assert_eq!(out, "artboard 400x300\nworld wind 100 0 gust 0.5\nellipse e\n");
    assert_eq!(scene(&out).doc.world.gust, 0.5);
}

#[test]
fn pulls_and_fans_parse_and_key() {
    use motif_core::Field;
    let s = scene("ellipse hole pull 3000 range 250\nrect fan1 rot -90 fan 500 cone 30\nellipse both pull -100 fan 50 range 80\n");
    assert_eq!(s.doc.node("hole").unwrap().pull, Some(Field { strength: 3000.0, range: 250.0, ..Field::pull() }));
    assert_eq!(s.doc.node("fan1").unwrap().fan, Some(Field { cone: 30.0, ..Field { strength: 500.0, ..Field::fan() } }));
    let both = s.doc.node("both").unwrap();
    assert_eq!((both.pull.unwrap().range, both.fan.unwrap().range), (80.0, 80.0));
    let errs = parse("ellipse a range 100\nellipse b pull 10 cone 30\nellipse c fan 10 cone 400\n").unwrap_err();
    assert_eq!(errs.len(), 3, "{errs:?}");
    let s = scene("ellipse hole pull 3000\nanim Pulse 2s {\n  hole.pull 0: 0 | 2: 4000\n}\n");
    let a = s.doc.animation("Pulse").unwrap();
    assert_eq!(a.keys_at(&s.doc, 1.0).node("hole").unwrap().pull.unwrap().strength, 2000.0);
}

#[test]
fn several_keys_move_scale_delete_and_paste() {
    let src = "rect a\nrect b\nanim Go 2s {\n  a.x  0: 0 | 1: 5 ease-in | 1.5: 8\n  b.y  0.5: 1 hold\n}\n";
    let s = scene(src);
    let at = |node: &str, prop, time| KeyAt { node: node.into(), prop, time };
    // Move two keys of different tracks together.
    let keys = [at("a", Prop::X, 1.0), at("b", Prop::Y, 0.5)];
    let out = retime_keys(src, &s, "Go", &keys, |t| t + 0.25).unwrap();
    assert!(out.contains("a.x  0: 0 | 1.25: 5 ease-in | 1.5: 8"), "{out}");
    assert!(out.contains("b.y  0.75: 1 hold"), "{out}");
    // Landing on an unpicked key is refused; moving past one reorders.
    assert!(retime_keys(src, &s, "Go", &[at("a", Prop::X, 1.0)], |t| t + 0.5).is_none());
    let out = retime_keys(src, &s, "Go", &[at("a", Prop::X, 1.0)], |t| t + 0.75).unwrap();
    assert!(out.contains("a.x  0: 0 | 1.5: 8 | 1.75: 5 ease-in"), "{out}");
    // Stretch around 0: every key of the track doubles its time.
    let all = [at("a", Prop::X, 0.0), at("a", Prop::X, 1.0), at("a", Prop::X, 1.5)];
    let out = retime_keys(src, &s, "Go", &all, |t| t * 2.0).unwrap();
    assert!(out.contains("a.x  0: 0 | 2: 5 ease-in | 3: 8"), "{out}");
    // A missing key fails the whole edit.
    assert!(retime_keys(src, &s, "Go", &[at("a", Prop::X, 0.7)], |t| t).is_none());

    // Delete: a track left empty disappears with its line.
    let out = delete_keys(src, &s, "Go", &[at("a", Prop::X, 1.0), at("b", Prop::Y, 0.5)]).unwrap();
    assert_eq!(out, "rect a\nrect b\nanim Go 2s {\n  a.x  0: 0 | 1.5: 8\n}\n");
    scene(&out);

    // Curves of several keys.
    let out = set_keys_ease(src, &s, "Go", &[at("a", Prop::X, 0.0), at("a", Prop::X, 1.0)], "linear").unwrap();
    assert!(out.contains("a.x  0: 0 linear | 1: 5 linear | 1.5: 8"), "{out}");

    // Copy two keys of a and paste them at 0.5 onto b: a new track.
    let clip = copy_keys(src, &s, "Go", &[at("a", Prop::X, 1.5), at("a", Prop::X, 1.0)]);
    assert_eq!(clip.len(), 2);
    assert_eq!(clip[0].key, KeyText { time: 1.0, value: "5".into(), ease: Some("ease-in".into()) });
    let (out, pasted) = paste_keys(src, &s, "Go", &clip, 0.5, Some("b")).unwrap();
    assert!(out.contains("  b.x  0.5: 5 ease-in | 1: 8\n}"), "{out}");
    assert_eq!(pasted, vec![at("b", Prop::X, 0.5), at("b", Prop::X, 1.0)]);
    scene(&out);
    // Pasting onto the same track replaces a key at the same time.
    let (out, _) = paste_keys(src, &s, "Go", &clip, 0.0, None).unwrap();
    assert!(out.contains("a.x  0: 5 ease-in | 0.5: 8 | 1: 5 ease-in | 1.5: 8"), "{out}");
    assert!(paste_keys(src, &s, "Go", &clip, 0.0, Some("nope")).is_none());

    // write_tracks keeps other text and sorts.
    let out = write_tracks(
        src,
        &s,
        "Go",
        &[(
            "b".into(),
            Prop::Y,
            vec![KeyText { time: 1.0, value: "3".into(), ease: None }, KeyText { time: 0.2, value: "2".into(), ease: Some("spring".into()) }],
        )],
    )
    .unwrap();
    assert!(out.contains("b.y  0.2: 2 spring | 1: 3\n"), "{out}");
}

#[test]
fn pointer_listeners_parse_edit_and_go_with_their_object() {
    use motif_core::{Listener, PointerEvent};
    let src = MACHINE
        .replace("Idle->Run", "Idle->Move")
        .replace("  state Idle at 40 60\n", "  listen b hover -> hover\n  listen b click -> jump\n  state Idle at 40 60\n");
    let s = scene(&src);
    let m = s.doc.machine("Player").unwrap();
    assert_eq!(
        m.listeners,
        vec![
            Listener { node: "b".into(), event: PointerEvent::Hover, input: "hover".into() },
            Listener { node: "b".into(), event: PointerEvent::Click, input: "jump".into() },
        ]
    );
    // Errors: a missing object, a missing input, an event that does not fit the input.
    let msg = |src: &str| parse(src).unwrap_err().iter().map(|e| e.message.clone()).collect::<Vec<_>>().join(" | ");
    assert!(msg(&src.replace("listen b hover", "listen nope hover")).contains("нет объекта «nope»"));
    assert!(msg(&src.replace("hover -> hover", "hover -> nope")).contains("нет входа «nope»"));
    assert!(msg(&src.replace("hover -> hover", "hover -> speed")).contains("флагом"));
    assert!(msg(&src.replace("hover -> hover", "x -> hover")).contains("числом"));
    assert!(msg(&src.replace("b hover", "b wiggle")).contains("нет события «wiggle»"));
    // Editing: change one, add one, remove one.
    let out = edit_machine(&src, &s, "Player", |m| {
        m.listeners[0].event = PointerEvent::Press;
        m.listeners.push(Listener { node: "b".into(), event: PointerEvent::X, input: "speed".into() });
    })
    .unwrap();
    assert!(out.contains("  listen b press -> hover\n  listen b click -> jump\n"), "{out}");
    assert!(out.contains("  listen b x -> speed\n}"), "{out}");
    let out = edit_machine(&src, &s, "Player", |m| {
        m.listeners.remove(0);
    })
    .unwrap();
    assert!(!out.contains("listen b hover") && out.contains("listen b click -> jump"), "{out}");
    scene(&out);
    // Deleting the input or the object removes its listeners.
    let out = delete_machine_item(&src, &s, "Player", MachineItem::Input(1)).unwrap();
    assert!(!out.contains("listen b hover"), "{out}");
    scene(&out);
    let out = delete_node(&src, &s, "b").unwrap();
    assert!(!out.contains("listen"), "{out}");
}

#[test]
fn stacks_panels_and_anchors() {
    use motif_core::{StackAlign, StackDir};
    let src = "artboard 400x300\ngroup g at 200 150 stack row end gap 4 pad 6 {\n  rect bg fit\n  rect a size 20 10\n  rect b size 30 30\n}\nrect c at -10 10 size 8 8 anchor right\n";
    let s = scene(src);
    let g = s.doc.node("g").unwrap();
    let st = g.stack.unwrap();
    assert_eq!((st.dir, st.align, st.gap, st.pad), (StackDir::Row, StackAlign::End, 4.0, 6.0));
    assert!(s.doc.node("bg").unwrap().fit);
    assert_eq!(s.doc.node("c").unwrap().anchor, Some((1.0, 0.0)));
    // Laid out on parse: the panel covers 20 + 4 + 30 plus padding, the anchored rect sits at the right edge.
    assert_eq!((s.doc.node("bg").unwrap().width, s.doc.node("bg").unwrap().height), (66.0, 42.0));
    let c = s.doc.world_transform(s.doc.index_of("c").unwrap()) * motif_core::kurbo::Point::ZERO;
    assert_eq!((c.x, c.y), (390.0, 10.0));
    // Words, not names: `center` and `bottom` need no `let`.
    let s = scene("group g stack column center {\n}\nrect c anchor center bottom\n");
    assert_eq!(s.doc.node("c").unwrap().anchor, Some((0.5, 1.0)));
    let msg = |src: &str| parse(src).unwrap_err().iter().map(|e| e.message.clone()).collect::<Vec<_>>().join(" | ");
    assert!(msg("rect r stack row\n").contains("бывает у group"));
    assert!(msg("group g stack diagonal {\n}\n").contains("stack row или stack column"));
    assert!(msg("rect r anchor up\n").contains("anchor"));
    assert!(msg("rect r anchor left right\n").contains("anchor"));
    // The panel writes these keys back like any other.
    let out = set_style(src, &scene(src), "g", "stack", Some("stack column")).unwrap();
    assert!(out.contains("group g at 200 150 stack column gap 4 pad 6 {"), "{out}");
    let out = set_style(src, &scene(src), "c", "anchor", None).unwrap();
    assert!(out.contains("rect c at -10 10 size 8 8\n"), "{out}");
}

#[test]
fn weight_strokes_parse_paint_and_clear() {
    use motif_core::WeightStroke;
    use motif_core::kurbo::Point;
    let src = "bone a at 0 0 len 50 {\n  bone b at 50 0 len 50\n}\nrect bar at 50 0 size 100 20 skin a b\n";
    let s = scene(src);
    let stroke = WeightStroke { bone: "b".into(), radius: 12.0, amount: 0.8, points: vec![Point::new(-10.04, -8.0), Point::new(-10.0, 8.0)] };
    let out = add_weight_stroke(src, &s, "bar", &stroke).unwrap();
    assert!(out.ends_with("\nweights bar {\n  b 12 0.8 at -10 -8 -10 8\n}\n"), "{out}");
    let s = scene(&out);
    let paint = &s.doc.node("bar").unwrap().skin.as_ref().unwrap().paint;
    assert_eq!(paint.len(), 1);
    assert_eq!(paint[0].points[0], Point::new(-10.0, -8.0));
    // The next stroke goes into the same block.
    let erase = WeightStroke { bone: "a".into(), radius: 6.0, amount: -1.0, points: vec![Point::new(20.0, 0.0)] };
    let out = add_weight_stroke(&out, &s, "bar", &erase).unwrap();
    assert!(out.contains("  b 12 0.8 at -10 -8 -10 8\n  a 6 -1 at 20 0\n}\n"), "{out}");
    let s = scene(&out);
    assert_eq!(s.doc.node("bar").unwrap().skin.as_ref().unwrap().paint.len(), 2);
    // A bone outside the skin is refused by the edit and reported by the script.
    assert!(add_weight_stroke(&out, &s, "bar", &WeightStroke { bone: "c".into(), ..erase.clone() }).is_none());
    assert!(parse(&out.replace("  a 6", "  zz 6")).is_err());
    // Clearing one bone keeps the others; clearing the last removes the block.
    let one = clear_weights(&out, &s, "bar", Some("a")).unwrap();
    assert!(one.contains("weights bar {\n  b 12") && !one.contains("a 6 -1"), "{one}");
    let s1 = scene(&one);
    assert_eq!(clear_weights(&one, &s1, "bar", Some("b")).unwrap(), src);
    assert_eq!(clear_weights(&out, &s, "bar", None).unwrap(), src);
    // Deleting the shape takes its weights along.
    let gone = delete_node(&out, &s, "bar").unwrap();
    assert!(!gone.contains("weights"), "{gone}");
}

#[test]
fn weight_brush_finds_the_drawn_point_on_a_folded_knee() {
    use motif_core::kurbo::Point;
    let s = scene(include_str!("../../../examples/weights.msc"));
    let mut d = s.doc.clone();
    let shin = d.index_of("shin2").unwrap();
    d.nodes[shin].rotation = -95.0;
    d.rig();
    let i = d.index_of("leg2").unwrap();
    // Next to the bent knee the plane folds: the point outside the leg that lands on the same
    // spot must not win over the one inside it.
    for p in [Point::new(7.5, 22.7), Point::new(5.0, 37.0), Point::new(-5.0, 30.0), Point::new(-12.0, -40.0)] {
        let back = d.unskin_point(i, d.skin_point(i, p).unwrap(), None).unwrap();
        assert!((back - p).hypot() < 0.01, "{p:?} → {back:?}");
    }
}

#[test]
fn events_are_read_checked_and_written() {
    let src = "rect event at 10 10\nanim Run 0.4s loop {\n  event.x  0: 0 | 0.4: 10\n  event step 0.1 0.3\n  event dust 0\n}\n";
    let s = scene(src);
    let run = s.doc.animation("Run").unwrap();
    assert_eq!(run.tracks.len(), 1, "event.x is a track of the object «event»");
    let got: Vec<(&str, f64)> = run.events.iter().map(|e| (e.name.as_str(), e.time)).collect();
    assert_eq!(got, [("dust", 0.0), ("step", 0.1), ("step", 0.3)]);
    // Out of range, twice the same name, no time.
    for bad in ["anim A 1s {\n  event hit 2\n}\n", "anim A 1s {\n  event hit 0.2\n  event hit 0.5\n}\n", "anim A 1s {\n  event hit\n}\n"] {
        assert!(parse(bad).is_err(), "{bad}");
    }
    // Moving a time, renaming, adding a new event and removing one.
    let out = write_event(src, &s, "Run", "step", "step", &[0.3, 0.12]).unwrap();
    assert!(out.contains("  event step 0.12 0.3\n"), "{out}");
    let out = write_event(src, &s, "Run", "step", "шаг", &[0.1, 0.3]).unwrap();
    assert!(out.contains("  event шаг 0.1 0.3\n"), "{out}");
    let out = write_event(src, &s, "Run", "land", "land", &[0.2]).unwrap();
    assert!(out.ends_with("  event dust 0\n  event land 0.2\n}\n"), "{out}");
    let out = write_event(src, &s, "Run", "dust", "dust", &[]).unwrap();
    assert!(!out.contains("dust") && scene(&out).doc.animation("Run").unwrap().events.len() == 2, "{out}");
    assert!(write_event(src, &s, "Run", "step", "dust", &[0.1]).is_none(), "name taken");
    assert!(write_event(src, &s, "Run", "step", "two words", &[0.1]).is_none());
}

#[test]
fn animations_are_renamed_copied_reversed_and_deleted() {
    let src = "rect a\nanim Open 1s {\n  a.x  0: 0 ease-in | 0.6: 80 cubic 0.3 0 0.6 1.2 | 1: 100\n  a.y  0: 0 hold | 0.5: 40\n  event click 0.2\n}\n\nanim Idle 1s loop {\n  a.r  0: 0 | 1: 5\n}\nmachine M {\n  state Shut anim Open\n  state Idle\n}\n";
    let s = scene(src);
    // Backwards: every moment of the copy is the original at `1 - t`.
    let (out, back) = reverse_anim(src, &s, "Open").unwrap();
    assert_eq!(back, "OpenBack");
    let r = scene(&out);
    let (a, b) = (r.doc.animation("Open").unwrap(), r.doc.animation("OpenBack").unwrap());
    let at = |anim: &motif_core::Animation, t: f64, p: Prop| anim.keys_at(&r.doc, t).node("a").unwrap().get(p).as_num().unwrap();
    for k in 0..=20 {
        let t = k as f64 / 20.0;
        assert!((at(b, t, Prop::X) - at(a, 1.0 - t, Prop::X)).abs() < 0.6, "x at {t}: {} vs {}", at(b, t, Prop::X), at(a, 1.0 - t, Prop::X));
        // The hold jumps at the start of its segment, 0.01 s late.
        if (t - 0.5).abs() > 0.02 {
            assert_eq!(at(b, t, Prop::Y), at(a, 1.0 - t, Prop::Y), "y at {t}");
        }
    }
    assert_eq!(b.events.iter().map(|e| (e.name.as_str(), e.time)).collect::<Vec<_>>(), [("click", 0.8)]);
    assert!(out.contains("\n\nanim OpenBack 1s {\n"), "{out}");
    // A plain copy, named after the original.
    let (out, copy) = duplicate_anim(src, &s, "Idle").unwrap();
    assert_eq!(copy, "Idle2");
    assert!(out.contains("anim Idle2 1s loop {\n  a.r  0: 0 | 1: 5\n}\n"), "{out}");
    // Renaming carries the states along: `state Idle` plays the new name now.
    let out = rename_anim(src, &s, "Idle", "Rest", None).unwrap();
    let m = scene(&out).doc.machines[0].clone();
    assert_eq!(m.states[1].anim, "Rest");
    assert!(rename_anim(src, &s, "Idle", "Open", None).is_err(), "taken");
    assert!(rename_anim(src, &s, "Idle", "two words", None).is_err());
    // A state plays it: no deleting; otherwise the block goes.
    assert!(delete_anim(src, &s, "Open").unwrap_err().contains("M.Shut"));
    let free = "rect a\nanim A 1s {\n  a.x  0: 0 | 1: 5\n}\n\nanim B 1s {\n}\n";
    let out = delete_anim(free, &scene(free), "A").unwrap();
    assert_eq!(out, "rect a\nanim B 1s {\n}\n");
}

#[test]
fn group_and_ungroup_keep_everything_in_place() {
    use motif_core::kurbo::Point;
    let src = "rect a at 10 10 size 20 20\nellipse b at 50 30 size 20 20\nrect c at 100 100 size 10 10\nanim Go 1s {\n  a.x  0: 10 | 1: 70\n  b.rot  0: 0 | 1: 90\n}\n";
    let s = scene(src);
    let at = |s: &Scene, id: &str, t: f64| {
        let posed = s.doc.animations[0].pose_with(&s.doc, t, &mut motif_core::SimCache::default());
        let i = posed.index_of(id).unwrap();
        let p = posed.world_transform(i) * Point::ZERO;
        ((p.x * 1000.0).round() / 1000.0, (p.y * 1000.0).round() / 1000.0)
    };
    let (out, g) = group_nodes(src, &s, &["b".into(), "a".into()], None).unwrap();
    assert_eq!(g, "group1");
    // In the middle of their box, where b (the topmost) was, under c.
    assert!(out.contains("group group1 at 30 20 {\n  rect a at -20 -10 size 20 20\n  ellipse b at 20 10 size 20 20\n}\nrect c"), "{out}");
    assert!(out.contains("a.x  0: -20 | 1: 40"), "keys follow the new parent: {out}");
    let grouped = scene(&out);
    for t in [0.0, 0.5, 1.0] {
        for id in ["a", "b", "c"] {
            assert_eq!(at(&grouped, id, t), at(&s, id, t), "{id} at {t}");
        }
    }
    // Taken apart again: the same places and the same order.
    let (back, kids) = ungroup(&out, &grouped, "group1", None).unwrap();
    assert_eq!(kids, ["a", "b"]);
    let back_s = scene(&back);
    assert_eq!(back_s.doc.nodes.iter().map(|n| n.id.as_str()).collect::<Vec<_>>(), ["a", "b", "c"]);
    assert!(back.contains("a.x  0: 10 | 1: 70"), "{back}");
    for t in [0.0, 0.5, 1.0] {
        assert_eq!(at(&back_s, "a", t), at(&s, "a", t));
    }
    // Things that would be lost are refused.
    let turned = out.replace("group group1 at 30 20 {", "group group1 at 30 20 rot 10 {");
    assert!(ungroup(&turned, &scene(&turned), "group1", None).is_err());
    let nested = "group g at 50 50 {\n  rect a\n}\nrect b at 10 10\n";
    assert!(group_nodes(nested, &scene(nested), &["a".into(), "b".into()], None).is_err(), "different parents");
    // One step up and down among neighbours.
    let up = restack(src, &s, "a", true).unwrap();
    assert_eq!(scene(&up).doc.nodes.iter().map(|n| n.id.as_str()).collect::<Vec<_>>(), ["b", "a", "c"]);
    assert!(restack(src, &s, "c", true).is_none());
    assert!(restack(src, &s, "a", false).is_none());
}

#[test]
fn boards_split_with_a_shared_head() {
    let src = "// kit\nlet ink = #10131d\n\n// the button\nartboard \"A\" 100x50 cell 1 0\nrect a fill ink\n\nartboard \"B\" 40x40\n// not a heading\nrect b fill ink\n";
    let (head, ranges) = boards(src);
    assert_eq!(&src[head], "// kit\nlet ink = #10131d\n\n");
    let texts: Vec<&str> = ranges.into_iter().map(|r| &src[r]).collect();
    assert_eq!(texts, ["// the button\nartboard \"A\" 100x50 cell 1 0\nrect a fill ink\n\n", "artboard \"B\" 40x40\n// not a heading\nrect b fill ink\n"]);
    assert_eq!(head_end(&format!("let x = 1\n{}", texts[1])), 10, "the board after the head");
    assert_eq!(head_end(&format!("let x = 1\n{}", texts[0])), 10, "with its comment");
    // One board: no head, the whole script.
    let one = "let c = #fff\nartboard \"A\" 10x10\nrect a fill c\n";
    let whole = 0..one.len();
    assert_eq!(boards(one), (0..0, vec![whole]));
    // `artboards` or an indented line opens no board.
    assert_eq!(boards("artboard \"A\" 10x10\nlet artboards = 1\n  artboard\n").1.len(), 1);

    let scenes = parse_boards(src, None).unwrap();
    assert_eq!(scenes.len(), 2);
    assert_eq!(scenes[0].board_cell, Some((1, 0)));
    assert_eq!(scenes[1].board_cell, None);
    assert_eq!(scenes[1].doc.artboard.name, "B");
    assert_eq!(scenes[1].doc.node("b").unwrap().fill, scenes[0].doc.node("a").unwrap().fill, "the head's colors reach every board");
    // Errors point into the whole file; one in the head is told once.
    let errs = parse_boards(&src.replace("rect b fill ink", "rect b fill nope").replace("#10131d", "missing"), None).unwrap_err();
    let lines: Vec<usize> = errs.iter().map(|e| e.line).collect();
    assert!(lines.contains(&2) && lines.contains(&10), "{errs:?}");
    assert_eq!(lines.iter().filter(|l| **l == 2).count(), 1, "{errs:?}");
    assert!(parse("artboard \"A\" 10x10 cell -1 0\n").is_err());
}

#[test]
fn board_lines_are_edited_in_place() {
    let src = "// x\nartboard \"Old\" 100x50 bg #fff ui\nrect a\n";
    assert_eq!(rename_artboard(src, "New \"one\"").unwrap(), "// x\nartboard \"New one\" 100x50 bg #fff ui\nrect a\n");
    assert_eq!(rename_artboard("artboard 10x10\n", "A").unwrap(), "artboard \"A\" 10x10\n");
    assert_eq!(set_artboard_size(src, 200.4, 0.0).unwrap(), "// x\nartboard \"Old\" 200x1 bg #fff ui\nrect a\n");
    let celled = set_board_cell(src, Some((2, 1))).unwrap();
    assert_eq!(celled, "// x\nartboard \"Old\" 100x50 bg #fff ui cell 2 1\nrect a\n");
    assert_eq!(set_board_cell(&celled, Some((0, 3))).unwrap(), "// x\nartboard \"Old\" 100x50 bg #fff ui cell 0 3\nrect a\n");
    assert_eq!(set_board_cell(&celled, None).unwrap(), src);
    assert_eq!(scene(&celled).board_cell, Some((2, 1)));
    let like = scene(src);
    assert_eq!(new_board("Б\"2", 64.0, 32.0, (1, 0), Some(&like.doc)), "artboard \"Б2\" 64x32 bg #ffffff ui cell 1 0\n");
    assert_eq!(new_board("C", 10.0, 10.0, (0, 2), None), "artboard \"C\" 10x10 cell 0 2\n");
    assert_eq!(board_file_name(" Кнопка / ok "), "Кнопка___ok");
    assert_eq!(board_file_name("  "), "board");
}

#[test]
fn boards_lie_on_a_grid() {
    use motif_core::kurbo::Point;
    // Three boards, no cells: two columns, filled row by row; columns as wide as their widest.
    let g = board_layout(&[(100.0, 50.0, None), (40.0, 80.0, None), (60.0, 30.0, None)]);
    assert_eq!(g.cells, [(0, 0), (1, 0), (0, 1)]);
    let gap = BOARD_GAP;
    assert_eq!(g.origins, [Point::new(0.0, 0.0), Point::new(100.0 + gap, 0.0), Point::new(0.0, 80.0 + gap)]);
    // Cells as written win; a taken cell goes to the next free one.
    let g = board_layout(&[(10.0, 10.0, Some((2, 0))), (10.0, 10.0, Some((2, 0))), (10.0, 10.0, Some((0, 0)))]);
    assert_eq!(g.cells, [(2, 0), (1, 0), (0, 0)]);
    assert_eq!(g.cell_at(Point::new(10.0 + gap + 5.0, 3.0)), (1, 0));
    assert_eq!(g.cell_at(Point::new(500.0, 500.0)), (3, 1), "past the grid: the next new cell");
    assert_eq!(g.cell_origin((3, 1)), Point::new(3.0 * (10.0 + gap), 10.0 + gap));
}

#[test]
fn interface_effects_parse_and_edit() {
    let src = "rect b size 100 40 fill #222 glow 12 3 #ffd34dcc inset 0 2 #00000088 noise 0.2 2 stroke-gradient linear 90 #fff #888\n";
    let s = scene(src);
    let b = s.doc.node("b").unwrap();
    assert_eq!(b.glow, Some(motif_core::Glow { size: 12.0, spread: 3.0, color: Color { r: 0xff, g: 0xd3, b: 0x4d, a: 0xcc } }));
    assert_eq!(b.inset.map(|i| (i.dx, i.dy, i.blur)), Some((0.0, 2.0, 0.0)));
    assert_eq!(b.noise, Some(motif_core::Noise { amount: 0.2, size: 2.0 }));
    assert_eq!(b.stroke_gradient.as_ref().map(|g| g.colors.len()), Some(2));
    // A gradient stroke without `stroke` is 2 wide.
    assert_eq!(b.stroke_width, 2.0);
    // The glow's size is edited in place; the inset gets its blur after the offset.
    let out = set_prop(src, &s, "b", Prop::Glow, Value::Num(20.0)).unwrap();
    assert!(out.contains("glow 20 3 #ffd34dcc"), "{out}");
    let out = set_prop(src, &s, "b", Prop::InsetBlur, Value::Num(5.0)).unwrap();
    assert!(out.contains("inset 0 2 5 #00000088"), "{out}");
    // Keys animate them, like a hover that lights the button up.
    let anim = scene(&format!("{src}anim Hover 0.2s {{\n  b.glow 0: 0 | 0.2: 16\n  b.noise 0: 0.2 | 0.2: 0\n}}\n"));
    assert_eq!(anim.doc.animation("Hover").unwrap().tracks.len(), 2);
    let bad = parse("rect b size 10 10 glow #fff\nrect c size 10 10 stroke-gradient #fff #000\n").err().unwrap();
    assert_eq!(bad.len(), 2, "{bad:?}");
}

#[test]
fn the_interface_check_reads_and_moves_the_kit() {
    use motif_core::{UiCheck, UiRole};
    let boards = crate::parse_boards(include_str!("../../../examples/ui-kit.msc"), None).unwrap_or_else(|e| panic!("{e:?}"));
    let doc = |name: &str| boards.iter().find(|s| s.doc.artboard.name == name).map(|s| s.doc.clone()).unwrap();
    // Values and check marks come from the drawing.
    let toggles = doc("Переключатели");
    let mut check = UiCheck::new(&toggles);
    assert_eq!(check.control("sound").map(|c| (c.role, c.checked)), Some((UiRole::Check, Some(true))));
    assert_eq!(check.control("music").map(|c| (c.role, c.checked)), Some((UiRole::Switch, Some(true))));
    let on = |c: &UiCheck| ["easy", "normal", "hard"].map(|r| c.control(r).and_then(|c| c.checked) == Some(true));
    assert_eq!(on(&check), [true, false, false]);
    let click = |c: &mut UiCheck, d: &motif_core::Document, x: f64, y: f64| {
        let at = Some(motif_core::kurbo::Point::new(x, y));
        c.pointer(d, at, true);
        c.pointer(d, at, false)
    };
    // A switch toggles; a radio button turns on and the others of its group off, and stays on.
    click(&mut check, &toggles, 460.0, 80.0);
    assert_eq!(check.control("music").and_then(|c| c.checked), Some(false));
    click(&mut check, &toggles, 200.0, 150.0);
    assert_eq!(on(&check), [false, true, false]);
    click(&mut check, &toggles, 200.0, 150.0);
    assert_eq!(on(&check), [false, true, false]);
    // A click puts the typing into a field, and a click elsewhere takes it out.
    let fields = doc("Поля");
    let mut check = UiCheck::new(&fields);
    assert_eq!(check.control("quality").map(|c| c.role), Some(UiRole::Option));
    let said = click(&mut check, &fields, 170.0, 80.0);
    assert_eq!(said.map(|s| s.0), Some("name_field".to_string()));
    assert!(check.control("name_field").is_some_and(|c| c.focused));
    click(&mut check, &fields, 170.0, 170.0);
    assert!(check.control("name_field").is_some_and(|c| !c.focused));
    let bar = doc("Полосы");
    let check = UiCheck::new(&bar);
    let health = check.control("health").and_then(|c| c.value).unwrap();
    assert!(health > 0.5 && health < 0.9, "{health}");
    // Drawn at its value, the bar looks as written: setting what was read changes nothing.
    let mut same = check.pose(&bar, &bar);
    same.rig();
    let mut orig = bar.clone();
    orig.rig();
    let fill = |d: &motif_core::Document| d.world_bounds(d.index_of("bar_fill").unwrap()).unwrap();
    let (a, b) = (fill(&orig), fill(&same));
    assert!((a.x0 - b.x0).abs() < 1e-6 && (a.x1 - b.x1).abs() < 1e-6, "{a:?} {b:?}");
    for (board, id) in [("Полосы", "volume"), ("Полосы", "scroller"), ("Полосы", "rating")] {
        let c = UiCheck::new(&doc(board));
        let v = c.control(id).and_then(|c| c.value);
        assert!(v.is_some_and(|v| (0.0..=1.0).contains(&v)), "{board}: {v:?}");
    } // Keys drawn like an animated one hover and press like it, as their shared style does in Godot.
    let calc = crate::parse_boards(include_str!("../../../godot-apps/calculator/calculator.msc"), None).unwrap_or_else(|e| panic!("{e:?}"));
    let check = UiCheck::new(&calc[0].doc);
    let buttons: Vec<&str> = check.controls.iter().filter(|c| c.role == UiRole::Button).map(|c| c.node.as_str()).collect();
    for key in ["k7", "k8", "k1", "k0", "kac", "kdiv", "kmul", "keq"] {
        assert!(buttons.contains(&key), "{key} in {buttons:?}");
    }
}

#[test]
fn directions_parse_and_edit() {
    let src = "artboard 400x300\ndirections 8 turn head body mirror\nellipse head\nellipse body\n";
    let s = scene(src);
    let d = s.doc.directions.clone().unwrap();
    assert_eq!((d.count, d.turn.clone(), d.mirror), (8, vec!["head".to_string(), "body".to_string()], true));
    assert!(parse("directions 6\nellipse e\n").is_err(), "4 or 8 sides");
    assert!(parse("directions 4 turn nobody\nellipse e\n").is_err(), "turned objects exist");
    assert!(parse("directions 4 spin\n").is_err());
    assert!(parse("group g {\n  directions 4\n}\n").is_err(), "top level only");
    assert!(parse("directions 4\ndirections 8\n").is_err(), "one line");
    // Editing rewrites the line, adds one under the artboard, or removes it.
    let four = motif_core::Directions { count: 4, turn: vec!["head".into()], mirror: false };
    assert_eq!(set_directions(src, &s, Some(&four)), "artboard 400x300\ndirections 4 turn head\nellipse head\nellipse body\n");
    assert_eq!(set_directions(src, &s, None), "artboard 400x300\nellipse head\nellipse body\n");
    let bare = "artboard 400x300\nellipse head\n";
    let out = set_directions(bare, &scene(bare), Some(&four));
    assert_eq!(out, "artboard 400x300\ndirections 4 turn head\nellipse head\n");
    assert_eq!(scene(&out).doc.directions, Some(four));
    // A deleted object leaves `turn`, and the script still reads.
    let out = delete_node(src, &s, "body").unwrap();
    assert_eq!(out, "artboard 400x300\ndirections 8 turn head mirror\nellipse head\n");
    let out = delete_node(&out, &scene(&out), "head").unwrap();
    assert_eq!(out, "artboard 400x300\ndirections 8 mirror\n");
    // Its group taking both with it.
    let grouped = "artboard 400x300\ndirections 4 turn head body\ngroup hero {\n  ellipse head\n  ellipse body\n}\nrect floor\n";
    let out = delete_node(grouped, &scene(grouped), "hero").unwrap();
    assert_eq!(out, "artboard 400x300\ndirections 4\nrect floor\n");
    assert!(scene(&out).doc.directions.unwrap().turn.is_empty());
    // An object not turned leaves the line alone.
    let out = delete_node(grouped, &scene(grouped), "floor").unwrap();
    assert!(out.contains("directions 4 turn head body\n"), "{out}");
}
