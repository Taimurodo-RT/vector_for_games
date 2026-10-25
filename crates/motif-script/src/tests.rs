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
