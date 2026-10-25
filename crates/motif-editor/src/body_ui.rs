//! The «3D-тело» section: pushing a flat shape into depth and turning it in space.

use crate::app::{App, fold};
use crate::style_ui::StyleEdit;
use crate::theme::{self, FAINT, FG};
use egui::{RichText, vec2};
use motif_core::{Node, Prop, ShapeKind, Value};

impl App {
    /// Depth, taper and the turn are keyable and go to `changes` and `diamonds` like the other
    /// fields of the panel; turning the body on or off is written through `styles`.
    pub(crate) fn body_section(
        &self,
        ui: &mut egui::Ui,
        node: &Node,
        half: f32,
        changes: &mut Vec<(Prop, Value)>,
        diamonds: &mut Vec<Prop>,
        styles: &mut Vec<StyleEdit>,
    ) {
        if node.kind.is_container() || node.kind == ShapeKind::Bone {
            return;
        }
        let id = node.id.clone();
        fold(ui, "3D-тело", |ui| {
            ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
            let mut on = node.body.is_some();
            if ui.checkbox(&mut on, RichText::new("Объём").size(12.5).color(FG)).changed() {
                // As deep as the shape is wide: a square becomes a cube.
                let depth = node.width.abs().max(1.0).round();
                styles.push(("extrude", on.then(|| format!("extrude {}", motif_script::fmt_num(depth)))));
            }
            let Some(_) = node.body else {
                ui.label(
                    RichText::new(
                        "Фигура выдавливается в глубину и поворачивается в пространстве: квадрат — куб, круг — цилиндр, с сужением — конус и пирамида.",
                    )
                    .size(11.5)
                    .color(FAINT),
                );
                return;
            };
            let mut field = |ui: &mut egui::Ui, label: &str, prop: Prop, range: std::ops::RangeInclusive<f64>, speed: f64, digits: usize| {
                let v = node.get(prop).as_num().unwrap_or(0.0);
                let out = theme::num_field(ui, half, label, v, speed, range, digits, self.key_state(&id, prop));
                if let Some(nv) = out.changed {
                    let k = 10f64.powi(digits as i32);
                    changes.push((prop, Value::Num((nv * k).round() / k)));
                }
                if out.diamond {
                    diamonds.push(prop);
                }
            };
            ui.horizontal(|ui| {
                field(ui, "Глубина", Prop::Depth, -10_000.0..=10_000.0, 0.5, 1);
                field(ui, "Сужение", Prop::Taper, 0.0..=4.0, 0.005, 2);
            });
            ui.horizontal(|ui| {
                field(ui, "Разворот", Prop::Yaw, -100_000.0..=100_000.0, 0.5, 1);
                field(ui, "Наклон", Prop::Pitch, -100_000.0..=100_000.0, 0.5, 1);
            });
            ui.label(
                RichText::new(
                    "Сужение — размер задней грани: 1 — призма или цилиндр, 0 — пирамида или конус. «Разворот» (yaw, влево-вправо) и «Наклон» (pitch) поворачивают тело в пространстве, «Поворот» — в плоскости. Грани освещает свет сцены, обводка рисуется только по рёбрам.",
                )
                .size(11.5)
                .color(FAINT),
            );
        });
    }
}
