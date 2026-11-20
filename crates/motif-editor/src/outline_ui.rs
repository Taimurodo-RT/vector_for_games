//! The outline commands in the «Контур» section: a shape becomes a path, a stroke becomes a
//! filled outline, and an outline is set off outwards or inwards.

use crate::app::App;
use crate::theme::{self, ButtonKind};
use eframe::egui;
use motif_core::{Node, ShapeKind};

/// What a button of the section asks for.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum OutlineCommand {
    ShapeToPath,
    StrokeToPath,
    /// By this many units: outwards when positive, inwards when negative.
    Offset(f64),
}

impl App {
    /// The buttons of the outline commands for `node`. A press is returned, not carried out:
    /// the panel first writes what its other fields changed.
    pub(crate) fn outline_buttons(&mut self, ui: &mut egui::Ui, node: &Node) -> Option<OutlineCommand> {
        let mut command = None;
        let shape = matches!(node.kind, ShapeKind::Rect | ShapeKind::Ellipse | ShapeKind::Star | ShapeKind::Polygon);
        let stroked = (node.stroke.is_some() || node.stroke_gradient.is_some()) && node.stroke_width > 0.0;
        ui.horizontal_wrapped(|ui| {
            if shape
                && theme::button(ui, "Фигуру в контур", ButtonKind::Normal)
                    .on_hover_text("Записать фигуру контуром той же формы: её точки можно править по Enter. Имя, место, заливка и ключи остаются")
                    .clicked()
            {
                command = Some(OutlineCommand::ShapeToPath);
            }
            ui.add_enabled_ui(stroked, |ui| {
                if theme::button(ui, "Обводку в контур", ButtonKind::Normal)
                    .on_hover_text("Превратить обводку в залитый контур: пунктир, концы и углы становятся его формой. У фигуры с заливкой контур обводки ложится отдельным объектом сверху")
                    .on_disabled_hover_text("У объекта нет обводки")
                    .clicked()
                {
                    command = Some(OutlineCommand::StrokeToPath);
                }
            });
        });
        ui.horizontal(|ui| {
            ui.label("Смещение");
            ui.add(egui::DragValue::new(&mut self.offset_by).speed(0.5).range(0.1..=10_000.0).max_decimals(1))
                .on_hover_text("На сколько единиц новый контур отстоит от этого");
            ui.checkbox(&mut self.offset_sharp, "Острые углы").on_hover_text("Углы нового контура остаются острыми. Без галочки они скругляются");
        });
        ui.horizontal(|ui| {
            if theme::button(ui, "Наружу", ButtonKind::Normal)
                .on_hover_text("Новый контур шире этого на «Смещение» с каждой стороны. Он ложится под объект, сам объект не меняется")
                .clicked()
            {
                command = Some(OutlineCommand::Offset(self.offset_by));
            }
            if theme::button(ui, "Внутрь", ButtonKind::Normal)
                .on_hover_text("Новый контур уже этого на «Смещение» с каждой стороны. Он ложится поверх объекта, сам объект не меняется")
                .clicked()
            {
                command = Some(OutlineCommand::Offset(-self.offset_by));
            }
        });
        command
    }

    /// Carries out an outline command on the object `id` as one undo step and selects what
    /// came out of it.
    pub(crate) fn outline_command(&mut self, id: &str, command: OutlineCommand) {
        if !self.can_edit() {
            return;
        }
        let done = match command {
            OutlineCommand::ShapeToPath => motif_script::shape_to_path(&self.src, &self.scene, id)
                .map(|out| (out, id.to_string(), format!("«{id}» теперь контур: точки правятся по Enter")))
                .map_err(|e| format!("Фигуру в контур: {e}")),
            OutlineCommand::StrokeToPath => motif_script::stroke_to_path(&self.src, &self.scene, id)
                .map(|(out, made, note)| {
                    let said = if made == id {
                        format!("Обводка «{id}» стала его контуром")
                    } else {
                        format!("Обводка «{id}» стала контуром «{made}»")
                    };
                    (out, made, note.map_or(said.clone(), |n| format!("{said}. {n}")))
                })
                .map_err(|e| format!("Обводку в контур: {e}")),
            OutlineCommand::Offset(by) => motif_script::offset_path(&self.src, &self.scene, id, by, self.offset_sharp)
                .map(|(out, made, note)| {
                    let side = if by > 0.0 { "наружу" } else { "внутрь" };
                    let by = (by.abs() * 10.0).round() / 10.0;
                    let said = format!("Смещение {side} на {by}: новый контур «{made}»");
                    (out, made, note.map_or(said.clone(), |n| format!("{said}. {n}")))
                })
                .map_err(|e| format!("Смещение: {e}")),
        };
        match done {
            Ok((out, made, said)) => {
                // The points being edited belong to the outline as it was.
                self.point_edit = None;
                self.apply_once("outline", Some(out));
                self.select_only(Some(made));
                self.status = said;
            }
            Err(e) => self.status = e,
        }
    }
}
