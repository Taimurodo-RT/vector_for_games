use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
static COUNTER: AtomicU64 = AtomicU64::new(0);
struct Fixture {
    root: PathBuf,
    html: PathBuf,
    css: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!("motif-html-{}-{}", std::process::id(), COUNTER.fetch_add(1, Ordering::Relaxed)));
        std::fs::create_dir_all(&root).unwrap();
        let html = root.join("кнопка.html");
        let css = root.join("button.css");
        std::fs::write(&html, "<button id='play'>Играть &amp; победить</button>").unwrap();
        std::fs::write(&css, "#play{padding:10px 22px;background:#3344aa}#play:hover{background:#6677dd}").unwrap();
        Self { root, html, css }
    }
    fn command(&self) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_motif"));
        command.arg("import-html").arg(&self.html).arg("--css").arg(&self.css);
        command
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        assert!(self.root.starts_with(std::env::temp_dir()));
        std::fs::remove_dir_all(&self.root).unwrap();
    }
}
#[test]
fn public_import_outputs_editable_msc_and_keeps_inputs_unchanged() {
    let f = Fixture::new();
    let html = std::fs::read(&f.html).unwrap();
    let css = std::fs::read(&f.css).unwrap();
    let out = f.command().output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    assert!(out.stderr.is_empty());
    let source = String::from_utf8(out.stdout).unwrap();
    let scene = motif_script::parse(&source).unwrap();
    assert_eq!(scene.doc.node("play").unwrap().ui_role, motif_core::UiRole::Button);
    assert!(scene.doc.animation("Hover").is_some());
    assert_eq!(std::fs::read(&f.html).unwrap(), html);
    assert_eq!(std::fs::read(&f.css).unwrap(), css);
    let path = f.root.join("nested/converted.msc");
    let out = f.command().arg("-o").arg(&path).output().unwrap();
    assert!(out.status.success());
    assert!(out.stdout.is_empty());
    assert_eq!(std::fs::read_to_string(path).unwrap(), source);
}
#[test]
fn warnings_are_on_stderr_and_output_cannot_replace_either_input() {
    let f = Fixture::new();
    std::fs::write(&f.css, "button{position:fixed;width:50vw}").unwrap();
    let original = std::fs::read(&f.html).unwrap();
    let out = f.command().output().unwrap();
    assert!(out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("50vw"));
    motif_script::parse(&String::from_utf8(out.stdout).unwrap()).unwrap();
    for path in [&f.html, &f.css] {
        let out = f.command().arg("-o").arg(path).output().unwrap();
        assert!(!out.status.success());
        assert!(String::from_utf8_lossy(&out.stderr).contains("не должен заменять"));
    }
    assert_eq!(std::fs::read(&f.html).unwrap(), original);
}

fn imported_with_browser(f: &Fixture, browser: Option<&str>) -> (String, motif_script::Scene) {
    let mut command = f.command();
    if let Some(browser) = browser {
        command.args(["--browser", browser]);
    }
    let result = command.output().unwrap();
    assert!(result.status.success(), "{}", String::from_utf8_lossy(&result.stderr));
    let source = String::from_utf8(result.stdout).unwrap();
    let scene = motif_script::parse(&source).unwrap();
    (source, scene)
}

#[test]
fn browser_profile_defaults_to_chrome_and_changes_real_ua_button_geometry() {
    let f = Fixture::new();
    std::fs::write(&f.html, "<button id='play'>Play</button>").unwrap();
    // Fix the authored font and border so the UA horizontal padding is isolated.
    std::fs::write(&f.css, "button{font-family:Arial;font-size:16px;border:0}").unwrap();
    let (default, _) = imported_with_browser(&f, None);
    let (chrome, chrome_scene) = imported_with_browser(&f, Some("chrome"));
    let (firefox, firefox_scene) = imported_with_browser(&f, Some("firefox"));
    assert_eq!(default, chrome, "the CLI default must match the explicit Chrome profile");
    assert_ne!(chrome, firefox);
    let chrome_width = chrome_scene.doc.node("play_bg").unwrap().width;
    let firefox_width = firefox_scene.doc.node("play_bg").unwrap().width;
    assert!((chrome_width - firefox_width - 4.0).abs() < 1e-5, "UA padding must affect editable geometry: Chrome={chrome_width}, Firefox={firefox_width}");
    assert_eq!(chrome_scene.doc.node("play").unwrap().ui_role, motif_core::UiRole::Button);
    assert_eq!(firefox_scene.doc.node("play").unwrap().ui_role, motif_core::UiRole::Button);
}

#[test]
fn authored_styles_override_both_browser_profiles_in_normal_and_state_animations() {
    let f = Fixture::new();
    std::fs::write(&f.html, "<button id='play'>Play</button>").unwrap();
    std::fs::write(
        &f.css,
        concat!(
            "button{width:180px;height:40px;box-sizing:border-box;padding:4px 10px;margin:0;border:2px solid #123456;border-radius:0;",
            "font-family:Arial;font-size:16px;font-weight:400;line-height:20px;color:#ffffff;background:#223344;transition:.2s linear}",
            "button:hover{padding:6px 12px;background:#667788}button:active{opacity:.5}"
        ),
    )
    .unwrap();
    let (_, chrome) = imported_with_browser(&f, Some("chrome"));
    let (_, firefox) = imported_with_browser(&f, Some("firefox"));
    assert!(chrome.doc == firefox.doc, "explicit authored styles must override all differing UA defaults");
    for name in ["Normal", "Hover", "Press"] {
        let a = chrome.doc.animation(name).unwrap();
        let b = firefox.doc.animation(name).unwrap();
        for time in [0.0, 0.1, 0.2] {
            assert!(a.pose(&chrome.doc, time) == b.pose(&firefox.doc, time), "the selected browser profile must be used consistently in {name}@{time}");
        }
    }
}

#[test]
fn invalid_browser_profiles_and_unrelated_commands_fail_before_reading_or_writing_files() {
    let f = Fixture::new();
    let output = f.root.join("keep.msc");
    std::fs::write(&output, "previous scene").unwrap();
    for browser in ["safari", "", "Chrome"] {
        let result = f.command().args(["--browser", browser, "-o"]).arg(&output).output().unwrap();
        assert!(!result.status.success());
        assert!(String::from_utf8_lossy(&result.stderr).contains("неизвестный профиль браузера"));
        assert_eq!(std::fs::read_to_string(&output).unwrap(), "previous scene");
    }
    let result = f.command().arg("--browser").output().unwrap();
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("после --browser"));
    for command in ["check", "render", "inspect", "web"] {
        let result =
            Command::new(env!("CARGO_BIN_EXE_motif")).arg(command).arg(f.root.join("does-not-exist.msc")).args(["--browser", "firefox"]).output().unwrap();
        assert!(!result.status.success());
        assert!(
            String::from_utf8_lossy(&result.stderr).contains("--browser доступен только для import-html"),
            "{command}: {}",
            String::from_utf8_lossy(&result.stderr)
        );
        assert!(result.stdout.is_empty());
    }
}

#[test]
fn indented_document_imports_text_and_controls_through_the_public_cli() {
    let f = Fixture::new();
    let html = concat!(
        "<!doctype html>\r\n<html lang='ru'>\r\n\t<head>\r\n",
        "\t\t<meta charset='utf-8'><title>Metadata must stay hidden</title>\r\n",
        "\t\t<style>h4 { color: #123456; }</style>\r\n\t</head>\r\n\t<body>\r\n",
        "\t\t<h4 id='heading'>Настройки &amp; профиль</h4>\r\n",
        "\t\t<p>Обычный <strong>жирный</strong><br>Следующая строка</p>\r\n",
        "\t\t<pre>let x = \"quoted\";\n\tpath\\n</pre>\r\n",
        "\t\t<hr><input id='name' type='text' placeholder='Имя'>\r\n",
        "\t\t<input id='check' type='checkbox' checked>\r\n",
        "\t\t<input id='volume' type='range' min='0' max='100' value='40'>\r\n",
        "\t\t<select id='choice'><option>Первый</option><option selected>Второй</option></select>\r\n",
        "\t</body>\r\n</html>\r\n"
    );
    std::fs::write(&f.html, html).unwrap();
    std::fs::write(&f.css, "body {\r\n\tpadding: 8px;\r\n}\r\n").unwrap();
    let out = f.command().output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let scene = motif_script::parse(&String::from_utf8(out.stdout).unwrap()).unwrap();
    let doc = &scene.doc;
    for (id, role) in [
        ("name", motif_core::UiRole::Input),
        ("check", motif_core::UiRole::Check),
        ("volume", motif_core::UiRole::Slider),
        ("choice", motif_core::UiRole::Option),
    ] {
        assert_eq!(doc.node(id).unwrap().ui_role, role, "{id}");
    }
    let texts: Vec<_> = doc.nodes.iter().filter_map(|n| n.text.as_ref().map(|t| t.content.as_str())).collect();
    assert!(texts.iter().any(|t| t.contains("Настройки & профиль")), "{texts:?}");
    assert!(texts.iter().any(|t| t.contains("жирный")), "{texts:?}");
    assert!(texts.iter().any(|t| t.contains("Следующая строка")), "{texts:?}");
    assert!(texts.iter().any(|t| t.contains("let x = \"quoted\";")), "{texts:?}");
    assert!(texts.iter().any(|t| t.contains("path\\n")), "literal backslash-n must survive: {texts:?}");
    assert!(!texts.iter().any(|t| t.contains("Metadata must stay hidden")), "{texts:?}");
    assert_eq!(std::fs::read_to_string(&f.html).unwrap(), html);
}

#[test]
fn imported_toggle_icons_keep_a_stable_frame_in_the_existing_ui_exporter() {
    let f = Fixture::new();
    std::fs::write(&f.html, "<input id='tick' type='checkbox' checked><input id='radio' type='radio'>").unwrap();
    std::fs::write(&f.css, "").unwrap();
    for browser in ["chrome", "firefox"] {
        let (_, scene) = imported_with_browser(&f, Some(browser));
        for id in ["tick", "radio"] {
            let item = motif_pixel::kit::render_piece(&scene.doc, id, Default::default()).unwrap();
            assert_eq!(item.base, "CheckBox");
            let checked = &item.pictures.iter().find(|p| p.name == "checked").unwrap().px;
            let unchecked = &item.pictures.iter().find(|p| p.name == "unchecked").unwrap().px;
            let background = scene.doc.node(&format!("{id}_bg")).unwrap();
            let frame_width = (background.width + background.stroke_width).ceil() as u32;
            let frame_height = (background.height + background.stroke_width).ceil() as u32;
            assert_eq!((checked.width(), checked.height()), (unchecked.width(), unchecked.height()), "{browser}: {id}");
            assert!(
                checked.width() >= frame_width && checked.height() >= frame_height,
                "the check mark must keep the complete imported UA icon frame: {browser}: {id}"
            );
            assert!(checked.pixels().iter().any(|p| p.alpha() > 0), "checked mark must be visible: {browser}: {id}");
        }
    }
}

#[test]
fn positioned_css_component_imports_and_exports_its_combined_animation() {
    let f = Fixture::new();
    let html = include_str!("../../../examples/import/uiverse-play.html");
    let css = include_str!("../../../examples/import/uiverse-play.css");
    std::fs::write(&f.html, html).unwrap();
    std::fs::write(&f.css, css).unwrap();
    let scene_path = f.root.join("play.msc");
    let out = f.command().arg("-o").arg(&scene_path).output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let scene = motif_script::parse(&std::fs::read_to_string(&scene_path).unwrap()).unwrap();
    assert!(scene.doc.animation("Hover").is_some());
    assert!(scene.doc.animation("HoverLoop").unwrap().looping);
    assert!(scene.doc.nodes.iter().any(|node| node.clip));
    let image_path = f.root.join("loop.png");
    let out = Command::new(env!("CARGO_BIN_EXE_motif"))
        .arg("render")
        .arg(&scene_path)
        .args(["--anim", "HoverLoop", "--time", "0.3", "-o"])
        .arg(&image_path)
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(&std::fs::read(image_path).unwrap()[..8], b"\x89PNG\r\n\x1a\n");
    assert_eq!(std::fs::read_to_string(&f.html).unwrap(), html);
    assert_eq!(std::fs::read_to_string(&f.css).unwrap(), css);
}
