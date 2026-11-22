//! Live operations in the editor: the buttons that put selected shapes into a `combine` group,
//! the section «Живая операция» of such a group and of its shapes, and the dashed outlines of
//! the shapes an operation has taken in, so they can be seen and grabbed.

use crate::app::{App, fold};
use crate::canvas::View;
use crate::theme::{self, FAINT, FG, MUTED, SEL};
use egui::{Rect, RichText, Stroke};
use motif_core::{Document, Node, ShapeKind};
use motif_script::BoolOp;

/// What the section of an operation asks for.
enum Ask {
    Set(Option<BoolOp>),
    Flatten,
    Select(String),
}

fn tip(op: BoolOp) -> &'static str {
    match op {
        BoolOp::Union => "Объединить: всё, что покрывает любая фигура",
        BoolOp::Subtract => "Вычесть: нижняя фигура минус те, что выше",
        BoolOp::Intersect => "Пересечь: только общая часть",
        BoolOp::Exclude => "Исключить: общие части становятся дырками",
    }
}

/// How an operation is called in a sentence.
fn noun(op: BoolOp) -> &'static str {
    match op {
        BoolOp::Union => "объединение",
        BoolOp::Subtract => "вычитание",
        BoolOp::Intersect => "пересечение",
        BoolOp::Exclude => "исключение",
    }
}

impl App {
    pub(crate) fn base_dir(&self) -> Option<std::path::PathBuf> {
        self.path.as_ref().and_then(|p| p.parent()).map(|p| p.to_path_buf())
    }

    /// The selected shapes go into a new group with a live operation; nothing moves.
    pub(crate) fn combine_selection(&mut self, op: BoolOp) {
        if !self.can_edit() {
            return;
        }
        match motif_script::combine_nodes(&self.src, &self.scene, &self.selection(), op.live(), self.base_dir().as_deref()) {
            Ok((out, group)) => {
                self.apply_once("combine", Some(out));
                self.point_edit = None;
                self.select_only(Some(group.clone()));
                self.status = format!("{}: живая операция «{group}». Фигуры остались — двигайте их, результат пересчитается", op.name());
            }
            Err(e) => self.status = format!("{}: {e}", op.name()),
        }
    }

    /// Another operation for group `id`, or none: a plain group again.
    pub(crate) fn set_combine(&mut self, id: &str, op: Option<BoolOp>) {
        if !self.can_edit() {
            return;
        }
        match motif_script::set_combine(&self.src, &self.scene, id, op.map(BoolOp::live)) {
            Some(out) => {
                // A soft body or particles inside are named by the script itself.
                if let Err(e) = motif_script::parse_in(&out, self.base_dir().as_deref()) {
                    self.status = format!("Операция не включается: {}", e.first().map_or("скрипт не читается", |e| e.message.as_str()));
                    return;
                }
                self.apply_once("combine", Some(out));
                self.status = match op {
                    Some(op) => format!("Операция группы «{id}»: {}", noun(op)),
                    None => format!("«{id}» снова обычная группа: каждая фигура рисует себя"),
                };
            }
            None => self.status = "Операцию группы из цикла меняют в тексте цикла".into(),
        }
    }

    /// «Разобрать»: the group becomes the one outline it draws.
    pub(crate) fn flatten_combine(&mut self, id: &str) {
        if !self.can_edit() {
            return;
        }
        match motif_script::flatten_combine(&self.src, &self.scene, id, self.base_dir().as_deref()) {
            Ok((out, path)) => {
                self.apply_once("combine", Some(out));
                self.point_edit = None;
                self.select_only(Some(path.clone()));
                self.status = format!("Операция разобрана: «{path}» теперь один контур, Enter — править точки");
            }
            Err(e) => self.status = format!("Не разобрать: {e}"),
        }
    }

    /// The section of the inspector: for a group, its operation; for a shape of an operation,
    /// what it does there.
    pub(crate) fn combine_ui(&mut self, ui: &mut egui::Ui, node: &Node) {
        let doc = &self.scene.doc;
        let Some(i) = doc.index_of(&node.id) else { return };
        let mut ask = None;
        if node.kind == ShapeKind::Group {
            // A copy of a symbol is set up in the symbol.
            if self.scene.instances.contains_key(&node.id) || node.clip {
                return;
            }
            let now = node.combine.map(BoolOp::of);
            let bearer = doc.combine_bearer(i).map(|b| doc.nodes[b].display_name().to_string());
            let shapes = doc.combine_members(i).len();
            let moved = doc.combine_moved_by(i).map(str::to_string);
            fold(ui, "Живая операция", |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;
                    for op in BoolOp::ALL {
                        if crate::selection::bool_button(ui, op, tip(op), now == Some(op)).clicked() && now != Some(op) {
                            ask = Some(Ask::Set(Some(op)));
                        }
                    }
                });
                let hint = match (now, &bearer) {
                    (Some(op), Some(b)) if shapes > 1 => {
                        format!(
                            "Сейчас: {}. Фигуры группы дают один контур, рисует его «{b}» — нижняя в слоях. Остальные видны пунктиром, когда группа выбрана.",
                            noun(op)
                        )
                    }
                    (Some(_), _) => "В группе меньше двух видимых фигур: операции не из чего складывать.".into(),
                    (None, _) if shapes > 1 => "Обычная группа. Кнопка сложит её фигуры в один контур; сами фигуры останутся.".into(),
                    (None, _) => "Обычная группа. Операция складывает фигуры группы в один контур: нужны хотя бы две.".into(),
                };
                ui.label(RichText::new(hint).size(11.5).color(MUTED));
                if now.is_some() {
                    if let Some(m) = &moved {
                        ui.label(RichText::new(format!("Считается заново на каждом кадре: «{m}» двигается в анимации.")).size(11.5).color(theme::ACCENT));
                    }
                    ui.horizontal_wrapped(|ui| {
                        if theme::button(ui, "Разобрать в контур", theme::ButtonKind::Normal)
                            .on_hover_text("Группа станет одним контуром с тем же видом. Фигуры исчезнут; Ctrl+Z вернёт")
                            .clicked()
                        {
                            ask = Some(Ask::Flatten);
                        }
                        if theme::button(ui, "Обычная группа", theme::ButtonKind::Normal)
                            .on_hover_text("Снять операцию: каждая фигура снова рисует себя")
                            .clicked()
                        {
                            ask = Some(Ask::Set(None));
                        }
                    });
                }
            });
        } else if let Some(g) = doc.combined_in(i).filter(|&g| doc.combine_members(g).len() > 1) {
            let group = &doc.nodes[g];
            let op = group.combine.map(BoolOp::of);
            let bearer = doc.combine_bearer(g);
            fold(ui, "Живая операция", |ui| {
                let what = op.map_or("операция", noun);
                let text = if bearer == Some(i) {
                    format!("Нижняя фигура операции «{}» ({what}): её заливка и обводка рисуют результат.", group.display_name())
                } else {
                    let b = bearer.map_or("нижняя фигура", |b| doc.nodes[b].display_name());
                    format!("Задаёт форму в операции «{}» ({what}). Её краска не видна: результат рисует «{b}».", group.display_name())
                };
                ui.label(RichText::new(text).size(11.5).color(FG));
                if theme::button(ui, "К операции", theme::ButtonKind::Normal).on_hover_text("Выбрать группу: сменить операцию или разобрать её").clicked()
                {
                    ask = Some(Ask::Select(group.id.clone()));
                }
                ui.label(RichText::new("Двигайте и меняйте фигуру как обычно: контур идёт следом.").size(11.5).color(FAINT));
            });
        }
        match ask {
            Some(Ask::Set(op)) => self.set_combine(&node.id, op),
            Some(Ask::Flatten) => self.flatten_combine(&node.id),
            Some(Ask::Select(id)) => self.select_only(Some(id)),
            None => {}
        }
    }

    /// The shapes of the operations the selection belongs to, each dashed along its own
    /// outline: what was taken into the result is still there to see and to grab.
    pub(crate) fn paint_combines(&self, painter: &egui::Painter, rect: Rect, view: View, doc: &Document) {
        let mut groups: Vec<usize> = Vec::new();
        for id in self.selection() {
            let Some(i) = doc.index_of(&id) else { continue };
            let own = doc.nodes[i].combine.is_some().then_some(i);
            for g in own.into_iter().chain(doc.combined_in(i)) {
                if !groups.contains(&g) {
                    groups.push(g);
                }
            }
        }
        let stroke = Stroke::new(1.0, SEL.gamma_multiply(0.75));
        for g in groups {
            let members = doc.combine_members(g);
            if members.len() < 2 {
                continue;
            }
            for k in members {
                if !doc.is_visible(k) {
                    continue;
                }
                if let Some(path) = doc.nodes[k].own_path() {
                    crate::game_ui::dashed_path(painter, rect, view, &(doc.world_transform(k) * path), stroke);
                }
            }
        }
    }
}
