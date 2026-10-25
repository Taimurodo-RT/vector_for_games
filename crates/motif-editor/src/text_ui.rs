//! Panel rows for text: the words, font, alignment and line height, and layered outlines.

use crate::style_ui::StyleEdit;
use crate::theme::{self, ButtonKind, FAINT, KeyState, LINE, MUTED};
use egui::{Color32, RichText, Sense, Stroke, StrokeKind, vec2};
use motif_core::{Color, Node, TextAlign};
use motif_script::fmt_num;

/// How the content is written in the script: one line, `\n` between lines, no double quotes.
pub fn content_text(s: &str) -> String {
    format!("content \"{}\"", s.replace('"', "″").replace('\n', "\\n"))
}

/// The words, edited in place; every change rewrites `content "…"`.
pub fn content_row(ui: &mut egui::Ui, node: &Node, edits: &mut Vec<StyleEdit>) {
    let mut text = node.text.as_ref().map(|t| t.content.clone()).unwrap_or_default();
    let edit = egui::TextEdit::multiline(&mut text).desired_rows(2).desired_width(ui.available_width()).hint_text("Текст");
    if ui.add(edit).changed() {
        edits.push(("content", Some(content_text(&text))));
    }
}

/// Font, alignment and line height. Returns true when a font file should be picked.
pub fn layout_rows(ui: &mut egui::Ui, node: &Node, font: Option<&str>, edits: &mut Vec<StyleEdit>) -> bool {
    let mut pick = false;
    ui.label(RichText::new("Шрифт").size(12.0).color(MUTED));
    {
        let cur = match font {
            None | Some("regular") => 0,
            Some("bold") => 1,
            Some(_) => 2,
        };
        if let Some(k) = ui.push_id("font", |ui| theme::segmented(ui, &["Обычный", "Жирный", "Файл…"], cur, false)).inner {
            match k {
                0 => edits.push(("font", None)),
                1 => edits.push(("font", Some("font bold".into()))),
                _ => pick = true,
            }
        }
    }
    if let Some(f) = font.filter(|f| *f != "bold" && *f != "regular") {
        ui.label(RichText::new(f).size(11.5).color(FAINT));
    }
    ui.label(RichText::new("Выравнивание").size(12.0).color(MUTED));
    {
        let cur = TextAlign::ALL.iter().position(|a| *a == node.text_style.align).unwrap_or(1);
        if let Some(k) = ui.push_id("align", |ui| theme::segmented(ui, &["Влево", "Центр", "Вправо"], cur, false)).inner {
            let a = TextAlign::ALL[k];
            edits.push(("align", (a != TextAlign::default()).then(|| format!("align {}", a.keyword()))));
        }
    }
    let w = ui.available_width();
    if let Some(v) = theme::num_field(ui, w, "Межстрочный ×", node.text_style.line_height, 0.01, 0.3..=5.0, 2, KeyState::Hidden).changed {
        let v = (v * 100.0).round() / 100.0;
        edits.push(("line-height", (v != 1.2).then(|| format!("line-height {}", fmt_num(v)))));
    }
    pick
}

fn outline_text(list: &[(Color, f64)]) -> Option<String> {
    (!list.is_empty()).then(|| {
        let parts: Vec<String> = list.iter().map(|(c, w)| format!("{} {}", c.to_hex(), fmt_num(*w))).collect();
        format!("outline {}", parts.join(" "))
    })
}

/// Extra strokes under the shape, like the thick outlines of game titles and damage numbers.
pub fn outline_rows(ui: &mut egui::Ui, node: &Node, edits: &mut Vec<StyleEdit>) {
    let mut list = node.outlines.clone();
    let mut changed = false;
    let mut remove = None;
    for (k, (c, w)) in list.iter_mut().enumerate() {
        ui.push_id(("outline", k), |ui| {
            ui.horizontal(|ui| {
                let (sw, resp) = ui.allocate_exact_size(vec2(28.0, 28.0), Sense::click());
                let c32 = Color32::from_rgba_unmultiplied(c.r, c.g, c.b, c.a);
                ui.painter().rect_filled(sw, 6.0, c32);
                ui.painter().rect_stroke(sw, 6.0, Stroke::new(1.0, LINE), StrokeKind::Inside);
                let mut pick = c32;
                egui::Popup::from_toggle_button_response(&resp).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside).show(|ui| {
                    if egui::color_picker::color_picker_color32(ui, &mut pick, egui::color_picker::Alpha::OnlyBlend) {
                        let [r, g, b, a] = pick.to_srgba_unmultiplied();
                        *c = Color { r, g, b, a };
                        changed = true;
                    }
                });
                let fw = ui.available_width() - 40.0;
                if let Some(v) = theme::num_field(ui, fw, "Толщина", *w, 0.2, 0.5..=500.0, 1, KeyState::Hidden).changed {
                    *w = (v * 10.0).round() / 10.0;
                    changed = true;
                }
                if theme::button(ui, "×", ButtonKind::Normal).on_hover_text("Убрать слой").clicked() {
                    remove = Some(k);
                }
            });
        });
    }
    if let Some(k) = remove {
        list.remove(k);
        changed = true;
    }
    if theme::button(ui, "+ Слой обводки", ButtonKind::Normal).on_hover_text("Ещё одна обводка под фигурой, шире предыдущей").clicked()
    {
        let widest = list.iter().map(|o| o.1).fold(node.stroke_width.max(2.0), f64::max);
        let color = if list.len().is_multiple_of(2) { Color::rgb(0x1b, 0x1d, 0x24) } else { Color::WHITE };
        list.push((color, (widest + 6.0).round()));
        changed = true;
    }
    if changed {
        list.sort_by(|a, b| b.1.total_cmp(&a.1));
        edits.push(("outline", outline_text(&list)));
    }
}
