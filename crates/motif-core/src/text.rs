//! Text as outlines: glyphs are read from a font once, then laid out at any size, spacing and
//! per-letter animation as plain paths, so every shape style (gradients, strokes, shadows, masks)
//! works on text too.

use kurbo::{Affine, BezPath, Shape, Vec2};

/// The font used when a text names none: IBM Plex Sans (SIL Open Font License), Latin and Cyrillic.
pub static DEFAULT_FONT: &[u8] = include_bytes!("../../motif-editor/assets/fonts/IBMPlexSans-Regular.ttf");
/// `font "bold"`.
pub static BOLD_FONT: &[u8] = include_bytes!("../../motif-editor/assets/fonts/IBMPlexSans-SemiBold.ttf");

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TextAlign {
    Left,
    #[default]
    Center,
    Right,
}

impl TextAlign {
    pub const ALL: [TextAlign; 3] = [TextAlign::Left, TextAlign::Center, TextAlign::Right];

    pub fn keyword(self) -> &'static str {
        match self {
            TextAlign::Left => "left",
            TextAlign::Center => "center",
            TextAlign::Right => "right",
        }
    }

    pub fn from_keyword(s: &str) -> Option<Self> {
        TextAlign::ALL.into_iter().find(|a| a.keyword() == s)
    }
}

/// One letter: its outline and advance, in em units (1.0 = font size), y down, baseline at 0.
#[derive(Clone, Debug, PartialEq)]
pub struct Glyph {
    pub outline: BezPath,
    pub advance: f64,
    /// Whitespace takes room but is not a letter for per-letter effects.
    pub blank: bool,
}

/// The font a text was read from, kept so a game can draw the same letters (the UI kit's theme).
#[derive(Clone, Debug)]
pub struct FontFile {
    /// The file name it goes by: `IBMPlexSans-SemiBold.ttf`, `мой.ttf`.
    pub name: String,
    pub data: std::sync::Arc<Vec<u8>>,
}

impl PartialEq for FontFile {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name && (std::sync::Arc::ptr_eq(&self.data, &other.data) || self.data == other.data)
    }
}

/// Text read from a font, ready to lay out.
#[derive(Clone, Debug, PartialEq)]
pub struct TextShape {
    pub content: String,
    /// Where the letters come from; `None` for text made in code without a file.
    pub font: Option<FontFile>,
    pub lines: Vec<Vec<Glyph>>,
    /// Above and below the baseline, in em.
    pub ascender: f64,
    pub descender: f64,
}

struct Pen<'a> {
    path: &'a mut BezPath,
    k: f64,
}

impl ttf_parser::OutlineBuilder for Pen<'_> {
    fn move_to(&mut self, x: f32, y: f32) {
        self.path.move_to((x as f64 * self.k, -y as f64 * self.k));
    }
    fn line_to(&mut self, x: f32, y: f32) {
        self.path.line_to((x as f64 * self.k, -y as f64 * self.k));
    }
    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        self.path.quad_to((x1 as f64 * self.k, -y1 as f64 * self.k), (x as f64 * self.k, -y as f64 * self.k));
    }
    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        self.path.curve_to((x1 as f64 * self.k, -y1 as f64 * self.k), (x2 as f64 * self.k, -y2 as f64 * self.k), (x as f64 * self.k, -y as f64 * self.k));
    }
    fn close(&mut self) {
        self.path.close_path();
    }
}

/// The horizontal pair adjustment of `a` followed by `b` from the font's GPOS `kern` feature.
fn gpos_kern(face: &ttf_parser::Face, a: ttf_parser::GlyphId, b: ttf_parser::GlyphId) -> Option<i16> {
    use ttf_parser::gpos::{PairAdjustment, PositioningSubtable};
    let gpos = face.tables().gpos?;
    let feature = gpos.features.find(ttf_parser::Tag::from_bytes(b"kern"))?;
    for index in feature.lookup_indices {
        let Some(lookup) = gpos.lookups.get(index) else { continue };
        for k in 0..lookup.subtables.len() {
            let Some(PositioningSubtable::Pair(pair)) = lookup.subtables.get::<PositioningSubtable>(k) else { continue };
            // The first subtable that holds the pair decides it; a class table holds every pair
            // whose first glyph it covers.
            let value = match pair {
                PairAdjustment::Format1 { coverage, sets } => coverage.get(a).and_then(|i| sets.get(i)?.get(b)).map(|v| v.0.x_advance),
                PairAdjustment::Format2 { coverage, classes, matrix } => {
                    coverage.get(a).map(|_| matrix.get((classes.0.get(a), classes.1.get(b))).map_or(0, |v| v.0.x_advance))
                }
            };
            if let Some(v) = value {
                return Some(v);
            }
        }
    }
    None
}

impl TextShape {
    /// Reads the outlines of `content` from a TrueType or OpenType font. Lines break at `\n`.
    pub fn new(content: &str, font: &[u8]) -> Result<TextShape, String> {
        let face = ttf_parser::Face::parse(font, 0).map_err(|e| format!("шрифт не читается: {e}"))?;
        let k = 1.0 / face.units_per_em() as f64;
        let kern = face.tables().kern;
        let lines = content
            .split('\n')
            .map(|line| {
                let mut out: Vec<Glyph> = Vec::new();
                let mut prev: Option<ttf_parser::GlyphId> = None;
                for ch in line.chars() {
                    let id = face.glyph_index(ch).unwrap_or(ttf_parser::GlyphId(0));
                    // Pair kerning: from the OpenType `kern` feature like a browser, or the old
                    // `kern` table of older fonts.
                    if let (Some(p), Some(last)) = (prev, out.last_mut()) {
                        let old = || kern?.subtables.into_iter().filter(|s| s.horizontal && !s.variable).find_map(|s| s.glyphs_kerning(p, id));
                        if let Some(v) = gpos_kern(&face, p, id).or_else(old) {
                            last.advance += v as f64 * k;
                        }
                    }
                    let mut outline = BezPath::new();
                    face.outline_glyph(id, &mut Pen { path: &mut outline, k });
                    let advance = face.glyph_hor_advance(id).unwrap_or(0) as f64 * k;
                    out.push(Glyph { outline, advance, blank: ch.is_whitespace() });
                    prev = Some(id);
                }
                out
            })
            .collect();
        Ok(TextShape { content: content.to_string(), font: None, lines, ascender: face.ascender() as f64 * k, descender: -(face.descender() as f64) * k })
    }
}

/// How a text is laid out and animated.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TextStyle {
    pub size: f64,
    /// Extra room after each letter, in artboard units (CSS `letter-spacing`).
    pub spacing: f64,
    /// Distance between baselines as a multiple of the size (CSS `line-height`).
    pub line_height: f64,
    pub align: TextAlign,
    /// 0..1: how much of the text has appeared, letter by letter, each with a small pop.
    pub reveal: f64,
    /// Height of a wave running through the letters, in artboard units.
    pub wave: f64,
    /// Position of the wave: 1 is one full cycle.
    pub phase: f64,
}

/// Letters that overlap while appearing: each takes this many letter slots to grow.
const REVEAL_SPREAD: f64 = 3.0;

impl TextShape {
    /// All letters as one path, centred on the origin like other shapes.
    pub fn layout(&self, s: &TextStyle) -> BezPath {
        let lh = s.size * s.line_height;
        let total = lh * self.lines.len() as f64;
        // The first baseline sits so the letters (ascender to descender) are centred in their line.
        let first = -total / 2.0 + (lh - (self.ascender + self.descender) * s.size) / 2.0 + self.ascender * s.size;
        let letters = self.lines.iter().flatten().filter(|g| !g.blank).count().max(1) as f64;
        let mut index = 0.0;
        let mut out = BezPath::new();
        for (row, line) in self.lines.iter().enumerate() {
            let width: f64 = line.iter().map(|g| g.advance * s.size).sum::<f64>() + s.spacing * line.len().saturating_sub(1) as f64;
            let mut x = match s.align {
                TextAlign::Left => 0.0,
                TextAlign::Center => -width / 2.0,
                TextAlign::Right => -width,
            };
            let y = first + row as f64 * lh;
            for g in line {
                if !g.blank {
                    let mut t = Affine::translate((x, y)) * Affine::scale(s.size);
                    // Letter i grows over its share of reveal; later letters start later.
                    if s.reveal < 1.0 {
                        let p = ((s.reveal * (letters + REVEAL_SPREAD) - index) / REVEAL_SPREAD).clamp(0.0, 1.0);
                        if p <= 0.0 {
                            x += g.advance * s.size + s.spacing;
                            index += 1.0;
                            continue;
                        }
                        // A pop: overshoots a little before settling.
                        let k = 1.0 + 0.25 * (p * std::f64::consts::PI).sin() - 0.25 * (1.0 - p);
                        let c = t * g.outline.bounding_box().center();
                        t = Affine::translate(c.to_vec2()) * Affine::scale(k * p.sqrt()) * Affine::translate(-c.to_vec2()) * t;
                        t = Affine::translate((0.0, (1.0 - p) * s.size * 0.3)) * t;
                    }
                    if s.wave != 0.0 {
                        let a = (s.phase - index / letters.max(8.0)) * std::f64::consts::TAU;
                        t = Affine::translate(Vec2::new(0.0, -s.wave * a.sin())) * t;
                    }
                    out.extend((t * g.outline.clone()).iter());
                    index += 1.0;
                }
                x += g.advance * s.size + s.spacing;
            }
        }
        out
    }

    /// The box a layout gives the text, as game engines and browsers measure it: each line as
    /// tall as the font from ascender to descender, lines a `line-height` apart.
    pub fn line_box(&self, s: &TextStyle) -> kurbo::Rect {
        let b = self.bounds(s);
        let lines = self.lines.len().max(1) as f64;
        let h = (lines - 1.0) * s.size * s.line_height + (self.ascender + self.descender) * s.size;
        kurbo::Rect::new(b.x0, -h / 2.0, b.x1, h / 2.0)
    }

    /// The box the laid-out text fills, for selecting it by clicking between letters.
    pub fn bounds(&self, s: &TextStyle) -> kurbo::Rect {
        let lh = s.size * s.line_height;
        let h = lh * self.lines.len() as f64;
        let w = self.lines.iter().map(|l| l.iter().map(|g| g.advance * s.size).sum::<f64>() + s.spacing * l.len().saturating_sub(1) as f64).fold(0.0, f64::max);
        let x0 = match s.align {
            TextAlign::Left => 0.0,
            TextAlign::Center => -w / 2.0,
            TextAlign::Right => -w,
        };
        kurbo::Rect::new(x0, -h / 2.0, x0 + w, h / 2.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opentype_kerning_matches_a_browser() {
        // Inter keeps its kerning in the GPOS table only. At 16px «AVATAR» is 66.19 wide letter
        // by letter and 61.13 kerned, as a browser lays it out.
        let font = include_bytes!("../../../examples/fonts/Inter-Medium.ttf");
        let style = TextStyle { size: 16.0, spacing: 0.0, line_height: 1.2, align: TextAlign::Center, reveal: 1.0, wave: 0.0, phase: 0.0 };
        let width = |s: &str| TextShape::new(s, font).unwrap().bounds(&style).width();
        assert!((width("AVATAR") - 61.125).abs() < 0.01, "{}", width("AVATAR"));
        assert!((width("Modern Button") - 113.9375).abs() < 0.001, "{}", width("Modern Button"));
    }
}
