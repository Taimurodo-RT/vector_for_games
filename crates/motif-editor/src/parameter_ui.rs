//! Pose controls are independent of wardrobe proportions: Animate records one control
//! track, while editing a response changes the reusable rig across every clip.
use crate::app::{App, section};
use crate::theme;
use motif_core::{Prop, Purpose, Value};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum CharacterWork {
    #[default]
    Appearance,
    Setup,
    Animate,
}

impl App {
    pub(crate) fn parameter_work_panel(&mut self, ui: &mut egui::Ui) {
        if self.scene.doc.parameters.is_empty() {
            return;
        }
        if self.anim.is_some() || self.run.is_some() {
            self.character_work = CharacterWork::Animate;
        }
        section(ui, |ui| {
            ui.strong("Работа с персонажем");
            ui.horizontal_wrapped(|ui| {
                for (mode, name) in
                    [(CharacterWork::Appearance, "Внешность"), (CharacterWork::Setup, "Настройка движений"), (CharacterWork::Animate, "Анимация")]
                {
                    if ui.selectable_label(self.character_work == mode, name).clicked() {
                        self.set_mode_animate(mode == CharacterWork::Animate);
                        self.character_work = mode;
                        self.run = None;
                        self.playing = false;
                    }
                }
            });
            ui.small(match self.character_work {
                CharacterWork::Appearance => "Нейтральная поза. Рисунок и одежда; связи и ключи защищены.",
                CharacterWork::Setup => "Проба позы и опорные значения. Рисунок и ключи защищены.",
                CharacterWork::Animate => "Ключи и проигрывание. Рисунок, одежда и опорные значения защищены.",
            });
        });
    }

    /// All visual commands pass through this guard, including canvas shortcuts. Legacy
    /// scenes without pose controls retain their existing Design/Animate workflow.
    pub(crate) fn parameter_edit_allowed(&mut self, kind: &str, src: &str) -> bool {
        if self.scene.doc.parameters.is_empty() {
            return true;
        }
        let mode = if self.anim.is_some() || self.run.is_some() { CharacterWork::Animate } else { self.character_work };
        let allowed = match mode {
            CharacterWork::Setup => kind.starts_with("form-"),
            mode => motif_script::parse_in(src, self.path.as_ref().and_then(|p| p.parent())).is_ok_and(|new| {
                let old = &self.scene.doc;
                if mode == CharacterWork::Appearance {
                    new.doc.animations == old.animations
                        && new.doc.machines == old.machines
                        && new.doc.parameters == old.parameters
                        && new.doc.drivers == old.drivers
                } else {
                    let mut a = old.clone();
                    let mut b = new.doc;
                    a.animations.clear();
                    b.animations.clear();
                    a.machines.clear();
                    b.machines.clear();
                    a == b
                }
            }),
        };
        if !allowed {
            self.status = "Правка защищена текущим режимом. Выберите «Внешность», «Настройка движений» или «Анимация» в панели персонажа".into();
        }
        allowed
    }

    pub(crate) fn parameter_preview(&mut self, id: &str, value: f64) {
        if !self.can_edit() {
            return;
        }
        self.set_mode_animate(false);
        self.character_work = CharacterWork::Setup;
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
        if self.scene.doc.parameters.is_empty() || self.scene.doc.purpose == Purpose::Ui || self.character_work == CharacterWork::Appearance {
            return;
        }
        let shown = self.display_doc();
        section(ui, |ui| {
            ui.strong("Параметры движения");
            ui.small(if self.anim.is_some() {
                "Animate: изменение записывает ключ. Ромб добавляет или удаляет ключ."
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
                    if self.character_work == CharacterWork::Setup {
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
