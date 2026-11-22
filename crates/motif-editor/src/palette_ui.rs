//! The palette: colors named at the top of the script (`let main = #3b6cff`) as swatches. A
//! click on a swatch under a fill or stroke writes its name (`fill main`), so recoloring the
//! swatch recolors everything that uses it. The eyedropper picks a color off the canvas.

use crate::app::{App, fold};
use crate::theme::{self, ACCENT, BG, FAINT, FG, LINE, MUTED};
use egui::{Color32, RichText, Sense, Stroke, StrokeKind, vec2};
use motif_core::kurbo::{Point, Rect};
use motif_core::{Color, Document, Gradient, Prop, Value};
use motif_render::CpuOptions;
use motif_script::{ColorUse, ColorWord};

/// Where a color picked off the canvas goes.
#[derive(Clone, Debug, PartialEq)]
pub enum Eyedrop {
    /// The fill or stroke of a node.
    Paint { node: String, prop: Prop },
    /// A palette color.
    Swatch(String),
}

fn c32(c: Color) -> Color32 {
    Color32::from_rgba_unmultiplied(c.r, c.g, c.b, c.a)
}

/// One square of color with a frame; `on` rings it.
fn swatch(ui: &mut egui::Ui, size: f32, c: Color, on: bool) -> egui::Response {
    let (r, resp) = ui.allocate_exact_size(vec2(size, size), Sense::click());
    ui.painter().rect_filled(r, 4.0, c32(c));
    let ring = if on {
        Stroke::new(2.0, ACCENT)
    } else if resp.hovered() {
        Stroke::new(1.0, FG)
    } else {
        Stroke::new(1.0, LINE)
    };
    ui.painter().rect_stroke(r, 4.0, ring, StrokeKind::Outside);
    resp
}

/// The color of the scene at `p` as drawn: shapes, gradients, shadows and the artboard behind.
pub fn sample(doc: &Document, p: Point) -> Option<Color> {
    let px = motif_render::render_region(
        doc,
        Rect::new(p.x.floor(), p.y.floor(), p.x.floor() + 1.0, p.y.floor() + 1.0),
        CpuOptions { scale: 1.0, antialias: true, background: true },
    )?;
    let c = px.pixels().first()?.demultiply();
    Some(Color { r: c.red(), g: c.green(), b: c.blue(), a: 255 })
}

/// A strip of a gradient with a frame; `on` rings it. A radial one is shown from its middle out.
pub(crate) fn gradient_swatch(ui: &mut egui::Ui, size: egui::Vec2, g: &Gradient, on: bool) -> egui::Response {
    let (r, resp) = ui.allocate_exact_size(size, Sense::click());
    let stops: Vec<Color> = if g.radial { g.colors.iter().rev().chain(g.colors.iter().skip(1)).copied().collect() } else { g.colors.clone() };
    if stops.len() >= 2 {
        let mut mesh = egui::Mesh::default();
        for (i, c) in stops.iter().enumerate() {
            let x = r.left() + r.width() * i as f32 / (stops.len() - 1) as f32;
            mesh.colored_vertex(egui::pos2(x, r.top()), c32(*c));
            mesh.colored_vertex(egui::pos2(x, r.bottom()), c32(*c));
            if i > 0 {
                let k = (i * 2) as u32;
                mesh.add_triangle(k - 2, k - 1, k);
                mesh.add_triangle(k - 1, k, k + 1);
            }
        }
        ui.painter().add(mesh);
    }
    let ring = if on {
        Stroke::new(2.0, ACCENT)
    } else if resp.hovered() {
        Stroke::new(1.0, FG)
    } else {
        Stroke::new(1.0, LINE)
    };
    ui.painter().rect_stroke(r, 3.0, ring, StrokeKind::Outside);
    resp
}

/// What the palette panel asks for.
pub(crate) enum Act {
    Set(String, Color),
    SetGradient(String, Gradient),
    Add,
    AddGradient,
    Pick(Eyedrop),
    Rename(String, String),
    /// Every place of the first color is written as the second.
    Replace(ColorWord, ColorWord),
    /// A color written by value gets a name in the palette.
    Name(Color),
    /// Every repeated color and gradient gets a name.
    Collect,
    /// The scene is shown in this theme; empty shows it as written.
    Theme(String),
    AddTheme,
    RenameTheme(String, String),
    DeleteTheme(String),
    /// The theme shown no longer changes this name of the palette.
    Reset(String),
}

/// The small button that takes a name out of the theme shown.
pub(crate) fn theme_reset_button(ui: &mut egui::Ui, theme: &str) -> bool {
    let hint = format!("Тема «{theme}» даёт этому имени своё значение. Щелчок убирает его из темы: будет как в исходной сцене");
    theme::button(ui, "↺", theme::ButtonKind::Normal).on_hover_text(hint).clicked()
}

/// How many rows of «Цвета сцены» are shown before the rest is summed up.
const SCENE_COLORS_SHOWN: usize = 24;

impl App {
    /// How often each color of the file is used: the palette colors of this board, then the
    /// colors written by value. Counted once for a scene, every board of the file is read.
    fn counted_colors(&mut self) -> Vec<ColorUse> {
        if self.color_uses.as_ref().is_none_or(|(rev, _)| *rev != self.scene_rev) {
            // One board is already read; several are read together, each with the shared head.
            let found = if self.board_count() < 2 {
                motif_script::scene_color_uses(&self.scene)
            } else {
                motif_script::color_uses(&self.file_text(), self.path.as_ref().and_then(|p| p.parent()), self.boards.active).unwrap_or_default()
            };
            self.color_uses = Some((self.scene_rev, found));
        }
        self.color_uses.as_ref().map(|(_, uses)| uses.clone()).unwrap_or_default()
    }

    /// The palette section: every named color and gradient, editable, and buttons for new ones.
    pub(crate) fn palette_section(&mut self, ui: &mut egui::Ui) {
        let palette = self.scene.palette.clone();
        let gradients = self.scene.gradients.clone();
        let uses = if self.errors.is_empty() { self.counted_colors() } else { Vec::new() };
        let places = |name: &str| uses.iter().find(|u| u.name.as_deref() == Some(name)).map_or(0, |u| u.places);
        let mut act: Option<Act> = None;
        let themes = self.scene.doc.themes.clone();
        let shown = self.scene.theme.clone().unwrap_or_default();
        fold(ui, "Палитра", |ui| {
            ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
            // The theme the scene is shown in: what is edited below is edited in it.
            ui.horizontal(|ui| {
                ui.label(RichText::new("Тема").size(12.0).color(MUTED));
                let mut chosen = shown.clone();
                egui::ComboBox::from_id_salt("palette-theme").width(104.0).selected_text(if chosen.is_empty() { "Исходная" } else { chosen.as_str() }).show_ui(
                    ui,
                    |ui| {
                        ui.selectable_value(&mut chosen, String::new(), "Исходная");
                        for name in &themes {
                            ui.selectable_value(&mut chosen, name.clone(), name);
                        }
                    },
                );
                if chosen != shown {
                    act = Some(Act::Theme(chosen));
                }
                if theme::button(ui, "+ Тема", theme::ButtonKind::Normal)
                    .on_hover_text("Новая тема: второй набор значений для тех же имён палитры и стилей, блок theme в начале скрипта")
                    .clicked()
                {
                    act = Some(Act::AddTheme);
                }
            });
            if !shown.is_empty() {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Имя").size(12.0).color(MUTED));
                    if let Some(new) = crate::machine_ui::name_edit(ui, ("theme-name", &shown), &shown, 104.0) {
                        act = Some(Act::RenameTheme(shown.clone(), new));
                    }
                    if theme::button(ui, "×", theme::ButtonKind::Normal).on_hover_text("Удалить тему. Исходная сцена не меняется").clicked()
                    {
                        act = Some(Act::DeleteTheme(shown.clone()));
                    }
                });
                let text = format!(
                    "Показана тема «{shown}». Правка цвета или градиента здесь и «Записать в стиль» у объекта меняют тему, исходная сцена остаётся. ↺ возвращает имени исходное значение."
                );
                ui.label(RichText::new(text).size(11.5).color(ACCENT));
            }
            if palette.is_empty() && gradients.is_empty() {
                ui.label(
                    RichText::new(
                        "Именованных цветов нет. Добавьте цвет — он встанет строкой let в начало скрипта, и его можно назначать заливкам и обводкам по имени.",
                    )
                    .size(11.5)
                    .color(FAINT),
                );
            }
            for s in &palette {
                ui.horizontal(|ui| {
                    let resp = swatch(ui, 22.0, s.color, self.eyedrop == Some(Eyedrop::Swatch(s.name.clone())));
                    let hex = if s.literal { s.color.to_hex() } else { "по имени".into() };
                    let count = format!("мест: {}", places(&s.name));
                    let width = |text: &str, font: egui::FontId| ui.painter().layout_no_wrap(text.to_string(), font, FAINT).size().x;
                    // While a theme is shown the row is about the value in it: the number of places
                    // moves into the hint, and the name stays readable next to «↺».
                    let counted = shown.is_empty();
                    let taken = width(&hex, theme::mono(11.0))
                        + if counted { width(&count, egui::FontId::proportional(11.5)) + 22.0 } else { 14.0 }
                        + if s.themed { 30.0 } else { 0.0 };
                    if let Some(new) = crate::machine_ui::name_edit(ui, ("swatch-name", &s.name), &s.name, (ui.available_width() - taken).max(50.0)) {
                        act = Some(Act::Rename(s.name.clone(), new));
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if s.themed && theme_reset_button(ui, &shown) {
                            act = Some(Act::Reset(s.name.clone()));
                        }
                        let value = ui.label(RichText::new(hex).font(theme::mono(11.0)).color(if s.themed { ACCENT } else { MUTED }));
                        if counted {
                            ui.label(RichText::new(count).size(11.5).color(FAINT)).on_hover_text("В скольких местах файла цвет записан этим именем");
                        } else {
                            value.on_hover_text(format!("{count} по всему файлу"));
                        }
                    });
                    theme::color_popup(&resp).show(|ui| {
                        if s.literal {
                            let mut c = c32(s.color);
                            if egui::color_picker::color_picker_color32(ui, &mut c, egui::color_picker::Alpha::OnlyBlend) {
                                act = Some(Act::Set(s.name.clone(), Color { r: c.r(), g: c.g(), b: c.b(), a: c.a() }));
                            }
                            if theme::button(ui, "Пипетка: взять с холста", theme::ButtonKind::Normal).clicked() {
                                act = Some(Act::Pick(Eyedrop::Swatch(s.name.clone())));
                            }
                        } else {
                            ui.label(RichText::new("Этот цвет задан именем другого: меняйте тот").size(11.5).color(MUTED));
                        }
                        if palette.len() > 1 {
                            ui.add_space(4.0);
                            ui.label(RichText::new(format!("Заменить «{}» везде на:", s.name)).size(11.5).color(MUTED));
                            ui.horizontal_wrapped(|ui| {
                                ui.spacing_mut().item_spacing = vec2(4.0, 4.0);
                                for other in palette.iter().filter(|o| o.name != s.name) {
                                    let hint = format!("{}: все места, где записано «{}», получат это имя", other.name, s.name);
                                    if swatch(ui, 16.0, other.color, false).on_hover_text(hint).clicked() {
                                        act = Some(Act::Replace(ColorWord::Name(s.name.clone()), ColorWord::Name(other.name.clone())));
                                        ui.close();
                                    }
                                }
                            });
                        }
                    });
                });
            }
            for s in &gradients {
                ui.horizontal(|ui| {
                    let resp = gradient_swatch(ui, vec2(44.0, 22.0), &s.gradient, false);
                    let kind = if s.gradient.radial {
                        "радиальный".to_string()
                    } else {
                        format!("линейный {}°", motif_script::fmt_num(s.gradient.angle))
                    };
                    let taken =
                        ui.painter().layout_no_wrap(kind.clone(), egui::FontId::proportional(11.5), FAINT).size().x + 14.0 + if s.themed { 30.0 } else { 0.0 };
                    if let Some(new) = crate::machine_ui::name_edit(ui, ("swatch-name", &s.name), &s.name, (ui.available_width() - taken).max(50.0)) {
                        act = Some(Act::Rename(s.name.clone(), new));
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if s.themed && theme_reset_button(ui, &shown) {
                            act = Some(Act::Reset(s.name.clone()));
                        }
                        ui.label(RichText::new(kind).size(11.5).color(if s.themed { ACCENT } else { FAINT }));
                    });
                    theme::color_popup(&resp).show(|ui| {
                        ui.set_max_width(260.0);
                        let mut next: Option<Gradient> = None;
                        if let Some(k) = theme::segmented(ui, &["Линейный", "Радиальный"], s.gradient.radial as usize, false)
                            && (k == 1) != s.gradient.radial
                        {
                            next = Some(Gradient { radial: k == 1, ..s.gradient.clone() });
                        }
                        ui.add_space(6.0);
                        if let Some(g) = crate::paint::gradient_editor(ui, &s.gradient) {
                            next = Some(g);
                        }
                        if let Some(g) = next {
                            act = Some(Act::SetGradient(s.name.clone(), g));
                        }
                    });
                });
            }
            ui.horizontal_wrapped(|ui| {
                if theme::button(ui, "+ Цвет", theme::ButtonKind::Normal)
                    .on_hover_text("Новый цвет палитры: строка let в начале скрипта; цвет — заливка выделенного объекта")
                    .clicked()
                {
                    act = Some(Act::Add);
                }
                if theme::button(ui, "+ Градиент", theme::ButtonKind::Normal)
                    .on_hover_text("Новый градиент палитры: строка let … = linear 90 … Градиент объекта попадает в палитру кнопкой «В палитру» в его заливке")
                    .clicked()
                {
                    act = Some(Act::AddGradient);
                }
            });
            ui.label(
                RichText::new(
                    "Цвет и градиент палитры назначаются щелчком по квадратику под заливкой или обводкой объекта. Поменяйте их здесь — перекрасится всё, что их использует. Имя меняется прямо в поле.",
                )
                .size(11.5)
                .color(FAINT),
            );
        });
        // The colors written by value: what the scene uses besides its palette.
        let literal: Vec<&ColorUse> = uses.iter().filter(|u| u.name.is_none()).collect();
        if !literal.is_empty() {
            fold(ui, "Цвета сцены", |ui| {
                ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
                let repeated = literal.iter().filter(|u| u.places > 1 || palette.iter().any(|s| s.color == u.color)).count();
                ui.horizontal(|ui| {
                    let hint = "Цвет, записанный значением в двух местах и больше, получает имя в палитре; цвет, равный цвету палитры, записывается его именем. Одинаковые градиенты тоже. Рисунок не меняется";
                    if theme::button(ui, "Собрать в палитру", theme::ButtonKind::Normal).on_hover_text(hint).clicked() {
                        act = Some(Act::Collect);
                    }
                    ui.label(RichText::new(format!("повторяются: {repeated}")).size(11.5).color(FAINT));
                });
                for u in literal.iter().take(SCENE_COLORS_SHOWN) {
                    ui.horizontal(|ui| {
                        let resp = swatch(ui, 18.0, u.color, false);
                        ui.label(RichText::new(u.color.to_hex()).font(theme::mono(11.5)).color(FG));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.label(RichText::new(format!("мест: {}", u.places)).size(11.5).color(FAINT));
                        });
                        let same = palette.iter().find(|s| s.color == u.color);
                        theme::color_popup(&resp.on_hover_text("Щелчок: дать имя или заменить везде")).show(|ui| {
                            ui.set_max_width(240.0);
                            match same {
                                Some(s) => {
                                    if theme::button(ui, &format!("Записать именем «{}»", s.name), theme::ButtonKind::Normal).clicked() {
                                        act = Some(Act::Replace(ColorWord::Hex(u.color), ColorWord::Name(s.name.clone())));
                                        ui.close();
                                    }
                                }
                                None => {
                                    let hint = "Цвет получит имя в палитре, и все его места будут записаны этим именем";
                                    if theme::button(ui, "В палитру", theme::ButtonKind::Normal).on_hover_text(hint).clicked() {
                                        act = Some(Act::Name(u.color));
                                        ui.close();
                                    }
                                }
                            }
                            if !palette.is_empty() {
                                ui.add_space(4.0);
                                ui.label(RichText::new("Заменить везде на цвет палитры:").size(11.5).color(MUTED));
                                ui.horizontal_wrapped(|ui| {
                                    ui.spacing_mut().item_spacing = vec2(4.0, 4.0);
                                    for s in &palette {
                                        if swatch(ui, 16.0, s.color, false).on_hover_text(format!("{} {}", s.name, s.color.to_hex())).clicked() {
                                            act = Some(Act::Replace(ColorWord::Hex(u.color), ColorWord::Name(s.name.clone())));
                                            ui.close();
                                        }
                                    }
                                });
                            }
                            ui.add_space(4.0);
                            ui.label(RichText::new("Или на другой цвет:").size(11.5).color(MUTED));
                            // The color being chosen lives with the popup until «Заменить».
                            let id = ui.make_persistent_id(("replace-with", u.color.to_hex()));
                            let mut c = ui.data(|d| d.get_temp::<Color32>(id)).unwrap_or(c32(u.color));
                            if egui::color_picker::color_picker_color32(ui, &mut c, egui::color_picker::Alpha::OnlyBlend) {
                                ui.data_mut(|d| d.insert_temp(id, c));
                            }
                            let to = Color { r: c.r(), g: c.g(), b: c.b(), a: c.a() };
                            if to != u.color && theme::button(ui, &format!("Заменить на {}", to.to_hex()), theme::ButtonKind::Normal).clicked() {
                                ui.data_mut(|d| d.remove::<Color32>(id));
                                act = Some(Act::Replace(ColorWord::Hex(u.color), ColorWord::Hex(to)));
                                ui.close();
                            }
                        });
                    });
                }
                if literal.len() > SCENE_COLORS_SHOWN {
                    ui.label(RichText::new(format!("…и ещё цветов: {}", literal.len() - SCENE_COLORS_SHOWN)).size(11.5).color(FAINT));
                }
                let text =
                    "Цвета, записанные значением, а не именем палитры, на всех холстах файла. Щелчок по квадратику: дать цвету имя или заменить его везде.";
                ui.label(RichText::new(text).size(11.5).color(FAINT));
            });
        }
        if let Some(act) = act {
            self.palette_act(act);
        }
    }

    pub(crate) fn palette_act(&mut self, act: Act) {
        if let Act::Pick(target) = act {
            self.start_eyedrop(target);
            return;
        }
        if !self.can_edit() {
            return;
        }
        let file = self.file_text();
        let base = self.path.as_ref().and_then(|p| p.parent()).map(|p| p.to_path_buf());
        let base = base.as_deref();
        // Why an edit of the whole file gave nothing: a board that cannot be read, or the reason given.
        let refused = |why: String| match motif_script::parse_boards(&file, base) {
            Ok(_) => why,
            Err(_) => "На одном из холстов файла ошибка в скрипте: сначала исправьте её".to_string(),
        };
        let word = |w: &ColorWord| match w {
            ColorWord::Hex(c) => c.to_hex(),
            ColorWord::Name(n) => format!("«{n}»"),
        };
        match act {
            Act::Pick(_) => {}
            Act::Theme(name) => {
                self.show_theme(&name);
                self.status = match self.scene.theme.as_deref() {
                    Some(t) => format!("Показана тема «{t}»: правки палитры и «Записать в стиль» идут в неё. Экспорт выгружает сцену в этой теме"),
                    None => "Показана исходная сцена".to_string(),
                };
            }
            Act::AddTheme => {
                let name = motif_script::free_theme_name(&file);
                match motif_script::add_theme(&file, base, &name) {
                    Some(out) => {
                        if self.apply_file("theme-add", Some(out)) {
                            self.show_theme(&name);
                            self.status =
                                format!("Тема «{name}» добавлена и показана. Меняйте цвета палитры: новые значения запишутся в тему, исходная сцена останется");
                        }
                    }
                    None => self.status = refused("Тему добавить не получилось".into()),
                }
            }
            Act::RenameTheme(old, new) => {
                let new = new.trim().to_string();
                if !motif_script::theme_name_ok(&file, &new) {
                    self.status = format!("Имя «{new}» не подходит: нужно одно слово из букв, цифр и «_», которого ещё нет среди тем");
                    return;
                }
                match motif_script::rename_theme(&file, base, &old, &new) {
                    Some(out) => {
                        // The theme shown is the renamed one: it stays shown under its new name.
                        self.theme.clone_from(&new);
                        if self.apply_file("theme-rename", Some(out)) {
                            self.status = format!("Тема «{old}» теперь «{new}»");
                        } else {
                            self.theme = old;
                        }
                    }
                    None => self.status = refused(format!("Тему «{old}» переименовать не получилось")),
                }
            }
            Act::DeleteTheme(name) => match motif_script::delete_theme(&file, base, &name) {
                Some(out) => {
                    self.theme.clear();
                    self.forget_other_boards();
                    if self.apply_file("theme-delete", Some(out)) {
                        self.status = format!("Тема «{name}» удалена. Показана исходная сцена: она не изменилась");
                    } else {
                        self.show_theme(&name);
                    }
                }
                None => self.status = refused(format!("Тему «{name}» удалить не получилось")),
            },
            Act::Reset(name) => {
                let out = motif_script::reset_in_theme(&self.src, &self.scene, &name, false);
                if self.apply_once("theme-reset", out) {
                    self.status = format!("«{name}» в теме «{}» снова как в исходной сцене", self.theme);
                }
            }
            Act::Set(name, c) => {
                let out = motif_script::set_swatch(&self.src, &self.scene, &name, c);
                self.apply(&format!("swatch-{name}"), out);
            }
            Act::SetGradient(name, g) => {
                let out = motif_script::set_gradient_swatch(&self.src, &self.scene, &name, &g);
                self.apply(&format!("swatch-{name}"), out);
            }
            Act::Add => {
                let name = motif_script::free_swatch_name(&file);
                let color = self.selected.as_ref().and_then(|s| self.scene.doc.node(s)).and_then(|n| n.fill).unwrap_or(Color::rgb(128, 128, 128));
                let out = motif_script::add_swatch(&self.src, &self.scene, &name, color);
                if self.apply("swatch-add", Some(out)) {
                    self.status = format!("В палитре новый цвет «{name}»: имя меняется в его поле");
                }
            }
            Act::AddGradient => {
                let name = motif_script::free_gradient_name(&file);
                let from = self.scene.palette.first().map_or(Color::rgb(0x4f, 0xd3, 0x8c), |s| s.color);
                let g = Gradient { radial: false, angle: 90.0, colors: vec![Color::rgb(255, 255, 255), from] };
                let out = motif_script::add_gradient_swatch(&self.src, &self.scene, &name, &g);
                if self.apply_once("swatch-add", Some(out)) {
                    self.status = format!("В палитре новый градиент «{name}»: щелчок по образцу меняет его цвета, имя меняется в поле");
                }
            }
            Act::Rename(old, new) => {
                let new = new.trim().to_string();
                if !motif_script::swatch_name_ok(&self.scene, &new) {
                    self.status = format!("Имя «{new}» не подходит: нужно одно слово из букв, цифр и «_», не занятое в палитре и не название свойства");
                    return;
                }
                match motif_script::rename_swatch(&file, base, self.boards.active, &old, &new) {
                    Some(out) => {
                        if self.apply_file("swatch-rename", Some(out)) {
                            self.status = format!("«{old}» в палитре теперь «{new}»");
                        }
                    }
                    None => self.status = refused(format!("Имя «{new}» занято на другом холсте этого файла")),
                }
            }
            Act::Replace(from, to) => match motif_script::replace_color(&file, base, self.boards.active, &from, &to) {
                Some(r) => {
                    if self.apply_file("color-replace", Some(r.file)) {
                        let above =
                            if r.above > 0 { format!(". Выше строки let осталось как было: {}", r.above) } else { String::new() };
                        self.status = format!("{} заменён на {}, мест: {}{above}", word(&from), word(&to), r.places);
                    }
                }
                None => {
                    let why = match &to {
                        ColorWord::Name(n) => {
                            format!("Заменить не получилось: {} нигде не используется ниже строки let «{n}», или «{n}» виден не на всех холстах", word(&from))
                        }
                        ColorWord::Hex(_) => format!("{} нигде не используется", word(&from)),
                    };
                    self.status = refused(why);
                }
            },
            Act::Name(color) => {
                let name = motif_script::free_swatch_name(&file);
                match motif_script::name_color(&file, base, &name, color) {
                    Some(r) => {
                        if self.apply_file("color-name", Some(r.file)) {
                            self.status = format!("{} теперь «{name}» в палитре, мест: {}. Имя меняется в разделе «Палитра»", color.to_hex(), r.places);
                        }
                    }
                    None => self.status = refused(format!("{} записан только выше палитры: имя там ещё не известно", color.to_hex())),
                }
            }
            Act::Collect => match motif_script::collect_palette(&file, base) {
                Some(c) => {
                    if self.apply_file("palette-collect", Some(c.file)) {
                        self.status = match c.names.len() {
                            0 => format!("По именам палитры записано мест: {}", c.places),
                            n => format!(
                                "В палитре новых имён: {n} ({}). По именам записано мест: {}. Имена меняются в разделе «Палитра»",
                                c.names.join(", "),
                                c.places
                            ),
                        };
                    }
                }
                None => self.status = refused("Собирать нечего: повторяющихся цветов, записанных значением, нет".into()),
            },
        }
    }

    /// The palette under a fill or stroke: a click writes the color's name into the property.
    pub(crate) fn palette_strip(&mut self, ui: &mut egui::Ui, id: &str, prop: Prop) {
        let current = self.scene.refs.get(&(id.to_string(), prop)).cloned();
        let mut chosen: Option<(String, Color)> = None;
        let mut pick = false;
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(4.0, 4.0);
            for s in &self.scene.palette {
                let resp = swatch(ui, 16.0, s.color, current.as_deref() == Some(s.name.as_str()));
                if resp.on_hover_text(format!("{} {}", s.name, s.color.to_hex())).clicked() {
                    chosen = Some((s.name.clone(), s.color));
                }
            }
            let on = self.eyedrop == Some(Eyedrop::Paint { node: id.to_string(), prop });
            let (r, resp) = ui.allocate_exact_size(vec2(16.0, 16.0), Sense::click());
            ui.painter().rect(r, 4.0, if on { ACCENT } else { BG }, Stroke::new(1.0, LINE), StrokeKind::Inside);
            // A dropper: a slanted stem and its drop.
            let ink = if on {
                theme::ACCENT_INK
            } else if resp.hovered() {
                FG
            } else {
                MUTED
            };
            ui.painter().line_segment([r.left_bottom() + vec2(4.0, -4.0), r.right_top() + vec2(-5.0, 5.0)], Stroke::new(1.6, ink));
            ui.painter().circle_filled(r.right_top() + vec2(-4.5, 4.5), 2.2, ink);
            if resp.on_hover_text("Пипетка: щёлкните по холсту, чтобы взять цвет (Esc — отмена)").clicked() {
                pick = true;
            }
            if let Some(name) = &current {
                ui.label(RichText::new(name).size(11.0).color(MUTED));
            }
        });
        if pick {
            self.start_eyedrop(Eyedrop::Paint { node: id.to_string(), prop });
        }
        if let Some((name, color)) = chosen {
            self.assign_color(id, prop, Some(&name), color);
        }
    }

    pub(crate) fn start_eyedrop(&mut self, target: Eyedrop) {
        self.status = match &target {
            Eyedrop::Paint { .. } => "Пипетка: щёлкните по холсту — цвет пойдёт в выделенный объект. Esc — отмена".into(),
            Eyedrop::Swatch(name) => format!("Пипетка: щёлкните по холсту — цвет станет «{name}». Esc — отмена"),
        };
        self.eyedrop = Some(target);
    }

    /// Writes `color` into a fill or stroke: by palette name in Design (`fill main`); in Animate
    /// a key holds the color itself.
    fn assign_color(&mut self, id: &str, prop: Prop, name: Option<&str>, color: Color) {
        match name {
            Some(name) if self.key_time().is_none() => {
                if self.can_edit() {
                    let out = motif_script::set_ref(&self.src, &self.scene, id, prop, name);
                    self.apply(&format!("{id}.{}", prop.path_name()), out);
                }
            }
            _ => self.edit(id, prop, Value::Color(color)),
        }
    }

    /// The canvas while the eyedropper is on: a click takes the color under the pointer.
    pub(crate) fn eyedrop_input(&mut self, ui: &egui::Ui, resp: &egui::Response, doc: &Document, at: Option<Point>) {
        if resp.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Crosshair);
        }
        if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.eyedrop = None;
            self.status = "Пипетка выключена".into();
            return;
        }
        if !resp.clicked() {
            return;
        }
        let Some(target) = self.eyedrop.take() else { return };
        let Some(color) = at.and_then(|p| sample(doc, p)) else { return };
        match target {
            Eyedrop::Paint { node, prop } => {
                // A color of the palette is written by its name.
                let name = self.scene.palette.iter().find(|s| s.color == color).map(|s| s.name.clone());
                self.assign_color(&node, prop, name.as_deref(), color);
                self.status = format!("Пипетка: {}{}", color.to_hex(), name.map_or(String::new(), |n| format!(" — это «{n}» из палитры")));
            }
            Eyedrop::Swatch(name) => {
                if self.can_edit() {
                    let out = motif_script::set_swatch(&self.src, &self.scene, &name, color);
                    self.apply(&format!("swatch-{name}"), out);
                    self.status = format!("«{name}» теперь {}", color.to_hex());
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::editing_tests::fixture;

    const FILE: &str = "let ink = #1b1d24
style card fill #ffffff stroke ink 2
artboard \"one\" 200x100
rect a at 50 50 size 40 40 fill #ffd23f stroke #1b1d24 2 gradient linear 90 #ffd23f #ff5a36
rect b at 100 50 size 40 40 style card
artboard \"two\" 200x100 cell 1 0
rect c at 50 50 size 40 40 fill #ffd23f gradient linear 90 #ffd23f #ff5a36
ellipse d at 100 50 size 20 20 fill #ff5a36 stroke ink 1
";

    fn docs(app: &App) -> Vec<Document> {
        motif_script::parse_boards(&app.file_text(), None).unwrap_or_else(|e| panic!("{e:?}\n{}", app.file_text())).into_iter().map(|s| s.doc).collect()
    }

    /// One frame of the panel shown when nothing is selected.
    fn draw(app: &mut App) {
        let ctx = app.ctx.clone();
        ctx.run_ui(egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, vec2(320.0, 900.0))), ..Default::default() }, |ui| {
            app.palette_section(ui)
        })
        .drop_without_applying_deltas();
    }

    fn hex(c: &str) -> Color {
        Color::from_hex(c).unwrap()
    }

    #[test]
    fn a_theme_is_made_shown_edited_and_removed_from_the_panel() {
        let mut app = fixture(FILE.replace("artboard \"one\" 200x100", "artboard \"one\" 200x100 bg ink").as_str());
        let written = docs(&app);
        let bg = |app: &App| app.scene.doc.artboard.background;
        draw(&mut app);
        // A new theme is empty and shown at once: the picture is the same.
        app.palette_act(Act::AddTheme);
        assert!(app.file_text().starts_with("let ink = #1b1d24\nstyle card fill #ffffff stroke ink 2\ntheme dark {\n}\n"), "{}", app.file_text());
        assert_eq!((app.theme.as_str(), app.scene.theme.as_deref(), bg(&app)), ("dark", Some("dark"), hex("#1b1d24")));
        assert!(app.status.contains("Тема «dark» добавлена и показана"), "{}", app.status);
        draw(&mut app);
        // A color of the palette edited now goes into the theme; the scene as written stays.
        app.palette_act(Act::Set("ink".into(), hex("#f4efe6")));
        assert!(app.file_text().contains("let ink = #1b1d24\n") && app.file_text().contains("theme dark {\n  ink = #f4efe6\n}\n"), "{}", app.file_text());
        assert_eq!(bg(&app), hex("#f4efe6"));
        assert!(app.scene.palette[0].themed);
        // The other board is shown in the theme too, and the files of an export are named after it.
        assert_eq!(app.board_doc(1).unwrap().node("d").unwrap().stroke, Some(hex("#f4efe6")));
        app.export.all_boards = true;
        assert_eq!(app.export_boards().unwrap().iter().map(|b| b.0.clone()).collect::<Vec<_>>(), ["one-dark", "two-dark"]);
        let dark = app.export_boards().unwrap()[0].1.clone();
        draw(&mut app);
        // The scene as written: nothing of the theme is seen or exported.
        app.palette_act(Act::Theme(String::new()));
        assert_eq!((app.scene.theme.clone(), bg(&app)), (None, hex("#1b1d24")));
        assert_eq!(app.export_boards().unwrap().iter().map(|b| b.0.clone()).collect::<Vec<_>>(), ["one", "two"]);
        assert_ne!(app.export_boards().unwrap()[0].1, dark);
        assert_eq!(app.board_doc(1).unwrap().node("d").unwrap().stroke, Some(hex("#1b1d24")));
        let mut now = docs(&app);
        now.iter_mut().for_each(|d| d.themes.clear());
        assert_eq!(now, written);
        // Back in the theme: its name is changed, a name goes back to the scene's value.
        app.palette_act(Act::Theme("dark".into()));
        assert_eq!(bg(&app), hex("#f4efe6"));
        app.palette_act(Act::RenameTheme("dark".into(), "night".into()));
        assert_eq!((app.scene.theme.as_deref(), bg(&app)), (Some("night"), hex("#f4efe6")));
        assert!(app.file_text().contains("theme night {\n  ink = #f4efe6\n}\n"), "{}", app.file_text());
        app.palette_act(Act::RenameTheme("night".into(), "1 bad".into()));
        assert!(app.status.contains("не подходит"), "{}", app.status);
        app.palette_act(Act::Reset("ink".into()));
        assert!(app.file_text().contains("theme night {\n}\n"), "{}", app.file_text());
        assert_eq!(bg(&app), hex("#1b1d24"));
        app.undo();
        assert_eq!(bg(&app), hex("#f4efe6"));
        // Removing the theme shows the scene as written.
        app.palette_act(Act::DeleteTheme("night".into()));
        assert!(!app.file_text().contains("theme"), "{}", app.file_text());
        assert_eq!((app.theme.as_str(), app.scene.theme.clone(), bg(&app)), ("", None, hex("#1b1d24")));
        assert_eq!(docs(&app), written);
        draw(&mut app);
    }

    #[test]
    fn the_palette_panel_counts_replaces_names_and_collects_over_the_whole_file() {
        let mut app = fixture(FILE);
        let before = docs(&app);
        draw(&mut app);
        // Counted over both boards: the palette color by name, then the values.
        let line = |u: &ColorUse| format!("{} {}", u.name.clone().unwrap_or_else(|| u.color.to_hex()), u.places);
        assert_eq!(app.counted_colors().iter().map(line).collect::<Vec<_>>(), ["ink 2", "#ffd23f 4", "#ff5a36 3", "#1b1d24 1", "#ffffff 1"]);

        // A value equal to a palette color is written by its name, on the other board too.
        app.palette_act(Act::Replace(ColorWord::Hex(hex("#1b1d24")), ColorWord::Name("ink".into())));
        assert!(app.status.contains("#1b1d24 заменён на «ink», мест: 1"), "{}", app.status);
        assert_eq!(docs(&app), before);
        // One color gets a name, and the name is changed in its field.
        app.palette_act(Act::Name(hex("#ff5a36")));
        assert!(app.status.contains("теперь «color1» в палитре, мест: 3"), "{}", app.status);
        assert!(app.file_text().starts_with("let ink = #1b1d24\nlet color1 = #ff5a36\nstyle card"), "{}", app.file_text());
        app.palette_act(Act::Rename("color1".into(), "fill".into()));
        assert!(app.status.contains("не подходит"), "{}", app.status);
        app.palette_act(Act::Rename("color1".into(), " hot ".into()));
        assert!(app.status.contains("теперь «hot»"), "{}", app.status);
        assert!(app.file_text().contains("ellipse d at 100 50 size 20 20 fill hot stroke ink 1\n"), "{}", app.file_text());
        assert_eq!(docs(&app), before);
        draw(&mut app);
        assert_eq!(app.counted_colors().iter().map(line).collect::<Vec<_>>(), ["ink 3", "hot 3", "#ffd23f 4", "#ffffff 1"]);

        // The rest is collected: the color written four times and the gradient written twice.
        app.palette_act(Act::Collect);
        assert!(app.status.contains("новых имён: 2 (color1, grad1)"), "{}", app.status);
        let file = app.file_text();
        assert!(file.starts_with("let ink = #1b1d24\nlet hot = #ff5a36\nlet color1 = #ffd23f\nlet grad1 = linear 90 color1 hot\nstyle card"), "{file}");
        assert!(file.contains("rect c at 50 50 size 40 40 fill color1 gradient grad1\n"), "{file}");
        assert_eq!(docs(&app), before);
        assert_eq!(app.scene.gradients.len(), 1);
        app.palette_act(Act::Collect);
        assert!(app.status.contains("Собирать нечего"), "{}", app.status);
        draw(&mut app);

        // The gradient of the palette is edited in one place and both boards follow.
        let mut g = app.scene.gradients[0].gradient.clone();
        g.angle = 0.0;
        app.palette_act(Act::SetGradient("grad1".into(), g.clone()));
        assert!(app.file_text().contains("let grad1 = linear 0 color1 hot\n"), "{}", app.file_text());
        assert!(docs(&app).iter().all(|d| d.nodes[0].gradient.as_ref() == Some(&g)));
        app.palette_act(Act::Rename("grad1".into(), "sunset".into()));
        assert!(app.file_text().contains("rect c at 50 50 size 40 40 fill color1 gradient sunset\n"), "{}", app.file_text());
        // A name by a name; a new color and a new gradient of the palette.
        app.palette_act(Act::Replace(ColorWord::Name("hot".into()), ColorWord::Name("ink".into())));
        assert!(app.status.contains("«hot» заменён на «ink», мест: 2"), "{}", app.status);
        assert!(app.file_text().contains("let sunset = linear 0 color1 ink\n"), "{}", app.file_text());
        app.palette_act(Act::Add);
        app.palette_act(Act::AddGradient);
        assert!(
            app.file_text().contains("let color2 = #808080\n") && app.file_text().contains("let grad1 = linear 90 #ffffff #1b1d24\n"),
            "{}",
            app.file_text()
        );
        draw(&mut app);

        // Every step is undone on its own, back to the file as it was.
        for _ in 0..9 {
            app.undo();
        }
        assert_eq!(app.file_text(), FILE);
    }

    #[test]
    fn an_object_takes_a_gradient_of_the_palette_and_gives_its_own_to_it() {
        let src = "artboard \"a\" 200x100
let ink = #1b1d24
let sky = linear 90 #6fb8ff #ffffff
rect a at 50 50 size 40 40 fill #ffffff
rect b at 100 50 size 40 40 gradient linear 0 ink #ffffff
";
        let mut app = fixture(src);
        let panel = |app: &mut App, id: &str| {
            let ctx = app.ctx.clone();
            let node = app.scene.doc.node(id).cloned().unwrap();
            ctx.run_ui(egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, vec2(320.0, 900.0))), ..Default::default() }, |ui| {
                app.fill_kind_ui(ui, &node);
                app.stroke_kind_ui(ui, &node);
            })
            .drop_without_applying_deltas();
        };
        panel(&mut app, "a");
        panel(&mut app, "b");
        let out = motif_script::set_gradient_ref(&app.src, &app.scene, "a", false, "sky");
        assert!(app.apply("gradient-ref", out));
        assert_eq!(app.scene.gradient_names.get(&("a".to_string(), false)).map(String::as_str), Some("sky"));
        panel(&mut app, "a");
        let out = motif_script::gradient_to_palette(&app.src, &app.scene, "b", false, &motif_script::free_gradient_name(&app.file_text()));
        assert!(app.apply("gradient-keep", out));
        assert!(app.src.contains("let grad1 = linear 0 ink #ffffff\n") && app.src.contains("rect b at 100 50 size 40 40 gradient grad1\n"), "{}", app.src);
        panel(&mut app, "b");
        draw(&mut app);
    }
}
