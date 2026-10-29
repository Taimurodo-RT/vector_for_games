use super::*;
use crate::preview::Mode;
use crate::workspace::{Layout, Panel};

pub(super) struct PreviewCanvas {
    texture: Option<TextureHandle>,
    rendered: Option<(Document, u32, bool)>,
    transparent: bool,
    bounds: Rect,
}

impl Default for PreviewCanvas {
    fn default() -> Self {
        Self { texture: None, rendered: None, transparent: false, bounds: Rect::NOTHING }
    }
}

impl App {
    pub(super) fn preview_panels(&mut self, ui: &mut egui::Ui, layout: Layout, strips: bool) {
        let frame = egui::Frame::new().fill(PANEL);
        let mut preview_visible = false;
        if layout.right_tabs {
            if self.panels.priority == Panel::Preview {
                self.preview_tab = true;
            }
            if self.panels.priority == Panel::Inspector {
                self.preview_tab = false;
            }
            let salt = self.panel_seed(ui, Panel::Preview, layout.preview_width);
            let response = egui::Panel::right(salt).resizable(true).default_size(layout.preview_width).size_range(240.0..=560.0).frame(frame).show(ui, |ui| {
                ui.horizontal(|ui| {
                    if ui.selectable_value(&mut self.preview_tab, false, "Свойства").clicked() {
                        self.panels.priority = Panel::Inspector;
                    }
                    if ui.selectable_value(&mut self.preview_tab, true, "Предпросмотр").clicked() {
                        self.panels.priority = Panel::Preview;
                    }
                    if ui.small_button("−").clicked() {
                        self.panels.set(if self.preview_tab { Panel::Preview } else { Panel::Inspector }, false);
                    }
                });
                if self.preview_tab {
                    preview_visible = true;
                    self.preview_panel(ui);
                } else {
                    self.props_panel(ui);
                }
            });
            self.panel_resized(ui, Panel::Preview, salt, response.response.rect);
            if ui.ctx().read_response(ui.scope_id().with(salt).with("__resize")).is_some_and(|r| r.dragged() || r.drag_stopped()) {
                self.panels.layout_mut().inspector_width = self.panels.layout().preview_width;
            }
        } else {
            for panel in [Panel::Preview, Panel::Inspector] {
                if layout.shown(panel) {
                    let width = if panel == Panel::Preview { layout.preview_width } else { layout.inspector_width };
                    let salt = self.panel_seed(ui, panel, width);
                    let reserved = 320.0
                        + if layout.tools { 48.0 } else { 0.0 }
                        + if layout.layers {
                            layout.layers_width
                        } else if strips {
                            28.0
                        } else {
                            0.0
                        }
                        + if layout.script {
                            layout.script_width
                        } else if strips {
                            28.0
                        } else {
                            0.0
                        }
                        + if panel == Panel::Preview {
                            if layout.inspector {
                                layout.inspector_width
                            } else if strips {
                                28.0
                            } else {
                                0.0
                            }
                        } else {
                            0.0
                        };
                    let max = (ui.available_width() - reserved).clamp(240.0, 560.0);
                    let response = egui::Panel::right(salt).resizable(true).default_size(width).size_range(240.0..=max).frame(frame).show(ui, |ui| {
                        self.panel_header(ui, panel);
                        if panel == Panel::Preview {
                            preview_visible = true;
                            self.preview_panel(ui);
                        } else {
                            self.props_panel(ui);
                        }
                    });
                    self.panel_resized(ui, panel, salt, response.response.rect);
                } else if strips {
                    egui::Panel::right(("closed-right", panel as u8)).exact_size(28.0).frame(frame).show(ui, |ui| self.collapsed_panel(ui, panel));
                }
            }
        }
        if !preview_visible {
            self.preview.hidden();
            self.preview_canvas.bounds = Rect::NOTHING;
        }
    }

    fn preview_panel(&mut self, ui: &mut egui::Ui) {
        self.preview.sync(&self.scene.doc, self.errors.is_empty());
        ui.small(format!("Артборд: {}", self.scene.doc.artboard.name));
        if self.preview.stale {
            ui.colored_label(DANGER, "Ошибка в скрипте: последний корректный результат");
        }
        egui::ScrollArea::vertical().id_salt("preview-options").max_height((ui.available_height() * 0.45).max(110.0)).show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                let mut mode = self.preview.mode;
                for (m, name) in [(Mode::Rest, "Рисунок"), (Mode::Clip, "Клип"), (Mode::Ui, "UI"), (Mode::Machine, "Машина")] {
                    ui.selectable_value(&mut mode, m, name);
                }
                self.preview.set_mode(mode);
            });
            if self.preview.mode == Mode::Clip {
                ui.checkbox(&mut self.preview.follow, "Следовать таймлайну")
                    .on_hover_text("Клип и время читаются с основного таймлайна. Пробная поза в предпросмотр не переносится.");
                ui.add_enabled_ui(!self.preview.follow, |ui| {
                    let mut clip = self.preview.clip.clone();
                    egui::ComboBox::from_id_salt("preview-clip").selected_text(clip.as_deref().unwrap_or("Нет клипов")).show_ui(ui, |ui| {
                        for a in &self.scene.doc.animations {
                            ui.selectable_value(&mut clip, Some(a.name.clone()), &a.name);
                        }
                    });
                    self.preview.set_clip(clip);
                    if let Some(a) = self.scene.doc.animations.iter().find(|a| Some(&a.name) == self.preview.clip.as_ref()) {
                        ui.add(egui::Slider::new(&mut self.preview.time, 0.0..=a.duration).text("с"));
                    }
                });
                if self.preview.follow && self.anim.is_none() {
                    ui.small("На рабочем холсте клип не выбран");
                }
            }
            if self.preview.mode == Mode::Machine {
                let mut machine = self.preview.machine.clone();
                egui::ComboBox::from_id_salt("preview-machine").selected_text(machine.as_deref().unwrap_or("Нет машин")).show_ui(ui, |ui| {
                    for m in &self.scene.doc.machines {
                        ui.selectable_value(&mut machine, Some(m.name.clone()), &m.name);
                    }
                });
                if machine != self.preview.machine {
                    self.preview.machine = machine;
                    self.preview.reset_player();
                }
                if let Some(m) = self.scene.doc.machines.iter().find(|m| Some(&m.name) == self.preview.machine.as_ref())
                    && let Some(p) = &mut self.preview.player
                {
                    if let Some(state) = m.states.get(p.state) {
                        ui.small(format!("Состояние: {}", state.name));
                    }
                    for (index, input) in m.inputs.iter().enumerate() {
                        ui.horizontal(|ui| {
                            use motif_core::InputKind;
                            match input.kind {
                                InputKind::Trigger => {
                                    if ui.button(format!("{} · Сработать", input.name)).clicked() {
                                        p.fire(m, &input.name);
                                    }
                                }
                                InputKind::Bool => {
                                    let mut value = p.values[index] != 0.0;
                                    if ui.checkbox(&mut value, &input.name).changed() {
                                        p.set(m, &input.name, if value { 1.0 } else { 0.0 });
                                    }
                                }
                                InputKind::Number => {
                                    let mut value = p.values[index];
                                    ui.label(&input.name);
                                    if ui.add(egui::DragValue::new(&mut value).speed(0.05)).changed() {
                                        p.set(m, &input.name, value);
                                    }
                                }
                            }
                        });
                    }
                }
            }
            if matches!(self.preview.mode, Mode::Clip | Mode::Machine) {
                let can_play = !(self.preview.stale || self.preview.mode == Mode::Clip && self.preview.follow)
                    && if self.preview.mode == Mode::Clip { self.preview.clip.is_some() } else { self.preview.machine.is_some() };
                ui.horizontal(|ui| {
                    if ui.add_enabled(can_play, egui::Button::new(if self.preview.playing { "Ⅱ Пауза" } else { "▶ Играть" })).clicked() {
                        self.preview.playing = !self.preview.playing;
                    }
                    if ui.button("С начала").clicked() {
                        self.preview.reset_player();
                    }
                });
            }
            if matches!(self.preview.mode, Mode::Rest | Mode::Ui) && !self.scene.doc.parameters.is_empty() {
                ui.collapsing("Параметры пробной позы", |ui| {
                    for p in &self.scene.doc.parameters {
                        let value = self.preview.parameters.entry(p.id.clone()).or_insert(p.value);
                        if ui.add(egui::Slider::new(value, p.min..=p.max).text(&p.id)).changed() {
                            self.preview.invalidate_simulation();
                        }
                    }
                    if ui.button("Нейтральная поза").clicked() {
                        self.preview.parameters.clear();
                        self.preview.invalidate_simulation();
                    }
                });
            }
            if self.preview.mode == Mode::Ui {
                ui.small("Наведите, нажмите или потяните деталь в окне ниже");
                ui.collapsing("Состояния UI", |ui| {
                    for c in &mut self.preview.check.controls {
                        ui.push_id(&c.node, |ui| {
                            ui.label(&c.node);
                            ui.checkbox(&mut c.disabled, "Недоступен");
                            if let Some(v) = &mut c.checked {
                                ui.checkbox(v, "Включён");
                            }
                            if let Some(v) = &mut c.value {
                                ui.add(egui::Slider::new(v, 0.0..=1.0).text("Значение"));
                            }
                        });
                    }
                    if ui.button("Сбросить состояния UI").clicked() {
                        self.preview.reset_player();
                    }
                });
            }
            ui.collapsing("Размер и фон", |ui| {
                let mut custom = self.preview.size.is_some();
                if ui.checkbox(&mut custom, "Проверить другой размер").changed() {
                    self.preview.size = custom.then_some((self.scene.doc.artboard.width, self.scene.doc.artboard.height));
                    self.preview.reset_player();
                }
                if let Some((w, h)) = &mut self.preview.size {
                    let mut changed = false;
                    ui.horizontal(|ui| {
                        changed |= ui.add(egui::DragValue::new(w).range(16.0..=4096.0).prefix("W ")).changed();
                        changed |= ui.add(egui::DragValue::new(h).range(16.0..=4096.0).prefix("H ")).changed();
                    });
                    if changed {
                        self.preview.reset_player();
                    }
                }
                ui.checkbox(&mut self.preview_canvas.transparent, "Скрыть фон артборда");
            });
        });
        let linked = self.anim.as_deref().map(|name| (name, self.time));
        let moving = self.preview.advance(ui.input(|i| i.stable_dt as f64), linked);
        if moving {
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(33));
        }
        let Some(shown) = self.preview.pose() else {
            return;
        };
        let height = (ui.available_height() - 28.0).max(40.0);
        let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), height), Sense::click_and_drag());
        let zoom = ((rect.width() - 12.0) / shown.artboard.width as f32).min((rect.height() - 12.0) / shown.artboard.height as f32).max(0.001);
        let view = View { zoom, pan: (rect.size() - vec2(shown.artboard.width as f32, shown.artboard.height as f32) * zoom) / 2.0 };
        self.preview_canvas.paint(ui, rect, view, &shown);
        let capture = response.hovered() || response.dragged() || response.is_pointer_button_down_on();
        let pointer = response.interact_pointer_pos().or(response.hover_pos()).filter(|_| capture).map(|p| view.to_doc(rect, p));
        let (down, quick) = ui.input(|i| (i.pointer.primary_down(), i.pointer.primary_pressed() && i.pointer.primary_released()));
        let down = down && (response.is_pointer_button_down_on() || response.dragged());
        if quick && pointer.is_some() && !down {
            self.preview.pointer(&shown, pointer, true);
        }
        if self.preview.pointer(&shown, pointer, down) {
            ui.ctx().request_repaint();
        }
        ui.small(&self.preview.message);
    }
}

impl PreviewCanvas {
    pub(super) fn contains_pointer(&self, ctx: &egui::Context) -> bool {
        ctx.input(|i| i.pointer.hover_pos().is_some_and(|p| self.bounds.contains(p)))
    }
    fn paint(&mut self, ui: &egui::Ui, rect: Rect, view: View, doc: &Document) {
        self.bounds = rect;
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 0.0, crate::canvas::STAGE);
        let longest = doc.artboard.width.max(doc.artboard.height);
        let pixels = (longest * view.zoom as f64 * ui.ctx().pixels_per_point() as f64).round().clamp(1.0, 1024.0) as u32;
        let changed = self.rendered.as_ref().is_none_or(|(old, size, transparent)| old != doc || *size != pixels || *transparent != self.transparent);
        if changed {
            let scale = pixels as f64 / longest;
            if let Some(px) = render_cpu(doc, CpuOptions { scale, antialias: true, background: !self.transparent }) {
                let image = egui::ColorImage::from_rgba_premultiplied([px.width() as usize, px.height() as usize], px.data());
                if let Some(texture) = &mut self.texture {
                    texture.set(image, TextureOptions::LINEAR);
                } else {
                    self.texture = Some(ui.ctx().load_texture("independent-preview", image, TextureOptions::LINEAR));
                }
                self.rendered = Some((doc.clone(), pixels, self.transparent));
            }
        }
        if let Some(texture) = &self.texture {
            painter.image(texture.id(), artboard_rect(rect, view, doc), Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
        }
    }
}
