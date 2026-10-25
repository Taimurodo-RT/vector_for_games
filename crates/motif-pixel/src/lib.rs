//! Bridges between Motif and pixel-art tools.
//!
//! * PNG frame sequences that PixaTool can batch-process.
//! * [`pixatool`]: drives PixaTool itself, so frames come back pixelized with no dialogs.
//! * JASC `.pal` palettes (the format PixaTool loads, up to 192 colors).
//! * A quick pixel preview: render at the target size without anti-aliasing, then snap to a palette.

pub mod pixatool;

use motif_core::{Color, Document, Value};
use motif_render::{CpuOptions, render_cpu};
use tiny_skia::Pixmap;

/// PixaTool refuses palettes larger than this.
pub const PIXATOOL_MAX_COLORS: usize = 192;

/// Every distinct opaque color used by the scene: background, fills, strokes and animated colors.
pub fn scene_palette(doc: &Document) -> Vec<Color> {
    let mut out: Vec<Color> = Vec::new();
    let mut add = |c: Color| {
        let c = Color { a: 255, ..c };
        if !out.contains(&c) {
            out.push(c);
        }
    };
    add(doc.artboard.background);
    for n in &doc.nodes {
        n.fill.into_iter().chain(n.stroke).for_each(&mut add);
    }
    for a in &doc.animations {
        for t in &a.tracks {
            for k in &t.keys {
                if let Value::Color(c) = k.value {
                    add(c);
                }
            }
        }
    }
    out.truncate(PIXATOOL_MAX_COLORS);
    out
}

/// Writes a JASC-PAL file (Paint Shop Pro palette), as read by PixaTool.
pub fn to_jasc(colors: &[Color]) -> String {
    let mut s = format!("JASC-PAL\r\n0100\r\n{}\r\n", colors.len());
    for c in colors {
        s.push_str(&format!("{} {} {}\r\n", c.r, c.g, c.b));
    }
    s
}

pub fn parse_jasc(text: &str) -> Result<Vec<Color>, String> {
    let mut lines = text.lines().map(str::trim).filter(|l| !l.is_empty());
    if lines.next() != Some("JASC-PAL") {
        return Err("это не палитра JASC-PAL".into());
    }
    lines.next();
    let count: usize = lines.next().and_then(|l| l.parse().ok()).ok_or("нет числа цветов")?;
    let colors: Vec<Color> = lines
        .take(count)
        .map(|l| {
            let v: Vec<u8> = l.split_whitespace().filter_map(|p| p.parse().ok()).collect();
            match v[..] {
                [r, g, b, ..] => Ok(Color::rgb(r, g, b)),
                _ => Err(format!("неверная строка палитры «{l}»")),
            }
        })
        .collect::<Result<_, _>>()?;
    if colors.len() != count {
        return Err(format!("в палитре заявлено {count} цветов, а найдено {}", colors.len()));
    }
    Ok(colors)
}

/// Snaps every pixel to the nearest palette color and makes alpha binary (cut at 50%).
pub fn quantize(pixmap: &mut Pixmap, palette: &[Color]) {
    if palette.is_empty() {
        return;
    }
    for px in pixmap.pixels_mut() {
        let c = px.demultiply();
        if c.alpha() < 128 {
            *px = tiny_skia::PremultipliedColorU8::TRANSPARENT;
            continue;
        }
        let best = palette
            .iter()
            .min_by_key(|p| {
                // Weighted RGB distance: cheap and close enough to perceptual for a preview.
                let dr = p.r as i32 - c.red() as i32;
                let dg = p.g as i32 - c.green() as i32;
                let db = p.b as i32 - c.blue() as i32;
                2 * dr * dr + 4 * dg * dg + 3 * db * db
            })
            .unwrap();
        *px = tiny_skia::PremultipliedColorU8::from_rgba(best.r, best.g, best.b, 255).unwrap();
    }
}

/// Settings for exporting frames.
#[derive(Clone, Debug)]
pub struct FrameExport {
    /// Frames per second; 12 gives the classic "on twos" pixel-art timing.
    pub fps: f64,
    pub render: CpuOptions,
}

impl Default for FrameExport {
    fn default() -> Self {
        FrameExport { fps: 12.0, render: CpuOptions::default() }
    }
}

/// Renders every frame of an animation. Looping animations skip the last frame, which equals the first.
pub fn render_frames(doc: &Document, anim: &str, opts: &FrameExport) -> Result<Vec<Pixmap>, String> {
    let animation = doc.animation(anim).ok_or_else(|| format!("нет анимации «{anim}»"))?;
    if opts.fps <= 0.0 {
        return Err("fps должен быть больше нуля".into());
    }
    let mut count = (animation.duration * opts.fps).round() as usize;
    if !animation.looping {
        count += 1;
    }
    (0..count.max(1))
        .map(|i| {
            let t = (i as f64 / opts.fps).min(animation.duration);
            render_cpu(&animation.pose(doc, t), opts.render).ok_or_else(|| "не удалось создать изображение".to_string())
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jasc_round_trip() {
        let colors = vec![Color::rgb(255, 90, 54), Color::rgb(0, 0, 0)];
        let text = to_jasc(&colors);
        assert!(text.starts_with("JASC-PAL\r\n0100\r\n2\r\n255 90 54\r\n"));
        assert_eq!(parse_jasc(&text).unwrap(), colors);
        assert!(parse_jasc("GIMP Palette").is_err());
    }

    #[test]
    fn quantize_snaps_to_palette() {
        let mut px = Pixmap::new(2, 1).unwrap();
        px.fill(tiny_skia::Color::from_rgba8(250, 10, 10, 255));
        quantize(&mut px, &[Color::rgb(255, 0, 0), Color::rgb(0, 0, 255)]);
        let c = px.pixel(0, 0).unwrap();
        assert_eq!((c.red(), c.green(), c.blue()), (255, 0, 0));
    }
}
