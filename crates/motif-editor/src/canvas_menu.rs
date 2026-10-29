//! Object-local inspector. It uses the same guarded authoring commands as the sidebar.
use super::*;
use crate::workspace::Workspace;

pub(super) struct FloatingInspector {
    target: String,
    anchor: Pos2,
}

impl App {
    pub(super) fn pointer_hits_selection(&self, doc: &Document, at: Point) -> bool {
        doc.hit_test(at).is_some_and(|hit| self.selection().iter().filter_map(|id| doc.index_of(id)).any(|root| hit == root || doc.is_inside(hit, root)))
    }

    pub(crate) fn toggle_playback(&mut self) {
        if self.playing {
            self.playing = false;
        } else if self.has_draft_pose() {
            self.play_needs_record = true;
            self.panels.set(crate::workspace::Panel::Timeline, true);
            self.status = "Поза ещё не записана: выберите запись или проигрывание без пробы".into();
        } else {
            self.playing = true;
        }
    }

    pub(crate) fn pose_actions(&mut self, ui: &mut egui::Ui) {
        if self.workspace() != Workspace::Animate {
            return;
        }
        ui.horizontal_wrapped(|ui| {
            ui.checkbox(&mut self.auto_key, "Автоключ")
                .on_hover_text("Включён: изменения пишут ключи в текущем времени. Выключен: пробная поза до явной записи.");
            if self.has_draft_pose() {
                ui.colored_label(theme::ACCENT, "Поза не записана");
                if ui.button("Записать позу").clicked() {
                    self.record_draft_pose();
                }
                if ui.button("Сбросить пробу").clicked() {
                    self.clear_draft_pose();
                }
            }
        });
        if self.play_needs_record && self.has_draft_pose() {
            ui.horizontal_wrapped(|ui| {
                if ui.button("Записать и ▶").clicked() {
                    self.record_draft_pose();
                    if !self.has_draft_pose() {
                        self.playing = true;
                    }
                }
                if ui.button("Проиграть без пробы").clicked() {
                    self.clear_draft_pose();
                    self.playing = true;
                }
            });
        }
    }

    // Stop before advancing the clock, not at drag release. This freezes both the
    // sampled transform and the key time under the pointer.
    pub(super) fn pause_for_canvas_press(&mut self, ctx: &egui::Context) {
        if !self.playing || self.anim.is_none() || self.run.is_some() || self.check.is_some() || self.floating.is_some() || self.tool != Tool::Select {
            return;
        }
        let pointer = ctx.input(|i| if i.pointer.primary_pressed() && !i.key_down(Key::Space) && !i.modifiers.alt { i.pointer.interact_pos() } else { None });
        let (Some(p), Some(view)) = (pointer, self.view) else { return };
        if !self.canvas_bounds.contains(p) || ctx.layer_id_at(p).is_some_and(|l| l.order != egui::Order::Background) {
            return;
        }
        let doc = self.display_doc();
        let handle = self
            .selected
            .as_deref()
            .and_then(|id| doc.index_of(id))
            .and_then(|i| handle_points(self.canvas_bounds, view, &doc, i))
            .is_some_and(|h| h.rotate.distance(p) < 8.0 || (h.resizable && h.corners.iter().any(|c| c.distance(p) < 7.0)));
        if handle || doc.hit_test(view.to_doc(self.canvas_bounds, p)).is_some() || crate::bone_ui::pick_bone(&doc, self.canvas_bounds, view, p).is_some() {
            self.playing = false;
        }
    }

    fn open_object_inspector(&mut self, id: String, doc: &Document, rect: Rect, view: View) {
        let anchor = doc
            .index_of(&id)
            .and_then(|i| crate::selection::world_bounds(doc, i))
            .map(|r| view.to_screen(rect, Point::new(r.x1, r.y0)) + vec2(12.0, 0.0))
            .unwrap_or(rect.center());
        self.floating = Some(FloatingInspector { target: id, anchor });
        self.playing = false;
    }

    pub(super) fn canvas_context_menu(&mut self, response: &egui::Response, doc: &Document, rect: Rect, view: View) {
        if response.secondary_clicked()
            && let Some(p) = response.interact_pointer_pos()
        {
            let hit = crate::bone_ui::pick_bone(doc, rect, view, p).or_else(|| doc.hit_test(view.to_doc(rect, p)));
            if let Some(i) = hit {
                let id = doc.nodes[i].id.clone();
                if !self.selection().contains(&id) {
                    self.select_only(Some(id));
                }
                self.playing = false;
            } else {
                self.select_only(None);
            }
        }
        response.context_menu(|menu| {
            let Some(id) = self.selected.clone() else {
                menu.label("Нажмите правой кнопкой на объект");
                return;
            };
            if menu.button("Свойства объекта…").clicked() {
                self.open_object_inspector(id.clone(), doc, rect, view);
                menu.close();
            }
            if self.workspace() == Workspace::Animate {
                if menu.button("Свойства и ключи…").clicked() {
                    self.open_object_inspector(id.clone(), doc, rect, view);
                    menu.close();
                }
                if self.has_draft_pose() && menu.button("Записать пробную позу").clicked() {
                    self.record_draft_pose();
                    menu.close();
                }
                if !self.protected_character_node(&id) && menu.button("Ключи трансформации · K").clicked() {
                    self.record_transform();
                    menu.close();
                }
            }
        });
    }

    pub(super) fn floating_inspector(&mut self, ctx: &egui::Context) {
        let Some(popup) = self.floating.take() else { return };
        if self.selected.as_deref() != Some(&popup.target) || self.scene.doc.node(&popup.target).is_none() {
            return;
        }
        let mut open = true;
        let available = ctx.content_rect();
        let anchor = pos2(
            popup.anchor.x.clamp(available.left(), (available.right() - 332.0).max(available.left())),
            popup.anchor.y.clamp(available.top(), (available.bottom() - 240.0).max(available.top())),
        );
        egui::Popup::new(egui::Id::unique("object-inspector"), ctx.clone(), anchor, egui::LayerId::background())
            .open_bool(&mut open)
            .close_behavior(if egui::Popup::is_any_open(ctx) { egui::PopupCloseBehavior::IgnoreClicks } else { egui::PopupCloseBehavior::CloseOnClickOutside })
            .width(320.0)
            .show(|ui| {
                ui.set_width(320.0);
                ui.set_max_height((available.height() - 60.0).clamp(180.0, 520.0));
                ui.horizontal(|ui| {
                    ui.strong(format!("Объект: {}", popup.target));
                    if ui.button("×").on_hover_text("Закрыть · Esc").clicked() {
                        ui.close();
                    }
                });
                if self.workspace() == Workspace::Animate
                    && !self.protected_character_node(&popup.target)
                    && ui.button("Записать ключи трансформации").clicked()
                {
                    self.record_transform();
                }
                self.props_panel(ui);
            });
        if open {
            self.floating = Some(popup);
        }
    }
}
