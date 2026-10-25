//! The «Раскладка» section: a group stacks its children in a row or a column, a child can be
//! the panel behind the stack, and any object can be measured from an edge of the artboard.

use crate::app::{App, fold};
use crate::style_ui::StyleEdit;
use crate::theme::{self, FAINT, FG, KeyState, MUTED};
use egui::{RichText, vec2};
use motif_core::{Node, ShapeKind, Stack, StackAlign, StackDir, anchor_text};
use motif_script::fmt_num;

fn stack_text(s: &Stack) -> String {
    match s.align {
        StackAlign::Center => format!("stack {}", s.dir.keyword()),
        a => format!("stack {} {}", s.dir.keyword(), a.keyword()),
    }
}

impl App {
    pub(crate) fn layout_section(&self, ui: &mut egui::Ui, node: &Node, half: f32, styles: &mut Vec<StyleEdit>) {
        if node.kind == ShapeKind::Bone {
            return;
        }
        let doc = &self.scene.doc;
        let Some(i) = doc.index_of(&node.id) else { return };
        let parent_stack = node.parent.and_then(|p| doc.nodes[p].stack);
        fold(ui, "Раскладка", |ui| {
            ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
            if node.kind == ShapeKind::Group {
                let at = match node.stack.map(|s| s.dir) {
                    None => 0,
                    Some(StackDir::Row) => 1,
                    Some(StackDir::Column) => 2,
                };
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Стек").size(12.0).color(MUTED));
                    if let Some(k) = theme::segmented(ui, &["Нет", "Ряд", "Столбик"], at, false) {
                        match k {
                            0 => {
                                styles.push(("stack", None));
                                styles.push(("gap", None));
                                styles.push(("pad", None));
                            }
                            k => {
                                let dir = if k == 1 { StackDir::Row } else { StackDir::Column };
                                let s = Stack { dir, ..node.stack.unwrap_or(Stack::new(dir)) };
                                styles.push(("stack", Some(stack_text(&s))));
                            }
                        }
                    }
                });
                if let Some(s) = node.stack {
                    let across = if s.dir == StackDir::Row { ["Верх", "Середина", "Низ"] } else { ["Лево", "Центр", "Право"] };
                    let a = StackAlign::ALL.iter().position(|x| *x == s.align).unwrap_or(1);
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("Поперёк").size(12.0).color(MUTED));
                        if let Some(k) = theme::segmented(ui, &across, a, false) {
                            styles.push(("stack", Some(stack_text(&Stack { align: StackAlign::ALL[k], ..s }))));
                        }
                    });
                    ui.horizontal(|ui| {
                        let gap = theme::num_field(ui, half, "Зазор", s.gap, 0.5, 0.0..=1000.0, 1, KeyState::Hidden);
                        if let Some(v) = gap.changed {
                            styles.push(("gap", Some(format!("gap {}", fmt_num(v.max(0.0))))));
                        }
                        let pad = theme::num_field(ui, half, "Поля", s.pad, 0.5, 0.0..=1000.0, 1, KeyState::Hidden);
                        if let Some(v) = pad.changed {
                            styles.push(("pad", Some(format!("pad {}", fmt_num(v.max(0.0))))));
                        }
                    });
                    let size = node.frame.map(|f| format!(" Сейчас стек {}×{}.", fmt_num(f.width()), fmt_num(f.height()))).unwrap_or_default();
                    ui.label(
                        RichText::new(format!(
                            "Дети встают по порядку в скрипте: первый — слева или сверху, их «at» не важен. Порядок меняется перетаскиванием в слоях.{size}"
                        ))
                        .size(11.5)
                        .color(FAINT),
                    );
                } else {
                    ui.label(
                        RichText::new("Ряд или столбик выстроит детей группы с зазором и полями, как flex в CSS: меню, списки, панели кнопок.")
                            .size(11.5)
                            .color(FAINT),
                    );
                }
            }
            if parent_stack.is_some() {
                let mut fit = node.fit;
                if ui.checkbox(&mut fit, RichText::new("Фон по размеру стека").size(12.5).color(FG)).changed() {
                    styles.push(("fit", fit.then(|| "fit".to_string())));
                }
                ui.label(RichText::new("Встаёт позади, во весь стек с полями, и растёт вместе с ним.").size(11.5).color(FAINT));
                return;
            }
            // Anchor: a 3×3 grid of the points of the box the position is measured from.
            let room = doc.anchor_room(i);
            let Some(room) = room else { return };
            let what = if node.parent.is_none() { "артборда" } else { "группы" };
            ui.label(RichText::new(format!("Привязка к краю {what}")).size(12.0).color(MUTED));
            let mut pick: Option<Option<(f64, f64)>> = None;
            ui.horizontal(|ui| {
                let (area, _) = ui.allocate_exact_size(vec2(64.0, 64.0), egui::Sense::hover());
                let p = ui.painter();
                p.rect(area, 4.0, theme::BG, egui::Stroke::new(1.0, theme::LINE), egui::StrokeKind::Inside);
                for (row, v) in [0.0, 0.5, 1.0].into_iter().enumerate() {
                    for (col, h) in [0.0, 0.5, 1.0].into_iter().enumerate() {
                        let c = area.min + vec2(11.0 + 21.0 * col as f32, 11.0 + 21.0 * row as f32);
                        let cell = egui::Rect::from_center_size(c, vec2(16.0, 16.0));
                        let resp = ui.interact(cell, ui.scope_id().with(("anchor", row, col)), egui::Sense::click());
                        let on = node.anchor == Some((h, v));
                        let color = if on {
                            theme::ACCENT
                        } else if resp.hovered() {
                            FG
                        } else {
                            theme::LINE
                        };
                        ui.painter().rect_filled(egui::Rect::from_center_size(c, vec2(if on { 10.0 } else { 7.0 }, if on { 10.0 } else { 7.0 })), 2.0, color);
                        if resp.on_hover_text(anchor_text((h, v))).clicked() {
                            pick = Some(if on { None } else { Some((h, v)) });
                        }
                    }
                }
                ui.vertical(|ui| {
                    let text = match node.anchor {
                        Some(a) => anchor_text(a),
                        None => "нет: at от левого верхнего угла; щелчок по точке — привязать".into(),
                    };
                    ui.label(RichText::new(text).font(theme::mono(11.5)).color(FG));
                    ui.label(
                        RichText::new("Объект держится этой точки, когда размер меняется: кнопка «закрыть» в углу, счёт сверху по центру.")
                            .size(11.5)
                            .color(FAINT),
                    );
                });
            });
            if let Some(anchor) = pick {
                // Keep the object where it is: `at` becomes the distance from the new point.
                let point = |a: Option<(f64, f64)>| a.map_or((0.0, 0.0), |(h, v)| (room.x0 + h * room.width(), room.y0 + v * room.height()));
                let (old, new) = (point(node.anchor), point(anchor));
                let (x, y) = (node.x + old.0 - new.0, node.y + old.1 - new.1);
                styles.push(("anchor", anchor.map(anchor_text)));
                styles.push(("at", Some(format!("at {} {}", fmt_num(x), fmt_num(y)))));
            }
        });
    }
}
