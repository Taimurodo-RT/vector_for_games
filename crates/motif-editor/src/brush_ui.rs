//! The «Кисть» section: a hand-drawn look for a shape — wobbly outline, hatching, grain,
//! banded light.

use crate::app::{App, fold};
use crate::style_ui::StyleEdit;
use crate::theme::{self, FAINT, FG, KeyState};
use egui::{RichText, vec2};
use motif_core::{Brush, Grain, Hatch, Node, Prop, Value};
use motif_script::fmt_num;

fn nums(v: &[f64]) -> String {
    v.iter().map(|x| fmt_num(*x)).collect::<Vec<_>>().join(" ")
}

pub fn rough_text(b: &Brush) -> String {
    if b.wave > 0.0 { format!("rough {}", nums(&[b.rough, b.wave])) } else { format!("rough {}", fmt_num(b.rough)) }
}

pub fn hatch_text(h: Hatch) -> String {
    let mut s = format!("hatch {}", nums(&[h.angle, h.gap]));
    if h.width != 1.0 || h.color.is_some() {
        s += &format!(" {}", fmt_num(h.width));
    }
    if let Some(c) = h.color {
        s += &format!(" {}", c.to_hex());
    }
    if h.cross {
        s += " cross";
    }
    s
}

pub fn grain_text(g: Grain) -> String {
    let mut s = format!("grain {}", nums(&[g.density, g.size]));
    if let Some(c) = g.color {
        s += &format!(" {}", c.to_hex());
    }
    s
}

impl App {
    /// The wobble is keyable and goes to `changes` and `diamonds`; everything else is written
    /// through `styles`.
    pub(crate) fn brush_section(
        &self,
        ui: &mut egui::Ui,
        node: &Node,
        half: f32,
        changes: &mut Vec<(Prop, Value)>,
        diamonds: &mut Vec<Prop>,
        styles: &mut Vec<StyleEdit>,
    ) {
        if node.kind.is_container() || node.kind == motif_core::ShapeKind::Bone {
            return;
        }
        let id = node.id.clone();
        let b = &node.brush;
        fold(ui, "Кисть", |ui| {
            ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
            let plain = |ui: &mut egui::Ui, label: &str, v: f64, speed: f64, range: std::ops::RangeInclusive<f64>, digits: usize| {
                theme::num_field(ui, half, label, v, speed, range, digits, KeyState::Hidden).changed.map(|n| {
                    let k = 10f64.powi(digits as i32);
                    (n * k).round() / k
                })
            };
            ui.horizontal(|ui| {
                let out = theme::num_field(ui, half, "Неровность", b.rough, 0.05, 0.0..=200.0, 1, self.key_state(&id, Prop::Rough));
                if let Some(v) = out.changed {
                    changes.push((Prop::Rough, Value::Num((v * 10.0).round() / 10.0)));
                }
                if out.diamond {
                    diamonds.push(Prop::Rough);
                }
                let wave = if b.wave > 0.0 { b.wave } else { Brush::WAVE };
                if let Some(w) = plain(ui, "Волна", wave, 0.2, 2.0..=2000.0, 0)
                    && b.rough > 0.0
                {
                    styles.push(("rough", Some(rough_text(&Brush { wave: w, ..b.clone() }))));
                }
            });
            ui.horizontal(|ui| {
                if let Some(f) = plain(ui, "Дрожание/с", b.boil, 0.1, 0.0..=60.0, 0) {
                    styles.push(("boil", (f > 0.0).then(|| format!("boil {}", fmt_num(f)))));
                }
                let seed = b.seed.unwrap_or(0) as f64;
                if let Some(s) = plain(ui, "Вариант", seed, 0.1, 0.0..=99_999.0, 0) {
                    styles.push(("seed", Some(format!("seed {}", fmt_num(s)))));
                }
            });
            let mut hatch = b.hatch.is_some();
            if ui.checkbox(&mut hatch, RichText::new("Штриховка").size(12.5).color(FG)).changed() {
                styles.push(("hatch", hatch.then(|| "hatch 45 6".to_string())));
            }
            if let Some(h) = b.hatch {
                let mut out = h;
                let mut changed = false;
                ui.horizontal(|ui| {
                    if let Some(v) = plain(ui, "Угол", h.angle, 0.5, -360.0..=360.0, 0) {
                        (out.angle, changed) = (v, true);
                    }
                    if let Some(v) = plain(ui, "Шаг", h.gap, 0.05, 0.5..=500.0, 1) {
                        (out.gap, changed) = (v, true);
                    }
                });
                ui.horizontal(|ui| {
                    if let Some(v) = plain(ui, "Толщина", h.width, 0.02, 0.1..=100.0, 1) {
                        (out.width, changed) = (v, true);
                    }
                    if ui.checkbox(&mut out.cross, RichText::new("Крест-накрест").size(12.0).color(FG)).changed() {
                        changed = true;
                    }
                });
                if changed {
                    styles.push(("hatch", Some(hatch_text(out))));
                }
            }
            let mut grain = b.grain.is_some();
            if ui.checkbox(&mut grain, RichText::new("Зерно").size(12.5).color(FG)).changed() {
                styles.push(("grain", grain.then(|| "grain 0.15 2".to_string())));
            }
            if let Some(g) = b.grain {
                let mut out = g;
                let mut changed = false;
                ui.horizontal(|ui| {
                    if let Some(v) = plain(ui, "Плотность", g.density, 0.005, 0.01..=1.0, 2) {
                        (out.density, changed) = (v, true);
                    }
                    if let Some(v) = plain(ui, "Точка", g.size, 0.02, 0.2..=100.0, 1) {
                        (out.size, changed) = (v, true);
                    }
                });
                if changed {
                    styles.push(("grain", Some(grain_text(out))));
                }
            }
            if let Some(n) = plain(ui, "Ступени света", b.bands as f64, 0.05, 0.0..=64.0, 0) {
                styles.push(("bands", (n >= 1.0).then(|| format!("bands {}", fmt_num(n)))));
            }
            ui.label(
                RichText::new(
                    "Неровность — насколько контур гуляет от руки, волна — длина одного изгиба. Дрожание перерисовывает линию заново несколько раз в секунду, как в рисованной анимации. Вариант — другой рисунок той же кисти. Ступени света делают градиенты и свет на 3D-телах плоскими полосами.",
                )
                .size(11.5)
                .color(FAINT),
            );
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn brush_settings_are_written_short() {
        let h = Hatch { angle: 45.0, gap: 6.0, width: 1.0, color: None, cross: false };
        assert_eq!(hatch_text(h), "hatch 45 6");
        assert_eq!(hatch_text(Hatch { cross: true, color: Some(motif_core::Color::BLACK), ..h }), "hatch 45 6 1 #000000 cross");
        assert_eq!(grain_text(Grain { density: 0.2, size: 2.0, color: None }), "grain 0.2 2");
        assert_eq!(rough_text(&Brush { rough: 3.0, ..Default::default() }), "rough 3");
        assert_eq!(rough_text(&Brush { rough: 3.0, wave: 40.0, ..Default::default() }), "rough 3 40");
    }
}
