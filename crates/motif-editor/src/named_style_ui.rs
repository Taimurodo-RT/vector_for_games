//! Named styles in the panels: the style of the selected object with what the object writes
//! over it, «Сделать стилем», and the list of the file's styles. A style is a `style` line of
//! the script; the look of an object that takes it follows that line.

use crate::app::{App, fold};
use crate::theme::{self, ButtonKind, FAINT, LINE, MUTED};
use egui::{Color32, RichText, Sense, Stroke, StrokeKind, vec2};
use motif_core::{Color, Node, ShapeKind};

/// What a click in the style rows asks for.
pub(crate) enum Act {
    Make,
    Assign(String),
    Detach,
    /// One of the object's own properties back to the style, or all of them.
    Reset(Option<String>),
    WriteInto,
    Rename(String, String),
    Delete(String),
    /// Every object of the first style takes the second.
    Replace(String, String),
    Select(String),
    /// The theme shown no longer changes this style.
    Unthemed(String),
}

/// A look property in the words of the panels: `r` is «скругление».
fn key_label(key: &str) -> &str {
    match key {
        "fill" => "заливка",
        "gradient" => "градиент",
        "stroke" => "обводка",
        "stroke-gradient" => "градиент обводки",
        "dash" => "пунктир",
        "cap" => "концы линии",
        "join" => "углы линии",
        "outline" => "слои обводки",
        "opacity" => "прозрачность",
        "blend" => "смешивание",
        "r" => "скругление",
        "shadow" => "тень",
        "inset" => "внутренняя тень",
        "glow" => "свечение",
        "blur" => "размытие",
        "noise" => "шум",
        "elevation" => "высота",
        "rough" => "неровность",
        "boil" => "дрожание линии",
        "bands" => "ступени цвета",
        "hatch" => "штриховка",
        "grain" => "зерно",
        "seed" => "случайность кисти",
        "font" => "шрифт",
        "font-size" => "размер букв",
        "letter-spacing" => "между буквами",
        "line-height" => "между строками",
        "align" => "выравнивание",
        other => other,
    }
}

/// An own property as the panel names it; `no shadow` is «тень: нет».
fn own_label(own: &str) -> String {
    match own.strip_prefix("no ") {
        Some(key) => format!("{}: нет", key_label(key)),
        None => key_label(own).to_string(),
    }
}

fn c32(c: Color) -> Color32 {
    Color32::from_rgba_unmultiplied(c.r, c.g, c.b, c.a)
}

/// A small square painted like an object of the style: its fill or gradient inside, its stroke
/// around. A style nothing takes yet is an empty frame.
fn sample(ui: &mut egui::Ui, like: Option<&Node>) -> egui::Response {
    let (r, resp) = ui.allocate_exact_size(vec2(22.0, 22.0), Sense::click());
    let inner = r.shrink(2.0);
    let radius = like.map_or(4.0, |n| (n.radius as f32).clamp(0.0, 9.0));
    match like {
        Some(n) => {
            match n.gradient.as_ref().filter(|g| g.colors.len() >= 2) {
                Some(g) => {
                    let (top, bottom) = (c32(g.colors[0]), c32(*g.colors.last().unwrap()));
                    let mut mesh = egui::Mesh::default();
                    mesh.colored_vertex(inner.left_top(), top);
                    mesh.colored_vertex(inner.right_top(), top);
                    mesh.colored_vertex(inner.right_bottom(), bottom);
                    mesh.colored_vertex(inner.left_bottom(), bottom);
                    mesh.add_triangle(0, 1, 2);
                    mesh.add_triangle(0, 2, 3);
                    ui.painter().add(mesh);
                }
                None => {
                    if let Some(fill) = n.fill {
                        ui.painter().rect_filled(inner, radius, c32(fill));
                    }
                }
            }
            let stroke =
                n.stroke.filter(|_| n.stroke_width > 0.0).map_or(Stroke::new(1.0, LINE), |c| Stroke::new((n.stroke_width as f32).clamp(1.0, 3.0), c32(c)));
            ui.painter().rect_stroke(inner, radius, stroke, StrokeKind::Inside);
        }
        None => {
            ui.painter().rect_stroke(inner, radius, Stroke::new(1.0, LINE), StrokeKind::Inside);
        }
    }
    resp
}

/// Said when an edit of the whole file cannot read one of its boards.
const OTHER_BOARD_ERRORS: &str = "Стиль общий для холстов файла, а на одном из них ошибка в скрипте: сначала исправьте её";

impl App {
    /// The objects that take style `name`, in the order they are drawn.
    fn styled_with(&self, name: &str) -> Vec<String> {
        self.scene.doc.nodes.iter().filter(|n| self.scene.styled.get(&n.id).is_some_and(|s| s == name)).map(|n| n.id.clone()).collect()
    }

    /// For the selected object: the style it takes, a list to pick another, «Сделать стилем», and
    /// what the object writes over its style.
    pub(crate) fn style_row(&mut self, ui: &mut egui::Ui, node: &Node) {
        if node.kind == ShapeKind::Bone {
            return;
        }
        let generated = self.scene.sites.get(&node.id).is_some_and(|s| s.generated);
        let current = self.scene.styled.get(&node.id).cloned();
        let names: Vec<String> = self.scene.styles.iter().map(|s| s.name.clone()).collect();
        let own = motif_script::own_look(&self.scene, &node.id);
        let mut act = None;
        fold(ui, "Стиль", |ui| {
            ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
            if generated {
                let text = match &current {
                    Some(name) => format!("Стиль «{name}». Объект создан циклом или символом: стиль пишется в его строке в скрипте."),
                    None => "Объект создан циклом или символом: стиль пишется в его строке в скрипте (style имя).".to_string(),
                };
                ui.label(RichText::new(text).size(11.5).color(FAINT));
                return;
            }
            ui.horizontal(|ui| {
                egui::ComboBox::from_id_salt(("named-style", &node.id))
                    .width((ui.available_width() - 140.0).max(90.0))
                    .selected_text(current.as_deref().unwrap_or("без стиля"))
                    .show_ui(ui, |ui| {
                        if ui.selectable_label(current.is_none(), "без стиля").on_hover_text("Объект сохранит вид: то, что давал стиль, запишется в его строку").clicked()
                            && current.is_some()
                        {
                            act = Some(Act::Detach);
                        }
                        for name in &names {
                            if ui.selectable_label(current.as_deref() == Some(name), name).clicked() && current.as_deref() != Some(name) {
                                act = Some(Act::Assign(name.clone()));
                            }
                        }
                    });
                let can_make = current.is_some() || !own.is_empty();
                let tip = if can_make {
                    "Вид объекта (заливка, обводка, скругление, тени, кисть, шрифт) получит имя и встанет строкой style в начало скрипта. Другим объектам стиль назначается в этом списке"
                } else {
                    "У объекта пока нет своего вида: задайте заливку или обводку"
                };
                if ui.add_enabled_ui(can_make, |ui| theme::button(ui, "Сделать стилем", ButtonKind::Normal)).inner.on_hover_text(tip).clicked() {
                    act = Some(Act::Make);
                }
            });
            let Some(name) = &current else {
                if names.is_empty() {
                    ui.label(RichText::new("Стиль хранит вид под именем. Поменяли стиль — изменились все объекты, которые его берут.").size(11.5).color(FAINT));
                }
                return;
            };
            if own.is_empty() {
                ui.label(
                    RichText::new("Объект выглядит как стиль. Меняйте его как обычно: правка станет его собственным свойством, стиль не изменится.")
                        .size(11.5)
                        .color(FAINT),
                );
                return;
            }
            ui.label(RichText::new("Своё поверх стиля (щелчок возвращает как в стиле):").size(12.0).color(MUTED));
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = vec2(4.0, 4.0);
                for key in &own {
                    if theme::button(ui, &format!("{} ×", own_label(key)), ButtonKind::Normal)
                        .on_hover_text("Убрать своё свойство: объект возьмёт его из стиля")
                        .clicked()
                    {
                        act = Some(Act::Reset(Some(key.clone())));
                    }
                }
            });
            ui.horizontal(|ui| {
                let hint = match self.scene.theme.as_deref() {
                    Some(theme) => format!(
                        "Свои свойства этого объекта запишутся в стиль «{name}» темы «{theme}»: в ней так же изменятся все объекты, которые его берут. У объекта они останутся: его строка общая для всех тем"
                    ),
                    None => format!("Свои свойства этого объекта уйдут в стиль «{name}»: так же изменятся все объекты, которые его берут"),
                };
                if theme::button(ui, "Записать в стиль", ButtonKind::Primary).on_hover_text(hint).clicked()
                {
                    act = Some(Act::WriteInto);
                }
                if theme::button(ui, "Как в стиле", ButtonKind::Normal).on_hover_text("Убрать все свои свойства: объект снова выглядит как стиль").clicked()
                {
                    act = Some(Act::Reset(None));
                }
            });
        });
        if let Some(act) = act {
            self.style_act(act, std::slice::from_ref(&node.id));
        }
    }

    /// For several selected objects: one style for all of them.
    pub(crate) fn style_row_many(&mut self, ui: &mut egui::Ui, ids: &[String]) {
        let names: Vec<String> = self.scene.styles.iter().map(|s| s.name.clone()).collect();
        if names.is_empty() {
            return;
        }
        let taken: Vec<Option<&String>> = ids.iter().map(|id| self.scene.styled.get(id)).collect();
        let shared = taken.first().copied().flatten().filter(|first| taken.iter().all(|t| *t == Some(*first))).cloned();
        let shown = match &shared {
            Some(name) => name.as_str(),
            None if taken.iter().any(Option::is_some) => "разные",
            None => "без стиля",
        };
        let mut act = None;
        fold(ui, "Стиль", |ui| {
            egui::ComboBox::from_id_salt("named-style-many").width(ui.available_width() - 8.0).selected_text(shown).show_ui(ui, |ui| {
                for name in &names {
                    if ui.selectable_label(shared.as_ref() == Some(name), name).clicked() {
                        act = Some(Act::Assign(name.clone()));
                    }
                }
            });
            ui.label(RichText::new("Стиль назначается всем выбранным объектам сразу.").size(11.5).color(FAINT));
        });
        if let Some(act) = act {
            self.style_act(act, ids);
        }
    }

    /// The styles of the file: a sample, the name (editable), how many objects take it.
    pub(crate) fn styles_section(&mut self, ui: &mut egui::Ui) {
        let styles: Vec<String> = self.scene.styles.iter().map(|s| s.name.clone()).collect();
        // The styles the theme shown writes its own line for.
        let themed: Vec<String> = self.scene.styles.iter().filter(|s| s.themed).map(|s| s.name.clone()).collect();
        let shown = self.scene.theme.clone().unwrap_or_default();
        let mut act = None;
        fold(ui, "Стили", |ui| {
            ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
            if styles.is_empty() {
                let text = "Стилей нет. Выделите объект и нажмите «Сделать стилем»: его вид получит имя, и его можно будет назначать другим объектам. Поменяли стиль — изменились все.";
                ui.label(RichText::new(text).size(11.5).color(FAINT));
                return;
            }
            for name in &styles {
                let users = self.styled_with(name);
                let like = users.first().and_then(|id| self.scene.doc.node(id));
                ui.horizontal(|ui| {
                    // The name takes what the count and the buttons leave. While a theme is shown
                    // the count moves into the hint: «↺» needs the room.
                    let count = format!("объектов: {}", users.len());
                    let counted = shown.is_empty();
                    let hint = if let Some(file) = self.style_from(name) {
                        format!("Выбрать объекты этого стиля ({count}). Стиль записан в подключённом файле «{file}»: меняют его там")
                    } else if counted {
                        "Выбрать объекты этого стиля".to_string()
                    } else {
                        format!("Выбрать объекты этого стиля ({count})")
                    };
                    if sample(ui, like).on_hover_text(hint).clicked() && !users.is_empty() {
                        act = Some(Act::Select(name.clone()));
                    }
                    let swap = if styles.len() > 1 { 34.0 } else { 0.0 } + if themed.contains(name) { 34.0 } else { 0.0 };
                    let label = if counted { ui.painter().layout_no_wrap(count.clone(), egui::FontId::proportional(11.5), FAINT).size().x + 28.0 } else { 8.0 };
                    let taken = label + 30.0 + swap;
                    if let Some(new) = crate::machine_ui::name_edit(ui, ("style-name", name), name, (ui.available_width() - taken).max(60.0)) {
                        act = Some(Act::Rename(name.clone(), new));
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if theme::button(ui, "×", ButtonKind::Normal).on_hover_text("Удалить стиль. Объекты сохранят вид: он запишется в их строки").clicked()
                        {
                            act = Some(Act::Delete(name.clone()));
                        }
                        if styles.len() > 1 {
                            let resp = theme::button(ui, "⇄", ButtonKind::Normal).on_hover_text("Заменить везде: все объекты этого стиля возьмут другой");
                            theme::color_popup(&resp).show(|ui| {
                                ui.label(RichText::new(format!("Заменить «{name}» везде на:")).size(11.5).color(MUTED));
                                for other in styles.iter().filter(|o| *o != name) {
                                    if theme::button(ui, other, ButtonKind::Normal).clicked() {
                                        act = Some(Act::Replace(name.clone(), other.clone()));
                                        ui.close();
                                    }
                                }
                            });
                        }
                        if themed.contains(name) && crate::palette_ui::theme_reset_button(ui, &shown) {
                            act = Some(Act::Unthemed(name.clone()));
                        }
                        if counted {
                            ui.label(RichText::new(count).size(11.5).color(FAINT));
                        }
                    });
                });
            }
            let text = "Чтобы поменять стиль, выберите объект с ним, измените вид и нажмите «Записать в стиль». Щелчок по квадратику выбирает объекты стиля.";
            ui.label(RichText::new(text).size(11.5).color(FAINT));
        });
        if let Some(act) = act {
            self.style_act(act, &[]);
        }
    }

    pub(crate) fn style_act(&mut self, act: Act, ids: &[String]) {
        if let Act::Select(name) = &act {
            let users = self.styled_with(name);
            self.status = format!("Выбраны объекты стиля «{name}»: {}", users.len());
            self.select_list(users);
            return;
        }
        if !self.can_edit() {
            return;
        }
        let first = ids.first().map(String::as_str).unwrap_or_default();
        // A style of an included file is a line of that file.
        let changed = match &act {
            Act::WriteInto => self.scene.styled.get(first).cloned(),
            Act::Unthemed(name) | Act::Rename(name, _) | Act::Delete(name) => Some(name.clone()),
            _ => None,
        };
        if let Some(name) = changed
            && let Some(file) = self.style_from(&name)
        {
            self.refuse_included(&format!("Стиль «{name}»"), &file);
            return;
        }
        let (src, scene) = (&self.src, &self.scene);
        match act {
            Act::Make => {
                let name = motif_script::free_style_name(&self.names_text());
                let out = motif_script::make_style(src, scene, first, &name);
                if self.apply("style-make", out) {
                    self.status =
                        format!("Вид объекта «{first}» стал стилем «{name}». Имя меняется в панели артборда (щелчок по пустому месту), раздел «Стили»");
                }
            }
            Act::Assign(name) => {
                let out = motif_script::assign_style(src, scene, ids, &name);
                let skipped = ids.iter().filter(|id| scene.sites.get(*id).is_none_or(|s| s.generated)).count();
                if self.apply("style-assign", out) {
                    self.status = match skipped {
                        0 => format!("Стиль «{name}» назначен"),
                        n => format!("Стиль «{name}» назначен. Пропущено объектов из циклов и символов: {n}"),
                    };
                } else if skipped > 0 {
                    self.status = "Объектам из циклов и символов стиль пишется в их строке в скрипте".into();
                }
            }
            Act::Detach => {
                let out = motif_script::detach_style(src, scene, first);
                if self.apply("style-detach", out) {
                    self.status = "Объект больше не следует за стилем, вид сохранён".into();
                }
            }
            Act::Reset(key) => {
                let out = motif_script::reset_to_style(src, scene, first, key.as_deref());
                self.apply("style-reset", out);
            }
            Act::WriteInto => {
                let name = scene.styled.get(first).cloned().unwrap_or_default();
                let out = motif_script::write_into_style(src, scene, first);
                if self.apply("style-write", out) {
                    let users = self.styled_with(&name).len();
                    self.status = match self.scene.theme.as_deref() {
                        Some(theme) => format!(
                            "Стиль «{name}» обновлён в теме «{theme}»: в ней изменились все объекты, которые его берут ({users}). Исходная сцена и свои свойства объекта остались"
                        ),
                        None => format!("Стиль «{name}» обновлён: изменились все объекты, которые его берут ({users})"),
                    };
                }
            }
            Act::Unthemed(name) => {
                let out = motif_script::reset_in_theme(src, scene, &name, true);
                if self.apply_once("theme-reset", out) {
                    self.status = format!("Стиль «{name}» в теме «{}» снова как в исходной сцене", self.theme);
                }
            }
            Act::Rename(old, new) => {
                let new = new.trim().to_string();
                if !motif_script::style_name_ok(scene, &new) {
                    self.status =
                        format!("Имя «{new}» не подходит: нужно одно слово из букв, цифр, «_» и «-», не занятое другим стилем и не название свойства");
                    return;
                }
                // A style of the shared head is taken on other boards too: they follow.
                let (file, base) = (self.file_text(), self.path.as_ref().and_then(|p| p.parent()));
                let out = motif_script::rename_style_in_file(&file, base, self.boards.active, &old, &new);
                if out.is_none() {
                    self.status = match motif_script::parse_boards(&file, base) {
                        Ok(_) => format!("Имя «{new}» занято стилем на другом холсте этого файла"),
                        Err(_) => OTHER_BOARD_ERRORS.into(),
                    };
                } else if self.apply_file("style-rename", out) {
                    self.status = format!("Стиль «{old}» теперь «{new}»");
                }
            }
            Act::Delete(name) => {
                let users = self.styled_with(&name).len();
                let (file, base) = (self.file_text(), self.path.as_ref().and_then(|p| p.parent()));
                let out = motif_script::delete_style_in_file(&file, base, self.boards.active, &name);
                if out.is_none() {
                    self.status = OTHER_BOARD_ERRORS.into();
                } else if self.apply_file("style-delete", out) {
                    self.status = format!("Стиль «{name}» удалён. Объекты сохранили вид: {users}");
                }
            }
            Act::Replace(from, to) => {
                let (file, base) = (self.file_text(), self.path.as_ref().and_then(|p| p.parent()));
                match motif_script::replace_style(&file, base, self.boards.active, &from, &to) {
                    Some((out, lines)) => {
                        if self.apply_file("style-replace", Some(out)) {
                            self.status = format!("Стиль «{from}» заменён на «{to}», строк: {lines}");
                        }
                    }
                    None => {
                        self.status = match motif_script::parse_boards(&file, base) {
                            Ok(_) => format!("Заменить не получилось: стиль «{from}» никто не берёт, или «{to}» виден не на всех холстах, где он используется"),
                            Err(_) => OTHER_BOARD_ERRORS.into(),
                        };
                    }
                }
            }
            Act::Select(_) => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::editing_tests::fixture;
    use motif_core::{Prop, Value};

    const SRC: &str = "artboard \"a\" 300x200
rect a at 60 60 size 80 40 r 8 fill #ffffff stroke #1b1d24 2
rect b at 160 60 size 80 40 fill #3b5bff
text t at 60 140 content \"Текст\" font-size 20 fill #1b1d24
group g at 0 0 {
  for i in 0..2 {
    ellipse dot{i} at {200 + i * 20} 140 size 10 10 fill #ff5a36
  }
}
";

    /// One frame of the panels the styles live in, for the current selection.
    fn draw(app: &mut App) {
        let ctx = app.ctx.clone();
        let node = app.selected.clone().and_then(|id| app.scene.doc.node(&id).cloned());
        let ids = app.selection();
        ctx.run_ui(
            egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, vec2(320.0, 900.0))), ..Default::default() },
            |ui| match &node {
                Some(node) if ids.len() == 1 => app.style_row(ui, node),
                Some(_) => app.style_row_many(ui, &ids),
                None => app.styles_section(ui),
            },
        )
        .drop_without_applying_deltas();
    }

    fn ids(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn a_look_becomes_a_style_and_other_objects_take_it() {
        let mut app = fixture(SRC);
        app.select_only(Some("a".into()));
        draw(&mut app);
        app.style_act(Act::Make, &ids(&["a"]));
        assert!(app.src.contains("style style1 r 8 fill #ffffff stroke #1b1d24 2\n"), "{}", app.src);
        assert!(app.src.contains("rect a at 60 60 size 80 40 style style1\n"), "{}", app.src);
        assert!(app.status.contains("стал стилем «style1»"), "{}", app.status);
        draw(&mut app);

        // The name is changed in the list of styles.
        app.style_act(Act::Rename("style1".into(), "fill".into()), &[]);
        assert!(app.status.contains("не подходит"), "{}", app.status);
        app.style_act(Act::Rename("style1".into(), " card ".into()), &[]);
        assert!(app.src.contains("style card r 8") && app.src.contains("size 80 40 style card\n"), "{}", app.src);

        // Several objects take it at once; the ones made by the loop are passed by and said so.
        app.select_list(ids(&["b", "t", "dot0"]));
        draw(&mut app);
        app.style_act(Act::Assign("card".into()), &ids(&["b", "t", "dot0"]));
        assert!(app.src.contains("rect b at 160 60 size 80 40 style card\n"), "{}", app.src);
        assert!(app.src.contains("text t at 60 140 content \"Текст\" font-size 20 style card\n"), "{}", app.src);
        assert!(app.status.contains("Пропущено объектов из циклов и символов: 1"), "{}", app.status);
        let doc = &app.scene.doc;
        assert_eq!(doc.node("b").unwrap().fill, doc.node("a").unwrap().fill);
        assert_eq!(doc.node("b").unwrap().radius, 8.0);
        assert_eq!(app.styled_with("card"), ["a", "b", "t"]);

        // Nothing selected: the list of styles, and a click on a sample selects its objects.
        app.select_only(None);
        draw(&mut app);
        app.style_act(Act::Select("card".into()), &[]);
        assert_eq!(app.selection(), ["a", "b", "t"]);
        // One undo step back per action.
        app.undo();
        assert!(!app.src.contains("rect b at 160 60 size 80 40 style card"), "{}", app.src);
    }

    #[test]
    fn an_edit_stays_on_the_object_until_it_is_written_into_the_style() {
        let mut app = fixture(&format!(
            "{}style card r 8 fill #ffffff stroke #1b1d24 2 glow 6 #ffd34d\n",
            SRC.replace("r 8 fill #ffffff stroke #1b1d24 2", "style card").replace("fill #3b5bff", "style card")
        ));
        app.select_only(Some("a".into()));
        // The usual edits of the panels: a colour, and a property switched off.
        app.edit("a", Prop::Fill, Value::Color(Color::from_hex("#ffd23f").unwrap()));
        app.apply_styles("a", vec![("glow", None)]);
        assert!(app.src.contains("rect a at 60 60 size 80 40 style card fill #ffd23f no glow\n"), "{}", app.src);
        assert!(app.src.contains("style card r 8 fill #ffffff stroke #1b1d24 2 glow 6 #ffd34d\n"), "{}", app.src);
        assert!(app.scene.doc.node("b").unwrap().glow.is_some());
        assert_eq!(motif_script::own_look(&app.scene, "a"), ["fill", "no glow"]);
        draw(&mut app);

        // One of them back to the style.
        app.style_act(Act::Reset(Some("no glow".into())), &ids(&["a"]));
        assert!(app.scene.doc.node("a").unwrap().glow.is_some());
        // The other one into the style: the second card follows.
        app.style_act(Act::WriteInto, &ids(&["a"]));
        assert!(app.src.contains("style card r 8 fill #ffd23f stroke #1b1d24 2 glow 6 #ffd34d\n"), "{}", app.src);
        assert!(app.src.contains("rect a at 60 60 size 80 40 style card\n"), "{}", app.src);
        assert_eq!(app.scene.doc.node("b").unwrap().fill, Color::from_hex("#ffd23f"));
        assert!(app.status.contains("(2)"), "{}", app.status);

        // Detached and deleted: the picture stays.
        let before = app.scene.doc.clone();
        app.style_act(Act::Detach, &ids(&["a"]));
        assert!(app.src.contains("rect a at 60 60 size 80 40 r 8 fill #ffd23f stroke #1b1d24 2 glow 6 #ffd34d\n"), "{}", app.src);
        app.style_act(Act::Delete("card".into()), &[]);
        assert!(!app.src.contains("style"), "{}", app.src);
        assert_eq!(app.scene.doc, before);
        assert!(app.status.contains("Объекты сохранили вид: 1"), "{}", app.status);
        app.select_only(None);
        draw(&mut app);
    }

    #[test]
    fn an_object_of_a_loop_shows_its_style_and_is_not_edited() {
        let src = SRC
            .replace("size 10 10 fill #ff5a36", "size 10 10 style dots")
            .replace("artboard \"a\" 300x200\n", "artboard \"a\" 300x200\nstyle dots fill #ff5a36\n");
        let mut app = fixture(&src);
        app.select_only(Some("dot1".into()));
        draw(&mut app);
        app.style_act(Act::Detach, &ids(&["dot1"]));
        app.style_act(Act::Make, &ids(&["dot1"]));
        assert_eq!(app.src, src);
        // Deleting the style writes the look into the line of the loop.
        app.style_act(Act::Delete("dots".into()), &[]);
        assert!(app.src.contains("ellipse dot{i} at {200 + i * 20} 140 size 10 10 fill #ff5a36\n"), "{}", app.src);
    }

    #[test]
    fn renaming_and_deleting_a_shared_style_reaches_the_other_boards() {
        let file = "let ink = #1b1d24
style card fill #ffffff stroke ink 2
artboard \"one\" 100x100
rect a at 50 50 size 40 40 style card
artboard \"two\" 100x100 cell 1 0
rect b at 50 50 size 40 40 style card r 4
";
        let docs = |app: &App| -> Vec<motif_core::Document> {
            motif_script::parse_boards(&app.file_text(), None).unwrap_or_else(|e| panic!("{e:?}\n{}", app.file_text())).into_iter().map(|s| s.doc).collect()
        };
        let mut app = fixture(file);
        let before = docs(&app);
        app.style_act(Act::Rename("card".into(), "plate".into()), &[]);
        assert!(app.status.contains("теперь «plate»"), "{}", app.status);
        assert!(app.file_text().contains("rect b at 50 50 size 40 40 style plate r 4\n"), "{}", app.file_text());
        assert_eq!(docs(&app), before);
        assert_eq!(app.boards.active, 0);
        app.undo();
        assert_eq!(app.file_text(), file);

        // From the second board: the first keeps its look when the style goes.
        app.switch_board(1);
        app.style_act(Act::Delete("card".into()), &[]);
        assert!(app.status.contains("удалён"), "{}", app.status);
        assert!(app.file_text().contains("rect a at 50 50 size 40 40 fill #ffffff stroke ink 2\n"), "{}", app.file_text());
        assert!(!app.file_text().contains("style"), "{}", app.file_text());
        assert_eq!(docs(&app), before);
        assert_eq!(app.boards.active, 1);
        app.undo();
        assert_eq!(app.file_text(), file);
    }
}
