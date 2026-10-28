//! Bridges between Motif and pixel-art tools.
//!
//! * PNG frame sequences that PixaTool can batch-process.
//! * [`godot`]: every animation in one atlas with SpriteFrames, a scene and its state machine.
//! * [`pixatool`]: drives PixaTool itself, so frames come back pixelized with no dialogs.
//! * JASC `.pal` palettes (the format PixaTool loads, up to 192 colors).
//! * A quick pixel preview: render at the target size without anti-aliasing, then snap to a palette.

pub mod godot;
pub mod kit;
pub mod pixatool;
pub mod runtime;
pub mod screen;
pub mod video;

use motif_core::{Color, Document, Value};
use motif_render::{CpuOptions, render_cpu};
use tiny_skia::Pixmap;

/// PixaTool refuses palettes larger than this.
pub const PIXATOOL_MAX_COLORS: usize = 192;

/// Every distinct opaque color used by the scene: background, fills, strokes and animated colors.
pub fn scene_palette(doc: &Document) -> Vec<Color> {
    boards_palette([doc])
}

/// [`scene_palette`] of several boards together, in board order.
pub fn boards_palette<'a>(docs: impl IntoIterator<Item = &'a Document>) -> Vec<Color> {
    let mut out: Vec<Color> = Vec::new();
    let mut add = |c: Color| {
        let c = Color { a: 255, ..c };
        if !out.contains(&c) {
            out.push(c);
        }
    };
    for doc in docs {
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

/// How many frames `animation` has at `fps`: a looping one leaves out its last frame, which
/// equals the first.
pub fn frame_count(animation: &motif_core::Animation, fps: f64) -> usize {
    let count = (animation.duration * fps).round() as usize;
    (if animation.looping { count } else { count + 1 }).max(1)
}

/// Renders every frame of an animation. Looping animations skip the last frame, which equals the first.
pub fn render_frames(doc: &Document, anim: &str, opts: &FrameExport) -> Result<Vec<Pixmap>, String> {
    render_frames_facing(doc, anim, opts, None)
}

/// [`render_frames`] from one side of the scene's `directions` (by number): the turned objects
/// face it, and `anim_side` plays instead of `anim` where the scene has one.
pub fn render_frames_facing(doc: &Document, anim: &str, opts: &FrameExport, face: Option<usize>) -> Result<Vec<Pixmap>, String> {
    let side = face.zip(doc.directions.as_ref()).and_then(|(k, d)| d.anim_for(doc, anim, k));
    let animation = side.or_else(|| doc.animation(anim)).ok_or_else(|| format!("нет анимации «{anim}»"))?;
    if opts.fps <= 0.0 {
        return Err("fps должен быть больше нуля".into());
    }
    // Soft bodies are simulated once for all frames.
    let mut sim = motif_core::SimCache::default();
    (0..frame_count(animation, opts.fps))
        .map(|i| {
            let t = (i as f64 / opts.fps).min(animation.duration);
            render_cpu(&animation.pose_facing(doc, t, &mut sim, face), opts.render).ok_or_else(|| "не удалось создать изображение".to_string())
        })
        .collect()
}

/// The side of the scene's `directions` named `name` (`right`, `up_left`…), by number.
pub fn side(doc: &Document, name: &str) -> Result<usize, String> {
    let d = doc.directions.as_ref().ok_or("в сцене нет строки directions")?;
    d.find(name).ok_or_else(|| format!("нет стороны «{name}»: {}", d.names().join(", ")))
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
