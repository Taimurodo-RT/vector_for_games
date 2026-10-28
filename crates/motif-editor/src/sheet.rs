//! Sprite sheets as backdrops: guessing the grid of a sheet, and cutting out the frame shown.

use tiny_skia::{IntRect, Pixmap};

/// The grid a sprite sheet was guessed to have.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Grid {
    pub cols: u32,
    pub rows: u32,
    /// Frames in use: the last row may be short.
    pub frames: u32,
    /// Found by the transparent gaps between frames — a sure sign; otherwise only square frames
    /// in a strip, which a plain wide picture can also be.
    pub gaps: bool,
}

/// The grid of a sprite sheet when the picture looks like one: frames set apart by fully
/// transparent rows and columns, or square frames in a strip.
pub fn guess(px: &Pixmap) -> Option<Grid> {
    let (w, h) = (px.width(), px.height());
    let alpha = |x: u32, y: u32| px.pixel(x, y).map_or(0, |p| p.alpha());
    let col_empty: Vec<bool> = (0..w).map(|x| (0..h).all(|y| alpha(x, y) == 0)).collect();
    let row_empty: Vec<bool> = (0..h).map(|y| (0..w).all(|x| alpha(x, y) == 0)).collect();
    let cols = cells(&col_empty);
    let rows = cells(&row_empty);
    if cols * rows > 1 {
        let (fw, fh) = (w / cols, h / rows);
        // The last frames of the last row may be missing.
        let used = (0..rows * cols)
            .filter(|i| {
                let (cx, cy) = (i % cols * fw, i / cols * fh);
                (cy..cy + fh).any(|y| (cx..cx + fw).any(|x| alpha(x, y) > 0))
            })
            .max()
            .map_or(0, |last| last + 1);
        return (used > 1).then_some(Grid { cols, rows, frames: used, gaps: true });
    }
    // No gaps: square frames side by side, in a row or a column.
    if h > 0 && w > h && w.is_multiple_of(h) {
        return Some(Grid { cols: w / h, rows: 1, frames: w / h, gaps: false });
    }
    if w > 0 && h > w && h.is_multiple_of(w) {
        return Some(Grid { cols: 1, rows: h / w, frames: h / w, gaps: false });
    }
    None
}

/// How many equal cells a line of `empty` flags splits into: runs of drawn pixels, each inside a
/// cell of the same width. 1 when the runs do not sit on such a grid.
fn cells(empty: &[bool]) -> u32 {
    let mut runs = Vec::new();
    let mut start = None;
    for (i, &e) in empty.iter().chain([true].iter()).enumerate() {
        match (e, start) {
            (false, None) => start = Some(i),
            (true, Some(s)) => {
                runs.push((s, i));
                start = None;
            }
            _ => {}
        }
    }
    let n = runs.len();
    if n < 2 || !empty.len().is_multiple_of(n) {
        return 1;
    }
    let cell = empty.len() / n;
    let fits = runs.iter().enumerate().all(|(k, &(s, e))| s >= k * cell && e <= (k + 1) * cell);
    if fits { n as u32 } else { 1 }
}

/// Frame `i` of a sheet `cols` × `rows`, cut out of `px`.
pub fn frame(px: &Pixmap, cols: u32, rows: u32, i: u32) -> Option<Pixmap> {
    let (fw, fh) = (px.width() / cols.max(1), px.height() / rows.max(1));
    let (cx, cy) = (i % cols.max(1) * fw, i / cols.max(1) * fh);
    px.clone_rect(IntRect::from_xywh(cx as i32, cy as i32, fw, fh)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tiny_skia::{Paint, Rect, Transform};

    /// A sheet of `cols` × `rows` cells of `size`, with a dot in the first `used` of them.
    fn sheet(cols: u32, rows: u32, size: u32, used: u32) -> Pixmap {
        let mut px = Pixmap::new(cols * size, rows * size).unwrap();
        let mut paint = Paint::default();
        paint.set_color_rgba8(255, 90, 54, 255);
        for i in 0..used {
            let (x, y) = ((i % cols * size) as f32, (i / cols * size) as f32);
            // Each frame drawn a little differently, never touching the cell's edges.
            let d = (i % 3) as f32;
            px.fill_rect(Rect::from_xywh(x + 2.0 + d, y + 3.0, size as f32 - 6.0 - d, size as f32 - 6.0).unwrap(), &paint, Transform::identity(), None);
        }
        px
    }

    #[test]
    fn grids_are_guessed() {
        let grid = |cols, rows, frames, gaps| Some(Grid { cols, rows, frames, gaps });
        assert_eq!(guess(&sheet(6, 1, 16, 6)), grid(6, 1, 6, true));
        assert_eq!(guess(&sheet(4, 3, 20, 10)), grid(4, 3, 10, true), "a short last row");
        assert_eq!(guess(&sheet(1, 1, 16, 1)), None, "one picture is not a sheet");
        // Frames that fill their cells: square frames in a strip.
        let mut strip = Pixmap::new(96, 24).unwrap();
        strip.fill(tiny_skia::Color::from_rgba8(10, 20, 30, 255));
        assert_eq!(guess(&strip), grid(4, 1, 4, false));
        let mut sketch = Pixmap::new(300, 200).unwrap();
        sketch.fill(tiny_skia::Color::WHITE);
        assert_eq!(guess(&sketch), None);
    }

    #[test]
    fn a_frame_is_cut_out() {
        let px = sheet(4, 2, 10, 8);
        let f = frame(&px, 4, 2, 5).unwrap();
        assert_eq!((f.width(), f.height()), (10, 10));
        assert_eq!(f.pixel(5, 5), px.pixel(15, 15));
        assert_eq!(f.pixel(0, 0).unwrap().alpha(), 0);
    }
}
