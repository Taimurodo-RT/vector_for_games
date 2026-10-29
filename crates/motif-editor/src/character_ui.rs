//! A wardrobe over ordinary editable drawings and the existing skeleton/timeline.
use crate::app::{App, fold};
use motif_core::Purpose;
use motif_script::character;
use std::collections::BTreeMap;

impl App {
    pub(crate) fn character_choice(&mut self, root: &str, choices: &BTreeMap<String, String>) {
        if !self.can_edit() {
            return;
        }
        let base = self.path.as_ref().and_then(|p| p.parent());
        match character::apply(&self.src, &self.scene, root, choices, base) {
            Ok(out) => {
                self.apply_once("character-look", Some(out));
                self.status = "Облик изменён. Скелет и движения сохранены".into();
            }
            Err(e) => self.status = format!("Облик не изменён: {e}"),
        }
    }

    pub(crate) fn character_copy(&mut self, root: &str, slot: &str) {
        if !self.can_edit() {
            return;
        }
        let base = self.path.as_ref().and_then(|p| p.parent());
        match character::copy_variant(&self.src, &self.scene, root, slot, base) {
            Ok((out, ids)) => {
                self.apply_once("character-copy", Some(out));
                self.set_mode_animate(false);
                self.run = None;
                self.time = 0.0;
                self.select_list(ids);
                self.status = "Создан свой вариант. Раскройте выбранные группы в слоях и меняйте их фигуры; исходный вариант сохранён".into();
            }
            Err(e) => self.status = format!("Копия не создана: {e}"),
        }
    }

    pub(crate) fn character_panel(&mut self, ui: &mut egui::Ui) {
        if self.scene.doc.purpose == Purpose::Ui {
            return;
        }
        let catalog = match character::read(&self.src) {
            Ok(Some(c)) => c,
            Ok(None) => return,
            Err(e) => {
                fold(ui, "Персонаж", |ui| {
                    ui.label(e);
                });
                return;
            }
        };
        if let Err(e) = catalog.validate(&self.scene) {
            fold(ui, "Персонаж", |ui| {
                ui.label(e);
                ui.small("Обычные инструменты остаются доступны. Восстановите деталь через Undo или исправьте описание персонажа.");
            });
            return;
        }
        let picked = self.selected.as_ref().and_then(|id| self.scene.doc.index_of(id));
        let character = catalog
            .characters
            .iter()
            .find(|c| self.scene.doc.index_of(&c.root).is_some_and(|root| picked.is_some_and(|i| i == root || self.scene.doc.is_inside(i, root))))
            .or_else(|| catalog.characters.first());
        let Some(character) = character else { return };
        let mut choices = None;
        let mut save_look = false;
        let mut copy = None;
        let mut select = None;
        let mut motion = None;
        fold(ui, "Персонаж", |ui| {
            if catalog.characters.len() > 1 {
                egui::ComboBox::from_id_salt("character-root").selected_text(&character.name).show_ui(ui, |ui| {
                    for c in &catalog.characters {
                        if ui.selectable_label(c.root == character.root, &c.name).clicked() {
                            select = Some(vec![c.root.clone()]);
                        }
                    }
                });
            } else {
                ui.label(&character.name);
            }
            let current = character
                .looks
                .iter()
                .find(|look| character.slots.iter().all(|s| s.selected(&self.scene).is_some_and(|v| look.slots.get(&s.id) == Some(&v.id))));
            ui.label("Комплект");
            egui::ComboBox::from_id_salt(("look", &character.root))
                .width(ui.available_width())
                .selected_text(current.map_or("Своя комбинация", |l| &l.name))
                .show_ui(ui, |ui| {
                    for look in &character.looks {
                        if ui.selectable_label(current.is_some_and(|c| c.name == look.name), &look.name).clicked() {
                            choices = Some(look.slots.clone());
                        }
                    }
                });
            save_look = ui
                .add_enabled(current.is_none(), egui::Button::new("Сохранить как комплект"))
                .on_hover_text("Запомнить выбранные варианты всех слотов для быстрого возврата")
                .clicked();
            ui.label("Движение");
            ui.horizontal_wrapped(|ui| {
                if ui.selectable_label(self.anim.is_none(), "Настройка").clicked() {
                    motion = Some(None);
                }
                for m in &character.motions {
                    if self.scene.doc.animation(&m.clip).is_some() && ui.selectable_label(self.anim.as_deref() == Some(&m.clip), &m.name).clicked() {
                        motion = Some(Some(m.clip.clone()));
                    }
                }
            });
            if self.anim.is_some() && ui.button(if self.playing { "Пауза" } else { "Проиграть" }).clicked() {
                if !self.playing && self.current_anim().is_some_and(|a| self.time >= a.duration) {
                    self.time = 0.0;
                }
                self.playing = !self.playing;
            }
            ui.small("Меняйте одежду прямо во время движения. «Своя копия» возвращает в позу настройки для рисования.");
            ui.collapsing("Части и одежда", |ui| {
                for slot in &character.slots {
                    let selected = slot.selected(&self.scene);
                    ui.label(&slot.name);
                    egui::ComboBox::from_id_salt((&character.root, &slot.id))
                        .width(ui.available_width())
                        .selected_text(selected.map_or("Изменено в слоях", |v| &v.name))
                        .show_ui(ui, |ui| {
                            for variant in &slot.variants {
                                if ui.selectable_label(selected.is_some_and(|s| s.id == variant.id), &variant.name).clicked() {
                                    choices = Some(BTreeMap::from([(slot.id.clone(), variant.id.clone())]));
                                }
                            }
                        });
                    ui.horizontal(|ui| {
                        let ids = selected.map(|v| v.groups.clone()).unwrap_or_default();
                        if ui.add_enabled(!ids.is_empty(), egui::Button::new("Выбрать")).on_hover_text("Выбрать группы детали в слоях").clicked()
                        {
                            select = Some(ids.clone());
                        }
                        if ui
                            .add_enabled(!ids.is_empty(), egui::Button::new("Своя копия"))
                            .on_hover_text("Скопировать деталь для правки, сохранив исходную и движения")
                            .clicked()
                        {
                            copy = Some(slot.id.clone());
                        }
                    });
                }
            });
        });
        if let Some(choices) = choices {
            self.character_choice(&character.root, &choices);
        }
        if save_look && self.can_edit() {
            match character::save_look(&self.src, &self.scene, &character.root) {
                Ok((out, name)) => {
                    self.apply_once("character-save-look", Some(out));
                    self.status = format!("Сохранён комплект «{name}»");
                }
                Err(e) => self.status = e,
            }
        }
        if let Some(slot) = copy {
            self.character_copy(&character.root, &slot);
        }
        if let Some(ids) = select {
            self.select_list(ids);
        }
        if let Some(motion) = motion {
            match motion {
                Some(clip) => {
                    self.select_anim(clip);
                    self.playing = true;
                }
                None => {
                    self.set_mode_animate(false);
                    self.run = None;
                    self.time = 0.0;
                }
            }
        }
    }
}
