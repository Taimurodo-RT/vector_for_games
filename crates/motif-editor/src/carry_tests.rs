//! The scene as one file, and a symbol saved as a file of its own.

use super::editing_tests::fixture;
use super::*;

const KIT: &str = "let ink = #1b1d24
let spare = #00ff00
symbol Button face=#ff5a36 {
  rect body size 120 40 fill face stroke ink 2 r 8
}
";

const SCREEN: &str = "motif 7
include \"kit.msc\"
let own = #3b6cff
symbol Row {
  use Button go at 0 0
  rect line at 0 30 size 120 2 fill own
}
artboard \"Экран\" 400x300
use Row r1 at 200 150
";

fn open(test: &str) -> (App, std::path::PathBuf) {
    let dir = std::env::temp_dir().join(format!("motif-editor-carry-{}-{test}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("kit.msc"), KIT).unwrap();
    std::fs::write(dir.join("screen.msc"), SCREEN).unwrap();
    let mut app = fixture("");
    app.path = Some(dir.join("screen.msc"));
    app.set_file(SCREEN, 0);
    app.saved_src = SCREEN.into();
    app.reparse();
    assert!(app.errors.is_empty(), "{:?}", app.errors);
    (app, dir)
}

#[test]
fn the_scene_is_written_as_one_file_and_stays_as_it_was() {
    let (mut app, dir) = open("bundle");
    let out = dir.join("given/one.msc");
    std::fs::create_dir_all(out.parent().unwrap()).unwrap();
    app.bundle_to(&out);
    assert!(app.status.contains("Сцена записана одним файлом") && app.status.contains("kit.msc"), "{}", app.status);
    let text = std::fs::read_to_string(&out).unwrap();
    // It reads in a folder that has no kit, and the open scene is the same.
    assert!(!text.contains("include \"") && motif_script::parse_in(&text, out.parent()).is_ok(), "{text}");
    assert_eq!((app.file_text().as_str(), app.undo.len()), (SCREEN, 0));
    // Over the scene itself nothing is written.
    app.bundle_to(&dir.join("screen.msc"));
    assert!(app.status.contains("Это сама сцена"), "{}", app.status);
    assert_eq!(std::fs::read_to_string(dir.join("screen.msc")).unwrap(), SCREEN);
}

#[test]
fn a_symbol_is_saved_with_what_it_needs() {
    let (mut app, dir) = open("extract");
    let out = dir.join("other/row.msc");
    std::fs::create_dir_all(out.parent().unwrap()).unwrap();
    // A symbol of the scene takes the kit's button and the colors both use.
    app.extract_to(None, "Row", &out);
    assert!(app.status.contains("Символ Row сохранён") && app.status.contains("символы Button") && app.status.contains("ink, own"), "{}", app.status);
    let text = std::fs::read_to_string(&out).unwrap();
    assert!(!text.contains("spare") && !text.contains("include \""), "{text}");
    let lib = motif_script::library_file(&out);
    assert_eq!(lib.symbols.iter().map(|s| s.name.as_str()).collect::<Vec<_>>(), ["Button", "Row"]);
    // A symbol of a library file is taken from that file.
    let button = dir.join("other/button.msc");
    app.extract_to(Some(&dir.join("kit.msc")), "Button", &button);
    assert!(app.status.contains("Символ Button сохранён"), "{}", app.status);
    assert!(motif_script::parse_in(&std::fs::read_to_string(&button).unwrap(), None).is_ok());
    // Not over the file the symbol is written in, and not a symbol that is not there.
    app.extract_to(Some(&dir.join("kit.msc")), "Button", &dir.join("kit.msc"));
    assert!(app.status.contains("под другим именем"), "{}", app.status);
    assert_eq!(std::fs::read_to_string(dir.join("kit.msc")).unwrap(), KIT);
    app.extract_to(None, "Nope", &dir.join("other/nope.msc"));
    assert!(app.status.contains("не сохранён") && !dir.join("other/nope.msc").exists(), "{}", app.status);
    assert_eq!(app.file_text(), SCREEN);
}

#[test]
fn another_file_opens_without_the_steps_back_of_the_first() {
    let (mut app, dir) = open("open");
    app.edit("r1", Prop::X, Value::Num(120.0));
    assert_eq!(app.undo.len(), 1);
    app.save(false);
    // «Открыть» of the kit: Ctrl+Z there must not bring the screen under the kit's name.
    app.open_path(dir.join("kit.msc")).unwrap();
    assert_eq!((app.undo.len(), app.file_text().as_str()), (0, KIT));
    app.undo();
    assert_eq!(app.file_text(), KIT);
    assert!(!app.dirty());
}
