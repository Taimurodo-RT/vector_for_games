use super::*;
use motif_core::kurbo::Shape;
use motif_core::{Color, Prop, UiRole, Value};

fn scene(result: &ImportResult) -> motif_script::Scene {
    motif_script::parse(&result.source).unwrap_or_else(|e| panic!("{e:?}\n{}", result.source))
}

#[test]
fn button_is_editable_with_semantics_entities_and_css_box_model() {
    let out = import(
        "<button id='play' class='primary'>Играть &amp; &quot;тест&quot; &#9733;</button>",
        ".primary { width: 120px; height: 32px; padding: 4px 10px; border: 2px solid #123456; border-radius: 8px; background: #2468ab; color: white; }",
    )
    .unwrap();
    let doc = scene(&out).doc;
    assert_eq!(doc.node("play").unwrap().ui_role, UiRole::Button);
    let plate = doc.node("play_bg").unwrap();
    assert_eq!(plate.width + plate.stroke_width, 144.0);
    assert_eq!(plate.height + plate.stroke_width, 44.0);
    assert_eq!(plate.fill, Color::from_hex("#2468ab"));
    assert_eq!(plate.stroke_width, 2.0);
    assert!(doc.nodes.iter().any(|n| n.text.as_ref().is_some_and(|t| t.content == "Играть & \"тест\" ★")));
    assert!(doc.nodes.iter().find(|n| n.text.is_some()).unwrap().part == Some(motif_core::UiPart::Text));
    assert!(out.warnings.is_empty(), "{:?}", out.warnings);
}

#[test]
fn cascade_inline_important_source_order_and_custom_properties_are_resolved() {
    let html = "<style>button{background:#111111}.x{background:#222222}#go{background:#333333;color:var(--ink, white)}</style><button class='x' id='go' style='background:#444444;--ink:#dedede'>Go</button>";
    let out = import(html, "button { background:#555555 !important; } #go {background:#666666 !important}").unwrap();
    let doc = scene(&out).doc;
    assert_eq!(doc.node("go_bg").unwrap().fill, Color::from_hex("#666666"));
    assert!(doc.nodes.iter().any(|n| n.text.is_some() && n.fill == Color::from_hex("#dedede")));
    let out = import("<button id='b' style='padding-left:3px;padding:10px 20px'>B</button>", "#b{padding:1px}").unwrap();
    let doc = scene(&out).doc;
    let plate = doc.node("b_bg").unwrap();
    let text = doc.nodes.iter().find(|n| n.text.is_some()).unwrap();
    assert!(plate.width - text.path().unwrap().bounding_box().width() > 39.0);
}

#[test]
fn state_styles_become_standard_ui_clips_and_leave_source_unchanged() {
    let out=import("<button id='b'>Go</button>","button{background:#112233;color:#ffffff}button:hover{background:#abcdef}button:active{opacity:.6}button:disabled{opacity:.3}button:focus{border-color:#ff0000}").unwrap();
    let doc = scene(&out).doc;
    for name in ["Hover", "Press", "Disabled", "Focus"] {
        assert!(doc.animation(name).is_some(), "{name}");
    }
    assert_eq!(doc.animation("Hover").unwrap().pose(&doc, 0.2).node("b_bg").unwrap().fill, Color::from_hex("#abcdef"));
    let press = doc.animation("Press").unwrap().pose(&doc, 0.2);
    assert_eq!(press.node("b").unwrap().opacity, 0.6);
    assert_eq!(press.node("b_bg").unwrap().fill, Color::from_hex("#abcdef"));
    assert_eq!(doc.node("b_bg").unwrap().fill, Color::from_hex("#112233"));
    assert_eq!(doc.animation("Disabled").unwrap().pose(&doc, 0.2).node("b").unwrap().opacity, 0.3);
    assert_eq!(doc.animations[0].tracks[0].prop, Prop::Fill);
    assert!(out.warnings.is_empty(), "{:?}", out.warnings);
}

#[test]
fn selectors_descendants_children_and_flex_place_nodes_without_flattening() {
    let out = import(
        "<div id='row'><button id='one'>One</button><div><button id='two' class='accent'>Two</button></div></div>",
        "#row{display:flex;gap:12px;padding:10px}#row > button{background:red}#row .accent{background:blue}",
    )
    .unwrap();
    let doc = scene(&out).doc;
    assert_eq!(doc.node("one_bg").unwrap().fill, Color::from_hex("#ff0000"));
    assert_eq!(doc.node("two_bg").unwrap().fill, Color::from_hex("#0000ff"));
    let a = doc.world_transform(doc.index_of("one").unwrap()) * motif_core::kurbo::Point::ZERO;
    let b = doc.world_transform(doc.index_of("two").unwrap()) * motif_core::kurbo::Point::ZERO;
    assert!(b.x > a.x + 30.0);
    assert!((b.y - a.y).abs() < 0.1);
    assert_eq!(doc.node("one").unwrap().parent, doc.index_of("row"));
    assert_ne!(doc.node("two").unwrap().parent, doc.index_of("row"));
}

#[test]
fn unsupported_css_units_resources_and_code_produce_warnings_without_execution() {
    let out=import("<script>throw new Error('<button>fake</button>')</script><link rel='stylesheet' href='https://example.invalid/x.css'><button id='b' onclick='danger()'>Safe</button>","@media(width>10px){button{color:red}} button::before{content:'x'}button {width:50vw;transform:translateX(4px);background:url(secret.png);unknown:1}").unwrap();
    let doc = scene(&out).doc;
    assert!(doc.node("b").is_some());
    assert!(!out.source.contains("fake"));
    assert!(!out.source.contains("danger"));
    for needle in ["script", "link", "onclick", "@media", "::before", "50vw", "transform", "url(", "unknown"] {
        assert!(out.warnings.iter().any(|w| w.contains(needle)), "missing {needle}: {:?}", out.warnings);
    }
    assert!(import("<button>X</button>", "button{color:red").is_err());
}

#[test]
fn text_is_visible_and_gradient_shadow_alpha_render_through_the_existing_renderer() {
    let out = import(
        "<button id='b'>Hello</button>",
        "button {background:linear-gradient(90deg,#123456,#abcdef);box-shadow:0 3px 8px rgba(0,0,0,.3);color:rgb(255,255,255);font-weight:700}",
    )
    .unwrap();
    let s = scene(&out);
    assert!(out.warnings.is_empty(), "{:?}", out.warnings);
    assert!(s.doc.node("b_bg").unwrap().gradient.is_some());
    assert!(s.doc.node("b_bg").unwrap().shadow.is_some());
    let p = motif_render::render_cpu(&s.doc, motif_render::CpuOptions { scale: 1.0, antialias: true, background: true }).unwrap();
    assert!(p.width() > 60 && p.height() > 40);
    assert!(s.doc.nodes.iter().any(|n| n.text.is_some() && n.get(Prop::FontSize) == Value::Num(16.0)));
}

#[test]
fn limits_and_duplicate_ids_do_not_generate_invalid_source() {
    assert!(import(&"x".repeat(200_001), "").is_err());
    let out = import("<button id='go'>A</button><button id='go'>B</button><button id='group'>C</button>", "").unwrap();
    let s = scene(&out);
    assert!(s.doc.node("go").is_some() && s.doc.node("go_2").is_some() && s.doc.node("html_group").is_some());
}

#[test]
fn formatted_html_and_generated_suffix_ids_keep_states_and_text_inheritance_valid() {
    let html = "<div>\n  <button id='x'>A</button>\n  <button id='x_bg'>B</button>\n</div>";
    let out = import(html, "*{color:#222222}button:hover{color:#ffffff}").unwrap();
    let s = scene(&out);
    assert!(s.doc.node("x_bg").is_some());
    assert!(s.doc.node("x_bg_2").is_some());
    assert!(s.doc.animation("Hover").is_some());
    let out = import("<button id='x'>X</button>", ":root{--accent:#aabbcc}button{background:var(--accent)}*{opacity:.5}").unwrap();
    let s = scene(&out);
    assert_eq!(s.doc.node("x_bg").unwrap().fill, Color::from_hex("#aabbcc"));
    assert_eq!(s.doc.nodes.iter().find(|n| n.text.is_some()).unwrap().opacity, 1.0, "universal CSS does not target character nodes");
}

#[test]
fn background_shorthand_resets_gradient_and_unrepresentable_state_does_not_fake_a_fill_key() {
    let out = import("<button id='b'>B</button>", "button{background-image:linear-gradient(90deg,red,blue);background:#00ff00}").unwrap();
    let doc = scene(&out).doc;
    assert!(doc.node("b_bg").unwrap().gradient.is_none());
    assert_eq!(doc.node("b_bg").unwrap().fill, Color::from_hex("#00ff00"));
    assert!(out.warnings.is_empty());
    let out = import("<button id='b'>B</button>", "button{background:linear-gradient(90deg,red,blue)}button:hover{background:#00ff00}").unwrap();
    let doc = scene(&out).doc;
    assert!(doc.node("b_bg").unwrap().gradient.is_some());
    assert!(!doc.animations.iter().any(|a| a.tracks.iter().any(|t| t.node == "b_bg" && t.prop == Prop::Fill)));
    assert!(out.warnings.iter().any(|w| w.contains("градиента") && w.contains("hover")), "{:?}", out.warnings);
}

#[test]
fn inline_text_retains_boundary_spaces_and_paragraph_baseline_and_br_is_explicitly_reported() {
    for tag in ["button", "p"] {
        let out = import(&format!("<{tag} id='p'>Hello <b>world</b> again</{tag}>"), "").unwrap();
        let doc = scene(&out).doc;
        let texts: Vec<_> = doc.nodes.iter().filter(|n| n.text.is_some()).collect();
        let contents: Vec<_> = texts.iter().map(|n| n.text.as_ref().unwrap().content.as_str()).collect();
        assert_eq!(contents, vec!["Hello ", "world", " again"]);
        let ys: Vec<_> = texts.iter().map(|n| (doc.world_transform(doc.index_of(&n.id).unwrap()) * motif_core::kurbo::Point::ZERO).y).collect();
        assert!(ys.iter().all(|y| (y - ys[0]).abs() < 0.1), "{ys:?}");
        assert!(out.warnings.is_empty(), "{:?}", out.warnings);
    }
    let out = import("<button>Line one<br>Line two</button>", "").unwrap();
    assert!(out.warnings.iter().any(|w| w.contains("<br>") && w.contains("пробелом")));
    let out = import("<button>A&nbsp;B</button>", "").unwrap();
    assert!(scene(&out).doc.nodes.iter().any(|n| n.text.as_ref().is_some_and(|t| t.content == "A\u{a0}B")));
}

#[test]
fn disabled_button_never_receives_active_or_focus_style() {
    let out = import(
        "<button id='a' disabled>Disabled</button>",
        "button:disabled{background:#999999}button:active{background:#ff0000}button:focus{background:#00ff00}",
    )
    .unwrap();
    let doc = scene(&out).doc;
    assert_eq!(doc.node("a_bg").unwrap().fill, Color::from_hex("#999999"));
    for clip in &doc.animations {
        assert!(
            clip.tracks.iter().all(|t| t.keys.iter().all(|k| k.value != Value::Color(Color::rgb(255, 0, 0)) && k.value != Value::Color(Color::rgb(0, 255, 0))))
        );
    }
    assert!(out.warnings.iter().any(|w| w.contains("недоступность")));
}

#[test]
fn external_import_statement_is_skipped_without_swallowing_following_css() {
    let out = import("<button id='b' class='btn'>B</button>", "@import url('https://example.invalid/font.css'); .btn{background:red}").unwrap();
    let doc = scene(&out).doc;
    assert_eq!(doc.node("b_bg").unwrap().fill, Color::from_hex("#ff0000"));
    assert!(out.warnings.iter().any(|w| w.contains("@import")));
}
