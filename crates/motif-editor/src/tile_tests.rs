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
