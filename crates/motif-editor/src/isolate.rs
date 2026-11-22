//! Isolation of a group: a double click on an object goes into its group. Until the way out
//! (Esc, the path above the canvas, a double click on empty canvas) everything outside the group
//! is dimmed and cannot be reached by a click, a marquee or Ctrl+A.
//!
//! Inside a copy of a symbol, in «Рисование», the symbol itself is edited: the script is read
//! with the lines of the symbol open for edits ([`motif_script::Reading::symbol_of`]), so what
//! is changed, added or removed there changes every copy. «Только эта копия» on the strip above
//! the canvas brings back edits of this copy alone.

use crate::app::App;
use crate::canvas::View;
use crate::theme::{self, ACCENT, FAINT, FG, LINE, MUTED, PANEL};
use egui::{Pos2, Rect, RichText, vec2};
use motif_core::kurbo::Point;
use motif_core::{Document, ShapeKind};

/// How much of its opacity an object outside the isolated group keeps on the canvas.
pub(crate) const DIM: f64 = 0.25;

/// The nearest group around node `i`.
pub(crate) fn group_of(doc: &Document, i: usize) -> Option<usize> {
    let mut cur = doc.nodes[i].parent;
    while let Some(p) = cur {
        if doc.nodes[p].kind == ShapeKind::Group {
            return Some(p);
        }
        cur = doc.nodes[p].parent;
    }
    None
}

impl App {
    /// The isolated group in `doc`.
    pub(crate) fn scope(&self, doc: &Document) -> Option<usize> {
        self.isolated.as_deref().and_then(|g| doc.index_of(g)).filter(|&g| doc.nodes[g].kind == ShapeKind::Group)
    }

    /// Whether node `i` can be reached now: always, or only inside the isolated group.
    pub(crate) fn in_scope(&self, doc: &Document, i: usize) -> bool {
        self.scope(doc).is_none_or(|g| doc.is_inside(i, g))
    }

    /// The topmost shape under an artboard point that can be reached now.
    pub(crate) fn pick_shape(&self, doc: &Document, at: Point) -> Option<usize> {
        doc.hit_test_in(at, self.scope(doc))
    }

    /// What a click at screen point `p` reaches: a bone first (they are thin), then a shape.
    pub(crate) fn pick(&self, doc: &Document, rect: Rect, view: View, p: Pos2) -> Option<usize> {
        crate::bone_ui::pick_bone(doc, rect, view, p).filter(|&b| self.in_scope(doc, b)).or_else(|| self.pick_shape(doc, view.to_doc(rect, p)))
    }

    /// What a marquee and Ctrl+A choose from: the objects of the artboard, or the objects of the
    /// isolated group.
    pub(crate) fn scope_children(&self, doc: &Document) -> Vec<usize> {
        let scope = self.scope(doc);
        (0..doc.nodes.len()).filter(|&i| doc.nodes[i].parent == scope).collect()
    }

    /// The copy of a symbol that the isolated group is, or lies in, with the name of its symbol.
    pub(crate) fn copy_around(&self) -> Option<(String, String)> {
        let doc = &self.scene.doc;
        let mut cur = self.scope(doc);
        while let Some(i) = cur {
            if let Some(inst) = self.scene.instances.get(&doc.nodes[i].id) {
                return Some((doc.nodes[i].id.clone(), inst.symbol.clone()));
            }
            cur = doc.nodes[i].parent;
        }
        None
    }

    /// The copy through which a symbol is edited now: the one being worked in, in «Рисование»,
    /// unless the edits are asked to stay with this copy. In the other workspaces keys are
    /// written, and they belong to the copy.
    pub(crate) fn symbol_copy(&self) -> Option<String> {
        if self.copy_only || self.workspace() != crate::workspace::Workspace::Drawing {
            return None;
        }
        self.copy_around().map(|(copy, _)| copy)
    }

    /// How many copies symbol `symbol` has on this artboard.
    pub(crate) fn copies_of(&self, symbol: &str) -> usize {
        self.scene.instances.values().filter(|i| i.symbol == symbol).count()
    }

    /// Reads the script again when what is edited changed: the symbol through a copy, or the
    /// scene as usual. «Только эта копия» holds for one stay inside a copy.
    pub(crate) fn sync_symbol(&mut self) {
        if self.copy_only && self.copy_around().is_none() {
            self.copy_only = false;
        }
        if self.errors.is_empty() && self.symbol_copy() != self.scene.editing {
            self.reparse();
        }
    }

    /// Sets whether the edits made inside a copy stay with this copy or change its symbol.
    pub(crate) fn set_copy_only(&mut self, on: bool) {
        self.copy_only = on;
        self.sync_symbol();
        self.status = match (on, self.copy_around()) {
            (true, _) => "Правки остаются у этой копии: они пишутся в блок override. Символ и другие копии не меняются".into(),
            (false, Some((_, symbol))) => format!("Правки меняют символ {symbol} и все его копии: {}", self.copies_of(&symbol)),
            (false, None) => return,
        };
    }

    /// Whether the edit that gave `next` would write something for the copy alone while its
    /// symbol is edited: a value that a parameter or a formula of the symbol sets cannot be
    /// rewritten in the symbol, and what the copy changes for itself is not drawn meanwhile.
    /// Such an edit is refused with the reason in the status line.
    pub(crate) fn stays_with_copy(&mut self, next: &motif_script::Scene) -> bool {
        let Some(copy) = next.editing.as_deref().filter(|c| self.scene.editing.as_deref() == Some(c)) else { return false };
        let own = |s: &motif_script::Scene| s.override_entries.iter().filter(|(node, _)| motif_script::part_of(s, copy, node).is_some()).count();
        if own(next) <= own(&self.scene) {
            return false;
        }
        self.status =
            "В символе это значение задаёт параметр или формула, поэтому в символ оно не пишется. Поменяйте параметр копии или выберите «только эту копию» на полосе над холстом"
                .into();
        true
    }

    /// Whether `next`, the open board after an edit of a symbol written in the shared head of
    /// the file, leaves another board unreadable: that board animates or changes for a copy an
    /// object the symbol no longer has. Such an edit is refused with the reason in the status.
    pub(crate) fn breaks_another_board(&mut self, next: &str) -> bool {
        if self.boards.texts.len() < 2 {
            return false;
        }
        let head = &next[..motif_script::head_end(next)];
        if head == &self.src[..motif_script::head_end(&self.src)] {
            return false;
        }
        let base = self.path.as_ref().and_then(|p| p.parent());
        let broken = self.boards.texts.iter().enumerate().filter(|(k, _)| *k != self.boards.active).find_map(|(k, board)| {
            let errors = motif_script::parse_in(&format!("{head}{board}"), base).err()?;
            Some((k + 1, errors.first().map(ToString::to_string).unwrap_or_default()))
        });
        let Some((number, why)) = broken else { return false };
        self.status = format!("Символ общий для холстов файла, и после этой правки холст №{number} не читается: {why}. Правка не записана");
        true
    }

    /// Goes into the copy `copy` to edit its symbol on the canvas.
    pub(crate) fn edit_symbol(&mut self, copy: &str) {
        if self.workspace() != crate::workspace::Workspace::Drawing {
            self.set_workspace(crate::workspace::Workspace::Drawing);
            if self.workspace() != crate::workspace::Workspace::Drawing {
                return;
            }
        }
        self.copy_only = false;
        self.isolate(Some(copy.to_string()));
    }

    /// Goes into group `id`, or out of every group. What is selected outside is let go.
    pub(crate) fn isolate(&mut self, id: Option<String>) {
        let doc = &self.scene.doc;
        let group = id.as_deref().and_then(|g| doc.index_of(g)).filter(|&g| doc.nodes[g].kind == ShapeKind::Group);
        self.isolated = group.map(|g| doc.nodes[g].id.clone());
        self.point_edit = None;
        if let Some(g) = group {
            let kept: Vec<String> = self.selection().into_iter().filter(|s| doc.index_of(s).is_some_and(|i| doc.is_inside(i, g))).collect();
            let name = doc.nodes[g].display_name().to_string();
            self.select_list(kept);
            self.status = format!("Внутри группы «{name}»: остальное приглушено и не выделяется. Esc — выйти");
        }
        self.sync_symbol();
        if let Some((copy, symbol)) = self.copy_around().filter(|(copy, _)| self.scene.editing.as_ref() == Some(copy)) {
            self.status = format!("Внутри копии «{copy}»: правки меняют символ {symbol} и все его копии ({}). Esc — выйти", self.copies_of(&symbol));
        }
    }

    /// One level out: to the group around the isolated one, or out of isolation. The group that
    /// was left becomes the selection, so it is seen and can be moved at once.
    pub(crate) fn leave_group(&mut self) {
        let doc = &self.scene.doc;
        let Some(g) = self.scope(doc) else {
            self.isolated = None;
            return;
        };
        let left = doc.nodes[g].id.clone();
        let up = group_of(doc, g).map(|p| doc.nodes[p].id.clone());
        self.isolate(up.clone());
        self.select_only(Some(left));
        if up.is_none() {
            self.status = "Вышли из группы".into();
        }
    }

    /// Isolation ends when its group is gone, and when something outside it is selected by
    /// other means (the layers, the script, an undo): what is selected must be reachable.
    pub(crate) fn sync_isolation(&mut self) {
        let doc = &self.scene.doc;
        match self.scope(doc) {
            Some(g) if self.selection().iter().filter_map(|s| doc.index_of(s)).any(|i| i != g && !doc.is_inside(i, g)) => self.isolated = None,
            Some(_) => {}
            None => self.isolated = None,
        }
        // Out of a copy by any way, its symbol is no longer what an edit changes.
        self.sync_symbol();
    }

    /// Dims what lies outside the isolated group in a document about to be shown on the canvas.
    pub(crate) fn dim_outside(&self, doc: &mut Document) {
        let Some(g) = self.scope(doc) else { return };
        // The topmost objects outside: what is inside them fades with them. The groups around
        // the isolated one stay as they are, it is drawn through them.
        let outside: Vec<usize> = (0..doc.nodes.len())
            .filter(|&k| k != g && !doc.is_inside(k, g) && !doc.is_inside(g, k))
            .filter(|&k| doc.nodes[k].parent.is_none_or(|p| doc.is_inside(g, p)))
            .collect();
        for k in outside {
            doc.nodes[k].opacity *= DIM;
        }
    }

    /// A double click on the canvas: into the group of the object under the pointer; on an
    /// object of this level, to the points of a path; on nothing, out of the isolation. `false`
    /// when it meant nothing and is an ordinary click.
    pub(crate) fn canvas_double_click(&mut self, doc: &Document, rect: Rect, view: View, p: Pos2) -> bool {
        let scope = self.scope(doc);
        match self.pick(doc, rect, view, p) {
            Some(i) => {
                let id = doc.nodes[i].id.clone();
                match group_of(doc, i) {
                    Some(g) if Some(g) != scope => {
                        self.isolate(Some(doc.nodes[g].id.clone()));
                        self.select_only(Some(id));
                        true
                    }
                    _ if doc.nodes[i].kind == ShapeKind::Path => {
                        self.select_only(Some(id));
                        self.begin_point_edit();
                        true
                    }
                    _ => false,
                }
            }
            None if scope.is_some() => {
                self.isolate(None);
                self.select_only(None);
                self.status = "Вышли из группы".into();
                true
            }
            None => false,
        }
    }

    /// New objects made while a group is isolated belong to it. `out` is the script with `ids`
    /// just written at the level of the artboard; the result has them as the last children of
    /// the group, standing where they stood. A group that cannot take them (it comes from a
    /// loop or a symbol) leaves them outside, and the isolation ends with them selected.
    ///
    /// While a symbol is edited through a copy, the objects go into the symbol: every copy gets
    /// them. There their ids begin with the id of the copy, so the ids come back too.
    pub(crate) fn put_in_scope(&self, out: String, ids: &[String]) -> (String, Vec<String>) {
        let Some(group) = self.isolated.clone() else { return (out, ids.to_vec()) };
        let base = self.path.as_ref().and_then(|p| p.parent());
        let copy = self.symbol_copy();
        let reading = motif_script::Reading { theme: None, symbol_of: copy.as_deref(), vary_seed: 0 };
        let Ok(scene) = motif_script::parse_as(&out, base, reading) else { return (out, ids.to_vec()) };
        let doc = &scene.doc;
        let Some(g) = doc.index_of(&group) else { return (out, ids.to_vec()) };
        // What already lies in the group stays where it is written.
        let outside: Vec<String> = ids.iter().filter(|id| doc.index_of(id).is_some_and(|i| !doc.is_inside(i, g))).cloned().collect();
        if outside.is_empty() {
            return (out, ids.to_vec());
        }
        // A new declaration is written with an empty line after it, to stand apart from the
        // blocks below. The object leaves that place, so the empty line goes with it: otherwise
        // every object drawn inside a group would leave one behind.
        let line_end = |id: &String| scene.sites.get(id).and_then(|site| out[site.span.end..].find('\n').map(|k| site.span.end + k + 1));
        let mut empty: Vec<usize> = outside.iter().filter_map(line_end).filter(|&at| out[at..].starts_with('\n')).collect();
        empty.sort_unstable();
        empty.dedup();
        let mut tidy = out.clone();
        for at in empty.into_iter().rev() {
            tidy.remove(at);
        }
        let Ok(scene) = motif_script::parse_as(&tidy, base, reading) else { return (out, ids.to_vec()) };
        match motif_script::move_nodes(&tidy, &scene, &outside, &group, motif_script::Place::Into, base) {
            Ok(moved) => {
                let named = |id: &String| match &scene.editing {
                    Some(copy) if outside.contains(id) => format!("{copy}-{id}"),
                    _ => id.clone(),
                };
                (moved, ids.iter().map(named).collect())
            }
            Err(_) => (out, ids.to_vec()),
        }
    }

    /// The script with copies of `snippets`, moved by 10 units, and their ids. Copies of the
    /// objects of one group are written into that group, beside the originals, so their numbers
    /// mean the same place; while a group is isolated, copies from outside it come into it and
    /// stand where the originals stand on the artboard.
    pub(crate) fn paste_snippets(&self, snippets: &[motif_script::Snippet]) -> (String, Vec<String>) {
        let home = snippets.first().and_then(|s| s.parent.clone()).filter(|h| snippets.iter().all(|s| s.parent.as_ref() == Some(h)));
        let (out, ids) = motif_script::paste_nodes_into(&self.src, &self.scene, snippets, (10.0, 10.0), home.as_deref());
        self.put_in_scope(out, &ids)
    }

    /// The path of nesting above the canvas: the artboard, the groups around the isolated one and
    /// the group itself. A click on a name goes out to that level.
    pub(crate) fn isolation_bar(&mut self, ui: &mut egui::Ui, rect: Rect, ruler: f32, below: f32) {
        let doc = &self.scene.doc;
        let Some(g) = self.scope(doc) else { return };
        let mut chain = vec![g];
        while let Some(p) = group_of(doc, chain[chain.len() - 1]) {
            chain.push(p);
        }
        chain.reverse();
        let board = doc.artboard.name.clone();
        let names: Vec<(String, String)> = chain.iter().map(|&i| (doc.nodes[i].id.clone(), doc.nodes[i].display_name().to_string())).collect();
        // Inside a copy, in «Рисование»: whether edits change the symbol or this copy alone.
        let whose = self
            .copy_around()
            .filter(|_| self.workspace() == crate::workspace::Workspace::Drawing)
            .map(|(_, symbol)| (self.copies_of(&symbol), symbol, self.copy_only));
        let mut only: Option<bool> = None;

        let at = rect.min + vec2(ruler + 12.0, ruler + 10.0 + below);
        let area = Rect::from_min_max(at, egui::pos2((rect.right() - 10.0).max(at.x + 40.0), at.y + 28.0));
        // The strip takes the clicks that fall on it: the objects under it are not picked through it.
        let seen = egui::Id::unique("isolation-bar");
        if let Some(last) = ui.data(|d| d.get_temp::<Rect>(seen)) {
            ui.interact(last, seen.with("block"), egui::Sense::click_and_drag());
        }
        let mut go: Option<Option<String>> = None;
        let mut bar = ui.new_child(egui::UiBuilder::new().max_rect(area).layout(egui::Layout::left_to_right(egui::Align::Center)));
        let frame = egui::Frame::new()
            .fill(PANEL)
            .stroke(egui::Stroke::new(1.0, theme::mix(ACCENT, LINE, 0.55)))
            .corner_radius(6.0)
            .inner_margin(egui::Margin::symmetric(10, 4))
            .show(&mut bar, |ui| {
                ui.spacing_mut().item_spacing.x = 6.0;
                let text = |t: &str, c| RichText::new(t).size(12.0).color(c);
                if ui.link(text(&board, MUTED)).on_hover_text("Выйти из всех групп").clicked() {
                    go = Some(None);
                }
                for (k, (id, name)) in names.iter().enumerate() {
                    ui.label(text("›", FAINT));
                    if k + 1 == names.len() {
                        ui.label(text(name, FG).strong());
                    } else if ui.link(text(name, MUTED)).on_hover_text("Выйти до этой группы").clicked() {
                        go = Some(Some(id.clone()));
                    }
                }
                if let Some((copies, symbol, copy_only)) = &whose {
                    ui.add_space(6.0);
                    ui.label(text("Правки меняют:", FAINT));
                    let all = ui
                        .selectable_label(!copy_only, text(&format!("символ {symbol} (копий: {copies})"), if *copy_only { MUTED } else { ACCENT }))
                        .on_hover_text("Что вы меняете, рисуете или удаляете внутри копии, меняется в самом символе и во всех его копиях");
                    if all.clicked() {
                        only = Some(false);
                    }
                    let own = ui
                        .selectable_label(*copy_only, text("только эту копию", if *copy_only { ACCENT } else { MUTED }))
                        .on_hover_text("Правки остаются у этой копии и пишутся в блок override. Их список — в панели копии, «Правки этой копии»");
                    if own.clicked() {
                        only = Some(true);
                    }
                }
                ui.add_space(6.0);
                ui.label(text("Esc — выйти", FAINT));
            });
        ui.data_mut(|d| d.insert_temp(seen, frame.response.rect));
        if let Some(on) = only.filter(|on| *on != self.copy_only) {
            self.set_copy_only(on);
        }
        if let Some(to) = go {
            // Out of every group: the outermost one is selected, so it is seen where the work was.
            let outer = names.first().map(|(id, _)| id.clone());
            self.isolate(to);
            if self.isolated.is_none() {
                self.select_only(outer);
                self.status = "Вышли из группы".into();
            }
        }
    }
}
