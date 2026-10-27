//! The «Интерфейс» section: marking a piece of game UI and its nine-slice margins.

use crate::app::{App, fold};
use crate::style_ui::StyleEdit;
use crate::theme::{self, FAINT, FG, KeyState};
use egui::{RichText, vec2};
use motif_core::kurbo::Shape;
use motif_core::{DialDrag, Document, Node, Prop, ShapeKind, UiPart, UiRole};
use motif_script::fmt_num;

/// `slice` written as short as it reads: one value, two (sides, top-bottom) or four.
pub fn slice_text(s: [f64; 4]) -> String {
    let [l, t, r, b] = s;
    let v: Vec<f64> = if l == t && t == r && r == b {
        vec![l]
    } else if l == r && t == b {
        vec![l, t]
    } else {
        vec![l, t, r, b]
    };
    format!("slice {}", v.iter().map(|x| fmt_num(*x)).collect::<Vec<_>>().join(" "))
}

/// What a piece becomes in Godot, as the menu names it.
fn role_name(r: UiRole) -> &'static str {
    match r {
        UiRole::Auto => "Кнопка или панель",
        UiRole::Button => "Кнопка — Button",
        UiRole::Panel => "Панель — PanelContainer",
        UiRole::Progress => "Полоса — ProgressBar",
        UiRole::Check => "Флажок — CheckBox",
        UiRole::Slider => "Слайдер — HSlider/VSlider",
        UiRole::Scroll => "Прокрутка — ScrollBar",
        UiRole::Label => "Надпись — Label",
        UiRole::Switch => "Переключатель — CheckButton",
        UiRole::Input => "Поле ввода — LineEdit",
        UiRole::Option => "Список — OptionButton",
    }
}

fn part_name(p: UiPart) -> &'static str {
    match p {
        UiPart::Background => "Фон",
        UiPart::Fill => "Заливка",
        UiPart::Unchecked => "Выключен",
        UiPart::Checked => "Включён",
        UiPart::Track => "Дорожка",
        UiPart::Grabber => "Ползунок",
        UiPart::Text => "Подпись — шрифтом Godot",
        UiPart::Arrow => "Стрелка",
    }
}

/// What the parts of a kind of piece are for, under the menu.
fn role_hint(r: UiRole) -> &'static str {
    match r {
        UiRole::Auto => "С состояниями или подписью — кнопка, без них — панель.",
        UiRole::Button | UiRole::Panel => "Подпись (text с деталью «Подпись») не запекается: Godot пишет её шрифтом темы, цвет — по состояниям.",
        UiRole::Progress => "Внутри нужны детали «Фон» и «Заливка»; заливка растёт от начала полосы.",
        UiRole::Check => "Внутри нужны «Выключен» и «Включён» — картинки галочки; подпись — по желанию.",
        UiRole::Slider => {
            "Внутри нужны «Дорожка» и «Ползунок»; «Заливка» — пройденная часть, по желанию. Или любой рисунок и анимация Value: значение проигрывает её."
        }
        UiRole::Scroll => "Внутри нужны «Дорожка» и «Ползунок»; ползунок тянется вдоль полосы.",
        UiRole::Label => "Шрифт, размер, цвет, обводка и тень текста — в тему Label.",
        UiRole::Switch => "Как флажок, но подпись слева, а «Выключен» и «Включён» — картинки переключателя справа.",
        UiRole::Input => "Рамка поля; «Подпись» — подсказка: полупрозрачной, а набранный текст будет того же цвета без прозрачности. Focus — рамка при вводе.",
        UiRole::Option => "Как кнопка; «Стрелка» справа — значок списка, подпись — выбранный пункт.",
    }
}

/// Distance of `v` from the nearest whole number.
fn off_grid(v: f64) -> f64 {
    (v - v.round()).abs()
}

/// Shapes of piece `i` (it and what is inside) whose edges land between pixels at `scale`,
/// with the moves that put them on whole pixels: `(node, X/Y/Width/Height, new value)`. A
/// stroke straddles the edge, so with an odd stroke width the edge sits on a half pixel instead.
/// Only shapes that are not rotated or skewed are checked; paths only move, they keep their size.
pub fn crisp_edits(doc: &Document, i: usize, scale: f64) -> (usize, Vec<(String, Prop, f64)>) {
    let mut blurry = 0;
    let mut edits = Vec::new();
    for k in (0..doc.nodes.len()).filter(|&k| k == i || doc.is_inside(k, i)) {
        let n = &doc.nodes[k];
        if matches!(n.kind, ShapeKind::Group | ShapeKind::Bone | ShapeKind::Text) || !doc.is_visible(k) {
            continue;
        }
        let Some(path) = n.path() else { continue };
        // Only straight edges along the axes can be crisp: slanted lines are soft anyway.
        let straight = |path: &motif_core::kurbo::BezPath| {
            path.segments().all(|s| match s {
                motif_core::kurbo::PathSeg::Line(l) => (l.p0.x - l.p1.x).abs() < 1e-9 || (l.p0.y - l.p1.y).abs() < 1e-9,
                _ => true,
            })
        };
        if matches!(n.kind, ShapeKind::Polygon | ShapeKind::Star) || (n.kind == ShapeKind::Path && !straight(&path)) {
            continue;
        }
        let t = doc.world_transform(k).as_coeffs();
        let parent = n.parent.map_or([1.0, 0.0, 0.0, 1.0, 0.0, 0.0], |p| doc.world_transform(p).as_coeffs());
        if t[1].abs() > 1e-9 || t[2].abs() > 1e-9 || parent[1].abs() > 1e-9 || parent[2].abs() > 1e-9 || parent[0] == 0.0 || parent[3] == 0.0 {
            continue;
        }
        let b = doc.world_transform(k).transform_rect_bbox(path.bounding_box());
        let stroke = |a: f64| if n.stroke.is_some() && n.stroke_width > 0.0 { (n.stroke_width * a.abs() * scale / 2.0).fract() } else { 0.0 };
        let (ox, oy) = (stroke(t[0]), stroke(t[3]));
        let snap = |v: f64, o: f64| ((v * scale - o).round() + o) / scale;
        let edges = [(b.x0, ox), (b.y0, oy), (b.x1, ox), (b.y1, oy)];
        if edges.iter().all(|(v, o)| off_grid(v * scale - o) < 0.01) {
            continue;
        }
        blurry += 1;
        let sized = n.kind != ShapeKind::Path;
        let [x0, y0] = [snap(b.x0, ox), snap(b.y0, oy)];
        let (x1, y1) = if sized { (snap(b.x1, ox).max(x0 + 1.0 / scale), snap(b.y1, oy).max(y0 + 1.0 / scale)) } else { (x0 + b.width(), y0 + b.height()) };
        let (dx, dy) = (((x0 + x1) - (b.x0 + b.x1)) / 2.0 / parent[0], ((y0 + y1) - (b.y0 + b.y1)) / 2.0 / parent[3]);
        let round = |v: f64| (v * 1000.0).round() / 1000.0;
        if dx.abs() > 1e-6 {
            edits.push((n.id.clone(), Prop::X, round(n.x + dx)));
        }
        if dy.abs() > 1e-6 {
            edits.push((n.id.clone(), Prop::Y, round(n.y + dy)));
        }
        if sized && b.width() > 0.0 && b.height() > 0.0 {
            let (w, h) = (n.width * (x1 - x0) / b.width(), n.height * (y1 - y0) / b.height());
            if (w - n.width).abs() > 1e-6 {
                edits.push((n.id.clone(), Prop::Width, round(w)));
            }
            if (h - n.height).abs() > 1e-6 {
                edits.push((n.id.clone(), Prop::Height, round(h)));
            }
        }
    }
    (blurry, edits)
}

/// What the Interface section asks the app to do.
#[derive(Default)]
pub struct KitAction {
    pub add_states: bool,
    pub add_value: bool,
    pub stretch: bool,
    pub crisp: bool,
}

/// How a dial is dragged, in words.
fn drag_name(d: DialDrag) -> &'static str {
    match d {
        DialDrag::Auto => "по форме",
        DialDrag::Across => "вбок",
        DialDrag::Up => "вверх",
        DialDrag::Turn => "по кругу",
    }
}

impl App {
    /// A slider drawn by its Value animation: which way the mouse moves it, the arc of a turn,
    /// where it stands. Without the animation, a button that adds it.
    fn dial_rows(&self, ui: &mut egui::Ui, node: &Node, half: f32, styles: &mut Vec<StyleEdit>, act: &mut KitAction) {
        let doc = &self.scene.doc;
        let Some(i) = doc.index_of(&node.id) else { return };
        if !motif_core::is_dial(doc, i) {
            if doc.animations.iter().all(|a| !a.name.eq_ignore_ascii_case("value"))
                && theme::button(ui, "Нарисовать анимацией Value", theme::ButtonKind::Normal)
                    .on_hover_text("Пустая анимация Value: её начало — значение 0, конец — 1. Любой рисунок, ключи в Animate.")
                    .clicked()
            {
                act.add_value = true;
            }
            return;
        }
        ui.label(RichText::new("Рисует анимация Value: значение от 0 до 1 проигрывает её от начала до конца.").size(11.5).color(FAINT));
        let run = motif_core::dial_run(doc, i);
        let dial = node.dial;
        let write = |drag: DialDrag, arc: f64| match (drag, arc == 270.0) {
            (DialDrag::Auto, true) => None,
            (DialDrag::Turn, false) | (DialDrag::Auto, false) => Some(format!("dial turn {}", fmt_num(arc))),
            (d, _) => Some(format!("dial {}", d.keyword())),
        };
        ui.horizontal(|ui| {
            ui.label(RichText::new("Тянуть").size(12.5).color(FG));
            let shown = match (dial.drag, run) {
                (DialDrag::Auto, Some(r)) => format!("{} — {}", drag_name(DialDrag::Auto), drag_name(r.drag)),
                (d, _) => drag_name(d).to_string(),
            };
            egui::ComboBox::from_id_salt(("dial-drag", &node.id)).width(ui.available_width() - 8.0).selected_text(shown).show_ui(ui, |ui| {
                for d in DialDrag::ALL {
                    if ui.selectable_label(dial.drag == d, drag_name(d)).clicked() && d != dial.drag {
                        styles.push(("dial", write(d, if d == DialDrag::Turn { dial.arc } else { 270.0 })));
                    }
                }
            });
        });
        ui.horizontal(|ui| {
            if run.is_some_and(|r| r.drag == DialDrag::Turn)
                && let Some(v) = theme::num_field(ui, half, "Дуга°", dial.arc, 1.0, 10.0..=360.0, 0, KeyState::Hidden).changed
            {
                styles.push(("dial", write(DialDrag::Turn, v.round())));
            }
            if let Some(v) = theme::num_field(ui, half, "Значение", dial.value, 0.01, 0.0..=1.0, 2, KeyState::Hidden).changed {
                let v = (v * 100.0).round() / 100.0;
                styles.push(("value", (v != 0.0).then(|| format!("value {}", fmt_num(v)))));
            }
        });
    }

    /// Both flags are written through `styles`: `ui` alone makes a fixed-size piece,
    /// `slice` a stretchable one. Returns true when the states button was pressed.
    pub(crate) fn kit_section(&self, ui: &mut egui::Ui, node: &Node, half: f32, styles: &mut Vec<StyleEdit>) -> KitAction {
        let mut act = KitAction::default();
        if node.kind == ShapeKind::Bone {
            return act;
        }
        let missing: Vec<&str> = ["Hover", "Press"].into_iter().filter(|n| self.scene.doc.animation(n).is_none()).collect();
        // The piece this node is inside, if any: then it can be one of its parts.
        let doc = &self.scene.doc;
        let mut up = node.parent;
        while let Some(k) = up.filter(|k| !doc.nodes[*k].ui) {
            up = doc.nodes[k].parent;
        }
        let owner = up.map(|k| &doc.nodes[k]);
        fold(ui, "Интерфейс", |ui| {
            ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
            if let Some(o) = owner.filter(|o| !o.ui_role.parts().is_empty() || node.part.is_some()) {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Деталь").size(12.5).color(FG));
                    egui::ComboBox::from_id_salt(("ui-part", &node.id))
                        .width(ui.available_width() - 8.0)
                        .selected_text(node.part.map_or("нет", part_name))
                        .show_ui(ui, |ui| {
                            if ui.selectable_label(node.part.is_none(), "нет").clicked() {
                                styles.push(("part", None));
                            }
                            for &p in o.ui_role.parts() {
                                if ui.selectable_label(node.part == Some(p), part_name(p)).clicked() {
                                    styles.push(("part", Some(format!("part {}", p.keyword()))));
                                }
                            }
                        });
                });
                if let Some(p) = node.part {
                    let whose = o.name.as_deref().unwrap_or(&o.id);
                    let hint = if p == UiPart::Text {
                        format!("Подпись «{whose}» не запекается: Godot пишет её шрифтом из темы, цвет — из каждого состояния.")
                    } else {
                        format!("Часть «{whose}»: снимается отдельной картинкой.")
                    };
                    ui.label(RichText::new(hint).size(11.5).color(FAINT));
                    return;
                }
            }
            let mut on = node.ui || node.slice.is_some();
            if ui.checkbox(&mut on, RichText::new("Деталь интерфейса").size(12.5).color(FG)).changed() {
                styles.push(("ui", on.then(|| "ui".to_string())));
                if !on {
                    styles.push(("slice", None));
                }
            }
            if !on {
                ui.label(
                    RichText::new("Кнопки, панели, рамки: экспорт «UI-набор для Godot» снимет каждую деталь отдельной картинкой в каждом состоянии.")
                        .size(11.5)
                        .color(FAINT),
                );
                return;
            }
            ui.horizontal(|ui| {
                ui.label(RichText::new("В Godot").size(12.5).color(FG));
                egui::ComboBox::from_id_salt(("ui-role", &node.id)).width(ui.available_width() - 8.0).selected_text(role_name(node.ui_role)).show_ui(
                    ui,
                    |ui| {
                        for r in UiRole::ALL {
                            if ui.selectable_label(node.ui_role == r, role_name(r)).clicked() && r != node.ui_role {
                                let word = r.keyword();
                                styles.push(("ui", Some(if word.is_empty() { "ui".to_string() } else { format!("ui {word}") })));
                            }
                        }
                    },
                );
            });
            ui.label(RichText::new(role_hint(node.ui_role)).size(11.5).color(FAINT));
            if node.ui_role == UiRole::Slider {
                self.dial_rows(ui, node, half, styles, &mut act);
            }
            // A dial is drawn frame by frame at its own size: it does not stretch.
            let dial = self.scene.doc.index_of(&node.id).is_some_and(|i| motif_core::is_dial(&self.scene.doc, i));
            let mut stretch = node.slice.is_some();
            if !dial && ui.checkbox(&mut stretch, RichText::new("Тянется (9-slice)").size(12.5).color(FG)).changed() {
                // A quarter of the shorter side; groups have no size of their own.
                let side = node.width.abs().min(node.height.abs());
                let edge = if side >= 1.0 { (side / 4.0).round().clamp(4.0, 32.0) } else { 12.0 };
                styles.push(("slice", stretch.then(|| slice_text([edge; 4]))));
            }
            if let Some(s) = node.slice {
                let mut out = s;
                let mut changed = false;
                for row in [[(0, "Слева"), (2, "Справа")], [(1, "Сверху"), (3, "Снизу")]] {
                    ui.horizontal(|ui| {
                        for (k, label) in row {
                            if let Some(v) = theme::num_field(ui, half, label, s[k], 0.5, 0.0..=10_000.0, 0, KeyState::Hidden).changed {
                                out[k] = v.round();
                                changed = true;
                            }
                        }
                    });
                }
                if changed {
                    styles.push(("slice", Some(slice_text(out))));
                }
            }
            if !missing.is_empty()
                && theme::button(ui, &format!("Добавить состояния {}", missing.join(" и ")), theme::ButtonKind::Normal)
                    .on_hover_text("Пустые анимации состояний: в Animate меняйте свойства, последний кадр станет картинкой состояния")
                    .clicked()
            {
                act.add_states = true;
            }
            if !dial
                && theme::button(ui, "Проверить растяжение…", theme::ButtonKind::Normal)
                    .on_hover_text("Деталь, как её растянет Godot: тяните уголок, наводите и нажимайте мышью")
                    .clicked()
            {
                act.stretch = true;
            }
            // Edges between pixels come out soft; whole pixels keep thin lines crisp.
            let scale = self.export.scale;
            if let Some(i) = self.scene.doc.index_of(&node.id) {
                let (blurry, edits) = crisp_edits(&self.scene.doc, i, scale);
                let at = format!("при масштабе ×{}", fmt_num(scale));
                if blurry == 0 {
                    ui.label(RichText::new(format!("Края по пикселям {at}: чётко.")).size(11.5).color(theme::GOOD));
                } else {
                    ui.label(RichText::new(format!("Края между пикселями {at}: фигур {blurry} — выйдут размытыми.")).size(11.5).color(theme::ACCENT));
                    if !edits.is_empty()
                        && theme::button(ui, "Выровнять по пикселям", theme::ButtonKind::Normal)
                            .on_hover_text("Сдвигает и чуть меняет размер фигур, чтобы края и обводки легли на целые пиксели экспорта")
                            .clicked()
                    {
                        act.crisp = true;
                    }
                }
            }
            ui.label(
                RichText::new(
                    "Поля 9-slice не растягиваются: углы остаются как нарисованы, середина тянется. Состояния берутся из анимаций Hover, Press, Disabled, Focus — их последний кадр.",
                )
                .size(11.5)
                .color(FAINT),
            );
        });
        act
    }
}

#[cfg(test)]
mod tests {
    use super::{crisp_edits, slice_text};
    use motif_core::Prop;

    #[test]
    fn the_ui_kit_template_is_crisp() {
        let src = include_str!("../../../examples/ui-kit.msc");
        for scene in motif_script::parse_boards(src, None).unwrap() {
            let doc = &scene.doc;
            for (i, n) in doc.nodes.iter().enumerate().filter(|(_, n)| n.ui || n.slice.is_some()) {
                for scale in [1.0, 2.0] {
                    assert_eq!(crisp_edits(doc, i, scale).0, 0, "«{}» ×{scale}", n.id);
                }
            }
        }
    }

    #[test]
    fn edges_go_onto_whole_pixels() {
        let src = "group b at 50.25 40 ui {\n  rect body size 30.5 20 fill #ffffff\n  rect frame size 40 20 fill none stroke #000000 1\n}\n";
        let doc = motif_script::parse(src).unwrap().doc;
        let i = doc.index_of("b").unwrap();
        let (blurry, edits) = crisp_edits(&doc, i, 1.0);
        assert_eq!(blurry, 2);
        // body: 35..65.5 → 35..66, wide 31, centre +0.25 inside the group. frame: a 1 px stroke
        // is crisp around half pixels, so 30.25 → 30.5 and 30 → 30.5.
        assert_eq!(
            edits,
            [
                ("body".to_string(), Prop::X, 0.25),
                ("body".to_string(), Prop::Width, 31.0),
                ("frame".to_string(), Prop::X, 0.25),
                ("frame".to_string(), Prop::Y, 0.5),
            ]
        );
        // At 2x the body's half pixel is whole; the frame's quarter is not.
        let (blurry, edits) = crisp_edits(&doc, i, 2.0);
        assert_eq!(blurry, 1);
        assert_eq!(edits, [("frame".to_string(), Prop::X, 0.25)]);
    }

    #[test]
    fn slice_is_written_short() {
        assert_eq!(slice_text([8.0; 4]), "slice 8");
        assert_eq!(slice_text([8.0, 4.0, 8.0, 4.0]), "slice 8 4");
        assert_eq!(slice_text([1.0, 2.0, 3.0, 4.0]), "slice 1 2 3 4");
    }
}
