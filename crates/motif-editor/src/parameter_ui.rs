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
                Workspace::Drawing => "Исходник: рисунок и постоянные свойства".into(),
                Workspace::Setup => "Настройка: связи и опорные формы; поза — пробная".into(),
                Workspace::Animate => self.anim.as_ref().map_or_else(|| "Анимация: клип не выбран".into(), |name| format!("Клип {name} · {:.2} с", self.time)),
            };
            ui.strong(label);
            if self.workspace() == Workspace::Animate {
                ui.checkbox(&mut self.auto_key, "Автоключ")
                    .on_hover_text("Записывать изменения разрешённых свойств в клип. Выключено: только пробная поза, запись ромбом или кнопкой");
                if self.has_draft_pose() {
                    ui.small("Пробная поза — ещё не записана");
                    ui.horizontal(|ui| {
                        if ui.button("Записать позу").clicked() {
                            self.record_draft_pose();
                        }
                        if ui.button("Сбросить пробу").clicked() {
                            self.clear_draft_pose();
                        }
                    });
                }
                if self.anim.is_none() && ui.button("Создать клип").clicked() {
                    self.create_animation();
                }
            }
        });
    }

    pub(crate) fn protected_character_node(&self, id: &str) -> bool {
        protected_node(&self.scene.doc, &self.src, id)
    }

    /// Protect the target data, including commands reached from menus and shortcuts.
    /// UI nodes on a board containing a rig remain editable and keyable.
    pub(crate) fn parameter_edit_allowed(&mut self, kind: &str, src: &str) -> bool {
        let mode = if self.run.is_some() { Workspace::Animate } else { self.workspace() };
        if self.scene.doc.parameters.is_empty() && mode != Workspace::Animate {
            return true;
        }
        let allowed = motif_script::parse_in(src, self.path.as_ref().and_then(|p| p.parent())).is_ok_and(|new| {
            let old = &self.scene.doc;
            match mode {
                Workspace::Drawing => {
                    new.doc.animations == old.animations
                        && new.doc.machines == old.machines
                        && new.doc.parameters == old.parameters
                        && new.doc.drivers == old.drivers
                }
                Workspace::Setup => {
                    if new.doc.animations != old.animations || new.doc.parameters != old.parameters {
                        return false;
                    }
                    if !kind.starts_with("form-") && new.doc.drivers != old.drivers {
                        return false;
                    }
                    let unchanged = |a: &motif_core::Document, text: &str, b: &motif_core::Document| {
                        a.nodes.iter().filter(|node| protected_node(a, text, &node.id)).all(|node| b.node(&node.id) == Some(node))
                    };
                    unchanged(old, &self.src, &new.doc) && unchanged(&new.doc, src, old)
                }
                Workspace::Animate => {
                    let mut a = old.clone();
                    let mut b = new.doc;
                    a.animations.clear();
                    b.animations.clear();
                    a.machines.clear();
                    b.machines.clear();
                    a == b
                }
            }
        });
        if !allowed {
            self.status =
                "Эта правка меняет данные другого пространства. Для рисунка выберите «Рисование», для связей — «Настройка», для клипов — «Анимация»".into();
        }
        allowed
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
        if self.scene.doc.parameters.is_empty() || self.tool_profile() == Purpose::Ui || self.workspace() == Workspace::Drawing {
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
            } else {
                "Проба позы не меняет файл. Для записи движения выберите «Анимация»."
            });
            for parameter in &shown.parameters {
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
                            ui.small("Правки здесь меняют все клипы. Кнопка позиции показывает опору в Design.");
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
                                                let enabled = self.anim.is_none() && self.run.is_none();
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
// top-level owners of driven objects define the protected artwork, not the entire board.
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
    doc.drivers.iter().flat_map(|d| &d.tracks).any(|track| {
        let Some(mut root) = doc.index_of(&track.node) else { return false };
        while let Some(parent) = doc.nodes[root].parent {
            root = parent;
        }
        index == root || doc.is_inside(index, root)
    })
}
