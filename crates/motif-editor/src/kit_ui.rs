//! The «Интерфейс» section: marking a piece of game UI and its nine-slice margins.

use crate::app::{App, fold};
use crate::style_ui::StyleEdit;
use crate::theme::{self, FAINT, FG, KeyState};
use egui::{RichText, vec2};
use motif_core::{Node, ShapeKind, UiPart, UiRole};
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

/// What a piece becomes in Godot, as the menu names it.
fn role_name(r: UiRole) -> &'static str {
    match r {
        UiRole::Auto => "Кнопка или панель",
        UiRole::Button => "Кнопка — Button",
        UiRole::Panel => "Панель — PanelContainer",
        UiRole::Progress => "Полоса — ProgressBar",
        UiRole::Check => "Флажок — CheckBox",
        UiRole::Slider => "Слайдер — HSlider/VSlider",
        UiRole::Scroll => "Прокрутка — ScrollBar",
        UiRole::Label => "Надпись — Label",
    }
}

fn part_name(p: UiPart) -> &'static str {
    match p {
        UiPart::Background => "Фон",
        UiPart::Fill => "Заливка",
        UiPart::Unchecked => "Выключен",
        UiPart::Checked => "Включён",
        UiPart::Track => "Дорожка",
        UiPart::Grabber => "Ползунок",
        UiPart::Text => "Подпись — шрифтом Godot",
    }
}

/// What the parts of a kind of piece are for, under the menu.
fn role_hint(r: UiRole) -> &'static str {
    match r {
        UiRole::Auto => "С состояниями или подписью — кнопка, без них — панель.",
        UiRole::Button | UiRole::Panel => "Подпись (text с деталью «Подпись») не запекается: Godot пишет её шрифтом темы, цвет — по состояниям.",
        UiRole::Progress => "Внутри нужны детали «Фон» и «Заливка»; заливка растёт от начала полосы.",
        UiRole::Check => "Внутри нужны «Выключен» и «Включён» — картинки галочки; подпись — по желанию.",
        UiRole::Slider => "Внутри нужны «Дорожка» и «Ползунок»; «Заливка» — пройденная часть, по желанию.",
        UiRole::Scroll => "Внутри нужны «Дорожка» и «Ползунок»; ползунок тянется вдоль полосы.",
        UiRole::Label => "Шрифт, размер, цвет, обводка и тень текста — в тему Label.",
    }
}

impl App {
    /// Both flags are written through `styles`: `ui` alone makes a fixed-size piece,
    /// `slice` a stretchable one. Returns true when the states button was pressed.
    pub(crate) fn kit_section(&self, ui: &mut egui::Ui, node: &Node, half: f32, styles: &mut Vec<StyleEdit>) -> bool {
        if node.kind == ShapeKind::Bone {
            return false;
        }
        let mut add_states = false;
        let missing: Vec<&str> = ["Hover", "Press"].into_iter().filter(|n| self.scene.doc.animation(n).is_none()).collect();
        // The piece this node is inside, if any: then it can be one of its parts.
        let doc = &self.scene.doc;
        let mut up = node.parent;
        while let Some(k) = up.filter(|k| !doc.nodes[*k].ui) {
            up = doc.nodes[k].parent;
        }
        let owner = up.map(|k| &doc.nodes[k]);
        fold(ui, "Интерфейс", |ui| {
            ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
            if let Some(o) = owner.filter(|o| !o.ui_role.parts().is_empty() || node.part.is_some()) {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Деталь").size(12.5).color(FG));
                    egui::ComboBox::from_id_salt(("ui-part", &node.id))
                        .width(ui.available_width() - 8.0)
                        .selected_text(node.part.map_or("нет", part_name))
                        .show_ui(ui, |ui| {
                            if ui.selectable_label(node.part.is_none(), "нет").clicked() {
                                styles.push(("part", None));
                            }
                            for &p in o.ui_role.parts() {
                                if ui.selectable_label(node.part == Some(p), part_name(p)).clicked() {
                                    styles.push(("part", Some(format!("part {}", p.keyword()))));
                                }
                            }
                        });
                });
                if let Some(p) = node.part {
                    let whose = o.name.as_deref().unwrap_or(&o.id);
                    let hint = if p == UiPart::Text {
                        format!("Подпись «{whose}» не запекается: Godot пишет её шрифтом из темы, цвет — из каждого состояния.")
                    } else {
                        format!("Часть «{whose}»: снимается отдельной картинкой.")
                    };
                    ui.label(RichText::new(hint).size(11.5).color(FAINT));
                    return;
                }
            }
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
            ui.horizontal(|ui| {
                ui.label(RichText::new("В Godot").size(12.5).color(FG));
                egui::ComboBox::from_id_salt(("ui-role", &node.id)).width(ui.available_width() - 8.0).selected_text(role_name(node.ui_role)).show_ui(
                    ui,
                    |ui| {
                        for r in UiRole::ALL {
                            if ui.selectable_label(node.ui_role == r, role_name(r)).clicked() && r != node.ui_role {
                                let word = r.keyword();
                                styles.push(("ui", Some(if word.is_empty() { "ui".to_string() } else { format!("ui {word}") })));
                            }
                        }
                    },
                );
            });
            ui.label(RichText::new(role_hint(node.ui_role)).size(11.5).color(FAINT));
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
            if !missing.is_empty()
                && theme::button(ui, &format!("Добавить состояния {}", missing.join(" и ")), theme::ButtonKind::Normal)
                    .on_hover_text("Пустые анимации состояний: в Animate меняйте свойства, последний кадр станет картинкой состояния")
                    .clicked()
            {
                add_states = true;
            }
            ui.label(
                RichText::new(
                    "Поля 9-slice не растягиваются: углы остаются как нарисованы, середина тянется. Состояния берутся из анимаций Hover, Press, Disabled, Focus — их последний кадр.",
                )
                .size(11.5)
                .color(FAINT),
            );
        });
        add_states
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
