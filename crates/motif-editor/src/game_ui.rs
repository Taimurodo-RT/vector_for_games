//! What only the game reads: attachment points (`socket`) and areas (`area`). Their part of the
//! inspector, «Для игры», and how they show on the canvas: an area dashed, never painted, and a
//! point as a small cross with its name.

use crate::app::{App, fold};
use crate::canvas::View;
use crate::style_ui::StyleEdit;
use crate::theme::{FAINT, FG, SEL};
use egui::{Color32, Rect, RichText, Stroke, vec2};
use motif_core::kurbo::{Affine, PathEl, Point};
use motif_core::{Document, Node, ShapeKind};

/// Areas and points on the canvas.
const MARK: Color32 = Color32::from_rgb(0xff, 0x9f, 0x43);

impl App {
    pub(crate) fn game_section(&mut self, ui: &mut egui::Ui, node: &Node) {
        let mut edits: Vec<StyleEdit> = Vec::new();
        fold(ui, "Для игры", |ui| {
            ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
            let mut socket = node.socket;
            if ui.checkbox(&mut socket, RichText::new("Точка крепления").size(12.5).color(FG)).changed() {
                edits.push(("socket", socket.then(|| "socket".to_string())));
            }
            ui.label(
                RichText::new(
                    "Игра каждый шаг читает, где объект, его поворот и масштаб, и крепит к нему своё: оружие в руке, вспышку у дула, надпись над головой.",
                )
                .size(11.5)
                .color(FAINT),
            );
            if node.socket {
                self.tryon_row(ui, &node.id);
            }
            if !node.kind.is_container() && node.kind != ShapeKind::Bone {
                let mut area = node.area;
                if ui.checkbox(&mut area, RichText::new("Область").size(12.5).color(FG)).changed() {
                    edits.push(("area", area.then(|| "area".to_string())));
                }
                ui.label(
                    RichText::new("Фигура не рисуется ни в игре, ни при выгрузке: игра читает её контур как тело или зону удара. Здесь она видна пунктиром.")
                        .size(11.5)
                        .color(FAINT),
                );
            }
        });
        if !edits.is_empty() {
            self.apply_styles(&node.id, edits);
        }
    }

    /// «Примерить…»: a scene on this point, or the one on it and «×».
    fn tryon_row(&mut self, ui: &mut egui::Ui, socket: &str) {
        let on = self.tryon.iter().position(|t| t.socket == socket);
        ui.horizontal(|ui| {
            let pick = ui.button(if on.is_some() { "Другая…" } else { "Примерить…" }).on_hover_text(
                "Повесить сцену .msc на точку только в редакторе: она держится своей первой точкой крепления (или серединой холста) и идёт за анимацией. В файл не попадает",
            );
            if let Some(k) = on {
                let name = self.tryon[k].path.file_name().map_or(String::new(), |n| n.to_string_lossy().into_owned());
                ui.label(RichText::new(name).size(12.0).color(FG));
                if ui.small_button("×").on_hover_text("Снять").clicked() {
                    self.tryon.remove(k);
                    return;
                }
            }
            if pick.clicked()
                && let Some(path) = rfd::FileDialog::new().add_filter("Сцена Motif", &["msc"]).pick_file()
            {
                self.try_on(socket, &path);
            }
        });
    }

    /// Hangs the scene at `path` on point `socket`, in place of what hung there.
    pub(crate) fn try_on(&mut self, socket: &str, path: &std::path::Path) {
        if !self.scene.doc.node(socket).is_some_and(|n| n.socket) {
            self.status = format!("«{socket}» не точка крепления: отметьте её в «Для игры»");
            return;
        }
        match TryOn::load(socket, path) {
            Ok(t) => {
                self.status = format!("На «{socket}» висит {}: только в редакторе", path.display());
                self.tryon.retain(|o| o.socket != socket);
                self.tryon.push(t);
            }
            Err(e) => self.status = format!("Не примерить: {e}"),
        }
    }

    /// Every area dashed along its outline and every point as a cross, stronger when selected.
    /// Only in the editor.
    pub(crate) fn paint_marks(&self, painter: &egui::Painter, rect: Rect, view: View, doc: &Document) {
        let selected = self.selection_ids();
        let screen = |q: Point| view.to_screen(rect, q);
        for (i, n) in doc.nodes.iter().enumerate() {
            if !(n.socket || n.area) || !doc.is_visible(i) {
                continue;
            }
            let on = selected.contains(&n.id.as_str());
            let stroke = Stroke::new(if on { 1.5 } else { 1.0 }, if on { SEL } else { MARK.gamma_multiply(0.8) });
            let w = doc.world_transform(i);
            if n.area
                && let Some(path) = n.path()
            {
                let mut line: Vec<egui::Pos2> = Vec::new();
                let mut lines = Vec::new();
                motif_core::kurbo::flatten((w * path).elements().iter().copied(), 0.5 / view.zoom.max(0.01) as f64, |el| match el {
                    PathEl::MoveTo(p) => {
                        if line.len() > 1 {
                            lines.push(std::mem::take(&mut line));
                        }
                        line = vec![screen(p)];
                    }
                    PathEl::LineTo(p) => line.push(screen(p)),
                    PathEl::ClosePath => {
                        if let Some(&first) = line.first() {
                            line.push(first);
                        }
                    }
                    _ => {}
                });
                lines.push(line);
                for l in lines.iter().filter(|l| l.len() > 1) {
                    painter.extend(egui::Shape::dashed_line(l, stroke, 5.0, 4.0));
                }
            }
            if n.socket {
                let at = screen(w * Point::ZERO);
                let x = (screen(w * Point::new(1.0, 0.0)) - at).normalized() * 9.0;
                let y = vec2(-x.y, x.x);
                painter.line_segment([at - x, at + x * 1.6], stroke);
                painter.line_segment([at - y, at + y], stroke);
                painter.circle_stroke(at, 4.0, stroke);
                let name = n.name.as_deref().unwrap_or(&n.id);
                painter.text(at + vec2(8.0, -8.0), egui::Align2::LEFT_BOTTOM, name, egui::FontId::proportional(11.0), stroke.color);
            }
        }
    }
}

/// A scene hung on an attachment point while editing, to see how the game will carry it: a
/// sword in a hand, a flash at a muzzle. Never saved into the scene.
pub(crate) struct TryOn {
    /// The attachment point it hangs on.
    pub socket: String,
    pub path: std::path::PathBuf,
    pub doc: Document,
    /// Where it is held, in its own space: its first attachment point, else the middle of its
    /// artboard.
    pub grip: Affine,
}

impl TryOn {
    pub(crate) fn load(socket: &str, path: &std::path::Path) -> Result<TryOn, String> {
        let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let scene =
            motif_script::parse_in(&text, path.parent()).map_err(|e| format!("{}: {}", path.display(), e.first().map_or(String::new(), |e| e.to_string())))?;
        let doc = scene.doc;
        let grip = match doc.nodes.iter().position(|n| n.socket) {
            Some(i) => doc.world_transform(i),
            None => Affine::translate((doc.artboard.width / 2.0, doc.artboard.height / 2.0)),
        };
        Ok(TryOn { socket: socket.to_string(), path: path.to_path_buf(), doc, grip })
    }

    /// Where it is drawn on the board shown as `doc`: its grip on the point, turned and scaled
    /// with it. `None` while the point is missing or hidden.
    pub(crate) fn placed(&self, doc: &Document) -> Option<Affine> {
        let i = doc.index_of(&self.socket).filter(|i| doc.nodes[*i].socket && doc.is_visible(*i))?;
        Some(doc.world_transform(i) * self.grip.inverse())
    }
}
