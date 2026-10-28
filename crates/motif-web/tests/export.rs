use motif_web::{Asset, Config, Layer, Profile};

fn scene(src: &str) -> motif_script::Scene {
    motif_script::parse(src).unwrap_or_else(|e| panic!("{e:?}"))
}
const SIMPLE: &str = "let ink = #234567\nartboard \"test\" 24x24\nrect box at 12 12 size 16 r 3 fill ink stroke #ffffff 1.75\n";

#[test]
fn config_roundtrip_is_backward_compatible_and_preserves_boards() {
    let src = format!("{SIMPLE}\nartboard \"second\" 16x16\nellipse dot at 8 8 size 6 fill ink\n");
    let mut c = Config::default();
    c.tokens.insert("ink".into(), "currentColor".into());
    let written = motif_web::write(&src, &c).unwrap();
    assert_eq!(motif_web::read(&written).unwrap(), c);
    assert_eq!(motif_web::write(&written, &c).unwrap(), written);
    assert_eq!(motif_script::parse_boards(&written, None).unwrap().len(), 2);
}

#[test]
fn config_rejects_typos_injection_duplicate_headers_and_escape_paths() {
    for file in ["../bad.svg", "C:/bad.svg", "/bad.svg", "x\\bad.svg", "CON.svg", "bad/../x.svg", "x/aux.svg"] {
        let mut c = Config::default();
        c.assets.insert("test".into(), Asset { file: file.into(), ..Default::default() });
        assert!(motif_web::write(SIMPLE, &c).is_err(), "{file}");
    }
    assert!(motif_web::read("// @motif-web {\"toknes\":{}}\n").is_err());
    assert!(motif_web::read("// @motif-web {}\n// @motif-web {}\n").is_err());
    assert!(motif_web::read("artboard \"a\" 24x24\n// @motif-web {}\n").is_err());
    let mut c = Config::default();
    c.tokens.insert("ink".into(), "var(--ink);background:url(x)".into());
    assert!(motif_web::write(SIMPLE, &c).is_err());
}

#[test]
fn token_provenance_does_not_recolor_equal_literals() {
    let s = scene("let ink = #234567\nartboard \"test\" 24x24\nrect a at 6 12 size 8 fill ink\nrect b at 18 12 size 8 fill #234567\n");
    let mut c = Config::default();
    c.tokens.insert("ink".into(), "--ink".into());
    let svg = motif_web::render(&s, &c, None).unwrap();
    let doc = roxmltree::Document::parse(&svg.text).unwrap();
    let a = doc.descendants().find(|n| n.attribute("id") == Some("test-node-a")).unwrap();
    let b = doc.descendants().find(|n| n.attribute("id") == Some("test-node-b")).unwrap();
    assert_eq!(a.attribute("fill"), Some("var(--ink,#234567)"));
    assert_eq!(b.attribute("fill"), Some("#234567"));
}

#[test]
fn symbol_parameter_does_not_bind_to_same_named_global_palette_color() {
    let s = scene("let ink = #234567\nartboard \"test\" 24x24\nsymbol Box ink=#234567 {\nrect shape size 10 fill ink\n}\nuse Box box at 12 12\n");
    let mut c = Config::default();
    c.tokens.insert("ink".into(), "currentColor".into());
    let svg = motif_web::render(&s, &c, None).unwrap();
    assert!(!svg.text.contains("currentColor"));
}

#[test]
fn gradient_refs_and_theme_preview_preserve_literal_stops() {
    let s = scene("let ink = #234567\nartboard \"test\" 24x24\nrect a at 12 12 size 20 gradient linear 30 ink #234567\n");
    let mut c = Config::default();
    c.tokens.insert("ink".into(), "--ink".into());
    c.themes.insert("dark".into(), [("ink".into(), "#eeeeee".into())].into());
    let svg = motif_web::render(&s, &c, None).unwrap();
    assert_eq!(svg.text.matches("var(--ink,#234567)").count(), 1);
    let mut doc = s.doc.clone();
    motif_web::preview_theme(&s, &mut doc, &c, "dark");
    assert_eq!(doc.nodes[0].gradient.as_ref().unwrap().colors[0].to_hex(), "#eeeeee");
    assert_eq!(doc.nodes[0].gradient.as_ref().unwrap().colors[1].to_hex(), "#234567");
    assert_eq!(s.doc.nodes[0].gradient.as_ref().unwrap().colors[0].to_hex(), "#234567");
}

#[test]
fn hierarchy_classes_ids_and_clip_references_are_valid() {
    let s = scene("artboard \"test\" 24x24\ngroup parent at 12 12 clip {\nellipse mask size 20\ngroup child \"base\" {\nrect box size 24 fill #123456\n}\n}\n");
    let mut c = Config::default();
    let mut a = Asset::default();
    a.layers.insert("child".into(), Layer { id: "custom".into(), class: "base anim-body".into() });
    c.assets.insert("test".into(), a);
    let svg = motif_web::render(&s, &c, None).unwrap();
    let d = roxmltree::Document::parse(&svg.text).unwrap();
    let child = d.descendants().find(|n| n.attribute("id") == Some("test-node-custom")).unwrap();
    assert_eq!(child.attribute("class"), Some("base anim-body"));
    assert_eq!(child.parent().unwrap().attribute("id"), Some("test-node-parent"));
    assert!(d.descendants().any(|n| n.has_tag_name("clipPath")));
    let mask = d.descendants().find(|n| n.attribute("id") == Some("test-node-mask")).unwrap();
    assert!(mask.parent().unwrap().has_tag_name("clipPath"));
}

#[test]
fn name_and_path_collisions_stop_the_whole_package() {
    let scenes = motif_script::parse_boards("artboard \"a.b\" 16x16\nrect x size 4\nartboard \"a-b\" 16x16\nrect x size 4\n", None).unwrap();
    assert!(motif_web::build(&scenes, &Config::default(), None).is_err());
    let mut c = Config::default();
    c.assets.insert("a.b".into(), Asset { file: "icons/first.svg".into(), id: "first".into(), ..Default::default() });
    assert!(motif_web::build(&scenes, &c, None).is_ok());
}

#[test]
fn semantic_asset_id_can_have_separate_size_variants() {
    let scenes = motif_script::parse_boards("artboard \"small\" 16x16\nrect x size 4\nartboard \"large\" 24x24\nrect x size 4\n", None).unwrap();
    let mut c = Config::default();
    c.assets.insert("small".into(), Asset { id: "need.food".into(), variant: "16".into(), ..Default::default() });
    c.assets.insert("large".into(), Asset { id: "need.food".into(), variant: "24".into(), metadata: serde_json::json!({"slot":[1,2]}), ..Default::default() });
    let p = motif_web::build(&scenes, &c, None).unwrap();
    let m: serde_json::Value = serde_json::from_str(&p.files["graphics.json"]).unwrap();
    assert_eq!(m["assets"][1]["metadata"]["slot"], serde_json::json!([1, 2]));
    c.assets.get_mut("large").unwrap().variant = "16".into();
    assert!(motif_web::build(&scenes, &c, None).is_err());
}

#[test]
fn duplicate_fragment_prefixes_across_directories_are_rejected() {
    let scenes = motif_script::parse_boards("artboard \"a\" 16x16\nrect x size 4\nartboard \"b\" 16x16\nrect x size 4\n", None).unwrap();
    let mut c = Config::default();
    c.assets.insert("a".into(), Asset { file: "icons-a/b.svg".into(), ..Default::default() });
    c.assets.insert("b".into(), Asset { file: "icons/a-b.svg".into(), ..Default::default() });
    assert!(motif_web::build(&scenes, &c, None).is_err());
}

#[test]
fn unsupported_effects_fail_instead_of_disappearing() {
    let mut s = scene(SIMPLE);
    s.doc.nodes[0].noise = Some(motif_core::Noise::DEFAULT);
    assert!(motif_web::render(&s, &Config::default(), None).unwrap_err_or_else().contains("noise"));
}

// Small helper keeps the public Svg type free from Debug solely for tests.
trait ErrorText {
    fn unwrap_err_or_else(self) -> String;
}
impl<T> ErrorText for Result<T, String> {
    fn unwrap_err_or_else(self) -> String {
        match self {
            Err(e) => e,
            Ok(_) => panic!("expected an error"),
        }
    }
}

#[test]
fn profiles_and_slice_bounds_are_enforced() {
    let s = scene(SIMPLE);
    assert!(motif_web::render(&s, &Config::default(), Some(Profile::Icon16)).is_err());
    assert!(motif_web::render(&s, &Config::default(), Some(Profile::Icon24)).is_ok());
    let mut c = Config::default();
    c.assets.insert("test".into(), Asset { slices: Some([1.0, 20.0, 1.0, 20.0]), ..Default::default() });
    assert!(motif_web::render(&s, &c, None).is_err());
}

#[test]
fn css_animation_keeps_loop_endpoint_pivot_and_group_opacity() {
    let s = scene(
        "artboard \"test\" 24x24\ngroup parent opacity 0.5 {\nrect box at 12 12 size 10 fill #123456\n}\nanim Move 1s loop {\nbox.x 0: 12 | 1: 20\nparent.opacity 0: 0.5 | 1: 0.8\n}\n",
    );
    let p = motif_web::build(&[s], &Config::default(), None).unwrap();
    let css = &p.files["animations.css"];
    assert!(css.contains("100%{transform:matrix(1,0,0,1,20,12);fill-opacity:0.8;stroke-opacity:0.8;}"), "{css}");
    assert!(css.contains("prefers-reduced-motion"));
    let manifest: serde_json::Value = serde_json::from_str(&p.files["graphics.json"]).unwrap();
    assert_eq!(manifest["assets"][0]["anim"][0]["css"], true);
}

#[test]
fn non_css_motion_keeps_original_keys_and_is_reported() {
    let s = scene("artboard \"test\" 24x24\nrect box at 12 12 size 10\nanim Grow 1s {\nbox.width 0: 10 | 1: 20\n}\n");
    let p = motif_web::build(&[s], &Config::default(), None).unwrap();
    assert!(!p.warnings.is_empty());
    let m: serde_json::Value = serde_json::from_str(&p.files["graphics.json"]).unwrap();
    assert_eq!(m["assets"][0]["anim"][0]["css"], false);
    assert_eq!(m["assets"][0]["anim"][0]["tracks"][0]["keys"][1]["value"], 20.0);
}

#[test]
fn svg_is_self_contained_safe_xml_and_gallery_ids_are_unique() {
    let p = motif_web::build(&[scene(SIMPLE)], &Config::default(), None).unwrap();
    let svg = &p.files["test.svg"];
    let d = roxmltree::Document::parse(svg).unwrap();
    for n in d.descendants().filter(|n| n.is_element()) {
        assert!(!["script", "style", "text", "image", "foreignObject"].contains(&n.tag_name().name()));
        for a in n.attributes() {
            assert!(!a.name().starts_with("on"));
        }
    }
    assert!(d.root_element().attribute("width").is_none());
    let html = &p.files["index.html"];
    let ids: Vec<_> = html.split(" id=\"").skip(1).map(|s| s.split('"').next().unwrap()).collect();
    let unique: std::collections::HashSet<_> = ids.iter().collect();
    assert_eq!(ids.len(), unique.len());
}

#[test]
fn export_is_repeatable_and_writes_all_files() {
    let p = motif_web::build(&[scene(SIMPLE)], &Config::default(), None).unwrap();
    let dir = std::env::temp_dir().join(format!("motif-web-test-{}", std::process::id()));
    motif_web::export(&p, &dir).unwrap();
    motif_web::export(&p, &dir).unwrap();
    for (name, content) in &p.files {
        assert_eq!(std::fs::read_to_string(dir.join(name)).unwrap(), *content);
        std::fs::remove_file(dir.join(name)).unwrap();
    }
    std::fs::remove_dir(dir).unwrap();
}

fn compare_render(src: &str) {
    let s = scene(src);
    let svg = motif_web::render(&s, &Config::default(), None).unwrap();
    let tree = resvg::usvg::Tree::from_str(&svg.text, &resvg::usvg::Options::default()).unwrap();
    let mut actual = tiny_skia::Pixmap::new(s.doc.artboard.width as u32, s.doc.artboard.height as u32).unwrap();
    resvg::render(&tree, tiny_skia::Transform::identity(), &mut actual.as_mut());
    let expected = motif_render::render_cpu(&s.doc, motif_render::CpuOptions { scale: 1.0, antialias: true, background: false }).unwrap();
    let error: usize = actual.data().iter().zip(expected.data()).map(|(a, b)| a.abs_diff(*b) as usize).sum();
    let mean = error as f64 / actual.data().len() as f64;
    assert!(mean < 2.5, "SVG/native mean RGBA error {mean}: {}", svg.text);
}

#[test]
fn raster_equivalence_nested_transform_opacity_gradient_clip() {
    compare_render(
        "artboard \"test\" 64x64\ngroup g at 32 32 rot 15 opacity 0.55 {\nrect a at -5 0 size 30 r 3 fill #f07020 stroke #ffffee 2\nellipse b at 5 0 size 30 fill #2080cc\n}\n",
    );
    compare_render("artboard \"test\" 64x64\ngroup g at 32 32 clip {\nellipse mask size 50\nrect a size 60 gradient linear 35 #ff0000 #00ff00 #0000ff\n}\n");
    compare_render("artboard \"test\" 64x64\nrect a at 32 32 size 4 fill #ff0000 blur 12\n");
    compare_render("artboard \"test\" 64x64\nrect a at 32 32 size 4 scale 2 fill #ff0000 blur 12\n");
}
