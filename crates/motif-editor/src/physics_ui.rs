//! The «Физика» section: a shape becomes a soft body, or a solid that soft bodies bump into.

use crate::app::{App, section};
use crate::style_ui::StyleEdit;
use crate::theme::{self, FAINT, FG, KeyState};
use egui::{RichText, vec2};
use motif_core::{Node, Soft};
use motif_script::fmt_num;

/// Script keys of a soft body, each with its label, value, range and drag speed.
const SOFT_KEYS: [&str; 6] = ["soft", "pressure", "friction", "damp", "drive", "gravity"];

impl App {
    pub(crate) fn physics_section(&mut self, ui: &mut egui::Ui, node: &Node) {
        if node.kind.is_container() {
            return;
        }
        let mut edits: Vec<StyleEdit> = Vec::new();
        section(ui, |ui| {
            theme::caption(ui, "Физика");
            ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
            let mut on = node.soft.is_some();
            if ui.checkbox(&mut on, RichText::new("Мягкое тело").size(12.5).color(FG)).changed() {
                if on {
                    edits.push(("soft", Some(format!("soft {}", fmt_num(Soft::default().stiff)))));
                } else {
                    edits.extend(SOFT_KEYS.map(|k| (k, None)));
                }
            }
            if let Some(s) = node.soft {
                let d = Soft::default();
                let half = (ui.available_width() - 6.0) / 2.0;
                let mut field = |ui: &mut egui::Ui, label: &str, key: &'static str, v: f64, def: f64, range: std::ops::RangeInclusive<f64>, speed: f64| {
                    let out = theme::num_field(ui, half, label, v, speed, range, 2, KeyState::Hidden);
                    if let Some(nv) = out.changed {
                        let nv = (nv * 100.0).round() / 100.0;
                        // `soft` stays written: it is what makes the shape a soft body.
                        let text = (key == "soft" || (nv - def).abs() > 1e-9).then(|| format!("{key} {}", fmt_num(nv)));
                        edits.push((key, text));
                    }
                };
                ui.horizontal(|ui| {
                    field(ui, "Упругость", "soft", s.stiff, d.stiff, 0.0..=1.0, 0.005);
                    field(ui, "Давление", "pressure", s.pressure, d.pressure, 0.0..=4.0, 0.01);
                });
                ui.horizontal(|ui| {
                    field(ui, "Трение", "friction", s.friction, d.friction, 0.0..=1.0, 0.005);
                    field(ui, "Затухание", "damp", s.damp, d.damp, 0.0..=1.0, 0.005);
                });
                ui.horizontal(|ui| {
                    field(ui, "За ключами", "drive", s.drive, d.drive, 0.0..=1.0, 0.005);
                    field(ui, "Гравитация", "gravity", s.gravity, d.gravity, -5000.0..=5000.0, 5.0);
                });
                ui.label(
                    RichText::new(
                        "Форму считает физика, пока играет анимация. Ключи положения задают, куда тело тянут («За ключами»: 0 — только гравитация, 1 — точно по ключам). Упругость возвращает нарисованную форму, давление держит объём.",
                    )
                    .size(11.5)
                    .color(FAINT),
                );
            }
            let mut solid = node.solid;
            if ui.checkbox(&mut solid, RichText::new("Препятствие").size(12.5).color(FG)).changed() {
                edits.push(("solid", solid.then(|| "solid".to_string())));
            }
            ui.label(RichText::new("Мягкие тела не проходят сквозь препятствия: стены, пол, тоннели.").size(11.5).color(FAINT));
        });
        if !edits.is_empty() {
            self.apply_styles(&node.id, edits);
        }
    }
}
