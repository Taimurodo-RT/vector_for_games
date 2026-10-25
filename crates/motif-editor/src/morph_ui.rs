//! The «Морфинг» section: which shapes this one flows into, and how far.

use crate::app::{App, section};
use crate::style_ui::StyleEdit;
use crate::theme::{self, FAINT, FG, MUTED};
use egui::{RichText, vec2};
use motif_core::{Node, Prop, ShapeKind, Value};
use motif_script::fmt_num;

impl App {
    /// The amount is keyable and goes to `changes` and `diamonds` like the other fields of the
    /// panel; the list of targets is written to the script through `styles`.
    pub(crate) fn morph_section(
        &self,
        ui: &mut egui::Ui,
        node: &Node,
        changes: &mut Vec<(Prop, Value)>,
        diamonds: &mut Vec<Prop>,
        styles: &mut Vec<StyleEdit>,
    ) {
        if node.kind.is_container() || node.kind == ShapeKind::Bone {
            return;
        }
        let id = node.id.clone();
        let targets: Vec<String> = node.morph.as_ref().map(|m| m.targets.clone()).unwrap_or_default();
        let shapes: Vec<String> = self
            .scene
            .doc
            .nodes
            .iter()
            .filter(|n| n.id != id && !n.kind.is_container() && n.kind != ShapeKind::Bone && !targets.contains(&n.id))
            .map(|n| n.id.clone())
            .collect();
        // How far, as written in the script, so that changing the targets keeps it.
        let written = self.scene.sources.get(&(id.clone(), Prop::Morph)).map(|s| self.src[s.span.start..s.span.end].to_string());
        let line =
            |list: &[String]| (!list.is_empty()).then(|| format!("morph {}{}", list.join(" "), written.as_ref().map_or(String::new(), |w| format!(" {w}"))));
        section(ui, |ui| {
            theme::caption(ui, "Морфинг");
            ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
            if !targets.is_empty() {
                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new("Перетекает в").size(12.0).color(MUTED));
                    for (i, t) in targets.iter().enumerate() {
                        let chip = ui.button(RichText::new(format!("{}  {t}  ×", i + 1)).size(12.0).color(FG)).on_hover_text("Убрать цель");
                        if chip.clicked() {
                            let mut rest = targets.clone();
                            rest.remove(i);
                            styles.push(("morph", line(&rest)));
                        }
                    }
                });
            }
            if !shapes.is_empty() {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Добавить цель").size(12.0).color(MUTED));
                    egui::ComboBox::from_id_salt(("morph", &id)).width(ui.available_width() - 8.0).selected_text("выбрать фигуру").show_ui(ui, |ui| {
                        for s in &shapes {
                            if ui.selectable_label(false, s).clicked() {
                                let mut more = targets.clone();
                                more.push(s.clone());
                                styles.push(("morph", line(&more)));
                            }
                        }
                    });
                });
            }
            if let Some(m) = &node.morph {
                let w = ui.available_width();
                let last = m.targets.len().max(1) as f64;
                let out = theme::num_field(ui, w, "Сколько, 0–цели", m.amount, 0.01, 0.0..=last, 2, self.key_state(&id, Prop::Morph));
                if let Some(v) = out.changed {
                    changes.push((Prop::Morph, Value::Num((v * 100.0).round() / 100.0)));
                }
                if out.diamond {
                    diamonds.push(Prop::Morph);
                }
                let hint = format!(
                    "0 — фигура как нарисована, 1 — первая цель{}. Дробные значения — между ними. Точки контуров сопоставляются сами, углы остаются острыми, цвет и обводка смешиваются. Цели обычно прячут (hidden): важен их контур, а не место.",
                    if m.targets.len() > 1 { format!(", {} — последняя", fmt_num(last)) } else { String::new() }
                );
                ui.label(RichText::new(hint).size(11.5).color(FAINT));
            } else {
                ui.label(RichText::new("Фигура может перетекать в контуры других фигур: выберите цель.").size(11.5).color(FAINT));
            }
        });
    }
}
