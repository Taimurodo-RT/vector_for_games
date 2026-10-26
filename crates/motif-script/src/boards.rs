//! Several artboards in one script: each `artboard` line at the start of a line opens a board
//! that runs to the next one. What comes before the first board is shared by all of them
//! (comments, `let` colors, fonts). The boards sit on a grid (`cell 1 0` on the artboard line),
//! each at the top-left corner of its cell, with a gap between cells.

use crate::edit::{edit_artboard_line, splice};
use crate::{Span, fmt_num};
use motif_core::kurbo::Point;
use std::ops::Range;

/// Space between neighbouring cells of the board grid.
pub const BOARD_GAP: f64 = 48.0;

/// Where the boards of `src` start: at each `artboard` line, or at the comment lines right
/// above it, which belong to the board they describe.
fn artboard_lines(src: &str) -> Vec<usize> {
    let mut starts = Vec::new();
    let mut at = 0;
    // Where the run of comment lines just above this line starts.
    let mut comments: Option<usize> = None;
    for line in src.split_inclusive('\n') {
        let rest = line.strip_prefix("artboard");
        if rest.is_some_and(|r| r.is_empty() || r.starts_with([' ', '\t', '"', '\n', '\r'])) {
            starts.push(comments.unwrap_or(at));
        }
        comments = if line.trim_start().starts_with("//") { comments.or(Some(at)) } else { None };
        at += line.len();
    }
    starts
}

/// The shared head of `src` and where each board is written: from its `artboard` line to the
/// next one. A script with fewer than two `artboard` lines is one board with no head.
pub fn boards(src: &str) -> (Range<usize>, Vec<Range<usize>>) {
    let starts = artboard_lines(src);
    if starts.len() < 2 {
        let whole = 0..src.len();
        return (0..0, vec![whole]);
    }
    let ranges = starts.iter().enumerate().map(|(i, &s)| s..starts.get(i + 1).copied().unwrap_or(src.len())).collect();
    (0..starts[0], ranges)
}

/// Where the first board of `src` starts: the end of the head of one board's script (the
/// shared head followed by the board). 0 when there is none.
pub fn head_end(src: &str) -> usize {
    artboard_lines(src).first().copied().unwrap_or(0)
}

/// The text of a new board: its `artboard` line, ready to be added to a script, with the
/// background and purpose of `like` (the board it is made next to).
pub fn new_board(name: &str, width: f64, height: f64, cell: (u32, u32), like: Option<&motif_core::Document>) -> String {
    let name: String = name.chars().filter(|c| *c != '"' && *c != '\n' && *c != '\r').collect();
    let mut line = format!("artboard \"{}\" {}x{}", name.trim(), fmt_num(width), fmt_num(height));
    if let Some(doc) = like {
        line.push_str(&format!(" bg {}", doc.artboard.background.to_hex()));
        match doc.purpose {
            motif_core::Purpose::Ui => line.push_str(" ui"),
            motif_core::Purpose::Game => line.push_str(" game"),
            motif_core::Purpose::Any => {}
        }
    }
    format!("{line} cell {} {}\n", cell.0, cell.1)
}

/// Returns the board with its size set; the top-left corner stays where it is.
pub fn set_artboard_size(src: &str, width: f64, height: f64) -> Option<String> {
    let size = format!("{}x{}", fmt_num(width.max(1.0).round()), fmt_num(height.max(1.0).round()));
    edit_artboard_line(src, |words| match words.first_mut() {
        Some(w) if w.contains('x') && w.starts_with(|c: char| c.is_ascii_digit()) => *w = size,
        _ => words.insert(0, size),
    })
}

/// Returns the board with its grid cell written, or removed with `None`.
pub fn set_board_cell(src: &str, cell: Option<(u32, u32)>) -> Option<String> {
    edit_artboard_line(src, |words| {
        if let Some(i) = words.iter().position(|w| w == "cell") {
            words.drain(i..(i + 3).min(words.len()));
        }
        if let Some((c, r)) = cell {
            words.extend(["cell".to_string(), c.to_string(), r.to_string()]);
        }
    })
}

/// Returns the board with its name set (quotes and line breaks are dropped).
pub fn rename_artboard(src: &str, name: &str) -> Option<String> {
    let start = src.match_indices("artboard").find(|(i, _)| *i == 0 || src.as_bytes()[i - 1] == b'\n')?.0;
    let after = start + "artboard".len();
    let name: String = name.chars().filter(|c| *c != '"' && *c != '\n' && *c != '\r').collect();
    let quoted = format!(" \"{}\"", name.trim());
    let line_end = src[after..].find('\n').map_or(src.len(), |n| after + n);
    let rest = &src[after..line_end];
    let trimmed = rest.trim_start();
    let old = if trimmed.starts_with('"') {
        let open = after + (rest.len() - trimmed.len());
        let close = src[open + 1..line_end].find('"').map(|n| open + 1 + n + 1)?;
        Span::new(after, close)
    } else {
        Span::new(after, after)
    };
    Some(splice(src, old, &quoted))
}

/// A board's width, height and the grid cell written for it.
pub type BoardSize = (f64, f64, Option<(u32, u32)>);

/// The boards laid out on their grid.
#[derive(Clone, Debug, PartialEq)]
pub struct BoardGrid {
    /// The cell each board ended up in, in the order given.
    pub cells: Vec<(u32, u32)>,
    /// The top-left corner of each board in the space all boards share.
    pub origins: Vec<Point>,
    col_x: Vec<f64>,
    row_y: Vec<f64>,
}

/// Lays the boards out: each `(width, height, cell)` keeps its cell when it has one that no
/// earlier board took; the rest fill the free cells row by row. Columns are as wide as their
/// widest board and rows as tall as their tallest, with [`BOARD_GAP`] between them.
pub fn board_layout(boards: &[BoardSize]) -> BoardGrid {
    let mut cells: Vec<Option<(u32, u32)>> = vec![None; boards.len()];
    let mut taken = std::collections::HashSet::new();
    for (i, b) in boards.iter().enumerate() {
        if let Some(c) = b.2
            && taken.insert(c)
        {
            cells[i] = Some(c);
        }
    }
    let auto = (boards.len() as f64).sqrt().ceil().max(1.0) as u32;
    let cols = taken.iter().map(|c| c.0 + 1).max().unwrap_or(0).max(auto);
    let mut next = 0u32;
    for cell in cells.iter_mut().filter(|c| c.is_none()) {
        while taken.contains(&(next % cols, next / cols)) {
            next += 1;
        }
        *cell = Some((next % cols, next / cols));
        taken.insert((next % cols, next / cols));
    }
    let cells: Vec<(u32, u32)> = cells.into_iter().map(|c| c.unwrap_or((0, 0))).collect();
    let ncols = cells.iter().map(|c| c.0 + 1).max().unwrap_or(1) as usize;
    let nrows = cells.iter().map(|c| c.1 + 1).max().unwrap_or(1) as usize;
    let (mut widths, mut heights) = (vec![0.0f64; ncols], vec![0.0f64; nrows]);
    for (b, c) in boards.iter().zip(&cells) {
        widths[c.0 as usize] = widths[c.0 as usize].max(b.0);
        heights[c.1 as usize] = heights[c.1 as usize].max(b.1);
    }
    // The start of each column (row) and, last, of the next new one.
    let starts = |sizes: &[f64]| {
        let mut out = vec![0.0];
        for s in sizes {
            out.push(out[out.len() - 1] + s + BOARD_GAP);
        }
        out
    };
    let (col_x, row_y) = (starts(&widths), starts(&heights));
    let origins = cells.iter().map(|c| Point::new(col_x[c.0 as usize], row_y[c.1 as usize])).collect();
    BoardGrid { cells, origins, col_x, row_y }
}

impl BoardGrid {
    /// The top-left corner of `cell`; cells past the grid start where the next new one would.
    pub fn cell_origin(&self, cell: (u32, u32)) -> Point {
        let at = |starts: &[f64], i: u32| starts[(i as usize).min(starts.len() - 1)];
        Point::new(at(&self.col_x, cell.0), at(&self.row_y, cell.1))
    }

    /// The cell under `p`: past the last column or row it is the next, new one.
    pub fn cell_at(&self, p: Point) -> (u32, u32) {
        let pick = |starts: &[f64], v: f64| starts.iter().rposition(|s| *s <= v).unwrap_or(0) as u32;
        (pick(&self.col_x, p.x), pick(&self.row_y, p.y))
    }
}

/// Every board of `src` parsed on its own, the shared head included, in the order written.
/// Errors point into `src`; one in the head is reported once.
pub fn parse_boards(src: &str, base: Option<&std::path::Path>) -> Result<Vec<crate::Scene>, Vec<crate::ScriptError>> {
    let (head, ranges) = boards(src);
    let head = &src[head];
    let head_lines = head.matches('\n').count();
    let mut scenes = Vec::new();
    let mut errors: Vec<crate::ScriptError> = Vec::new();
    for r in ranges {
        let line = src[..r.start].matches('\n').count();
        match crate::parse_in(&format!("{head}{}", &src[r.clone()]), base) {
            Ok(scene) => scenes.push(scene),
            Err(errs) => {
                for e in errs {
                    let e = if e.offset < head.len() {
                        e
                    } else {
                        crate::ScriptError { line: e.line - head_lines + line, offset: e.offset - head.len() + r.start, ..e }
                    };
                    if !errors.contains(&e) {
                        errors.push(e);
                    }
                }
            }
        }
    }
    if errors.is_empty() { Ok(scenes) } else { Err(errors) }
}

/// A board name made safe for a file name: letters, digits, `-` and `_` stay, the rest is `_`.
pub fn board_file_name(name: &str) -> String {
    let s: String = name.trim().chars().map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '_' }).collect();
    if s.is_empty() { "board".to_string() } else { s }
}
