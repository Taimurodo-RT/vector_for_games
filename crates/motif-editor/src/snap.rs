//! Steps for moving, resizing and rotating with the mouse, or a free mode without them.

use crate::app::App;
use crate::theme::{self, FAINT, Icon, MUTED};
use egui::{RichText, vec2};
use motif_core::kurbo::Point;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Snap {
    /// No steps: values keep one decimal so the script stays readable.
    pub free: bool,
    /// Position step in artboard pixels (also the arrow-key nudge).
    pub moving: f64,
    pub size: f64,
    /// Rotation step in degrees; Shift always gives 15°.
    pub rotation: f64,
    /// Smart guides: moved objects stick to the edges and centres of others and of the artboard.
    pub guides: bool,
    /// Points being placed (pen, corners of a new shape, size handles) also stick to the anchors
    /// and centres of other shapes.
    pub nodes: bool,
    /// The guide lines of the board are shown, can be pulled out of the rulers and are stuck to.
    pub user_guides: bool,
    /// Grid lines over the artboard every `grid_size` units.
    pub grid: bool,
    pub grid_size: f64,
    /// Lines between the pixels of the export (at its scale), when zoomed in far enough to see them.
    pub pixels: bool,
    pub rulers: bool,
    /// While dragging: distances to the neighbours, and the size, shift or angle at the pointer.
    pub labels: bool,
}

impl Default for Snap {
    fn default() -> Self {
        Snap {
            free: false,
            moving: 1.0,
            size: 1.0,
            rotation: 1.0,
            guides: true,
            nodes: true,
            user_guides: true,
            grid: false,
            grid_size: 16.0,
            pixels: false,
            rulers: true,
            labels: true,
        }
    }
}

fn to_step(v: f64, step: f64, free: bool) -> f64 {
    if free || step <= 0.0 { (v * 10.0).round() / 10.0 } else { (v / step).round() * step }
}

impl Snap {
    pub fn pos(&self, v: f64) -> f64 {
        to_step(v, self.moving, self.free)
    }

    pub fn size(&self, v: f64) -> f64 {
        to_step(v, self.size, self.free)
    }

    pub fn rot(&self, v: f64) -> f64 {
        to_step(v, self.rotation, self.free)
    }

    /// Where a point placed at `p` goes: onto an anchor, an edge or a centre line of `targets`
    /// when the guides are on, and by the position step on the axes that stuck to nothing. `off`
    /// (Ctrl held) places it freely, to a tenth of a unit.
    pub fn point(&self, targets: &crate::guides::Targets, p: Point, zoom: f32, off: bool) -> crate::guides::PointSnap {
        let fine = |v: f64| (v * 10.0).round() / 10.0;
        if off {
            return crate::guides::PointSnap { at: Point::new(fine(p.x), fine(p.y)), ..Default::default() };
        }
        let mut s = if self.guides { targets.snap_point(p, zoom) } else { crate::guides::PointSnap { at: p, ..Default::default() } };
        if s.mark.is_some() && !self.nodes {
            // Anchors are switched off: the edges and centre lines still count.
            let mut lines = targets.clone();
            lines.clear_points();
            s = lines.snap_point(p, zoom);
        }
        // What stuck keeps its exact place (to a hundredth, as the script writes it).
        let exact = |v: f64| (v * 100.0).round() / 100.0;
        s.at = Point::new(if s.stuck[0] { exact(s.at.x) } else { self.pos(s.at.x) }, if s.stuck[1] { exact(s.at.y) } else { self.pos(s.at.y) });
        s
    }

    /// Short label for the rail tooltip and the popup title.
    pub fn label(&self) -> String {
        if self.free {
            "свободно".into()
        } else {
            format!("{} px · {} px · {}°", motif_script::fmt_num(self.moving), motif_script::fmt_num(self.size), motif_script::fmt_num(self.rotation))
        }
    }
}

impl App {
    /// The step button at the bottom of the tool rail, with its settings popup.
    pub(crate) fn snap_button(&mut self, ui: &mut egui::Ui) {
        let tip = format!("Шаг трансформации: {}", self.snap.label());
        let resp = theme::tool_button(ui, Icon::Step, self.snap.free, "", &tip);
        egui::Popup::from_toggle_button_response(&resp).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside).show(|ui| {
            ui.set_min_width(220.0);
            ui.label(RichText::new("Шаг трансформации").font(theme::semibold(13.0)).color(theme::FG));
            ui.add_space(4.0);
            ui.checkbox(&mut self.snap.free, "Свободно (без шага)");
            ui.add_enabled_ui(!self.snap.free, |ui| {
                let w = 210.0;
                ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
                for (label, value, max) in
                    [("Сдвиг, px", &mut self.snap.moving, 200.0), ("Размер, px", &mut self.snap.size, 200.0), ("Поворот, °", &mut self.snap.rotation, 90.0)]
                {
                    let out = theme::num_field(ui, w, label, *value, 0.1, 0.1..=max, 1, theme::KeyState::Hidden);
                    if let Some(v) = out.changed {
                        *value = v;
                    }
                }
                ui.horizontal_wrapped(|ui| {
                    for (name, s) in [("1 px", (1.0, 1.0, 1.0)), ("8 px · 15°", (8.0, 8.0, 15.0)), ("16 px · 45°", (16.0, 16.0, 45.0))] {
                        if theme::button(ui, name, theme::ButtonKind::Normal).clicked() {
                            (self.snap.moving, self.snap.size, self.snap.rotation) = s;
                        }
                    }
                });
            });
            ui.label(
                RichText::new("Шаг действует на перетаскивание, ручки размера и поворота, стрелки. Shift при повороте — всегда 15°, на ручке размера — пропорции, Alt — от середины.").size(11.5).color(MUTED),
            );
            ui.label(RichText::new("В свободном режиме значения пишутся с точностью 0.1.").size(11.5).color(FAINT));
            ui.separator();
            ui.label(RichText::new("Направляющие").font(theme::semibold(13.0)).color(theme::FG));
            ui.checkbox(&mut self.snap.guides, "Умные направляющие").on_hover_text("Края и центры прилипают к другим объектам и артборду. Ctrl — без них");
            ui.add_enabled_ui(self.snap.guides, |ui| {
                ui.checkbox(&mut self.snap.nodes, "Узлы и центры фигур")
                    .on_hover_text("Точка пера, угол новой фигуры и ручка размера встают точно на узел или центр другой фигуры");
            });
            ui.label(RichText::new("Ctrl — поставить точно под курсор, без привязки.").size(11.5).color(FAINT));
            ui.checkbox(&mut self.snap.rulers, "Линейки");
            ui.checkbox(&mut self.snap.labels, "Подписи расстояний")
                .on_hover_text("Пока объект тянут: расстояния до соседей и краёв холста, у курсора — сдвиг, размер или угол");
            ui.checkbox(&mut self.snap.user_guides, "Свои направляющие")
                .on_hover_text("Потяните из линейки на холст. Обратно на линейку — убрать. Точные числа и замок — в панели справа, когда ничего не выделено");
            ui.horizontal(|ui| {
                ui.checkbox(&mut self.snap.grid, "Сетка");
                let out = theme::num_field(ui, 110.0, "шаг", self.snap.grid_size, 0.2, 1.0..=512.0, 0, theme::KeyState::Hidden);
                if let Some(v) = out.changed {
                    self.snap.grid_size = v.round().max(1.0);
                }
            });
            if theme::button(ui, "Шаг сдвига и размера = сетка", theme::ButtonKind::Normal).on_hover_text("Объекты будут вставать на линии сетки").clicked()
            {
                (self.snap.free, self.snap.moving, self.snap.size) = (false, self.snap.grid_size, self.snap.grid_size);
                self.snap.grid = true;
            }
            ui.label(RichText::new("G — показать или скрыть сетку.").size(11.5).color(FAINT));
            ui.separator();
            let px = 1.0 / self.export.scale.max(0.01);
            ui.label(
                RichText::new(format!("Пиксели экспорта (масштаб ×{})", motif_script::fmt_num(self.export.scale))).font(theme::semibold(13.0)).color(theme::FG),
            );
            ui.checkbox(&mut self.snap.pixels, "Пиксельная сетка").on_hover_text("Границы пикселей картинки после экспорта; видна при сильном увеличении");
            if theme::button(ui, "Шаг = пиксель экспорта", theme::ButtonKind::Normal)
                .on_hover_text("Сдвиг и размер — целыми пикселями экспорта: края не размываются")
                .clicked()
            {
                (self.snap.free, self.snap.moving, self.snap.size) = (false, px, px);
                self.snap.pixels = true;
            }
        });
    }
}
