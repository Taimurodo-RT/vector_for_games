//! A tile set: the boards of one file, all of one size, as one atlas picture with a description
//! a game reads it by. The description says where each tile lies in the picture, what it is
//! called, the words it is found by (`tags`) and the sides it repeats along (`tile`), so that
//! nobody marks the atlas up by hand. Which tile goes next to which is the game's own business:
//! the set holds no rules for neighbours.

use motif_core::Document;
use motif_render::{CpuOptions, render_cpu};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use tiny_skia::Pixmap;

/// The version of the description. It grows when a game that reads the old one would read the
/// new one wrong; a new field alone does not change it.
pub const VERSION: u32 = 1;

/// The widest and tallest atlas written, in pixels: what every video card takes as one texture.
pub const ATLAS_LIMIT: u32 = 8192;

/// One board of the file that goes into the set.
#[derive(Clone, Copy)]
pub struct TileBoard<'a> {
    pub doc: &'a Document,
    /// The grid cell written on its `artboard` line (`cell 1 0`), if any.
    pub cell: Option<(u32, u32)>,
}

/// How the set is drawn.
#[derive(Clone, Copy, Debug)]
pub struct TileSetOptions {
    pub render: CpuOptions,
    /// Pixels of its own continuation around each tile. A game that scales or smooths the atlas
    /// then never takes a pixel of the neighbouring tile. Along a side the tile repeats on, the
    /// room holds what comes from the opposite edge; along the others, the edge pixels again.
    pub bleed: u32,
}

/// The description of an atlas: the file `набор.json` beside `набор.png`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TileSetDescription {
    /// The version of this description: [`VERSION`].
    pub motif_tiles: u32,
    /// The file of the atlas, beside the description.
    pub image: String,
    /// The size of the atlas in pixels.
    pub width: u32,
    pub height: u32,
    /// The size of one tile in the atlas in pixels, without its bleed.
    pub tile_width: u32,
    pub tile_height: u32,
    /// The size of one tile in the units of the scene: the size of its artboard.
    pub unit_width: f64,
    pub unit_height: f64,
    /// How many tiles the atlas holds across and down. A cell may be empty.
    pub columns: u32,
    pub rows: u32,
    /// Pixels of its own continuation around each tile. Tiles stand `tile_width + 2 * bleed`
    /// apart.
    pub bleed: u32,
    /// The tiles, row by row.
    pub tiles: Vec<TileEntry>,
}

/// One tile of the atlas.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TileEntry {
    /// The name of its artboard. No two tiles of a set share one.
    pub name: String,
    /// Its cell in the atlas, from the left and from the top, counted from 0.
    pub column: u32,
    pub row: u32,
    /// The top-left corner of the tile itself in the atlas, in pixels (its bleed lies around).
    pub x: u32,
    pub y: u32,
    /// The sides it repeats along without a seam: `"xy"`, `"x"` or `"y"`. Left out of a tile that
    /// does not repeat.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tile: Option<String>,
    /// The words of its `tags` line, as written.
    #[serde(default)]
    pub tags: Vec<String>,
}

impl TileSetDescription {
    /// The description as the text of its file.
    pub fn json(&self) -> String {
        let mut s = serde_json::to_string_pretty(self).expect("a description is plain data");
        s.push('\n');
        s
    }

    /// Reads the text of a description file.
    pub fn parse(json: &str) -> Result<Self, String> {
        let d: Self = serde_json::from_str(json).map_err(|e| format!("описание набора тайлов не читается: {e}"))?;
        if d.motif_tiles > VERSION {
            return Err(format!("описание набора тайлов версии {}, а эта программа читает до {VERSION}", d.motif_tiles));
        }
        Ok(d)
    }

    /// The tile called `name`.
    pub fn tile(&self, name: &str) -> Option<&TileEntry> {
        self.tiles.iter().find(|t| t.name == name)
    }

    /// The tiles that have the word `tag` (letter case is not told apart), row by row.
    pub fn tagged<'a>(&'a self, tag: &'a str) -> impl Iterator<Item = &'a TileEntry> {
        let tag = tag.to_lowercase();
        self.tiles.iter().filter(move |t| t.tags.iter().any(|w| w.to_lowercase() == tag))
    }
}

/// An atlas with its description.
pub struct TileSet {
    pub atlas: Pixmap,
    pub description: TileSetDescription,
}

/// Draws the boards as one atlas. Each tile stands in the cell of its board (`cell 1 0`), so
/// the atlas looks as the boards do in the editor; boards without a cell fill the free cells
/// row by row, and empty columns and rows before the first tile are left out. The atlas file
/// will be `stem.png`, and the description `stem.json`.
pub fn tile_set(boards: &[TileBoard<'_>], opts: TileSetOptions, stem: &str) -> Result<TileSet, String> {
    let first = boards.first().ok_or("в файле нет холстов: набор тайлов собирается из холстов одного размера")?.doc;
    let (unit_w, unit_h) = (first.artboard.width, first.artboard.height);
    let odd: Vec<String> = boards
        .iter()
        .map(|b| &b.doc.artboard)
        .filter(|a| a.width != unit_w || a.height != unit_h)
        .map(|a| format!("«{}» {}×{}", a.name, a.width, a.height))
        .collect();
    if !odd.is_empty() {
        return Err(format!(
            "тайлы набора одного размера, а в файле холсты разные: «{}» {}×{}, но {}. Оставьте в файле набора только тайлы одного размера",
            first.artboard.name,
            unit_w,
            unit_h,
            odd.join(", ")
        ));
    }
    for (i, b) in boards.iter().enumerate() {
        if boards[..i].iter().any(|a| a.doc.artboard.name == b.doc.artboard.name) {
            return Err(format!("в наборе два тайла с именем «{}»: игра находит тайл по имени, назовите холсты по-разному", b.doc.artboard.name));
        }
    }
    let scale = opts.render.scale;
    let (exact_w, exact_h) = (unit_w * scale, unit_h * scale);
    if (exact_w - exact_w.round()).abs() > 1e-6 || (exact_h - exact_h.round()).abs() > 1e-6 || exact_w < 1.0 || exact_h < 1.0 {
        return Err(format!(
            "тайл {unit_w}×{unit_h} в масштабе {scale} занимает {exact_w}×{exact_h} пикселей: размер тайла в атласе должен быть целым, иначе тайлы не сойдутся без шва"
        ));
    }
    let (tw, th) = (exact_w.round() as u32, exact_h.round() as u32);

    let sizes: Vec<motif_script::BoardSize> = boards.iter().map(|b| (unit_w, unit_h, b.cell)).collect();
    let cells = motif_script::board_layout(&sizes).cells;
    let (left, top) = (cells.iter().map(|c| c.0).min().unwrap_or(0), cells.iter().map(|c| c.1).min().unwrap_or(0));
    let cells: Vec<(u32, u32)> = cells.iter().map(|c| (c.0 - left, c.1 - top)).collect();
    let columns = cells.iter().map(|c| c.0 + 1).max().unwrap_or(1);
    let rows = cells.iter().map(|c| c.1 + 1).max().unwrap_or(1);

    let bleed = opts.bleed;
    let (step_x, step_y) = (tw + 2 * bleed, th + 2 * bleed);
    let (width, height) = (columns as u64 * step_x as u64, rows as u64 * step_y as u64);
    if width > ATLAS_LIMIT as u64 || height > ATLAS_LIMIT as u64 {
        return Err(format!(
            "атлас получается {width}×{height} пикселей, а пишется не больше {ATLAS_LIMIT}×{ATLAS_LIMIT}: уменьшите масштаб или разделите набор на два файла"
        ));
    }
    let (width, height) = (width as u32, height as u32);
    let mut atlas = Pixmap::new(width, height).ok_or("не удалось создать картинку атласа")?;

    let mut tiles = Vec::new();
    for (b, cell) in boards.iter().zip(&cells) {
        let name = &b.doc.artboard.name;
        let picture = render_cpu(b.doc, opts.render).ok_or_else(|| format!("не удалось нарисовать тайл «{name}»"))?;
        if (picture.width(), picture.height()) != (tw, th) {
            return Err(format!("тайл «{name}» нарисован {}×{} пикселей вместо {tw}×{th}", picture.width(), picture.height()));
        }
        let (x, y) = (cell.0 * step_x + bleed, cell.1 * step_y + bleed);
        put(&mut atlas, &picture, (x, y), bleed, (b.doc.tile.x, b.doc.tile.y));
        tiles.push(TileEntry {
            name: name.clone(),
            column: cell.0,
            row: cell.1,
            x,
            y,
            tile: match (b.doc.tile.x, b.doc.tile.y) {
                (true, true) => Some("xy".into()),
                (true, false) => Some("x".into()),
                (false, true) => Some("y".into()),
                (false, false) => None,
            },
            tags: b.doc.tags.clone(),
        });
    }
    tiles.sort_by_key(|t| (t.row, t.column));
    let description = TileSetDescription {
        motif_tiles: VERSION,
        image: format!("{stem}.png"),
        width,
        height,
        tile_width: tw,
        tile_height: th,
        unit_width: unit_w,
        unit_height: unit_h,
        columns,
        rows,
        bleed,
        tiles,
    };
    Ok(TileSet { atlas, description })
}

/// Copies `tile` into `atlas` with its top-left corner at `at`, and `bleed` pixels of its own
/// continuation around: what comes from the opposite edge along a side it repeats on (`wrap`),
/// the edge pixels again along the others.
fn put(atlas: &mut Pixmap, tile: &Pixmap, at: (u32, u32), bleed: u32, wrap: (bool, bool)) {
    let (tw, th) = (tile.width() as i64, tile.height() as i64);
    let (b, across) = (bleed as i64, atlas.width() as i64);
    let from = tile.data();
    let to = atlas.data_mut();
    let source = |v: i64, size: i64, wraps: bool| if wraps { v.rem_euclid(size) } else { v.clamp(0, size - 1) };
    for j in -b..th + b {
        let sy = source(j, th, wrap.1);
        for i in -b..tw + b {
            let sx = source(i, tw, wrap.0);
            let s = ((sy * tw + sx) * 4) as usize;
            let d = (((at.1 as i64 + j) * across + at.0 as i64 + i) * 4) as usize;
            to[d..d + 4].copy_from_slice(&from[s..s + 4]);
        }
    }
}

/// Writes the set into `dir`: the atlas under the name its description says (`image`) and the
/// description beside it as `.json`. Returns the two paths.
pub fn write_tile_set(set: &TileSet, dir: &Path) -> Result<(PathBuf, PathBuf), String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let picture = dir.join(&set.description.image);
    let text = picture.with_extension("json");
    set.atlas.save_png(&picture).map_err(|e| format!("{}: {e}", picture.display()))?;
    std::fs::write(&text, set.description.json()).map_err(|e| format!("{}: {e}", text.display()))?;
    Ok((picture, text))
}

#[cfg(test)]
mod tests {
    use super::*;

    const SET: &str = "let ink = #2f2a26
artboard \"Трава\" 16x16 bg #7fb069 tile cell 0 0
tags земля трава
ellipse rock at 16 8 size 8 6 fill #b9a98c stroke ink 1

artboard \"Забор\" 16x16 bg #00000000 tile-x cell 1 0
tags преграда
rect rail at 8 8 size 24 4 fill #a9744f stroke ink 1

artboard \"Пень\" 16x16 bg #7fb069 cell 1 1
ellipse stump at 8 8 size 10 8 fill #a9744f stroke ink 1
";

    fn boards(src: &str) -> Vec<motif_script::Scene> {
        motif_script::parse_boards(src, None).unwrap()
    }

    fn set_of(scenes: &[motif_script::Scene], scale: f64, bleed: u32) -> Result<TileSet, String> {
        let boards: Vec<TileBoard<'_>> = scenes.iter().map(|s| TileBoard { doc: &s.doc, cell: s.board_cell }).collect();
        let render = CpuOptions { scale, antialias: true, background: true };
        tile_set(&boards, TileSetOptions { render, bleed }, "set")
    }

    /// The part of `atlas` that is `w`×`h` pixels from `(x, y)`.
    fn cut(atlas: &Pixmap, x: u32, y: u32, w: u32, h: u32) -> Vec<u8> {
        let mut out = Vec::new();
        for j in y..y + h {
            let from = ((j * atlas.width() + x) * 4) as usize;
            out.extend_from_slice(&atlas.data()[from..from + (w * 4) as usize]);
        }
        out
    }

    #[test]
    fn a_file_of_tiles_becomes_an_atlas_read_by_its_description() {
        let scenes = boards(SET);
        let set = set_of(&scenes, 2.0, 0).unwrap();
        let d = &set.description;
        assert_eq!((d.motif_tiles, d.image.as_str()), (1, "set.png"));
        assert_eq!((d.width, d.height, d.columns, d.rows), (64, 64, 2, 2));
        assert_eq!((d.tile_width, d.tile_height, d.unit_width, d.unit_height, d.bleed), (32, 32, 16.0, 16.0, 0));
        // Row by row, each in the cell of its board.
        let names: Vec<&str> = d.tiles.iter().map(|t| t.name.as_str()).collect();
        assert_eq!(names, ["Трава", "Забор", "Пень"]);
        let grass = d.tile("Трава").unwrap();
        assert_eq!((grass.column, grass.row, grass.x, grass.y, grass.tile.as_deref()), (0, 0, 0, 0, Some("xy")));
        assert_eq!(grass.tags, ["земля", "трава"]);
        let fence = d.tile("Забор").unwrap();
        assert_eq!((fence.column, fence.row, fence.x, fence.y, fence.tile.as_deref()), (1, 0, 32, 0, Some("x")));
        let stump = d.tile("Пень").unwrap();
        assert_eq!((stump.column, stump.row, stump.x, stump.y, stump.tile.as_deref()), (1, 1, 32, 32, None));
        assert!(stump.tags.is_empty());
        assert_eq!(d.tagged("ЗЕМЛЯ").map(|t| t.name.as_str()).collect::<Vec<_>>(), ["Трава"]);
        // What the description points at is the picture of that board, pixel for pixel.
        for (scene, tile) in scenes.iter().zip([grass, fence, stump]) {
            let alone = render_cpu(&scene.doc, CpuOptions { scale: 2.0, antialias: true, background: true }).unwrap();
            assert_eq!(cut(&set.atlas, tile.x, tile.y, 32, 32), alone.data(), "{}", tile.name);
        }
        // The cell no board stands in is empty.
        assert!(cut(&set.atlas, 0, 32, 32, 32).iter().all(|&v| v == 0));
        // The description reads back from its file as it was.
        let text = d.json();
        assert_eq!(&TileSetDescription::parse(&text).unwrap(), d);
        assert!(text.contains("\"tile\": \"xy\"") && text.contains("\"tags\": []"), "{text}");
        assert!(TileSetDescription::parse(&text.replace("\"motif_tiles\": 1", "\"motif_tiles\": 2")).unwrap_err().contains("версии 2"));
        // Written into a folder, the two files stand side by side under the name of the set.
        let dir = std::env::temp_dir().join(format!("motif-tiles-{}", std::process::id()));
        let (picture, file) = write_tile_set(&set, &dir).unwrap();
        assert_eq!((picture.file_name().unwrap().to_str(), file.file_name().unwrap().to_str()), (Some("set.png"), Some("set.json")));
        assert_eq!(&TileSetDescription::parse(&std::fs::read_to_string(&file).unwrap()).unwrap(), d);
        let back = Pixmap::load_png(&picture).unwrap();
        assert_eq!(back.data(), set.atlas.data());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_bleed_of_a_tile_is_its_own_continuation() {
        let scenes = boards(SET);
        let plain = set_of(&scenes, 1.0, 0).unwrap();
        let set = set_of(&scenes, 1.0, 3).unwrap();
        let d = &set.description;
        assert_eq!((d.width, d.height, d.bleed), (44, 44, 3));
        let px = |atlas: &Pixmap, x: u32, y: u32| cut(atlas, x, y, 1, 1);
        for name in ["Трава", "Забор", "Пень"] {
            let (tile, bare) = (d.tile(name).unwrap(), plain.description.tile(name).unwrap());
            assert_eq!((tile.x, tile.y), (tile.column * 22 + 3, tile.row * 22 + 3));
            // The tile itself is the same picture as without the bleed.
            assert_eq!(cut(&set.atlas, tile.x, tile.y, 16, 16), cut(&plain.atlas, bare.x, bare.y, 16, 16), "{name}");
            let (across, down) = match tile.tile.as_deref() {
                Some("xy") => (true, true),
                Some("x") => (true, false),
                _ => (false, false),
            };
            for k in 0..16 {
                for b in 1..=3 {
                    // Left of the tile: its right edge when it repeats across, else its first column.
                    let left = if across { tile.x + 16 - b } else { tile.x };
                    assert_eq!(px(&set.atlas, tile.x - b, tile.y + k), px(&set.atlas, left, tile.y + k), "{name}: слева");
                    let right = if across { tile.x + b - 1 } else { tile.x + 15 };
                    assert_eq!(px(&set.atlas, tile.x + 15 + b, tile.y + k), px(&set.atlas, right, tile.y + k), "{name}: справа");
                    let above = if down { tile.y + 16 - b } else { tile.y };
                    assert_eq!(px(&set.atlas, tile.x + k, tile.y - b), px(&set.atlas, tile.x + k, above), "{name}: сверху");
                    let below = if down { tile.y + b - 1 } else { tile.y + 15 };
                    assert_eq!(px(&set.atlas, tile.x + k, tile.y + 15 + b), px(&set.atlas, tile.x + k, below), "{name}: снизу");
                }
            }
        }
        // The rock on the right edge of the grass shows in the room left of the tile.
        let grass = d.tile("Трава").unwrap();
        assert_ne!(px(&set.atlas, grass.x - 2, grass.y + 8), px(&set.atlas, grass.x + 8, grass.y + 1));
    }

    #[test]
    fn a_set_says_what_keeps_it_from_being_one() {
        let other_size = format!("{SET}\nartboard \"Дом\" 32x16\nrect wall size 8 8\n");
        let e = set_of(&boards(&other_size), 1.0, 0).err().unwrap();
        assert!(e.contains("«Дом» 32×16") && e.contains("«Трава» 16×16"), "{e}");
        let same_name = format!("{SET}\nartboard \"Трава\" 16x16\nrect wall size 8 8\n");
        let e = set_of(&boards(&same_name), 1.0, 0).err().unwrap();
        assert!(e.contains("два тайла с именем «Трава»"), "{e}");
        let e = set_of(&boards(SET), 1.3, 0).err().unwrap();
        assert!(e.contains("должен быть целым"), "{e}");
        let e = set_of(&boards(SET), 512.0, 0).err().unwrap();
        assert!(e.contains("не больше 8192×8192"), "{e}");
        // 1.5 gives whole pixels for a 16-unit tile: 24.
        assert_eq!(set_of(&boards(SET), 1.5, 0).unwrap().description.tile_width, 24);
    }

    #[test]
    fn empty_columns_before_the_first_tile_are_left_out() {
        let src = "artboard \"А\" 8x8 bg #ff0000 cell 2 1\nartboard \"Б\" 8x8 bg #00ff00 cell 4 1\n";
        let set = set_of(&boards(src), 1.0, 0).unwrap();
        let d = &set.description;
        assert_eq!((d.columns, d.rows, d.width, d.height), (3, 1, 24, 8));
        assert_eq!((d.tile("А").unwrap().x, d.tile("Б").unwrap().x), (0, 16));
        // Boards without a cell fill the grid row by row, as the editor lays them.
        let src = "artboard \"А\" 8x8\nartboard \"Б\" 8x8\nartboard \"В\" 8x8\n";
        let d = set_of(&boards(src), 1.0, 0).unwrap().description;
        assert_eq!(d.tiles.iter().map(|t| (t.column, t.row)).collect::<Vec<_>>(), [(0, 0), (1, 0), (0, 1)]);
    }
    /// The set of the examples: 16 ground tiles. Its description is kept beside it as a sample
    /// (`ground-tiles.tiles.json`), since a game binds to it; after a deliberate change rewrite
    /// it with `MOTIF_BLESS=1 cargo test -p motif-pixel tiles`.
    #[test]
    fn the_ground_set_of_the_examples_is_read_by_its_description() {
        let file = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/ground-tiles.msc");
        let scenes = motif_script::parse_boards(&std::fs::read_to_string(&file).unwrap(), file.parent()).unwrap();
        let boards: Vec<TileBoard<'_>> = scenes.iter().map(|s| TileBoard { doc: &s.doc, cell: s.board_cell }).collect();
        let render = CpuOptions { scale: 1.0, antialias: true, background: true };
        let set = tile_set(&boards, TileSetOptions { render, bleed: 0 }, "ground-tiles").unwrap();
        let d = &set.description;
        assert_eq!((d.tiles.len(), d.columns, d.rows, d.width, d.height), (16, 4, 4, 256, 256));
        // Every tile repeats, has words, and is found by them without looking at the picture.
        assert!(d.tiles.iter().all(|t| t.tile.as_deref() == Some("xy") && !t.tags.is_empty()));
        assert_eq!(d.tagged("вода").map(|t| t.name.as_str()).collect::<Vec<_>>(), ["Вода", "Вода с кувшинкой"]);
        assert_eq!(d.tagged("преграда").count(), 2);
        assert_eq!(d.tagged("проходимо").count(), 14);
        let lane = d.tile("Тропа по вертикали").unwrap();
        assert_eq!((lane.column, lane.row, lane.x, lane.y), (3, 3, 192, 192));
        let sample = file.with_extension("tiles.json");
        if std::env::var_os("MOTIF_BLESS").is_some() {
            std::fs::write(&sample, d.json()).unwrap();
        }
        let kept = std::fs::read_to_string(&sample).unwrap_or_default().replace("\r\n", "\n");
        assert_eq!(kept, d.json(), "описание набора не совпало с образцом (MOTIF_BLESS=1 перепишет образец)");
    }
}
