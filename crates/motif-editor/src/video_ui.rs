//! «Видео» in the export window: the open animation as one MP4, WebM, GIF or APNG file, drawn
//! and encoded on another thread. GIF and APNG are written by Motif, MP4 and WebM by ffmpeg.

use crate::app::App;
use crate::theme::{self, ButtonKind, FAINT, MUTED};
use egui::RichText;
use motif_pixel::video::{self, Progress, VideoFormat, VideoOptions};
use motif_render::CpuOptions;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{Receiver, channel};

pub struct VideoExport {
    pub format: VideoFormat,
    pub fps: f64,
    /// Keep see-through pixels where the format can (WebM, GIF, APNG).
    pub transparent: bool,
    /// The ffmpeg found or picked, looked up again when the pick changes.
    found: Option<PathBuf>,
    job: Option<Job>,
    /// The last video saved: its file (a folder with every board) and the line that said so.
    saved: Option<(PathBuf, String)>,
    /// The last machine run recorded with «Запись», ready to be saved again in another format.
    pub last_take: Option<motif_core::Take>,
}

/// An export running on another thread.
struct Job {
    done: Receiver<Result<(PathBuf, String), String>>,
    progress: Arc<Progress>,
}

impl VideoExport {
    pub fn new(picked: Option<PathBuf>) -> VideoExport {
        VideoExport {
            format: VideoFormat::Mp4,
            fps: 30.0,
            transparent: false,
            found: video::locate_ffmpeg(picked.as_deref()),
            job: None,
            saved: None,
            last_take: None,
        }
    }

    /// The status line once the running export finishes.
    pub fn poll(&mut self) -> Option<String> {
        let r = self.job.as_ref()?.done.try_recv().ok()?;
        self.job = None;
        Some(match r {
            Ok((path, status)) => {
                self.saved = Some((path, status.clone()));
                status
            }
            Err(e) if e.ends_with(video::CANCELLED) => "Видео отменено: файл не сохранён".into(),
            Err(e) => format!("Видео не сохранено: {e}"),
        })
    }

    pub fn busy(&self) -> bool {
        self.job.is_some()
    }

    /// Starts `work` on another thread; its progress is shown until it sends the saved file.
    fn start(&mut self, ctx: &egui::Context, work: impl FnOnce(&Arc<Progress>) -> Result<(PathBuf, String), String> + Send + 'static) {
        let (tx, rx) = channel();
        let progress = Progress::new();
        let shared = progress.clone();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(work(&shared));
            ctx.request_repaint();
        });
        self.saved = None;
        self.job = Some(Job { done: rx, progress });
    }

    /// «Кадр 12 из 60» (and the file, with several), or None when nothing is being saved.
    fn progress_text(&self) -> Option<(f32, String)> {
        let p = &self.job.as_ref()?.progress;
        let (done, total) = p.frames();
        let mut text = if total == 0 {
            "Готовлю кадры…".to_string()
        } else if done >= total {
            "Дописываю файл…".to_string()
        } else {
            format!("Кадр {} из {total}", done + 1)
        };
        if let Some((i, n)) = p.file() {
            text = format!("Файл {i} из {n} · {text}");
        }
        Some((p.fraction(), text))
    }

    /// The bar, the frame it is on and «Отменить», while a video is being saved. `wide` in the
    /// export window, narrow in the status line.
    pub fn progress_ui(&mut self, ui: &mut egui::Ui, wide: bool) {
        let Some((fraction, text)) = self.progress_text() else { return };
        // The frames move on another thread: keep the bar moving with them.
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(100));
        let bar = egui::ProgressBar::new(fraction).fill(theme::ACCENT).corner_radius(3.0).desired_height(6.0);
        ui.add_sized([if wide { 140.0 } else { 120.0 }, 6.0], bar);
        let text = if wide { RichText::new(text).size(12.0) } else { RichText::new(format!("Видео: {text}")).font(theme::mono(11.0)) };
        ui.label(text.color(MUTED));
        let cancelled = self.job.as_ref().is_some_and(|j| j.progress.cancelled());
        let label = if cancelled { "Отменяю…" } else { "Отменить" };
        let r = ui.add_enabled_ui(!cancelled, |ui| if wide { theme::button(ui, label, ButtonKind::Normal) } else { link(ui, label) }).inner;
        if r.on_hover_text("Остановить запись видео; недописанный файл удалится").clicked()
            && let Some(j) = &self.job
        {
            j.progress.cancel();
        }
    }

    /// «Открыть папку» for the video just saved, while the status line still talks about it
    /// (`status`), or always in the export window (`status` None). Says why when the folder
    /// cannot be shown.
    pub fn reveal_ui(&self, ui: &mut egui::Ui, status: Option<&str>) -> Option<String> {
        let (path, said) = self.saved.as_ref()?;
        if self.job.is_some() || status.is_some_and(|s| s != said) {
            return None;
        }
        let r = if status.is_some() { link(ui, "Открыть папку") } else { theme::button(ui, "Открыть папку", ButtonKind::Normal) };
        if r.on_hover_text(path.display().to_string()).clicked() {
            return reveal(path).err().map(|e| format!("Не удалось открыть папку: {e}"));
        }
        None
    }
}

/// A button as small as the status line's text.
fn link(ui: &mut egui::Ui, text: &str) -> egui::Response {
    let r = ui.add(egui::Label::new(RichText::new(text).font(theme::mono(11.0)).color(theme::ACCENT)).sense(egui::Sense::click()));
    if r.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    r
}

/// Shows `path` in the file manager: the file picked out in its folder where the system can, the
/// folder itself otherwise.
pub(crate) fn reveal(path: &Path) -> std::io::Result<()> {
    #[cfg(windows)]
    let spawned = {
        use std::os::windows::process::CommandExt;
        let mut cmd = std::process::Command::new("explorer");
        if path.is_file() {
            // explorer wants the quotes inside the one argument: /select,"C:\a b\c.mp4".
            cmd.raw_arg(format!("/select,\"{}\"", path.display()));
        } else {
            cmd.arg(path);
        }
        cmd.spawn()
    };
    #[cfg(target_os = "macos")]
    let spawned = {
        let mut cmd = std::process::Command::new("open");
        if path.is_file() {
            cmd.arg("-R");
        }
        cmd.arg(path).spawn()
    };
    #[cfg(not(any(windows, target_os = "macos")))]
    let spawned = {
        let dir = if path.is_file() { path.parent().unwrap_or(path) } else { path };
        std::process::Command::new("xdg-open").arg(dir).spawn()
    };
    spawned.map(|_| ())
}

impl App {
    pub(crate) fn video_section(&mut self, ui: &mut egui::Ui) {
        theme::caption(ui, "Видео");
        ui.horizontal(|ui| {
            for f in VideoFormat::ALL {
                let kind = if self.video.format == f { ButtonKind::On } else { ButtonKind::Normal };
                let hint = match f {
                    VideoFormat::Mp4 => "H.264: маленький файл, играет везде; без прозрачности. Через ffmpeg",
                    VideoFormat::Webm => "VP9: как MP4, но с прозрачностью — для сайтов и OBS. Через ffmpeg",
                    VideoFormat::Gif => "256 цветов, прозрачность вкл/выкл: для чатов, вики и Steam",
                    VideoFormat::Apng => "Анимированный PNG: все цвета и мягкая прозрачность, играет в браузерах",
                };
                if theme::button(ui, f.label(), kind).on_hover_text(hint).clicked() {
                    self.video.format = f;
                }
            }
        });
        let format = self.video.format;
        egui::Grid::new("video").num_columns(2).spacing([12.0, 8.0]).show(ui, |ui| {
            ui.label("Кадров в секунду");
            ui.add(egui::DragValue::new(&mut self.video.fps).range(1.0..=60.0));
            ui.end_row();
            if format.needs_ffmpeg() {
                ui.label("ffmpeg");
                ui.horizontal(|ui| {
                    let shown = self.video.found.as_deref().map_or("не найден".to_string(), crate::pixabridge::short);
                    let hover = self.video.found.as_ref().map_or("Укажите ffmpeg.exe".into(), |p| p.display().to_string());
                    if theme::button(ui, &shown, ButtonKind::Normal).on_hover_text(hover).clicked()
                        && let Some(p) = rfd::FileDialog::new().set_title("ffmpeg").pick_file()
                    {
                        self.video.found = video::locate_ffmpeg(Some(&p));
                        self.pixa.ffmpeg = Some(p);
                        self.pixa.save();
                    }
                    if self.pixa.ffmpeg.is_some() && theme::button(ui, "×", ButtonKind::Normal).on_hover_text("Искать рядом с Motif и в PATH").clicked()
                    {
                        self.pixa.ffmpeg = None;
                        self.pixa.save();
                        self.video.found = video::locate_ffmpeg(None);
                    }
                });
                ui.end_row();
            }
        });
        ui.add_enabled(format.keeps_alpha(), egui::Checkbox::new(&mut self.video.transparent, "Прозрачный фон"))
            .on_disabled_hover_text("В MP4 нет прозрачности: под ней будет цвет фона холста");
        if format.needs_ffmpeg() && self.video.found.is_none() {
            ui.label(
                RichText::new("Для MP4 и WebM нужен ffmpeg: скачайте его с ffmpeg.org (для Windows — сборка gyan.dev) и положите ffmpeg.exe рядом с motif.exe или укажите выше. GIF и APNG Motif пишет сам.")
                    .size(12.0)
                    .color(MUTED),
            );
        }
        let all = self.board_count() > 1 && self.export.all_boards;
        // The animation the timeline shows, in Design as well.
        let anim = self.shown_anim().map(|a| a.name);
        let busy = self.video.busy();
        let label = match (&anim, all) {
            (Some(a), true) => format!("Сохранить видео… («{a}» со всех холстов)"),
            (Some(a), false) => format!("Сохранить видео… («{a}»)"),
            (None, _) => "Сохранить видео…".to_string(),
        };
        let ready = anim.is_some() && !busy && !(format.needs_ffmpeg() && self.video.found.is_none());
        let r = ui.add_enabled_ui(ready, |ui| theme::button(ui, &label, ButtonKind::Primary)).inner;
        let r = if anim.is_none() {
            r.on_disabled_hover_text("В сцене нет анимаций: добавьте её на таймлайне (+)")
        } else {
            r
        };
        if r.clicked()
            && let Some(a) = anim
        {
            let ctx = ui.ctx().clone();
            self.export_video(&a, all, &ctx);
        }
        if let Some(take) = &self.video.last_take {
            let label = format!("Сохранить прогон «{}» ({} с)…", take.machine, motif_script::fmt_num((take.length * 10.0).round() / 10.0));
            let ready = !busy && !(format.needs_ffmpeg() && self.video.found.is_none());
            let r = ui.add_enabled_ui(ready, |ui| theme::button(ui, &label, ButtonKind::Normal)).inner;
            if r.on_hover_text("Последняя запись машины состояний («Запись» на панели машины) в выбранном формате").clicked()
            {
                let ctx = ui.ctx().clone();
                self.save_take(&ctx);
            }
        }
        if busy {
            ui.horizontal(|ui| self.video.progress_ui(ui, true));
        } else if let Some((path, _)) = &self.video.saved {
            let name = path.file_name().map_or(String::new(), |n| n.to_string_lossy().into_owned());
            ui.horizontal(|ui| {
                ui.label(RichText::new(format!("Сохранено: {name}")).size(12.0).color(MUTED));
                if let Some(e) = self.video.reveal_ui(ui, None) {
                    self.status = e;
                }
            });
        }
        ui.label(
            RichText::new("Масштаб и сглаживание — как у кадров PNG выше. Повторяющаяся анимация в GIF и APNG крутится по кругу. Прогон машины записывает «Запись» на панели машины состояний.")
                .size(12.0)
                .color(FAINT),
        );
    }

    /// Asks where to save the last recorded machine run, then plays it again frame by frame and
    /// encodes it on another thread.
    pub(crate) fn save_take(&mut self, ctx: &egui::Context) {
        let Some(take) = self.video.last_take.clone() else { return };
        let Some(m) = self.scene.doc.machine(&take.machine).cloned() else {
            self.status = format!("Прогон не сохранён: машины «{}» больше нет", take.machine);
            return;
        };
        let format = self.video.format;
        if format.needs_ffmpeg() && self.video.found.is_none() {
            self.status = "Для MP4 и WebM нужен ffmpeg: укажите его в «Экспорт → Видео» или выберите там GIF или APNG".into();
            return;
        }
        let ext = format.extension();
        let name = format!("{}_{}.{ext}", self.file_stem(), take.machine);
        let dialog = rfd::FileDialog::new().set_title("Сохранить прогон машины").set_file_name(&name).add_filter(format.label(), &[ext]);
        let Some(mut path) = dialog.save_file() else { return };
        if VideoFormat::from_path(&path) != Some(format) {
            path.set_extension(ext);
        }
        let transparent = self.video.transparent && format.keeps_alpha();
        let render = CpuOptions { scale: self.export.scale, antialias: self.export.antialias, background: !transparent };
        let (doc, fps, ffmpeg) = (self.scene.doc.clone(), self.video.fps, self.video.found.clone());
        // From the side the canvas shows, as the run was seen.
        let side = self.shown_side();
        let length = take.length;
        self.video.start(ctx, move |progress| {
            let opts = VideoOptions { fps, ffmpeg, progress: Some(progress.clone()), ..Default::default() };
            let r = video::export_take(&doc, &m, &take, side, render, opts, &path).map_err(|e| format!("{}: {e}", path.display()))?;
            let status = format!("Прогон «{}» {}: {} кадров {}×{} → {}", m.name, r.format.label(), r.frames, r.width, r.height, path.display());
            Ok((path, status))
        });
        self.status = format!("Прогон {} с: рисую и кодирую {}…", motif_script::fmt_num((length * 10.0).round() / 10.0), format.label());
    }

    /// Asks where to save, then draws and encodes on another thread: one file, or with every
    /// board a folder of `BOARD_ANIM.ext` from each board that has the animation.
    fn export_video(&mut self, anim: &str, all: bool, ctx: &egui::Context) {
        let format = self.video.format;
        let ext = format.extension();
        let boards = match self.export_boards() {
            Ok(b) => b.into_iter().filter(|(_, d)| d.animation(anim).is_some()).collect::<Vec<_>>(),
            Err(e) => {
                self.status = format!("Видео не сохранено: {e}");
                return;
            }
        };
        if boards.is_empty() {
            self.status = format!("Видео не сохранено: нет анимации «{anim}»");
            return;
        }
        let jobs: Vec<(PathBuf, motif_core::Document)> = if all {
            let Some(dir) = rfd::FileDialog::new().set_title("Папка для видео").pick_folder() else { return };
            boards.into_iter().map(|(board, doc)| (dir.join(format!("{board}_{anim}.{ext}")), doc)).collect()
        } else {
            let name = format!("{}_{anim}.{ext}", self.file_stem());
            let dialog = rfd::FileDialog::new().set_title("Сохранить видео").set_file_name(&name).add_filter(format.label(), &[ext]);
            let Some(mut path) = dialog.save_file() else { return };
            if VideoFormat::from_path(&path) != Some(format) {
                path.set_extension(ext);
            }
            boards.into_iter().take(1).map(|(_, doc)| (path.clone(), doc)).collect()
        };
        let transparent = self.video.transparent && format.keeps_alpha();
        let render = CpuOptions { scale: self.export.scale, antialias: self.export.antialias, background: !transparent };
        let (fps, ffmpeg, anim) = (self.video.fps, self.video.found.clone(), anim.to_string());
        // From the side the canvas shows, when the scene has sides.
        let side = self.shown_side();
        let anim_name = anim.clone();
        self.video.start(ctx, move |progress| {
            let mut last = None;
            for (i, (path, doc)) in jobs.iter().enumerate() {
                progress.begin_file(i, jobs.len());
                let opts = VideoOptions { fps, ffmpeg: ffmpeg.clone(), progress: Some(progress.clone()), ..Default::default() };
                let r = video::export_animation(doc, &anim, side, render, opts, path).map_err(|e| format!("{}: {e}", path.display()))?;
                last = Some((path.clone(), r));
            }
            match (jobs.len(), last) {
                (1, Some((path, r))) => {
                    let status = format!("Видео {}: {} кадров {}×{} → {}", r.format.label(), r.frames, r.width, r.height, path.display());
                    Ok((path, status))
                }
                (n, Some((path, r))) => {
                    let dir = path.parent().map_or(path.clone(), Path::to_path_buf);
                    Ok((dir.clone(), format!("Видео {}: файлов {n} в {}", r.format.label(), dir.display())))
                }
                (_, None) => Err("нечего сохранять".into()),
            }
        });
        self.status = format!("Видео {}: рисую и кодирую «{anim_name}»…", format.label());
    }
}
