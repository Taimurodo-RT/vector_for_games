//! The «Орбита» section: putting an object on a ball by latitude and longitude, and turning a
//! ball so that everything on it goes round.

use crate::app::{App, section};
use crate::style_ui::StyleEdit;
use crate::theme::{self, FAINT, FG, KeyState, MUTED};
use egui::{RichText, vec2};
use motif_core::{Node, Prop, Roll, ShapeKind, Value};
use motif_script::fmt_num;

impl App {
    /// Keyable fields go to `changes` and `diamonds` like the other fields of the panel; the
    /// ball and `wrap` are written to the script through `styles`.
    pub(crate) fn orbit_section(
        &self,
        ui: &mut egui::Ui,
        node: &Node,
        half: f32,
        changes: &mut Vec<(Prop, Value)>,
        diamonds: &mut Vec<Prop>,
        styles: &mut Vec<StyleEdit>,
    ) {
        if node.kind == ShapeKind::Bone {
            return;
        }
        let doc = &self.scene.doc;
        let id = node.id.clone();
        let balls: Vec<String> =
            doc.nodes.iter().filter(|n| n.id != id && !matches!(n.kind, ShapeKind::Group | ShapeKind::Bone)).map(|n| n.id.clone()).collect();
        // Something sits on this shape: it is a ball, and can be turned.
        let is_ball = doc.nodes.iter().any(|n| n.on.as_deref() == Some(id.as_str()));
        section(ui, |ui| {
            theme::caption(ui, "Орбита (2.5D)");
            ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
            let mut field = |ui: &mut egui::Ui, label: &str, prop: Prop, range: std::ops::RangeInclusive<f64>| {
                let v = node.get(prop).as_num().unwrap_or(0.0);
                let out = theme::num_field(ui, half, label, v, 0.5, range, 1, self.key_state(&id, prop));
                if let Some(nv) = out.changed {
                    changes.push((prop, Value::Num((nv * 10.0).round() / 10.0)));
                }
                if out.diamond {
                    diamonds.push(prop);
                }
            };
            ui.horizontal(|ui| {
                ui.label(RichText::new("На шаре").size(12.0).color(MUTED));
                let shown = node.on.clone().unwrap_or_else(|| "нет".to_string());
                egui::ComboBox::from_id_salt(("orbit-ball", &id)).width(ui.available_width() - 8.0).selected_text(shown).show_ui(ui, |ui| {
                    if ui.selectable_label(node.on.is_none(), "нет").clicked() && node.on.is_some() {
                        styles.extend([("on", None), ("lat", None), ("lon", None), ("alt", None), ("wrap", None)]);
                    }
                    for b in &balls {
                        if ui.selectable_label(node.on.as_ref() == Some(b), b).clicked() {
                            styles.push(("on", Some(format!("on {b}"))));
                        }
                    }
                });
            });
            if node.on.is_some() {
                ui.horizontal(|ui| {
                    field(ui, "Широта °", Prop::Lat, -90.0..=90.0);
                    field(ui, "Долгота °", Prop::Lon, -100_000.0..=100_000.0);
                });
                ui.horizontal(|ui| field(ui, "Над шаром", Prop::Alt, -1000.0..=1000.0));
                let mut wrap = node.wrap;
                if ui.checkbox(&mut wrap, RichText::new("Лежит на поверхности").size(12.5).color(FG)).changed() {
                    styles.push(("wrap", wrap.then(|| "wrap".to_string())));
                }
                ui.label(
                    RichText::new(
                        "Объект ездит по шару как по глобусу: долгота — влево-вправо, широта — вверх-вниз. За шаром он прячется. «Лежит на поверхности» изгибает фигуру по шару: у края она сужается, как рисунок на глобусе.",
                    )
                    .size(11.5)
                    .color(FAINT),
                );
                roll_editor(ui, node, half, styles);
            }
            if is_ball {
                ui.add_space(2.0);
                ui.label(RichText::new("Поворот этого шара").size(12.0).color(MUTED));
                ui.horizontal(|ui| {
                    field(ui, "Влево-вправо", Prop::Yaw, -100_000.0..=100_000.0);
                    field(ui, "Наклон", Prop::Pitch, -100_000.0..=100_000.0);
                });
                ui.label(RichText::new("Поворачивает всё, что лежит на шаре: так голова поворачивается, а глаза и нос съезжают вбок.").size(11.5).color(FAINT));
            }
        });
    }
}

/// «Катится по шару»: the object rolls over its ball under gravity.
fn roll_editor(ui: &mut egui::Ui, node: &Node, half: f32, styles: &mut Vec<StyleEdit>) {
    let mut on = node.roll.is_some();
    if ui.checkbox(&mut on, RichText::new("Катится по шару").size(12.5).color(FG)).changed() {
        styles.push(("roll", on.then(|| "roll".to_string())));
        if !on && node.emit.is_none() && node.soft.is_none() && node.rope.is_none() && node.fluid.is_none() {
            styles.push(("gravity", None));
        }
    }
    let Some(r) = node.roll else { return };
    let d = Roll::default();
    let head = |friction: f64, damp: f64| {
        if damp != d.damp {
            format!("roll {} {}", fmt_num(friction), fmt_num(damp))
        } else if friction != d.friction {
            format!("roll {}", fmt_num(friction))
        } else {
            "roll".to_string()
        }
    };
    let round = |v: f64| (v * 100.0).round() / 100.0;
    ui.horizontal(|ui| {
        if let Some(v) = theme::num_field(ui, half, "Сцепление", r.friction, 0.005, 0.0..=1.0, 2, KeyState::Hidden).changed {
            styles.push(("roll", Some(head(round(v), r.damp))));
        }
        if let Some(v) = theme::num_field(ui, half, "Затухание", r.damp, 0.002, 0.0..=1.0, 2, KeyState::Hidden).changed {
            styles.push(("roll", Some(head(r.friction, round(v)))));
        }
    });
    ui.label(
        RichText::new("Объект сам катится по шару вниз и крутится на ходу; широта и долгота задают, откуда он стартует. Сцепление 1 — вращается вместе с шаром, 0 — шар скользит под ним. Объекты на одном шаре толкаются. Вес и парусность — в разделе «Физика».")
            .size(11.5)
            .color(FAINT),
    );
}
