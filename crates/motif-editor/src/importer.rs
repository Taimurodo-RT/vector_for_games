//! Bringing outside pictures in: a PNG reference under the artboard (`backdrop`), SVG files as
//! editable paths, and files dropped onto the window.

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
}

/// The backdrop to draw under the shapes, when the scene has a visible one that loaded.
pub fn backdrop_view<'a>(scene: &motif_script::Scene, cache: &'a BackdropCache) -> Option<BackdropView<'a>> {
    let b = scene.backdrop.as_ref().filter(|b| !b.hidden)?;
    let image = cache.image.as_ref()?;
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
                match std::fs::read(&path).map_err(|e| e.to_string()).and_then(|bytes| Backdrop::from_png(&bytes)) {
                    Ok(img) => {
                        cache.image = Some(img);
                        cache.error = None;
                    }
                    Err(e) => {
                        cache.image = None;
                        cache.error = Some(format!("{}: {e}", path.display()));
                    }
                }
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

    /// Asks for a PNG and sets it as the backdrop, fitted to the artboard width.
    pub(crate) fn pick_backdrop(&mut self) {
        let Some(file) = rfd::FileDialog::new().set_title("Подложка").add_filter("PNG", &["png"]).pick_file() else { return };
        self.set_backdrop_file(&file);
    }

    pub(crate) fn set_backdrop_file(&mut self, file: &Path) {
        let size = std::fs::read(file).map_err(|e| e.to_string()).and_then(|b| Backdrop::from_png(&b)).map(|img| img.size());
        let (w, h) = match size {
            Ok(s) => s,
            Err(e) => {
                self.status = format!("Не удалось открыть {}: {e}", file.display());
                return;
            }
        };
        let ab = &self.scene.doc.artboard;
        // Fit inside the artboard, centred; small pictures keep their size.
        let scale = (ab.width / w as f64).min(ab.height / h as f64).min(1.0);
        let scale = (scale * 1000.0).round() / 1000.0;
        let (x, y) = (((ab.width - w as f64 * scale) / 2.0).round(), ((ab.height - h as f64 * scale) / 2.0).round());
        let old = self.scene.backdrop.clone();
        let b = motif_script::Backdrop {
            file: self.script_name(file),
            x,
            y,
            scale,
            opacity: old.as_ref().map_or(0.5, |o| o.opacity),
            hidden: false,
            span: motif_script::Span::new(0, 0),
        };
        self.write_backdrop(Some(&b));
        self.status = format!("Подложка {} ({w}×{h}) — видна только в редакторе, в экспорт не попадает", b.file);
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

    /// PNG files dropped onto the window become the backdrop, SVG files are imported.
    pub(crate) fn dropped_files(&mut self, ctx: &egui::Context) {
        let files: Vec<PathBuf> = ctx.input(|i| i.raw.dropped_files.iter().map(|f| f.path().to_path_buf()).filter(|p| !p.as_os_str().is_empty()).collect());
        for f in files {
            match f.extension().map(|e| e.to_string_lossy().to_lowercase()).as_deref() {
                Some("svg") => self.import_svg_file(&f),
                Some("png") => self.set_backdrop_file(&f),
                Some("msc") => self.status = "Сцену открывайте через Файл → Открыть".into(),
                _ => self.status = format!("{}: перетащите SVG (импорт) или PNG (подложка)", f.display()),
            }
        }
    }

    /// Backdrop controls in the panel shown when nothing is selected.
    pub(crate) fn backdrop_section(&mut self, ui: &mut egui::Ui) {
        fold(ui, "Подложка", |ui| {
            let Some(b) = self.scene.backdrop.clone() else {
                ui.label(RichText::new("Картинка-референс под фигурами: эскиз, кадр, макет. Видна только в редакторе.").size(12.0).color(FAINT));
                if theme::button(ui, "Загрузить PNG…", ButtonKind::Normal).clicked() {
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
            ui.horizontal(|ui| {
                ui.label(RichText::new("Непрозрачность").size(12.0).color(MUTED));
                ui.spacing_mut().slider_width = (ui.available_width() - 70.0).max(40.0);
                let mut pct = (b.opacity * 100.0).round();
                let slider = egui::Slider::new(&mut pct, 0.0..=100.0).suffix("%").step_by(1.0);
                if ui.add(slider).changed() {
                    next.opacity = pct / 100.0;
                }
            });
            let w = ui.available_width();
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
