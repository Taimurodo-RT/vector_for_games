//! «Стороны» of the artboard: the `directions` line (4 or 8 sides, which objects turn, left
//! sides mirrored) and a compass that shows the character from any side on the canvas.

use crate::app::{App, fold};
use crate::theme::{self, ACCENT, ButtonKind, FAINT, FG, LINE, MUTED, PANEL_2, RAISE};
use egui::{Color32, Rect, RichText, Sense, Stroke, StrokeKind, vec2};
use motif_core::Directions;

/// Where each of the eight sides sits in the 3×3 compass: column, row.
const CELLS: [(usize, usize); 8] = [(1, 2), (2, 2), (2, 1), (2, 0), (1, 0), (0, 0), (0, 1), (0, 2)];
/// The sides in Russian, for hints.
const NAMES: [&str; 8] = ["вниз (лицом к нам)", "вниз-вправо", "вправо", "вверх-вправо", "вверх (спиной)", "вверх-влево", "влево", "вниз-влево"];

impl App {
    /// The side the canvas shows: the one picked on the compass, or the side of an animation
    /// such as `Walk_up` while it is open.
    pub(crate) fn shown_side(&self) -> Option<usize> {
        let d = self.scene.doc.directions.as_ref()?;
        self.side.filter(|&k| k < d.count).or_else(|| {
            let anim = self.anim.as_deref()?;
            d.names().iter().position(|n| anim.strip_suffix(n).is_some_and(|b| b.ends_with('_')) && d.is_side(&self.scene.doc, anim))
        })
    }

    /// Machine `m` as it plays from the side the canvas shows: `Walk_up` instead of `Walk`.
    pub(crate) fn faced(&self, m: &motif_core::Machine) -> motif_core::Machine {
        match self.shown_side() {
            Some(k) => m.facing(&self.scene.doc, k),
            None => m.clone(),
        }
    }

    pub(crate) fn sides_section(&mut self, ui: &mut egui::Ui) {
        let doc = &self.scene.doc;
        let current = doc.directions.clone();
        // Objects a turn shows: balls with something on them, and 3D bodies.
        let turnable: Vec<(String, String)> = doc
            .nodes
            .iter()
            .filter(|n| n.body.is_some() || doc.nodes.iter().any(|o| o.on.as_deref() == Some(n.id.as_str())))
            .map(|n| (n.id.clone(), n.display_name().to_string()))
            .collect();
        let mut next: Option<Option<Directions>> = None;
        let mut pick = None;
        fold(ui, "Стороны", |ui| {
            ui.horizontal(|ui| {
                for (count, label) in [(0, "Нет"), (4, "4"), (8, "8")] {
                    let on = current.as_ref().map_or(0, |d| d.count) == count;
                    let r = theme::button(ui, label, if on { ButtonKind::On } else { ButtonKind::Normal });
                    if r.clicked() && !on {
                        next = Some((count > 0).then(|| match &current {
                            Some(d) => Directions { count, ..d.clone() },
                            None => Directions { count, turn: turnable.iter().map(|t| t.0.clone()).take(1).collect(), mirror: true },
                        }));
                    }
                }
            });
            let Some(d) = &current else {
                ui.label(
                    RichText::new("Для игр с видом сверху: персонаж снимается с 4 или 8 сторон, и выгрузка в Godot кладёт их в один атлас.")
                        .size(11.5)
                        .color(FAINT),
                );
                return;
            };
            let mut mirror = d.mirror;
            if ui
                .checkbox(&mut mirror, "Левые стороны — отражение правых")
                .on_hover_text("Не рисовать левые стороны: в игре их показывают правые, отражённые скриптом")
                .changed()
            {
                next = Some(Some(Directions { mirror, ..d.clone() }));
            }
            ui.label(RichText::new("Поворачиваются").size(12.0).color(MUTED));
            if turnable.is_empty() && d.turn.is_empty() {
                ui.label(
                    RichText::new("Поставьте глаза и детали на шар («На шаре» у фигуры) или сделайте 3D-тело: поворот по сторонам крутит их.")
                        .size(11.5)
                        .color(FAINT),
                );
            }
            ui.horizontal_wrapped(|ui| {
                let mut ids: Vec<(String, String)> = turnable.clone();
                for id in &d.turn {
                    if !ids.iter().any(|t| &t.0 == id) {
                        ids.push((id.clone(), id.clone()));
                    }
                }
                for (id, name) in ids {
                    let on = d.turn.contains(&id);
                    if theme::button(ui, &name, if on { ButtonKind::On } else { ButtonKind::Normal }).clicked() {
                        let mut turn = d.turn.clone();
                        if on {
                            turn.retain(|t| t != &id);
                        } else {
                            turn.push(id);
                        }
                        next = Some(Some(Directions { turn, ..d.clone() }));
                    }
                }
            });
            ui.label(RichText::new("Вид на холсте").size(12.0).color(MUTED));
            pick = compass(ui, d, self.shown_side());
            ui.label(
                RichText::new(
                    "Экспорт в Godot снимает каждую анимацию со всех сторон: Walk_down, Walk_right… Анимация Walk_up заменяет Walk только для этой стороны.",
                )
                .size(11.5)
                .color(FAINT),
            );
        });
        if let Some(k) = pick {
            self.side = k;
        }
        if let Some(d) = next
            && self.can_edit()
        {
            if d.as_ref().map(|d| d.count) != current.as_ref().map(|d| d.count) {
                // Side numbers mean other sides now.
                self.side = None;
            }
            let out = motif_script::set_directions(&self.src, &self.scene, d.as_ref());
            self.apply("directions", Some(out));
        }
    }
}

/// The 3×3 compass: a click on a side shows it, on the shown side or the middle goes back to
/// the scene as written. Returns the new choice when one is made.
fn compass(ui: &mut egui::Ui, d: &Directions, shown: Option<usize>) -> Option<Option<usize>> {
    const CELL: f32 = 26.0;
    let (rect, _) = ui.allocate_exact_size(vec2(CELL * 3.0 + 4.0, CELL * 3.0 + 4.0), Sense::hover());
    let mut out = None;
    let cell = |c: usize, r: usize| Rect::from_min_size(rect.min + vec2(c as f32 * (CELL + 2.0), r as f32 * (CELL + 2.0)), vec2(CELL, CELL));
    let mid = cell(1, 1);
    let resp = ui.interact(mid, ui.scope_id().with("side-none"), Sense::click()).on_hover_text("Как в скрипте");
    ui.painter().rect(mid, 5.0, if shown.is_none() { RAISE } else { PANEL_2 }, Stroke::new(1.0, LINE), StrokeKind::Inside);
    ui.painter().circle_filled(mid.center(), 3.0, if shown.is_none() { FG } else { MUTED });
    if resp.clicked() {
        out = Some(None);
    }
    for k in 0..d.count {
        // Four sides sit on the cardinal cells of the eight.
        let at = if d.count == 4 { k * 2 } else { k };
        let (c, r) = CELLS[at];
        let rc = cell(c, r);
        let mirrored = d.mirror_of(k);
        let hint = match mirrored {
            Some(m) => format!("{} — в игре отражение стороны «{}»", NAMES[at], d.names()[m]),
            None => format!("{} ({})", NAMES[at], d.names()[k]),
        };
        let resp = ui.interact(rc, ui.scope_id().with(("side", k)), Sense::click()).on_hover_text(hint);
        let on = shown == Some(k);
        let fill = if on {
            RAISE
        } else if resp.hovered() {
            PANEL_2
        } else {
            Color32::TRANSPARENT
        };
        ui.painter().rect(rc, 5.0, fill, Stroke::new(1.0, if on { ACCENT } else { LINE }), StrokeKind::Inside);
        // An arrow pointing the way the character looks.
        let a = (at as f32) * std::f32::consts::TAU / 8.0;
        let dir = vec2(a.sin(), a.cos());
        let color = if on {
            ACCENT
        } else if mirrored.is_some() {
            FAINT
        } else {
            FG
        };
        let c0 = rc.center();
        let tip = c0 + dir * 8.0;
        let stroke = Stroke::new(1.6, color);
        ui.painter().line_segment([c0 - dir * 7.0, tip], stroke);
        let side = vec2(-dir.y, dir.x);
        ui.painter().line_segment([tip, tip - dir * 5.0 + side * 4.0], stroke);
        ui.painter().line_segment([tip, tip - dir * 5.0 - side * 4.0], stroke);
        if resp.clicked() {
            out = Some(if on { None } else { Some(k) });
        }
    }
    out
}
