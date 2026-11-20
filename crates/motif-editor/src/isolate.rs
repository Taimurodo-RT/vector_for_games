//! Isolation of a group: a double click on an object goes into its group. Until the way out
//! (Esc, the path above the canvas, a double click on empty canvas) everything outside the group
//! is dimmed and cannot be reached by a click, a marquee or Ctrl+A.

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
        if self.isolated.is_none() {
            return;
        }
        let doc = &self.scene.doc;
        let Some(g) = self.scope(doc) else {
            self.isolated = None;
            return;
        };
        if self.selection().iter().filter_map(|s| doc.index_of(s)).any(|i| i != g && !doc.is_inside(i, g)) {
            self.isolated = None;
        }
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
    pub(crate) fn put_in_scope(&self, out: String, ids: &[String]) -> String {
        let Some(group) = self.isolated.clone() else { return out };
        let base = self.path.as_ref().and_then(|p| p.parent());
        let Ok(scene) = motif_script::parse_in(&out, base) else { return out };
        let doc = &scene.doc;
        let Some(g) = doc.index_of(&group) else { return out };
        // What already lies in the group stays where it is written.
        let outside: Vec<String> = ids.iter().filter(|id| doc.index_of(id).is_some_and(|i| !doc.is_inside(i, g))).cloned().collect();
        if outside.is_empty() {
            return out;
        }
        motif_script::move_nodes(&out, &scene, &outside, &group, motif_script::Place::Into, base).unwrap_or(out)
    }

    /// The script with copies of `snippets`, moved by 10 units, and their ids. Copies of the
    /// objects of one group are written into that group, beside the originals, so their numbers
    /// mean the same place; while a group is isolated, copies from outside it come into it and
    /// stand where the originals stand on the artboard.
    pub(crate) fn paste_snippets(&self, snippets: &[motif_script::Snippet]) -> (String, Vec<String>) {
        let home = snippets.first().and_then(|s| s.parent.clone()).filter(|h| snippets.iter().all(|s| s.parent.as_ref() == Some(h)));
        let (out, ids) = motif_script::paste_nodes_into(&self.src, &self.scene, snippets, (10.0, 10.0), home.as_deref());
        (self.put_in_scope(out, &ids), ids)
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
                ui.add_space(6.0);
                ui.label(text("Esc — выйти", FAINT));
            });
        ui.data_mut(|d| d.insert_temp(seen, frame.response.rect));
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
