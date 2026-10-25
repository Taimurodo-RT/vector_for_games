//! The timeline under the canvas: animation tabs and transport on top, key lanes below.
//! Laid out like the prototype: a 172 px names column and lanes with a ruler and a red playhead.

use crate::app::{App, FPS, KeyRef};
use crate::theme::{self, ACCENT, ButtonKind, DANGER, FAINT, FG, Icon, LINE, MUTED, PANEL, PANEL_2, RAISE, SEL};
use egui::text::{LayoutJob, TextFormat};
use egui::{Align, Align2, Color32, Layout, Pos2, Rect, Sense, Shape, Stroke, StrokeKind, Ui, UiBuilder, pos2, vec2};
use motif_core::{Animation, Ease, Prop, ShapeKind};
use motif_script::{add_anim, fmt_num, move_key, set_anim_duration, set_anim_loop, set_key_ease};

const NAMES_W: f32 = 172.0;
const ROW_H: f32 = 24.0;
const BAR_H: f32 = 38.0;
/// Padding inside the lanes so that keys at 0 and at the end are not cut.
const LANE_PAD_L: f32 = 12.0;
const LANE_PAD_R: f32 = 16.0;

pub fn prop_label(p: Prop) -> &'static str {
    match p {
        Prop::X => "Позиция X",
        Prop::Y => "Позиция Y",
        Prop::Width => "Ширина",
        Prop::Height => "Высота",
        Prop::Rotation => "Поворот",
        Prop::ScaleX => "Масштаб X",
        Prop::ScaleY => "Масштаб Y",
        Prop::Opacity => "Прозрачность",
        Prop::Radius => "Скругление",
        Prop::Points => "Лучи",
        Prop::Inner => "Внутр. радиус",
        Prop::Fill => "Заливка",
        Prop::StrokeColor => "Обводка",
        Prop::StrokeWidth => "Толщина обводки",
        Prop::SkewX => "Наклон X",
        Prop::SkewY => "Наклон Y",
        Prop::FontSize => "Размер шрифта",
        Prop::LetterSpacing => "Межбуквенный",
        Prop::Reveal => "Появление букв",
        Prop::Wave => "Волна",
        Prop::Phase => "Фаза волны",
        Prop::Blur => "Размытие",
        Prop::ShadowX => "Тень X",
        Prop::ShadowY => "Тень Y",
        Prop::ShadowBlur => "Размытие тени",
        Prop::Elevation => "Высота",
    }
}

fn ease_label(e: Ease) -> String {
    match e {
        Ease::Linear => "Linear".into(),
        Ease::Css => "Ease".into(),
        Ease::EaseIn => "Ease In".into(),
        Ease::EaseOut => "Ease Out".into(),
        Ease::EaseInOut => "Ease In-Out".into(),
        Ease::Hold => "Hold".into(),
        Ease::Spring => "Spring".into(),
        Ease::Cubic(_) => "Своя".into(),
        Ease::Steps(n) => format!("Steps {n}"),
        named => named.keyword().to_string(),
    }
}

/// One line of the lanes: a node with its key summary, or one animated property of it.
enum Row {
    Head { id: String, name: String, kind: ShapeKind, times: Vec<f64> },
    Prop { id: String, prop: Prop },
    Hint,
}

/// Maps time to x inside the lanes and back.
#[derive(Clone, Copy)]
struct Scale {
    x0: f32,
    x1: f32,
    dur: f64,
}

impl Scale {
    fn x(self, t: f64) -> f32 {
        let u = if self.dur > 0.0 { (t / self.dur).clamp(0.0, 1.0) } else { 0.0 };
        self.x0 + (self.x1 - self.x0) * u as f32
    }

    fn t(self, x: f32) -> f64 {
        let u = ((x - self.x0) / (self.x1 - self.x0).max(1.0)).clamp(0.0, 1.0) as f64;
        (u * self.dur * 100.0).round() / 100.0
    }
}

impl App {
    /// The animation the timeline shows: the selected one, or in Design the one Animate returns to.
    fn shown_anim(&self) -> Option<Animation> {
        let doc = &self.scene.doc;
        self.anim.as_deref().or(self.last_anim.as_deref()).and_then(|n| doc.animation(n)).or(doc.animations.first()).cloned()
    }

    /// Enters Animate on `name` (a click on the timeline in Design does this).
    fn ensure_anim(&mut self, name: &str) {
        if self.anim.as_deref() != Some(name) {
            self.select_anim(name.to_string());
        }
    }

    pub(crate) fn timeline(&mut self, ui: &mut Ui) {
        let full = ui.available_rect_before_wrap();
        ui.painter().line_segment([full.left_top(), full.right_top()], Stroke::new(1.0, LINE));
        let anim = self.shown_anim();
        let bar = Rect::from_min_size(full.min, vec2(full.width(), BAR_H));
        self.timeline_bar(ui, bar, anim.as_ref());
        ui.painter().line_segment([bar.left_bottom(), bar.right_bottom()], Stroke::new(1.0, LINE));
        let body = Rect::from_min_max(pos2(full.left(), bar.bottom() + 1.0), full.max);
        match anim {
            Some(a) => self.timeline_body(ui, body, &a),
            None => {
                ui.painter().text(
                    body.left_top() + vec2(12.0, 16.0),
                    Align2::LEFT_TOP,
                    "Анимаций пока нет. Нажмите «+» или Tab: появится анимация, а правки объектов начнут ставить ключи.",
                    theme::ui_font(12.5),
                    FAINT,
                );
            }
        }
        ui.allocate_rect(full, Sense::hover());
    }

    fn timeline_bar(&mut self, ui: &mut Ui, bar: Rect, anim: Option<&Animation>) {
        let mut ui = ui.new_child(UiBuilder::new().max_rect(bar.shrink2(vec2(10.0, 0.0))).layout(Layout::left_to_right(Align::Center)));
        let ui = &mut ui;
        ui.spacing_mut().item_spacing.x = 8.0;
        ui.label(egui::RichText::new("ТАЙМЛАЙН").font(theme::semibold(11.0)).color(MUTED).extra_letter_spacing(0.9));
        vsep(ui);

        // Animation tabs.
        let names: Vec<String> = self.scene.doc.animations.iter().map(|a| a.name.clone()).collect();
        ui.scope(|ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            for name in &names {
                let on = self.anim.as_deref() == Some(name.as_str());
                let shown = anim.is_some_and(|a| &a.name == name);
                if tab(ui, name, on, shown).clicked() {
                    self.select_anim(name.clone());
                }
            }
        });
        if theme::icon_button(ui, Icon::Plus, "Новая анимация").clicked() && self.errors.is_empty() {
            let (out, name) = add_anim(&self.src, &self.scene);
            if self.apply("anim", Some(out)) {
                self.select_anim(name.clone());
                self.status = format!("Новая анимация «{name}»: двигайте объекты, ключи появятся сами");
            }
        }
        let Some(anim) = anim else { return };
        vsep(ui);

        if theme::icon_button(ui, Icon::First, "В начало").clicked() {
            self.ensure_anim(&anim.name);
            self.time = 0.0;
            self.playing = false;
        }
        let play_icon = if self.playing { Icon::Pause } else { Icon::Play };
        if theme::icon_button(ui, play_icon, "Пуск и пауза (пробел)").clicked() {
            self.ensure_anim(&anim.name);
            self.playing = !self.playing;
        }
        let loop_kind = if anim.looping { ButtonKind::On } else { ButtonKind::Normal };
        if theme::button(ui, "Повтор", loop_kind).on_hover_text("Повторять анимацию по кругу").clicked() {
            let out = set_anim_loop(&self.src, &self.scene, &anim.name, !anim.looping);
            self.apply("loop", out);
        }

        let onion_kind = if self.onion { ButtonKind::On } else { ButtonKind::Normal };
        if theme::button(ui, "Калька", onion_kind).on_hover_text("Onion skin: соседние позы на паузе — красные раньше, зелёные позже (по 6 кадров)").clicked()
        {
            self.onion = !self.onion;
            if self.anim.is_none() {
                self.ensure_anim(&anim.name);
            }
        }

        // Timecode: 0.42 / 1.20 с · кадр 25
        let t = if self.anim.is_some() { anim.local_time(self.time) } else { 0.0 };
        let mut job = LayoutJob::default();
        let font = theme::mono(12.0);
        let part = |job: &mut LayoutJob, s: &str, c: Color32| job.append(s, 0.0, TextFormat { font_id: font.clone(), color: c, ..Default::default() });
        part(&mut job, &format!("{t:.2}"), FG);
        part(&mut job, &format!(" / {:.2} с · кадр ", anim.duration), MUTED);
        part(&mut job, &format!("{}", (t * FPS).round() as i64), FG);
        let galley = ui.painter().layout_job(job);
        let (r, _) = ui.allocate_exact_size(vec2(galley.size().x.max(150.0), 20.0), Sense::hover());
        ui.painter().galley(pos2(r.left(), r.center().y - galley.size().y / 2.0), galley, FG);
        vsep(ui);

        // Duration.
        ui.label(egui::RichText::new("Длит.").size(12.0).color(MUTED));
        let mut d = anim.duration;
        let resp =
            ui.add_sized([58.0, 22.0], egui::DragValue::new(&mut d).speed(0.01).range(0.05..=600.0).max_decimals(2).custom_formatter(|n, _| format!("{n:.2}")));
        ui.label(egui::RichText::new("с").size(12.0).color(MUTED));
        if resp.changed() && (d - anim.duration).abs() > 1e-9 {
            let out = set_anim_duration(&self.src, &self.scene, &anim.name, (d * 100.0).round() / 100.0);
            self.apply("anim-duration", out);
        }

        // Curve of the selected key.
        ui.label(egui::RichText::new("Кривая").size(12.0).color(MUTED));
        let sel = self.sel_key.clone().filter(|_| self.anim.as_deref() == Some(anim.name.as_str()));
        let cur = sel.as_ref().and_then(|k| key_of(anim, k)).map(|k| k.ease);
        ui.add_enabled_ui(cur.is_some(), |ui| {
            let mut pick = None;
            egui::ComboBox::from_id_salt("ease")
                .width(108.0)
                .selected_text(ease_label(cur.unwrap_or(Ease::EaseInOut)))
                .show_ui(ui, |ui| {
                    for e in Ease::BASIC {
                        if ui.selectable_label(cur == Some(e), ease_label(e)).clicked() {
                            pick = Some(e);
                        }
                    }
                })
                .response
                .on_disabled_hover_text("Выберите ключ на таймлайне");
            let resp = ease_preview(ui, cur).on_hover_text("Редактор кривой: тяните ручки");
            egui::Popup::from_toggle_button_response(&resp).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside).show(|ui| {
                ui.horizontal_top(|ui| {
                    ui.vertical(|ui| {
                        if let Some(e) = curve_editor(ui, cur.unwrap_or(Ease::EaseInOut)) {
                            pick = Some(e);
                        }
                    });
                    ui.add_space(8.0);
                    ui.vertical(|ui| {
                        if let Some(e) = ease_library(ui, cur.unwrap_or(Ease::EaseInOut)) {
                            pick = Some(e);
                        }
                    });
                });
            });
            if let (Some(e), Some(k)) = (pick, &sel)
                && Some(e) != cur
            {
                let out = set_key_ease(&self.src, &self.scene, &anim.name, &k.node, k.prop, k.time, &e.script());
                if self.apply("ease", out) {
                    self.status = format!("Кривая ключа {}.{} на {} с: {}", k.node, k.prop.path_name(), fmt_num(k.time), e.script());
                }
            }
        });
    }

    fn timeline_body(&mut self, ui: &mut Ui, body: Rect, anim: &Animation) {
        let animate = self.anim.as_deref() == Some(anim.name.as_str());
        let painter = ui.painter_at(body);
        let names_col = Rect::from_min_max(body.min, pos2(body.left() + NAMES_W, body.bottom()));
        painter.line_segment([names_col.right_top(), names_col.right_bottom()], Stroke::new(1.0, LINE));
        let lanes = Rect::from_min_max(pos2(names_col.right() + 1.0, body.top()), body.max);
        let scale = Scale { x0: lanes.left() + LANE_PAD_L, x1: lanes.right() - LANE_PAD_R, dur: anim.duration };

        // Ruler.
        let ruler = Rect::from_min_size(lanes.min, vec2(lanes.width(), ROW_H));
        painter.line_segment([pos2(body.left(), ruler.bottom()), ruler.right_bottom()], Stroke::new(1.0, LINE));
        paint_ruler(&painter, ruler, scale);
        let rresp = ui.interact(ruler, ui.scope_id().with("ruler"), Sense::click_and_drag());
        if rresp.hovered() || rresp.dragged() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeColumn);
        }
        if (rresp.clicked() || rresp.dragged() || rresp.drag_started())
            && let Some(p) = rresp.interact_pointer_pos()
        {
            self.ensure_anim(&anim.name);
            self.time = scale.t(p.x);
            self.playing = false;
        }

        // Rows.
        let rows = self.timeline_rows(anim);
        let rows_rect = Rect::from_min_max(pos2(body.left(), ruler.bottom() + 1.0), body.max);
        let mut child = ui.new_child(UiBuilder::new().max_rect(rows_rect).layout(Layout::top_down(Align::Min)));
        egui::ScrollArea::vertical().id_salt("tl-rows").auto_shrink([false, false]).show(&mut child, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            for row in &rows {
                self.timeline_row(ui, row, anim, scale, animate);
            }
            if rows.is_empty() {
                let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), ROW_H + 8.0), Sense::hover());
                ui.painter().text(
                    pos2(lanes.left() + LANE_PAD_L, r.center().y),
                    Align2::LEFT_CENTER,
                    "В анимации пока нет ключей. Выберите объект и измените свойство в режиме Animate.",
                    theme::ui_font(12.0),
                    FAINT,
                );
            }
        });

        // Design mode: the lanes are only a preview.
        if !animate {
            painter.rect_filled(lanes, 0.0, PANEL.gamma_multiply(0.45));
            let text = "Design: правки меняют сцену. Клик по таймлайну или Tab включает Animate";
            let g = painter.layout_no_wrap(text.into(), theme::ui_font(12.0), MUTED);
            let r = Rect::from_center_size(pos2(lanes.center().x, lanes.bottom() - 22.0), g.size() + vec2(20.0, 10.0));
            painter.rect(r, 6.0, PANEL_2, Stroke::new(1.0, LINE), StrokeKind::Inside);
            painter.galley(r.center() - g.size() / 2.0, g, MUTED);
        }

        // Playhead over the ruler and all rows.
        if animate {
            let t = match &self.key_drag {
                Some((_, t)) => *t,
                None => anim.local_time(self.time),
            };
            let x = scale.x(t);
            let p = ui.painter_at(lanes);
            p.line_segment([pos2(x, lanes.top()), pos2(x, lanes.bottom())], Stroke::new(1.5, DANGER));
            let y = lanes.top();
            let head = vec![pos2(x - 5.5, y), pos2(x + 5.5, y), pos2(x + 5.5, y + 5.0), pos2(x, y + 10.0), pos2(x - 5.5, y + 5.0)];
            p.add(Shape::convex_polygon(head, DANGER, Stroke::NONE));
        }
    }

    /// Nodes with tracks (and the selected node), topmost first, each followed by its animated properties.
    fn timeline_rows(&self, anim: &Animation) -> Vec<Row> {
        let mut rows = Vec::new();
        for n in self.scene.doc.nodes.iter().rev() {
            let tracks: Vec<_> = Prop::ALL.iter().filter_map(|p| anim.tracks.iter().find(|t| t.node == n.id && t.prop == *p)).collect();
            if tracks.is_empty() && self.selected.as_deref() != Some(n.id.as_str()) {
                continue;
            }
            let mut times: Vec<f64> = tracks.iter().flat_map(|t| t.keys.iter().map(|k| k.time)).collect();
            times.sort_by(f64::total_cmp);
            times.dedup_by(|a, b| (*a - *b).abs() < 0.005);
            rows.push(Row::Head { id: n.id.clone(), name: n.display_name().to_string(), kind: n.kind, times });
            if tracks.is_empty() {
                rows.push(Row::Hint);
            }
            for t in tracks {
                rows.push(Row::Prop { id: n.id.clone(), prop: t.prop });
            }
        }
        rows
    }

    fn timeline_row(&mut self, ui: &mut Ui, row: &Row, anim: &Animation, scale: Scale, animate: bool) {
        let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), ROW_H), Sense::click_and_drag());
        let names = Rect::from_min_max(rect.min, pos2(rect.left() + NAMES_W, rect.bottom()));
        let lanes = Rect::from_min_max(pos2(names.right() + 1.0, rect.top()), rect.max);
        let p = ui.painter().clone();
        let head = matches!(row, Row::Head { .. });
        if head {
            p.rect_filled(lanes, 0.0, theme::mix(PANEL_2, PANEL, 0.6));
        }
        p.line_segment([rect.left_bottom(), rect.right_bottom()], Stroke::new(1.0, theme::mix(LINE, PANEL, 0.5)));
        p.line_segment([names.right_top(), names.right_bottom()], Stroke::new(1.0, LINE));

        let node = match row {
            Row::Head { id, .. } | Row::Prop { id, .. } => Some(id.as_str()),
            Row::Hint => None,
        };
        // A click or drag on the empty part of a lane scrubs; a click on a name selects the node.
        if let Some(pos) = resp.interact_pointer_pos()
            && (resp.clicked() || resp.dragged() || resp.drag_started())
        {
            if (names.contains(pos) || !resp.dragged())
                && let Some(id) = node
            {
                self.selected = Some(id.to_string());
            }
            if lanes.contains(pos) || resp.dragged() {
                self.ensure_anim(&anim.name);
                self.time = scale.t(pos.x);
                self.playing = false;
                self.sel_key = None;
            }
        }

        match row {
            Row::Head { id, name, kind, times } => {
                let sel = self.selected.as_deref() == Some(id.as_str());
                let color = if sel { SEL } else { FG };
                theme::paint_icon(
                    &p,
                    Rect::from_center_size(pos2(names.left() + 18.0, names.center().y), vec2(14.0, 14.0)),
                    theme::shape_icon(*kind),
                    if sel { SEL } else { MUTED },
                );
                let g = p.layout_no_wrap(name.clone(), theme::semibold(12.0), color);
                p.with_clip_rect(names.shrink(2.0)).galley(pos2(names.left() + 30.0, names.center().y - g.size().y / 2.0), g, color);
                for t in times {
                    theme::diamond(&p, pos2(scale.x(*t), lanes.center().y), 3.5, MUTED, Stroke::NONE);
                }
            }
            Row::Hint => {
                p.text(pos2(names.left() + 28.0, names.center().y), Align2::LEFT_CENTER, "—", theme::ui_font(12.0), FAINT);
                p.text(pos2(scale.x0, lanes.center().y), Align2::LEFT_CENTER, "Режим Animate + изменение свойства = новый ключ", theme::ui_font(11.5), FAINT);
            }
            Row::Prop { id, prop } => {
                p.text(pos2(names.left() + 28.0, names.center().y), Align2::LEFT_CENTER, prop_label(*prop), theme::ui_font(12.0), MUTED);
                let Some(track) = anim.tracks.iter().find(|t| &t.node == id && t.prop == *prop) else { return };
                let dragging = |k: f64| match &self.key_drag {
                    Some((r, t)) if &r.node == id && r.prop == *prop && (r.time - k).abs() < 0.005 => Some(*t),
                    _ => None,
                };
                let times: Vec<f64> = track.keys.iter().map(|k| dragging(k.time).unwrap_or(k.time)).collect();
                // Segments between keys; dotted after a hold key.
                let seg = ACCENT.gamma_multiply(0.45);
                for (i, k) in track.keys.iter().enumerate().take(track.keys.len().saturating_sub(1)) {
                    let (a, b) = (pos2(scale.x(times[i]), lanes.center().y), pos2(scale.x(times[i + 1]), lanes.center().y));
                    if k.ease == Ease::Hold {
                        p.extend(Shape::dashed_line(&[a, b], Stroke::new(2.0, seg), 2.0, 3.0));
                    } else {
                        p.line_segment([a, b], Stroke::new(2.0, seg));
                    }
                }
                for (i, k) in track.keys.iter().enumerate() {
                    let c = pos2(scale.x(times[i]), lanes.center().y);
                    let r = Rect::from_center_size(c, vec2(14.0, ROW_H));
                    let kresp = ui.interact(r, ui.scope_id().with(("key", id, *prop, i)), Sense::click_and_drag());
                    let me = KeyRef { node: id.clone(), prop: *prop, time: k.time };
                    let selected = animate && self.sel_key.as_ref() == Some(&me);
                    if kresp.hovered() || kresp.dragged() {
                        ui.ctx().set_cursor_icon(if kresp.dragged() { egui::CursorIcon::Grabbing } else { egui::CursorIcon::Grab });
                    }
                    if selected {
                        theme::diamond(&p, c, 8.5, ACCENT.gamma_multiply(0.5), Stroke::NONE);
                    }
                    let fill = if selected {
                        FG
                    } else if k.ease == Ease::Hold {
                        PANEL
                    } else {
                        ACCENT
                    };
                    theme::diamond(&p, c, 6.0, fill, Stroke::new(1.5, if selected { FG } else { ACCENT }));
                    let kresp = kresp.on_hover_text(format!("{} · {:.2} с · {}", prop_label(*prop), k.time, ease_label(k.ease)));
                    if kresp.clicked() || kresp.drag_started() {
                        self.ensure_anim(&anim.name);
                        self.sel_key = Some(me.clone());
                        self.selected = Some(id.clone());
                        self.time = k.time;
                        self.playing = false;
                    }
                    if kresp.dragged()
                        && let Some(pos) = kresp.interact_pointer_pos()
                    {
                        let t = scale.t(pos.x);
                        self.key_drag = Some((me.clone(), t));
                        self.time = t;
                    }
                    if kresp.drag_stopped()
                        && let Some((from, to)) = self.key_drag.take()
                        && (from.time - to).abs() >= 0.005
                    {
                        match move_key(&self.src, &self.scene, &anim.name, &from.node, from.prop, from.time, to) {
                            Some(out) => {
                                self.apply("key-move", Some(out));
                                self.sel_key = Some(KeyRef { time: to, ..from });
                                self.status = format!("Ключ перенесён на {} с", fmt_num(to));
                            }
                            None => self.status = format!("На {} с у этого свойства уже есть ключ", fmt_num(to)),
                        }
                    }
                }
            }
        }
    }
}

fn key_of<'a>(anim: &'a Animation, k: &KeyRef) -> Option<&'a motif_core::Keyframe> {
    anim.tracks.iter().find(|t| t.node == k.node && t.prop == k.prop)?.keys.iter().find(|key| (key.time - k.time).abs() < 0.005)
}

fn vsep(ui: &mut Ui) {
    let (r, _) = ui.allocate_exact_size(vec2(1.0, 20.0), Sense::hover());
    ui.painter().rect_filled(r, 0.0, LINE);
}

/// An animation tab: raised when it is the active one, outlined when Design shows it as a preview.
fn tab(ui: &mut Ui, text: &str, on: bool, shown: bool) -> egui::Response {
    let g = ui.painter().layout_no_wrap(text.to_string(), theme::ui_font(13.0), Color32::PLACEHOLDER);
    let (r, resp) = ui.allocate_exact_size(vec2(g.size().x + 20.0, 26.0), Sense::click());
    let (bg, fg) = if on {
        (RAISE, FG)
    } else if resp.hovered() {
        (PANEL_2, FG)
    } else {
        (Color32::TRANSPARENT, MUTED)
    };
    ui.painter().rect_filled(r, 5.0, bg);
    if shown && !on {
        ui.painter().rect_stroke(r, 5.0, Stroke::new(1.0, LINE), StrokeKind::Inside);
    }
    ui.painter().galley(r.center() - g.size() / 2.0, g, fg);
    resp.on_hover_text("Открыть анимацию в режиме Animate")
}

fn paint_ruler(p: &egui::Painter, ruler: Rect, s: Scale) {
    let d = s.dur;
    let major = if d <= 1.2 {
        0.1
    } else if d <= 2.5 {
        0.25
    } else if d <= 6.0 {
        0.5
    } else if d <= 20.0 {
        1.0
    } else {
        5.0
    };
    let minor = major / 5.0;
    let mut i = 0;
    loop {
        let t = i as f64 * minor;
        if t > d + 1e-6 {
            break;
        }
        let is_major = i % 5 == 0;
        let x = s.x(t);
        let (h, c) = if is_major { (9.0, MUTED) } else { (5.0, FAINT) };
        p.line_segment([pos2(x, ruler.bottom() - h), pos2(x, ruler.bottom())], Stroke::new(1.0, c));
        if is_major {
            let label = format!("{}s", fmt_num((t * 100.0).round() / 100.0));
            let near_end = t > d - major / 2.0;
            let (anchor, at) =
                if near_end { (Align2::RIGHT_BOTTOM, pos2(x - 3.0, ruler.bottom() - 9.0)) } else { (Align2::LEFT_BOTTOM, pos2(x + 3.0, ruler.bottom() - 9.0)) };
            p.text(at, anchor, label, theme::mono(10.5), MUTED);
        }
        i += 1;
    }
}

/// Small curve preview next to the ease picker; a click opens the curve editor.
fn ease_preview(ui: &mut Ui, ease: Option<Ease>) -> egui::Response {
    let (r, resp) = ui.allocate_exact_size(vec2(34.0, 24.0), Sense::click());
    let p = ui.painter();
    let border = if resp.hovered() {
        ACCENT
    } else if ease.is_some() {
        Color32::from_rgb(0x3a, 0x41, 0x52)
    } else {
        LINE
    };
    p.rect_stroke(r, 4.0, Stroke::new(1.0, border), StrokeKind::Inside);
    let e = ease.unwrap_or(Ease::EaseInOut);
    let pts: Vec<Pos2> = (0..=24)
        .map(|i| {
            let u = i as f64 / 24.0;
            let v = e.apply(u).clamp(-0.3, 1.3) as f32;
            pos2(r.left() + 3.0 + u as f32 * 28.0, r.top() + 21.0 - v * 16.0)
        })
        .collect();
    p.add(Shape::line(pts, Stroke::new(1.5, if ease.is_some() { ACCENT } else { FAINT })));
    resp
}

/// A 1×1 graph of the curve with two draggable Bézier handles, and the presets.
/// Returns the new curve when the user changes it.
fn curve_editor(ui: &mut Ui, ease: Ease) -> Option<Ease> {
    let mut out = None;
    ui.set_max_width(240.0);
    ui.label(egui::RichText::new("Кривая ключа").font(theme::semibold(13.0)).color(FG));
    ui.label(egui::RichText::new("Время вправо, значение вверх. Ручки можно тянуть выше и ниже рамки — будет перелёт.").size(11.5).color(MUTED));
    ui.add_space(4.0);
    let (r, _) = ui.allocate_exact_size(vec2(240.0, 240.0), Sense::hover());
    // The unit square sits in the middle; space above and below is for overshoot.
    let sq = Rect::from_min_size(r.min + vec2(30.0, 60.0), vec2(180.0, 120.0));
    let to = |x: f64, y: f64| pos2(sq.left() + x as f32 * sq.width(), sq.bottom() - y as f32 * sq.height());
    let from = |p: Pos2| (((p.x - sq.left()) / sq.width()) as f64, ((sq.bottom() - p.y) / sq.height()) as f64);
    let painter = ui.painter_at(r);
    painter.rect_filled(r, 6.0, theme::BG);
    for i in 0..=4 {
        let f = i as f32 / 4.0;
        let c = if i == 0 || i == 4 { LINE } else { theme::mix(LINE, theme::BG, 0.5) };
        painter.line_segment([pos2(sq.left() + f * sq.width(), r.top()), pos2(sq.left() + f * sq.width(), r.bottom())], Stroke::new(1.0, c));
        painter.line_segment([pos2(r.left(), sq.bottom() - f * sq.height()), pos2(r.right(), sq.bottom() - f * sq.height())], Stroke::new(1.0, c));
    }
    let pts: Vec<Pos2> = (0..=64).map(|i| i as f64 / 64.0).map(|u| to(u, ease.apply(u))).collect();
    painter.add(Shape::line(pts, Stroke::new(2.0, ACCENT)));
    // Curves without handles (Hold, Spring, elastic, bounce, steps) start from Ease In-Out once a handle is dragged.
    let mut h = ease.handles().unwrap_or([0.65, 0.0, 0.35, 1.0]);
    let ends = [to(0.0, 0.0), to(1.0, 1.0)];
    for k in 0..2 {
        let at = to(h[k * 2], h[k * 2 + 1]);
        let resp = ui.interact(Rect::from_center_size(at, vec2(16.0, 16.0)), ui.scope_id().with(("handle", k)), Sense::drag());
        if ease.handles().is_some() {
            painter.line_segment([ends[k], at], Stroke::new(1.0, MUTED));
            painter.circle(at, 5.0, if resp.hovered() || resp.dragged() { ACCENT } else { FG }, Stroke::new(1.5, theme::BG));
        }
        if resp.dragged()
            && let Some(p) = resp.interact_pointer_pos()
        {
            let (x, y) = from(p);
            h[k * 2] = (x.clamp(0.0, 1.0) * 100.0).round() / 100.0;
            h[k * 2 + 1] = (y.clamp(-0.5, 1.5) * 100.0).round() / 100.0;
            out = Some(Ease::Cubic(h));
        }
    }
    ui.add_space(4.0);
    ui.label(egui::RichText::new(ease.script()).font(theme::mono(11.5)).color(FAINT));
    out
}

/// The curve library: the basic curves, the 30 easings.net functions as a grid of previews, and steps.
fn ease_library(ui: &mut Ui, ease: Ease) -> Option<Ease> {
    let mut out = None;
    ui.label(egui::RichText::new("Библиотека").font(theme::semibold(13.0)).color(FG));
    let mut cell = |ui: &mut Ui, e: Ease, tip: &str| {
        let (r, resp) = ui.allocate_exact_size(vec2(44.0, 30.0), Sense::click());
        let on = e == ease;
        let p = ui.painter();
        p.rect(r, 4.0, if resp.hovered() { RAISE } else { theme::BG }, Stroke::new(1.0, if on { ACCENT } else { LINE }), StrokeKind::Inside);
        let pts: Vec<Pos2> = (0..=28)
            .map(|i| {
                let u = i as f64 / 28.0;
                let v = e.apply(u).clamp(-0.4, 1.4) as f32;
                pos2(r.left() + 5.0 + u as f32 * 34.0, r.bottom() - 8.0 - v * 14.0)
            })
            .collect();
        p.add(Shape::line(pts, Stroke::new(1.3, if on || resp.hovered() { ACCENT } else { MUTED })));
        if resp.on_hover_text(tip).clicked() {
            out = Some(e);
        }
    };
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(4.0, 4.0);
        for e in Ease::BASIC {
            cell(ui, e, e.keyword());
        }
    });
    ui.add_space(2.0);
    egui::Grid::new("ease-grid").num_columns(4).spacing([4.0, 4.0]).show(ui, |ui| {
        ui.label("");
        for d in ["in", "out", "in-out"] {
            ui.label(egui::RichText::new(d).font(theme::mono(10.5)).color(FAINT));
        }
        ui.end_row();
        for f in motif_core::Family::ALL {
            ui.label(egui::RichText::new(f.name()).font(theme::mono(11.0)).color(MUTED));
            for d in motif_core::Dir::ALL {
                let e = Ease::Named(f, d);
                cell(ui, e, e.keyword());
            }
            ui.end_row();
        }
    });
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        let mut n = if let Ease::Steps(n) = ease { n } else { 4 };
        ui.label(egui::RichText::new("Шаги").size(12.0).color(MUTED));
        let changed = ui.add(egui::DragValue::new(&mut n).range(1..=60)).changed();
        let on = matches!(ease, Ease::Steps(_));
        if changed
            || theme::button(ui, "steps", if on { ButtonKind::On } else { ButtonKind::Normal })
                .on_hover_text("Скачки равными шагами — как покадровая анимация")
                .clicked()
        {
            out = Some(Ease::Steps(n));
        }
    });
    out
}
