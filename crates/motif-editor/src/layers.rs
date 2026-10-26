//! The layers panel: the scene's objects topmost first, folded groups, dragging to reorder and
//! regroup, and a context menu to rename, lock, hide and delete.

use crate::app::App;
use crate::theme::{self, ACCENT, FAINT, FG, Icon, LINE, MUTED, PANEL, SEL};
use egui::{Align2, Pos2, Rect, Sense, Stroke, pos2, vec2};
use motif_core::ShapeKind;
use motif_script::{Place, fmt_num, move_node, rename_node, set_style};
use std::collections::HashSet;

const ROW_H: f32 = 28.0;

#[derive(Default)]
pub struct Layers {
    /// Groups whose children are folded away.
    folded: HashSet<String>,
    /// The layer being dragged.
    dragging: Option<String>,
    /// The layer being renamed and the text typed so far; `true` once its field took focus.
    renaming: Option<(String, String, bool)>,
}

enum Action {
    Select(String),
    Fold(String),
    Rename(String),
    Lock(String, bool),
    Hide(String, bool),
    Delete(String),
    Group(String),
    Ungroup(String),
    Restack(String, bool),
    Board(usize),
    BoardCopy(usize),
    BoardDelete(usize),
}

impl App {
    pub(crate) fn layers_panel(&mut self, ui: &mut egui::Ui) {
        let doc = self.scene.doc.clone();
        crate::app::panel_header(ui, "Слои", &doc.nodes.len().to_string());
        let mut action = None;
        let mut rows: Vec<(usize, Rect)> = Vec::new();
        let mut renamed: Option<(String, String)> = None;
        let foot_h = 52.0;
        egui::ScrollArea::vertical().id_salt("layers").max_height(ui.available_height() - foot_h).auto_shrink([false, false]).show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            ui.add_space(4.0);
            // The boards of the file; the active one's objects follow.
            let specs = self.board_specs();
            let many = specs.len() > 1;
            for (k, (size, name)) in specs.iter().enumerate() {
                let active = k == self.boards.active;
                let (r, resp) = ui.allocate_exact_size(vec2(ui.available_width(), ROW_H), if many { Sense::click() } else { Sense::hover() });
                if active && many {
                    ui.painter().rect_filled(r, 0.0, theme::PANEL_2);
                } else if resp.hovered() {
                    ui.painter().rect_filled(r, 0.0, theme::mix(theme::PANEL_2, PANEL, 0.5));
                }
                let color = if active { FG } else { MUTED };
                theme::paint_icon(
                    ui.painter(),
                    Rect::from_center_size(pos2(r.left() + 19.0, r.center().y), vec2(14.0, 14.0)),
                    Icon::Artboard,
                    if active { ACCENT } else { MUTED },
                );
                ui.painter().text(
                    pos2(r.left() + 32.0, r.center().y),
                    Align2::LEFT_CENTER,
                    name,
                    if active { theme::semibold(13.0) } else { theme::ui_font(13.0) },
                    color,
                );
                ui.painter().text(
                    pos2(r.right() - 10.0, r.center().y),
                    Align2::RIGHT_CENTER,
                    format!("{}×{}", fmt_num(size.0), fmt_num(size.1)),
                    theme::mono(11.0),
                    FAINT,
                );
                if resp.clicked() {
                    action = Some(Action::Board(k));
                }
                if many {
                    resp.context_menu(|ui| {
                        if ui.button("Открыть").clicked() {
                            action = Some(Action::Board(k));
                            ui.close();
                        }
                        if ui.button("Копия холста").clicked() {
                            action = Some(Action::BoardCopy(k));
                            ui.close();
                        }
                        if ui.button("Удалить холст").clicked() {
                            action = Some(Action::BoardDelete(k));
                            ui.close();
                        }
                    });
                }
            }
            if many {
                ui.add_space(2.0);
                let (line, _) = ui.allocate_exact_size(vec2(ui.available_width(), 1.0), Sense::hover());
                ui.painter().hline(line.x_range(), line.center().y, Stroke::new(1.0, LINE));
                ui.add_space(2.0);
            }
            // Topmost first, like in every layer list; a group's children under it.
            for i in self.layer_order() {
                let n = &doc.nodes[i];
                let (r, resp) = ui.allocate_exact_size(vec2(ui.available_width(), ROW_H), Sense::click_and_drag());
                rows.push((i, r));
                let sel = self.is_selected(&n.id);
                if sel {
                    ui.painter().rect_filled(r, 0.0, theme::mix(SEL, PANEL, 0.18));
                } else if resp.hovered() || self.hovered.as_deref() == Some(n.id.as_str()) {
                    ui.painter().rect_filled(r, 0.0, theme::PANEL_2);
                }
                let x = r.left() + 26.0 + 16.0 * doc.depth(i) as f32;
                let dim = if doc.is_visible(i) { 1.0 } else { 0.4 };
                let has_children = doc.nodes.iter().any(|c| c.parent == Some(i));
                let arrow = Rect::from_center_size(pos2(x - 19.0, r.center().y), vec2(14.0, ROW_H));
                if n.kind == ShapeKind::Group && has_children {
                    let open = !self.layers.folded.contains(&n.id);
                    let hot = resp.hover_pos().is_some_and(|p| arrow.contains(p));
                    crate::app::fold_arrow(ui.painter(), arrow.center(), open, if hot { FG } else { FAINT });
                }
                theme::paint_icon(
                    ui.painter(),
                    Rect::from_center_size(pos2(x - 7.0, r.center().y), vec2(14.0, 14.0)),
                    theme::shape_icon(n.kind),
                    MUTED.gamma_multiply(dim),
                );
                let editing = self.layers.renaming.as_ref().is_some_and(|(id, ..)| *id == n.id);
                if editing {
                    let field = Rect::from_min_max(pos2(x + 2.0, r.top() + 3.0), pos2(r.right() - 8.0, r.bottom() - 3.0));
                    let (_, text, focused) = self.layers.renaming.as_mut().expect("checked above");
                    let out = ui.put(field, egui::TextEdit::singleline(text).font(theme::ui_font(13.0)).margin(egui::Margin::symmetric(4, 2)));
                    // The release of the double click that opened the field takes focus away, so
                    // ask again until the field has it.
                    if !*focused {
                        if out.has_focus() {
                            *focused = true;
                        } else {
                            out.request_focus();
                        }
                    } else if out.lost_focus() {
                        // Enter or a click elsewhere keeps the name, Esc drops it.
                        if !ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                            renamed = Some((n.id.clone(), text.clone()));
                        }
                        self.layers.renaming = None;
                    }
                } else {
                    ui.painter().text(
                        pos2(x + 6.0, r.center().y),
                        Align2::LEFT_CENTER,
                        n.display_name(),
                        theme::ui_font(13.0),
                        if sel { FG } else { FG.gamma_multiply(dim) },
                    );
                }
                // Tags on the right: the lock, then what the layer is.
                let mut right = r.right() - 10.0;
                if doc.is_locked(i) {
                    let own = n.locked;
                    theme::paint_icon(
                        ui.painter(),
                        Rect::from_center_size(pos2(right - 6.0, r.center().y), vec2(12.0, 12.0)),
                        Icon::Lock,
                        if own { MUTED } else { FAINT },
                    );
                    right -= 18.0;
                }
                let is_mask = n.parent.is_some_and(|p| doc.nodes[p].clip && doc.nodes.iter().position(|c| c.parent == Some(p)) == Some(i));
                let symbol = self.symbol_of(&n.id);
                let tag = match (is_mask, self.scene.instances.get(&n.id), symbol, self.scene.sites.get(&n.id).is_some_and(|s| s.generated)) {
                    (true, ..) => Some(("маска", ACCENT)),
                    (false, Some(inst), ..) => Some((inst.symbol.as_str(), ACCENT)),
                    (false, None, Some(sym), _) => Some((sym, FAINT)),
                    (false, None, None, true) => Some(("for", FAINT)),
                    _ => None,
                };
                if let Some((tag, color)) = tag
                    && !editing
                {
                    ui.painter().text(pos2(right, r.center().y), Align2::RIGHT_CENTER, tag, theme::mono(10.5), color);
                }

                if resp.drag_started() && !editing {
                    self.layers.dragging = Some(n.id.clone());
                }
                let resp = resp.on_hover_text(format!("id: {}", n.id));
                // Quick clicks on the arrow fold and unfold; a double click elsewhere renames.
                let on_arrow = n.kind == ShapeKind::Group && has_children && resp.interact_pointer_pos().is_some_and(|p| arrow.contains(p));
                if resp.clicked() && on_arrow {
                    action = Some(Action::Fold(n.id.clone()));
                } else if resp.double_clicked() {
                    action = Some(Action::Rename(n.id.clone()));
                } else if resp.clicked() {
                    action = Some(Action::Select(n.id.clone()));
                }
                resp.context_menu(|ui| {
                    ui.set_min_width(170.0);
                    if ui.button("Переименовать").clicked() {
                        action = Some(Action::Rename(n.id.clone()));
                    }
                    if ui.button(if n.locked { "Разблокировать" } else { "Заблокировать" }).clicked() {
                        action = Some(Action::Lock(n.id.clone(), !n.locked));
                    }
                    if ui.button(if n.visible { "Скрыть" } else { "Показать" }).clicked() {
                        action = Some(Action::Hide(n.id.clone(), n.visible));
                    }
                    ui.separator();
                    let several = self.is_selected(&n.id) && self.also.len() + usize::from(self.selected.is_some()) > 1;
                    if ui.add(egui::Button::new(if several { "Сгруппировать выделенные" } else { "Сгруппировать" }).shortcut_text("Ctrl+G")).clicked()
                    {
                        action = Some(Action::Group(n.id.clone()));
                    }
                    if n.kind == ShapeKind::Group && ui.add(egui::Button::new("Разгруппировать").shortcut_text("Ctrl+Shift+G")).clicked() {
                        action = Some(Action::Ungroup(n.id.clone()));
                    }
                    if ui.add(egui::Button::new("Выше").shortcut_text("Ctrl+]")).clicked() {
                        action = Some(Action::Restack(n.id.clone(), true));
                    }
                    if ui.add(egui::Button::new("Ниже").shortcut_text("Ctrl+[")).clicked() {
                        action = Some(Action::Restack(n.id.clone(), false));
                    }
                    ui.separator();
                    if ui.button(egui::RichText::new("Удалить").color(theme::DANGER)).clicked() {
                        action = Some(Action::Delete(n.id.clone()));
                    }
                });
            }
        });

        // Dragging a layer: a line where it lands, or a frame around the group it goes into.
        if let Some(dragged) = self.layers.dragging.clone() {
            let pointer = ui.input(|i| i.pointer.interact_pos());
            let target = pointer.and_then(|p| drop_target(&doc, &rows, p, &self.layers.folded));
            let from = doc.index_of(&dragged);
            let valid = |t: usize, place: Place| {
                from.is_some_and(|f| f != t && !doc.is_inside(t, f) && !(place == Place::Into && doc.nodes[t].kind != ShapeKind::Group))
            };
            if let Some((t, place, r)) = target
                && valid(t, place)
                && from.is_some_and(|f| rows.iter().any(|(i, _)| *i == f))
            {
                let p = ui.painter();
                let x = r.left() + 20.0 + 16.0 * doc.depth(t) as f32;
                match place {
                    Place::Into => {
                        p.rect_stroke(r.shrink(1.0), 4.0, Stroke::new(1.5, ACCENT), egui::StrokeKind::Inside);
                    }
                    Place::After => {
                        p.line_segment([pos2(x, r.top()), pos2(r.right() - 6.0, r.top())], Stroke::new(2.0, ACCENT));
                    }
                    Place::Before => {
                        p.line_segment([pos2(x, r.bottom()), pos2(r.right() - 6.0, r.bottom())], Stroke::new(2.0, ACCENT));
                    }
                }
                ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
            }
            if ui.input(|i| i.pointer.any_released()) {
                self.layers.dragging = None;
                if let Some((t, place, _)) = target
                    && valid(t, place)
                {
                    self.move_layer(&dragged, &doc.nodes[t].id.clone(), place);
                }
            }
        }

        let (foot, _) = ui.allocate_exact_size(vec2(ui.available_width(), foot_h), Sense::hover());
        ui.painter().line_segment([foot.left_top(), foot.right_top()], Stroke::new(1.0, LINE));
        ui.painter().text(
            foot.left_top() + vec2(12.0, 10.0),
            Align2::LEFT_TOP,
            "Тяните слой, чтобы переставить.\nПравый клик — меню слоя",
            theme::ui_font(12.0),
            FAINT,
        );

        if let Some((id, name)) = renamed {
            self.rename_layer(&id, &name);
        }
        match action {
            Some(Action::Select(id)) => {
                if ui.input(|i| i.modifiers.shift || i.modifiers.command) {
                    self.toggle_selected(id);
                } else {
                    self.select_only(Some(id));
                }
            }
            Some(Action::Fold(id)) => {
                if !self.layers.folded.remove(&id) {
                    self.layers.folded.insert(id);
                }
            }
            Some(Action::Rename(id)) => {
                if self.scene.sites.get(&id).is_some_and(|s| s.generated) {
                    self.status = format!("«{id}» создан циклом: имя задаётся в цикле");
                } else {
                    let name = doc.node(&id).map(|n| n.name.clone().unwrap_or_default()).unwrap_or_default();
                    self.layers.renaming = Some((id, name, false));
                }
            }
            Some(Action::Lock(id, on)) => self.style_layer(
                &id,
                "locked",
                on,
                if on { "заблокирован: клики по холсту проходят сквозь него" } else { "разблокирован" },
            ),
            Some(Action::Hide(id, on)) => self.style_layer(&id, "hidden", on, if on { "скрыт" } else { "снова виден" }),
            Some(Action::Delete(id)) => {
                self.select_only(Some(id));
                self.delete_selection();
            }
            Some(Action::Group(id)) => {
                if !self.is_selected(&id) {
                    self.select_only(Some(id));
                }
                self.group_selection();
            }
            Some(Action::Ungroup(id)) => {
                self.select_only(Some(id));
                self.ungroup_selection();
            }
            Some(Action::Restack(id, up)) => {
                self.select_only(Some(id));
                self.restack_selection(up);
            }
            Some(Action::Board(k)) => {
                self.switch_board(k);
                self.boards.reveal = true;
            }
            Some(Action::BoardCopy(k)) if self.can_edit() => self.duplicate_board(k),
            Some(Action::BoardDelete(k)) if self.can_edit() => self.delete_board(k),
            Some(Action::BoardCopy(_) | Action::BoardDelete(_)) | None => {}
        }
    }

    /// Rows of the layers panel: topmost first, each group followed by its children unless folded.
    fn layer_order(&self) -> Vec<usize> {
        let doc = &self.scene.doc;
        // Children of each node (and of the artboard, last), topmost first.
        let mut kids: Vec<Vec<usize>> = vec![Vec::new(); doc.nodes.len() + 1];
        for i in (0..doc.nodes.len()).rev() {
            kids[doc.nodes[i].parent.unwrap_or(doc.nodes.len())].push(i);
        }
        let mut out = Vec::with_capacity(doc.nodes.len());
        let mut stack: Vec<usize> = kids[doc.nodes.len()].iter().rev().copied().collect();
        while let Some(i) = stack.pop() {
            out.push(i);
            if !self.layers.folded.contains(&doc.nodes[i].id) {
                stack.extend(kids[i].iter().rev());
            }
        }
        out
    }

    fn rename_layer(&mut self, id: &str, name: &str) {
        if !self.can_edit() {
            return;
        }
        let out = rename_node(&self.src, &self.scene, id, name);
        if self.apply("rename", out) {
            self.status = format!("«{id}» переименован");
        }
    }

    /// Adds or removes a flag such as `locked` or `hidden` on a layer.
    fn style_layer(&mut self, id: &str, flag: &str, on: bool, what: &str) {
        if !self.can_edit() {
            return;
        }
        match set_style(&self.src, &self.scene, id, flag, on.then_some(flag)) {
            Some(out) => {
                self.apply(flag, Some(out));
                self.status = format!("«{id}» {what}");
            }
            None => self.status = format!("«{id}» создан циклом: допишите {flag} в цикл"),
        }
    }

    fn move_layer(&mut self, id: &str, target: &str, place: Place) {
        if !self.can_edit() {
            return;
        }
        match move_node(&self.src, &self.scene, id, target, place) {
            Some(out) => {
                if self.apply("move-layer", Some(out)) {
                    self.status = match place {
                        Place::Into => format!("«{id}» теперь в группе «{target}»"),
                        Place::After => format!("«{id}» теперь над «{target}»"),
                        Place::Before => format!("«{id}» теперь под «{target}»"),
                    };
                }
            }
            None => self.status = format!("«{id}» не переносится: объекты из циклов и строки с несколькими объектами правятся в скрипте"),
        }
    }
}

/// The layer under `p` and where a dropped layer goes. Rows run topmost first, so the upper part of
/// a row means over it (after it in the script) and the lower part under it. The middle of a
/// group, and the lower part of an open group whose children follow it, mean into the group.
fn drop_target(doc: &motif_core::Document, rows: &[(usize, Rect)], p: Pos2, folded: &HashSet<String>) -> Option<(usize, Place, Rect)> {
    let &(i, r) = rows.iter().find(|(_, r)| p.y >= r.top() && p.y < r.bottom()).or_else(|| rows.last().filter(|(_, r)| p.y >= r.bottom()))?;
    let n = &doc.nodes[i];
    let f = ((p.y - r.top()) / r.height()).clamp(0.0, 1.0);
    let group = n.kind == ShapeKind::Group;
    let open_with_children = group && !folded.contains(&n.id) && doc.nodes.iter().any(|c| c.parent == Some(i));
    let place = if f < 0.3 {
        Place::After
    } else if group && (f < 0.7 || open_with_children) {
        Place::Into
    } else if group || f >= 0.5 {
        Place::Before
    } else {
        Place::After
    };
    Some((i, place, r))
}
