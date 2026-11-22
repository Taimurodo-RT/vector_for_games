//! «Библиотека»: the symbols of the project folder, found, drawn and placed into the scene.

use super::editing_tests::fixture;
use super::*;
use crate::shelf_ui::Item;

const KIT: &str = "let ink = #1b1d24
symbol Button face=#ff5a36 {
  tags кнопка меню
  variant quiet face=#8a8f9c
  rect body size 120 40 fill face stroke ink 2 r 8
}
symbol Coin {
  tags hud
  ellipse body size 20 20 fill #ffd23f stroke ink 2
}
";

const SCREEN: &str = "motif 7
symbol Mark {
  rect dot size 12 12 fill #3b6cff
}
artboard \"Меню\" 400x300 bg #fff4e4
use Mark m1 at 40 40

artboard \"Пауза\" 400x300 bg #20222b
rect veil at 200 150 size 400 300 fill #00000066
";

/// A project folder with a kit, a broken file and the screen, and the screen open from it.
fn open(test: &str) -> (App, std::path::PathBuf) {
    let dir = std::env::temp_dir().join(format!("motif-editor-shelf-{}-{test}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("kits")).unwrap();
    std::fs::write(dir.join("kits/kit.msc"), KIT).unwrap();
    std::fs::write(dir.join("broken.msc"), "symbol Bad {\n  frobnicate\n}\n").unwrap();
    std::fs::write(dir.join("screen.msc"), SCREEN).unwrap();
    let mut app = fixture("");
    app.path = Some(dir.join("screen.msc"));
    app.set_file(SCREEN, 0);
    app.saved_src = SCREEN.into();
    app.reparse();
    assert!(app.errors.is_empty(), "{:?}", app.errors);
    (app, dir)
}

/// One frame of the left panel, where the library is a tab.
fn panel(app: &mut App) {
    let ctx = app.ctx.clone();
    ctx.run_ui(egui::RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(300.0, 700.0))), ..Default::default() }, |ui| app.layers_panel(ui))
        .drop_without_applying_deltas();
}

#[test]
fn the_library_lists_the_symbols_of_the_project_folder() {
    let (mut app, dir) = open("list");
    app.shelf.open = true;
    panel(&mut app);
    while app.shelf_unread() > 0 {
        panel(&mut app);
    }
    // The scene itself is not a file of its library; the kit is, by its path from the scene.
    let files: Vec<(&str, Vec<&str>, bool)> =
        app.shelf.files.iter().map(|(f, name)| (name.as_str(), f.symbols.iter().map(|s| s.name.as_str()).collect(), f.error.is_some())).collect();
    assert_eq!(files, [("broken.msc", vec![], true), ("kits/kit.msc", vec!["Button", "Coin"], false)]);
    // A picture of a symbol is the symbol alone, drawn to fit.
    let (scene, id) = motif_script::library_preview(&dir.join("kits/kit.msc"), "Button").unwrap();
    let p = crate::shelf_ui::picture(&scene, &id, 192.0).unwrap();
    assert!(p.width() == 192 && p.height() < 100, "{}x{}", p.width(), p.height());
    assert!(p.pixels().iter().any(|c| c.alpha() == 255) && p.pixels()[0].alpha() == 0);
    // The search and the tags narrow the list while the panel is drawn.
    app.shelf.query = "hud".into();
    panel(&mut app);
    app.shelf.query.clear();
    app.shelf.tag = Some("меню".into());
    panel(&mut app);
    assert_eq!(app.shelf.tag.as_deref(), Some("меню"));
    // A tag nothing has any more is dropped.
    app.shelf.tag = Some("нет-такого".into());
    panel(&mut app);
    assert_eq!(app.shelf.tag, None);
}

#[test]
fn a_symbol_of_the_library_is_placed_with_its_include_line() {
    let (mut app, dir) = open("place");
    let kit = dir.join("kits/kit.msc");
    // On the second board: the include line goes into the head both boards share.
    app.switch_board(1);
    app.shelf_place(&Item { file: Some(kit.clone()), symbol: "Button".into() }, Some((120.4, 80.6)));
    assert!(app.errors.is_empty(), "{:?} {}", app.errors, app.status);
    assert!(app.status.contains("Вставлена копия button1 символа Button из «kits/kit.msc»") && app.status.contains("include"), "{}", app.status);
    let file = app.file_text();
    assert!(file.starts_with("motif 7\ninclude \"kits/kit.msc\"\n"), "{file}");
    assert!(file.contains("use Button button1 at 120 81\n"), "{file}");
    assert_eq!(app.selected.as_deref(), Some("button1"));
    // One step back takes away both the copy and the line.
    app.undo();
    assert_eq!(app.file_text(), SCREEN);
    app.redo();
    // The next symbol of the same file adds only its copy, in the middle of the artboard.
    app.shelf_place(&Item { file: Some(kit.clone()), symbol: "Coin".into() }, None);
    let file = app.file_text();
    assert!(file.matches("include").count() == 1 && file.contains("use Coin coin1 at 200 150\n"), "{file}");
    // The first board sees the kit too, and takes the scene's own symbol without any file.
    app.switch_board(0);
    app.shelf_place(&Item { file: None, symbol: "Mark".into() }, Some((10.0, 10.0)));
    assert!(app.src.contains("use Mark mark1 at 10 10\n"), "{}", app.src);
    app.shelf_place(&Item { file: Some(kit), symbol: "Button".into() }, None);
    assert!(app.errors.is_empty() && app.src.contains("use Button button1 at 200 150\n"), "{} {}", app.status, app.src);
    // A file that does not read is not taken, and the scene stays as it was.
    let before = app.file_text();
    app.shelf_place(&Item { file: Some(dir.join("broken.msc")), symbol: "Bad".into() }, None);
    assert!(app.status.contains("Сцена не изменена") && app.file_text() == before, "{}", app.status);
}

#[test]
fn the_tags_of_a_symbol_are_written_from_its_panel() {
    let (mut app, _) = open("tags");
    app.set_symbol_tags("Mark", &["метка".to_string(), "две части".to_string()]);
    assert!(app.src.contains("symbol Mark {\n  tags метка \"две части\"\n"), "{}", app.src);
    assert!(app.status.contains("Теги символа Mark: метка, две части"), "{}", app.status);
    // The panel of a copy shows them in a field.
    let node = app.scene.doc.node("m1").unwrap().clone();
    let ctx = app.ctx.clone();
    ctx.run_ui(egui::RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(300.0, 700.0))), ..Default::default() }, |ui| {
        app.symbol_section(ui, &node)
    })
    .drop_without_applying_deltas();
    assert_eq!(app.tags_edit, None);
    app.set_symbol_tags("Mark", &[]);
    assert!(!app.src.contains("tags") && app.status.contains("больше нет тегов"), "{}", app.status);
    // The tags of a symbol of an included file are written in that file.
    app.shelf_place(&Item { file: Some(app.base_dir().unwrap().join("kits/kit.msc")), symbol: "Coin".into() }, None);
    let before = app.file_text();
    app.set_symbol_tags("Coin", &["монета".to_string()]);
    assert!(app.status.contains("в подключённом файле «kits/kit.msc»") && app.file_text() == before, "{}", app.status);
}

#[test]
fn a_folder_of_many_files_is_read_a_few_a_frame() {
    use std::time::{Duration, SystemTime};
    let (mut app, dir) = open("queue");
    // Screens with no symbol of their own, one of them with a mistake: the library passes them by.
    for k in 0..4 {
        std::fs::write(dir.join(format!("level{k}.msc")), "artboard \"Уровень\" 200x100\nrect a size 10 10\n").unwrap();
    }
    std::fs::write(dir.join("level4.msc"), "artboard \"Уровень\" 200x100\nfrobnicate\n").unwrap();
    let names = |app: &App| -> Vec<String> { app.shelf.files.iter().map(|(_, name)| name.clone()).collect() };
    // A call with no time to spend reads one file and leaves the rest for the next frames.
    app.shelf_scan(0.0, Duration::ZERO);
    assert_eq!((names(&app), app.shelf_unread()), (vec!["broken.msc".to_string()], 6));
    let mut calls = 1;
    while app.shelf_unread() > 0 {
        app.shelf_scan(0.1, Duration::ZERO);
        calls += 1;
    }
    assert_eq!(calls, 7);
    assert_eq!(names(&app), ["broken.msc", "kits/kit.msc", "level0.msc", "level1.msc", "level2.msc", "level3.msc", "level4.msc"]);
    let errors: Vec<bool> = app.shelf.files.iter().map(|(f, _)| f.error.is_some()).collect();
    assert_eq!(errors, [true, false, false, false, false, false, false]);
    // Nothing changed: the next look into the folder reads nothing.
    app.shelf_scan(10.0, Duration::ZERO);
    assert_eq!(app.shelf_unread(), 0);
    // The kit changed: what it gave stays listed until the file is read again.
    let kit = dir.join("kits/kit.msc");
    std::fs::write(&kit, "symbol Star {\n  star body size 20 20 fill #ffd23f\n}\n").unwrap();
    std::fs::File::options().write(true).open(&kit).unwrap().set_modified(SystemTime::now() + Duration::from_secs(5)).unwrap();
    let gives = |app: &App| -> Vec<String> { app.shelf.files[1].0.symbols.iter().map(|s| s.name.clone()).collect() };
    app.shelf_scan(20.0, Duration::ZERO);
    assert_eq!((gives(&app), app.shelf_unread()), (vec!["Button".to_string(), "Coin".to_string()], 6));
    app.shelf_scan(20.1, Duration::from_secs(60));
    assert_eq!((gives(&app), app.shelf_unread()), (vec!["Star".to_string()], 0));
    // A file that went away leaves the list at once.
    std::fs::remove_file(dir.join("broken.msc")).unwrap();
    app.shelf_scan(30.0, Duration::ZERO);
    assert_eq!(names(&app)[0], "kits/kit.msc");
    let _ = std::fs::remove_dir_all(&dir);
}
