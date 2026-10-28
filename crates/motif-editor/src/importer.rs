//! Bringing outside pictures in: a PNG or PSD reference under the artboard (`backdrop`), SVG
//! files as editable paths, and files dropped onto the window.

use crate::app::{App, fold};
use crate::theme::{self, ButtonKind, FAINT, FG, KeyState, MUTED};
use egui::RichText;
use motif_core::kurbo::Affine;
use motif_render::{Backdrop, BackdropView};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// The decoded backdrop picture, reloaded when its file or its modification time changes.
#[derive(Default)]
pub struct BackdropCache {
    key: Option<(PathBuf, Option<SystemTime>)>,
    image: Option<Backdrop>,
    pub error: Option<String>,
    last_check: Option<std::time::Instant>,
    /// The frame of a sprite sheet shown now, cut out of the picture: columns, rows and its index.
    frame: Option<((u32, u32, u32), Backdrop)>,
    /// The layers of a PSD, and the layers hidden and shown when `image` was put together.
    pub layered: Option<crate::psd::Layered>,
    composed: (Vec<String>, Vec<String>),
}

impl BackdropCache {
    /// The whole picture, as loaded.
    pub fn pixmap(&self) -> Option<&tiny_skia::Pixmap> {
        self.image.as_ref().map(|b| b.pixmap.as_ref())
    }
}

/// Reads a backdrop picture: a PNG, or a PSD put together with the named layers hidden and shown.
fn load_picture(path: &Path, hide: &[String], show: &[String]) -> Result<(Backdrop, Option<crate::psd::Layered>), String> {
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    if crate::psd::is_layered(path) {
        let psd = crate::psd::read(&bytes)?;
        let px = psd.compose(hide, show).ok_or("пустая картинка")?;
        Ok((Backdrop::new(px), Some(psd)))
    } else {
        Ok((Backdrop::from_png(&bytes)?, None))
    }
}

/// The backdrop to draw under the shapes, when the scene has a visible one that loaded: the whole
/// picture, or the frame of a sprite sheet shown now.
pub fn backdrop_view<'a>(scene: &motif_script::Scene, cache: &'a BackdropCache) -> Option<BackdropView<'a>> {
    let b = scene.backdrop.as_ref().filter(|b| !b.hidden)?;
    let image = match &b.sheet {
        Some(_) => &cache.frame.as_ref()?.1,
        None => cache.image.as_ref()?,
    };
    Some(BackdropView { image, transform: Affine::translate((b.x, b.y)) * Affine::scale(b.scale), opacity: b.opacity })
}

impl App {
    /// Where a file named in the script lives: next to the script, or as written when absolute.
    fn resolve(&self, file: &str) -> PathBuf {
        let p = PathBuf::from(file);
        match (&self.path, p.is_absolute()) {
            (Some(script), false) => script.parent().map_or(p.clone(), |d| d.join(&p)),
            _ => p,
        }
    }

    /// How a picked file is written in the script: relative to the script's folder when it is
    /// inside it, so the scene and its pictures can be moved together.
    fn script_name(&self, file: &Path) -> String {
        let rel = self.path.as_ref().and_then(|s| s.parent()).and_then(|d| file.strip_prefix(d).ok());
        rel.unwrap_or(file).to_string_lossy().replace('\\', "/")
    }

    /// Loads the backdrop picture, or reloads it when the file changed.
    pub(crate) fn refresh_backdrop(&mut self) {
        let Some(b) = self.scene.backdrop.clone() else { return };
        let path = self.resolve(&b.file);
        // Checking the file time is cheap, but not every frame.
        let check = self.backdrop_img.last_check.is_none_or(|t| t.elapsed().as_millis() > 700) || self.backdrop_img.key.as_ref().is_none_or(|k| k.0 != path);
        if check {
            self.backdrop_img.last_check = Some(std::time::Instant::now());
            let key = (path.clone(), crate::app::mtime(&path));
            if self.backdrop_img.key.as_ref() != Some(&key) {
                let cache = &mut self.backdrop_img;
                cache.key = Some(key);
                cache.frame = None;
                cache.composed = (b.hide.clone(), b.show.clone());
                match load_picture(&path, &b.hide, &b.show) {
                    Ok((img, layered)) => {
                        cache.image = Some(img);
                        cache.layered = layered;
                        cache.error = None;
                    }
                    Err(e) => {
                        (cache.image, cache.layered) = (None, None);
                        cache.error = Some(format!("{}: {e}", path.display()));
                    }
                }
            }
        }
        // Layers of a PSD turned off or on: put the picture together again.
        let cache = &mut self.backdrop_img;
        if let Some(psd) = &cache.layered
            && (cache.composed.0 != b.hide || cache.composed.1 != b.show)
        {
            cache.composed = (b.hide.clone(), b.show.clone());
            if let Some(px) = psd.compose(&b.hide, &b.show) {
                cache.image = Some(Backdrop::new(px));
                cache.frame = None;
            }
        }
        // A sprite sheet shows one frame: the one at the timeline's time while animating.
        if let Some(sheet) = b.sheet {
            let at = self.anim.is_some().then_some(self.time);
            let key = (sheet.cols, sheet.rows, sheet.frame_at(at));
            let cache = &mut self.backdrop_img;
            if cache.frame.as_ref().is_none_or(|(k, _)| *k != key)
                && let Some(px) = cache.pixmap().and_then(|px| crate::sheet::frame(px, key.0, key.1, key.2))
            {
                cache.frame = Some((key, Backdrop::new(px)));
            }
        }
    }

    fn write_backdrop(&mut self, b: Option<&motif_script::Backdrop>) {
        if !self.can_edit() {
            return;
        }
        let out = motif_script::set_backdrop(&self.src, &self.scene, b);
        self.apply("backdrop", Some(out));
    }

    /// Asks for a PNG or PSD and sets it as the backdrop, fitted to the artboard width.
    pub(crate) fn pick_backdrop(&mut self) {
        let Some(file) = rfd::FileDialog::new().set_title("Подложка").add_filter("PNG и PSD", &["png", "psd", "psb"]).pick_file() else { return };
        self.set_backdrop_file(&file);
    }

    pub(crate) fn set_backdrop_file(&mut self, file: &Path) {
        let (image, layered) = match load_picture(file, &[], &[]) {
            Ok(img) => img,
            Err(e) => {
                self.status = format!("Не удалось открыть {}: {e}", file.display());
                return;
            }
        };
        let (w, h) = image.size();
        // Frames set apart by transparent gaps: a sprite sheet, shown frame by frame.
        let grid = crate::sheet::guess(&image.pixmap);
        let sheet = grid.filter(|g| g.gaps).map(|g| motif_script::Sheet { frames: g.frames, ..motif_script::Sheet::new(g.cols, g.rows) });
        let old = self.scene.backdrop.clone();
        let mut b = motif_script::Backdrop {
            file: self.script_name(file),
            x: 0.0,
            y: 0.0,
            scale: 1.0,
            opacity: old.as_ref().map_or(0.5, |o| o.opacity),
            hidden: false,
            sheet,
            hide: Vec::new(),
            show: Vec::new(),
            span: motif_script::Span::new(0, 0),
        };
        self.fit_backdrop(&mut b, (w, h));
        self.write_backdrop(Some(&b));
        let layers = layered.as_ref().map(|l| l.rows().iter().filter(|r| !r.group).count());
        self.status = match (sheet, grid, layers) {
            (_, _, Some(n)) => format!("Подложка {} ({w}×{h}), слоёв: {n} — включайте и выключайте их в разделе «Подложка»", b.file),
            (Some(s), _, _) => format!("Подложка {}: лист спрайтов {}×{}, кадров {} — на таймлайне идёт кадр за кадром", b.file, s.cols, s.rows, s.frames),
            (None, Some(g), _) => format!("Подложка {} ({w}×{h}). Если это лист из {} кадров — «Лист спрайтов» в разделе «Подложка»", b.file, g.frames),
            (None, None, _) => format!("Подложка {} ({w}×{h}) — видна только в редакторе, в экспорт не попадает", b.file),
        };
    }

    /// Scales and centres `b` to fit the artboard: the whole picture, or one frame of a sheet.
    /// Small pictures keep their size.
    fn fit_backdrop(&self, b: &mut motif_script::Backdrop, (w, h): (u32, u32)) {
        let (w, h) = match &b.sheet {
            Some(s) => ((w / s.cols) as f64, (h / s.rows) as f64),
            None => (w as f64, h as f64),
        };
        let ab = &self.scene.doc.artboard;
        let scale = (ab.width / w).min(ab.height / h).min(1.0);
        b.scale = (scale * 1000.0).round() / 1000.0;
        (b.x, b.y) = (((ab.width - w * b.scale) / 2.0).round(), ((ab.height - h * b.scale) / 2.0).round());
    }

    /// The layers of a PSD backdrop, top to bottom, each with its eye.
    fn layer_controls(&mut self, ui: &mut egui::Ui, b: &motif_script::Backdrop, next: &mut motif_script::Backdrop) {
        let Some(psd) = &self.backdrop_img.layered else { return };
        let rows = psd.rows();
        if rows.is_empty() {
            return;
        }
        ui.add_space(4.0);
        ui.label(RichText::new("Слои").size(12.0).color(MUTED));
        if !psd.unreadable.is_empty() {
            ui.label(RichText::new(format!("Не прочитаны, показаны пустыми: {}", psd.unreadable.join(", "))).size(11.5).color(theme::DANGER));
        }
        // Whether the groups around the row are drawn, by depth.
        let mut open: Vec<bool> = Vec::new();
        ui.scope(|ui| {
            ui.spacing_mut().item_spacing.y = 2.0;
            for row in &rows {
                open.truncate(row.depth);
                let props = crate::psd::Props { name: row.name.clone(), visible: row.visible, opacity: 1.0, blend: *b"norm", clipped: false };
                let mut on = crate::psd::shown(&props, &b.hide, &b.show);
                let inside_shown = open.iter().all(|&o| o);
                ui.horizontal(|ui| {
                    ui.add_space(row.depth as f32 * 14.0);
                    let text = RichText::new(&row.name).size(12.0).color(if on && inside_shown { FG } else { MUTED });
                    let text = if row.group { text.strong() } else { text };
                    if ui.checkbox(&mut on, text).on_hover_text(if row.group { "Группа слоёв" } else { "Слой" }).changed() {
                        crate::psd::set_shown(&mut next.hide, &mut next.show, row, on);
                    }
                });
                if row.group {
                    open.push(on);
                }
            }
        });
        if (!b.hide.is_empty() || !b.show.is_empty())
            && theme::button(ui, "Как в файле", ButtonKind::Normal).on_hover_text("Слои видны так, как сохранены в PSD").clicked()
        {
            (next.hide, next.show) = (Vec::new(), Vec::new());
        }
    }

    /// Sprite sheet controls under the backdrop: the grid, the frame rate and the still frame.
    fn sheet_controls(&mut self, ui: &mut egui::Ui, b: &motif_script::Backdrop, next: &mut motif_script::Backdrop) {
        let size = self.backdrop_img.pixmap().map(|p| (p.width(), p.height()));
        let Some(sheet) = b.sheet else {
            let r = theme::button(ui, "Лист спрайтов", ButtonKind::Normal).on_hover_text(
                "Картинка — сетка кадров: подложка покажет по одному кадру, на таймлайне они пойдут по времени. Сетка угадывается по прозрачным промежуткам",
            );
            if r.clicked()
                && let Some(px) = self.backdrop_img.pixmap()
            {
                let grid = crate::sheet::guess(px);
                let (cols, rows, frames) = grid.map_or((1, 1, 1), |g| (g.cols, g.rows, g.frames));
                next.sheet = Some(motif_script::Sheet { frames, ..motif_script::Sheet::new(cols, rows) });
                if let Some(size) = size {
                    self.fit_backdrop(next, size);
                }
                if grid.is_none() {
                    self.status = "Сетку не угадать: укажите колонки и ряды кадров".into();
                }
            }
            return;
        };
        ui.label(RichText::new("Лист спрайтов").size(12.0).color(MUTED));
        let w = ui.available_width();
        let half = (w - 6.0) / 2.0;
        let mut s = sheet;
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            if let Some(v) = theme::num_field(ui, half, "Колонки", s.cols as f64, 0.1, 1.0..=4096.0, 0, KeyState::Hidden).changed {
                s.cols = v as u32;
            }
            if let Some(v) = theme::num_field(ui, half, "Ряды", s.rows as f64, 0.1, 1.0..=4096.0, 0, KeyState::Hidden).changed {
                s.rows = v as u32;
            }
        });
        if (s.cols, s.rows) != (sheet.cols, sheet.rows) {
            // A new grid: every cell is a frame again.
            s.frames = s.cols * s.rows;
        }
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            if let Some(v) = theme::num_field(ui, half, "Кадров", s.frames as f64, 0.1, 1.0..=(s.cols * s.rows) as f64, 0, KeyState::Hidden).changed {
                s.frames = v as u32;
            }
            if let Some(v) = theme::num_field(ui, half, "к/с", s.fps, 0.1, 1.0..=120.0, 1, KeyState::Hidden).changed {
                s.fps = v;
            }
        });
        s.frames = s.frames.clamp(1, s.cols * s.rows);
        // While animating the timeline picks the frame.
        let still = self.anim.is_none();
        let picked =
            ui.add_enabled_ui(still, |ui| theme::num_field(ui, w, "Кадр", s.frame as f64, 0.1, 0.0..=(s.frames - 1) as f64, 0, KeyState::Hidden).changed).inner;
        if let Some(v) = picked {
            s.frame = v as u32;
        }
        s.frame = s.frame.min(s.frames - 1);
        if s != sheet {
            next.sheet = Some(s);
            if (s.cols, s.rows) != (sheet.cols, sheet.rows)
                && let Some(size) = size
            {
                self.fit_backdrop(next, size);
            }
        }
        ui.horizontal_wrapped(|ui| {
            if theme::button(ui, "Угадать сетку", ButtonKind::Normal).on_hover_text("По прозрачным промежуткам между кадрами").clicked()
                && let Some(g) = self.backdrop_img.pixmap().and_then(crate::sheet::guess)
            {
                next.sheet = Some(motif_script::Sheet { frames: g.frames, fps: sheet.fps, ..motif_script::Sheet::new(g.cols, g.rows) });
                if let Some(size) = size {
                    self.fit_backdrop(next, size);
                }
            }
            if theme::button(ui, "Целиком", ButtonKind::Normal).on_hover_text("Показывать картинку целиком, а не по кадру").clicked()
            {
                next.sheet = None;
                if let Some(size) = size {
                    self.fit_backdrop(next, size);
                }
            }
        });
        let hint = if self.anim.is_some() {
            format!(
                "Кадр {} из {} идёт по времени таймлайна, {} к/с: обводите его в Animate.",
                sheet.frame_at(Some(self.time)) + 1,
                sheet.frames,
                motif_script::fmt_num(sheet.fps)
            )
        } else {
            "В Design стоит кадр из поля «Кадр», в Animate кадры идут по времени таймлайна.".to_string()
        };
        ui.label(RichText::new(hint).size(11.5).color(FAINT));
    }

    /// Asks for a TrueType or OpenType file for text `node`.
    pub(crate) fn pick_font(&mut self, node: &str) {
        let Some(file) = rfd::FileDialog::new().set_title("Шрифт").add_filter("Шрифт", &["ttf", "otf"]).pick_file() else { return };
        let text = format!("font \"{}\"", self.script_name(&file));
        self.apply_styles(node, vec![("font", Some(text))]);
    }

    /// Asks for an SVG file and imports it.
    pub(crate) fn pick_svg(&mut self) {
        let Some(file) = rfd::FileDialog::new().set_title("Импорт SVG").add_filter("SVG", &["svg"]).pick_file() else { return };
        self.import_svg_file(&file);
    }

    pub(crate) fn import_svg_file(&mut self, file: &Path) {
        if !self.can_edit() {
            return;
        }
        let name = file.file_stem().map_or("svg".into(), |s| s.to_string_lossy().into_owned());
        let result = std::fs::read(file).map_err(|e| e.to_string()).and_then(|bytes| motif_script::import_svg(&self.src, &self.scene, &bytes, &name));
        match result {
            Ok((out, id)) => {
                self.apply("import", Some(out));
                let count = self.scene.doc.nodes.iter().filter(|n| n.parent.is_some_and(|p| self.scene.doc.nodes[p].id == id)).count();
                self.selected = Some(id);
                self.also.clear();
                self.status = format!("Импортировано фигур: {count}. Текст в SVG нужно заранее перевести в кривые.");
            }
            Err(e) => self.status = format!("Импорт {}: {e}", file.display()),
        }
    }

    /// PNG and PSD files dropped onto the window become the backdrop, SVG files are imported.
    pub(crate) fn dropped_files(&mut self, ctx: &egui::Context) {
        let files: Vec<PathBuf> = ctx.input(|i| i.raw.dropped_files.iter().map(|f| f.path().to_path_buf()).filter(|p| !p.as_os_str().is_empty()).collect());
        for f in files {
            match f.extension().map(|e| e.to_string_lossy().to_lowercase()).as_deref() {
                Some("svg") => self.import_svg_file(&f),
                Some("png" | "psd" | "psb") => self.set_backdrop_file(&f),
                Some("msc") => self.status = "Сцену открывайте через Файл → Открыть".into(),
                _ => self.status = format!("{}: перетащите SVG (импорт), PNG или PSD (подложка)", f.display()),
            }
        }
    }

    /// Backdrop controls in the panel shown when nothing is selected.
    pub(crate) fn backdrop_section(&mut self, ui: &mut egui::Ui) {
        fold(ui, "Подложка", |ui| {
            let Some(b) = self.scene.backdrop.clone() else {
                ui.label(RichText::new("Картинка-референс под фигурами: эскиз, кадр, макет. Видна только в редакторе.").size(12.0).color(FAINT));
                if theme::button(ui, "Загрузить PNG или PSD…", ButtonKind::Normal).clicked() {
                    self.pick_backdrop();
                }
                return;
            };
            let mut next = b.clone();
            ui.horizontal(|ui| {
                let mut shown = !b.hidden;
                if ui.checkbox(&mut shown, "").on_hover_text("Показывать подложку").changed() {
                    next.hidden = !shown;
                }
                ui.label(RichText::new(&b.file).size(12.5).color(if b.hidden { MUTED } else { FG }));
            });
            if let Some(e) = &self.backdrop_img.error {
                ui.label(RichText::new(format!("Не загружена: {e}")).size(11.5).color(theme::DANGER));
            }
            let w = ui.available_width();
            // A field the width of the panel, as the others: a slider beside a label kept
            // nudging the panel's width.
            if let Some(v) = theme::num_field(ui, w, "Непрозрачность, %", (b.opacity * 100.0).round(), 0.5, 0.0..=100.0, 0, KeyState::Hidden).changed
            {
                next.opacity = v.round() / 100.0;
            }
            let half = (w - 6.0) / 2.0;
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 6.0;
                if let Some(v) = theme::num_field(ui, half, "X", b.x, 0.5, -100_000.0..=100_000.0, 1, KeyState::Hidden).changed {
                    next.x = (v * 10.0).round() / 10.0;
                }
                if let Some(v) = theme::num_field(ui, half, "Y", b.y, 0.5, -100_000.0..=100_000.0, 1, KeyState::Hidden).changed {
                    next.y = (v * 10.0).round() / 10.0;
                }
            });
            if let Some(v) = theme::num_field(ui, w, "Масштаб", b.scale, 0.005, 0.001..=100.0, 3, KeyState::Hidden).changed {
                next.scale = v;
            }
            self.layer_controls(ui, &b, &mut next);
            self.sheet_controls(ui, &b, &mut next);
            ui.horizontal_wrapped(|ui| {
                if theme::button(ui, "Заменить…", ButtonKind::Normal).clicked() {
                    self.pick_backdrop();
                }
                if theme::button(ui, "Убрать", ButtonKind::Normal).clicked() {
                    self.write_backdrop(None);
                }
            });
            ui.label(RichText::new("Не попадает в экспорт PNG. Файл ищется рядом со сценой.").size(11.5).color(FAINT));
            if next != b && self.scene.backdrop.as_ref() == Some(&b) {
                self.write_backdrop(Some(&next));
            }
        });
    }
}
