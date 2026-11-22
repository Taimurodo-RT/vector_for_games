//! A board that is a tile: the word on its line, the field of its neighbours, and picking an
//! object by the part that came in from the opposite edge.

use super::editing_tests::fixture;
use super::snap_tests::{click, frame, screen};
use super::*;
use motif_core::Tile;

const TILES: &str = "motif 8
let ink = #2f2a26
artboard \"Трава\" 96x96 bg #7fb069 cell 0 0
ellipse rock at 96 48 size 30 20 fill #b9a98c stroke ink 2
rect patch at 20 20 size 10 10 fill #4f7a3c

artboard \"Камень\" 96x96 bg #6f6558 cell 1 0
rect brick at 48 48 size 40 40 fill #b9a98c
";

fn open() -> App {
    let mut app = fixture("");
    app.set_file(TILES, 0);
    app.saved_src = TILES.into();
    app.reparse();
    assert!(app.errors.is_empty(), "{:?}", app.errors);
    app
}

#[test]
fn a_board_becomes_a_tile_and_shows_its_field() {
    let mut app = open();
    // Both boards in view.
    app.boards.fit_all = true;
    frame(&mut app, vec![], Modifiers::NONE);
    assert!(!app.field_shown() && app.scene.doc.tile == Tile::default());
    // The half of the rock past the right edge is cut off: nothing to pick at the left edge.
    let left = screen(&mut app, 6.0, 48.0);
    click(&mut app, left, Modifiers::NONE);
    assert_eq!(app.selected, None);
    // «Весь»: the word goes onto the line of this board only, in one step back.
    app.set_document_tile(Tile::BOTH);
    assert!(app.file_text().contains("artboard \"Трава\" 96x96 bg #7fb069 cell 0 0 tile\n"), "{}", app.file_text());
    assert!(app.file_text().contains("artboard \"Камень\" 96x96 bg #6f6558 cell 1 0\n"));
    assert!(app.status.contains("Холст стал тайлом"), "{}", app.status);
    // The field is shown at once: eight neighbours, each the tile again one tile away.
    assert!(app.field_shown());
    let moves = app.field_moves();
    assert_eq!(moves.len(), 8);
    assert!(moves.iter().all(|m| (m.0.abs() == 96.0 || m.1.abs() == 96.0) && m.0.abs() <= 96.0 && m.1.abs() <= 96.0));
    frame(&mut app, vec![], Modifiers::NONE);
    // The half that came in from the right edge picks the rock.
    let left = screen(&mut app, 6.0, 48.0);
    click(&mut app, left, Modifiers::NONE);
    assert_eq!(app.selected.as_deref(), Some("rock"));
    // Where the other board lies there is a neighbour now: a click there does not open it.
    let grid = app.board_grid();
    let other = (grid.origins[1].x - grid.origins[0].x + 48.0, grid.origins[1].y - grid.origins[0].y + 48.0);
    let beside = screen(&mut app, other.0, other.1);
    click(&mut app, beside, Modifiers::NONE);
    assert_eq!(app.boards.active, 0);
    // A tile along one side has two neighbours.
    app.set_document_tile(Tile { x: true, y: false });
    assert!(app.src.contains("cell 0 0 tile-x\n"), "{}", app.src);
    assert_eq!(app.field_moves(), [(-96.0, 0.0), (96.0, 0.0)]);
    // The field put away: the other boards are back, and a click opens one.
    app.boards.tile_field = false;
    assert!(app.field_moves().is_empty());
    frame(&mut app, vec![], Modifiers::NONE);
    let beside = screen(&mut app, other.0, other.1);
    click(&mut app, beside, Modifiers::NONE);
    assert_eq!(app.boards.active, 1);
    // The other board is not a tile: nothing is shown as a field there.
    app.boards.tile_field = true;
    assert!(!app.field_shown());
    app.switch_board(0);
    assert!(app.field_shown());
    // «Нет» takes the word away; two steps back and the file is as it was.
    app.set_document_tile(Tile::default());
    assert!(!app.file_text().contains("tile") && !app.field_shown(), "{}", app.file_text());
    assert!(app.status.contains("больше не тайл"), "{}", app.status);
}

#[test]
fn the_boards_of_a_file_are_saved_as_a_set_of_tiles() {
    let mut app = open();
    app.set_document_tile(Tile::BOTH);
    assert!(app.has_tiles());
    // The words a game finds the board by: a line under the `artboard` line of this board only.
    app.set_board_tags(&["трава".to_string(), "проходимо".to_string()]);
    assert!(app.file_text().contains("artboard \"Трава\" 96x96 bg #7fb069 cell 0 0 tile\ntags трава проходимо\n"), "{}", app.file_text());
    assert_eq!(app.scene.doc.tags, ["трава", "проходимо"]);
    assert!(app.status.contains("Метки холста «Трава»: трава, проходимо"), "{}", app.status);
    assert!(app.board_doc(1).unwrap().tags.is_empty());
    // A board made next to a tile is a tile of the same size and sides.
    app.add_board();
    assert_eq!(app.board_count(), 3);
    let made = app.scene.doc.clone();
    assert_eq!((made.tile, made.artboard.width, made.artboard.height), (Tile::BOTH, 96.0, 96.0));
    assert!(made.tags.is_empty());
    // The set: one atlas and its description, named after the file, tiles where the boards are.
    let dir = std::env::temp_dir().join(format!("motif-editor-tiles-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    app.export.tile_bleed = 2;
    let (picture, text) = app.tiles_to(&dir).expect("saved");
    assert!(app.status.starts_with("Набор тайлов: scene.png и scene.json"), "{}", app.status);
    let d = motif_pixel::tiles::TileSetDescription::parse(&std::fs::read_to_string(&text).unwrap()).unwrap();
    assert_eq!((d.image.as_str(), d.tiles.len(), d.tile_width, d.tile_height, d.bleed), ("scene.png", 3, 96, 96, 2));
    let grass = d.tile("Трава").unwrap();
    assert_eq!((grass.column, grass.row, grass.x, grass.y, grass.tile.as_deref()), (0, 0, 2, 2, Some("xy")));
    assert_eq!(grass.tags, ["трава", "проходимо"]);
    let stone = d.tile("Камень").unwrap();
    assert_eq!((stone.column, stone.row, stone.tile.as_deref()), (1, 0, None));
    let atlas = tiny_skia::Pixmap::load_png(&picture).unwrap();
    assert_eq!((atlas.width(), atlas.height()), (d.width, d.height));
    // The tile in the atlas is the board as `motif render` draws it.
    let alone = motif_render::render_cpu(&app.board_doc(0).unwrap(), CpuOptions { scale: 1.0, antialias: true, background: true }).unwrap();
    let row = |px: &tiny_skia::Pixmap, x: u32, y: u32, w: u32| px.data()[((y * px.width() + x) * 4) as usize..((y * px.width() + x + w) * 4) as usize].to_vec();
    for y in 0..96 {
        assert_eq!(row(&atlas, grass.x, grass.y + y, 96), row(&alone, 0, y, 96), "ряд {y}");
    }
    // No words: the line goes away.
    app.switch_board(0);
    app.set_board_tags(&[]);
    assert!(!app.file_text().contains("tags"), "{}", app.file_text());
    assert!(app.status.contains("больше нет меток"), "{}", app.status);
    // A board of another size keeps the file from being a set, and the editor says which.
    app.switch_board(1);
    app.resize_board(48.0, 96.0);
    assert!(app.tiles_to(&dir).is_none());
    assert!(app.status.contains("Набор тайлов не сохранён") && app.status.contains("«Камень» 48×96"), "{}", app.status);
    let _ = std::fs::remove_dir_all(&dir);
}
