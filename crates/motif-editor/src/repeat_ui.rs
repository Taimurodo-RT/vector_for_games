//! Live repeats in the editor: the section «Повтор» of the inspector, the buttons that put an
//! object into a group that repeats it, and the grips of a grid on the canvas — one pair for
//! the step, one for how many columns and rows there are. Under the numbers of a repeat stands
//! «Разброс»: how much the repeats differ from the written one, and the seed it is counted from.

use crate::app::{App, Tool, fold};
use crate::canvas::View;
use crate::theme::{self, FAINT, FG, KeyState, MUTED, SEL};
use egui::{Color32, Pos2, Rect, RichText, Sense, Stroke, StrokeKind, vec2};
use motif_core::kurbo::{Point, Rect as KRect, Vec2};
use motif_core::{Color, Document, Node, Prop, ShapeKind};
use motif_script::{RepeatKind, Vary, VaryFill, fmt_num};

/// How close the pointer must be to a grip to take it, in screen points.
const GRAB: f32 = 7.0;
/// How far from the first repeat the line of grips runs, in screen points.
const AWAY: f32 = 16.0;
/// The line over the first repeat runs higher: the knob that turns the frame stands there.
const ABOVE: f32 = 40.0;
/// The most repeats one group makes (the script refuses more).
const MOST: f64 = 2000.0;

/// What a grip of a grid changes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Grip {
    StepX,
    StepY,
    Cols,
    Rows,
}

impl Grip {
    fn across(self) -> bool {
        matches!(self, Grip::StepX | Grip::Cols)
    }

    fn step(self) -> bool {
        matches!(self, Grip::StepX | Grip::StepY)
    }

    fn note(self, held: bool) -> &'static str {
        match (self, held) {
            (Grip::StepX, false) => "Шаг вправо: тяните",
            (Grip::StepY, false) => "Шаг вниз: тяните",
            (Grip::Cols, false) => "Столбцы: тяните, чтобы добавить или убрать",
            (Grip::Rows, false) => "Строки: тяните, чтобы добавить или убрать",
            (Grip::StepX | Grip::StepY, true) => "Шаг сетки",
            (Grip::Cols, true) => "Столбцы",
            (Grip::Rows, true) => "Строки",
        }
    }
}

/// A grip of a grid being dragged.
pub(crate) struct RepeatDrag {
    node: String,
    grip: Grip,
}

/// The grid of the selected group as the canvas shows it.
struct Grips {
    node: usize,
    cols: u32,
    rows: u32,
    step: (f64, f64),
    /// The middle of the first repeat, in the space of the group.
    middle: Point,
    /// Each grip with its place on the screen, and where its line starts.
    marks: Vec<(Grip, Pos2, Pos2)>,
    /// From the first repeat to the start of each line: over it, then beside it.
    stems: [(Pos2, Pos2); 2],
}

/// What the section asks for.
enum Ask {
    Set(Option<RepeatKind>),
    /// Another kind for the group, by its place in the switch: 0 is no repeat.
    Kind(usize),
    Wrap(usize),
    Select(String),
    /// Another variation of the repeats. The word names what was changed: one number being
    /// scrubbed is one undo step. `false` for a press of a button, a step of its own.
    Vary(Vary, &'static str, bool),
}

/// «Разброс»: how the repeats differ from the written one. Returns the variation when something
/// changed, with a word for what it was and whether it is a value being dragged.
fn vary_ui(ui: &mut egui::Ui, vary: &Vary, half: f32) -> Option<(Vary, &'static str, bool)> {
    let mut out = None;
    ui.add_space(4.0);
    ui.label(RichText::new("Разброс").size(12.0).color(MUTED));
    ui.horizontal(|ui| {
        if let Some(v) = theme::num_field(ui, half, "Сдвиг X", vary.at.0, 0.25, 0.0..=10_000.0, 1, KeyState::Hidden).changed {
            out = Some((Vary { at: (v, vary.at.1), ..vary.clone() }, "vary-x", true));
        }
        if let Some(v) = theme::num_field(ui, half, "Сдвиг Y", vary.at.1, 0.25, 0.0..=10_000.0, 1, KeyState::Hidden).changed {
            out = Some((Vary { at: (vary.at.0, v), ..vary.clone() }, "vary-y", true));
        }
    });
    ui.horizontal(|ui| {
        if let Some(v) = theme::num_field(ui, half, "Поворот °", vary.rot, 0.5, 0.0..=180.0, 1, KeyState::Hidden).changed {
            out = Some((Vary { rot: v, ..vary.clone() }, "vary-rot", true));
        }
        let most = Vary::MAX_SCALE * 100.0;
        if let Some(v) = theme::num_field(ui, half, "Размер %", (vary.scale * 100.0).round(), 0.5, 0.0..=most, 0, KeyState::Hidden).changed {
            out = Some((Vary { scale: v.round().clamp(0.0, most) / 100.0, ..vary.clone() }, "vary-scale", true));
        }
    });
    ui.horizontal(|ui| {
        // The colour the fills are mixed with. Until one is chosen it is black: darker shades.
        let now = vary.fill.clone().unwrap_or(VaryFill { text: "#000000".into(), color: Color::BLACK, amount: 0.0 });
        let (sw, resp) = ui.allocate_exact_size(vec2(28.0, 28.0), Sense::click());
        let mut c32 = Color32::from_rgb(now.color.r, now.color.g, now.color.b);
        ui.painter().rect_filled(sw, 6.0, c32);
        ui.painter().rect_stroke(sw, 6.0, Stroke::new(1.0, theme::LINE), StrokeKind::Inside);
        let resp = resp.on_hover_text(format!("{} · с этим цветом смешиваются заливки повторов. Клик — изменить", now.text));
        theme::color_popup(&resp).show(|ui| {
            if egui::color_picker::color_picker_color32(ui, &mut c32, egui::color_picker::Alpha::Opaque) {
                let color = Color::rgb(c32.r(), c32.g(), c32.b());
                // A colour chosen with nothing of it mixed in would show nothing.
                let amount = if now.amount == 0.0 { 0.3 } else { now.amount };
                out = Some((Vary { fill: Some(VaryFill { text: color.to_hex(), color, amount }), ..vary.clone() }, "vary-fill", true));
            }
        });
        let w = half * 2.0 - 28.0;
        if let Some(v) = theme::num_field(ui, w, "Цвет %", (now.amount * 100.0).round(), 0.5, 0.0..=100.0, 0, KeyState::Hidden).changed {
            let fill = VaryFill { amount: v.round().clamp(0.0, 100.0) / 100.0, ..now.clone() };
            out = Some((Vary { fill: Some(fill), ..vary.clone() }, "vary-amount", true));
        }
    });
    // The seed counts something only when something varies.
    ui.add_enabled_ui(!vary.is_none(), |ui| {
        ui.horizontal(|ui| {
            let most = f64::from(Vary::MAX_SEED);
            if let Some(v) = theme::num_field(ui, half, "Зерно", f64::from(vary.seed), 0.05, 0.0..=most, 0, KeyState::Hidden).changed {
                out = Some((Vary { seed: v.round().clamp(0.0, most) as u32, ..vary.clone() }, "vary-seed", true));
            }
            if theme::button(ui, "Другой вариант", theme::ButtonKind::Normal)
                .on_hover_text("Следующее зерно: те же пределы, другой рисунок. Прежний вернёт Ctrl+Z или прежнее число")
                .clicked()
            {
                out = Some((Vary { seed: if vary.seed >= Vary::MAX_SEED { 0 } else { vary.seed + 1 }, ..vary.clone() }, "vary-next", false));
            }
        });
    });
    let hint = if vary.is_none() {
        "Повторы точные. Задайте, на сколько самое большее каждый может сдвинуться, повернуться, изменить размер или цвет заливки: камни, трава, заклёпки станут живее."
    } else {
        "Первый повтор остаётся таким, как нарисован, остальные отличаются от него в этих пределах. Одно и то же зерно всегда даёт один и тот же рисунок."
    };
    ui.label(RichText::new(hint).size(11.5).color(FAINT));
    out
}

/// The box of what is written in the group `i`, without the repeats made from it, in the space
/// of the group.
fn first_repeat(doc: &Document, i: usize, clones: &[String]) -> Option<KRect> {
    let inv = doc.world_transform(i).inverse();
    doc.children(i)
        .filter(|&k| !clones.contains(&doc.nodes[k].id))
        .filter_map(|k| crate::app::node_bounds(doc, k).map(|b| (inv * doc.world_transform(k)).transform_rect_bbox(b)))
        .reduce(|a, b| a.union(b))
}

impl App {
    /// The group of a repeat that `id` belongs to as a made repeat, when it does: the object is
    /// one of the repeats or lies in one.
    pub(crate) fn repeat_of(&self, id: &str) -> Option<&str> {
        let doc = &self.scene.doc;
        let i = doc.index_of(id)?;
        self.scene
            .repeats
            .iter()
            .find_map(|(group, site)| site.clones.iter().filter_map(|c| doc.index_of(c)).any(|c| c == i || doc.is_inside(i, c)).then_some(group.as_str()))
    }

    /// Why the place of `node` is not the editor's to change, when it is not: a repeat along an
    /// outline stands where the outline puts it.
    pub(crate) fn placed_by_outline(&self, node: &str, prop: Prop) -> Option<String> {
        let placed = matches!(prop, Prop::X | Prop::Y | Prop::Rotation | Prop::ScaleX | Prop::ScaleY | Prop::SkewX | Prop::SkewY);
        match self.scene.repeats.get(node).map(|site| &site.kind) {
            Some(RepeatKind::Along { path, .. }) if placed => {
                Some(format!("Повтор «{node}» стоит вдоль контура «{path}»: двигайте и меняйте контур, повторы пойдут следом"))
            }
            _ => None,
        }
    }

    /// The id of an object as a line of the script names it: inside the copy a symbol is edited
    /// through, the objects of the symbol are written without the id of the copy.
    fn written_id(&self, id: &str) -> String {
        match self.scene.editing.as_deref().and_then(|copy| id.strip_prefix(copy)).and_then(|rest| rest.strip_prefix('-')) {
            Some(part) => part.to_string(),
            None => id.to_string(),
        }
    }

    /// The shapes a repeat of `id` can go along: everything with an outline that is not the
    /// object itself or inside it. Each with the id to write.
    fn outlines_for(&self, id: &str) -> Vec<(usize, String)> {
        let doc = &self.scene.doc;
        let Some(i) = doc.index_of(id) else { return Vec::new() };
        (0..doc.nodes.len())
            .filter(|&k| k != i && !doc.is_inside(k, i) && !doc.nodes[k].kind.is_container() && doc.nodes[k].kind != ShapeKind::Bone)
            .filter(|&k| self.repeat_of(&doc.nodes[k].id).is_none() && doc.nodes[k].path().is_some())
            .map(|k| (k, self.written_id(&doc.nodes[k].id)))
            .collect()
    }

    /// The outline to start with: the smallest shape the object lies on, else the nearest one.
    fn nearest_outline(&self, id: &str) -> Option<String> {
        let doc = &self.scene.doc;
        let at = doc.index_of(id).and_then(|i| crate::selection::world_bounds(doc, i))?.center();
        let boxes: Vec<(KRect, String)> =
            self.outlines_for(id).into_iter().filter_map(|(k, name)| crate::selection::world_bounds(doc, k).map(|b| (b, name))).collect();
        let under = boxes.iter().filter(|(b, _)| b.contains(at)).min_by(|a, b| a.0.area().total_cmp(&b.0.area()));
        under.or_else(|| boxes.iter().min_by(|a, b| a.0.center().distance(at).total_cmp(&b.0.center().distance(at)))).map(|(_, name)| name.clone())
    }

    /// A repeat to start with for the object `id`: `which` is 1 for a grid, 2 for a circle, 3
    /// for an outline, 4 for a mirror. With it, where the centre of a circle or the lines of a
    /// mirror are from the middle of the object: a circle has its centre below the object, a
    /// mirror reflects in its right and its lower side.
    fn first_kind(&self, id: &str, which: usize) -> Result<(RepeatKind, Vec2), String> {
        let doc = &self.scene.doc;
        // The size of what is repeated: of a group that repeats already, its first repeat.
        let first = doc.index_of(id).and_then(|i| match self.scene.repeats.get(id) {
            Some(site) => first_repeat(doc, i, &site.clones),
            None => crate::app::node_bounds(doc, i),
        });
        let size = first.map_or((40.0, 40.0), |b| (b.width(), b.height()));
        let room = |v: f64| (v * 1.25).ceil().max(4.0);
        Ok(match which {
            1 => (RepeatKind::Grid { cols: 3, rows: 1, step: (room(size.0), room(size.1)) }, Vec2::ZERO),
            2 => (RepeatKind::Circle { count: 6, arc: 360.0, upright: false, mirror: false }, Vec2::new(0.0, (size.0.max(size.1) * 1.5).ceil().max(8.0))),
            3 => {
                let path = self.nearest_outline(id).ok_or("нет фигуры, вдоль которой идти: нарисуйте прямоугольник, круг или контур")?;
                (RepeatKind::Along { path, count: 8, offset: 0.0, upright: false }, Vec2::ZERO)
            }
            _ => (RepeatKind::Mirror { x: true, y: false }, Vec2::new(size.0 / 2.0, size.1 / 2.0)),
        })
    }

    /// The selected objects go into a new group that repeats them.
    pub(crate) fn repeat_selection(&mut self, which: usize) {
        if !self.can_edit() {
            return;
        }
        let ids = self.selection();
        let Some(first) = ids.first() else { return };
        let made = self
            .first_kind(first, which)
            .and_then(|(kind, centre)| motif_script::repeat_nodes(&self.src, &self.scene, &ids, &kind, centre, self.base_dir().as_deref()));
        match made {
            Ok((out, group)) => {
                if self.apply_once("repeat", Some(out)) {
                    self.point_edit = None;
                    self.select_only(Some(group.clone()));
                    self.status = match which {
                        1 => format!(
                            "Повтор сеткой «{group}»: шаг и число — в панели и ручками у группы. Первый объект остался вашим: меняете его — меняются все"
                        ),
                        2 => format!("Повтор по кругу «{group}»: центр — опорная точка группы (клавиша Y и перетащите метку), число — в панели"),
                        3 => format!("Повтор вдоль контура «{group}»: меняете контур — повторы идут следом. Число и отступ — в панели"),
                        _ => format!("Зеркало «{group}»: ось идёт по правому краю объекта. Сдвинуть её — клавиша Y и перетащите метку; вторая ось — в панели"),
                    };
                }
            }
            Err(e) => self.status = format!("Не повторить: {e}"),
        }
    }

    /// Another repeat for the group `id`, or none. `live` joins the steps of one drag or of one
    /// number being scrubbed into one undo step.
    pub(crate) fn set_repeat(&mut self, id: &str, kind: Option<RepeatKind>, live: bool) -> bool {
        self.write_repeat(id, kind, None, live)
    }

    /// [`Self::set_repeat`], with the pivot a circle starts around when the group has none.
    fn write_repeat(&mut self, id: &str, kind: Option<RepeatKind>, pivot: Option<Vec2>, live: bool) -> bool {
        if !self.can_edit() {
            return false;
        }
        let out = match (&kind, pivot) {
            (Some(kind), Some(pivot)) => motif_script::set_repeat_around(&self.src, &self.scene, id, kind, pivot),
            _ => motif_script::set_repeat(&self.src, &self.scene, id, kind.as_ref()),
        };
        let Some(out) = out else {
            return false;
        };
        let (before, after) = (self.scene.repeats.get(id).map_or(1, |r| r.kind.count()), kind.as_ref().map_or(1, RepeatKind::count));
        let changed = if live { self.apply("repeat", Some(out)) } else { self.apply_once("repeat", Some(out)) };
        if changed {
            self.status = match &kind {
                None => format!("«{id}» снова обычная группа: в ней остался первый набор объектов"),
                Some(_) if after < before => {
                    format!("Повторов стало {after}. Что было сказано про убранные (ключи, правки в override), ушло вместе с ними; Ctrl+Z вернёт")
                }
                Some(kind) => format!("Повтор «{id}»: {}", kind.words()),
            };
        }
        changed
    }

    /// Another variation for the repeats of the group `id`. `live` joins the steps of one number
    /// being scrubbed, named by `what`, into one undo step.
    pub(crate) fn set_vary(&mut self, id: &str, vary: &Vary, what: &str, live: bool) -> bool {
        if !self.can_edit() {
            return false;
        }
        let Some(out) = motif_script::set_vary(&self.src, &self.scene, id, vary) else {
            return false;
        };
        let changed = if live { self.apply(what, Some(out)) } else { self.apply_once(what, Some(out)) };
        if changed {
            self.status = if vary.is_none() {
                format!("Повторы «{id}» снова точные: каждый такой же, как первый")
            } else if what == "vary-next" {
                format!("Другой вариант «{id}»: зерно {}. Прежний вернёт Ctrl+Z", vary.seed)
            } else {
                format!("Разброс «{id}»:{}", vary.words())
            };
        }
        changed
    }

    /// Another kind of repeat for the group `id`, with numbers to start with: `which` is 0 for
    /// none, 1 for a grid, 2 for a circle, 3 for an outline, 4 for a mirror.
    pub(crate) fn switch_repeat(&mut self, id: &str, which: usize) {
        if which == 0 {
            self.set_repeat(id, None, false);
            return;
        }
        match self.first_kind(id, which) {
            Ok((kind, centre)) => {
                // A group keeps its pivot: the circle goes around it. A group without one, with
                // its objects on its own point, would turn them in place: the centre goes below
                // them, as for an object that is repeated by a button. The same for a mirror:
                // a line through the middle of the objects would lay the reflection over them.
                let doc = &self.scene.doc;
                let clones = self.scene.repeats.get(id).map_or(&[][..], |s| &s.clones);
                let pivot = doc
                    .index_of(id)
                    .filter(|&i| matches!(which, 2 | 4) && doc.nodes[i].pivot == Vec2::ZERO)
                    .and_then(|i| first_repeat(doc, i, clones))
                    .filter(|b| if which == 2 { b.inflate(0.5, 0.5).contains(Point::ZERO) } else { b.x0 < -0.5 && b.x1 > 0.5 })
                    .map(|b| b.center().to_vec2() + centre);
                self.write_repeat(id, Some(kind), pivot, false);
            }
            Err(e) => self.status = format!("Не повторить: {e}"),
        }
    }

    /// The section of the inspector.
    pub(crate) fn repeat_ui(&mut self, ui: &mut egui::Ui, node: &Node, half: f32) {
        if node.kind == ShapeKind::Bone || !self.also.is_empty() {
            return;
        }
        let written = self.scene.sites.get(&node.id).is_some_and(|s| !s.generated);
        let site = self.scene.repeats.get(&node.id).cloned();
        let made_by = self.repeat_of(&node.id).map(str::to_string);
        // A plain group repeats itself; anything else goes into a new group that repeats it.
        let own = node.kind == ShapeKind::Group
            && written
            && !self.scene.instances.contains_key(&node.id)
            && !node.clip
            && node.stack.is_none()
            && node.combine.is_none();
        if !written && made_by.is_none() {
            return;
        }
        let outlines = if matches!(site.as_ref().map(|s| &s.kind), Some(RepeatKind::Along { .. })) { self.outlines_for(&node.id) } else { Vec::new() };
        let mut ask = None;
        fold(ui, "Повтор", |ui| {
            ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
            if let Some(group) = &made_by {
                ui.label(
                    RichText::new(format!("Один из повторов группы «{group}». Он сделан из первого набора её объектов: меняйте тот, изменятся все."))
                        .size(11.5)
                        .color(FG),
                );
                if theme::button(ui, "К группе повтора", theme::ButtonKind::Normal).on_hover_text("Выбрать группу: число повторов, шаг и вид").clicked()
                {
                    ask = Some(Ask::Select(group.clone()));
                }
                return;
            }
            if !own {
                ui.label(RichText::new("Повторить объект").size(12.0).color(MUTED));
                ui.horizontal_wrapped(|ui| {
                    for (which, text, tip) in [
                        (1, "Сеткой", "Ряды и столбцы с шагом: заклёпки в ряд, плитки, точки"),
                        (2, "По кругу", "Вокруг точки: спицы, лепестки, деления шкалы"),
                        (3, "Вдоль контура", "Равномерно по краю другой фигуры: заклёпки по рамке панели"),
                        (4, "Зеркально", "Вторая половина отражением: герб, лицо, крылья. Рисуете одну сторону"),
                    ] {
                        if theme::button(ui, text, theme::ButtonKind::Normal).on_hover_text(tip).clicked() {
                            ask = Some(Ask::Wrap(which));
                        }
                    }
                });
                ui.label(
                    RichText::new("Объект ляжет в новую группу, она покажет его несколько раз. Меняете его — меняются все повторы.").size(11.5).color(FAINT),
                );
                return;
            }
            let now = match site.as_ref().map(|s| &s.kind) {
                None => 0,
                Some(RepeatKind::Grid { .. }) => 1,
                Some(RepeatKind::Circle { .. }) => 2,
                Some(RepeatKind::Along { .. }) => 3,
                Some(RepeatKind::Mirror { .. }) => 4,
            };
            if site.as_ref().is_some_and(|s| !s.editable) {
                ui.label(
                    RichText::new(format!(
                        "{}. Повтор записан именами или формулами: его меняют в скрипте.",
                        site.as_ref().map_or(String::new(), |s| format!("{}{}", s.kind.words(), s.vary.words()))
                    ))
                    .size(11.5)
                    .color(FG),
                );
                return;
            }
            // Five kinds do not fit the panel in one row: the mirror stands under the others.
            if let Some(k) = theme::segmented(ui, &["Нет", "Сетка", "Круг", "Контур"], now, false)
                && k != now
            {
                ask = Some(Ask::Kind(k));
            }
            if theme::segmented(ui, &["Зеркало"], if now == 4 { 0 } else { usize::MAX }, false).is_some() && now != 4 {
                ask = Some(Ask::Kind(4));
            }
            // A whole number in a field that scrubs.
            let whole = |ui: &mut egui::Ui, label: &str, v: u32, most: f64| {
                theme::num_field(ui, half, label, f64::from(v), 0.05, 1.0..=most, 0, KeyState::Hidden).changed.map(|n| n.round().clamp(1.0, most) as u32)
            };
            match site.as_ref().map(|s| s.kind.clone()) {
                None => {
                    ui.label(
                        RichText::new(
                            "Группа покажет то, что в ней лежит, несколько раз: сеткой, по кругу, вдоль контура другой фигуры или отражением в зеркале.",
                        )
                        .size(11.5)
                        .color(FAINT),
                    );
                }
                Some(RepeatKind::Grid { cols, rows, step }) => {
                    ui.horizontal(|ui| {
                        if let Some(n) = whole(ui, "Столбцы", cols, (MOST / f64::from(rows)).floor()) {
                            ask = Some(Ask::Set(Some(RepeatKind::Grid { cols: n, rows, step })));
                        }
                        if let Some(n) = whole(ui, "Строки", rows, (MOST / f64::from(cols)).floor()) {
                            ask = Some(Ask::Set(Some(RepeatKind::Grid { cols, rows: n, step })));
                        }
                    });
                    ui.horizontal(|ui| {
                        if let Some(v) = theme::num_field(ui, half, "Шаг X", step.0, 0.5, -10_000.0..=10_000.0, 1, KeyState::Hidden).changed {
                            ask = Some(Ask::Set(Some(RepeatKind::Grid { cols, rows, step: (v, step.1) })));
                        }
                        if let Some(v) = theme::num_field(ui, half, "Шаг Y", step.1, 0.5, -10_000.0..=10_000.0, 1, KeyState::Hidden).changed {
                            ask = Some(Ask::Set(Some(RepeatKind::Grid { cols, rows, step: (step.0, v) })));
                        }
                    });
                    ui.label(
                        RichText::new("Шаг и число можно тянуть ручками у группы на холсте: ромб — шаг, кружок — сколько столбцов или строк.")
                            .size(11.5)
                            .color(FAINT),
                    );
                }
                Some(RepeatKind::Circle { count, arc, upright, mirror }) => {
                    ui.horizontal(|ui| {
                        if let Some(n) = whole(ui, "Число", count, if mirror { MOST / 2.0 } else { MOST }) {
                            ask = Some(Ask::Set(Some(RepeatKind::Circle { count: n, arc, upright, mirror })));
                        }
                        if let Some(v) = theme::num_field(ui, half, "Дуга °", arc, 1.0, -360.0..=360.0, 1, KeyState::Hidden).changed
                            && v != 0.0
                        {
                            ask = Some(Ask::Set(Some(RepeatKind::Circle { count, arc: v, upright, mirror })));
                        }
                    });
                    // A reflection turns with its repeat: the two do not go together.
                    let mut keep = upright;
                    if ui.checkbox(&mut keep, RichText::new("Не поворачивать повторы").size(12.5).color(FG)).changed() {
                        ask = Some(Ask::Set(Some(RepeatKind::Circle { count, arc, upright: keep, mirror: mirror && !keep })));
                    }
                    let mut both = mirror;
                    if ui
                        .checkbox(&mut both, RichText::new("Отражать каждый повтор").size(12.5).color(FG))
                        .on_hover_text("Рядом с каждым повтором встаёт его отражение в оси через центр: из половины лепестка выходит целый цветок")
                        .changed()
                        && (!both || count <= (MOST / 2.0) as u32)
                    {
                        ask = Some(Ask::Set(Some(RepeatKind::Circle { count, arc, upright: upright && !both, mirror: both })));
                    }
                    ui.label(RichText::new("Центр круга — опорная точка группы: нажмите Y и перетащите метку. 360° — весь круг; у дуги меньше повторы стоят на обоих её концах.").size(11.5).color(FAINT));
                }
                Some(RepeatKind::Along { path, count, offset, upright }) => {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("Контур").size(12.0).color(MUTED));
                        egui::ComboBox::from_id_salt("repeat-outline").selected_text(RichText::new(&path).color(FG)).width(half * 1.4).show_ui(ui, |ui| {
                            for (k, name) in &outlines {
                                let shown = self.scene.doc.nodes[*k].display_name().to_string();
                                let text = if shown == *name { name.clone() } else { format!("{shown} ({name})") };
                                if ui.selectable_label(*name == path, text).clicked() && *name != path {
                                    ask = Some(Ask::Set(Some(RepeatKind::Along { path: name.clone(), count, offset, upright })));
                                }
                            }
                        });
                    });
                    ui.horizontal(|ui| {
                        if let Some(n) = whole(ui, "Число", count, MOST) {
                            ask = Some(Ask::Set(Some(RepeatKind::Along { path: path.clone(), count: n, offset, upright })));
                        }
                        if let Some(v) = theme::num_field(ui, half, "Внутрь", offset, 0.5, -10_000.0..=10_000.0, 1, KeyState::Hidden).changed {
                            ask = Some(Ask::Set(Some(RepeatKind::Along { path: path.clone(), count, offset: v, upright })));
                        }
                    });
                    let mut keep = upright;
                    if ui.checkbox(&mut keep, RichText::new("Не поворачивать повторы").size(12.5).color(FG)).changed() {
                        ask = Some(Ask::Set(Some(RepeatKind::Along { path: path.clone(), count, offset, upright: keep })));
                    }
                    ui.label(RichText::new(format!("Повторы стоят равномерно по контуру «{path}», первый — в его начале. Место группы задаёт контур: двигайте и меняйте его, повторы пойдут следом.")).size(11.5).color(FAINT));
                }
                Some(RepeatKind::Mirror { .. }) => {}
            }
            if let Some(site) = site.as_ref().filter(|s| s.kind.takes_vary())
                && let Some((vary, what, live)) = vary_ui(ui, &site.vary, half)
            {
                ask = Some(Ask::Vary(vary, what, live));
            }
            if let Some(RepeatKind::Mirror { x, y }) = site.as_ref().map(|s| s.kind.clone()) {
                // One line at least: the last one cannot be taken away, «Нет» removes the mirror.
                let (mut across, mut down) = (x, y);
                if ui.checkbox(&mut across, RichText::new("Слева направо").size(12.5).color(FG)).on_hover_text("Отражение в вертикальной оси").changed()
                    && (across || y)
                {
                    ask = Some(Ask::Set(Some(RepeatKind::Mirror { x: across, y })));
                }
                if ui.checkbox(&mut down, RichText::new("Сверху вниз").size(12.5).color(FG)).on_hover_text("Отражение в горизонтальной оси").changed()
                    && (down || x)
                {
                    ask = Some(Ask::Set(Some(RepeatKind::Mirror { x, y: down })));
                }
                ui.label(
                    RichText::new("Оси зеркала идут через опорную точку группы: нажмите Y и перетащите метку. Рисуйте по одну сторону от оси, с двумя осями — в одной четверти.")
                        .size(11.5)
                        .color(FAINT),
                );
            }
            if site.is_some() {
                ui.label(
                    RichText::new("То, что нарисовано в группе, — первый повтор. Остальные делаются из него и меняются вместе с ним.").size(11.5).color(FAINT),
                );
            }
        });
        match ask {
            Some(Ask::Set(kind)) => {
                // A number being scrubbed is one undo step; another kind is a step of its own.
                let same = matches!(
                    (site.as_ref().map(|s| &s.kind), &kind),
                    (Some(RepeatKind::Grid { .. }), Some(RepeatKind::Grid { .. }))
                        | (Some(RepeatKind::Circle { .. }), Some(RepeatKind::Circle { .. }))
                        | (Some(RepeatKind::Along { .. }), Some(RepeatKind::Along { .. }))
                        | (Some(RepeatKind::Mirror { .. }), Some(RepeatKind::Mirror { .. }))
                );
                self.set_repeat(&node.id, kind, same);
            }
            Some(Ask::Kind(k)) => self.switch_repeat(&node.id, k),
            Some(Ask::Wrap(which)) => self.repeat_selection(which),
            Some(Ask::Select(id)) => self.select_only(Some(id)),
            Some(Ask::Vary(vary, what, live)) => {
                self.set_vary(&node.id, &vary, what, live);
            }
            None => {}
        }
    }

    /// The grips of the selected grid, when there is one to show.
    fn grips(&self, doc: &Document, rect: Rect, view: View) -> Option<Grips> {
        if self.tool != Tool::Select || !self.also.is_empty() || self.point_edit.is_some() || self.pivot_mode {
            return None;
        }
        let id = self.selected.as_ref()?;
        let site = self.scene.repeats.get(id).filter(|s| s.editable)?;
        let RepeatKind::Grid { cols, rows, step } = site.kind else { return None };
        let node = doc.index_of(id)?;
        let cell = first_repeat(doc, node, &site.clones)?;
        let world = doc.world_transform(node);
        let screen = |p: Point| view.to_screen(rect, world * p);
        let middle = cell.center();
        // The lines of grips run beside the first repeat, off its top and its left side, so that
        // a grip never covers a repeat.
        let off = |from: Point, to: Point, away: f32| {
            let d = screen(to) - screen(from);
            if d.length() > 1e-3 { d / d.length() * away } else { vec2(0.0, 0.0) }
        };
        let up = off(middle, Point::new(middle.x, middle.y - 1.0), ABOVE);
        let left = off(middle, Point::new(middle.x - 1.0, middle.y), AWAY);
        let top = |x: f64| screen(Point::new(x, cell.y0)) + up;
        let side = |y: f64| screen(Point::new(cell.x0, y)) + left;
        let mut marks = Vec::new();
        if step.0.abs() >= 0.5 {
            if cols > 1 {
                marks.push((Grip::StepX, top(middle.x + step.0), top(middle.x)));
            }
            marks.push((Grip::Cols, top(middle.x + step.0 * f64::from(cols)), top(middle.x)));
        }
        if step.1.abs() >= 0.5 {
            if rows > 1 {
                marks.push((Grip::StepY, side(middle.y + step.1), side(middle.y)));
            }
            marks.push((Grip::Rows, side(middle.y + step.1 * f64::from(rows)), side(middle.y)));
        }
        let stems = [(top(middle.x) - up, top(middle.x)), (side(middle.y) - left, side(middle.y))];
        Some(Grips { node, cols, rows, step, middle, marks, stems })
    }

    /// The pointer on a grip of the selected grid. Returns true when it is busy with one and the
    /// canvas should leave the press alone.
    pub(crate) fn repeat_input(&mut self, ui: &egui::Ui, resp: &egui::Response, rect: Rect, view: View, doc: &Document) -> bool {
        let pointer = resp.interact_pointer_pos().or(resp.hover_pos());
        self.repeat_hover = None;
        if let Some(drag) = &self.repeat_drag {
            let (id, grip) = (drag.node.clone(), drag.grip);
            if let (Some(p), Some(g)) = (pointer, self.grips(doc, rect, view)) {
                let at = doc.world_transform(g.node).inverse() * view.to_doc(rect, p);
                // Whole numbers; Ctrl leaves the step as the pointer has it, to a tenth.
                let fine = ui.input(|i| i.modifiers.command);
                let step_of = |d: f64| {
                    let v = if fine { (d * 10.0).round() / 10.0 } else { d.round() };
                    if v.abs() < 1.0 { 1.0_f64.copysign(if d == 0.0 { 1.0 } else { d }) } else { v }
                };
                let count_of = |d: f64, step: f64, other: u32| (d / step).round().clamp(1.0, (MOST / f64::from(other)).floor()) as u32;
                let (mut cols, mut rows, mut step) = (g.cols, g.rows, g.step);
                match grip {
                    Grip::StepX => step.0 = step_of(at.x - g.middle.x),
                    Grip::StepY => step.1 = step_of(at.y - g.middle.y),
                    Grip::Cols => cols = count_of(at.x - g.middle.x, step.0, rows),
                    Grip::Rows => rows = count_of(at.y - g.middle.y, step.1, cols),
                }
                if (cols, rows, step) != (g.cols, g.rows, g.step) {
                    // The whole drag is one undo step, however long the pointer rests on the way.
                    self.last_checkpoint.1 = std::time::Instant::now();
                    self.set_repeat(&id, Some(RepeatKind::Grid { cols, rows, step }), true);
                    self.status = match grip {
                        Grip::StepX | Grip::StepY => format!("Шаг сетки {} {}. Ctrl — без округления", fmt_num(step.0), fmt_num(step.1)),
                        Grip::Cols | Grip::Rows => format!("Сетка {cols} × {rows}: повторов {}", cols * rows),
                    };
                }
                self.repeat_hover = Some(grip);
                ui.ctx().set_cursor_icon(if grip.across() { egui::CursorIcon::ResizeHorizontal } else { egui::CursorIcon::ResizeVertical });
            }
            if resp.drag_stopped() || !ui.input(|i| i.pointer.primary_down()) {
                self.repeat_drag = None;
                self.last_checkpoint.0.clear();
            }
            return true;
        }
        if self.drag.is_some() {
            return false;
        }
        let Some(p) = pointer.filter(|_| resp.hovered() || resp.dragged()) else { return false };
        let Some(g) = self.grips(doc, rect, view) else { return false };
        let started = resp.drag_started_by(egui::PointerButton::Primary);
        let press = ui.input(|i| i.pointer.press_origin()).unwrap_or(p);
        let from = if started { press } else { p };
        let Some(&(grip, _, _)) =
            g.marks.iter().filter(|(_, at, _)| at.distance(from) <= GRAB).min_by(|a, b| a.1.distance(from).total_cmp(&b.1.distance(from)))
        else {
            return false;
        };
        ui.ctx().set_cursor_icon(if grip.across() { egui::CursorIcon::ResizeHorizontal } else { egui::CursorIcon::ResizeVertical });
        self.repeat_hover = Some(grip);
        if started && self.can_edit() {
            self.last_checkpoint.0.clear();
            self.repeat_drag = Some(RepeatDrag { node: doc.nodes[g.node].id.clone(), grip });
        }
        true
    }

    /// The outline of a selected repeat that goes along an outline: a box around each repeat. The
    /// group stands turned at the first point of the outline, so a box around all of it says
    /// nothing. Returns false when node `i` is not such a group.
    pub(crate) fn outline_along(&self, painter: &egui::Painter, rect: Rect, view: View, doc: &Document, i: usize, stroke: Stroke) -> bool {
        let Some(site) = self.scene.repeats.get(&doc.nodes[i].id).filter(|s| matches!(s.kind, RepeatKind::Along { .. })) else { return false };
        let boxed = |world: motif_core::kurbo::Affine, b: KRect| {
            let corners = [(b.x0, b.y0), (b.x1, b.y0), (b.x1, b.y1), (b.x0, b.y1)].map(|(x, y)| view.to_screen(rect, world * Point::new(x, y)));
            painter.add(egui::Shape::closed_line(corners.to_vec(), stroke));
        };
        if let Some(b) = first_repeat(doc, i, &site.clones) {
            boxed(doc.world_transform(i), b);
        }
        for k in site.clones.iter().filter_map(|c| doc.index_of(c)) {
            if let Some(b) = crate::app::node_bounds(doc, k) {
                boxed(doc.world_transform(k), b);
            }
        }
        true
    }

    /// The group with a mirror that the selection is, or lies in: its lines are shown while the
    /// group or anything written in it is worked on.
    fn mirror_around(&self, doc: &Document) -> Option<(usize, RepeatKind)> {
        let from = self.selected.as_deref().and_then(|id| doc.index_of(id)).or_else(|| self.scope(doc))?;
        std::iter::successors(Some(from), |&i| doc.nodes[i].parent).find_map(|i| {
            let kind = &self.scene.repeats.get(&doc.nodes[i].id)?.kind;
            matches!(kind, RepeatKind::Mirror { .. } | RepeatKind::Circle { mirror: true, .. }).then(|| (i, kind.clone()))
        })
    }

    /// The lines a mirror reflects in, dashed, through the pivot of its group and a little past
    /// everything the group shows. A circle with reflections has one, from its centre.
    fn paint_axes(&self, painter: &egui::Painter, rect: Rect, view: View, doc: &Document) {
        let Some((i, kind)) = self.mirror_around(doc) else { return };
        let Some(b) = crate::app::node_bounds(doc, i) else { return };
        let (world, p) = (doc.world_transform(i), doc.nodes[i].pivot.to_point());
        let stroke = Stroke::new(1.0, SEL.gamma_multiply(0.85));
        let line = |from: Point, to: Point| {
            let (a, z) = (view.to_screen(rect, world * from), view.to_screen(rect, world * to));
            let d = z - a;
            if d.length() > 1e-3 {
                // Past the drawing by a few points of the screen, so that the line is seen at its ends.
                let more = d / d.length() * 14.0;
                painter.extend(egui::Shape::dashed_line(&[a - more, z + more], stroke, 5.0, 4.0));
            }
        };
        match kind {
            RepeatKind::Mirror { x, y } => {
                if x {
                    line(Point::new(p.x, b.y0.min(p.y)), Point::new(p.x, b.y1.max(p.y)));
                }
                if y {
                    line(Point::new(b.x0.min(p.x), p.y), Point::new(b.x1.max(p.x), p.y));
                }
            }
            _ => line(Point::new(p.x, b.y0.min(p.y)), p),
        }
    }

    /// The grips of the selected grid: a line from the first repeat, a diamond for the step and a
    /// ring for the number of columns or rows.
    pub(crate) fn paint_repeat(&self, painter: &egui::Painter, rect: Rect, view: View, doc: &Document) {
        self.paint_axes(painter, rect, view, doc);
        let Some(g) = self.grips(doc, rect, view) else { return };
        let line = Stroke::new(1.0, SEL.gamma_multiply(0.7));
        for (across, stem) in [(true, g.stems[0]), (false, g.stems[1])] {
            let ends: Vec<&(Grip, Pos2, Pos2)> = g.marks.iter().filter(|(grip, _, _)| grip.across() == across).collect();
            if let Some((_, far, from)) = ends.last() {
                painter.line_segment([stem.0, stem.1], line);
                painter.line_segment([*from, *far], line);
                painter.circle_filled(*from, 2.0, SEL);
            }
        }
        for &(grip, at, _) in &g.marks {
            let held = self.repeat_hover == Some(grip);
            let r = if held { 6.5 } else { 5.0 };
            if grip.step() {
                let points = vec![at + vec2(0.0, -r), at + vec2(r, 0.0), at + vec2(0.0, r), at + vec2(-r, 0.0)];
                painter.add(egui::Shape::convex_polygon(points, Color32::WHITE, Stroke::new(1.5, SEL)));
            } else {
                painter.circle(at, r, Color32::WHITE, Stroke::new(1.5, SEL));
                painter.circle_filled(at, 1.6, SEL);
            }
            if held && self.snap.labels {
                let text = match (grip, self.repeat_drag.is_some()) {
                    (Grip::StepX, true) => format!("{} {}", grip.note(true), fmt_num(g.step.0)),
                    (Grip::StepY, true) => format!("{} {}", grip.note(true), fmt_num(g.step.1)),
                    (Grip::Cols, true) => format!("{}: {}", grip.note(true), g.cols),
                    (Grip::Rows, true) => format!("{}: {}", grip.note(true), g.rows),
                    (grip, false) => grip.note(false).to_string(),
                };
                crate::measure::paint_pointer_label(painter, at, &text);
            }
        }
    }
}
