//! Object-local categories, sharing authoring controls with the sidebar.
use super::*;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum InspectorCategory {
    Transform,
    Shape,
    Contour,
    Fill,
    Stroke,
    Effects,
    Text,
    Letters,
    Mask,
    Deformer,
    Parameter(String),
    Bones,
    Weights,
    Physics,
    Orbit,
    Body,
    Morph,
    Brush,
    Layout,
    Interface,
    Symbol,
    Character,
}

impl InspectorCategory {
    pub(super) fn label<'a>(&'a self, doc: &'a Document) -> &'a str {
        match self {
            Self::Transform => "Трансформация",
            Self::Shape => "Форма",
            Self::Contour => "Контур",
            Self::Fill => "Заливка",
            Self::Stroke => "Обводка",
            Self::Effects => "Эффекты",
            Self::Text => "Текст",
            Self::Letters => "Анимация букв",
            Self::Mask => "Маска",
            Self::Deformer => "Деформер группы",
            Self::Parameter(id) => doc.parameter(id).map_or(id, |p| p.name.as_str()),
            Self::Bones => "Кости и IK",
            Self::Weights => "Веса кистью",
            Self::Physics => "Физика",
            Self::Orbit => "Орбита (2.5D)",
            Self::Body => "3D-тело",
            Self::Morph => "Морфинг",
            Self::Brush => "Кисть",
            Self::Layout => "Раскладка",
            Self::Interface => "Интерфейс",
            Self::Symbol => "Символ",
            Self::Character => "Персонаж и одежда",
        }
    }
}

pub(super) struct FloatingInspector {
    target: String,
    anchor: Pos2,
    category: InspectorCategory,
}

impl App {
    pub(super) fn pointer_hits_selection(&self, doc: &Document, at: Point) -> bool {
        doc.hit_test(at).is_some_and(|hit| self.selection().iter().filter_map(|id| doc.index_of(id)).any(|root| hit == root || doc.is_inside(hit, root)))
    }

    pub(crate) fn toggle_playback(&mut self) {
        if !self.playing && (self.deformer.form.is_some() || self.deformer.corrective.is_some()) {
            self.deformer.form = None;
            self.deformer.corrective = None;
            self.pose_preview.clear();
        }
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
        if self.anim.is_none() {
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
        if self.deformer.target.is_some()
            || handle
            || doc.hit_test(view.to_doc(self.canvas_bounds, p)).is_some()
            || crate::bone_ui::pick_bone(&doc, self.canvas_bounds, view, p).is_some()
        {
            self.playing = false;
        }
    }

    fn open_object_inspector(&mut self, id: String, category: InspectorCategory, doc: &Document, rect: Rect, view: View) {
        let anchor = doc
            .index_of(&id)
            .and_then(|i| crate::selection::world_bounds(doc, i))
            .map(|r| view.to_screen(rect, Point::new(r.x1, r.y0)) + vec2(12.0, 0.0))
            .unwrap_or(rect.center());
        self.floating = Some(FloatingInspector { target: id, anchor, category });
        self.playing = false;
    }

    pub(super) fn inspector_categories(&self, id: &str) -> Vec<InspectorCategory> {
        use InspectorCategory::*;
        let Some(n) = self.scene.doc.node(id) else { return vec![] };
        let mut categories = vec![Transform];
        if !self.also.is_empty() {
            return categories;
        }
        if matches!(n.kind, ShapeKind::Rect | ShapeKind::Star | ShapeKind::Polygon) {
            categories.push(Shape);
        }
        if n.kind == ShapeKind::Path {
            categories.push(Contour);
        }
        if !n.kind.is_container() {
            categories.extend([Fill, Stroke]);
        }
        if n.kind != ShapeKind::Bone {
            categories.extend([Effects, Layout, Interface]);
        }
        if n.kind == ShapeKind::Text {
            categories.extend([Text, Letters]);
        }
        if n.kind == ShapeKind::Group {
            categories.push(Deformer);
            if !self.scene.instances.get(id).is_some_and(|inst| self.scene.symbols.iter().any(|s| s.name == inst.symbol && s.clip)) {
                categories.push(Mask);
            }
            if self.workspace() == crate::workspace::Workspace::Drawing && motif_script::character::read(&self.src).ok().flatten().is_some() {
                categories.push(Character);
            }
        }
        if self.protected_character_node(id) {
            let Some(i) = self.scene.doc.index_of(id) else { return categories };
            for p in &self.scene.doc.parameters {
                if self
                    .scene
                    .doc
                    .drivers
                    .iter()
                    .filter(|d| d.parameter == p.id)
                    .flat_map(|d| &d.tracks)
                    .filter_map(|t| self.scene.doc.index_of(&t.node))
                    .any(|j| i == j || self.scene.doc.is_inside(i, j) || self.scene.doc.is_inside(j, i))
                {
                    categories.push(Parameter(p.id.clone()));
                }
            }
            for c in &self.scene.doc.correctives {
                if c.tracks
                    .iter()
                    .filter_map(|t| self.scene.doc.index_of(&t.node))
                    .any(|j| i == j || self.scene.doc.is_inside(i, j) || self.scene.doc.is_inside(j, i))
                {
                    for id in &c.parameters {
                        let item = Parameter(id.clone());
                        if !categories.contains(&item) {
                            categories.push(item);
                        }
                    }
                }
            }
        }
        if n.kind == ShapeKind::Bone || self.scene.doc.nodes.iter().any(|n| n.kind == ShapeKind::Bone) {
            categories.push(Bones);
        }
        if n.skin.as_ref().is_some_and(|s| !s.bones.is_empty()) {
            categories.push(Weights);
        }
        categories.push(Physics);
        if n.kind != ShapeKind::Bone {
            categories.push(Orbit);
        }
        if !n.kind.is_container() {
            categories.extend([Body, Morph, Brush]);
        }
        if n.kind == ShapeKind::Group || self.scene.instances.contains_key(id) {
            categories.push(Symbol);
        }
        categories
    }

    pub(super) fn canvas_context_menu(&mut self, response: &egui::Response, doc: &Document, rect: Rect, view: View) {
        if response.secondary_clicked()
            && let Some(p) = response.interact_pointer_pos()
        {
            let hit = crate::bone_ui::pick_bone(doc, rect, view, p).or_else(|| doc.hit_test(view.to_doc(rect, p)));
            if let Some(i) = hit {
                let id = doc.nodes[i].id.clone();
                if !self.selection().iter().filter_map(|root| doc.index_of(root)).any(|root| root == i || doc.is_inside(i, root)) {
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
            menu.label("Параметры объекта");
            for category in self.inspector_categories(&id) {
                if menu.button(category.label(&self.scene.doc)).clicked() {
                    self.open_object_inspector(id.clone(), category, doc, rect, view);
                    menu.close();
                }
            }
            if self.anim.is_some() {
                menu.separator();
                if self.has_draft_pose() && menu.button("Записать пробную позу").clicked() {
                    self.record_draft_pose();
                    menu.close();
                }
                if menu.button("Ключи трансформации · K").clicked() {
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
        if !self.inspector_categories(&popup.target).contains(&popup.category) {
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
                ui.horizontal(|ui| {
                    ui.strong(format!("{} · {}", popup.category.label(&self.scene.doc), popup.target));
                    if ui.button("×").on_hover_text("Закрыть · Esc").clicked() {
                        ui.close();
                    }
                });
                egui::ScrollArea::vertical()
                    .id_salt(("property-category", &popup.target, format!("{:?}", popup.category)))
                    .max_height((available.height() - 110.0).clamp(80.0, 650.0))
                    .auto_shrink([false, true])
                    .show(ui, |ui| {
                        ui.scope_builder(egui::UiBuilder::new().ui_stack_info(egui::UiStackInfo::default().with_tag("motif-property-category")), |ui| {
                            if self.anim.is_some() {
                                self.pose_actions(ui);
                            }
                            if popup.category == InspectorCategory::Transform && self.anim.is_some() && ui.button("Записать ключи трансформации").clicked()
                            {
                                self.record_transform();
                            }
                            self.props_contents(ui, Some(&popup.category));
                        });
                    });
            });
        if open {
            self.floating = Some(popup);
        }
    }
}
