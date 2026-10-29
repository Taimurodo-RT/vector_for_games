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
    let out = f.command().output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let scene = motif_script::parse(&String::from_utf8(out.stdout).unwrap()).unwrap();
    for id in ["tick", "radio"] {
        let item = motif_pixel::kit::render_piece(&scene.doc, id, Default::default()).unwrap();
        assert_eq!(item.base, "CheckBox");
        let checked = &item.pictures.iter().find(|p| p.name == "checked").unwrap().px;
        let unchecked = &item.pictures.iter().find(|p| p.name == "unchecked").unwrap().px;
        assert_eq!((checked.width(), checked.height()), (unchecked.width(), unchecked.height()), "{id}");
        assert!(checked.width() >= 18 && checked.height() >= 18, "the check mark must keep the whole icon frame: {id}");
        assert!(checked.pixels().iter().any(|p| p.alpha() > 0), "checked mark must be visible: {id}");
    }
}
