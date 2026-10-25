//! The «Интерфейс» section: marking a piece of game UI and its nine-slice margins.

use crate::app::{App, section};
use crate::style_ui::StyleEdit;
use crate::theme::{self, FAINT, FG, KeyState};
use egui::{RichText, vec2};
use motif_core::{Node, ShapeKind};
use motif_script::fmt_num;

/// `slice` written as short as it reads: one value, two (sides, top-bottom) or four.
pub fn slice_text(s: [f64; 4]) -> String {
    let [l, t, r, b] = s;
    let v: Vec<f64> = if l == t && t == r && r == b {
        vec![l]
    } else if l == r && t == b {
        vec![l, t]
    } else {
        vec![l, t, r, b]
    };
    format!("slice {}", v.iter().map(|x| fmt_num(*x)).collect::<Vec<_>>().join(" "))
}

impl App {
    /// Both flags are written through `styles`: `ui` alone makes a fixed-size piece,
    /// `slice` a stretchable one.
    pub(crate) fn kit_section(&self, ui: &mut egui::Ui, node: &Node, half: f32, styles: &mut Vec<StyleEdit>) {
        if node.kind == ShapeKind::Bone {
            return;
        }
        section(ui, |ui| {
            theme::caption(ui, "Интерфейс");
            ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
            let mut on = node.ui || node.slice.is_some();
            if ui.checkbox(&mut on, RichText::new("Деталь интерфейса").size(12.5).color(FG)).changed() {
                styles.push(("ui", on.then(|| "ui".to_string())));
                if !on {
                    styles.push(("slice", None));
                }
            }
            if !on {
                ui.label(
                    RichText::new("Кнопки, панели, рамки: экспорт «UI-набор для Godot» снимет каждую деталь отдельной картинкой в каждом состоянии.")
                        .size(11.5)
                        .color(FAINT),
                );
                return;
            }
            let mut stretch = node.slice.is_some();
            if ui.checkbox(&mut stretch, RichText::new("Тянется (9-slice)").size(12.5).color(FG)).changed() {
                // A quarter of the shorter side; groups have no size of their own.
                let side = node.width.abs().min(node.height.abs());
                let edge = if side >= 1.0 { (side / 4.0).round().clamp(4.0, 32.0) } else { 12.0 };
                styles.push(("slice", stretch.then(|| slice_text([edge; 4]))));
            }
            if let Some(s) = node.slice {
                let mut out = s;
                let mut changed = false;
                for row in [[(0, "Слева"), (2, "Справа")], [(1, "Сверху"), (3, "Снизу")]] {
                    ui.horizontal(|ui| {
                        for (k, label) in row {
                            if let Some(v) = theme::num_field(ui, half, label, s[k], 0.5, 0.0..=10_000.0, 0, KeyState::Hidden).changed {
                                out[k] = v.round();
                                changed = true;
                            }
                        }
                    });
                }
                if changed {
                    styles.push(("slice", Some(slice_text(out))));
                }
            }
            ui.label(
                RichText::new(
                    "Поля 9-slice не растягиваются: углы остаются как нарисованы, середина тянется. Состояния берутся из анимаций Hover, Press, Disabled, Focus — их последний кадр.",
                )
                .size(11.5)
                .color(FAINT),
            );
        });
    }
}

#[cfg(test)]
mod tests {
    use super::slice_text;

    #[test]
    fn slice_is_written_short() {
        assert_eq!(slice_text([8.0; 4]), "slice 8");
        assert_eq!(slice_text([8.0, 4.0, 8.0, 4.0]), "slice 8 4");
        assert_eq!(slice_text([1.0, 2.0, 3.0, 4.0]), "slice 1 2 3 4");
    }
}
