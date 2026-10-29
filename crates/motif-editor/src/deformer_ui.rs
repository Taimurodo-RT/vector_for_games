use super::*;
use crate::workspace::Workspace;

#[derive(Default)]
pub(super) struct DeformerEdit {
    pub target: Option<String>,
    pub grab: Option<usize>,
    pub form: Option<(String, f64)>,
}

impl App {
    pub(super) fn deformer_panel(&mut self, ui: &mut egui::Ui) {
        let Some(id) = self.selected.clone() else {
            return;
        };
        let Some(node) = self.scene.doc.node(&id).cloned() else {
            return;
        };
        if node.kind != ShapeKind::Group {
            return;
        }
        fold(ui, "Деформер группы", |ui| {
            if let Some(lattice) = &node.lattice {
                let mut on = self.deformer.target.as_deref() == Some(&id);
                if ui.checkbox(&mut on, "Править сетку на холсте").changed() {
                    self.deformer.target = on.then_some(id.clone());
                    self.point_edit = None;
                }
                if self.workspace() == Workspace::Setup {
                    let (mut w, mut h) = (lattice.width, lattice.height);
                    let mut changed = false;
                    ui.horizontal(|ui| {
                        changed |= ui.add(egui::DragValue::new(&mut w).range(1.0..=10000.0).prefix("W ")).changed();
                        changed |= ui.add(egui::DragValue::new(&mut h).range(1.0..=10000.0).prefix("H ")).changed();
                    });
                    if changed {
                        self.apply(
                            "lattice-size",
                            motif_script::set_style(&self.src, &self.scene, &id, "deform", Some(&format!("deform {} {}", fmt_num(w), fmt_num(h)))),
                        );
                    }
                }
                let bindings: Vec<_> = self
                    .scene
                    .doc
                    .drivers
                    .iter()
                    .filter(|d| d.tracks.iter().any(|t| t.node == id && matches!(t.prop, Prop::WarpX(_) | Prop::WarpY(_))))
                    .map(|d| d.parameter.clone())
                    .collect();
                if bindings.is_empty() && ui.button("Создать параметр сетки").clicked() {
                    self.bind_lattice(&id);
                }
                if self.workspace() == Workspace::Setup {
                    for parameter in bindings {
                        let Some(p) = self.scene.doc.parameter(&parameter).cloned() else { continue };
                        ui.label(format!("Опорные формы: {}", p.name));
                        ui.horizontal(|ui| {
                            for value in [p.min, p.default, p.max] {
                                if ui.selectable_label(self.deformer.form.as_ref() == Some(&(parameter.clone(), value)), fmt_num(value)).clicked() {
                                    self.parameter_preview(&parameter, value);
                                    self.deformer.form = Some((parameter.clone(), value));
                                    self.deformer.target = Some(id.clone());
                                }
                            }
                        });
                    }
                }
                if let Some((parameter, value)) = &self.deformer.form {
                    ui.colored_label(ACCENT, format!("Правка опоры {parameter} = {}", fmt_num(*value)));
                    if ui.button("Вернуться к кадру клипа").clicked() {
                        self.deformer.form = None;
                        self.pose_preview.clear();
                    }
                } else {
                    ui.small("Тяните точки сетки. При выбранном клипе изменения записываются в текущий кадр; новые детали группы наследуют деформацию.");
                }
                if ui.button("Сбросить смещения сетки").clicked() {
                    self.last_checkpoint.0.clear();
                    self.edit_group = Some("lattice-reset".into());
                    for i in 0..9 {
                        for prop in [Prop::WarpX(i), Prop::WarpY(i)] {
                            self.write_lattice_value(&id, prop, 0.0);
                        }
                    }
                    self.edit_group = None;
                    self.last_checkpoint.0.clear();
                }
            } else if ui.button("Добавить сетку 3×3").clicked() {
                self.add_lattice(&id);
            }
        });
    }

    pub(super) fn add_lattice(&mut self, id: &str) {
        let doc = self.display_doc();
        let Some(i) = doc.index_of(id) else {
            return;
        };
        let world = doc.world_transform(i);
        if world.determinant().abs() < 1e-10 {
            return;
        }
        let local = crate::selection::world_bounds(&doc, i).map(|r| world.inverse().transform_rect_bbox(r));
        let (w, h) = local.map(|r| (r.x0.abs().max(r.x1.abs()) * 2.0 + 20.0, r.y0.abs().max(r.y1.abs()) * 2.0 + 20.0)).unwrap_or((200.0, 200.0));
        let out = motif_script::set_style(&self.src, &self.scene, id, "deform", Some(&format!("deform {} {}", fmt_num(w.max(1.0)), fmt_num(h.max(1.0)))));
        if self.apply_once("add-lattice", out) {
            self.deformer.target = Some(id.into());
        }
    }

    pub(super) fn bind_lattice(&mut self, id: &str) {
        let mut name = format!("pose_{id}");
        let mut suffix = 2;
        while self.scene.doc.node(&name).is_some() || self.scene.doc.parameter(&name).is_some() {
            name = format!("pose_{id}_{suffix}");
            suffix += 1;
        }
        let Some(node) = self.scene.doc.node(id) else {
            return;
        };
        let mut out = format!("{}\nparameter {name} \"Сетка {id}\" range -1 1 default 0 value 0\ndriver {name} {{\n", self.src);
        for i in 0..9 {
            for prop in [Prop::WarpX(i), Prop::WarpY(i)] {
                let value = fmt_num(node.get(prop).as_num().unwrap_or(0.0));
                out.push_str(&format!("  {id}.{} -1: {value} | 0: {value} | 1: {value}\n", prop.path_name()));
            }
        }
        out.push_str("}\n");
        if self.apply_once("bind-lattice", Some(out)) {
            self.set_workspace(Workspace::Setup);
            self.parameter_preview(&name, 1.0);
            self.deformer.form = Some((name, 1.0));
            self.deformer.target = Some(id.into());
        }
    }

    fn write_lattice_value(&mut self, id: &str, prop: Prop, value: f64) {
        if let Some((parameter, position)) = self.deformer.form.clone() {
            let out = motif_script::parameters::set_form_value(&self.src, &self.scene, &parameter, id, prop, position, value);
            self.apply("lattice-form", out);
        } else {
            self.edit(id, prop, Value::Num(value));
        }
    }

    pub(super) fn deformer_input(&mut self, ui: &egui::Ui, response: &egui::Response, rect: Rect, view: View, doc: &Document) -> bool {
        let Some(id) = self.deformer.target.clone() else {
            return false;
        };
        let Some(i) = doc.index_of(&id) else {
            self.deformer = Default::default();
            return false;
        };
        let Some(lattice) = &doc.nodes[i].lattice else {
            self.deformer = Default::default();
            return false;
        };
        let world = doc.world_transform(i);
        if world.determinant().abs() < 1e-10 {
            return false;
        }
        let screen = |p| view.to_screen(rect, world * doc.map_lattice_point(i, p));
        let points: [Pos2; 9] = std::array::from_fn(|n| screen(lattice.control(n)));
        let painter = ui.painter_at(rect);
        for y in 0..3 {
            for x in 0..2 {
                painter.line_segment([points[y * 3 + x], points[y * 3 + x + 1]], Stroke::new(1.0, ACCENT));
            }
        }
        for x in 0..3 {
            for y in 0..2 {
                painter.line_segment([points[y * 3 + x], points[(y + 1) * 3 + x]], Stroke::new(1.0, ACCENT));
            }
        }
        for (k, p) in points.iter().enumerate() {
            painter.circle_filled(*p, 5.0, if self.deformer.grab == Some(k) { Color32::WHITE } else { ACCENT });
        }
        let pointer = response.interact_pointer_pos().or(response.hover_pos());
        let hit = pointer.and_then(|p| points.iter().position(|c| c.distance(p) < 9.0));
        if hit.is_some() && response.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Move);
        }
        if response.drag_started_by(egui::PointerButton::Primary) {
            let press = ui.input(|i| i.pointer.press_origin()).or(pointer);
            if let Some(k) = press.and_then(|p| points.iter().position(|c| c.distance(p) < 9.0)) {
                self.deformer.grab = Some(k);
                self.playing = false;
                self.last_checkpoint.0.clear();
            }
        }
        if let Some(k) = self.deformer.grab {
            if response.dragged()
                && let Some(p) = pointer
            {
                let local = doc.unmap_lattice_point(i, world.inverse() * view.to_doc(rect, p));
                let neutral = lattice.control(k) - lattice.offsets[k];
                let delta = local - neutral;
                self.preview(&id, Prop::WarpX(k as u8), Value::Num(delta.x));
                self.preview(&id, Prop::WarpY(k as u8), Value::Num(delta.y));
            }
            if response.drag_stopped() {
                self.deformer.grab = None;
                let live = std::mem::take(&mut self.live);
                self.edit_group = Some("lattice-drag".into());
                for (id, prop, value) in live {
                    if let Value::Num(v) = value {
                        self.write_lattice_value(&id, prop, v);
                    }
                }
                self.edit_group = None;
                self.last_checkpoint.0.clear();
            }
            return true;
        }
        hit.is_some() && (response.clicked() || response.is_pointer_button_down_on())
    }
}
