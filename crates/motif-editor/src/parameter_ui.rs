//! Pose controls are independent of wardrobe proportions: Animate records one control
//! track, while editing a response changes the reusable rig across every clip.
use crate::app::{App, section};
use crate::theme;
use motif_core::{Prop, Purpose, Value};

use crate::workspace::Workspace;

impl App {
    pub(crate) fn parameter_work_panel(&mut self, ui: &mut egui::Ui) {
        section(ui, |ui| {
            let label = match self.workspace() {
                Workspace::Drawing => self.anim.as_ref().map_or_else(|| "Рисунок без клипа".into(), |name| format!("Рисование · {name} · {:.2} с", self.time)),
                Workspace::Setup => {
                    self.anim.as_ref().map_or_else(|| "Настройка связей и опор".into(), |name| format!("Настройка · {name} · {:.2} с", self.time))
                }
                Workspace::Animate => self.anim.as_ref().map_or_else(|| "Анимация: клип не выбран".into(), |name| format!("Клип {name} · {:.2} с", self.time)),
            };
            ui.strong(label);
            if self.anim.is_some() || self.workspace() == Workspace::Animate {
                self.pose_actions(ui);
                if self.anim.is_none() && ui.button("Создать клип").clicked() {
                    self.create_animation();
                }
            }
        });
    }

    pub(crate) fn protected_character_node(&self, id: &str) -> bool {
        protected_node(&self.scene.doc, &self.src, id)
    }

    pub(crate) fn parameter_preview(&mut self, id: &str, value: f64) {
        if !self.can_edit() {
            return;
        }
        self.set_workspace(Workspace::Setup);
        self.run = None;
        self.playing = false;
        if let Some(p) = self.scene.doc.parameter(id) {
            self.pose_preview.insert(id.to_string(), value.clamp(p.min, p.max));
        }
    }

    pub(crate) fn parameter_form_edit(&mut self, parameter: &str, node: &str, prop: Prop, position: f64, value: f64) {
        if !self.can_edit() {
            return;
        }
        let out = motif_script::parameters::set_form_value(&self.src, &self.scene, parameter, node, prop, position, value);
        self.apply(&format!("form-{parameter}-{node}-{}-{position}", prop.path_name()), out);
    }

    pub(crate) fn parameter_panel(&mut self, ui: &mut egui::Ui) {
        self.parameter_category_panel(ui, None);
    }

    pub(crate) fn parameter_category_panel(&mut self, ui: &mut egui::Ui, only: Option<&str>) {
        if self.scene.doc.parameters.is_empty() || only.is_none() && self.tool_profile() == Purpose::Ui {
            return;
        }
        if self.selected.as_deref().is_some_and(|id| !self.protected_character_node(id)) {
            return;
        }
        let shown = self.display_doc();
        section(ui, |ui| {
            ui.strong("Параметры движения");
            ui.small(if self.anim.is_some() {
                if self.auto_key {
                    "Автоключ включён: изменение записывает ключ."
                } else {
                    "Проба позы. Ромб или «Записать позу» сохраняет ключи."
                }
            } else if self.workspace() == Workspace::Setup {
                "Проба позы не меняет файл. Для записи движения выберите «Анимация»."
            } else {
                "Без клипа изменяются исходные значения параметров."
            });
            for parameter in shown.parameters.iter().filter(|p| only.is_none_or(|id| id == p.id)) {
                ui.push_id(&parameter.id, |ui| {
                    ui.label(&parameter.name);
                    let field = theme::num_field(
                        ui,
                        ui.available_width(),
                        "Значение",
                        parameter.value,
                        0.01,
                        parameter.min..=parameter.max,
                        3,
                        self.key_state(&parameter.id, Prop::Parameter),
                    );
                    if let Some(value) = field.changed {
                        self.edit(&parameter.id, Prop::Parameter, Value::Num(value));
                    }
                    if field.diamond {
                        self.toggle_key(&parameter.id, Prop::Parameter);
                    }
                    let mut value = parameter.value;
                    if ui.add(egui::Slider::new(&mut value, parameter.min..=parameter.max).show_value(false)).changed() {
                        self.edit(&parameter.id, Prop::Parameter, Value::Num(value));
                    }
                    if ui.small_button("Нейтрально").clicked() {
                        self.edit(&parameter.id, Prop::Parameter, Value::Num(parameter.default));
                    }
                    if self.workspace() == Workspace::Setup {
                        ui.collapsing("Опорные значения", |ui| {
                            ui.small("Правки здесь меняют все клипы. Кнопка позиции временно показывает опорную форму.");
                            let Some(driver) = self.scene.doc.drivers.iter().find(|d| d.parameter == parameter.id).cloned() else { return };
                            for track in &driver.tracks {
                                ui.push_id((&track.node, track.prop), |ui| {
                                    let name = self.scene.doc.node(&track.node).map_or(track.node.as_str(), |n| n.display_name());
                                    ui.label(format!("{} · {}", name, crate::timeline::prop_label(track.prop)));
                                    for key in &track.keys {
                                        ui.push_id(key.time.to_bits(), |ui| {
                                            ui.horizontal(|ui| {
                                                if ui.small_button(motif_script::fmt_num(key.time)).on_hover_text("Показать эту опорную позу").clicked()
                                                {
                                                    self.parameter_preview(&parameter.id, key.time);
                                                }
                                                let mut value = key.value.as_num().unwrap_or(0.0);
                                                let enabled = self.run.is_none();
                                                if ui
                                                    .add_enabled(enabled, egui::DragValue::new(&mut value).speed(0.05).max_decimals(3))
                                                    .on_disabled_hover_text("Нажмите кнопку позиции слева, чтобы перейти в настройку опоры")
                                                    .changed()
                                                {
                                                    self.parameter_form_edit(&parameter.id, &track.node, track.prop, key.time, value);
                                                }
                                            });
                                        });
                                    }
                                });
                            }
                        });
                    }
                });
            }
        });
    }
}

// A UI element remains a UI element inside a character group. Metadata roots and the
// top-level owners of driven objects define the parameter panel's scope, not the entire board.
fn protected_node(doc: &motif_core::Document, src: &str, id: &str) -> bool {
    if doc.parameters.is_empty() {
        return false;
    }
    let Some(index) = doc.index_of(id) else { return false };
    let mut at = Some(index);
    while let Some(i) = at {
        if doc.nodes[i].ui {
            return false;
        }
        at = doc.nodes[i].parent;
    }
    if let Ok(Some(catalog)) = motif_script::character::read(src)
        && catalog.characters.iter().any(|c| doc.index_of(&c.root).is_some_and(|root| index == root || doc.is_inside(index, root)))
    {
        return true;
    }
    doc.drivers.iter().flat_map(|d| &d.tracks).map(|t| t.node.as_str()).chain(doc.correctives.iter().flat_map(|c| &c.tracks).map(|t| t.node.as_str())).any(
        |target| {
            let Some(mut root) = doc.index_of(target) else { return false };
            while let Some(parent) = doc.nodes[root].parent {
                root = parent;
            }
            index == root || doc.is_inside(index, root)
        },
    )
}
