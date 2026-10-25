//! The «Физика» section: springs on keyed values, jiggling bones, soft bodies and the solids
//! soft bodies bump into.

use crate::app::{App, section};
use crate::style_ui::StyleEdit;
use crate::theme::{self, FAINT, FG, KeyState};
use egui::{RichText, vec2};
use motif_core::{Emitter, Node, ShapeKind, Soft, Spring, Squash};
use motif_script::fmt_num;

/// Script keys of a soft body, each with its label, value, range and drag speed.
const SOFT_KEYS: [&str; 6] = ["soft", "pressure", "friction", "damp", "drive", "gravity"];

impl App {
    pub(crate) fn physics_section(&mut self, ui: &mut egui::Ui, node: &Node) {
        let mut edits: Vec<StyleEdit> = Vec::new();
        section(ui, |ui| {
            theme::caption(ui, "Физика");
            ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
            spring_editor(
                ui,
                &mut edits,
                "spring",
                "Пружина",
                node.spring,
                "Ключи положения, размера и поворота догоняются с перелётом и успокаиваются. Частота — колебаний в секунду, затухание 1 — без перелёта.",
            );
            squash_editor(ui, &mut edits, node.squash);
            if node.kind == ShapeKind::Bone {
                spring_editor(
                    ui,
                    &mut edits,
                    "jiggle",
                    "Покачивание",
                    node.jiggle,
                    "Кость отстаёт от родительской и раскачивается после остановки: хвосты, уши, волосы, антенны.",
                );
            }
            if node.kind.is_container() {
                crate::rope_ui::rope_editor(ui, &mut edits, node, &self.scene.doc);
                return;
            }
            emitter_editor(ui, &mut edits, node);
            crate::rope_ui::rope_editor(ui, &mut edits, node, &self.scene.doc);
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

/// A checkbox that turns a spring property on, and its frequency and damping fields.
fn spring_editor(ui: &mut egui::Ui, edits: &mut Vec<StyleEdit>, key: &'static str, label: &str, value: Option<Spring>, hint: &str) {
    let d = Spring::default();
    let text = |s: Spring| {
        if s == d { key.to_string() } else { format!("{key} {} {}", fmt_num(s.freq), fmt_num(s.damp)) }
    };
    let mut on = value.is_some();
    if ui.checkbox(&mut on, RichText::new(label).size(12.5).color(FG)).changed() {
        edits.push((key, on.then(|| key.to_string())));
    }
    let Some(s) = value else { return };
    let half = (ui.available_width() - 6.0) / 2.0;
    ui.horizontal(|ui| {
        if let Some(v) = theme::num_field(ui, half, "Частота", s.freq, 0.02, 0.1..=Spring::MAX_FREQ, 2, KeyState::Hidden).changed {
            edits.push((key, Some(text(Spring { freq: (v * 100.0).round() / 100.0, ..s }))));
        }
        if let Some(v) = theme::num_field(ui, half, "Затухание", s.damp, 0.005, 0.0..=2.0, 2, KeyState::Hidden).changed {
            edits.push((key, Some(text(Spring { damp: (v * 100.0).round() / 100.0, ..s }))));
        }
    });
    ui.label(RichText::new(hint).size(11.5).color(FAINT));
}

/// The «Сжатие» checkbox and its strength, frequency and damping fields.
fn squash_editor(ui: &mut egui::Ui, edits: &mut Vec<StyleEdit>, value: Option<Squash>) {
    let d = Squash::default();
    let text = |s: Squash| {
        if s == d { "squash".to_string() } else { format!("squash {} {} {}", fmt_num(s.amount), fmt_num(s.spring.freq), fmt_num(s.spring.damp)) }
    };
    let mut on = value.is_some();
    if ui.checkbox(&mut on, RichText::new("Сжатие и растяжение").size(12.5).color(FG)).changed() {
        edits.push(("squash", on.then(|| "squash".to_string())));
    }
    let Some(s) = value else { return };
    let round = |v: f64| (v * 100.0).round() / 100.0;
    let third = (ui.available_width() - 12.0) / 3.0;
    ui.horizontal(|ui| {
        if let Some(v) = theme::num_field(ui, third, "Сила", s.amount, 0.01, 0.0..=4.0, 2, KeyState::Hidden).changed {
            edits.push(("squash", Some(text(Squash { amount: round(v), ..s }))));
        }
        if let Some(v) = theme::num_field(ui, third, "Част.", s.spring.freq, 0.02, 0.1..=Spring::MAX_FREQ, 2, KeyState::Hidden).changed {
            edits.push(("squash", Some(text(Squash { spring: Spring { freq: round(v), ..s.spring }, ..s }))));
        }
        if let Some(v) = theme::num_field(ui, third, "Зат.", s.spring.damp, 0.005, 0.0..=2.0, 2, KeyState::Hidden).changed {
            edits.push(("squash", Some(text(Squash { spring: Spring { damp: round(v), ..s.spring }, ..s }))));
        }
    });
    ui.label(
        RichText::new("Объект вытягивается по ходу движения и сплющивается при резкой остановке, площадь не меняется. Сжатие идёт от оси (pivot): поставьте её внизу, чтобы мяч сплющивался о пол.")
            .size(11.5)
            .color(FAINT),
    );
}

/// Script keys of an emitter, removed together when it is turned off.
const EMIT_KEYS: [&str; 11] = ["emit", "burst", "life", "speed", "angle", "spread", "gravity", "drag", "fade", "grow", "spin"];

/// The «Частицы» checkbox and the emitter's fields.
fn emitter_editor(ui: &mut egui::Ui, edits: &mut Vec<StyleEdit>, node: &Node) {
    let mut on = node.emit.is_some();
    if ui.checkbox(&mut on, RichText::new("Частицы").size(12.5).color(FG)).changed() {
        if on {
            edits.push(("emit", Some(format!("emit {}", fmt_num(Emitter::default().rate)))));
        } else {
            edits.extend(EMIT_KEYS.map(|k| (k, None)));
        }
    }
    let Some(e) = node.emit else { return };
    let d = Emitter::default();
    let half = (ui.available_width() - 6.0) / 2.0;
    let mut field = |ui: &mut egui::Ui, label: &str, key: &'static str, v: f64, def: f64, range: std::ops::RangeInclusive<f64>, speed: f64| {
        if let Some(nv) = theme::num_field(ui, half, label, v, speed, range, 2, KeyState::Hidden).changed {
            let nv = (nv * 100.0).round() / 100.0;
            // `emit` stays written: it is what makes the shape an emitter.
            let text = (key == "emit" || (nv - def).abs() > 1e-9).then(|| format!("{key} {}", fmt_num(nv)));
            edits.push((key, text));
        }
    };
    ui.horizontal(|ui| {
        field(ui, "В секунду", "emit", e.rate, d.rate, 0.0..=1000.0, 0.5);
        field(ui, "Залп", "burst", e.burst, d.burst, 0.0..=1000.0, 0.5);
    });
    ui.horizontal(|ui| {
        field(ui, "Жизнь, с", "life", e.life, d.life, 0.01..=60.0, 0.01);
        field(ui, "Скорость", "speed", e.speed, d.speed, -5000.0..=5000.0, 1.0);
    });
    ui.horizontal(|ui| {
        field(ui, "Угол °", "angle", e.angle, d.angle, -360.0..=360.0, 1.0);
        field(ui, "Конус °", "spread", e.spread, d.spread, 0.0..=360.0, 1.0);
    });
    ui.horizontal(|ui| {
        field(ui, "Гравитация", "gravity", e.gravity, d.gravity, -5000.0..=5000.0, 5.0);
        field(ui, "Сопротивл.", "drag", e.drag, d.drag, 0.0..=1.0, 0.005);
    });
    ui.horizontal(|ui| {
        field(ui, "Альфа к концу", "fade", e.fade, d.fade, 0.0..=1.0, 0.005);
        field(ui, "Размер к концу", "grow", e.grow, d.grow, 0.0..=10.0, 0.01);
    });
    ui.horizontal(|ui| field(ui, "Вращение °/с", "spin", e.spin, d.spin, -3600.0..=3600.0, 2.0));
    ui.label(
        RichText::new("Пока играет анимация, из фигуры вылетают её копии; сама она не рисуется. Угол −90 — вверх, конус 360 — во все стороны. Источник можно двигать ключами и вешать на кости.")
            .size(11.5)
            .color(FAINT),
    );
}
