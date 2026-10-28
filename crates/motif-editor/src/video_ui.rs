//! «Видео» in the export window: the open animation as one MP4, WebM, GIF or APNG file, drawn
//! and encoded on another thread. GIF and APNG are written by Motif, MP4 and WebM by ffmpeg.

use crate::app::App;
use crate::theme::{self, ButtonKind, FAINT, MUTED};
use egui::RichText;
use motif_pixel::video::{self, VideoFormat};
use motif_render::CpuOptions;
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, channel};

pub struct VideoExport {
    pub format: VideoFormat,
    pub fps: f64,
    /// Keep see-through pixels where the format can (WebM, GIF, APNG).
    pub transparent: bool,
    /// The ffmpeg found or picked, looked up again when the pick changes.
    found: Option<PathBuf>,
    job: Option<Receiver<Result<String, String>>>,
    /// The last machine run recorded with «Запись», ready to be saved again in another format.
    pub last_take: Option<motif_core::Take>,
}

impl VideoExport {
    pub fn new(picked: Option<PathBuf>) -> VideoExport {
        VideoExport { format: VideoFormat::Mp4, fps: 30.0, transparent: false, found: video::locate_ffmpeg(picked.as_deref()), job: None, last_take: None }
    }

    /// The status line once the running export finishes.
    pub fn poll(&mut self) -> Option<String> {
        let r = self.job.as_ref()?.try_recv().ok()?;
        self.job = None;
        Some(r.unwrap_or_else(|e| format!("Видео не сохранено: {e}")))
    }
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
        let busy = self.video.job.is_some();
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
            ui.add(egui::Spinner::new());
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
        let (tx, rx) = channel();
        let ctx = ctx.clone();
        let length = take.length;
        std::thread::spawn(move || {
            let r = video::export_take(&doc, &m, &take, render, fps, ffmpeg, &path)
                .map(|r| format!("Прогон «{}» {}: {} кадров {}×{} → {}", m.name, r.format.label(), r.frames, r.width, r.height, path.display()))
                .map_err(|e| format!("{}: {e}", path.display()));
            let _ = tx.send(r);
            ctx.request_repaint();
        });
        self.video.job = Some(rx);
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
        let anim_name = anim.clone();
        let (tx, rx) = channel();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let mut last = None;
            let result = jobs.iter().try_for_each(|(path, doc)| {
                let r = video::export_animation(doc, &anim, render, fps, ffmpeg.clone(), path).map_err(|e| format!("{}: {e}", path.display()))?;
                last = Some((path.clone(), r));
                Ok::<(), String>(())
            });
            let status = result.map(|()| match (jobs.len(), last) {
                (1, Some((path, r))) => format!("Видео {}: {} кадров {}×{} → {}", r.format.label(), r.frames, r.width, r.height, path.display()),
                (n, Some((path, r))) => {
                    format!("Видео {}: файлов {n} в {}", r.format.label(), path.parent().map_or(String::new(), |d| d.display().to_string()))
                }
                (_, None) => "Видео: нечего сохранять".to_string(),
            });
            let _ = tx.send(status);
            ctx.request_repaint();
        });
        self.video.job = Some(rx);
        self.status = format!("Видео {}: рисую и кодирую «{anim_name}»…", format.label());
    }
}
