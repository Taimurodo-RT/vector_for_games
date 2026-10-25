//! Pulls and fans in the editor: where they reach, drawn over the canvas, and their part of
//! the «Физика» section.

use crate::app::App;
use crate::canvas::View;
use crate::style_ui::StyleEdit;
use crate::theme::{self, FAINT, FG, SEL};
use egui::{Color32, Rect, RichText, Stroke};
use motif_core::kurbo::{Point, Vec2};
use motif_core::{Document, Field, Node, Prop, Value};
use motif_script::fmt_num;

/// Fields on the canvas.
const FIELD: Color32 = Color32::from_rgb(0xb4, 0x8c, 0xff);

impl App {
    /// The reach of every pull (a dashed circle with arrows in or out) and every fan (its
    /// stream), stronger for the selected objects. Only in the editor.
    pub(crate) fn paint_fields(&self, painter: &egui::Painter, rect: Rect, view: View, doc: &Document) {
        let selected = self.selection_ids();
        for (i, n) in doc.nodes.iter().enumerate() {
            if (n.pull.is_none() && n.fan.is_none()) || !doc.is_visible(i) {
                continue;
            }
            let on = selected.contains(&n.id.as_str());
            let color = if on { SEL } else { FIELD.gamma_multiply(0.55) };
            let stroke = Stroke::new(if on { 1.5 } else { 1.0 }, color);
            let w = doc.world_transform(i);
            let at = w * Point::ZERO;
            let screen = |q: Point| view.to_screen(rect, q);
            if let Some(f) = n.pull {
                let r = f.range.max(1.0);
                let pts: Vec<egui::Pos2> = (0..=72).map(|k| screen(at + Vec2::from_angle(k as f64 / 72.0 * std::f64::consts::TAU) * r)).collect();
                painter.extend(egui::Shape::dashed_line(&pts, stroke, 5.0, 4.0));
                // Arrows half way out: inwards for a pull, outwards for a push, none while off.
                for k in (0..8).filter(|_| f.strength != 0.0) {
                    let d = Vec2::from_angle(k as f64 / 8.0 * std::f64::consts::TAU);
                    let (a, b) = (at + d * r * 0.62, at + d * r * 0.42);
                    let (a, b) = if f.strength >= 0.0 { (a, b) } else { (b, a) };
                    arrow(painter, screen(a), screen(b), stroke);
                }
            }
            if let Some(f) = n.fan {
                let x = w * Point::new(1.0, 0.0) - at;
                let dir = if x.hypot() > 1e-9 { x.normalize() } else { Vec2::new(1.0, 0.0) };
                let half = (f.cone.clamp(1.0, 360.0) / 2.0).to_radians();
                let base = dir.atan2();
                let r = f.range.max(1.0);
                let mut edge: Vec<egui::Pos2> = vec![screen(at)];
                edge.extend((0..=24).map(|k| screen(at + Vec2::from_angle(base - half + 2.0 * half * k as f64 / 24.0) * r)));
                edge.push(screen(at));
                painter.extend(egui::Shape::dashed_line(&edge, stroke, 5.0, 4.0));
                for k in [-0.5, 0.0, 0.5].into_iter().filter(|_| f.strength != 0.0) {
                    let d = Vec2::from_angle(base + half * k);
                    let (a, b) = (at + d * r * 0.35, at + d * r * 0.6);
                    let (a, b) = if f.strength >= 0.0 { (a, b) } else { (b, a) };
                    arrow(painter, screen(a), screen(b), stroke);
                }
            }
        }
    }
}

fn arrow(painter: &egui::Painter, from: egui::Pos2, to: egui::Pos2, stroke: Stroke) {
    painter.line_segment([from, to], stroke);
    let d = to - from;
    if d.length() < 1.0 {
        return;
    }
    let d = d.normalized() * 6.0;
    let side = egui::vec2(-d.y, d.x) * 0.6;
    painter.line_segment([to, to - d + side], stroke);
    painter.line_segment([to, to - d - side], stroke);
}

/// What the «Притяжение» and «Вентилятор» parts of the «Физика» section changed: script keys,
/// and strengths, which can be keyed.
pub(crate) struct FieldEdits {
    pub(crate) styles: Vec<StyleEdit>,
    pub(crate) values: Vec<(Prop, f64)>,
    pub(crate) diamonds: Vec<Prop>,
}

/// One of the two fields in the panel.
struct Part {
    label: &'static str,
    word: &'static str,
    prop: Prop,
    value: Option<Field>,
    def: Field,
    hint: &'static str,
}

/// The «Притяжение» and «Вентилятор» checkboxes and their fields. `key` gives the key diamond of
/// a strength.
pub(crate) fn field_editor(ui: &mut egui::Ui, node: &Node, key: impl Fn(Prop) -> theme::KeyState) -> FieldEdits {
    let mut out = FieldEdits { styles: Vec::new(), values: Vec::new(), diamonds: Vec::new() };
    let half = (ui.available_width() - 6.0) / 2.0;
    let parts = [
        Part {
            label: "Притяжение",
            word: "pull",
            prop: Prop::Pull,
            value: node.pull,
            def: Field::pull(),
            hint: "Тянет к себе всё, что считает физика: частицы, мягкие тела, верёвки, ткань, воду. Сила меньше 0 — отталкивает. Слабеет к краю радиуса.",
        },
        Part {
            label: "Вентилятор",
            word: "fan",
            prop: Prop::Fan,
            value: node.fan,
            def: Field::fan(),
            hint: "Дует в сторону, куда повёрнут объект (поворот 0 — вправо), струёй шириной в конус. Слабеет к краю радиуса; парусность объектов усиливает его.",
        },
    ];
    for Part { label, word, prop, value, def, hint } in parts {
        let mut on = value.is_some();
        if ui.checkbox(&mut on, RichText::new(label).size(12.5).color(FG)).changed() {
            out.styles.push((word, on.then(|| format!("{word} {}", fmt_num(def.strength)))));
            let other = if word == "pull" { node.fan.is_some() } else { node.pull.is_some() };
            if !on && !other {
                out.styles.push(("range", None));
            }
            if !on && word == "fan" {
                out.styles.push(("cone", None));
            }
        }
        let Some(f) = value else { continue };
        ui.horizontal(|ui| {
            let o = theme::num_field(ui, half, "Сила", f.strength, 10.0, -100_000.0..=100_000.0, 0, key(prop));
            if let Some(v) = o.changed {
                out.values.push((prop, v.round()));
            }
            if o.diamond {
                out.diamonds.push(prop);
            }
            if let Some(v) = theme::num_field(ui, half, "Радиус", f.range, 1.0, 1.0..=10_000.0, 0, theme::KeyState::Hidden).changed {
                let v = v.round().max(1.0);
                out.styles.push(("range", (v != def.range).then(|| format!("range {}", fmt_num(v)))));
            }
        });
        if word == "fan" {
            ui.horizontal(|ui| {
                if let Some(v) = theme::num_field(ui, half, "Конус °", f.cone, 0.5, 1.0..=360.0, 0, theme::KeyState::Hidden).changed {
                    let v = v.round().clamp(1.0, 360.0);
                    out.styles.push(("cone", (v != def.cone).then(|| format!("cone {}", fmt_num(v)))));
                }
            });
        }
        ui.label(RichText::new(hint).size(11.5).color(FAINT));
    }
    out
}

/// Applies what `field_editor` changed to `node`.
pub(crate) fn apply_field_edits(app: &mut App, node: &str, e: FieldEdits) {
    for (prop, v) in e.values {
        app.edit(node, prop, Value::Num(v));
    }
    for prop in e.diamonds {
        app.toggle_key(node, prop);
    }
    if !e.styles.is_empty() {
        app.apply_styles(node, e.styles);
    }
}
