//! Independent end-to-end checks for editable positioned CSS components.
use super::import;
use motif_core::kurbo::{Point, Shape};
use motif_core::{Color, Document, UiRole};

fn document(html: &str, css: &str) -> Document {
    let result = import(html, css).unwrap();
    motif_script::parse(&result.source).unwrap_or_else(|e| panic!("{e:?}\n{}", result.source)).doc
}

fn center(doc: &Document, id: &str) -> Point {
    doc.world_transform(doc.index_of(id).unwrap_or_else(|| panic!("missing {id}"))) * Point::ZERO
}

fn near(a: f64, b: f64) {
    assert!((a - b).abs() < 1e-4, "{a} != {b}");
}

fn sample(doc: &Document, point: Point) -> [u8; 4] {
    let px = motif_render::render_cpu(doc, motif_render::CpuOptions { background: false, ..Default::default() }).unwrap();
    let x = point.x.round() as u32;
    let y = point.y.round() as u32;
    let p = px.pixel(x, y).unwrap_or_else(|| panic!("sample outside artboard: {point:?}"));
    [p.red(), p.green(), p.blue(), p.alpha()]
}

#[test]
fn absolute_percent_sizes_and_offsets_use_the_positioned_padding_box() {
    let doc = document(
        "<div id='parent'><span id='pin'></span><div id='flow'></div></div>",
        "#parent{position:relative;width:200px;height:100px;padding:0;border:0}#pin{position:absolute;left:10%;top:30%;width:25%;height:20%;background:red}#flow{width:40px;height:10px}",
    );
    near(doc.node("pin_bg").unwrap().width, 50.0);
    near(doc.node("pin_bg").unwrap().height, 20.0);
    let delta = center(&doc, "pin") - center(&doc, "parent");
    near(delta.x, -55.0);
    near(delta.y, -10.0);
}

#[test]
fn absolute_nodes_skip_static_wrappers_and_do_not_inflate_auto_flow() {
    let doc = document(
        "<div id='parent'><div id='wrapper'><span id='pin'></span></div></div>",
        "#parent{position:relative;width:200px;height:100px}#wrapper{width:60px;height:20px}#pin{position:absolute;left:50%;top:50%;width:10px;height:10px;background:red}",
    );
    let delta = center(&doc, "pin") - center(&doc, "parent");
    near(delta.x, 5.0);
    near(delta.y, 5.0);

    let doc = document(
        "<div id='parent'><div id='flow'></div><span id='decoration'></span></div>",
        "#parent{position:relative}#flow{width:40px;height:20px}#decoration{position:absolute;left:0;top:0;width:200px;height:100px;background:blue}",
    );
    near(doc.node("parent_bg").unwrap().width, 40.0);
    near(doc.node("parent_bg").unwrap().height, 20.0);
}

#[test]
fn transform_function_order_and_top_left_origin_preserve_world_corners() {
    let doc = document(
        "<div id='parent'><span id='pin'></span></div>",
        "#parent{position:relative;width:200px;height:200px}#pin{position:absolute;left:20px;top:30px;width:40px;height:20px;background:red;transform-origin:0 0;transform:translateX(20px) rotate(90deg) scale(2,1)}",
    );
    let parent = center(&doc, "parent");
    let transform = doc.world_transform(doc.index_of("pin_bg").unwrap());
    let a = transform * Point::new(-20.0, -10.0);
    let b = transform * Point::new(20.0, -10.0);
    let c = transform * Point::new(-20.0, 10.0);
    near(a.x - parent.x, -60.0);
    near(a.y - parent.y, -70.0);
    near(b.x - a.x, 0.0);
    near(b.y - a.y, 80.0);
    near(c.x - a.x, -20.0);
    near(c.y - a.y, 0.0);
}

#[test]
fn polygon_and_overflow_intersect_as_editable_vector_masks() {
    let doc = document(
        "<div id='mask'><span id='paint'></span></div>",
        "#mask{position:relative;width:100px;height:100px;overflow:hidden;clip-path:polygon(-50% 0%,150% 0%,50% 100%)}#paint{position:absolute;left:-30px;top:-30px;width:160px;height:160px;background:red}",
    );
    assert!(doc.nodes.iter().any(|n| n.clip), "the imported mask must remain an editable native clip group");
    assert!(doc.node("paint_bg").unwrap().path().is_some());
    let middle = center(&doc, "mask");
    let inside = sample(&doc, middle + (-40.0, -40.0));
    assert!(inside[0] > 240 && inside[3] > 240, "{inside:?}");
    assert_eq!(sample(&doc, middle + (-60.0, -40.0))[3], 0, "overflow must clip the polygon outside its box");
    assert_eq!(sample(&doc, middle + (35.0, 35.0))[3], 0, "the polygon must clip the bottom corner");
}

#[test]
fn double_border_keeps_a_transparent_gap_and_inset_stays_inside() {
    let doc = document("<div id='frame'></div>", "#frame{width:100px;height:60px;box-sizing:border-box;border:6px double blue;background:transparent}");
    let middle = center(&doc, "frame");
    for dx in [-49.0, -45.0] {
        let p = sample(&doc, middle + (dx, 0.0));
        assert!(p[2] > 220 && p[3] > 220, "double-border bands: {dx}: {p:?}");
    }
    assert_eq!(sample(&doc, middle + (-47.0, 0.0))[3], 0, "double border must retain its gap");
    assert_eq!(sample(&doc, middle + (-40.0, 0.0))[3], 0, "transparent interior must remain transparent");

    let doc = document("<div id='well'></div>", "#well{width:80px;height:60px;background:white;box-shadow:inset 0 0 12px black}");
    assert!(doc.nodes.iter().any(|n| n.inset.is_some()), "inset must use an editable native effect");
    let middle = center(&doc, "well");
    let edge = sample(&doc, middle + (-38.0, 0.0));
    let inner = sample(&doc, middle);
    assert!(u16::from(edge[0]) + 20 < u16::from(inner[0]), "inner shadow must darken the edge: {edge:?} vs {inner:?}");
    assert_eq!(sample(&doc, middle + (-43.0, 0.0))[3], 0, "inset must not leak outside the shape");
}

#[test]
fn supplied_play_component_retains_text_shapes_arrows_and_state_geometry() {
    let result = import(include_str!("../../../examples/import/uiverse-play.html"), include_str!("../../../examples/import/uiverse-play.css")).unwrap();
    let doc = motif_script::parse(&result.source).unwrap().doc;
    let button = doc.nodes.iter().find(|n| n.ui_role == UiRole::Button).unwrap();
    let button_id = button.id.clone();
    let plate = doc.node(&format!("{button_id}_bg")).unwrap();
    near(plate.width, 143.0);
    near(plate.height, 52.0);
    assert!(doc.nodes.iter().any(|n| n.text.as_ref().is_some_and(|t| t.content.trim() == "P L A Y")));
    for id in ["leftTop", "rightTop", "leftBottom", "rightBottom"] {
        let node = doc.node(&format!("{id}_bg")).unwrap();
        near(node.width, 52.0);
        near(node.height, 52.0);
        near(doc.world_transform(doc.index_of(&format!("{id}_bg")).unwrap()).as_coeffs()[0].abs(), std::f64::consts::FRAC_1_SQRT_2);
        assert!(doc.node(&format!("{id}__inset")).unwrap().inset.is_some());
    }
    for id in ["leftArrow", "rightArrow"] {
        let node = doc.node(&format!("{id}_bg")).unwrap();
        near(node.width, 15.73);
        near(node.height, 15.6);
        assert_eq!(node.fill, Color::from_hex("#2761c3"));
    }
    let left = center(&doc, "leftArrow") - center(&doc, &button_id);
    let right = center(&doc, "rightArrow") - center(&doc, &button_id);
    near(left.x, -82.94);
    near(right.x, 82.225);
    near(left.y, 0.0);
    near(right.y, 0.0);
    assert!(doc.nodes.iter().filter(|n| n.clip).count() >= 3, "frame and both arrows must have vector clipping");
    assert!(doc.node("leftTop_bg").unwrap().path().unwrap().bounding_box().width() > 0.0);
}

#[test]
fn original_css_ids_survive_reserved_words_and_generated_mask_name_collisions() {
    let doc = document(
        "<div id='clip'></div><div id='html_clip'></div><div id='fill'></div><div id='shadow'></div><div id='at'></div><div id='panel'></div><div id='panel__polygon_mask'></div>",
        "div{width:20px;height:20px}#clip{background:yellow}#html_clip{background:purple}#fill{background:red}#shadow{background:green}#at{background:blue}#panel{clip-path:polygon(0 0,100% 0,0 100%);border:3px double blue;overflow:hidden}#panel__polygon_mask{background:#123456}",
    );
    for id in ["fill", "shadow", "at"] {
        assert!(doc.node(id).is_some(), "legal existing MSC ID must remain available: {id}");
    }
    for color in ["#ffff00", "#800080", "#ff0000", "#008000", "#0000ff", "#123456"] {
        assert!(doc.nodes.iter().any(|n| n.fill == Color::from_hex(color)), "selectors must keep matching the original CSS ID: {color}");
    }
    let unique: std::collections::HashSet<_> = doc.nodes.iter().map(|n| &n.id).collect();
    assert_eq!(unique.len(), doc.nodes.len());
}

fn play_document() -> Document {
    document(include_str!("../../../examples/import/uiverse-play.html"), include_str!("../../../examples/import/uiverse-play.css"))
}

#[test]
fn supplied_keyframes_combine_color_scale_shadows_and_a_continuous_alternating_loop() {
    let doc = play_document();
    let button = doc.nodes.iter().find(|n| n.ui_role == UiRole::Button).unwrap().id.clone();
    let frame = doc.nodes.iter().find(|n| n.inset.is_some_and(|s| (s.blur - 15.0).abs() < 1e-6)).unwrap().id.clone();
    let entry = doc.animation("Hover").expect("hover entrance must be editable on the normal timeline");
    let repeated = doc.animation("HoverLoop").expect("infinite animation needs its own repeating segment");
    assert!(!entry.looping && repeated.looping);
    near(entry.duration, 0.75);
    near(repeated.duration, 1.2);
    let midway = entry.pose(&doc, 0.2);
    assert_eq!(midway.node("leftTop_bg").unwrap().fill, Color::from_hex("#27c39f"));
    assert_eq!(midway.node("leftBottom_bg").unwrap().fill, Color::from_hex("#2781c3"), "delayed keyframes must retain their backwards fill");
    let final_pose = entry.pose(&doc, entry.duration);
    for id in ["leftTop", "rightTop", "leftBottom", "rightBottom"] {
        let bg = format!("{id}_bg");
        let node = final_pose.node(&bg).unwrap();
        assert_eq!(node.fill, Color::from_hex("#27c39f"));
        assert!(node.inset.is_none_or(|s| s.color.a == 0), "the outer-shadow keyframe must replace the initial inset: {id}");
        assert!(final_pose.node(&format!("{id}__inset")).unwrap().inset.is_none_or(|s| s.color.a == 0));
        let shadow = node.shadow.unwrap();
        near(shadow.blur, 2.0);
        assert_eq!(shadow.color, Color::from_hex("#27c39f").unwrap());
        let matrix = final_pose.world_transform(final_pose.index_of(&bg).unwrap()).as_coeffs();
        near(matrix[0].hypot(matrix[1]), 1.25);
    }
    let glow = final_pose.node(&frame).unwrap().inset.unwrap();
    near(glow.blur, 32.0);
    assert_eq!(glow.color, Color::from_hex("#27c39f").unwrap());
    assert!(center(&final_pose, "leftArrow").x > center(&final_pose, &button).x);
    assert!(center(&final_pose, "rightArrow").x < center(&final_pose, &button).x);
    let begin = repeated.pose(&doc, 0.0);
    let end = repeated.pose(&doc, repeated.duration - 1e-7);
    for id in ["leftArrow", "rightArrow"] {
        near(center(&begin, id).x, center(&final_pose, id).x);
        near(center(&begin, id).x, center(&end, id).x);
        let positions: Vec<_> = (0..=12).map(|i| center(&repeated.pose(&doc, repeated.duration * i as f64 / 12.0), id).x).collect();
        let lo = positions.iter().copied().fold(f64::INFINITY, f64::min);
        let hi = positions.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        assert!(hi - lo > 9.0, "the arrow must actually oscillate by its CSS translate distance: {id}: {positions:?}");
    }
    for animation in [entry, repeated] {
        let mut times: Vec<_> = animation.tracks.iter().flat_map(|track| track.keys.iter().map(|key| key.time)).collect();
        times.sort_by(f64::total_cmp);
        times.dedup();
        for time in times {
            let pose = animation.pose(&doc, time);
            for id in ["leftArrow_bg", "rightArrow_bg"] {
                let i = pose.index_of(id).unwrap();
                let bounds = pose.world_transform(i).transform_rect_bbox(pose.nodes[i].path().unwrap().bounding_box());
                assert!(
                    bounds.x0 >= -1e-5 && bounds.y0 >= -1e-5 && bounds.x1 <= doc.artboard.width + 1e-5 && bounds.y1 <= doc.artboard.height + 1e-5,
                    "visible animation must fit its exported artboard: {} t={time} {id} {bounds:?}, board={}x{}",
                    animation.name,
                    doc.artboard.width,
                    doc.artboard.height
                );
            }
        }
    }
    let opts = motif_render::CpuOptions { background: false, ..Default::default() };
    let base_px = motif_render::render_cpu(&doc, opts).unwrap();
    let end_px = motif_render::render_cpu(&final_pose, opts).unwrap();
    assert_ne!(base_px.data(), end_px.data(), "the complete animation must reach the existing CPU renderer");
}

#[test]
fn actual_ui_hover_enters_the_imported_loop_and_leaving_restores_the_drawing() {
    let doc = play_document();
    let button = doc.nodes.iter().find(|n| n.ui_role == UiRole::Button).unwrap().id.clone();
    let mut check = motif_core::UiCheck::new(&doc);
    check.pointer(&doc, Some(center(&doc, &button)), false);
    assert!(check.control(&button).unwrap().hover);
    assert!(check.advance(&doc, 0.85), "infinite hover must keep requesting redraws after its entrance");
    let a = check.pose(&doc, &doc);
    assert_eq!(a.node("leftTop_bg").unwrap().fill, Color::from_hex("#27c39f"));
    assert!(check.advance(&doc, 0.25));
    let b = check.pose(&doc, &doc);
    assert!((center(&a, "leftArrow").x - center(&b, "leftArrow").x).abs() > 1.0, "hover must keep moving in the actual UI path");
    check.pointer(&b, None, false);
    check.advance(&doc, 1.0);
    assert!(!check.control(&button).unwrap().hover);
    let restored = check.pose(&doc, &doc);
    let opts = motif_render::CpuOptions { background: false, ..Default::default() };
    assert_eq!(
        motif_render::render_cpu(&restored, opts).unwrap().data(),
        motif_render::render_cpu(&doc, opts).unwrap().data(),
        "leaving hover must remove all loop geometry and effects"
    );
}
