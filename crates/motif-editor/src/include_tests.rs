//! A scene that includes another file: what the file gives is used here and changed there.

use super::editing_tests::fixture;
use super::snap_tests::frame;
use super::*;
use crate::variant_ui::VariantAct;
use crate::workspace::Workspace;

const KIT: &str = "let ink = #1b1d24
let paper = #fff4e4
let sky = linear 90 #6fb8ff #ffffff
style card fill paper stroke ink 2 r 10
theme night {
  paper = #20222b
}
symbol Button face=#ff5a36 {
  variant quiet face=#8a8f9c
  rect body size 120 40 fill face stroke ink 2 r 8
}
artboard \"Набор\" 300x200
use Button sample at 150 100
";

const SCREEN: &str = "include \"kit.msc\"
let own = #3b6cff
artboard \"Экран\" 400x300 bg paper
rect panel at 200 150 size 300 200 style card
rect mark at 60 60 size 20 20 fill own
use Button ok at 200 220
";

/// A folder with the kit and the screen, and the screen open from it.
fn open(test: &str, screen: &str) -> (App, std::path::PathBuf) {
    let dir = std::env::temp_dir().join(format!("motif-editor-include-{}-{test}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("kit.msc"), KIT).unwrap();
    std::fs::write(dir.join("screen.msc"), screen).unwrap();
    let mut app = fixture("");
    app.path = Some(dir.join("screen.msc"));
    app.set_file(screen, 0);
    app.saved_src = screen.into();
    app.reparse();
    assert!(app.errors.is_empty(), "{:?}", app.errors);
    (app, dir)
}

fn fill(app: &App, id: &str) -> String {
    app.scene.doc.node(id).unwrap().fill.unwrap().to_hex()
}

#[test]
fn what_an_included_file_gives_is_not_changed_from_the_scene() {
    let (mut app, dir) = open("refused", SCREEN);
    // The panel of the artboard lists the file, and the panels of what it gives are drawn.
    frame(&mut app, vec![], Modifiers::NONE);
    assert_eq!((app.symbol_from("Button").as_deref(), app.style_from("card").as_deref()), (Some("kit.msc"), Some("kit.msc")));
    assert_eq!((app.swatch_from("ink").as_deref(), app.swatch_from("sky").as_deref(), app.swatch_from("own")), (Some("kit.msc"), Some("kit.msc"), None));
    let refused = |app: &App| app.src == SCREEN && app.status.contains("в подключённом файле «kit.msc»");
    // Colors and gradients of the palette.
    app.palette_act(crate::palette_ui::Act::Set("ink".into(), Color::rgb(255, 0, 0)));
    assert!(refused(&app), "{}", app.status);
    app.palette_act(crate::palette_ui::Act::Rename("sky".into(), "dawn".into()));
    assert!(refused(&app), "{}", app.status);
    // Styles.
    app.style_act(crate::named_style_ui::Act::Rename("card".into(), "sheet".into()), &[]);
    assert!(refused(&app), "{}", app.status);
    app.style_act(crate::named_style_ui::Act::Delete("card".into()), &[]);
    assert!(refused(&app), "{}", app.status);
    app.style_act(crate::named_style_ui::Act::WriteInto, &["panel".to_string()]);
    assert!(refused(&app), "{}", app.status);
    // The variants of a symbol; a copy still takes one.
    app.variant_act("ok", VariantAct::New);
    assert!(refused(&app), "{}", app.status);
    app.detach_copy("ok");
    assert!(refused(&app), "{}", app.status);
    app.variant_act("ok", VariantAct::Take(Some("quiet".into())));
    assert_eq!(fill(&app, "ok-body"), "#8a8f9c");
    app.undo();
    // An edit that would write at the `include` line, whatever made it, does not get through.
    app.status.clear();
    let out = motif_script::set_swatch(&app.src, &app.scene, "paper", Color::rgb(0, 0, 0));
    assert!(out.is_some() && !app.apply("swatch-paper", out));
    assert!(app.src == SCREEN && app.status.contains("записано в подключённом файле «kit.msc»"), "{}", app.status);
    // The scene's own names are changed as ever, and new names avoid the names of the file.
    app.palette_act(crate::palette_ui::Act::Set("own".into(), Color::rgb(255, 0, 0)));
    assert_eq!(fill(&app, "mark"), "#ff0000");
    std::fs::write(dir.join("kit.msc"), KIT.replace("let sky", "let color1 = #00ff00\nlet sky")).unwrap();
    app.reparse();
    app.palette_act(crate::palette_ui::Act::Add);
    assert!(app.src.contains("let color2 = ") && !app.src.contains("let color1"), "{}", app.src);
    assert_eq!(std::fs::read_to_string(dir.join("kit.msc")).unwrap().matches("color1").count(), 1);
}

#[test]
fn inside_a_copy_of_an_included_symbol_edits_stay_with_the_copy() {
    let (mut app, dir) = open("copy", SCREEN);
    app.set_workspace(Workspace::Drawing);
    app.edit_symbol("ok");
    frame(&mut app, vec![], Modifiers::NONE);
    // The symbol is not open for edits: its lines are not in this file.
    assert_eq!((app.symbol_copy(), app.scene.editing.clone()), (None, None));
    assert!(app.status.contains("символ Button записан в подключённом файле «kit.msc»"), "{}", app.status);
    app.select_only(Some("ok-body".into()));
    app.edit("ok-body", Prop::Width, Value::Num(150.0));
    assert!(app.src.contains("override {\n  ok-body.w = 150\n}"), "{}", app.src);
    assert_eq!(app.scene.doc.node("ok-body").unwrap().width, 150.0);
    assert_eq!(std::fs::read_to_string(dir.join("kit.msc")).unwrap(), KIT);
    frame(&mut app, vec![], Modifiers::NONE);
}

#[test]
fn a_file_is_included_and_taken_away_from_the_panel() {
    let plain = "let own = #3b6cff\nartboard \"Экран\" 400x300\nrect mark at 60 60 size 20 20 fill own\n";
    let (mut app, dir) = open("panel", plain);
    app.include_file(&dir.join("kit.msc"));
    assert!(app.src.starts_with("include \"kit.msc\"\nlet own"), "{}", app.src);
    assert!(app.status.contains("Подключён «kit.msc»"), "{}", app.status);
    assert_eq!(app.scene.symbols.len(), 1);
    frame(&mut app, vec![], Modifiers::NONE);
    // The same file twice, the scene itself and a file that is not there.
    app.include_file(&dir.join("kit.msc"));
    assert!(app.status.contains("уже подключён"), "{}", app.status);
    app.include_file(&dir.join("screen.msc"));
    assert!(app.status.contains("Это сама сцена"), "{}", app.status);
    app.include_file(&dir.join("none.msc"));
    assert_eq!(app.src.matches("include").count(), 1);
    // While the scene uses the file, the line stays.
    let symbol = app.scene.symbols[0].name.clone();
    let (out, _) = motif_script::add_use(&app.src, &app.scene, &symbol, (200.0, 150.0));
    assert!(app.apply("add-use", Some(out)));
    let used = app.src.clone();
    app.include_act_remove(0);
    assert!(app.src == used && app.status.contains("Сцена использует то, что даёт «kit.msc»"), "{}", app.status);
    app.undo();
    app.include_act_remove(0);
    assert_eq!(app.src, plain);
    app.undo();
    assert!(app.src.starts_with("include \"kit.msc\"\n"));
    // A scene that is not saved has no folder to write the path from.
    let mut fresh = fixture(plain);
    fresh.include_file(&dir.join("kit.msc"));
    assert!(fresh.src == plain && fresh.status.contains("Сначала сохраните сцену"), "{}", fresh.status);
}

#[test]
fn a_changed_file_is_read_again_and_a_theme_of_the_file_takes_own_lines() {
    let (mut app, dir) = open("changed", SCREEN);
    app.poll_includes();
    assert_eq!(fill(&app, "ok-body"), "#ff5a36");
    // The kit is saved with another color of the button, a moment later.
    let kit = dir.join("kit.msc");
    std::fs::write(&kit, KIT.replace("face=#ff5a36", "face=#00aa55")).unwrap();
    let later = std::time::SystemTime::now() + std::time::Duration::from_secs(5);
    std::fs::File::options().write(true).open(&kit).unwrap().set_modified(later).unwrap();
    app.poll_includes();
    assert_eq!(fill(&app, "ok-body"), "#00aa55");
    assert!(app.status.contains("«kit.msc» изменился: сцена перечитана"), "{}", app.status);
    assert!(!app.dirty());
    // A mistake in the kit is told at the line, and the picture stays until it is mended.
    std::fs::write(&kit, KIT.replace("face=#ff5a36 {", "face=#ff5a36")).unwrap();
    std::fs::File::options().write(true).open(&kit).unwrap().set_modified(later + std::time::Duration::from_secs(5)).unwrap();
    app.poll_includes();
    assert!(app.errors[0].message.starts_with("в подключённом файле «kit.msc» ошибка, строка 8"), "{:?}", app.errors);
    assert_eq!(fill(&app, "ok-body"), "#00aa55");
    std::fs::write(&kit, KIT).unwrap();
    std::fs::File::options().write(true).open(&kit).unwrap().set_modified(later + std::time::Duration::from_secs(10)).unwrap();
    app.poll_includes();
    assert!(app.errors.is_empty());
    assert_eq!(fill(&app, "ok-body"), "#ff5a36");
    // The theme of the kit is shown; the scene's own color changed in it gets a block of the
    // scene's own, and the lines of the kit stay where they are.
    app.show_theme("night");
    assert_eq!(app.scene.doc.artboard.background.to_hex(), "#20222b");
    app.palette_act(crate::palette_ui::Act::Set("own".into(), Color::rgb(255, 255, 0)));
    assert!(app.src.contains("let own = #3b6cff\ntheme night {\n  own = #ffff00\n}\n"), "{}", app.src);
    assert_eq!(fill(&app, "mark"), "#ffff00");
    app.palette_act(crate::palette_ui::Act::RenameTheme("night".into(), "dusk".into()));
    assert!(app.status.contains("пишет и подключённый файл «kit.msc»"), "{}", app.status);
    app.palette_act(crate::palette_ui::Act::DeleteTheme("night".into()));
    assert!(!app.src.contains("theme night") && app.status.contains("Сама тема осталась"), "{}", app.status);
    assert!(app.scene.doc.themes.contains(&"night".to_string()));
    assert_eq!(std::fs::read_to_string(&kit).unwrap(), KIT);
}
