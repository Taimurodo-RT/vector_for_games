//! Several selected objects: marquee and Shift-click, copy and paste, duplicate, align.

use crate::app::App;
use crate::theme::{self, Icon};
use motif_core::kurbo::{Point, Rect as KRect, Vec2};
use motif_core::{Document, Prop, Value};
use motif_script::{copy_nodes, delete_nodes, paste_nodes};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Align {
    Left,
    CenterX,
    Right,
    Top,
    CenterY,
    Bottom,
    /// Equal gaps between objects, left to right.
    SpreadX,
    /// Equal gaps between objects, top to bottom.
    SpreadY,
}

/// Bounding box of a node (and its children) in artboard space.
pub fn world_bounds(doc: &Document, i: usize) -> Option<KRect> {
    crate::app::node_bounds(doc, i).map(|b| doc.world_transform(i).transform_rect_bbox(b))
}

impl App {
    /// Selected ids, the primary one first.
    pub(crate) fn selection(&self) -> Vec<String> {
        self.selected.iter().chain(self.also.iter()).cloned().collect()
    }

    pub(crate) fn is_selected(&self, id: &str) -> bool {
        self.selected.as_deref() == Some(id) || self.also.iter().any(|a| a == id)
    }

    pub(crate) fn select_only(&mut self, id: Option<String>) {
        self.selected = id;
        self.also.clear();
        self.sel_keys.clear();
    }

    /// Shift-click: adds an object to the selection or takes it out.
    pub(crate) fn toggle_selected(&mut self, id: String) {
        if self.selected.as_deref() == Some(id.as_str()) {
            self.selected = if self.also.is_empty() { None } else { Some(self.also.remove(0)) };
        } else if let Some(k) = self.also.iter().position(|a| *a == id) {
            self.also.remove(k);
        } else if self.selected.is_none() {
            self.selected = Some(id);
        } else {
            self.also.push(id);
        }
        self.sel_keys.clear();
    }

    /// Drops ids that no longer exist, e.g. after undo.
    pub(crate) fn prune_selection(&mut self) {
        let doc = &self.scene.doc;
        self.also.retain(|a| doc.node(a).is_some());
        if self.selected.is_none() && !self.also.is_empty() {
            self.selected = Some(self.also.remove(0));
        }
        let primary = self.selected.clone();
        self.also.retain(|a| Some(a) != primary.as_ref());
        self.also.dedup();
    }

    pub(crate) fn select_all(&mut self) {
        let mut ids = self.scene.doc.nodes.iter().filter(|n| n.parent.is_none()).map(|n| n.id.clone());
        self.selected = ids.next();
        self.also = ids.collect();
        self.sel_keys.clear();
    }

    /// Copies the selection: kept for pasting and put on the system clipboard as script text.
    pub(crate) fn copy_selection(&mut self, ctx: &egui::Context) -> bool {
        let ids = self.selection();
        if ids.is_empty() {
            return false;
        }
        let snippets = copy_nodes(&self.scene, &self.src, &ids);
        if snippets.is_empty() {
            self.status = "Объекты из циклов копируются вместе с циклом в скрипте".into();
            return false;
        }
        let text = snippets.iter().map(|s| s.text.as_str()).collect::<Vec<_>>().join("\n");
        ctx.copy_text(text.clone());
        self.status = format!("Скопировано объектов: {}", snippets.len());
        self.clipboard = (text, snippets);
        true
    }

    /// Pastes what was copied in the editor; text copied elsewhere is inserted as declarations.
    pub(crate) fn paste(&mut self, text: Option<&str>) {
        if !self.can_edit() {
            return;
        }
        let ours = text.is_none_or(|t| t.trim() == self.clipboard.0.trim());
        if ours && !self.clipboard.1.is_empty() {
            let (out, ids) = paste_nodes(&self.src, &self.scene, &self.clipboard.1, (10.0, 10.0));
            if self.apply("paste", Some(out)) {
                self.select_list(ids);
                self.status = "Вставлено. Копия сдвинута на 10 px".into();
            }
            return;
        }
        let Some(text) = text else { return };
        // Text from elsewhere: try it as declarations; keep the script unchanged if it does not parse.
        let candidate = format!("{}\n{}\n", self.src.trim_end(), text.trim_end());
        match motif_script::parse_in(&candidate, self.path.as_ref().and_then(|p| p.parent())) {
            Ok(_) => {
                self.apply("paste", Some(candidate));
                self.status = "Текст из буфера добавлен в конец скрипта".into();
            }
            Err(e) => self.status = format!("Буфер не похож на объекты сцены: {}", e[0]),
        }
    }

    /// Ctrl+D: copy and paste in one step.
    pub(crate) fn duplicate(&mut self) {
        let ids = self.selection();
        if ids.is_empty() || !self.can_edit() {
            return;
        }
        let snippets = copy_nodes(&self.scene, &self.src, &ids);
        if snippets.is_empty() {
            self.status = "Объекты из циклов дублируются вместе с циклом в скрипте".into();
            return;
        }
        let (out, new) = paste_nodes(&self.src, &self.scene, &snippets, (10.0, 10.0));
        if self.apply("duplicate", Some(out)) {
            self.select_list(new);
            self.status = "Дубликат сдвинут на 10 px".into();
        }
    }

    pub(crate) fn select_list(&mut self, ids: Vec<String>) {
        let mut ids = ids.into_iter();
        self.selected = ids.next();
        self.also = ids.collect();
        self.sel_keys.clear();
    }

    /// Ctrl+G: the selected objects into a new group, which becomes the selection.
    pub(crate) fn group_selection(&mut self) {
        let ids = self.selection();
        if ids.is_empty() || !self.can_edit() {
            return;
        }
        let base = self.path.as_ref().and_then(|p| p.parent()).map(|p| p.to_path_buf());
        match motif_script::group_nodes(&self.src, &self.scene, &ids, base.as_deref()) {
            Ok((out, group)) => {
                self.apply("group", Some(out));
                self.select_only(Some(group.clone()));
                self.status = format!("Сгруппировано в «{group}»: объекты на месте, анимации тоже. Ctrl+Shift+G — разгруппировать");
            }
            Err(e) => self.status = format!("Не сгруппировать: {e}"),
        }
    }

    /// Ctrl+Shift+G: the selected group taken apart, its children selected.
    pub(crate) fn ungroup_selection(&mut self) {
        let Some(id) = self.selected.clone() else { return };
        if !self.can_edit() {
            return;
        }
        let base = self.path.as_ref().and_then(|p| p.parent()).map(|p| p.to_path_buf());
        match motif_script::ungroup(&self.src, &self.scene, &id, base.as_deref()) {
            Ok((out, kids)) => {
                self.apply("ungroup", Some(out));
                self.status = format!("Группа «{id}» разобрана: объектов {}", kids.len());
                self.select_list(kids);
            }
            Err(e) => self.status = format!("Не разгруппировать «{id}»: {e}"),
        }
    }

    /// Ctrl+] and Ctrl+[: the selected object one step over or under its neighbour.
    pub(crate) fn restack_selection(&mut self, up: bool) {
        let ids = self.selection();
        if ids.is_empty() || !self.can_edit() {
            return;
        }
        match motif_script::restack_nodes(&self.src, &self.scene, &ids, up, self.path.as_ref().and_then(|p| p.parent())) {
            Ok(out) => {
                self.apply_once("restack", Some(out));
            }
            Err(e) => self.status = format!("Набор не переставлен: {e}"),
        }
    }

    /// Deletes every selected object with its keys. Returns how many were removed.
    pub(crate) fn delete_selected_nodes(&mut self) -> usize {
        let (out, removed, skipped) = match delete_nodes(&self.src, &self.scene, &self.selection(), self.path.as_ref().and_then(|p| p.parent())) {
            Ok(result) => result,
            Err(e) => {
                self.status = format!("Набор не удалён: {e}");
                return 0;
            }
        };
        self.apply_once("delete", Some(out));
        self.select_only(None);
        self.status = match (removed, skipped.is_empty()) {
            (_, false) => format!("Удалено: {removed}. Из циклов не удаляются: {}", skipped.join(", ")),
            (1, true) => "Объект удалён вместе с его ключами".into(),
            _ => format!("Удалено объектов: {removed}"),
        };
        removed
    }

    /// Moves the selected objects so their edges or centres line up, or spreads them evenly.
    pub(crate) fn align(&mut self, how: Align) {
        let doc = self.display_doc();
        let mut items: Vec<(String, usize, KRect)> =
            self.selection().into_iter().filter_map(|id| doc.index_of(&id).and_then(|i| world_bounds(&doc, i).map(|b| (id, i, b)))).collect();
        if items.len() < 2 || (matches!(how, Align::SpreadX | Align::SpreadY) && items.len() < 3) {
            return;
        }
        let all = items.iter().fold(items[0].2, |acc, it| acc.union(it.2));
        let mut moves: Vec<(String, usize, Vec2)> = Vec::new();
        match how {
            Align::SpreadX | Align::SpreadY => {
                let x = how == Align::SpreadX;
                items.sort_by(|a, b| if x { a.2.x0.total_cmp(&b.2.x0) } else { a.2.y0.total_cmp(&b.2.y0) });
                let total: f64 = items.iter().map(|it| if x { it.2.width() } else { it.2.height() }).sum();
                let span = if x { all.width() } else { all.height() };
                let gap = (span - total) / (items.len() - 1) as f64;
                let mut at = if x { all.x0 } else { all.y0 };
                for (id, i, b) in &items {
                    let d = if x { Vec2::new(at - b.x0, 0.0) } else { Vec2::new(0.0, at - b.y0) };
                    moves.push((id.clone(), *i, d));
                    at += (if x { b.width() } else { b.height() }) + gap;
                }
            }
            _ => {
                for (id, i, b) in &items {
                    let d = match how {
                        Align::Left => Vec2::new(all.x0 - b.x0, 0.0),
                        Align::CenterX => Vec2::new(all.center().x - b.center().x, 0.0),
                        Align::Right => Vec2::new(all.x1 - b.x1, 0.0),
                        Align::Top => Vec2::new(0.0, all.y0 - b.y0),
                        Align::CenterY => Vec2::new(0.0, all.center().y - b.center().y),
                        _ => Vec2::new(0.0, all.y1 - b.y1),
                    };
                    moves.push((id.clone(), *i, d));
                }
            }
        }
        self.edit_group = Some("align".into());
        for (id, i, d) in moves {
            if d.hypot() < 1e-6 {
                continue;
            }
            // Artboard-space offset into the parent's space.
            let n = &doc.nodes[i];
            let to_parent = n.parent.map_or(motif_core::kurbo::Affine::IDENTITY, |p| doc.world_transform(p).inverse());
            let origin = doc.world_transform(i) * Point::ZERO;
            let target = to_parent * (origin + d);
            self.edit(&id, Prop::X, Value::Num((target.x * 10.0).round() / 10.0));
            self.edit(&id, Prop::Y, Value::Num((target.y * 10.0).round() / 10.0));
        }
        self.edit_group = None;
    }

    /// The properties panel for several objects: count, align and spread.
    pub(crate) fn multi_panel(&mut self, ui: &mut egui::Ui) {
        let ids = self.selection();
        crate::app::section(ui, |ui| {
            ui.label(egui::RichText::new(format!("Выбрано объектов: {}", ids.len())).font(theme::semibold(14.0)).color(theme::FG));
            ui.label(egui::RichText::new(ids.join(", ")).size(11.5).color(theme::FAINT));
        });
        let mut pick = None;
        crate::app::fold(ui, "Выравнивание", |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                for (icon, how, tip) in [
                    (Icon::AlignLeft, Align::Left, "По левому краю"),
                    (Icon::AlignCenterX, Align::CenterX, "По центру по горизонтали"),
                    (Icon::AlignRight, Align::Right, "По правому краю"),
                    (Icon::AlignTop, Align::Top, "По верху"),
                    (Icon::AlignCenterY, Align::CenterY, "По центру по вертикали"),
                    (Icon::AlignBottom, Align::Bottom, "По низу"),
                ] {
                    if theme::icon_button(ui, icon, tip).clicked() {
                        pick = Some(how);
                    }
                }
            });
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                let enough = ids.len() >= 3;
                ui.add_enabled_ui(enough, |ui| {
                    if theme::icon_button(ui, Icon::SpreadX, "Равные промежутки по горизонтали (от 3 объектов)").clicked()
                    {
                        pick = Some(Align::SpreadX);
                    }
                    if theme::icon_button(ui, Icon::SpreadY, "Равные промежутки по вертикали (от 3 объектов)").clicked() {
                        pick = Some(Align::SpreadY);
                    }
                });
            });
        });
        let mut bool_op = None;
        let mut group = false;
        crate::app::fold(ui, "Булевы операции", |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                for op in motif_script::BoolOp::ALL {
                    let tip = match op {
                        motif_script::BoolOp::Union => "Объединить: всё, что покрывает любая фигура",
                        motif_script::BoolOp::Subtract => "Вычесть: нижняя фигура минус те, что выше",
                        motif_script::BoolOp::Intersect => "Пересечь: только общая часть",
                        motif_script::BoolOp::Exclude => "Исключить: общие части становятся дырками",
                    };
                    if bool_button(ui, op, tip).clicked() {
                        bool_op = Some(op);
                    }
                }
            });
            ui.label(egui::RichText::new("Фигуры заменяются одним контуром со стилем нижней.").size(11.5).color(theme::FAINT));
        });
        crate::app::section(ui, |ui| {
            if theme::button(ui, "Сгруппировать", theme::ButtonKind::Primary).on_hover_text("Ctrl+G: в новую группу, всё остаётся на месте").clicked()
            {
                group = true;
            }
            let hint = "Shift+клик или рамка — добавить к выделению. Ctrl+G — сгруппировать, Ctrl+C / Ctrl+V — копировать и вставить, Ctrl+D — дублировать, Delete — удалить.";
            ui.label(egui::RichText::new(hint).size(12.0).color(theme::MUTED));
        });
        if let Some(how) = pick {
            self.align(how);
        }
        if let Some(op) = bool_op {
            self.boolean(op);
        }
        if group {
            self.group_selection();
        }
    }

    /// Replaces the selected shapes by the result of a boolean operation.
    pub(crate) fn boolean(&mut self, op: motif_script::BoolOp) {
        if !self.can_edit() {
            return;
        }
        match motif_script::boolean(&self.src, &self.scene, &self.selection(), op) {
            Ok((out, id)) => {
                self.apply("boolean", Some(out));
                self.select_only(Some(id));
                self.status = format!("{}: готово, получился один контур", op.name());
            }
            Err(e) => self.status = format!("{}: {e}", op.name()),
        }
    }
}

/// A button whose icon shows the operation on two overlapping squares.
fn bool_button(ui: &mut egui::Ui, op: motif_script::BoolOp, tip: &str) -> egui::Response {
    use egui::{Rect, Stroke, StrokeKind, vec2};
    use motif_script::BoolOp;
    let (rect, resp) = ui.allocate_exact_size(vec2(34.0, 28.0), egui::Sense::click());
    let bg = if resp.hovered() { theme::RAISE } else { theme::PANEL_2 };
    let p = ui.painter();
    p.rect(rect, 6.0, bg, Stroke::new(1.0, theme::LINE), StrokeKind::Inside);
    let c = rect.center();
    let a = Rect::from_min_size(c + vec2(-8.0, -8.0), vec2(11.0, 11.0));
    let b = Rect::from_min_size(c + vec2(-3.0, -3.0), vec2(11.0, 11.0));
    let both = a.intersect(b);
    let fg = theme::FG;
    let line = Stroke::new(1.0, fg);
    match op {
        BoolOp::Union => {
            p.rect_filled(a, 0.0, fg);
            p.rect_filled(b, 0.0, fg);
        }
        BoolOp::Subtract => {
            p.rect_filled(a, 0.0, fg);
            p.rect_filled(both, 0.0, bg);
            p.rect_stroke(b, 0.0, Stroke::new(1.0, theme::MUTED), StrokeKind::Inside);
        }
        BoolOp::Intersect => {
            p.rect_stroke(a, 0.0, Stroke::new(1.0, theme::MUTED), StrokeKind::Inside);
            p.rect_stroke(b, 0.0, Stroke::new(1.0, theme::MUTED), StrokeKind::Inside);
            p.rect_filled(both, 0.0, fg);
        }
        BoolOp::Exclude => {
            p.rect_filled(a, 0.0, fg);
            p.rect_filled(b, 0.0, fg);
            p.rect_filled(both, 0.0, bg);
            p.rect_stroke(both, 0.0, line, StrokeKind::Inside);
        }
    }
    resp.on_hover_text(tip)
}
