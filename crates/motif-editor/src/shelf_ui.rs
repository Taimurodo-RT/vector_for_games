//! «Библиотека»: the symbols of the project beside the layers. It lists the symbols of the scene
//! and of the `.msc` files in its folder, each with a picture, finds them by name, tag and file,
//! and places a copy by a drag onto the canvas or a double click. A symbol of a file the scene
//! does not include yet brings its `include` line with it.

use crate::app::{App, mtime};
use crate::theme::{self, ACCENT, DANGER, DOT, FAINT, FG, LINE, MUTED, PANEL_2, RAISE};
use egui::{Align2, Pos2, Rect, RichText, Sense, Stroke, StrokeKind, TextureHandle, TextureOptions, pos2, vec2};
use motif_render::{CpuOptions, render_cpu};
use motif_script::LibraryFile;
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::SystemTime;

const ROW: f32 = 48.0;
/// The box of a picture in a row: wide, since buttons and bars are.
const PIC_W: f32 = 64.0;
const PIC: f32 = 40.0;
/// The pixels of a picture on its longer side: the row shows it small, the hint shows it whole.
const PIC_PX: f64 = 192.0;
/// How often the folders are looked into again, in seconds.
const SCAN_EVERY: f64 = 1.5;

/// A symbol of the library: of `file`, or of the scene itself.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Item {
    pub file: Option<PathBuf>,
    pub symbol: String,
}

/// The state of the library tab.
#[derive(Default)]
pub(crate) struct Shelf {
    /// The tab is shown in place of the layers.
    pub open: bool,
    pub query: String,
    /// The tag chosen under the search.
    pub tag: Option<String>,
    /// The files found the last time, with their times: a change reads them again.
    stamps: Vec<(PathBuf, Option<SystemTime>)>,
    /// What each file gives, with its path as an `include` line of this scene writes it.
    pub files: Vec<(LibraryFile, String)>,
    checked: Option<f64>,
    thumbs: HashMap<(PathBuf, String), Option<TextureHandle>>,
    own: HashMap<String, Option<TextureHandle>>,
    /// The script the pictures of the scene's own symbols were drawn from.
    own_src: Option<String>,
    own_checked: f64,
    /// The symbol being dragged and where the pointer is.
    drag: Option<(Item, Pos2)>,
}

/// One list of the tab: the scene or a file.
struct Group {
    title: String,
    hint: String,
    included: bool,
    error: Option<String>,
    rows: Vec<(Item, Vec<String>, String)>,
}

/// The copy `id` of `scene` drawn alone, `px` pixels on its longer side.
pub(crate) fn picture(scene: &motif_script::Scene, id: &str, px: f64) -> Option<tiny_skia::Pixmap> {
    let mut doc = scene.doc.clone();
    let i = doc.index_of(id)?;
    let b = doc.world_bounds(i)?;
    for k in 0..doc.nodes.len() {
        if doc.nodes[k].parent.is_none() && k != i {
            doc.nodes[k].visible = false;
        }
    }
    // Room for outlines and shadows around the shapes.
    let pad = (b.width().max(b.height()) * 0.08).max(3.0);
    let (w, h) = (b.width() + 2.0 * pad, b.height() + 2.0 * pad);
    doc.nodes[i].x -= b.x0 - pad;
    doc.nodes[i].y -= b.y0 - pad;
    doc.artboard.width = w;
    doc.artboard.height = h;
    render_cpu(&doc, CpuOptions { scale: px / w.max(h), antialias: true, background: false })
}

fn texture(ctx: &egui::Context, name: String, p: &tiny_skia::Pixmap) -> TextureHandle {
    let image = egui::ColorImage::from_rgba_premultiplied([p.width() as usize, p.height() as usize], p.data());
    ctx.load_texture(name, image, TextureOptions::LINEAR)
}

impl App {
    /// «Слои | Библиотека» at the top of the left panel. Returns whether the library is shown.
    pub(crate) fn shelf_tabs(&mut self, ui: &mut egui::Ui) -> bool {
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.add_space(8.0);
            ui.spacing_mut().item_spacing.x = 4.0;
            for (open, name, hint) in
                [(false, "Слои", "Объекты холста"), (true, "Библиотека", "Символы этой сцены и файлов её папки: перетащите на холст или дважды щёлкните")]
            {
                let kind = if self.shelf.open == open { theme::ButtonKind::On } else { theme::ButtonKind::Normal };
                if theme::button(ui, name, kind).on_hover_text(hint).clicked() {
                    self.shelf.open = open;
                }
            }
        });
        self.shelf.open
    }

    /// The folders the library looks into: the folder of the scene and of the files it includes.
    fn shelf_folders(&self) -> Vec<PathBuf> {
        let Some(base) = self.base_dir().and_then(|b| std::fs::canonicalize(b).ok()) else { return Vec::new() };
        let mut out = vec![base];
        for dir in self.scene.includes.iter().flat_map(|i| &i.files).filter_map(|f| std::fs::canonicalize(f).ok()?.parent().map(|p| p.to_path_buf())) {
            if !out.iter().any(|o| dir.starts_with(o)) {
                out.push(dir);
            }
        }
        out
    }

    /// Reads the files of the project again when one of them appeared, went away or changed.
    pub(crate) fn shelf_scan(&mut self, now: f64) {
        if self.shelf.checked.is_some_and(|t| (now - t).abs() < SCAN_EVERY) {
            return;
        }
        self.shelf.checked = Some(now);
        let me = self.path.as_deref().and_then(|p| std::fs::canonicalize(p).ok());
        let mut stamps: Vec<(PathBuf, Option<SystemTime>)> = Vec::new();
        for folder in self.shelf_folders() {
            for f in motif_script::library_files(&folder) {
                if Some(&f) != me.as_ref() && !stamps.iter().any(|(g, _)| g == &f) {
                    let t = mtime(&f);
                    stamps.push((f, t));
                }
            }
        }
        if stamps == self.shelf.stamps {
            return;
        }
        let base = self.base_dir();
        self.shelf.files = stamps
            .iter()
            .map(|(f, _)| {
                let name = base.as_deref().and_then(|b| motif_script::include_path(b, f)).unwrap_or_else(|| f.display().to_string());
                (motif_script::library_file(f), name)
            })
            .collect();
        self.shelf.stamps = stamps;
        self.shelf.thumbs.clear();
    }

    /// Draws the pictures of the scene's own symbols again after the script changed.
    fn shelf_own_pictures(&mut self, ctx: &egui::Context, now: f64) {
        if self.shelf.own_src.as_deref() == Some(self.src.as_str()) || !self.errors.is_empty() {
            return;
        }
        // Not in the middle of a drag, and not more often than twice a second.
        if ctx.input(|i| i.pointer.any_down()) || (now - self.shelf.own_checked).abs() < 0.5 {
            ctx.request_repaint_after(std::time::Duration::from_millis(300));
            return;
        }
        self.shelf.own_checked = now;
        self.shelf.own_src = Some(self.src.clone());
        let names: Vec<String> = self.scene.symbols.iter().filter(|s| self.scene.included(s.span).is_none()).map(|s| s.name.clone()).collect();
        let mut own = HashMap::new();
        if !names.is_empty()
            && let Some((scene, ids)) = motif_script::library_own_preview(&self.src, &self.scene, self.base_dir().as_deref(), &names)
        {
            for (name, id) in names.iter().zip(&ids) {
                let tex = picture(&scene, id, PIC_PX).map(|p| texture(ctx, format!("shelf-own-{name}"), &p));
                own.insert(name.clone(), tex);
            }
        }
        self.shelf.own = own;
    }

    /// What the tab lists now: the scene's own symbols, then every file, with the search and
    /// the chosen tag applied.
    fn shelf_groups(&self) -> Vec<Group> {
        let (query, tag) = (self.shelf.query.as_str(), self.shelf.tag.as_deref());
        let fits = |name: &str, tags: &[String], file: &str| {
            motif_script::library_match(query, name, tags, file) && tag.is_none_or(|t| tags.iter().any(|w| w.to_lowercase() == t))
        };
        let mut out = Vec::new();
        let own: Vec<(Item, Vec<String>, String)> = self
            .scene
            .symbols
            .iter()
            .filter(|s| self.scene.included(s.span).is_none() && fits(&s.name, &s.tags, ""))
            .map(|s| {
                let about = about(&s.params.iter().map(|(n, _)| n.clone()).collect::<Vec<_>>(), &s.variants.iter().map(|v| v.name.clone()).collect::<Vec<_>>());
                (Item { file: None, symbol: s.name.clone() }, s.tags.clone(), about)
            })
            .collect();
        if !own.is_empty() {
            out.push(Group {
                title: "Эта сцена".into(), hint: "Символы, записанные в этой сцене".into(), included: false, error: None, rows: own
            });
        }
        for (f, name) in &self.shelf.files {
            let rows: Vec<(Item, Vec<String>, String)> = f
                .symbols
                .iter()
                .filter(|s| fits(&s.name, &s.tags, name))
                .map(|s| (Item { file: Some(f.file.clone()), symbol: s.name.clone() }, s.tags.clone(), about(&s.params, &s.variants)))
                .collect();
            // A file that gives no symbols is not a shelf; a file that is not read says why.
            let told = f.error.is_some() && query.is_empty() && tag.is_none();
            if rows.is_empty() && !told {
                continue;
            }
            let included = self.scene.includes.iter().any(|i| i.path.replace('\\', "/") == *name);
            out.push(Group { title: name.clone(), hint: f.file.display().to_string(), included, error: f.error.clone(), rows });
        }
        out
    }

    /// Every tag of the library, lower case, in the order of the alphabet.
    fn shelf_tags(&self) -> Vec<String> {
        let own = self.scene.symbols.iter().filter(|s| self.scene.included(s.span).is_none()).flat_map(|s| &s.tags);
        let mut tags: Vec<String> = own.chain(self.shelf.files.iter().flat_map(|(f, _)| &f.symbols).flat_map(|s| &s.tags)).map(|t| t.to_lowercase()).collect();
        tags.sort();
        tags.dedup();
        tags
    }

    /// The library tab of the left panel.
    pub(crate) fn shelf_panel(&mut self, ui: &mut egui::Ui) {
        let now = ui.input(|i| i.time);
        self.shelf_scan(now);
        self.shelf_own_pictures(ui.ctx(), now);
        let total = self.scene.symbols.iter().filter(|s| self.scene.included(s.span).is_none()).count()
            + self.shelf.files.iter().map(|(f, _)| f.symbols.len()).sum::<usize>();
        crate::app::panel_header(ui, "Библиотека", &total.to_string());
        ui.horizontal(|ui| {
            ui.add(egui::TextEdit::singleline(&mut self.shelf.query).hint_text("Поиск: имя, тег, файл").desired_width((ui.available_width() - 30.0).max(40.0)));
            if ui.small_button("×").on_hover_text("Очистить поиск").clicked() {
                self.shelf.query.clear();
            }
        });
        let tags = self.shelf_tags();
        if self.shelf.tag.as_ref().is_some_and(|t| !tags.contains(t)) {
            self.shelf.tag = None;
        }
        if !tags.is_empty() {
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = vec2(4.0, 4.0);
                for t in &tags {
                    let on = self.shelf.tag.as_deref() == Some(t.as_str());
                    if ui
                        .selectable_label(on, RichText::new(t).size(11.5))
                        .on_hover_text("Показать символы с этим тегом. Второй щелчок снимает выбор")
                        .clicked()
                    {
                        self.shelf.tag = (!on).then(|| t.clone());
                    }
                }
            });
        }
        let groups = self.shelf_groups();
        let mut thumbs = std::mem::take(&mut self.shelf.thumbs);
        let mut drag = self.shelf.drag.take();
        let mut place: Option<(Item, Option<Pos2>)> = None;
        // A few new pictures a frame: a big library does not stall the editor.
        let mut budget = 3;
        let foot_h = 58.0;
        egui::ScrollArea::vertical().id_salt("shelf").max_height(ui.available_height() - foot_h).auto_shrink([false, false]).show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            if groups.is_empty() {
                let text = if self.base_dir().is_none() && self.scene.symbols.is_empty() {
                    "Сохраните сцену в файл: библиотека покажет символы всех файлов .msc из её папки."
                } else if self.shelf.query.is_empty() && self.shelf.tag.is_none() {
                    "Символов пока нет. Выделите группу и нажмите «Сделать символом» или положите в папку сцены файл набора."
                } else {
                    "Ничего не найдено. Поиск смотрит имя символа, его теги и имя файла."
                };
                ui.add_space(10.0);
                ui.horizontal_wrapped(|ui| {
                    ui.add_space(8.0);
                    ui.label(RichText::new(text).size(11.5).color(FAINT));
                });
            }
            for g in &groups {
                let (r, head) = ui.allocate_exact_size(vec2(ui.available_width(), 26.0), Sense::hover());
                let p = ui.painter().with_clip_rect(r);
                p.line_segment([r.left_top(), r.right_top()], Stroke::new(1.0, LINE));
                p.text(pos2(r.left() + 12.0, r.center().y), Align2::LEFT_CENTER, &g.title, theme::semibold(12.0), if g.included { ACCENT } else { MUTED });
                let meta = if g.included { "подключён".to_string() } else { g.rows.len().to_string() };
                p.text(pos2(r.right() - 10.0, r.center().y), Align2::RIGHT_CENTER, meta, theme::ui_font(11.0), FAINT);
                head.on_hover_text(&g.hint);
                if let Some(e) = &g.error {
                    ui.horizontal_wrapped(|ui| {
                        ui.add_space(12.0);
                        ui.label(RichText::new(format!("Файл не читается, {e}")).size(11.5).color(DANGER));
                    });
                    ui.add_space(6.0);
                }
                for (item, tags, about) in &g.rows {
                    let (r, resp) = ui.allocate_exact_size(vec2(ui.available_width(), ROW), Sense::click_and_drag());
                    let dragged = drag.as_ref().is_some_and(|(d, _)| d == item);
                    if resp.hovered() || dragged {
                        ui.painter().rect_filled(r, 0.0, if dragged { RAISE } else { PANEL_2 });
                    }
                    let pic = Rect::from_min_size(pos2(r.left() + 10.0, r.top() + (ROW - PIC) / 2.0), vec2(PIC_W, PIC));
                    ui.painter().rect(pic, 6.0, DOT, Stroke::new(1.0, LINE), StrokeKind::Inside);
                    let tex = match &item.file {
                        None => self.shelf.own.get(&item.symbol).cloned().flatten(),
                        Some(file) => {
                            let key = (file.clone(), item.symbol.clone());
                            if !thumbs.contains_key(&key) && budget > 0 && ui.is_rect_visible(r) {
                                budget -= 1;
                                let made = motif_script::library_preview(file, &item.symbol)
                                    .and_then(|(scene, id)| picture(&scene, &id, PIC_PX))
                                    .map(|p| texture(ui.ctx(), format!("shelf-{}-{}", file.display(), item.symbol), &p));
                                thumbs.insert(key.clone(), made);
                                ui.ctx().request_repaint();
                            }
                            thumbs.get(&key).cloned().flatten()
                        }
                    };
                    if let Some(tex) = &tex {
                        let [w, h] = tex.size().map(|v| v as f32);
                        let k = ((PIC_W - 8.0) / w).min((PIC - 8.0) / h);
                        egui::Image::new(tex).paint_at(ui, Rect::from_center_size(pic.center(), vec2(w * k, h * k)));
                    }
                    let text = ui.painter().with_clip_rect(r.shrink2(vec2(6.0, 0.0)));
                    let left = pic.right() + 10.0;
                    let lone = tags.is_empty();
                    let name_at = pos2(left, if lone { r.center().y } else { r.center().y - 8.0 });
                    text.text(name_at, Align2::LEFT_CENTER, &item.symbol, theme::semibold(13.0), FG);
                    if !lone {
                        text.text(pos2(left, r.center().y + 9.0), Align2::LEFT_CENTER, tags.join(" · "), theme::ui_font(11.0), FAINT);
                    }
                    if resp.double_clicked() {
                        place = Some((item.clone(), None));
                    }
                    if resp.drag_started() {
                        drag = Some((item.clone(), resp.interact_pointer_pos().unwrap_or(r.center())));
                    }
                    if dragged && let Some(at) = resp.interact_pointer_pos().or_else(|| ui.ctx().pointer_latest_pos()) {
                        drag = Some((item.clone(), at));
                    }
                    if resp.drag_stopped()
                        && let Some((item, at)) = drag.take()
                    {
                        place = Some((item, Some(at)));
                    }
                    let whole = tex.clone();
                    resp.on_hover_ui(|ui| {
                        ui.set_max_width(260.0);
                        if let Some(tex) = &whole {
                            let [w, h] = tex.size().map(|v| v as f32);
                            let k = (160.0 / w).min(120.0 / h).min(1.0);
                            ui.add(egui::Image::new(tex).fit_to_exact_size(vec2(w * k, h * k)));
                        }
                        ui.label(RichText::new(&item.symbol).font(theme::semibold(13.0)).color(FG));
                        if !about.is_empty() {
                            ui.label(RichText::new(about).size(11.5).color(MUTED));
                        }
                        ui.label(RichText::new("Перетащите на холст или дважды щёлкните: в сцену встанет копия.").size(11.5).color(FAINT));
                    });
                }
            }
        });
        ui.separator();
        ui.horizontal_wrapped(|ui| {
            ui.add_space(8.0);
            let text = "Перетащите на холст или дважды щёлкните.";
            ui.label(RichText::new(text).size(11.0).color(FAINT));
        });
        // The symbol follows the pointer while it is dragged.
        if let Some((item, at)) = &drag {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
            let over = self.canvas_bounds.contains(*at);
            let label = if over { format!("{}: отпустите, и копия встанет здесь", item.symbol) } else { item.symbol.clone() };
            let painter = ui.ctx().layer_painter(egui::LayerId::new(egui::Order::Tooltip, egui::Id::unique("shelf-drag")));
            let galley = painter.layout_no_wrap(label, theme::semibold(12.0), FG);
            let chip = Rect::from_min_size(*at + vec2(14.0, 10.0), galley.size() + vec2(16.0, 10.0));
            painter.rect(chip, 6.0, RAISE, Stroke::new(1.0, if over { ACCENT } else { LINE }), StrokeKind::Inside);
            painter.galley(chip.min + vec2(8.0, 5.0), galley, FG);
            ui.ctx().request_repaint();
        }
        self.shelf.thumbs = thumbs;
        self.shelf.drag = drag;
        if let Some((item, at)) = place {
            match at {
                None => self.shelf_place(&item, None),
                Some(at) if self.canvas_bounds.contains(at) => {
                    let p = self.view.map(|v| v.to_doc(self.canvas_bounds, at));
                    self.shelf_place(&item, p.map(|p| (p.x, p.y)));
                }
                Some(_) => self.status = "Символ отпущен не на холсте: перетащите его на холст, и копия встанет в сцену".into(),
            }
        }
    }

    /// Writes `tags` as the words the library finds symbol `symbol` by.
    pub(crate) fn set_symbol_tags(&mut self, symbol: &str, tags: &[String]) {
        if !self.can_edit() {
            return;
        }
        if let Some(file) = self.symbol_from(symbol) {
            self.refuse_included(&format!("Символ {symbol}"), &file);
            return;
        }
        let out = motif_script::set_tags(&self.src, &self.scene, symbol, tags);
        if self.apply_once("symbol-tags", out) {
            let now = self.scene.symbols.iter().find(|s| s.name == symbol).map(|s| s.tags.join(", ")).unwrap_or_default();
            self.status = if now.is_empty() {
                format!("У символа {symbol} больше нет тегов")
            } else {
                format!("Теги символа {symbol}: {now}. По ним его находит «Библиотека»")
            };
        }
    }

    /// Places a copy of a symbol of the library at an artboard point, or in the middle of the
    /// artboard. A file the scene does not include yet is included in the same step.
    pub(crate) fn shelf_place(&mut self, item: &Item, at: Option<(f64, f64)>) {
        if !self.can_edit() {
            return;
        }
        let ab = &self.scene.doc.artboard;
        let (x, y) = at.unwrap_or((ab.width / 2.0, ab.height / 2.0));
        let center = (x.round(), y.round());
        let symbol = &item.symbol;
        let Some(file) = &item.file else {
            let (out, id) = motif_script::add_use(&self.src, &self.scene, symbol, center);
            if self.apply("add-use", Some(out)) {
                self.select_only(Some(id.clone()));
                self.status = format!("Вставлена копия {id} символа {symbol}");
            }
            return;
        };
        let Some(folder) = self.base_dir() else {
            self.status = "Сначала сохраните сцену в файл: путь к файлу символа пишется от её папки".into();
            return;
        };
        match motif_script::add_library_use(&self.src, &self.scene, &folder, file, symbol, center) {
            Ok((out, id, included)) => {
                let name = motif_script::include_path(&folder, file).unwrap_or_else(|| file.display().to_string());
                if self.apply_once(if included { "include-use" } else { "add-use" }, Some(out)) {
                    self.select_only(Some(id.clone()));
                    let more = if included { ". Файл подключён строкой include в начале скрипта" } else { "" };
                    self.status = format!("Вставлена копия {id} символа {symbol} из «{name}»{more}");
                } else {
                    self.status = format!("Копия символа {symbol} не вставлена. {}", self.status);
                }
            }
            Err(e) => {
                let mut chars = e.chars();
                let told: String = chars.next().map(|c| c.to_uppercase().chain(chars).collect()).unwrap_or_default();
                self.status = format!("{told}. Сцена не изменена");
            }
        }
    }
}

/// «Параметры: w, face. Варианты: go, quiet» for the hint of a symbol.
fn about(params: &[String], variants: &[String]) -> String {
    let mut parts = Vec::new();
    if !params.is_empty() {
        parts.push(format!("Параметры: {}", params.join(", ")));
    }
    if !variants.is_empty() {
        parts.push(format!("Варианты: {}", variants.join(", ")));
    }
    parts.join(". ")
}
