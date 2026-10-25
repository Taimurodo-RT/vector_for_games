//! The «Мир» section of the artboard: the scene's gravity and wind, keyable like any property.

use crate::app::{App, section};
use crate::theme::{self, FAINT};
use egui::{RichText, vec2};
use motif_core::{Prop, Value};

impl App {
    pub(crate) fn world_section(&mut self, ui: &mut egui::Ui) {
        let world = self.display_doc().world;
        let mut change = None;
        let mut diamond = None;
        section(ui, |ui| {
            theme::caption(ui, "Мир");
            ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
            let half = (ui.available_width() - 6.0) / 2.0;
            let mut field = |ui: &mut egui::Ui, label: &str, prop: Prop, range: std::ops::RangeInclusive<f64>, speed: f64, decimals: usize| {
                let Value::Num(v) = world.get(prop) else { return };
                let out = theme::num_field(ui, half, label, v, speed, range, decimals, self.key_state("world", prop));
                if let Some(nv) = out.changed {
                    let step = 10f64.powi(decimals as i32);
                    change = Some((prop, (nv * step).round() / step));
                }
                if out.diamond {
                    diamond = Some(prop);
                }
            };
            ui.horizontal(|ui| {
                field(ui, "Гравитация X", Prop::GravityX, -10_000.0..=10_000.0, 5.0, 0);
                field(ui, "Гравитация Y", Prop::GravityY, -10_000.0..=10_000.0, 5.0, 0);
            });
            ui.horizontal(|ui| {
                field(ui, "Ветер X", Prop::WindX, -10_000.0..=10_000.0, 5.0, 0);
                field(ui, "Ветер Y", Prop::WindY, -10_000.0..=10_000.0, 5.0, 0);
            });
            ui.horizontal(|ui| field(ui, "Порывы", Prop::Gust, 0.0..=1.0, 0.005, 2));
            ui.label(
                RichText::new(
                    "Куда всё падает и куда дует ветер: частицы, мягкие тела, верёвки, ткань, вода и то, что катится по шару. Гравитация 0 900 — вниз, как обычно. Порывы 0 — ровный ветер, 1 — шквалы. Каждый объект берёт свою долю: «Вес» и «Парусность» в его разделе «Физика».",
                )
                .size(11.5)
                .color(FAINT),
            );
        });
        if let Some((prop, v)) = change {
            self.edit("world", prop, Value::Num(v));
        }
        if let Some(prop) = diamond {
            self.toggle_key("world", prop);
        }
    }
}
