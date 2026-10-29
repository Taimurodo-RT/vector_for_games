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
