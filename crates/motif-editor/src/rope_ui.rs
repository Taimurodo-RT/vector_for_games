//! Ropes and cloth in the «Физика» section: making a shape a rope or a cloth, where it is
//! pinned, and hanging objects and chain links on ropes.

use crate::style_ui::StyleEdit;
use crate::theme::{self, FAINT, FG, KeyState, MUTED};
use egui::RichText;
use motif_core::{Document, Node, Pin, Rope};
use motif_script::fmt_num;

/// The «Верёвка» and «Ткань» checkboxes with their settings, then hanging on a rope and
/// chain links along one.
pub(crate) fn rope_editor(ui: &mut egui::Ui, edits: &mut Vec<StyleEdit>, node: &Node, doc: &Document) {
    let rope = node.rope.as_ref();
    // Groups and bones cannot be ropes, but can hang on one.
    let shape = !node.kind.is_container();
    for cloth in [false, true].into_iter().filter(|_| shape) {
        let (key, label) = if cloth { ("cloth", "Ткань") } else { ("rope", "Верёвка") };
        let mut on = rope.is_some_and(|r| r.cloth == cloth);
        if ui.checkbox(&mut on, RichText::new(label).size(12.5).color(FG)).changed() {
            let other = if cloth { "rope" } else { "cloth" };
            edits.extend([(other, None), ("pin", None), ("wind", None), ("tie", None)]);
            if node.emit.is_none() && node.soft.is_none() {
                edits.push(("gravity", None));
            }
            edits.push((key, on.then(|| key.to_string())));
        }
    }
    if let Some(r) = rope {
        settings(ui, edits, node, r, doc);
    }
    let ropes: Vec<&Node> = doc.nodes.iter().filter(|n| n.id != node.id && n.rope.as_ref().is_some_and(|r| !r.cloth)).collect();
    if ropes.is_empty() {
        return;
    }
    let ids: Vec<String> = ropes.iter().map(|n| n.id.clone()).collect();
    let hang = node.hang.clone();
    if let Some(id) = pick(ui, ("hang", &node.id), "Висит на верёвке", hang.as_ref().map(|h| &h.0), &ids) {
        let at = hang.as_ref().map_or(1.0, |h| h.1);
        edits.push(("hang", id.map(|id| if at == 1.0 { format!("hang {id}") } else { format!("hang {id} {}", fmt_num(at)) })));
    }
    if let Some((id, at)) = &hang {
        let half = (ui.available_width() - 6.0) / 2.0;
        if let Some(v) = theme::num_field(ui, half, "Где, 0–1", *at, 0.005, 0.0..=1.0, 2, KeyState::Hidden).changed {
            let v = (v * 100.0).round() / 100.0;
            edits.push(("hang", Some(if v == 1.0 { format!("hang {id}") } else { format!("hang {id} {}", fmt_num(v)) })));
        }
        ui.label(
            RichText::new("Точка вращения объекта (ось) цепляется к верёвке: 0 — её начало, 1 — конец. На конце объект поворачивается вместе с верёвкой.")
                .size(11.5)
                .color(FAINT),
        );
    }
    if shape && let Some(id) = pick(ui, ("links", &node.id), "Звенья цепи вдоль", node.links.as_ref(), &ids) {
        edits.push(("links", id.map(|id| format!("links {id}"))));
    }
    if node.links.is_some() {
        ui.label(RichText::new("Фигура рисуется копиями вдоль верёвки, каждая вторая повёрнута боком: так выглядит цепь.").size(11.5).color(FAINT));
    }
}

/// Firmness, damping, gravity, wind, pins and what holds the rope's end.
fn settings(ui: &mut egui::Ui, edits: &mut Vec<StyleEdit>, node: &Node, r: &Rope, doc: &Document) {
    let key = if r.cloth { "cloth" } else { "rope" };
    let d = if r.cloth { Rope::cloth() } else { Rope::default() };
    let head = |stiff: f64, damp: f64| {
        if stiff == d.stiff && damp == d.damp {
            key.to_string()
        } else if damp == d.damp {
            format!("{key} {}", fmt_num(stiff))
        } else {
            format!("{key} {} {}", fmt_num(stiff), fmt_num(damp))
        }
    };
    let round = |v: f64| (v * 100.0).round() / 100.0;
    let half = (ui.available_width() - 6.0) / 2.0;
    ui.horizontal(|ui| {
        if let Some(v) = theme::num_field(ui, half, "Жёсткость", r.stiff, 0.005, 0.0..=1.0, 2, KeyState::Hidden).changed {
            edits.push((key, Some(head(round(v), r.damp))));
        }
        if let Some(v) = theme::num_field(ui, half, "Затухание", r.damp, 0.002, 0.0..=1.0, 2, KeyState::Hidden).changed {
            edits.push((key, Some(head(r.stiff, round(v)))));
        }
    });
    ui.horizontal(|ui| {
        if let Some(v) = theme::num_field(ui, half, "Свой ветер", r.wind, 5.0, -5000.0..=5000.0, 0, KeyState::Hidden).changed {
            let v = v.round();
            edits.push(("wind", (v != 0.0).then(|| format!("wind {}", fmt_num(v)))));
        }
    });
    // Pins: a toggle for each end of a rope, each edge and corner of a cloth.
    let choices: &[(Pin, &str)] = if r.cloth {
        &[
            (Pin::Top, "Верх"),
            (Pin::Bottom, "Низ"),
            (Pin::Left, "Лево"),
            (Pin::Right, "Право"),
            (Pin::TopLeft, "↖"),
            (Pin::TopRight, "↗"),
            (Pin::BottomLeft, "↙"),
            (Pin::BottomRight, "↘"),
        ]
    } else {
        &[(Pin::Start, "Начало"), (Pin::End, "Конец")]
    };
    ui.horizontal_wrapped(|ui| {
        ui.label(RichText::new("Закреплено").size(12.0).color(MUTED));
        for &(pin, label) in choices {
            let on = r.pins.contains(&pin);
            if ui.selectable_label(on, RichText::new(label).size(12.0)).clicked() {
                let mut pins: Vec<Pin> = r.pins.clone();
                if on {
                    pins.retain(|p| *p != pin);
                } else {
                    pins.push(pin);
                }
                let text = if pins == Rope::default_pins(r.cloth) {
                    None
                } else if pins.is_empty() {
                    Some("pin none".to_string())
                } else {
                    Some(format!("pin {}", pins.iter().map(|p| p.word()).collect::<Vec<_>>().join(" ")))
                };
                edits.push(("pin", text));
            }
        }
    });
    if !r.cloth {
        let others: Vec<String> =
            doc.nodes.iter().filter(|n| n.id != node.id && n.hang.as_ref().is_none_or(|h| h.0 != node.id)).map(|n| n.id.clone()).collect();
        let tied = node.tie.last();
        if let Some(id) = pick(ui, ("tie", &node.id), "Конец держит", tied, &others) {
            // Keep a tied start, if the script has one.
            let start = (node.tie.len() > 1).then(|| node.tie[0].clone());
            edits.push(("tie", id.map(|id| start.map_or(format!("tie {id}"), |s| format!("tie {s} {id}")))));
        }
    }
    let hint = if r.cloth {
        "Фигура становится сеткой, которая висит на закреплённых краях, провисает и полощется на ветру. Края следуют за фигурой, когда её двигают ключи или кости."
    } else {
        "Контур становится верёвкой: провисает, качается, ложится на препятствия. Жёсткость 1 — не тянется, меньше — резинка. Закреплённые концы следуют за фигурой; «Конец держит» привязывает конец к другому объекту."
    };
    ui.label(RichText::new(hint).size(11.5).color(FAINT));
}

/// A combo box over `ids` with «нет» first. Returns the new choice when it changes: `None`
/// for «нет».
fn pick(ui: &mut egui::Ui, salt: (&str, &str), label: &str, current: Option<&String>, ids: &[String]) -> Option<Option<String>> {
    let mut out = None;
    ui.horizontal(|ui| {
        ui.label(RichText::new(label).size(12.0).color(MUTED));
        let shown = current.cloned().unwrap_or_else(|| "нет".to_string());
        egui::ComboBox::from_id_salt(salt).width(ui.available_width() - 8.0).selected_text(shown).show_ui(ui, |ui| {
            if ui.selectable_label(current.is_none(), "нет").clicked() && current.is_some() {
                out = Some(None);
            }
            for id in ids {
                if ui.selectable_label(current == Some(id), id).clicked() && current != Some(id) {
                    out = Some(Some(id.clone()));
                }
            }
        });
    });
    out
}
