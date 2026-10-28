//! An animation as one video file: GIF and APNG are written here, MP4 and WebM by ffmpeg, which
//! gets the frames through a pipe. The format comes from the file's extension.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
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
    /// Where the export reports its frames and hears a request to stop.
    pub progress: Option<Arc<Progress>>,
}

impl Default for VideoOptions {
    fn default() -> Self {
        VideoOptions { fps: 30.0, looping: true, background: [0, 0, 0], ffmpeg: None, progress: None }
    }
}

/// What an export running on another thread shares with the window watching it: how many frames
/// are written, of how many, in which file of how many, and a request to stop.
#[derive(Debug, Default)]
pub struct Progress {
    done: AtomicUsize,
    total: AtomicUsize,
    file: AtomicUsize,
    files: AtomicUsize,
    cancel: AtomicBool,
}

/// The error of an export stopped with [`Progress::cancel`]; nothing is left of its file.
pub const CANCELLED: &str = "отменено";

impl Progress {
    pub fn new() -> Arc<Progress> {
        Arc::new(Progress::default())
    }

    /// Asks the export to stop at the next frame.
    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }

    pub fn cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }

    /// Frames written and frames in the video being written now.
    pub fn frames(&self) -> (usize, usize) {
        (self.done.load(Ordering::Relaxed), self.total.load(Ordering::Relaxed))
    }

    /// The file being written (from 1) and how many there are, when there is more than one.
    pub fn file(&self) -> Option<(usize, usize)> {
        let files = self.files.load(Ordering::Relaxed);
        (files > 1).then(|| (self.file.load(Ordering::Relaxed) + 1, files))
    }

    /// The part of the whole job done, 0 to 1, files included.
    pub fn fraction(&self) -> f32 {
        let (done, total) = self.frames();
        let files = self.files.load(Ordering::Relaxed).max(1);
        let file = self.file.load(Ordering::Relaxed).min(files - 1);
        ((file as f32 + done as f32 / total.max(1) as f32) / files as f32).clamp(0.0, 1.0)
    }

    /// The next file of `files` begins (`index` from 0).
    pub fn begin_file(&self, index: usize, files: usize) {
        self.file.store(index, Ordering::Relaxed);
        self.files.store(files, Ordering::Relaxed);
        self.done.store(0, Ordering::Relaxed);
        self.total.store(0, Ordering::Relaxed);
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

/// Animation `anim` of `doc` drawn at `opts.fps` and saved as the video `path` names: it loops
/// when the animation does, and the artboard colour stands behind transparency the format cannot
/// keep. `face` shoots it from one side of the scene's `directions`. Each frame is drawn just
/// before it is encoded, so a long video never sits in memory.
pub fn export_animation(
    doc: &motif_core::Document,
    anim: &str,
    face: Option<usize>,
    render: motif_render::CpuOptions,
    opts: VideoOptions,
    path: &Path,
) -> Result<VideoReport, String> {
    // From a side, `Walk_up` plays instead of `Walk` where the scene has it.
    let side = face.zip(doc.directions.as_ref()).and_then(|(k, d)| d.anim_for(doc, anim, k));
    let animation = side.or_else(|| doc.animation(anim)).ok_or_else(|| format!("нет анимации «{anim}»"))?;
    if opts.fps.is_nan() || opts.fps <= 0.0 {
        return Err("fps должен быть больше нуля".into());
    }
    let bg = doc.artboard.background;
    let opts = VideoOptions { looping: animation.looping, background: [bg.r, bg.g, bg.b], ..opts };
    let count = crate::frame_count(animation, opts.fps);
    // Soft bodies are simulated once, frame after frame.
    let mut sim = motif_core::SimCache::default();
    let fps = opts.fps;
    let mut draw = |i: usize| {
        let t = (i as f64 / fps).min(animation.duration);
        motif_render::render_cpu(&animation.pose_facing(doc, t, &mut sim, face), render).ok_or_else(|| "не удалось создать изображение".to_string())
    };
    encode_to(path, count, &mut draw, &opts)
}

/// A recorded run of machine `m` ([`motif_core::Take`]) played again at `opts.fps` and saved as
/// the video `path` names. It plays once: a run has a start and an end. `face` shoots it from one
/// side of the scene's `directions`.
pub fn export_take(
    doc: &motif_core::Document,
    m: &motif_core::Machine,
    take: &motif_core::Take,
    face: Option<usize>,
    render: motif_render::CpuOptions,
    opts: VideoOptions,
    path: &Path,
) -> Result<VideoReport, String> {
    if opts.fps.is_nan() || opts.fps <= 0.0 {
        return Err("fps должен быть больше нуля".into());
    }
    if m.states.is_empty() {
        return Err(format!("в машине «{}» нет состояний", m.name));
    }
    let bg = doc.artboard.background;
    let opts = VideoOptions { looping: false, background: [bg.r, bg.g, bg.b], ..opts };
    // From a side every state plays its side animation (`Walk_up`), turned to face it.
    let m = face.map_or_else(|| m.clone(), |k| m.facing(doc, k));
    let mut poses = take.poses(&m, doc, opts.fps);
    if let Some(k) = face.filter(|_| doc.directions.is_some()) {
        for pose in &mut poses {
            pose.face(k);
            pose.rig();
        }
    }
    let mut draw = |i: usize| motif_render::render_cpu(&poses[i], render).ok_or_else(|| "не удалось создать изображение".to_string());
    encode_to(path, poses.len(), &mut draw, &opts)
}

/// Makes the folder of `path`, then encodes `count` frames drawn by `draw`.
fn encode_to(path: &Path, count: usize, draw: &mut dyn FnMut(usize) -> Result<Pixmap, String>, opts: &VideoOptions) -> Result<VideoReport, String> {
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    let mut feed = Feed::new(count, draw, opts)?;
    let (width, height) = feed.size;
    let format = encode(path, &mut feed, opts)?;
    Ok(VideoReport { format, frames: count, width, height })
}

/// Saves `frames` (all the same size) as the video `path` names by its extension.
pub fn write_video(path: &Path, frames: &[Pixmap], opts: &VideoOptions) -> Result<VideoFormat, String> {
    let mut draw = |i: usize| Ok(frames[i].clone());
    let mut feed = Feed::new(frames.len(), &mut draw, opts)?;
    encode(path, &mut feed, opts)
}

/// Frames handed to an encoder one at a time, in order: each is drawn when its turn comes, is
/// checked against the size of the first, counted in the progress, and a stop request ends it.
struct Feed<'a> {
    count: usize,
    size: (u32, u32),
    first: Option<Pixmap>,
    draw: &'a mut dyn FnMut(usize) -> Result<Pixmap, String>,
    progress: Option<Arc<Progress>>,
}

impl<'a> Feed<'a> {
    fn new(count: usize, draw: &'a mut dyn FnMut(usize) -> Result<Pixmap, String>, opts: &VideoOptions) -> Result<Feed<'a>, String> {
        if count == 0 {
            return Err("нет кадров".into());
        }
        if let Some(p) = &opts.progress {
            p.total.store(count, Ordering::Relaxed);
            p.done.store(0, Ordering::Relaxed);
        }
        let first = draw(0)?;
        Ok(Feed { count, size: (first.width(), first.height()), first: Some(first), draw, progress: opts.progress.clone() })
    }

    /// Frame `i`, asked for in order from 0.
    fn frame(&mut self, i: usize) -> Result<Pixmap, String> {
        if self.progress.as_ref().is_some_and(|p| p.cancelled()) {
            return Err(CANCELLED.into());
        }
        let px = match self.first.take() {
            Some(px) if i == 0 => px,
            _ => (self.draw)(i)?,
        };
        if (px.width(), px.height()) != self.size {
            return Err("кадры разного размера".into());
        }
        if let Some(p) = &self.progress {
            p.done.store(i + 1, Ordering::Relaxed);
        }
        Ok(px)
    }
}

fn encode(path: &Path, feed: &mut Feed, opts: &VideoOptions) -> Result<VideoFormat, String> {
    let format = VideoFormat::from_path(path).ok_or_else(|| format!("{}: формат видео по расширению — .mp4, .webm, .gif или .png (APNG)", path.display()))?;
    if opts.fps.is_nan() || opts.fps <= 0.0 {
        return Err("fps должен быть больше нуля".into());
    }
    if format == VideoFormat::Gif && (feed.size.0 > u16::MAX as u32 || feed.size.1 > u16::MAX as u32) {
        return Err("GIF не бывает больше 65535 точек".into());
    }
    // The video is written next to `path` and takes its place only when it is whole: a file that
    // was there survives an encoder that fails to start, fails halfway or is stopped.
    let part = part_path(path);
    let written = match format {
        VideoFormat::Gif => write_gif(&part, feed, opts),
        VideoFormat::Apng => write_apng(&part, feed, opts),
        VideoFormat::Mp4 | VideoFormat::Webm => {
            let exe = locate_ffmpeg(opts.ffmpeg.as_deref())
                .ok_or("для MP4 и WebM нужен ffmpeg: положите ffmpeg.exe рядом с Motif, добавьте его в PATH или укажите путь (ffmpeg.org → Download)")?;
            write_ffmpeg(&exe, &part, feed, format, opts)
        }
    }
    .and_then(|()| std::fs::rename(&part, path).map_err(|e| format!("{}: {e}", path.display())));
    if written.is_err() {
        // Half a video is no use to anyone.
        let _ = std::fs::remove_file(&part);
    }
    written.map(|()| format)
}

/// `.NAME.part.EXT` next to `path`: hidden, and with the extension ffmpeg reads the format from.
fn part_path(path: &Path) -> PathBuf {
    let stem = path.file_stem().map_or_else(String::new, |s| s.to_string_lossy().into_owned());
    let ext = path.extension().map_or_else(String::new, |e| e.to_string_lossy().into_owned());
    path.with_file_name(format!(".{stem}.part.{ext}"))
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

fn write_gif(path: &Path, feed: &mut Feed, opts: &VideoOptions) -> Result<(), String> {
    let (w, h) = feed.size;
    let file = std::fs::File::create(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut enc = gif::Encoder::new(std::io::BufWriter::new(file), w as u16, h as u16, &[]).map_err(|e| e.to_string())?;
    if opts.looping {
        enc.set_repeat(gif::Repeat::Infinite).map_err(|e| e.to_string())?;
    }
    for i in 0..feed.count {
        let mut data = rgba(&feed.frame(i)?, true, opts.background);
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

fn write_apng(path: &Path, feed: &mut Feed, opts: &VideoOptions) -> Result<(), String> {
    let (w, h) = feed.size;
    let file = std::fs::File::create(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut enc = png::Encoder::new(std::io::BufWriter::new(file), w, h);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    let err = |e: png::EncodingError| format!("{}: {e}", path.display());
    enc.set_animated(feed.count as u32, if opts.looping { 0 } else { 1 }).map_err(err)?;
    let mut writer = enc.write_header().map_err(err)?;
    for i in 0..feed.count {
        let data = rgba(&feed.frame(i)?, true, opts.background);
        writer.set_frame_delay(delay(i, opts.fps, 1000.0), 1000).map_err(err)?;
        writer.write_image_data(&data).map_err(err)?;
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

fn write_ffmpeg(exe: &Path, path: &Path, feed: &mut Feed, format: VideoFormat, opts: &VideoOptions) -> Result<(), String> {
    let (w, h) = feed.size;
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
    let mut fed = Ok(());
    for i in 0..feed.count {
        let px = match feed.frame(i) {
            Ok(px) => px,
            Err(e) => {
                // Stopped or a frame failed: ffmpeg is not left half done in the background.
                drop(stdin);
                let _ = child.kill();
                let _ = child.wait();
                return Err(e);
            }
        };
        if let Err(e) = stdin.write_all(&rgba(&px, alpha, opts.background)) {
            fed = Err(e);
            break;
        }
    }
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
            // Stopped halfway: ffmpeg is ended and its half-written file removed.
            let stop = Progress::new();
            let opts = VideoOptions { fps: 12.0, ffmpeg: Some(exe.clone()), progress: Some(stop.clone()), ..Default::default() };
            let mut draw = |i: usize| {
                if i == 3 {
                    stop.cancel();
                }
                Ok(frames(1).remove(0))
            };
            let before = std::fs::read(&path).unwrap();
            assert_eq!(encode_to(&path, 8, &mut draw, &opts).unwrap_err(), CANCELLED);
            assert_eq!(std::fs::read(&path).unwrap(), before, "{name}: the finished video stays");
            assert!(!part_path(&path).exists(), "{name}");
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
    fn progress_counts_frames_and_a_stop_leaves_no_file() {
        let d = dir("progress");
        let progress = Progress::new();
        let opts = VideoOptions { fps: 10.0, progress: Some(progress.clone()), ..Default::default() };
        write_video(&d.join("a.gif"), &frames(5), &opts).unwrap();
        assert_eq!(progress.frames(), (5, 5));
        assert_eq!(progress.fraction(), 1.0);
        progress.begin_file(1, 2);
        assert_eq!((progress.file(), progress.fraction()), (Some((2, 2)), 0.5));

        // Stopped while the third frame is being drawn.
        let stop = Progress::new();
        let opts = VideoOptions { fps: 10.0, progress: Some(stop.clone()), ..Default::default() };
        let mut draw = |i: usize| {
            if i == 2 {
                stop.cancel();
            }
            Ok(frames(1).remove(0))
        };
        for name in ["b.gif", "b.png"] {
            stop.cancel.store(false, Ordering::Relaxed);
            let path = d.join(name);
            assert_eq!(encode_to(&path, 6, &mut draw, &opts).unwrap_err(), CANCELLED);
            assert_eq!(stop.frames(), (3, 6), "{name}");
            assert!(!path.exists(), "{name}: a stopped video leaves nothing behind");
        }
        // A file that was there is kept when the video cannot even start.
        std::fs::write(d.join("c.mp4"), b"old").unwrap();
        assert!(write_video(&d.join("c.mp4"), &frames(2), &VideoOptions { ffmpeg: Some(d.join("nope")), ..Default::default() }).is_err());
        assert_eq!(std::fs::read(d.join("c.mp4")).unwrap(), b"old");
        // …nor when ffmpeg is there but will not run (a damaged ffmpeg.exe).
        let broken = d.join(if cfg!(windows) { "ffmpeg.exe" } else { "ffmpeg" });
        std::fs::write(&broken, b"not a program").unwrap();
        assert!(write_video(&d.join("c.mp4"), &frames(2), &VideoOptions { ffmpeg: Some(broken), ..Default::default() }).is_err());
        assert_eq!(std::fs::read(d.join("c.mp4")).unwrap(), b"old");
        // …nor when a video over it is stopped; one that is finished replaces it.
        for name in ["c.gif", "c.png"] {
            let path = d.join(name);
            std::fs::write(&path, b"old").unwrap();
            stop.cancel.store(false, Ordering::Relaxed);
            assert_eq!(encode_to(&path, 6, &mut draw, &opts).unwrap_err(), CANCELLED);
            assert_eq!(std::fs::read(&path).unwrap(), b"old", "{name}");
            write_video(&path, &frames(3), &VideoOptions { fps: 10.0, ..Default::default() }).unwrap();
            assert_ne!(std::fs::read(&path).unwrap(), b"old", "{name}");
        }
        let left: Vec<_> =
            std::fs::read_dir(&d).unwrap().filter_map(|e| e.ok()).map(|e| e.file_name()).filter(|n| n.to_string_lossy().contains(".part.")).collect();
        assert!(left.is_empty(), "{left:?}");
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
