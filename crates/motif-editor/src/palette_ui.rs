//! The palette: colors named at the top of the script (`let main = #3b6cff`) as swatches. A
//! click on a swatch under a fill or stroke writes its name (`fill main`), so recoloring the
//! swatch recolors everything that uses it. The eyedropper picks a color off the canvas.

use crate::app::{App, fold};
use crate::theme::{self, ACCENT, BG, FAINT, FG, LINE, MUTED};
use egui::{Color32, RichText, Sense, Stroke, StrokeKind, vec2};
use motif_core::kurbo::{Point, Rect};
use motif_core::{Color, Document, Prop, Value};
use motif_render::CpuOptions;

/// Where a color picked off the canvas goes.
#[derive(Clone, Debug, PartialEq)]
pub enum Eyedrop {
    /// The fill or stroke of a node.
    Paint { node: String, prop: Prop },
    /// A palette color.
    Swatch(String),
}

fn c32(c: Color) -> Color32 {
    Color32::from_rgba_unmultiplied(c.r, c.g, c.b, c.a)
}

/// One square of color with a frame; `on` rings it.
fn swatch(ui: &mut egui::Ui, size: f32, c: Color, on: bool) -> egui::Response {
    let (r, resp) = ui.allocate_exact_size(vec2(size, size), Sense::click());
    ui.painter().rect_filled(r, 4.0, c32(c));
    let ring = if on {
        Stroke::new(2.0, ACCENT)
    } else if resp.hovered() {
        Stroke::new(1.0, FG)
    } else {
        Stroke::new(1.0, LINE)
    };
    ui.painter().rect_stroke(r, 4.0, ring, StrokeKind::Outside);
    resp
}

/// The color of the scene at `p` as drawn: shapes, gradients, shadows and the artboard behind.
pub fn sample(doc: &Document, p: Point) -> Option<Color> {
    let px = motif_render::render_region(
        doc,
        Rect::new(p.x.floor(), p.y.floor(), p.x.floor() + 1.0, p.y.floor() + 1.0),
        CpuOptions { scale: 1.0, antialias: true, background: true },
    )?;
    let c = px.pixels().first()?.demultiply();
    Some(Color { r: c.red(), g: c.green(), b: c.blue(), a: 255 })
}

impl App {
    /// The palette section: every named color, editable, and a button for a new one.
    pub(crate) fn palette_section(&mut self, ui: &mut egui::Ui) {
        let palette = self.scene.palette.clone();
        let mut set: Option<(String, Color)> = None;
        let mut add = false;
        let mut pick: Option<Eyedrop> = None;
        fold(ui, "Палитра", |ui| {
            ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
            if palette.is_empty() {
                ui.label(
                    RichText::new(
                        "Именованных цветов нет. Добавьте цвет — он встанет строкой let в начало скрипта, и его можно назначать заливкам и обводкам по имени.",
                    )
                    .size(11.5)
                    .color(FAINT),
                );
            }
            for s in &palette {
                ui.horizontal(|ui| {
                    let resp = swatch(ui, 22.0, s.color, self.eyedrop == Some(Eyedrop::Swatch(s.name.clone())));
                    ui.label(RichText::new(&s.name).size(12.5).color(FG));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let hex = if s.literal { s.color.to_hex() } else { "имя другого цвета".into() };
                        ui.label(RichText::new(hex).font(theme::mono(11.0)).color(MUTED));
                    });
                    if !s.literal {
                        resp.on_hover_text("Этот цвет задан именем другого: меняйте тот");
                        return;
                    }
                    theme::color_popup(&resp).show(|ui| {
                        let mut c = c32(s.color);
                        if egui::color_picker::color_picker_color32(ui, &mut c, egui::color_picker::Alpha::OnlyBlend) {
                            set = Some((s.name.clone(), Color { r: c.r(), g: c.g(), b: c.b(), a: c.a() }));
                        }
                        if theme::button(ui, "Пипетка: взять с холста", theme::ButtonKind::Normal).clicked() {
                            pick = Some(Eyedrop::Swatch(s.name.clone()));
                        }
                    });
                });
            }
            if theme::button(ui, "+ Цвет в палитру", theme::ButtonKind::Normal)
                .on_hover_text("Новая строка let в начале скрипта; цвет — заливка выделенного объекта")
                .clicked()
            {
                add = true;
            }
            ui.label(
                RichText::new("Цвет из палитры назначается щелчком по квадратику под заливкой или обводкой объекта. Поменяйте цвет здесь — перекрасится всё, что его использует.")
                    .size(11.5)
                    .color(FAINT),
            );
        });
        if let Some((name, c)) = set
            && self.can_edit()
        {
            let out = motif_script::set_swatch(&self.src, &self.scene, &name, c);
            self.apply(&format!("swatch-{name}"), out);
        }
        if add && self.can_edit() {
            let name = motif_script::free_swatch_name(&self.src);
            let color = self.selected.as_ref().and_then(|s| self.scene.doc.node(s)).and_then(|n| n.fill).unwrap_or(Color::rgb(128, 128, 128));
            let out = motif_script::add_swatch(&self.src, &self.scene, &name, color);
            if self.apply("swatch-add", Some(out)) {
                self.status = format!("В палитре новый цвет «{name}»: имя можно поменять в строке let, а затем там, где он используется");
            }
        }
        if let Some(target) = pick {
            self.start_eyedrop(target);
        }
    }

    /// The palette under a fill or stroke: a click writes the color's name into the property.
    pub(crate) fn palette_strip(&mut self, ui: &mut egui::Ui, id: &str, prop: Prop) {
        let current = self.scene.refs.get(&(id.to_string(), prop)).cloned();
        let mut chosen: Option<(String, Color)> = None;
        let mut pick = false;
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(4.0, 4.0);
            for s in &self.scene.palette {
                let resp = swatch(ui, 16.0, s.color, current.as_deref() == Some(s.name.as_str()));
                if resp.on_hover_text(format!("{} {}", s.name, s.color.to_hex())).clicked() {
                    chosen = Some((s.name.clone(), s.color));
                }
            }
            let on = self.eyedrop == Some(Eyedrop::Paint { node: id.to_string(), prop });
            let (r, resp) = ui.allocate_exact_size(vec2(16.0, 16.0), Sense::click());
            ui.painter().rect(r, 4.0, if on { ACCENT } else { BG }, Stroke::new(1.0, LINE), StrokeKind::Inside);
            // A dropper: a slanted stem and its drop.
            let ink = if on {
                theme::ACCENT_INK
            } else if resp.hovered() {
                FG
            } else {
                MUTED
            };
            ui.painter().line_segment([r.left_bottom() + vec2(4.0, -4.0), r.right_top() + vec2(-5.0, 5.0)], Stroke::new(1.6, ink));
            ui.painter().circle_filled(r.right_top() + vec2(-4.5, 4.5), 2.2, ink);
            if resp.on_hover_text("Пипетка: щёлкните по холсту, чтобы взять цвет (Esc — отмена)").clicked() {
                pick = true;
            }
            if let Some(name) = &current {
                ui.label(RichText::new(name).size(11.0).color(MUTED));
            }
        });
        if pick {
            self.start_eyedrop(Eyedrop::Paint { node: id.to_string(), prop });
        }
        if let Some((name, color)) = chosen {
            self.assign_color(id, prop, Some(&name), color);
        }
    }

    pub(crate) fn start_eyedrop(&mut self, target: Eyedrop) {
        self.status = match &target {
            Eyedrop::Paint { .. } => "Пипетка: щёлкните по холсту — цвет пойдёт в выделенный объект. Esc — отмена".into(),
            Eyedrop::Swatch(name) => format!("Пипетка: щёлкните по холсту — цвет станет «{name}». Esc — отмена"),
        };
        self.eyedrop = Some(target);
    }

    /// Writes `color` into a fill or stroke: by palette name in Design (`fill main`); in Animate
    /// a key holds the color itself.
    fn assign_color(&mut self, id: &str, prop: Prop, name: Option<&str>, color: Color) {
        match name {
            Some(name) if self.key_time().is_none() => {
                if self.can_edit() {
                    let out = motif_script::set_ref(&self.src, &self.scene, id, prop, name);
                    self.apply(&format!("{id}.{}", prop.path_name()), out);
                }
            }
            _ => self.edit(id, prop, Value::Color(color)),
        }
    }

    /// The canvas while the eyedropper is on: a click takes the color under the pointer.
    pub(crate) fn eyedrop_input(&mut self, ui: &egui::Ui, resp: &egui::Response, doc: &Document, at: Option<Point>) {
        if resp.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Crosshair);
        }
        if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.eyedrop = None;
            self.status = "Пипетка выключена".into();
            return;
        }
        if !resp.clicked() {
            return;
        }
        let Some(target) = self.eyedrop.take() else { return };
        let Some(color) = at.and_then(|p| sample(doc, p)) else { return };
        match target {
            Eyedrop::Paint { node, prop } => {
                // A color of the palette is written by its name.
                let name = self.scene.palette.iter().find(|s| s.color == color).map(|s| s.name.clone());
                self.assign_color(&node, prop, name.as_deref(), color);
                self.status = format!("Пипетка: {}{}", color.to_hex(), name.map_or(String::new(), |n| format!(" — это «{n}» из палитры")));
            }
            Eyedrop::Swatch(name) => {
                if self.can_edit() {
                    let out = motif_script::set_swatch(&self.src, &self.scene, &name, color);
                    self.apply(&format!("swatch-{name}"), out);
                    self.status = format!("«{name}» теперь {}", color.to_hex());
                }
            }
        }
    }
}
