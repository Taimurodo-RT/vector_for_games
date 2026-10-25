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
    let src = "let c = #ff0000\nrect a fill c\nfor i in 0..2 {\n  rect b{i} at {i * 10} 0\n}\n";
    let s = scene(src);
    let out = set_prop(src, &s, "a", Prop::Fill, Value::Color(Color::WHITE)).unwrap();
    assert!(out.ends_with("override {\n  a.fill = #ffffff\n}\n"), "{out}");
    let s2 = scene(&out);
    assert_eq!(s2.doc.node("a").unwrap().fill, Some(Color::WHITE));
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
        let value = if prop.is_color() { Value::Color(Color::rgb(1, 2, 3)) } else { Value::Num(7.0) };
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
    let mut b = crate::Backdrop { file: "ref.png".into(), x: 0.0, y: 0.0, scale: 1.0, opacity: 0.4, hidden: false, span: Span::new(0, 0) };
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
