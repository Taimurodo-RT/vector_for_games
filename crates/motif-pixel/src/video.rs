//! An animation as one video file: GIF and APNG are written here, MP4 and WebM by ffmpeg, which
//! gets the frames through a pipe. The format comes from the file's extension.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use tiny_skia::Pixmap;

/// The kinds of file an animation can be saved as.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VideoFormat {
    /// 256 colours a frame, on/off transparency: plays everywhere, from chats to wikis.
    Gif,
    /// Animated PNG: every colour and soft transparency, played by browsers.
    Apng,
    /// H.264: small and everywhere, no transparency (the background colour fills it).
    Mp4,
    /// VP9: like MP4, with transparency.
    Webm,
}

impl VideoFormat {
    pub const ALL: [VideoFormat; 4] = [VideoFormat::Mp4, VideoFormat::Webm, VideoFormat::Gif, VideoFormat::Apng];

    pub fn from_path(path: &Path) -> Option<VideoFormat> {
        let ext = path.extension()?.to_string_lossy().to_lowercase();
        Some(match ext.as_str() {
            "gif" => VideoFormat::Gif,
            "apng" | "png" => VideoFormat::Apng,
            "mp4" | "m4v" => VideoFormat::Mp4,
            "webm" => VideoFormat::Webm,
            _ => return None,
        })
    }

    pub fn extension(self) -> &'static str {
        match self {
            VideoFormat::Gif => "gif",
            VideoFormat::Apng => "png",
            VideoFormat::Mp4 => "mp4",
            VideoFormat::Webm => "webm",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            VideoFormat::Gif => "GIF",
            VideoFormat::Apng => "APNG",
            VideoFormat::Mp4 => "MP4",
            VideoFormat::Webm => "WebM",
        }
    }

    /// Whether ffmpeg encodes it.
    pub fn needs_ffmpeg(self) -> bool {
        matches!(self, VideoFormat::Mp4 | VideoFormat::Webm)
    }

    /// Whether the file keeps transparent pixels.
    pub fn keeps_alpha(self) -> bool {
        self != VideoFormat::Mp4
    }
}

/// How a video is written.
#[derive(Clone, Debug)]
pub struct VideoOptions {
    pub fps: f64,
    /// Plays over and over (GIF and APNG; MP4 and WebM leave it to the player).
    pub looping: bool,
    /// Behind transparent pixels where the format has no transparency: the artboard colour.
    pub background: [u8; 3],
    /// ffmpeg for MP4 and WebM; found on PATH or next to Motif when not given.
    pub ffmpeg: Option<PathBuf>,
}

impl Default for VideoOptions {
    fn default() -> Self {
        VideoOptions { fps: 30.0, looping: true, background: [0, 0, 0], ffmpeg: None }
    }
}

/// What [`export_animation`] wrote.
#[derive(Clone, Copy, Debug)]
pub struct VideoReport {
    pub format: VideoFormat,
    pub frames: usize,
    pub width: u32,
    pub height: u32,
}

/// Animation `anim` of `doc` drawn at `fps` and saved as the video `path` names: it loops when the
/// animation does, and the artboard colour stands behind transparency the format cannot keep.
/// `face` shoots it from one side of the scene's `directions`.
pub fn export_animation(
    doc: &motif_core::Document,
    anim: &str,
    face: Option<usize>,
    render: motif_render::CpuOptions,
    fps: f64,
    ffmpeg: Option<PathBuf>,
    path: &Path,
) -> Result<VideoReport, String> {
    let looping = doc.animation(anim).ok_or_else(|| format!("нет анимации «{anim}»"))?.looping;
    let frames = crate::render_frames_facing(doc, anim, &crate::FrameExport { fps, render }, face)?;
    let bg = doc.artboard.background;
    let opts = VideoOptions { fps, looping, background: [bg.r, bg.g, bg.b], ffmpeg };
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    let format = write_video(path, &frames, &opts)?;
    Ok(VideoReport { format, frames: frames.len(), width: frames[0].width(), height: frames[0].height() })
}

/// A recorded run of machine `m` ([`motif_core::Take`]) played again at `fps` and saved as the
/// video `path` names. It plays once: a run has a start and an end.
pub fn export_take(
    doc: &motif_core::Document,
    m: &motif_core::Machine,
    take: &motif_core::Take,
    render: motif_render::CpuOptions,
    fps: f64,
    ffmpeg: Option<PathBuf>,
    path: &Path,
) -> Result<VideoReport, String> {
    if fps.is_nan() || fps <= 0.0 {
        return Err("fps должен быть больше нуля".into());
    }
    if m.states.is_empty() {
        return Err(format!("в машине «{}» нет состояний", m.name));
    }
    let frames = take
        .poses(m, doc, fps)
        .iter()
        .map(|pose| motif_render::render_cpu(pose, render).ok_or_else(|| "не удалось создать изображение".to_string()))
        .collect::<Result<Vec<_>, _>>()?;
    let bg = doc.artboard.background;
    let opts = VideoOptions { fps, looping: false, background: [bg.r, bg.g, bg.b], ffmpeg };
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    let format = write_video(path, &frames, &opts)?;
    Ok(VideoReport { format, frames: frames.len(), width: frames[0].width(), height: frames[0].height() })
}

/// Saves `frames` (all the same size) as the video `path` names by its extension.
pub fn write_video(path: &Path, frames: &[Pixmap], opts: &VideoOptions) -> Result<VideoFormat, String> {
    let format = VideoFormat::from_path(path).ok_or_else(|| format!("{}: формат видео по расширению — .mp4, .webm, .gif или .png (APNG)", path.display()))?;
    let first = frames.first().ok_or("нет кадров")?;
    if frames.iter().any(|f| f.width() != first.width() || f.height() != first.height()) {
        return Err("кадры разного размера".into());
    }
    if opts.fps.is_nan() || opts.fps <= 0.0 {
        return Err("fps должен быть больше нуля".into());
    }
    match format {
        VideoFormat::Gif => write_gif(path, frames, opts),
        VideoFormat::Apng => write_apng(path, frames, opts),
        VideoFormat::Mp4 | VideoFormat::Webm => {
            let exe = locate_ffmpeg(opts.ffmpeg.as_deref())
                .ok_or("для MP4 и WebM нужен ffmpeg: положите ffmpeg.exe рядом с Motif, добавьте его в PATH или укажите путь (ffmpeg.org → Download)")?;
            write_ffmpeg(&exe, path, frames, format, opts)
        }
    }
    .map(|_| format)
}

/// Straight (not premultiplied) RGBA of a frame; without `keep_alpha` it lies on `background`.
fn rgba(px: &Pixmap, keep_alpha: bool, background: [u8; 3]) -> Vec<u8> {
    let mut out = Vec::with_capacity(px.data().len());
    for p in px.pixels() {
        let a = p.alpha();
        if keep_alpha {
            let c = p.demultiply();
            out.extend_from_slice(&[c.red(), c.green(), c.blue(), a]);
        } else {
            // Premultiplied colour plus the background where the frame lets it through.
            let under = |c: u8, b: u8| (c as u32 + (b as u32 * (255 - a as u32) + 127) / 255).min(255) as u8;
            out.extend_from_slice(&[under(p.red(), background[0]), under(p.green(), background[1]), under(p.blue(), background[2]), 255]);
        }
    }
    out
}

/// Frame `i`'s length in `unit`ths of a second, rounded so the lengths add up to the true time.
fn delay(i: usize, fps: f64, unit: f64) -> u16 {
    let at = |k: usize| (k as f64 * unit / fps).round();
    (at(i + 1) - at(i)).clamp(1.0, u16::MAX as f64) as u16
}

fn write_gif(path: &Path, frames: &[Pixmap], opts: &VideoOptions) -> Result<(), String> {
    let (w, h) = (frames[0].width(), frames[0].height());
    if w > u16::MAX as u32 || h > u16::MAX as u32 {
        return Err("GIF не бывает больше 65535 точек".into());
    }
    let file = std::fs::File::create(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut enc = gif::Encoder::new(std::io::BufWriter::new(file), w as u16, h as u16, &[]).map_err(|e| e.to_string())?;
    if opts.looping {
        enc.set_repeat(gif::Repeat::Infinite).map_err(|e| e.to_string())?;
    }
    for (i, px) in frames.iter().enumerate() {
        let mut data = rgba(px, true, opts.background);
        // Half-transparent edges cannot be half in a GIF: they keep their colour, fully shown.
        for p in data.as_chunks_mut::<4>().0 {
            p[3] = if p[3] < 128 { 0 } else { 255 };
        }
        // Frames of up to 256 colours (pixel art) keep them exactly; others get the best 256.
        let mut frame = gif::Frame::from_rgba_speed(w as u16, h as u16, &mut data, 5);
        frame.delay = delay(i, opts.fps, 100.0);
        frame.dispose = gif::DisposalMethod::Background;
        enc.write_frame(&frame).map_err(|e| format!("{}: {e}", path.display()))?;
    }
    // The trailer and the last bytes, with their errors seen rather than lost in a drop.
    enc.into_inner().and_then(|mut w| w.flush()).map_err(|e| format!("{}: {e}", path.display()))
}

fn write_apng(path: &Path, frames: &[Pixmap], opts: &VideoOptions) -> Result<(), String> {
    let (w, h) = (frames[0].width(), frames[0].height());
    let file = std::fs::File::create(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut enc = png::Encoder::new(std::io::BufWriter::new(file), w, h);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    let err = |e: png::EncodingError| format!("{}: {e}", path.display());
    enc.set_animated(frames.len() as u32, if opts.looping { 0 } else { 1 }).map_err(err)?;
    let mut writer = enc.write_header().map_err(err)?;
    for (i, px) in frames.iter().enumerate() {
        writer.set_frame_delay(delay(i, opts.fps, 1000.0), 1000).map_err(err)?;
        writer.write_image_data(&rgba(px, true, opts.background)).map_err(err)?;
    }
    writer.finish().map_err(err)
}

/// ffmpeg: the given path, else next to this program, else on PATH.
pub fn locate_ffmpeg(given: Option<&Path>) -> Option<PathBuf> {
    let name = if cfg!(windows) { "ffmpeg.exe" } else { "ffmpeg" };
    if let Some(p) = given {
        return p.is_file().then(|| p.to_path_buf()).or_else(|| p.is_dir().then(|| p.join(name)).filter(|p| p.is_file()));
    }
    let near = std::env::current_exe().ok().and_then(|e| e.parent().map(|d| d.join(name))).filter(|p| p.is_file());
    near.or_else(|| std::env::var_os("PATH").and_then(|paths| std::env::split_paths(&paths).map(|d| d.join(name)).find(|p| p.is_file())))
}

fn write_ffmpeg(exe: &Path, path: &Path, frames: &[Pixmap], format: VideoFormat, opts: &VideoOptions) -> Result<(), String> {
    let (w, h) = (frames[0].width(), frames[0].height());
    let alpha = format.keeps_alpha();
    let mut cmd = Command::new(exe);
    cmd.args(["-hide_banner", "-loglevel", "error", "-y", "-f", "rawvideo", "-pix_fmt", "rgba", "-s"])
        .arg(format!("{w}x{h}"))
        .arg("-r")
        .arg(format!("{}", opts.fps))
        .args(["-i", "-"])
        // 4:2:0 colour needs even sides: an odd one gets one more row or column.
        .args(["-vf", "pad=ceil(iw/2)*2:ceil(ih/2)*2"]);
    match format {
        VideoFormat::Mp4 => cmd.args(["-c:v", "libx264", "-pix_fmt", "yuv420p", "-crf", "16", "-preset", "slow", "-movflags", "+faststart"]),
        _ => cmd.args(["-c:v", "libvpx-vp9", "-pix_fmt", if alpha { "yuva420p" } else { "yuv420p" }, "-b:v", "0", "-crf", "24", "-row-mt", "1"]),
    };
    cmd.arg(path).stdin(Stdio::piped()).stdout(Stdio::null()).stderr(Stdio::piped());
    #[cfg(windows)]
    {
        // No console window flashing up from the editor.
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000);
    }
    let mut child = cmd.spawn().map_err(|e| format!("{}: {e}", exe.display()))?;
    let mut stdin = child.stdin.take().ok_or("ffmpeg не принял кадры")?;
    let fed = frames.iter().try_for_each(|px| stdin.write_all(&rgba(px, alpha, opts.background)));
    drop(stdin);
    let out = child.wait_with_output().map_err(|e| e.to_string())?;
    if !out.status.success() {
        let said = String::from_utf8_lossy(&out.stderr);
        let said = said.trim();
        return Err(format!("ffmpeg: {}", if said.is_empty() { "ошибка без описания" } else { said }));
    }
    fed.map_err(|e| format!("ffmpeg: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frames(n: usize) -> Vec<Pixmap> {
        (0..n)
            .map(|i| {
                let mut px = Pixmap::new(9, 7).unwrap();
                px.fill(tiny_skia::Color::from_rgba8(0, 0, 0, 0));
                let mut paint = tiny_skia::Paint::default();
                paint.set_color_rgba8(255, 90, 54, 255);
                px.fill_rect(tiny_skia::Rect::from_xywh(i as f32, 1.0, 3.0, 3.0).unwrap(), &paint, tiny_skia::Transform::identity(), None);
                px
            })
            .collect()
    }

    fn dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("motif-video-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn delays_add_up_to_the_true_time() {
        // 30 fps in hundredths: 3, 4, 3, 3, 4, 3… — 30 frames make exactly one second.
        let total: u32 = (0..30).map(|i| delay(i, 30.0, 100.0) as u32).sum();
        assert_eq!(total, 100);
        assert_eq!((0..12).map(|i| delay(i, 12.0, 1000.0) as u32).sum::<u32>(), 1000);
    }

    #[test]
    fn a_gif_has_every_frame_and_loops() {
        let d = dir("gif");
        let path = d.join("a.gif");
        assert_eq!(write_video(&path, &frames(5), &VideoOptions { fps: 10.0, ..Default::default() }), Ok(VideoFormat::Gif));
        let mut opts = gif::DecodeOptions::new();
        opts.set_color_output(gif::ColorOutput::RGBA);
        let mut dec = opts.read_info(std::fs::File::open(&path).unwrap()).unwrap();
        let mut n = 0;
        while let Some(f) = dec.read_next_frame().unwrap() {
            assert_eq!(f.delay, 10);
            // The square moves one pixel a frame; the corner stays see-through.
            let at = |x: usize, y: usize| &f.buffer[(y * 9 + x) * 4..][..4];
            assert_eq!(at(n + 1, 2), [255, 90, 54, 255]);
            assert_eq!(at(8, 6)[3], 0);
            n += 1;
        }
        assert_eq!(n, 5);
        assert_eq!(dec.repeat(), gif::Repeat::Infinite);
        std::fs::remove_dir_all(d).ok();
    }

    #[test]
    fn an_apng_has_every_frame() {
        let d = dir("apng");
        let path = d.join("a.png");
        write_video(&path, &frames(4), &VideoOptions { fps: 8.0, looping: false, ..Default::default() }).unwrap();
        let dec = png::Decoder::new(std::io::BufReader::new(std::fs::File::open(&path).unwrap()));
        let reader = dec.read_info().unwrap();
        let ctl = reader.info().animation_control.unwrap();
        assert_eq!((ctl.num_frames, ctl.num_plays), (4, 1));
        std::fs::remove_dir_all(d).ok();
    }

    #[test]
    fn mp4_and_webm_go_through_ffmpeg() {
        let Some(exe) = locate_ffmpeg(None) else { return };
        let d = dir("ffmpeg");
        for name in ["a.mp4", "a.webm"] {
            let path = d.join(name);
            // 9×7: odd sides are padded for 4:2:0.
            write_video(&path, &frames(6), &VideoOptions { fps: 12.0, ffmpeg: Some(exe.clone()), ..Default::default() }).unwrap();
            assert!(std::fs::metadata(&path).unwrap().len() > 100, "{name}");
        }
        std::fs::remove_dir_all(d).ok();
    }

    #[test]
    fn without_ffmpeg_the_error_says_where_to_get_it() {
        let d = dir("noff");
        let e = write_video(&d.join("a.mp4"), &frames(2), &VideoOptions { ffmpeg: Some(d.join("nope")), ..Default::default() }).unwrap_err();
        assert!(e.contains("ffmpeg"), "{e}");
        assert!(write_video(&d.join("a.avi"), &frames(2), &VideoOptions::default()).is_err());
        std::fs::remove_dir_all(d).ok();
    }

    #[test]
    fn mp4_lays_see_through_pixels_on_the_background() {
        let px = &frames(1)[0];
        let flat = rgba(px, false, [10, 20, 30]);
        assert_eq!(&flat[..4], [10, 20, 30, 255]);
        let kept = rgba(px, true, [10, 20, 30]);
        assert_eq!(kept[3], 0);
    }
}
