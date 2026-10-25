//! Painting bone weights with a brush: the «Веса кистью» panel of a skinned shape, the brush on
//! the canvas and the heat map of how strongly the chosen bone pulls each part of the shape.

use crate::app::{App, Tool, fold};
use crate::canvas::View;
use crate::theme::{self, ButtonKind, FAINT, MUTED};
use egui::{Color32, Rect, RichText, Stroke, vec2};
use motif_core::kurbo::{Point, Shape};
use motif_core::{Document, Node, WeightStroke};

/// Heat-map dots across the shape's longer side.
const DOTS: f64 = 34.0;

/// The weight brush: which bone it paints, how wide and how strong, and the stroke in progress.
pub(crate) struct WeightBrush {
    pub bone: String,
    /// In the shape's own units.
    pub radius: f64,
    /// 0..1: how far towards the bone one pass goes.
    pub amount: f64,
    /// The shape being painted; `None` while the brush is off.
    pub on: Option<String>,
    /// Points of the stroke being drawn, in the shape's space as drawn.
    pub stroke: Vec<Point>,
    pub erase: bool,
}

impl Default for WeightBrush {
    fn default() -> Self {
        WeightBrush { bone: String::new(), radius: 14.0, amount: 0.6, on: None, stroke: Vec::new(), erase: false }
    }
}

impl WeightBrush {
    /// The stroke in progress as it will be written.
    pub(crate) fn live(&self) -> Option<(&str, WeightStroke)> {
        let node = self.on.as_deref().filter(|_| !self.stroke.is_empty())?;
        let amount = if self.erase { -self.amount } else { self.amount };
        Some((node, WeightStroke { bone: self.bone.clone(), radius: self.radius, amount, points: self.stroke.clone() }))
    }
}

/// Blue where the bone does not pull, through green and yellow to red where it pulls fully.
fn heat(w: f64) -> Color32 {
    let stops = [(0.0, [0x2b, 0x4c, 0xd8]), (0.35, [0x2f, 0xc4, 0xb0]), (0.65, [0xf2, 0xd2, 0x3a]), (1.0, [0xff, 0x4a, 0x3a])];
    let w = w.clamp(0.0, 1.0);
    let k = stops.windows(2).position(|s| w <= s[1].0).unwrap_or(2);
    let ((a, ca), (b, cb)) = (stops[k], stops[k + 1]);
    let t = ((w - a) / (b - a)) as f32;
    let c = |i: usize| (ca[i] as f32 + (cb[i] as f32 - ca[i] as f32) * t).round() as u8;
    Color32::from_rgba_unmultiplied(c(0), c(1), c(2), 210)
}

impl App {
    /// «Веса кистью»: the bone to paint, the brush, and clearing what was painted.
    pub(crate) fn weights_section(&mut self, ui: &mut egui::Ui, node: &Node) {
        let Some(skin) = node.skin.as_ref().filter(|s| !s.bones.is_empty()) else { return };
        if !skin.bones.contains(&self.brush_w.bone) {
            self.brush_w.bone = skin.bones[0].clone();
        }
        let doc = &self.scene.doc;
        let label = |id: &str| doc.node(id).map_or_else(|| id.to_string(), |n| n.display_name().to_string());
        let painting = self.brush_w.on.as_deref() == Some(node.id.as_str());
        let painted = |b: &str| skin.paint.iter().filter(|s| s.bone == b).count();
        let (mut toggle, mut clear, mut clear_all) = (false, false, false);
        let mut bone = self.brush_w.bone.clone();
        let (mut radius, mut amount) = (self.brush_w.radius, self.brush_w.amount);
        fold(ui, "Веса кистью", |ui| {
            ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
            ui.horizontal(|ui| {
                ui.label(RichText::new("Кость").size(12.0).color(MUTED));
                egui::ComboBox::from_id_salt(("weight-bone", &node.id)).width(ui.available_width() - 8.0).selected_text(label(&bone)).show_ui(ui, |ui| {
                    for b in &skin.bones {
                        let n = painted(b);
                        let text = if n > 0 { format!("{} · мазков: {n}", label(b)) } else { label(b) };
                        if ui.selectable_label(*b == bone, text).clicked() {
                            bone = b.clone();
                        }
                    }
                });
            });
            let w = ui.available_width();
            if let Some(v) = theme::num_field(ui, w, "Радиус кисти", radius, 0.2, 1.0..=400.0, 1, theme::KeyState::Hidden).changed {
                radius = (v * 10.0).round() / 10.0;
            }
            if let Some(v) = theme::num_field(ui, w, "Сила", amount, 0.01, 0.05..=1.0, 2, theme::KeyState::Hidden).changed {
                amount = (v * 100.0).round() / 100.0;
            }
            ui.horizontal_wrapped(|ui| {
                let (text, kind) = if painting { ("Готово", ButtonKind::On) } else { ("Рисовать веса", ButtonKind::Primary) };
                toggle = theme::button(ui, text, kind).clicked();
                if painted(&bone) > 0 {
                    clear = theme::button(ui, "Стереть веса кости", ButtonKind::Normal).clicked();
                }
                if skin.paint.len() > painted(&bone) {
                    clear_all = theme::button(ui, "Сбросить все", ButtonKind::Normal).clicked();
                }
            });
            ui.label(
                RichText::new(
                    "Тяните кистью по фигуре: под ней контур следует за костью сильнее. Ctrl — стирать (отдать соседним костям). Цвет: красный — кость тянет полностью, синий — не тянет. Рисовать можно в любой позе, мазки ложатся на фигуру в позе по скрипту.",
                )
                .size(11.5)
                .color(FAINT),
            );
        });
        self.brush_w.bone = bone;
        (self.brush_w.radius, self.brush_w.amount) = (radius, amount);
        if toggle {
            if painting {
                self.brush_w.on = None;
            } else {
                self.brush_w.on = Some(node.id.clone());
                self.tool = Tool::Select;
                self.point_edit = None;
                self.status = "Кисть весов: тяните по фигуре, Ctrl — стирать, Esc — готово".into();
            }
        }
        if (clear || clear_all) && self.can_edit() {
            let bone = self.brush_w.bone.clone();
            let out = motif_script::clear_weights(&self.src, &self.scene, &node.id, (!clear_all).then_some(bone.as_str()));
            self.apply("weights-clear", out);
        }
    }

    /// The brush on the canvas while weights are painted: strokes, the brush circle and the heat
    /// map. `false` when the brush is off (or its shape is gone), so the canvas works as usual.
    pub(crate) fn weight_paint_input(&mut self, ui: &egui::Ui, resp: &egui::Response, rect: Rect, view: View, doc: &Document) -> bool {
        let Some(node_id) = self.brush_w.on.clone() else { return false };
        let ok = self.selected.as_deref() == Some(node_id.as_str()) && self.tool == Tool::Select;
        let Some(i) = doc.index_of(&node_id).filter(|&i| ok && doc.nodes[i].skin.as_ref().is_some_and(|s| s.bones.contains(&self.brush_w.bone))) else {
            self.brush_w.on = None;
            self.brush_w.stroke.clear();
            return false;
        };
        let world = doc.world_transform(i);
        let scale = world.determinant().abs().sqrt().max(1e-6);
        // The point of the shape as drawn that the bones moved under the pointer, found on the
        // shape as it was before this stroke: what the stroke bends must not slide under it.
        let mut still = doc.clone();
        if !self.brush_w.stroke.is_empty()
            && let Some(skin) = still.nodes[i].skin.as_mut()
        {
            skin.paint.pop();
        }
        let near = self.brush_w.stroke.last().copied();
        let unbend = |target: Point| still.unskin_point(i, target, near).unwrap_or(target);
        let pointer = resp.hover_pos().or(resp.interact_pointer_pos()).filter(|_| resp.hovered() || resp.dragged());
        let local = pointer.map(|s| unbend(world.inverse() * view.to_doc(rect, s)));
        let erase = ui.input(|i| i.modifiers.command);
        let down = ui.input(|i| i.pointer.primary_down());
        if down
            && resp.is_pointer_button_down_on()
            && let Some(p) = local
        {
            if self.brush_w.stroke.is_empty() {
                self.brush_w.erase = erase;
                self.brush_w.stroke.push(p);
            } else if self.brush_w.stroke.last().is_some_and(|l| (p - *l).hypot() > self.brush_w.radius * 0.2) {
                self.brush_w.stroke.push(p);
            }
        }
        if !down && !self.brush_w.stroke.is_empty() {
            if let Some((node, stroke)) = self.brush_w.live().map(|(n, s)| (n.to_string(), s))
                && self.can_edit()
            {
                let out = motif_script::add_weight_stroke(&self.src, &self.scene, &node, &stroke);
                self.apply(&format!("weights{}", self.undo.len()), out);
            }
            self.brush_w.stroke.clear();
        }

        // The heat map of the chosen bone, over the shape as it is posed now.
        let painter = ui.painter_at(rect);
        let node = &doc.nodes[i];
        if let (Some(base), Some(skin)) = (node.base_path(), node.skin.as_ref())
            && let Some(k) = skin.bind.iter().position(|b| b.bone == self.brush_w.bone)
        {
            let bb = base.bounding_box();
            let step = (bb.width().max(bb.height()) / DOTS).max(0.5);
            let dot = ((step * scale) as f32 * view.zoom * 0.32).clamp(1.2, 6.0);
            let mut y = bb.min_y() + step / 2.0;
            while y < bb.max_y() {
                let mut x = bb.min_x() + step / 2.0;
                while x < bb.max_x() {
                    let p = Point::new(x, y);
                    if base.contains(p) {
                        let w = skin.weights(p)[k];
                        let at = doc.skin_point(i, p).unwrap_or(p);
                        painter.circle_filled(view.to_screen(rect, world * at), dot, heat(w));
                    }
                    x += step;
                }
                y += step;
            }
        }
        if let Some(s) = pointer
            && let Some(p) = local
        {
            let r = (self.brush_w.radius * scale) as f32 * view.zoom;
            let c = if erase { Color32::from_rgb(0x9a, 0xb8, 0xff) } else { Color32::WHITE };
            let at = still.skin_point(i, p).map_or(s, |q| view.to_screen(rect, world * q));
            painter.circle_stroke(at, r, Stroke::new(1.5, Color32::from_black_alpha(160)));
            painter.circle_stroke(at, r, Stroke::new(1.0, c));
            painter.circle_stroke(at, r * self.brush_w.amount as f32, Stroke::new(1.0, c.gamma_multiply(0.45)));
            ui.ctx().set_cursor_icon(egui::CursorIcon::Crosshair);
        }
        true
    }
}
