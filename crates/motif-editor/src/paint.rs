//! Gradient fills and clip masks in the properties panel.

use crate::app::App;
use crate::theme::{self, MUTED};
use egui::{Color32, RichText, Sense, Stroke, StrokeKind, vec2};
use motif_core::{Color, Gradient, Node, ShapeKind};
use motif_script::{set_clip, set_gradient};

/// Fill type choices, in the order of the switch.
const KINDS: [&str; 3] = ["Цвет", "Линейный", "Радиальный"];

impl App {
    /// The fill type switch, and for a gradient its colours and angle.
    /// Returns true when the plain colour row should be shown.
    pub(crate) fn fill_kind_ui(&mut self, ui: &mut egui::Ui, node: &Node) -> bool {
        let current = match &node.gradient {
            None => 0,
            Some(g) if g.radial => 2,
            Some(_) => 1,
        };
        let mut next: Option<Option<Gradient>> = None;
        if let Some(k) = theme::segmented(ui, &KINDS, current, false)
            && k != current
        {
            next = Some(match (k, &node.gradient) {
                (0, _) => None,
                (_, Some(g)) => Some(Gradient { radial: k == 2, ..g.clone() }),
                (_, None) => {
                    let base = node.fill.unwrap_or(Color::rgb(0x4f, 0xd3, 0x8c));
                    Some(Gradient { radial: k == 2, angle: 90.0, colors: vec![lighten(base, 0.45), base] })
                }
            });
        }
        if let Some(g) = &node.gradient {
            ui.add_space(6.0);
            let mut g2 = g.clone();
            let mut changed = false;
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
                let mut remove = None;
                for (i, c) in g.colors.iter().enumerate() {
                    let (sw, resp) = ui.allocate_exact_size(vec2(28.0, 28.0), Sense::click());
                    ui.painter().rect_filled(sw, 6.0, Color32::from_rgb(c.r, c.g, c.b));
                    ui.painter().rect_stroke(sw, 6.0, Stroke::new(1.0, theme::LINE), StrokeKind::Inside);
                    let resp = resp.on_hover_text(format!("{} · клик — изменить", c.to_hex()));
                    let mut c32 = Color32::from_rgb(c.r, c.g, c.b);
                    theme::color_popup(&resp).show(|ui| {
                        if egui::color_picker::color_picker_color32(ui, &mut c32, egui::color_picker::Alpha::Opaque) {
                            g2.colors[i] = Color { r: c32.r(), g: c32.g(), b: c32.b(), a: c.a };
                            changed = true;
                        }
                        if g.colors.len() > 2 && ui.button("Убрать цвет").clicked() {
                            remove = Some(i);
                        }
                    });
                }
                if g.colors.len() < 8 && theme::button(ui, "+", theme::ButtonKind::Normal).on_hover_text("Добавить цвет в конец").clicked() {
                    let last = *g.colors.last().unwrap();
                    g2.colors.push(lighten(last, 0.5));
                    changed = true;
                }
                if let Some(i) = remove {
                    g2.colors.remove(i);
                    changed = true;
                }
            });
            if !g.radial {
                ui.add_space(6.0);
                let w = ui.available_width();
                let out = theme::num_field(ui, w, "Угол °", g.angle, 1.0, -360.0..=360.0, 0, theme::KeyState::Hidden);
                if let Some(a) = out.changed {
                    g2.angle = a.round();
                    changed = true;
                }
            }
            if changed {
                next = Some(Some(g2));
            }
            ui.label(RichText::new("Цвета идут по порядку вдоль градиента. Угол 0 — слева направо, 90 — сверху вниз.").size(11.5).color(theme::FAINT));
        }
        if let Some(g) = next
            && self.can_edit()
        {
            match set_gradient(&self.src, &self.scene, &node.id, g.as_ref()) {
                Some(out) => {
                    self.apply("gradient", Some(out));
                }
                None => self.status = "Градиент объекта из цикла меняется в тексте цикла".into(),
            }
        }
        node.gradient.is_none()
    }

    /// The mask switch of a group: its first child clips the others.
    pub(crate) fn mask_ui(&mut self, ui: &mut egui::Ui, node: &Node) {
        if node.kind != ShapeKind::Group {
            return;
        }
        let mut on = node.clip;
        crate::app::section(ui, |ui| {
            theme::caption(ui, "Маска");
            ui.checkbox(&mut on, RichText::new("Первый объект обрезает остальные").size(12.5).color(theme::FG));
            let first = self.scene.doc.index_of(&node.id).and_then(|gi| self.scene.doc.nodes.iter().find(|n| n.parent == Some(gi)));
            let hint = match (node.clip, first) {
                (true, Some(m)) => format!("Маска: «{}». Её форма видна только как граница, сама она не рисуется.", m.display_name()),
                (_, None) => "Добавьте в группу объекты: первый станет маской.".into(),
                (false, Some(_)) => "Первый объект группы (нижний в слоях) станет формой обрезки.".into(),
            };
            ui.label(RichText::new(hint).size(11.5).color(MUTED));
        });
        if on != node.clip && self.can_edit() {
            match set_clip(&self.src, &self.scene, &node.id, on) {
                Some(out) => {
                    self.apply("clip", Some(out));
                }
                None => self.status = "Маску группы из цикла включают в тексте цикла".into(),
            }
        }
    }
}

/// Mixes a colour towards white (`t` > 0) or black (`t` < 0).
fn lighten(c: Color, t: f32) -> Color {
    let (to, t) = if t >= 0.0 { (255.0, t) } else { (0.0, -t) };
    let m = |v: u8| (v as f32 + (to - v as f32) * t).round() as u8;
    Color { r: m(c.r), g: m(c.g), b: m(c.b), a: c.a }
}
