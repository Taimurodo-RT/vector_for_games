//! Regressions derived from a browser reference, including transparent padding-box effects.
use motif_core::{Document, UiRole};

fn document(html: &str, css: &str) -> Document {
    let source = super::import(html, css).unwrap().source;
    motif_script::parse(&source).unwrap().doc
}

#[test]
fn imported_states_open_with_the_authored_normal_and_keep_a_transparent_canvas() {
    let doc = document(include_str!("../../../examples/import/uiverse-play.html"), include_str!("../../../examples/import/uiverse-play.css"));
    assert_eq!(doc.artboard.background.a, 0, "component import must not add a white rectangle");
    let first = &doc.animations[0];
    assert_eq!(first.name, "Normal");
    assert_eq!(first.pose(&doc, 0.0).nodes, doc.nodes, "initial selection must not start inside hover");
    let text = doc.nodes.iter().find(|node| node.text.is_some()).unwrap();
    let font = text.text.as_ref().unwrap().font.as_ref().unwrap();
    assert_eq!(font.name, "LiberationSans-Bold.ttf");
    assert!((text.text.as_ref().unwrap().bounds(&text.text_style).width() - 43.836914).abs() < 0.02);
    let button = doc.nodes.iter().find(|n| n.ui_role == UiRole::Button).unwrap();
    assert_eq!(doc.node(&format!("{}_bg", button.id)).unwrap().width, 143.0);
    let image = motif_render::render_cpu(&doc, Default::default()).unwrap();
    assert_eq!(image.pixel(0, 0).unwrap().alpha(), 0);
}

#[test]
fn transparent_inset_uses_the_padding_edge_and_matches_browser_alpha_profile() {
    let css = "#frame{width:143px;height:52px;border:5px double #2761c3;box-shadow:inset 0 0 15px #195480}";
    let mut doc = document("<div id='frame'></div>", css);
    let inset = doc.index_of("frame__inset").unwrap();
    assert_eq!((doc.nodes[inset].width, doc.nodes[inset].height), (143.0, 52.0));
    let bg = doc.node("frame_bg").unwrap();
    assert_eq!((bg.width, bg.height), (153.0, 62.0));
    // Isolate the CSS inner-shadow layer, so double-border ink does not pollute
    // the alpha measurements just outside the padding edge.
    for node in &mut doc.nodes {
        if node.id.contains("__border_") {
            node.stroke = None;
        }
    }
    let b = doc.world_bounds(inset).unwrap();
    let x = b.center().x.floor() as u32;
    let top = b.y0.round() as u32;
    let bottom = b.y1.round() as u32 - 1;
    let image = motif_render::render_cpu(&doc, Default::default()).unwrap();
    // Edge/Chromium, blur=15 and transparent background: 118,106,94,82,70.
    for (offset, expected) in [118_i32, 106, 94, 82, 70].into_iter().enumerate() {
        let a = image.pixel(x, top + offset as u32).unwrap().alpha() as i32;
        let z = image.pixel(x, bottom - offset as u32).unwrap().alpha() as i32;
        assert!((a - expected).abs() <= 10, "padding-edge alpha {offset}: {a} versus browser {expected}");
        assert!((a - z).abs() <= 2, "top/bottom symmetry: {a} vs {z}");
    }
    assert_eq!(image.pixel(x, top - 1).unwrap().alpha(), 0, "inset must not bleed into the border area");
    assert!(image.pixel(x, top + 20).unwrap().alpha() < 10, "centre remains transparent");
}

#[test]
fn missing_background_and_explicit_transparency_have_identical_shadow_pixels() {
    let html = "<div id='frame'></div>";
    let css = "#frame{width:100px;height:60px;border:4px double blue;box-shadow:inset 0 0 12px green}";
    let implicit = document(html, css);
    let explicit = document(html, &format!("{css}#frame{{background:transparent}}"));
    let a = motif_render::render_cpu(&implicit, Default::default()).unwrap();
    let b = motif_render::render_cpu(&explicit, Default::default()).unwrap();
    assert_eq!(a.data(), b.data());
}
