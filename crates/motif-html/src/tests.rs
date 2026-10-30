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
        ".primary { box-sizing: content-box; width: 120px; height: 32px; padding: 4px 10px; border: 2px solid #123456; border-radius: 8px; background: #2468ab; color: white; }",
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
    assert_eq!(doc.animation("Hover").unwrap().tracks[0].prop, Prop::Fill);
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
    let untransformed = import("<button id='b'>Safe</button>", "button{width:50vw}").unwrap();
    assert!((doc.node("b").unwrap().x - scene(&untransformed).doc.node("b").unwrap().x - 4.0).abs() < 1e-9);
    for needle in ["script", "link", "onclick", "@media", "::before", "50vw", "url(", "unknown"] {
        assert!(out.warnings.iter().any(|w| w.contains(needle)), "missing {needle}: {:?}", out.warnings);
    }
    assert!(import("<button>X</button>", "button{color:red").is_err());
}

#[test]
fn text_is_visible_and_gradient_shadow_alpha_render_through_the_existing_renderer() {
    let out = import(
        "<button id='b'>Hello</button>",
        "button {background:linear-gradient(90deg,#123456,#abcdef);box-shadow:0 3px 8px rgba(0,0,0,.3);color:rgb(255,255,255);font-weight:700;font-size:16px}",
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
fn inline_text_retains_boundary_spaces_and_paragraph_baseline_and_br_creates_a_new_line() {
    for tag in ["button", "p"] {
        let out = import(&format!("<{tag} id='p'>Hello <b>world</b> again</{tag}>"), "").unwrap();
        let doc = scene(&out).doc;
        let texts: Vec<_> = doc.nodes.iter().filter(|n| n.text.is_some()).collect();
        let contents: Vec<_> = texts.iter().map(|n| n.text.as_ref().unwrap().content.as_str()).collect();
        assert_eq!(contents, vec!["Hello ", "world", " again"]);
        let ys: Vec<_> = texts.iter().map(|n| (doc.world_transform(doc.index_of(&n.id).unwrap()) * motif_core::kurbo::Point::ZERO).y).collect();
        assert!(ys.iter().all(|y| (y - ys[0]).abs() < 0.1), "{ys:?}");
        assert!(out.warnings.iter().all(|w| tag == "button" && w.contains("составная подпись")), "{:?}", out.warnings);
    }
    let out = import("<button>Line one<br>Line two</button>", "").unwrap();
    let doc = scene(&out).doc;
    let texts: Vec<_> =
        doc.nodes.iter().enumerate().filter(|(_, n)| n.text.is_some()).map(|(i, _)| doc.world_transform(i) * motif_core::kurbo::Point::ZERO).collect();
    assert!((texts[1].y - texts[0].y - 16.0).abs() < 0.1, "{texts:?}");
    assert!(out.warnings.iter().all(|w| w.contains("составная подпись")), "{:?}", out.warnings);
    let out = import("<button>A&nbsp;B</button>", "").unwrap();
    assert!(scene(&out).doc.nodes.iter().any(|n| n.text.as_ref().is_some_and(|t| t.content == "A\u{a0}B")));
}

fn visible_text_positions(doc: &motif_core::Document) -> Vec<(String, f64, f64)> {
    doc.nodes
        .iter()
        .enumerate()
        .filter_map(|(i, n)| {
            n.text.as_ref().map(|t| {
                let point = doc.world_transform(i) * motif_core::kurbo::Point::ZERO;
                (t.content.clone(), point.x, point.y)
            })
        })
        .collect()
}

#[test]
fn complete_tab_indented_document_has_the_same_text_and_positions_as_compact_markup() {
    let compact = "<html><body><div id='box'><p id='a'>A <b>B</b> C</p><p id='b'><b>D</b> <i>E</i><br>F</p><button id='go'>Go</button></div></body></html>";
    let formatted = concat!(
        "<!doctype html>\r\n<html>\r\n\t<head><meta charset='utf-8'><title>Hidden title</title>",
        "<!-- metadata --><style>#box{padding:4px}</style></head>\r\n\t<body>\r\n\t\t<div id='box'>\r\n",
        "\t\t\t<p id='a'>A <b>B</b> C</p>\r\n\t\t\t<p id='b'><b>D</b> <i>E</i><br>\r\nF</p>\r\n",
        "\t\t\t<button id='go'>Go</button>\r\n\t\t</div>\r\n\t</body>\r\n</html>"
    );
    let a = scene(&import(compact, "#box{padding:4px}").unwrap()).doc;
    let b = scene(&import(formatted, "").unwrap()).doc;
    assert_eq!(visible_text_positions(&a), visible_text_positions(&b));
    assert_eq!(a.artboard.width, b.artboard.width);
    assert_eq!(a.artboard.height, b.artboard.height);
    for id in ["box", "a", "b", "go"] {
        assert_eq!(a.world_transform(a.index_of(id).unwrap()), b.world_transform(b.index_of(id).unwrap()));
    }
    let contents: String = a.nodes.iter().filter_map(|n| n.text.as_ref().map(|t| t.content.as_str())).collect();
    assert_eq!(contents, "A B CD EFGo");
}

#[test]
fn spaces_between_styled_inline_nodes_are_single_real_layout_spaces() {
    let out = import("<p><b>A</b> \n\t <i>B</i> <small>C</small></p>", "").unwrap();
    let doc = scene(&out).doc;
    let text = visible_text_positions(&doc);
    assert_eq!(text.iter().map(|t| t.0.as_str()).collect::<Vec<_>>(), ["A", " ", "B", " ", "C"]);
    assert!(text.windows(2).all(|pair| pair[1].1 > pair[0].1), "{text:?}");
    assert!(!doc.nodes.iter().any(|n| n.text.as_ref().is_some_and(|t| t.content.contains('\t') || t.content.contains('\n'))));
}

#[test]
fn br_empty_lines_pre_tabs_and_literal_escapes_survive_as_editable_text() {
    let out = import("<p>A<br>\nB<br><br>C</p><pre id='code'>\n a\n\tb  c \\n &quot;q&quot; </pre><p> a\n\tb  c </p>", "pre{tab-size:4}").unwrap();
    let doc = scene(&out).doc;
    let texts = visible_text_positions(&doc);
    assert_eq!(texts[0].0, "A");
    assert_eq!(texts[1].0, "B");
    assert_eq!(texts[2].0, "C");
    assert!((texts[1].2 - texts[0].2 - 19.2).abs() < 0.1);
    assert!((texts[2].2 - texts[1].2 - 38.4).abs() < 0.1);
    assert_eq!(texts[3].0, " a\n    b  c \\n \"q\" ");
    assert_eq!(texts[4].0, "a b c");
    assert!(doc.nodes.iter().any(|n| n.text.as_ref().is_some_and(|t| t.font.as_ref().is_some_and(|f| f.name == "LiberationMono-Regular.ttf"))));
    assert!(!out.warnings.iter().any(|w| w.contains("<br>")));
}

#[test]
fn heading_inline_faces_decorations_color_and_subscripts_remain_editable() {
    let out = import("<h4>Title</h4><h5>Smaller</h5><h6>Tiny</h6><p><span id='u'><u>under</u></span> <s>old</s> <em>ital</em> H<sub>2</sub> O<sup>+</sup></p><a id='link' href='https://example.invalid'>Visit</a>", "#u{color:red}#u:hover{color:blue}").unwrap();
    let doc = scene(&out).doc;
    let under = doc.nodes.iter().find(|n| n.text.as_ref().is_some_and(|t| t.content == "under")).unwrap();
    assert_eq!(under.fill, Color::from_hex("#ff0000"));
    assert!(doc.nodes.iter().any(|n| n.id.contains("_underline_") && n.fill == Color::from_hex("#ff0000")));
    assert!(doc.nodes.iter().any(|n| n.id.contains("_strike_")));
    let italic = doc.nodes.iter().find(|n| n.text.as_ref().is_some_and(|t| t.content == "ital")).unwrap();
    assert_eq!(italic.skew_x, -12.0);
    let hover = doc.animation("Hover").unwrap().pose(&doc, 0.2);
    for node in doc.nodes.iter().filter(|n| n.id.contains("_underline_") && n.fill == Color::from_hex("#ff0000")) {
        assert_eq!(hover.node(&node.id).unwrap().fill, Color::from_hex("#0000ff"));
    }
    assert_eq!(doc.node("link").unwrap().ui_role, UiRole::Auto);
    assert!(out.warnings.iter().any(|w| w.contains("href")));
    assert!(!out.warnings.iter().any(|w| w.contains("импортируется как обычная группа")));
    let h = doc.nodes.iter().find(|n| n.text.as_ref().is_some_and(|t| t.content.trim() == "H")).unwrap();
    let sub = doc.nodes.iter().find(|n| n.text.as_ref().is_some_and(|t| t.content == "2")).unwrap();
    let hy = (doc.world_transform(doc.index_of(&h.id).unwrap()) * motif_core::kurbo::Point::ZERO).y;
    let sy = (doc.world_transform(doc.index_of(&sub.id).unwrap()) * motif_core::kurbo::Point::ZERO).y;
    assert!(sy > hy);
    assert!(sub.get(Prop::FontSize).as_num().unwrap() < h.get(Prop::FontSize).as_num().unwrap());
}

#[test]
fn flex_indentation_does_not_create_anonymous_items_or_extra_gaps() {
    let compact = "<div id='row'><label id='label'>Имя</label><input id='name' placeholder='Имя'><button id='go'>Go</button></div>";
    let formatted = "<div id='row'>\r\n\t<label id='label'>Имя</label>\r\n\t<input id='name' placeholder='Имя'>\r\n\t<button id='go'>Go</button>\r\n</div>";
    let css = "#row{display:flex;gap:10px;align-items:center}";
    let a = scene(&import(compact, css).unwrap()).doc;
    let b = scene(&import(formatted, css).unwrap()).doc;
    assert_eq!(visible_text_positions(&a), visible_text_positions(&b));
    assert_eq!(a.artboard.width, b.artboard.width);
    assert_eq!(a.artboard.height, b.artboard.height);
    for id in ["row", "label", "name", "go"] {
        assert_eq!(a.world_transform(a.index_of(id).unwrap()), b.world_transform(b.index_of(id).unwrap()));
    }
    assert!(!a.nodes.iter().any(|n| n.text.as_ref().is_some_and(|t| t.content.trim().is_empty())));
}

#[test]
fn generated_control_and_multiline_decoration_ids_cannot_collide_with_explicit_ids() {
    let out =
        import("<input id='x' type='checkbox' checked><div id='x_mark'>Safe</div><pre id='code'><u>A\nB</u></pre><p id='text_6_underline_1'>Also safe</p>", "")
            .unwrap();
    let doc = scene(&out).doc;
    let ids: BTreeSet<_> = doc.nodes.iter().map(|n| &n.id).collect();
    assert_eq!(ids.len(), doc.nodes.len());
    assert!(doc.node("x_mark").is_some());
    assert!(doc.node("x_mark_2").is_some());
}

#[test]
fn lists_preserve_markers_numbers_explicit_values_and_nested_indent() {
    let out = import(
        "<ul id='list'><li>First<ul id='nested'><li>Nested</li></ul></li><li>Second</li></ul><ol start='3'><li>Third</li><li value='7'>Seventh</li></ol>",
        "",
    )
    .unwrap();
    let doc = scene(&out).doc;
    let texts: Vec<_> = doc.nodes.iter().filter_map(|n| n.text.as_ref().map(|t| t.content.as_str())).collect();
    assert_eq!(texts, ["• ", "First", "• ", "Nested", "• ", "Second", "3. ", "Third", "7. ", "Seventh"]);
    let list = doc.world_transform(doc.index_of("list").unwrap()) * motif_core::kurbo::Point::ZERO;
    let nested = doc.world_transform(doc.index_of("nested").unwrap()) * motif_core::kurbo::Point::ZERO;
    assert!(nested.x > list.x);
    assert!(out.warnings.is_empty(), "{:?}", out.warnings);
}

#[test]
fn basic_form_controls_get_native_roles_parts_initial_values_and_explicit_behavior_warnings() {
    let out = import(
        concat!(
            "<input id='name' value='A &amp; B'><input id='hint' placeholder='Name'><input id='pwd' type='password' value='secret'>",
            "<input id='yes' type='checkbox' checked><input id='no' type='checkbox'><input id='r' type='radio' name='quality' checked>",
            "<input id='volume' type='range' min='10' max='30' value='15'><input id='send' type='submit' value='Send'>",
            "<textarea id='note'>\nLine 1\nLine 2</textarea><select id='choice'><option>First</option><option selected>Second</option></select>",
            "<input id='hidden' type='hidden' value='secret-hidden'><hr id='rule'>"
        ),
        "input:focus{color:blue}",
    )
    .unwrap();
    let doc = scene(&out).doc;
    for (id, role) in [
        ("name", UiRole::Input),
        ("hint", UiRole::Input),
        ("pwd", UiRole::Input),
        ("yes", UiRole::Check),
        ("r", UiRole::Check),
        ("volume", UiRole::Slider),
        ("send", UiRole::Button),
        ("note", UiRole::Input),
        ("choice", UiRole::Option),
    ] {
        assert_eq!(doc.node(id).unwrap().ui_role, role, "{id}");
    }
    assert!(doc.node("yes_checked").unwrap().visible);
    assert!(!doc.node("no_checked").unwrap().visible);
    assert_eq!(doc.node("r").unwrap().radio.as_deref(), Some("html_radio_quality"));
    assert_eq!(doc.node("volume").unwrap().dial.value, 0.25);
    let check = motif_core::UiCheck::new(&doc);
    assert!((check.control("volume").unwrap().value.unwrap() - 0.25).abs() < 0.0001);
    assert_eq!(check.control("yes").unwrap().checked, Some(true));
    assert_eq!(check.control("no").unwrap().checked, Some(false));
    for (id, part) in [
        ("volume_track", motif_core::UiPart::Track),
        ("volume_fill", motif_core::UiPart::Fill),
        ("volume_grabber", motif_core::UiPart::Grabber),
        ("choice_arrow", motif_core::UiPart::Arrow),
    ] {
        assert_eq!(doc.node(id).unwrap().part, Some(part));
    }
    let texts: Vec<_> = doc.nodes.iter().filter_map(|n| n.text.as_ref().map(|t| t.content.as_str())).collect();
    for text in ["A & B", "Name", "••••••", "Send", "Line 1\nLine 2", "Second"] {
        assert!(texts.contains(&text), "{texts:?}");
    }
    assert!(!texts.contains(&"First") && !texts.contains(&"secret") && !texts.contains(&"secret-hidden"));
    assert!(doc.node("hidden").is_none());
    assert_eq!(doc.node("rule_bg").unwrap().height, 1.0);
    assert!(out.warnings.iter().any(|w| w.contains("input") && w.contains("игровом контроле")));
    assert!(out.warnings.iter().any(|w| w.contains("select") && w.contains("раскрытие")));
    assert!(out.warnings.iter().any(|w| w.contains("textarea") && w.contains("многострочный")));
}

#[test]
fn empty_input_keeps_an_editable_caption_part_and_check_parts_keep_full_bounds() {
    let out = import("<input id='empty'><input id='check' type='checkbox' checked><input id='radio' type='radio'>", "").unwrap();
    let doc = scene(&out).doc;
    let empty = doc.index_of("empty").unwrap();
    assert!(
        doc.nodes.iter().any(|n| n.parent == Some(empty) && n.text.as_ref().is_some_and(|t| t.content.is_empty()) && n.part == Some(motif_core::UiPart::Text))
    );
    let frame = doc.node("empty_bg").unwrap();
    assert!((frame.height + frame.stroke_width - 21.0).abs() < 0.1);
    for id in ["check", "radio"] {
        let off = doc.node(&format!("{id}_unchecked")).unwrap().path().unwrap().bounding_box();
        let on = doc.node(&format!("{id}_checked_bounds")).unwrap().path().unwrap().bounding_box();
        assert_eq!((off.width(), off.height()), (on.width(), on.height()));
    }
}

#[test]
fn multiline_nested_inline_and_unknown_css_are_reported_without_discarding_content() {
    let out = import("<p><span>A<br>B</span>C</p>", "p{white-space:normal;unsupported-feature:1}").unwrap();
    let doc = scene(&out).doc;
    assert_eq!(doc.nodes.iter().filter_map(|n| n.text.as_ref().map(|t| t.content.as_str())).collect::<String>(), "ABC");
    assert!(out.warnings.iter().any(|w| w.contains("многострочный вложенный inline")));
    assert!(out.warnings.iter().any(|w| w.contains("unsupported-feature")));
    let select = import("<p>A<select>\n\t<option>First</option>\n\t<option selected>Second</option>\n</select></p>", "").unwrap();
    assert!(!select.warnings.iter().any(|w| w.contains("многострочный вложенный inline")), "{:?}", select.warnings);
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
