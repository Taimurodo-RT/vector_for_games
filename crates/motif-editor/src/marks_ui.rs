//! «Метки перетекания»: the artist's marks between neighbouring key contours of a morphing
//! shape (`match`), placed on the canvas. One click on the solid outline (this contour), one on
//! the dashed one (the next): those two places meet as the shape flows. Motif turns the click
//! into a piece and a share of its length, so the mark survives editing the outline.

use crate::app::{App, Tool, fold};
use crate::canvas::View;
use crate::theme::{self, ButtonKind, FAINT, FG, MUTED};
use egui::{Color32, Pos2, Rect, RichText, Stroke, vec2};
use motif_core::kurbo::{Affine, BezPath, PathEl, Point};
use motif_core::{Document, MatchEnd, MorphLink, Node};

/// Marks being placed on the canvas.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Marking {
    /// The shape the marks are on.
    pub(crate) node: String,
    /// Between key contour `step` and `step + 1`.
    pub(crate) step: usize,
    /// The first click of a mark, on contour `step`, waiting for its partner.
    pub(crate) pending: Option<MatchEnd>,
}

/// How near (in pixels) the pointer must be to an outline to place a mark on it.
const REACH: f32 = 14.0;
/// The next contour's colour; this one's is the accent.
const NEXT: Color32 = Color32::from_rgb(0x5a, 0xa9, 0xff);

/// One end of a mark as the panel and the status line say it.
fn say(e: Option<MatchEnd>) -> String {
    match e {
        None => "растёт из точки".into(),
        Some(MatchEnd { piece, at: None }) => format!("кусок {piece}"),
        Some(MatchEnd { piece, at: Some(f) }) => format!("кусок {piece} · {}", motif_script::fmt_num_with(f, 3)),
    }
}

impl App {
    /// The `match` line for these marks, written to the script; `None` removes it.
    fn write_marks(&mut self, node: &str, links: &[MorphLink]) {
        let text = (!links.is_empty())
            .then(|| format!("match {}", motif_script::marks::format(links).iter().map(|s| format!("\"{s}\"")).collect::<Vec<_>>().join(" ")));
        let out = motif_script::set_style(&self.src, &self.scene, node, "match", text.as_deref());
        self.apply(&format!("match{}", self.undo.len()), out);
    }

    /// The panel section: which step, the marks on it, and the switch for placing them.
    pub(crate) fn marks_section(&mut self, ui: &mut egui::Ui, node: &Node) {
        let Some(i) = self.scene.doc.index_of(&node.id) else { return };
        let keys = self.scene.doc.key_contours(i);
        if keys.len() < 2 {
            return;
        }
        let id = node.id.clone();
        let steps = keys.len() - 1;
        let on = self.marking.as_ref().filter(|m| m.node == id).cloned();
        let mut step = on.as_ref().map_or(0, |m| m.step).min(steps - 1);
        let links = node.morph_links.clone();
        let mut remove = None;
        let mut toggle = false;
        let mut clear = false;
        fold(ui, "Метки перетекания", |ui| {
            ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
            if steps > 1 {
                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new("Шаг").size(12.0).color(MUTED));
                    for k in 0..steps {
                        let kind = if k == step { ButtonKind::On } else { ButtonKind::Normal };
                        if theme::button(ui, &format!("{k} → {}", k + 1), kind).clicked() {
                            step = k;
                        }
                    }
                });
            }
            let mine: Vec<(usize, &MorphLink)> = links.iter().enumerate().filter(|(_, l)| l.key == step).collect();
            for (n, (k, l)) in mine.iter().enumerate() {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(format!("{}. {} → {}", n + 1, say(l.from), say(l.to))).size(12.0).color(FG));
                    if ui.small_button("×").on_hover_text("Убрать метку").clicked() {
                        remove = Some(*k);
                    }
                });
            }
            ui.horizontal(|ui| {
                let label = if on.is_some() { "Готово" } else { "Ставить на холсте" };
                if theme::button(ui, label, if on.is_some() { ButtonKind::On } else { ButtonKind::Normal }).clicked() {
                    toggle = true;
                }
                if !mine.is_empty() && theme::button(ui, "Убрать все", ButtonKind::Normal).clicked() {
                    clear = true;
                }
            });
            let hint = if on.is_some() {
                "Щёлкните место на сплошном контуре, затем на пунктирном: эти места встретятся. Esc — отмена."
            } else {
                "Если точки сопоставились не так, как задумано, метки скажут, какое место становится каким. Место хранится долей длины куска, поэтому метки не ломаются при правке контура."
            };
            ui.label(RichText::new(hint).size(11.5).color(FAINT));
        });
        if let Some(m) = self.marking.as_mut().filter(|m| m.node == id)
            && m.step != step
        {
            m.step = step;
            m.pending = None;
        }
        if toggle {
            self.marking = if on.is_some() { None } else { Some(Marking { node: id.clone(), step, pending: None }) };
        }
        if let Some(k) = remove {
            let mut rest = links.clone();
            rest.remove(k);
            self.write_marks(&id, &rest);
        }
        if clear {
            let rest: Vec<MorphLink> = links.iter().filter(|l| l.key != step).copied().collect();
            self.write_marks(&id, &rest);
        }
    }

    /// The canvas while marks are placed: both outlines, the marks, and the clicks that add
    /// them. `false` when nothing is being marked, so the canvas works as usual.
    pub(crate) fn marks_input(&mut self, ui: &egui::Ui, resp: &egui::Response, rect: Rect, view: View, doc: &Document) -> bool {
        let Some(m) = self.marking.clone() else { return false };
        let ok = self.selected.as_deref() == Some(m.node.as_str()) && self.tool == Tool::Select;
        let Some(i) = doc.index_of(&m.node).filter(|_| ok) else {
            self.marking = None;
            return false;
        };
        let keys = doc.key_contours(i);
        if m.step + 1 >= keys.len() {
            self.marking = None;
            return false;
        }
        let world = doc.world_transform(i);
        let (from, to) = (&keys[m.step], &keys[m.step + 1]);
        let painter = ui.painter_at(rect);
        let screen = |p: Point| view.to_screen(rect, world * p);
        outline(&painter, from, world, view, rect, theme::ACCENT, false);
        outline(&painter, to, world, view, rect, NEXT, true);
        // The marks already there, numbered, each a line from its place to its partner's.
        let mine: Vec<&MorphLink> = doc.nodes[i].morph_links.iter().filter(|l| l.key == m.step).collect();
        for (n, l) in mine.iter().enumerate() {
            let a = l.from.and_then(|e| motif_core::place_point(from, e)).map(screen);
            let b = l.to.and_then(|e| motif_core::place_point(to, e)).map(screen);
            if let (Some(a), Some(b)) = (a, b) {
                painter.line_segment([a, b], Stroke::new(1.0, Color32::from_white_alpha(140)));
            }
            for (p, c) in [(a, theme::ACCENT), (b, NEXT)] {
                if let Some(p) = p {
                    painter.circle_filled(p, 4.5, c);
                    painter.circle_stroke(p, 4.5, Stroke::new(1.0, Color32::BLACK));
                }
            }
            if let Some(p) = a.or(b) {
                painter.text(p + vec2(6.0, -6.0), egui::Align2::LEFT_BOTTOM, format!("{}", n + 1), egui::FontId::proportional(12.0), FG);
            }
        }
        // The first click of the next mark, waiting for its partner.
        let pending = m.pending.and_then(|e| motif_core::place_point(from, e)).map(screen);
        if let Some(p) = pending {
            painter.circle_filled(p, 5.0, theme::ACCENT);
        }
        // What the pointer would mark: a place on this outline first, then on the next.
        let target = if m.pending.is_some() { to } else { from };
        let pointer = resp.hover_pos().filter(|_| resp.hovered());
        let near = pointer.and_then(|s| {
            let local = world.inverse() * view.to_doc(rect, s);
            motif_core::nearest_place(target, local).filter(|(_, q, _)| (screen(*q) - s).length() <= REACH)
        });
        if let Some((_, q, _)) = near {
            let c = if m.pending.is_some() { NEXT } else { theme::ACCENT };
            painter.circle_stroke(screen(q), 6.0, Stroke::new(2.0, c));
            if let (Some(a), Some(s)) = (pending, pointer) {
                painter.line_segment([a, s], Stroke::new(1.0, Color32::from_white_alpha(120)));
            }
            ui.ctx().set_cursor_icon(egui::CursorIcon::Crosshair);
        }
        if resp.clicked() {
            match (near, m.pending) {
                (Some((end, ..)), None) => {
                    let end = MatchEnd { piece: end.piece, at: end.at.map(|f| (f * 1000.0).round() / 1000.0) };
                    if let Some(mk) = self.marking.as_mut() {
                        mk.pending = Some(end);
                    }
                    self.status = format!("Метка: {} → щёлкните место на пунктирном контуре", say(Some(end)));
                }
                (Some((end, ..)), Some(first)) => {
                    let end = MatchEnd { piece: end.piece, at: end.at.map(|f| (f * 1000.0).round() / 1000.0) };
                    let mut links = doc.nodes[i].morph_links.clone();
                    links.push(MorphLink { key: m.step, from: Some(first), to: Some(end) });
                    let errs = motif_core::check_links(&keys, &links);
                    if let Some((_, e)) = errs.first() {
                        self.status = format!("Метка не поставлена: {e}");
                    } else if self.can_edit() {
                        let node = m.node.clone();
                        self.write_marks(&node, &links);
                        self.status = format!("Метка {}: {} → {}", links.iter().filter(|l| l.key == m.step).count(), say(Some(first)), say(Some(end)));
                    }
                    if let Some(mk) = self.marking.as_mut() {
                        mk.pending = None;
                    }
                }
                (None, _) => {}
            }
        }
        true
    }
}

/// An outline of the shape, in its place, solid or dashed.
fn outline(painter: &egui::Painter, path: &BezPath, world: Affine, view: View, rect: Rect, color: Color32, dashed: bool) {
    let mut line: Vec<Pos2> = Vec::new();
    let mut lines: Vec<Vec<Pos2>> = Vec::new();
    let mut start = None;
    motif_core::kurbo::flatten(world * path.clone(), 0.25, |el| match el {
        PathEl::MoveTo(p) => {
            if line.len() > 1 {
                lines.push(std::mem::take(&mut line));
            }
            line.clear();
            let s = view.to_screen(rect, p);
            start = Some(s);
            line.push(s);
        }
        PathEl::LineTo(p) => line.push(view.to_screen(rect, p)),
        PathEl::ClosePath => {
            if let Some(s) = start {
                line.push(s);
            }
        }
        _ => {}
    });
    if line.len() > 1 {
        lines.push(line);
    }
    for l in lines {
        painter.add(egui::Shape::line(l.clone(), Stroke::new(3.5, Color32::from_black_alpha(140))));
        if dashed {
            painter.extend(egui::Shape::dashed_line(&l, Stroke::new(1.8, color), 6.0, 4.0));
        } else {
            painter.add(egui::Shape::line(l, Stroke::new(1.8, color)));
        }
    }
}
