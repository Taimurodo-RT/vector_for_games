//! The layers panel: the scene's objects topmost first, folded groups, dragging to reorder and
//! regroup, and a context menu to rename, lock, hide and delete.

use crate::app::App;
use crate::theme::{self, ACCENT, FAINT, FG, Icon, LINE, MUTED, PANEL, SEL};
use egui::{Align2, Pos2, Rect, Sense, Stroke, pos2, vec2};
use motif_core::ShapeKind;
use motif_script::{Place, fmt_num, move_nodes, rename_node, style_nodes};
use std::collections::HashSet;

const ROW_H: f32 = 28.0;

#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum Filter {
    #[default]
    All,
    Hidden,
    Locked,
    Paths,
}
impl Filter {
    fn label(self) -> &'static str {
        match self {
            Self::All => "Все",
            Self::Hidden => "Скрытые",
            Self::Locked => "Заблокированные",
            Self::Paths => "Контуры",
        }
    }
}

fn visible_order(doc: &motif_core::Document, folded: &HashSet<String>, query: &str, filter: Filter) -> Vec<usize> {
    let query = query.trim().to_lowercase();
    let filtering = !query.is_empty() || filter != Filter::All;
    let mut included = vec![!filtering; doc.nodes.len()];
    if filtering {
        for (i, n) in doc.nodes.iter().enumerate() {
            let matches = (n.id.to_lowercase().contains(&query) || n.display_name().to_lowercase().contains(&query))
                && match filter {
                    Filter::All => true,
                    Filter::Hidden => !doc.is_visible(i),
                    Filter::Locked => doc.is_locked(i),
                    Filter::Paths => n.kind == ShapeKind::Path,
                };
            if matches {
                included[i] = true;
                let mut parent = n.parent;
                while let Some(p) = parent {
                    included[p] = true;
                    parent = doc.nodes[p].parent;
                }
            }
        }
    }
    let mut kids = vec![Vec::new(); doc.nodes.len() + 1];
    for i in (0..doc.nodes.len()).rev() {
        kids[doc.nodes[i].parent.unwrap_or(doc.nodes.len())].push(i);
    }
    let mut out = Vec::new();
    let mut stack: Vec<_> = kids[doc.nodes.len()].iter().rev().copied().collect();
    while let Some(i) = stack.pop() {
        if !included[i] {
            continue;
        }
        out.push(i);
        if filtering || !folded.contains(&doc.nodes[i].id) {
            stack.extend(kids[i].iter().rev());
        }
    }
    out
}

fn select_range(visible: &[String], selected: &[String], anchor: Option<&str>, clicked: &str, shift: bool, command: bool) -> Vec<String> {
    if shift
        && let Some(a) = anchor.and_then(|id| visible.iter().position(|s| s == id))
        && let Some(b) = visible.iter().position(|s| s == clicked)
    {
        let mut out = if command { selected.to_vec() } else { Vec::new() };
        for id in &visible[a.min(b)..=a.max(b)] {
            if !out.contains(id) {
                out.push(id.clone());
            }
        }
        out
    } else if command {
        let mut out = selected.to_vec();
        if let Some(i) = out.iter().position(|s| s == clicked) {
            out.remove(i);
        } else {
            out.push(clicked.to_owned());
        }
        out
    } else {
        vec![clicked.to_owned()]
    }
}

#[derive(Default)]
pub struct Layers {
    /// Groups whose children are folded away.
    folded: HashSet<String>,
    query: String,
    filter: Filter,
    anchor: Option<String>,
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
        ui.horizontal(|ui| {
            ui.add(egui::TextEdit::singleline(&mut self.layers.query).hint_text("Поиск по имени / id").desired_width((ui.available_width() - 30.0).max(40.0)));
            if ui.small_button("×").on_hover_text("Очистить поиск").clicked() {
                self.layers.query.clear();
            }
        });
        ui.horizontal(|ui| {
            egui::ComboBox::from_id_salt("layer-filter").selected_text(self.layers.filter.label()).show_ui(ui, |ui| {
                for f in [Filter::All, Filter::Hidden, Filter::Locked, Filter::Paths] {
                    ui.selectable_value(&mut self.layers.filter, f, f.label());
                }
            });
            ui.label(format!("Выбрано: {}", self.selection().len()));
        });
        let order = visible_order(&doc, &self.layers.folded, &self.layers.query, self.layers.filter);
        ui.horizontal(|ui| {
            if ui.small_button("Свернуть").clicked() {
                self.layers.folded = doc.nodes.iter().filter(|n| n.kind == ShapeKind::Group).map(|n| n.id.clone()).collect();
            }
            if ui.small_button("Развернуть").clicked() {
                self.layers.folded.clear();
            }
        });
        let mut drop_area = Rect::NOTHING;
        let mut rows: Vec<(usize, Rect)> = Vec::new();
        let mut renamed: Option<(String, String)> = None;
        let foot_h = 52.0;
        egui::ScrollArea::vertical().id_salt("layers").max_height(ui.available_height() - foot_h).auto_shrink([false, false]).show(ui, |ui| {
            drop_area = ui.clip_rect();
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
            for &i in &order {
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
                    let open = !self.layers.folded.contains(&n.id) || !self.layers.query.trim().is_empty() || self.layers.filter != Filter::All;
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
                    if !self.is_selected(&n.id) {
                        self.select_only(Some(n.id.clone()));
                    }
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
            let unfolded = HashSet::new();
            let folded = if self.layers.query.trim().is_empty() && self.layers.filter == Filter::All { &self.layers.folded } else { &unfolded };
            let target = pointer.filter(|p| drop_area.contains(*p)).and_then(|p| drop_target(&doc, &rows, p, folded));
            let ids = self.layer_targets(&dragged);
            let valid = |t: usize, place: Place| {
                ids.iter().all(|id| doc.index_of(id).is_some_and(|f| f != t && !doc.is_inside(t, f)))
                    && !(place == Place::Into && doc.nodes[t].kind != ShapeKind::Group)
            };
            if let Some((t, place, r)) = target
                && valid(t, place)
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
            "Shift — диапазон · Ctrl — добавить\nТяните выделение · правый клик — меню",
            theme::ui_font(12.0),
            FAINT,
        );

        if let Some((id, name)) = renamed {
            self.rename_layer(&id, &name);
        }
        match action {
            Some(Action::Select(id)) => {
                let modifiers = ui.input(|i| i.modifiers);
                let visible: Vec<_> = order.iter().map(|&i| doc.nodes[i].id.clone()).collect();
                let next = select_range(&visible, &self.selection(), self.layers.anchor.as_deref(), &id, modifiers.shift, modifiers.command);
                self.select_list(next);
                if !modifiers.shift {
                    self.layers.anchor = Some(id);
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
                if !self.is_selected(&id) {
                    self.select_only(Some(id));
                }
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
                if !self.is_selected(&id) {
                    self.select_only(Some(id));
                }
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

    fn layer_targets(&self, id: &str) -> Vec<String> {
        if self.is_selected(id) { self.selection() } else { vec![id.to_owned()] }
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
    pub(crate) fn style_layer(&mut self, id: &str, flag: &str, on: bool, what: &str) {
        if !self.can_edit() {
            return;
        }
        let ids = self.layer_targets(id);
        let base = self.path.as_ref().and_then(|p| p.parent());
        match style_nodes(&self.src, &self.scene, &ids, flag, on, base) {
            Ok(out) => {
                self.apply_once(flag, Some(out));
                self.status = format!("Слоёв: {} · {what}", ids.len());
            }
            Err(e) => self.status = format!("Набор не изменён: {e}"),
        }
    }

    pub(crate) fn move_layer(&mut self, id: &str, target: &str, place: Place) {
        if !self.can_edit() {
            return;
        }
        let ids = self.layer_targets(id);
        let base = self.path.as_ref().and_then(|p| p.parent());
        match move_nodes(&self.src, &self.scene, &ids, target, place, base) {
            Ok(out) => {
                self.apply_once("move-layers", Some(out));
                self.status = format!("Перенесено слоёв: {}. Порядок и положение сохранены", motif_script::selected_roots(&self.scene, &ids).len());
            }
            Err(e) => self.status = format!("Набор не перенесён: {e}"),
        }
    }
}

/// The layer under `p` and where a dropped layer goes. Rows run topmost first, so the upper part of
/// a row means over it (after it in the script) and the lower part under it. The middle of a
/// group, and the lower part of an open group whose children follow it, mean into the group.
fn drop_target(doc: &motif_core::Document, rows: &[(usize, Rect)], p: Pos2, folded: &HashSet<String>) -> Option<(usize, Place, Rect)> {
    let &(i, r) = rows.iter().find(|(_, r)| r.contains(p)).or_else(|| rows.last().filter(|(_, r)| p.y >= r.bottom() && p.x >= r.left() && p.x < r.right()))?;
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

#[cfg(test)]
mod tests {
    use super::*;
    fn scene() -> motif_script::Scene {
        motif_script::parse("artboard \"Layers\" 100x100\ngroup g hidden locked {\n rect a \"Голова\" size 10\n path b d \"M0 0 L10 10\"\n}\nrect c size 10\n")
            .unwrap()
    }
    fn ids(doc: &motif_core::Document, indices: Vec<usize>) -> Vec<String> {
        indices.iter().map(|&i| doc.nodes[i].id.clone()).collect()
    }
    #[test]
    fn search_reveals_matches_inside_folded_groups_without_changing_folding() {
        let s = scene();
        let folded = HashSet::from(["g".into()]);
        assert_eq!(ids(&s.doc, visible_order(&s.doc, &folded, "", Filter::All)), ["c", "g"]);
        assert_eq!(ids(&s.doc, visible_order(&s.doc, &folded, "ГОЛОВА", Filter::All)), ["g", "a"]);
        assert_eq!(ids(&s.doc, visible_order(&s.doc, &folded, "", Filter::Paths)), ["g", "b"]);
        assert!(folded.contains("g"));
    }
    #[test]
    fn filters_include_inherited_hidden_and_locked_layers() {
        let s = scene();
        let folded = HashSet::new();
        for f in [Filter::Hidden, Filter::Locked] {
            assert_eq!(ids(&s.doc, visible_order(&s.doc, &folded, "", f)), ["g", "b", "a"]);
        }
    }
    #[test]
    fn shift_range_uses_visible_order_and_ctrl_preserves_toggle() {
        let visible = vec!["d".into(), "c".into(), "b".into(), "a".into()];
        assert_eq!(select_range(&visible, &[], Some("c"), "a", true, false), ["c", "b", "a"]);
        assert_eq!(select_range(&visible, &["a".into()], Some("b"), "d", true, true), ["a", "d", "c", "b"]);
        assert_eq!(select_range(&visible, &["c".into(), "b".into()], None, "c", false, true), ["b"]);
        assert_eq!(select_range(&visible, &["c".into()], Some("filtered-out"), "a", true, false), ["a"]);
    }
    #[test]
    fn dropping_outside_layer_rows_is_not_a_reorder() {
        let s = scene();
        let row = Rect::from_min_size(pos2(0.0, 0.0), vec2(200.0, 28.0));
        let rows = vec![(s.doc.index_of("c").unwrap(), row)];
        assert!(drop_target(&s.doc, &rows, pos2(250.0, 20.0), &HashSet::new()).is_none());
        assert!(drop_target(&s.doc, &rows, pos2(250.0, 40.0), &HashSet::new()).is_none());
    }
}
