use crate::canvas::{Canvas, View, artboard_rect};
use egui::{Color32, Key, Modifiers, Pos2, Rect, RichText, Sense, Stroke, TextureHandle, TextureOptions};
use motif_core::kurbo::{Affine, Point, Shape};
use motif_core::{Color, Document, Prop, ShapeKind, Value};
use motif_pixel::{FrameExport, PIXATOOL_MAX_COLORS, parse_jasc, quantize, render_frames, scene_palette, to_jasc};
use motif_render::{CpuOptions, render_cpu};
use motif_script::{Scene, ScriptError, parse, set_prop};
use std::path::PathBuf;
use std::time::{Instant, SystemTime};

const NEW_SCENE: &str = "artboard \"Сцена\" 320x240 bg #f4f1ff\n\nrect bg at 160 200 size 280 16 r 8 fill #2a2e39\nellipse hero at 160 120 size 64 fill #ff5a36 stroke #1b1d24 3\n\nanim Idle 1s loop {\n  hero.y 0: 120 ease-in-out | 0.5: 100 ease-in-out | 1: 120\n}\n";
const EXAMPLE: &str = include_str!("../../../examples/bounce.msc");
const SELECT: Color32 = Color32::from_rgb(90, 169, 255);
const ACCENT: Color32 = Color32::from_rgb(244, 166, 42);

struct Drag {
    node: String,
    /// Pointer position (artboard space) when the drag started.
    from: Point,
    /// Node origin in artboard space when the drag started.
    origin: Point,
    /// Artboard space to the node's parent space.
    to_parent: Affine,
}

struct PixelPreview {
    on: bool,
    width: u32,
    palette: Option<(String, Vec<Color>)>,
    texture: Option<TextureHandle>,
}

struct ExportSettings {
    fps: f64,
    scale: f64,
    antialias: bool,
    transparent: bool,
}

pub struct App {
    path: Option<PathBuf>,
    src: String,
    saved_src: String,
    disk_mtime: Option<SystemTime>,
    scene: Scene,
    errors: Vec<ScriptError>,
    selected: Option<String>,
    hovered: Option<String>,
    view: Option<View>,
    anim: Option<String>,
    time: f64,
    playing: bool,
    undo: Vec<String>,
    redo: Vec<String>,
    last_checkpoint: (String, Instant),
    drag: Option<Drag>,
    panning: bool,
    pixel: PixelPreview,
    export: ExportSettings,
    canvas: Canvas,
    status: String,
    last_poll: Instant,
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>, path: Option<PathBuf>) -> Self {
        cc.egui_ctx.set_visuals(egui::Visuals::dark());
        let (src, status) = match &path {
            Some(p) => match std::fs::read_to_string(p) {
                Ok(s) => (s, format!("Открыт {}", p.display())),
                Err(e) => (NEW_SCENE.to_string(), format!("Не удалось открыть {}: {e}", p.display())),
            },
            None => (EXAMPLE.to_string(), "Открыт пример. Сохраните его, чтобы работать с файлом.".into()),
        };
        let scene = parse(&src).or_else(|_| parse(NEW_SCENE)).expect("built-in scene parses");
        let mut app = App {
            disk_mtime: path.as_ref().and_then(|p| mtime(p)),
            path,
            saved_src: src.clone(),
            src,
            scene,
            errors: Vec::new(),
            selected: None,
            hovered: None,
            view: None,
            anim: None,
            time: 0.0,
            playing: false,
            undo: Vec::new(),
            redo: Vec::new(),
            last_checkpoint: (String::new(), Instant::now()),
            drag: None,
            panning: false,
            pixel: PixelPreview { on: false, width: 96, palette: None, texture: None },
            export: ExportSettings { fps: 12.0, scale: 1.0, antialias: true, transparent: true },
            canvas: Canvas::new(),
            status,
            last_poll: Instant::now(),
        };
        app.reparse();
        app
    }

    fn reparse(&mut self) {
        match parse(&self.src) {
            Ok(scene) => {
                self.scene = scene;
                self.errors.clear();
                if let Some(id) = &self.selected
                    && self.scene.doc.node(id).is_none()
                {
                    self.selected = None;
                }
                if let Some(a) = &self.anim
                    && self.scene.doc.animation(a).is_none()
                {
                    self.anim = None;
                }
            }
            Err(errors) => self.errors = errors,
        }
    }

    /// Saves an undo step, merging rapid changes of the same kind (typing, dragging) into one.
    fn checkpoint(&mut self, kind: &str) {
        if self.last_checkpoint.0 != kind || self.last_checkpoint.1.elapsed().as_secs_f32() > 0.8 {
            self.undo.push(self.src.clone());
            if self.undo.len() > 300 {
                self.undo.remove(0);
            }
            self.redo.clear();
        }
        self.last_checkpoint = (kind.to_string(), Instant::now());
    }

    fn edit(&mut self, node: &str, prop: Prop, value: Value) {
        if !self.errors.is_empty() {
            self.status = "Сначала исправьте ошибки в скрипте: правки мышью пишутся в рабочую версию".into();
            return;
        }
        if let Some(new_src) = set_prop(&self.src, &self.scene, node, prop, value) {
            self.checkpoint(&format!("{node}.{}", prop.path_name()));
            self.src = new_src;
            self.reparse();
        }
    }

    fn undo(&mut self) {
        if let Some(prev) = self.undo.pop() {
            self.redo.push(std::mem::replace(&mut self.src, prev));
            self.last_checkpoint.0.clear();
            self.reparse();
        }
    }

    fn redo(&mut self) {
        if let Some(next) = self.redo.pop() {
            self.undo.push(std::mem::replace(&mut self.src, next));
            self.last_checkpoint.0.clear();
            self.reparse();
        }
    }

    fn dirty(&self) -> bool {
        self.src != self.saved_src
    }

    fn display_doc(&self) -> Document {
        match self.anim.as_deref().and_then(|a| self.scene.doc.animation(a)) {
            Some(a) => a.pose(&self.scene.doc, self.time),
            None => self.scene.doc.clone(),
        }
    }

    // ---------- files ----------

    fn open(&mut self) {
        if self.dirty() {
            self.status = "Есть несохранённые правки: сохраните их перед открытием другого файла".into();
            return;
        }
        let Some(p) = rfd::FileDialog::new().add_filter("Сцена Motif", &["msc"]).pick_file() else { return };
        match std::fs::read_to_string(&p) {
            Ok(s) => {
                self.undo.push(std::mem::replace(&mut self.src, s.clone()));
                self.saved_src = s;
                self.disk_mtime = mtime(&p);
                self.status = format!("Открыт {}", p.display());
                self.path = Some(p);
                self.selected = None;
                self.view = None;
                self.reparse();
            }
            Err(e) => self.status = format!("Не удалось открыть {}: {e}", p.display()),
        }
    }

    fn save(&mut self, save_as: bool) {
        let path = match (&self.path, save_as) {
            (Some(p), false) => p.clone(),
            _ => match rfd::FileDialog::new().add_filter("Сцена Motif", &["msc"]).set_file_name("scene.msc").save_file() {
                Some(p) => p,
                None => return,
            },
        };
        match std::fs::write(&path, &self.src) {
            Ok(()) => {
                self.saved_src = self.src.clone();
                self.disk_mtime = mtime(&path);
                self.status = format!("Сохранено: {}", path.display());
                self.path = Some(path);
            }
            Err(e) => self.status = format!("Не удалось сохранить {}: {e}", path.display()),
        }
    }

    /// Picks up changes made to the file in another editor (VS Code, Notepad++…).
    fn poll_disk(&mut self) {
        if self.last_poll.elapsed().as_millis() < 500 {
            return;
        }
        self.last_poll = Instant::now();
        let Some(path) = self.path.clone() else { return };
        let now = mtime(&path);
        if now.is_none() || now == self.disk_mtime {
            return;
        }
        self.disk_mtime = now;
        if self.dirty() {
            self.status = "Файл изменён другой программой, но здесь есть несохранённые правки — оставляю версию редактора".into();
            return;
        }
        if let Ok(s) = std::fs::read_to_string(&path)
            && s != self.src
        {
            self.undo.push(std::mem::replace(&mut self.src, s.clone()));
            self.saved_src = s;
            self.reparse();
            self.status = "Файл обновлён с диска".into();
        }
    }

    fn export_frames(&mut self) {
        let Some(dir) = rfd::FileDialog::new().set_title("Папка для PNG-кадров").pick_folder() else { return };
        let render = CpuOptions { scale: self.export.scale, antialias: self.export.antialias, background: !self.export.transparent };
        let doc = &self.scene.doc;
        let (name, frames) = match &self.anim {
            Some(a) => (a.clone(), render_frames(doc, a, &FrameExport { fps: self.export.fps, render })),
            None => ("scene".to_string(), render_cpu(doc, render).map(|p| vec![p]).ok_or_else(|| "не удалось создать изображение".to_string())),
        };
        let result = frames.and_then(|frames| {
            for (i, px) in frames.iter().enumerate() {
                let path = dir.join(format!("{name}_{i:04}.png"));
                px.save_png(&path).map_err(|e| format!("{}: {e}", path.display()))?;
            }
            Ok(frames.len())
        });
        self.status = match result {
            Ok(n) => format!("Сохранено кадров: {n} в {}", dir.display()),
            Err(e) => format!("Экспорт не удался: {e}"),
        };
    }

    fn export_palette(&mut self) {
        let colors = scene_palette(&self.scene.doc);
        let Some(path) = rfd::FileDialog::new().add_filter("Палитра JASC", &["pal"]).set_file_name("palette.pal").save_file() else { return };
        self.status = match std::fs::write(&path, to_jasc(&colors)) {
            Ok(()) => format!("Палитра на {} цветов сохранена: {}", colors.len(), path.display()),
            Err(e) => format!("Не удалось сохранить палитру: {e}"),
        };
    }

    fn load_palette(&mut self) {
        let Some(path) = rfd::FileDialog::new().add_filter("Палитра JASC", &["pal"]).pick_file() else { return };
        match std::fs::read_to_string(&path).map_err(|e| e.to_string()).and_then(|t| parse_jasc(&t)) {
            Ok(colors) => {
                let name = path.file_name().map_or("палитра".into(), |n| n.to_string_lossy().into_owned());
                self.status = format!("Палитра {name}: {} цветов", colors.len());
                self.pixel.palette = Some((name, colors));
                self.pixel.on = true;
            }
            Err(e) => self.status = format!("Не удалось прочитать палитру: {e}"),
        }
    }

    // ---------- panels ----------

    fn top_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new("Motif").strong().color(ACCENT));
            ui.separator();
            if ui.button("Новая").clicked() {
                if self.dirty() {
                    self.status = "Есть несохранённые правки: сохраните их перед созданием новой сцены".into();
                } else {
                    self.checkpoint("file");
                    self.src = NEW_SCENE.into();
                    self.saved_src.clear();
                    self.path = None;
                    self.view = None;
                    self.reparse();
                }
            }
            if ui.button("Открыть…").on_hover_text("Ctrl+O").clicked() {
                self.open();
            }
            if ui.button("Сохранить").on_hover_text("Ctrl+S").clicked() {
                self.save(false);
            }
            if ui.button("Сохранить как…").clicked() {
                self.save(true);
            }
            let name = self.path.as_ref().and_then(|p| p.file_name()).map_or("без имени.msc".into(), |n| n.to_string_lossy().into_owned());
            ui.label(RichText::new(format!("{name}{}", if self.dirty() { " *" } else { "" })).weak());
            ui.separator();
            if ui.button("Вписать").on_hover_text("F").clicked() {
                self.view = None;
            }
            ui.separator();
            ui.toggle_value(&mut self.pixel.on, "Пиксельное превью")
                .on_hover_text("Рендер в низком разрешении без сглаживания с привязкой к палитре — быстрая проверка перед PixaTool/PixelOver");
            if self.pixel.on {
                ui.add(egui::DragValue::new(&mut self.pixel.width).range(8..=1024).suffix(" px"));
            }
            ui.menu_button("Пиксели и экспорт", |ui| {
                ui.label(RichText::new("Кадры PNG для PixaTool и PixelOver").strong());
                ui.horizontal(|ui| {
                    ui.label("Кадров в секунду");
                    ui.add(egui::DragValue::new(&mut self.export.fps).range(1.0..=60.0));
                });
                ui.horizontal(|ui| {
                    ui.label("Масштаб");
                    ui.add(egui::DragValue::new(&mut self.export.scale).range(0.1..=8.0).speed(0.05));
                });
                ui.checkbox(&mut self.export.antialias, "Сглаживание краёв");
                ui.checkbox(&mut self.export.transparent, "Прозрачный фон");
                let what = self.anim.as_deref().map_or("Текущий кадр сцены".to_string(), |a| format!("Анимация «{a}»"));
                if ui.button(format!("Сохранить кадры… ({what})")).clicked() {
                    ui.close();
                    self.export_frames();
                }
                ui.separator();
                if ui.button(format!("Палитра сцены в .pal… (до {PIXATOOL_MAX_COLORS} цветов, для PixaTool)")).clicked() {
                    ui.close();
                    self.export_palette();
                }
                if ui.button("Загрузить палитру .pal для превью…").clicked() {
                    ui.close();
                    self.load_palette();
                }
                if self.pixel.palette.is_some() && ui.button("Вернуть палитру сцены").clicked() {
                    self.pixel.palette = None;
                }
            });
        });
    }

    fn script_panel(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.heading("Скрипт сцены");
            if let Some(id) = &self.selected
                && let Some(site) = self.scene.sites.get(id)
            {
                ui.label(RichText::new(format!("«{id}» — строка {}", site.line)).weak());
            }
        });
        let errors_h = if self.errors.is_empty() { 22.0 } else { 22.0 + 18.0 * self.errors.len().min(5) as f32 };
        egui::ScrollArea::both().max_height(ui.available_height() - errors_h).show(ui, |ui| {
            let before = self.src.clone();
            // Code lines never wrap: the horizontal scroll bar shows instead.
            let mut layouter = |ui: &egui::Ui, text: &dyn egui::TextBuffer, _wrap: f32| {
                let job = egui::text::LayoutJob::simple(text.as_str().to_owned(), egui::FontId::monospace(13.0), ui.visuals().text_color(), f32::INFINITY);
                ui.painter().layout_job(job)
            };
            let resp = ui.add(
                egui::TextEdit::multiline(&mut self.src).code_editor().desired_width(f32::INFINITY).desired_rows(30).lock_focus(true).layouter(&mut layouter),
            );
            if resp.changed() {
                let after = std::mem::replace(&mut self.src, before);
                self.checkpoint("text");
                self.src = after;
                self.reparse();
            }
        });
        if self.errors.is_empty() {
            ui.label(RichText::new("✔ Скрипт без ошибок").color(Color32::from_rgb(79, 211, 140)));
        } else {
            ui.label(RichText::new(format!("Ошибок: {} — на холсте последняя рабочая версия", self.errors.len())).color(Color32::from_rgb(255, 107, 91)));
            for e in self.errors.iter().take(5) {
                ui.label(RichText::new(e.to_string()).small().color(Color32::from_rgb(255, 150, 140)));
            }
        }
    }

    fn layers_panel(&mut self, ui: &mut egui::Ui) {
        ui.heading("Слои");
        let doc = &self.scene.doc;
        ui.label(RichText::new(format!("{} · {}×{}", doc.artboard.name, doc.artboard.width, doc.artboard.height)).weak());
        let mut clicked = None;
        egui::ScrollArea::vertical().id_salt("layers").max_height(ui.available_height() * 0.45).show(ui, |ui| {
            for (i, n) in doc.nodes.iter().enumerate() {
                let depth = doc.depth(i);
                let icon = match n.kind {
                    ShapeKind::Group => "▣",
                    ShapeKind::Rect => "⬜",
                    ShapeKind::Ellipse => "○",
                    ShapeKind::Star => "☆",
                    ShapeKind::Polygon => "⬟",
                };
                let mut text = RichText::new(format!("{}{icon} {}", "    ".repeat(depth), n.display_name()));
                if !doc.is_visible(i) {
                    text = text.weak();
                }
                let sel = self.selected.as_deref() == Some(n.id.as_str());
                if ui.selectable_label(sel, text).on_hover_text(format!("id: {}", n.id)).clicked() {
                    clicked = Some(n.id.clone());
                }
            }
        });
        if let Some(id) = clicked {
            self.selected = Some(id);
        }
    }

    fn props_panel(&mut self, ui: &mut egui::Ui) {
        ui.separator();
        ui.heading("Свойства");
        let Some(id) = self.selected.clone() else {
            ui.label(RichText::new("Выберите объект на холсте или в списке слоёв.").weak());
            return;
        };
        let Some(node) = self.scene.doc.node(&id).cloned() else { return };
        let generated = self.scene.sites.get(&id).is_some_and(|s| s.generated);
        ui.label(RichText::new(format!("{} · id {}", node.kind.keyword(), node.id)).weak());
        if generated {
            ui.label(RichText::new("Создан циклом: правки попадут в блок override { }").small().color(ACCENT));
        }
        if self.anim.is_some() {
            ui.label(RichText::new("Показаны базовые значения; ключи анимации пока правятся в тексте.").small().weak());
        }
        let mut changes: Vec<(Prop, Value)> = Vec::new();
        let mut num = |ui: &mut egui::Ui, label: &str, prop: Prop, speed: f64, range: std::ops::RangeInclusive<f64>| {
            let mut v = node.get(prop).as_num().unwrap_or(0.0);
            ui.label(label);
            if ui.add(egui::DragValue::new(&mut v).speed(speed).range(range).max_decimals(2)).changed() {
                changes.push((prop, Value::Num(v)));
            }
        };
        egui::Grid::new("props").num_columns(4).spacing([8.0, 6.0]).show(ui, |ui| {
            num(ui, "X", Prop::X, 1.0, f64::MIN..=f64::MAX);
            num(ui, "Y", Prop::Y, 1.0, f64::MIN..=f64::MAX);
            ui.end_row();
            if node.kind != ShapeKind::Group {
                num(ui, "W", Prop::Width, 1.0, 0.0..=100_000.0);
                num(ui, "H", Prop::Height, 1.0, 0.0..=100_000.0);
                ui.end_row();
            }
            num(ui, "Угол °", Prop::Rotation, 1.0, -3600.0..=3600.0);
            num(ui, "Альфа", Prop::Opacity, 0.01, 0.0..=1.0);
            ui.end_row();
            num(ui, "Масштаб X", Prop::ScaleX, 0.01, -100.0..=100.0);
            num(ui, "Масштаб Y", Prop::ScaleY, 0.01, -100.0..=100.0);
            ui.end_row();
            match node.kind {
                ShapeKind::Rect => {
                    num(ui, "Скругл.", Prop::Radius, 0.5, 0.0..=10_000.0);
                    ui.end_row();
                }
                ShapeKind::Star => {
                    num(ui, "Лучи", Prop::Points, 0.1, 3.0..=64.0);
                    num(ui, "Внутр.", Prop::Inner, 0.01, 0.05..=1.0);
                    ui.end_row();
                }
                ShapeKind::Polygon => {
                    num(ui, "Стороны", Prop::Points, 0.1, 3.0..=64.0);
                    ui.end_row();
                }
                _ => {}
            }
        });
        if node.kind != ShapeKind::Group {
            ui.add_space(6.0);
            paint_row(ui, "Заливка", node.fill, Prop::Fill, &mut changes);
            paint_row(ui, "Обводка", node.stroke, Prop::StrokeColor, &mut changes);
            if node.stroke.is_some() {
                ui.horizontal(|ui| {
                    let mut w = node.stroke_width;
                    ui.label("Толщина");
                    if ui.add(egui::DragValue::new(&mut w).speed(0.1).range(0.0..=1000.0).max_decimals(2)).changed() {
                        changes.push((Prop::StrokeWidth, Value::Num(w)));
                    }
                });
            }
        }
        for (prop, value) in changes {
            self.edit(&id, prop, value);
        }
    }

    fn timeline(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            let label = self.anim.clone().unwrap_or_else(|| "Сцена (без анимации)".into());
            egui::ComboBox::from_id_salt("anim").selected_text(label).show_ui(ui, |ui| {
                if ui.selectable_label(self.anim.is_none(), "Сцена (без анимации)").clicked() {
                    self.anim = None;
                    self.playing = false;
                }
                for a in &self.scene.doc.animations {
                    if ui.selectable_label(self.anim.as_deref() == Some(&a.name), &a.name).clicked() {
                        self.anim = Some(a.name.clone());
                        self.time = 0.0;
                    }
                }
            });
            let Some(anim) = self.anim.as_deref().and_then(|a| self.scene.doc.animation(a)).cloned() else {
                ui.label(RichText::new("Выберите анимацию, чтобы проиграть её. Объекты двигаются мышью в режиме «Сцена».").weak());
                return;
            };
            if ui.button("⏮").on_hover_text("В начало").clicked() {
                self.time = 0.0;
            }
            if ui.button(if self.playing { "⏸" } else { "▶" }).on_hover_text("Пробел").clicked() {
                self.playing = !self.playing;
            }
            let mut t = anim.local_time(self.time);
            ui.spacing_mut().slider_width = (ui.available_width() - 260.0).max(120.0);
            if ui.add(egui::Slider::new(&mut t, 0.0..=anim.duration).show_value(false)).changed() {
                self.time = t;
                self.playing = false;
            }
            ui.monospace(format!("{:.2} / {:.2} с · кадр {} при 60 к/с{}", t, anim.duration, (t * 60.0).round(), if anim.looping { " · повтор" } else { "" }));
        });
    }

    fn status_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label(RichText::new(self.canvas.backend()).small().color(Color32::from_rgb(79, 211, 140)));
            ui.separator();
            let d = &self.scene.doc;
            ui.label(RichText::new(format!("объектов {} · анимаций {}", d.nodes.len(), d.animations.len())).small());
            ui.separator();
            ui.label(RichText::new(&self.status).small());
        });
    }

    // ---------- canvas ----------

    fn canvas_ui(&mut self, ui: &mut egui::Ui, frame: &eframe::Frame) {
        let rect = ui.available_rect_before_wrap();
        let resp = ui.allocate_rect(rect, Sense::click_and_drag());
        let view = *self.view.get_or_insert_with(|| View::fit(rect, &self.scene.doc));
        let doc = self.display_doc();
        self.canvas.paint(ui, frame, rect, view, &doc);
        if self.pixel.on {
            self.paint_pixel_preview(ui, rect, view, &doc);
        }

        // Zoom around the pointer.
        if resp.hovered() {
            let (scroll, pointer) = ui.input(|i| (i.smooth_scroll_delta.y + (i.zoom_delta() - 1.0) * 200.0, i.pointer.hover_pos()));
            if scroll != 0.0
                && let (Some(p), Some(v)) = (pointer, self.view.as_mut())
            {
                let nz = (v.zoom * (scroll * 0.0025).exp()).clamp(0.05, 32.0);
                let anchor = p - rect.min;
                v.pan = anchor - (anchor - v.pan) * (nz / v.zoom);
                v.zoom = nz;
            }
        }

        let pointer = resp.interact_pointer_pos().or(resp.hover_pos());
        let doc_pt = pointer.map(|p| view.to_doc(rect, p));
        self.hovered = doc_pt.and_then(|p| doc.hit_test(p)).map(|i| doc.nodes[i].id.clone());

        let pan_button = ui.input(|i| i.pointer.middle_down() || (i.modifiers.alt && i.pointer.primary_down()));
        if resp.drag_started() {
            if pan_button {
                self.panning = true;
            } else if let Some(p) = doc_pt {
                match doc.hit_test(p) {
                    Some(i) => {
                        let id = doc.nodes[i].id.clone();
                        self.selected = Some(id.clone());
                        if self.anim.is_some() {
                            self.status = "Двигать объекты мышью можно в режиме «Сцена» (внизу, слева от таймлайна)".into();
                        } else {
                            let to_parent = doc.nodes[i].parent.map_or(Affine::IDENTITY, |pi| doc.world_transform(pi).inverse());
                            let origin = doc.world_transform(i) * Point::ZERO;
                            self.drag = Some(Drag { node: id, from: p, origin, to_parent });
                        }
                    }
                    None => self.panning = true,
                }
            }
        }
        if resp.dragged() {
            if self.panning {
                if let Some(v) = self.view.as_mut() {
                    v.pan += resp.drag_delta();
                }
            } else if let (Some(d), Some(p)) = (&self.drag, doc_pt) {
                let target = d.to_parent * (d.origin + (p - d.from));
                let (node, x, y) = (d.node.clone(), target.x.round(), target.y.round());
                let cur = self.scene.doc.node(&node).map(|n| (n.x, n.y));
                if cur.is_some_and(|c| c.0 != x) {
                    self.edit(&node, Prop::X, Value::Num(x));
                }
                if cur.is_some_and(|c| c.1 != y) {
                    self.edit(&node, Prop::Y, Value::Num(y));
                }
            }
        }
        if resp.drag_stopped() {
            self.drag = None;
            self.panning = false;
        }
        if resp.clicked() {
            self.selected = doc_pt.and_then(|p| doc.hit_test(p)).map(|i| doc.nodes[i].id.clone());
        }

        // Overlays: artboard label, hover and selection outlines.
        let painter = ui.painter_at(rect);
        let board = artboard_rect(rect, view, &doc);
        painter.text(
            board.left_top() - egui::vec2(0.0, 6.0),
            egui::Align2::LEFT_BOTTOM,
            format!("{}  {}×{}", doc.artboard.name, doc.artboard.width, doc.artboard.height),
            egui::FontId::proportional(12.0),
            Color32::from_gray(150),
        );
        if let Some(h) = &self.hovered
            && self.selected.as_ref() != Some(h)
            && let Some(i) = doc.index_of(h)
        {
            outline(&painter, rect, view, &doc, i, Stroke::new(1.0, SELECT.gamma_multiply(0.6)));
        }
        if let Some(s) = &self.selected
            && let Some(i) = doc.index_of(s)
        {
            outline(&painter, rect, view, &doc, i, Stroke::new(1.5, SELECT));
        }
        if let Some(p) = doc_pt
            && resp.hovered()
        {
            painter.text(
                rect.right_bottom() - egui::vec2(10.0, 8.0),
                egui::Align2::RIGHT_BOTTOM,
                format!("x {:.0}  y {:.0}  ·  {:.0}%", p.x, p.y, view.zoom * 100.0),
                egui::FontId::monospace(11.0),
                Color32::from_gray(120),
            );
        }
        if self.anim.is_some() {
            painter.rect_stroke(rect.shrink(1.0), 0.0, Stroke::new(2.0, ACCENT), egui::StrokeKind::Inside);
        }
    }

    fn paint_pixel_preview(&mut self, ui: &mut egui::Ui, rect: Rect, view: View, doc: &Document) {
        let scale = self.pixel.width as f64 / doc.artboard.width;
        let Some(mut px) = render_cpu(doc, CpuOptions { scale, antialias: false, background: true }) else { return };
        let scene_colors;
        let palette: &[Color] = match &self.pixel.palette {
            Some((_, p)) => p,
            None => {
                scene_colors = scene_palette(&self.scene.doc);
                &scene_colors
            }
        };
        quantize(&mut px, palette);
        let image = egui::ColorImage::from_rgba_premultiplied([px.width() as usize, px.height() as usize], px.data());
        match &mut self.pixel.texture {
            Some(t) => t.set(image, TextureOptions::NEAREST),
            None => self.pixel.texture = Some(ui.ctx().load_texture("pixel-preview", image, TextureOptions::NEAREST)),
        }
        let tex = self.pixel.texture.as_ref().unwrap();
        let board = artboard_rect(rect, view, doc);
        let painter = ui.painter_at(rect);
        painter.image(tex.id(), board, Rect::from_min_max(Pos2::ZERO, egui::pos2(1.0, 1.0)), Color32::WHITE);
        let name = self.pixel.palette.as_ref().map_or_else(|| format!("палитра сцены, {} цв.", palette.len()), |(n, p)| format!("{n}, {} цв.", p.len()));
        painter.text(
            board.right_top() - egui::vec2(0.0, 6.0),
            egui::Align2::RIGHT_BOTTOM,
            format!("пиксели {}×{} · {name}", px.width(), px.height()),
            egui::FontId::proportional(12.0),
            ACCENT,
        );
    }

    fn shortcuts(&mut self, ctx: &egui::Context) {
        let typing = ctx.text_edit_focused();
        let i = ctx.input(|i| i.clone());
        let cmd = |k: Key| i.modifiers.command && !i.modifiers.shift && i.key_pressed(k);
        if cmd(Key::S) {
            self.save(false);
        }
        if cmd(Key::O) {
            self.open();
        }
        if typing {
            return;
        }
        if cmd(Key::Z) {
            self.undo();
        }
        if (i.modifiers.command && i.modifiers.shift && i.key_pressed(Key::Z)) || cmd(Key::Y) {
            self.redo();
        }
        if i.modifiers == Modifiers::NONE && i.key_pressed(Key::Space) && self.anim.is_some() {
            self.playing = !self.playing;
        }
        if i.modifiers == Modifiers::NONE && i.key_pressed(Key::F) {
            self.view = None;
        }
        if i.key_pressed(Key::Escape) {
            self.selected = None;
        }
        if let Some(id) = self.selected.clone()
            && self.anim.is_none()
        {
            let step = if i.modifiers.shift { 10.0 } else { 1.0 };
            let node = self.scene.doc.node(&id).cloned();
            let nudge = [(Key::ArrowLeft, Prop::X, -step), (Key::ArrowRight, Prop::X, step), (Key::ArrowUp, Prop::Y, -step), (Key::ArrowDown, Prop::Y, step)];
            for (key, prop, delta) in nudge {
                if i.key_pressed(key)
                    && let Some(n) = &node
                    && let Some(v) = n.get(prop).as_num()
                {
                    self.edit(&id, prop, Value::Num(v + delta));
                }
            }
        }
    }
}

fn paint_row(ui: &mut egui::Ui, label: &str, paint: Option<Color>, prop: Prop, changes: &mut Vec<(Prop, Value)>) {
    ui.horizontal(|ui| {
        ui.label(label);
        let mut on = paint.is_some();
        let c = paint.unwrap_or(Color::BLACK);
        let mut rgb = [c.r, c.g, c.b];
        if ui.checkbox(&mut on, "").changed() {
            changes.push((prop, if on { Value::Color(c) } else { Value::None }));
        }
        if on {
            if ui.color_edit_button_srgb(&mut rgb).changed() {
                changes.push((prop, Value::Color(Color { r: rgb[0], g: rgb[1], b: rgb[2], a: c.a })));
            }
            ui.monospace(c.to_hex());
        } else {
            ui.label(RichText::new("нет").weak());
        }
    });
}

fn outline(painter: &egui::Painter, rect: Rect, view: View, doc: &Document, i: usize, stroke: Stroke) {
    let node = &doc.nodes[i];
    let bounds = match node.path() {
        Some(p) => p.bounding_box(),
        // Groups: outline the union of their direct children.
        None => {
            let kids: Vec<_> = (0..doc.nodes.len()).filter(|&k| doc.nodes[k].parent == Some(i)).collect();
            if kids.is_empty() {
                return;
            }
            let inv = doc.world_transform(i).inverse();
            kids.iter()
                .filter_map(|&k| doc.nodes[k].path().map(|p| (inv * doc.world_transform(k)).transform_rect_bbox(p.bounding_box())))
                .reduce(|a, b| a.union(b))
                .unwrap_or_default()
        }
    };
    let t = doc.world_transform(i);
    let corners = [(bounds.x0, bounds.y0), (bounds.x1, bounds.y0), (bounds.x1, bounds.y1), (bounds.x0, bounds.y1)]
        .map(|(x, y)| view.to_screen(rect, t * Point::new(x, y)));
    painter.add(egui::Shape::closed_line(corners.to_vec(), stroke));
    if stroke.width > 1.2 {
        for c in corners {
            painter.rect(Rect::from_center_size(c, egui::vec2(7.0, 7.0)), 1.0, Color32::WHITE, Stroke::new(1.0, SELECT), egui::StrokeKind::Middle);
        }
    }
}

fn mtime(p: &std::path::Path) -> Option<SystemTime> {
    std::fs::metadata(p).and_then(|m| m.modified()).ok()
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.poll_disk();
        self.shortcuts(&ctx);
        if self.playing {
            self.time += ctx.input(|i| i.stable_dt).min(0.1) as f64;
            ctx.request_repaint();
        } else if self.path.is_some() {
            // Keep polling the file on disk for external edits.
            ctx.request_repaint_after(std::time::Duration::from_millis(500));
        }
        let title = format!("Motif — {}{}", self.path.as_ref().map_or("без имени".into(), |p| p.display().to_string()), if self.dirty() { " *" } else { "" });
        ctx.send_viewport_cmd(egui::ViewportCommand::Title(title));

        egui::Panel::top("top").show(ui, |ui| self.top_bar(ui));
        egui::Panel::bottom("status").show(ui, |ui| self.status_bar(ui));
        egui::Panel::bottom("timeline").show(ui, |ui| self.timeline(ui));
        egui::Panel::left("script").resizable(true).default_size(520.0).show(ui, |ui| self.script_panel(ui));
        egui::Panel::right("inspector").resizable(true).default_size(280.0).show(ui, |ui| {
            self.layers_panel(ui);
            self.props_panel(ui);
        });
        egui::CentralPanel::no_frame().show(ui, |ui| self.canvas_ui(ui, frame));
    }
}
