//! The timeline under the canvas: animation tabs and transport on top, key lanes below.
//! Laid out like the prototype: a 172 px names column and lanes with a ruler and a red playhead.

use crate::app::{App, FPS, KeyDrag, KeyRef};
use crate::machine_ui::vsep;
use crate::theme::{self, ACCENT, ButtonKind, DANGER, FAINT, FG, Icon, LINE, MUTED, PANEL, PANEL_2, RAISE, SEL};
use egui::text::{LayoutJob, TextFormat};
use egui::{Align, Align2, Color32, Layout, Pos2, Rect, Sense, Shape, Stroke, StrokeKind, Ui, UiBuilder, pos2, vec2};
use motif_core::{Animation, Ease, Prop, ShapeKind};
use motif_script::{add_anim, copy_keys, fmt_num, paste_keys, retime_keys, set_anim_duration, set_anim_loop, set_keys_ease};

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
        Prop::Glow => "Свечение",
        Prop::InsetX => "Вн. тень X",
        Prop::InsetY => "Вн. тень Y",
        Prop::InsetBlur => "Размытие вн. тени",
        Prop::Noise => "Шум",
        Prop::Elevation => "Высота",
        Prop::Lat => "Широта",
        Prop::Lon => "Долгота",
        Prop::Alt => "Над шаром",
        Prop::Yaw => "Разворот",
        Prop::Pitch => "Наклон",
        Prop::Pull => "Притяжение",
        Prop::Fan => "Вентилятор",
        Prop::Morph => "Морфинг",
        Prop::Depth => "Глубина",
        Prop::Taper => "Сужение",
        Prop::Rough => "Неровность",
        Prop::GravityX => "Гравитация X",
        Prop::GravityY => "Гравитация Y",
        Prop::WindX => "Ветер X",
        Prop::WindY => "Ветер Y",
        Prop::Gust => "Порывы",
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
        {
            let mut ui = ui.new_child(UiBuilder::new().max_rect(bar.shrink2(vec2(10.0, 0.0))).layout(Layout::left_to_right(Align::Center)));
            ui.spacing_mut().item_spacing.x = 8.0;
            if let Some(i) = theme::segmented(&mut ui, &["Таймлайн", "Машина состояний"], usize::from(self.bottom_machine), false) {
                self.bottom_machine = i == 1;
                if self.bottom_machine {
                    self.playing = false;
                } else if self.run.take().is_some() {
                    self.status = "Машина остановлена".into();
                }
            }
            vsep(&mut ui);
            if self.bottom_machine {
                self.machine_bar(&mut ui);
            } else {
                self.timeline_bar(&mut ui, anim.as_ref());
            }
        }
        ui.painter().line_segment([bar.left_bottom(), bar.right_bottom()], Stroke::new(1.0, LINE));
        let body = Rect::from_min_max(pos2(full.left(), bar.bottom() + 1.0), full.max);
        if self.bottom_machine {
            self.machine_body(ui, body);
            ui.allocate_rect(full, Sense::hover());
            return;
        }
        self.graph.hovered = false;
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

    fn timeline_bar(&mut self, ui: &mut Ui, anim: Option<&Animation>) {
        self.anim_picker(ui, anim);
        if theme::icon_button(ui, Icon::Plus, "Новая анимация").clicked() && self.errors.is_empty() {
            let (out, name) = add_anim(&self.src, &self.scene);
            if self.apply("anim", Some(out)) {
                self.select_anim(name.clone());
                self.status = format!("Новая анимация «{name}»: двигайте объекты, ключи появятся сами");
            }
        }
        let Some(anim) = anim else { return };
        self.anim_menu(ui, anim);
        // The curve picker goes first, at the right end; the rest fills what is left and is cut
        // off at it when the window is narrow.
        let rest = ui.available_rect_before_wrap();
        let mut right = ui.new_child(UiBuilder::new().max_rect(rest).layout(Layout::right_to_left(Align::Center)));
        right.spacing_mut().item_spacing.x = 8.0;
        let narrow = rest.width() < 820.0;
        self.curve_picker(&mut right, anim, narrow);
        vsep(&mut right);
        if let Some(i) = theme::segmented(&mut right, &["Ключи", "Графики"], usize::from(self.curves), false) {
            self.curves = i == 1;
            self.curve_drag = None;
            self.key_marquee = None;
        }
        let left_end = right.min_rect().left() - 8.0;
        ui.set_clip_rect(ui.clip_rect().intersect(Rect::from_x_y_ranges(rest.left()..=left_end, rest.y_range())));
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

        let path_kind = if self.motion_path { ButtonKind::On } else { ButtonKind::Normal };
        if theme::button(ui, "Путь", path_kind)
            .on_hover_text("Траектория выделенного объекта: точка на каждый кадр, кружки — ключи положения. Тяните кружок, чтобы сдвинуть ключ; клик по линии переводит время")
            .clicked()
        {
            self.motion_path = !self.motion_path;
        }

        // Timecode: 0.42 / 1.20 с · кадр 25
        let t = if self.anim.is_some() { anim.local_time(self.time) } else { 0.0 };
        let mut job = LayoutJob::default();
        let font = theme::mono(12.0);
        let part = |job: &mut LayoutJob, s: &str, c: Color32| job.append(s, 0.0, TextFormat { font_id: font.clone(), color: c, ..Default::default() });
        // In a narrow window only the time, so that the duration still fits.
        let wide = left_end - ui.cursor().left() > 310.0;
        part(&mut job, &format!("{t:.2}"), FG);
        if wide {
            part(&mut job, &format!(" / {:.2} с · кадр ", anim.duration), MUTED);
            part(&mut job, &format!("{}", (t * FPS).round() as i64), FG);
        } else {
            part(&mut job, " с", MUTED);
        }
        let galley = ui.painter().layout_job(job);
        let (r, _) = ui.allocate_exact_size(vec2(galley.size().x.max(if wide { 150.0 } else { 0.0 }), 20.0), Sense::hover());
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
    }

    /// The curve of the picked keys: a list of curves and a preview that opens the curve editor.
    /// Laid out right to left at the end of the bar, so it never slides off a narrow window.
    fn curve_picker(&mut self, ui: &mut Ui, anim: &Animation, narrow: bool) {
        let sel = self.sel_keys.last().cloned().filter(|_| self.anim.as_deref() == Some(anim.name.as_str()));
        let cur = sel.as_ref().and_then(|k| key_of(anim, k)).map(|k| k.ease);
        ui.add_enabled_ui(cur.is_some(), |ui| {
            let mut pick = None;
            let resp = ease_preview(ui, cur).on_hover_text("Редактор кривой: тяните ручки").on_disabled_hover_text("Выберите ключ на таймлайне");
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
            egui::ComboBox::from_id_salt("ease")
                .width(108.0)
                .selected_text(match cur {
                    Some(e) => ease_label(e),
                    None => "нет ключа".into(),
                })
                .show_ui(ui, |ui| {
                    for e in Ease::BASIC {
                        if ui.selectable_label(cur == Some(e), ease_label(e)).clicked() {
                            pick = Some(e);
                        }
                    }
                })
                .response
                .on_hover_text("Кривая выбранных ключей")
                .on_disabled_hover_text("Выберите ключ на таймлайне: щелчок по ромбику. Правый щелчок по ромбику тоже меняет кривую");
            if !narrow {
                ui.label(egui::RichText::new("Кривая").size(12.0).color(MUTED));
            }
            if let Some(e) = pick
                && Some(e) != cur
            {
                self.set_ease(anim, e);
            }
        });
    }

    /// Sets the curve of every picked key.
    fn set_ease(&mut self, anim: &Animation, e: Ease) {
        let keys = self.sel_keys.clone();
        let out = set_keys_ease(&self.src, &self.scene, &anim.name, &keys, &e.script());
        if self.apply("ease", out) {
            self.status = match keys.as_slice() {
                [k] => format!("Кривая ключа {}.{} на {} с: {}", k.node, k.prop.path_name(), fmt_num(k.time), e.script()),
                _ => format!("Кривая {} ключей: {}", keys.len(), e.script()),
            };
        }
    }

    pub(crate) fn keys_picked(&self) -> bool {
        self.anim.is_some() && !self.sel_keys.is_empty()
    }

    /// Ctrl+C on picked keys: they go to the key clipboard, and as text to the system one.
    pub(crate) fn copy_keys(&mut self, ctx: &egui::Context) {
        let Some(anim) = self.anim.clone() else { return };
        let clip = copy_keys(&self.src, &self.scene, &anim, &self.sel_keys);
        if clip.is_empty() {
            return;
        }
        let text = clip
            .iter()
            .map(|c| {
                format!(
                    "{}.{}  {}: {}{}",
                    c.node,
                    c.prop.path_name(),
                    fmt_num(c.key.time),
                    c.key.value,
                    c.key.ease.as_ref().map(|e| format!(" {e}")).unwrap_or_default()
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        ctx.copy_text(text.clone());
        self.status = format!("Скопировано ключей: {}. Ctrl+V вставит их на плейхед; выберите другой объект, чтобы перенести анимацию на него", clip.len());
        self.key_clip = (text, clip);
    }

    /// Ctrl+V after copying keys: pastes them at the playhead. Keys copied from one object go to
    /// another object when that one is selected.
    pub(crate) fn paste_keys(&mut self) {
        let Some(anim) = self.current_anim().cloned() else { return };
        if !self.can_edit() {
            return;
        }
        let clip = self.key_clip.1.clone();
        let from: Vec<&str> = clip.iter().map(|c| c.node.as_str()).collect();
        let onto = match (self.selection().as_slice(), from.split_first()) {
            ([one], Some((first, rest))) if rest.iter().all(|n| n == first) && one != first => Some(one.clone()),
            _ => None,
        };
        let at = anim.local_time(self.time);
        let Some((out, pasted)) = paste_keys(&self.src, &self.scene, &anim.name, &clip, at, onto.as_deref()) else {
            self.status = "Некуда вставить ключи: объекта больше нет".into();
            return;
        };
        if let Err(e) = motif_script::parse_in(&out, self.path.as_ref().and_then(|p| p.parent())) {
            self.status = format!("Эти ключи не подходят объекту: {}", e[0]);
            return;
        }
        if self.apply("paste-keys", Some(out)) {
            self.status = match &onto {
                Some(id) => format!("Вставлено ключей на {id}: {}, начиная с {} с", pasted.len(), fmt_num(at)),
                None => format!("Вставлено ключей: {}, начиная с {} с", pasted.len(), fmt_num(at)),
            };
            self.sel_keys = pasted;
        }
    }

    fn is_key_picked(&self, k: &KeyRef) -> bool {
        self.sel_keys.iter().any(|x| same_key(x, k))
    }

    /// Picks `keys`: alone, or added to (Shift) the picked ones.
    fn pick_keys(&mut self, keys: &[KeyRef], add: bool) {
        if !add {
            self.sel_keys.clear();
        }
        for k in keys {
            if !self.is_key_picked(k) {
                self.sel_keys.push(k.clone());
            }
        }
    }

    /// Mouse on a key mark of the lanes: `keys` are the keys it stands for (one, or all keys of an
    /// object at one time). The selection of objects stays, so the rows do not change under the
    /// mouse. Click picks, Shift+click adds or removes, dragging moves every picked
    /// key and Alt+drag stretches them in time.
    fn key_mark(&mut self, resp: &egui::Response, anim: &Animation, keys: &[KeyRef], scale: Scale) {
        let Some(first) = keys.first() else { return };
        let (shift, alt) = resp.ctx.input(|i| (i.modifiers.shift, i.modifiers.alt));
        if resp.hovered() || resp.dragged() {
            resp.ctx.set_cursor_icon(if resp.dragged() { egui::CursorIcon::Grabbing } else { egui::CursorIcon::Grab });
        }
        if resp.clicked() {
            self.ensure_anim(&anim.name);
            if shift && keys.iter().all(|k| self.is_key_picked(k)) {
                self.sel_keys.retain(|x| !keys.iter().any(|k| same_key(x, k)));
            } else {
                self.pick_keys(keys, shift);
            }
            self.time = first.time;
            self.playing = false;
        }
        if resp.secondary_clicked() && !keys.iter().all(|k| self.is_key_picked(k)) {
            self.ensure_anim(&anim.name);
            self.pick_keys(keys, false);
        }
        if resp.drag_started() {
            self.ensure_anim(&anim.name);
            if !keys.iter().all(|k| self.is_key_picked(k)) {
                self.pick_keys(keys, shift);
            }
            self.playing = false;
            self.key_drag = Some(KeyDrag { grabbed: first.clone(), to: first.time, stretch: alt, keys: self.sel_keys.clone() });
        }
        if resp.dragged()
            && let Some(pos) = resp.interact_pointer_pos()
            && let Some(d) = &mut self.key_drag
        {
            d.to = scale.t(pos.x);
            d.stretch = alt;
            self.time = d.to;
        }
        if resp.drag_stopped()
            && let Some(d) = self.key_drag.take()
            && (d.to - d.grabbed.time).abs() >= 0.005
        {
            let map = d.map();
            match retime_keys(&self.src, &self.scene, &anim.name, &d.keys, &map) {
                Some(out) => {
                    self.apply("key-move", Some(out));
                    self.sel_keys = d.keys.iter().map(|k| KeyRef { time: (map(k.time) * 100.0).round() / 100.0, ..k.clone() }).collect();
                    self.status = match (d.keys.len(), d.stretch) {
                        (1, _) => format!("Ключ перенесён на {} с", fmt_num(d.to)),
                        (n, true) => format!("Ключей растянуто: {n}"),
                        (n, false) => format!("Ключей сдвинуто: {n}, на {} с", fmt_num(d.to - d.grabbed.time)),
                    };
                }
                None => self.status = "Ключи нельзя поставить так: у свойства уже есть ключ на этом времени".into(),
            }
        }
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

        // Events, under the ruler: in Animate always (to add one), in Design when there are some.
        let mut top = ruler.bottom() + 1.0;
        if animate || !anim.events.is_empty() {
            let row = Rect::from_min_max(pos2(body.left(), top), pos2(body.right(), top + ROW_H));
            self.events_row(ui, row, NAMES_W, scale, anim, animate);
            painter.line_segment([pos2(body.left(), row.bottom()), pos2(body.right(), row.bottom())], Stroke::new(1.0, LINE));
            top = row.bottom() + 1.0;
        }

        // Rows.
        let rows = self.timeline_rows(anim);
        let rows_rect = Rect::from_min_max(pos2(body.left(), top), body.max);
        let graph_rect = Rect::from_min_max(pos2(lanes.left(), rows_rect.top()), rows_rect.max);
        // Key marks on screen with the keys each stands for, for frame selection.
        let mut marks: Vec<(Pos2, Vec<KeyRef>)> = Vec::new();
        let curves = self.curves;
        if curves {
            let focus = self.curve_focus(&rows, anim);
            if self.curve_focus != focus {
                self.curve_focus = focus;
            }
        }
        let mut child = ui.new_child(UiBuilder::new().max_rect(rows_rect).layout(Layout::top_down(Align::Min)));
        egui::ScrollArea::vertical().id_salt("tl-rows").auto_shrink([false, false]).show(&mut child, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            for (i, row) in rows.iter().enumerate() {
                self.timeline_row(ui, row, i, anim, scale, animate, &mut marks);
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
        if curves && !rows.is_empty() {
            self.curve_graph(ui, graph_rect, &rows, anim, scale, &mut marks);
        }
        if animate {
            self.key_marquee(ui, graph_rect, &marks);
        }

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
                Some(d) => d.to,
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

    /// «···» by the animation picker: rename, copy, a reversed copy, delete.
    fn anim_menu(&mut self, ui: &mut Ui, anim: &Animation) {
        let resp = theme::button(ui, "···", ButtonKind::Normal).on_hover_text("Анимация: переименовать, копировать, обратная копия, удалить");
        if resp.clicked() {
            self.anim_name = anim.name.clone();
        }
        let base = self.path.as_ref().and_then(|p| p.parent()).map(|p| p.to_path_buf());
        let mut done: Option<Result<(String, String, String), String>> = None;
        egui::Popup::menu(&resp).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside).show(|ui| {
            ui.set_min_width(230.0);
            ui.label(egui::RichText::new(format!("Анимация «{}»", anim.name)).size(12.0).color(MUTED));
            let edit = ui.add(egui::TextEdit::singleline(&mut self.anim_name).desired_width(210.0));
            let enter = edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            if (enter || ui.button("Переименовать").on_hover_text("Состояния машины, которые её играют, тоже получат новое имя").clicked())
                && self.anim_name.trim() != anim.name
            {
                let name = self.anim_name.trim().to_string();
                done = Some(
                    motif_script::rename_anim(&self.src, &self.scene, &anim.name, &name, base.as_deref())
                        .map(|out| (out, name.clone(), format!("Анимация теперь «{name}»"))),
                );
                ui.close();
            }
            ui.separator();
            if ui.button("Копия").clicked() {
                done = motif_script::duplicate_anim(&self.src, &self.scene, &anim.name)
                    .map(|(out, copy)| Ok((out, copy.clone(), format!("Копия «{copy}»: правьте её, оригинал не изменится"))));
                ui.close();
            }
            if ui.button("Обратная копия").on_hover_text("Та же анимация задом наперёд: открыть → закрыть, вдох → выдох").clicked()
            {
                done = motif_script::reverse_anim(&self.src, &self.scene, &anim.name)
                    .map(|(out, copy)| Ok((out, copy.clone(), format!("«{copy}» играет «{}» задом наперёд, кривые отражены", anim.name))));
                ui.close();
            }
            ui.separator();
            if ui.button(egui::RichText::new("Удалить").color(DANGER)).clicked() {
                done = Some(
                    motif_script::delete_anim(&self.src, &self.scene, &anim.name).map(|out| (out, String::new(), format!("Анимация «{}» удалена", anim.name))),
                );
                ui.close();
            }
        });
        match done {
            Some(Ok((out, select, status))) if self.can_edit() => {
                if self.apply("anim-menu", Some(out)) {
                    if select.is_empty() {
                        self.anim = None;
                        self.last_anim = None;
                        self.playing = false;
                    } else {
                        self.select_anim(select);
                    }
                    self.status = status;
                }
            }
            Some(Err(e)) => self.status = format!("«{}»: {e}", anim.name),
            _ => {}
        }
    }

    /// The events of the animation as flags on one row: drag a flag to move it, right-click to
    /// rename or remove it, «+» or a double click on the row adds one.
    fn events_row(&mut self, ui: &mut Ui, row: Rect, names_w: f32, scale: Scale, anim: &Animation, animate: bool) {
        let p = ui.painter_at(row);
        let names = Rect::from_min_max(row.min, pos2(row.left() + names_w, row.bottom()));
        let lane = Rect::from_min_max(pos2(names.right() + 1.0, row.top()), row.max);
        p.text(pos2(names.left() + 12.0, names.center().y), Align2::LEFT_CENTER, "⚑ События", theme::ui_font(12.5), MUTED);
        let now = anim.local_time(self.time);
        // A name no event of this animation uses yet, for a new one.
        let fresh = || (1..).map(|i| format!("event{i}")).find(|n| !anim.events.iter().any(|e| &e.name == n)).unwrap_or_default();
        let mut write: Option<(String, String, Vec<f64>)> = None;
        let times_of = |name: &str| anim.events.iter().filter(|e| e.name == name).map(|e| e.time).collect::<Vec<f64>>();
        if animate {
            let plus = Rect::from_center_size(pos2(names.right() - 18.0, names.center().y), vec2(20.0, 18.0));
            let r = ui.interact(plus, ui.scope_id().with("event-add"), Sense::click()).on_hover_text("Событие на месте указателя времени");
            p.text(plus.center(), Align2::CENTER_CENTER, "+", theme::ui_font(15.0), if r.hovered() { FG } else { MUTED });
            if r.clicked() {
                write = Some((String::new(), fresh(), vec![now]));
            }
        }
        let bg = ui.interact(lane, ui.scope_id().with("event-lane"), Sense::click());
        if bg.double_clicked()
            && let Some(at) = bg.interact_pointer_pos()
        {
            self.ensure_anim(&anim.name);
            write = Some((String::new(), fresh(), vec![scale.t(at.x)]));
        }
        bg.on_hover_text("Двойной клик — событие здесь. Игра узнаёт о нём, когда анимация проходит это время");
        for (k, e) in anim.events.iter().enumerate() {
            let dragging = self.event_drag.as_ref().filter(|d| d.0 == e.name && (d.1 - e.time).abs() < 1e-6);
            let t = dragging.map_or(e.time, |d| d.2);
            let x = scale.x(t);
            let g = p.layout_no_wrap(e.name.clone(), theme::ui_font(11.5), FG);
            let w = g.size().x + 10.0;
            // Near the end the label hangs to the left of its line, so it stays readable.
            let left = if x + w > lane.right() { x - w } else { x };
            let flag = Rect::from_min_size(pos2(left, row.top() + 4.0), vec2(w, row.height() - 8.0));
            let hit = flag.union(Rect::from_center_size(pos2(x, row.center().y), vec2(10.0, row.height())));
            let resp = ui.interact(hit, ui.scope_id().with(("event", k)), Sense::click_and_drag());
            let passing = animate && self.playing && (now - e.time).abs() < 0.06;
            let color = if passing || resp.hovered() || dragging.is_some() { ACCENT } else { theme::mix(ACCENT, PANEL, 0.35) };
            p.line_segment([pos2(x, row.top() + 2.0), pos2(x, row.bottom() - 2.0)], Stroke::new(1.5, color));
            p.rect(flag, 3.0, color.gamma_multiply(if passing { 0.55 } else { 0.25 }), Stroke::NONE, StrokeKind::Inside);
            p.galley(pos2(flag.left() + 5.0, flag.center().y - g.size().y / 2.0), g, FG);
            if resp.drag_started() {
                self.ensure_anim(&anim.name);
                self.event_drag = Some((e.name.clone(), e.time, e.time));
            }
            if resp.dragged()
                && let Some(at) = resp.interact_pointer_pos()
                && let Some(d) = &mut self.event_drag
            {
                d.2 = scale.t(at.x);
                ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeColumn);
            }
            if resp.drag_stopped()
                && let Some((name, from, to)) = self.event_drag.take()
                && (to - from).abs() > 1e-6
            {
                let mut times = times_of(&name);
                if let Some(slot) = times.iter_mut().find(|v| (**v - from).abs() < 1e-6) {
                    *slot = to;
                }
                write = Some((name.clone(), name, times));
            }
            if resp.clicked() {
                self.ensure_anim(&anim.name);
                self.time = e.time;
                self.playing = false;
            }
            if resp.secondary_clicked() {
                self.event_name = e.name.clone();
            }
            let resp = resp.on_hover_text(format!("«{}» на {} с. Тяните, чтобы сдвинуть; правый клик — имя и удаление", e.name, fmt_num(e.time)));
            // The menu holds a text field: clicks inside must not close it.
            egui::Popup::context_menu(&resp).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside).show(|ui| {
                ui.set_min_width(200.0);
                ui.label(egui::RichText::new("Имя события").size(12.0).color(MUTED));
                let edit = ui.add(egui::TextEdit::singleline(&mut self.event_name).desired_width(180.0));
                let enter = edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                if (enter || ui.button("Переименовать").clicked()) && self.event_name.trim() != e.name {
                    write = Some((e.name.clone(), self.event_name.trim().to_string(), times_of(&e.name)));
                    ui.close();
                }
                ui.separator();
                if ui.button("Удалить это время").clicked() {
                    write = Some((e.name.clone(), e.name.clone(), times_of(&e.name).into_iter().filter(|v| (v - e.time).abs() > 1e-6).collect()));
                    ui.close();
                }
                if times_of(&e.name).len() > 1 && ui.button(format!("Удалить «{}» целиком", e.name)).clicked() {
                    write = Some((e.name.clone(), e.name.clone(), Vec::new()));
                    ui.close();
                }
            });
        }
        if let Some((name, new_name, times)) = write
            && self.can_edit()
        {
            match motif_script::write_event(&self.src, &self.scene, &anim.name, &name, &new_name, &times) {
                Some(out) => {
                    self.apply(&format!("event{}", self.undo.len()), Some(out));
                }
                None => self.status = format!("Имя «{new_name}» не подходит: одно слово без пробелов, и не занятое другим событием"),
            }
        }
    }

    /// Frame selection: a drag on empty lanes (or empty graph) draws a frame; the keys inside are
    /// picked on release, added to the picked ones with Shift.
    fn key_marquee(&mut self, ui: &Ui, area: Rect, marks: &[(Pos2, Vec<KeyRef>)]) {
        let Some(start) = self.key_marquee else { return };
        let (now, down, shift) = ui.input(|i| (i.pointer.latest_pos(), i.pointer.primary_down(), i.modifiers.shift));
        let Some(now) = now else { return };
        let frame = Rect::from_two_pos(start, now).intersect(area);
        let p = ui.painter_at(area);
        p.rect(frame, 2.0, SEL.gamma_multiply(0.12), Stroke::new(1.0, SEL.gamma_multiply(0.8)), StrokeKind::Inside);
        let inside: Vec<KeyRef> = marks.iter().filter(|(at, _)| frame.contains(*at)).flat_map(|(_, k)| k.iter().cloned()).collect();
        for (at, _) in marks.iter().filter(|(at, _)| frame.contains(*at)) {
            p.circle_stroke(*at, 7.0, Stroke::new(1.5, SEL));
        }
        if !down {
            self.key_marquee = None;
            self.pick_keys(&inside, shift);
            if !inside.is_empty() {
                self.status = format!("Выбрано ключей: {}. Тяните — сдвиг, Alt+тяните — растяжение, Ctrl+C / Ctrl+V, Delete", self.sel_keys.len());
            }
        }
    }

    /// The selected objects (and what is inside selected groups), topmost first, each followed by
    /// its animated properties. With nothing selected, every animated object and the world.
    fn timeline_rows(&self, anim: &Animation) -> Vec<Row> {
        let mut rows = Vec::new();
        let doc = &self.scene.doc;
        let picked: Vec<usize> = self.selection().iter().filter_map(|id| doc.index_of(id)).collect();
        for (i, n) in doc.nodes.iter().enumerate().rev() {
            let tracks: Vec<_> = Prop::ALL.iter().filter_map(|p| anim.tracks.iter().find(|t| t.node == n.id && t.prop == *p)).collect();
            let chosen = picked.contains(&i);
            let shown = if picked.is_empty() { !tracks.is_empty() } else { chosen || !tracks.is_empty() && picked.iter().any(|&g| doc.is_inside(i, g)) };
            if !shown {
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
        // The scene's gravity and wind, when they are keyed.
        let world: Vec<_> = motif_core::World::PROPS.iter().filter_map(|p| anim.tracks.iter().find(|t| t.node == "world" && t.prop == *p)).collect();
        if !world.is_empty() && picked.is_empty() {
            let mut times: Vec<f64> = world.iter().flat_map(|t| t.keys.iter().map(|k| k.time)).collect();
            times.sort_by(f64::total_cmp);
            times.dedup_by(|a, b| (*a - *b).abs() < 0.005);
            rows.push(Row::Head { id: "world".into(), name: "Мир".into(), kind: ShapeKind::Group, times });
            for t in world {
                rows.push(Row::Prop { id: "world".into(), prop: t.prop });
            }
        }
        rows
    }

    #[allow(clippy::too_many_arguments)]
    fn timeline_row(&mut self, ui: &mut Ui, row: &Row, index: usize, anim: &Animation, scale: Scale, animate: bool, marks: &mut Vec<(Pos2, Vec<KeyRef>)>) {
        // With curves on, the lanes belong to the graph: rows only answer on their names.
        let sense = if self.curves { Sense::hover() } else { Sense::click_and_drag() };
        let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), ROW_H), sense);
        let names = Rect::from_min_max(rect.min, pos2(rect.left() + NAMES_W, rect.bottom()));
        let lanes = Rect::from_min_max(pos2(names.right() + 1.0, rect.top()), rect.max);
        let resp = if self.curves { ui.interact(names, ui.scope_id().with(("row-name", index)), Sense::click()) } else { resp };
        let p = ui.painter().clone();
        let head = matches!(row, Row::Head { .. });
        if head && !self.curves {
            p.rect_filled(lanes, 0.0, theme::mix(PANEL_2, PANEL, 0.6));
        }
        if !self.curves {
            p.line_segment([rect.left_bottom(), rect.right_bottom()], Stroke::new(1.0, theme::mix(LINE, PANEL, 0.5)));
        } else {
            p.line_segment([names.left_bottom(), names.right_bottom()], Stroke::new(1.0, theme::mix(LINE, PANEL, 0.5)));
        }
        p.line_segment([names.right_top(), names.right_bottom()], Stroke::new(1.0, LINE));

        let node = match row {
            Row::Head { id, .. } | Row::Prop { id, .. } => Some(id.as_str()),
            Row::Hint => None,
        };
        // A click on a name selects the node (and in the graph, the property's curve); a click on
        // the empty part of a lane sets the time, a drag there draws a selection frame.
        if let Some(pos) = resp.interact_pointer_pos() {
            if resp.clicked()
                && names.contains(pos)
                && let Some(id) = node
            {
                self.selected = Some(id.to_string());
                if let Row::Prop { id, prop } = row {
                    self.curve_focus = Some((id.clone(), *prop));
                }
            }
            if resp.clicked() && lanes.contains(pos) {
                self.ensure_anim(&anim.name);
                self.time = scale.t(pos.x);
                self.playing = false;
                if !ui.input(|i| i.modifiers.shift) {
                    self.sel_keys.clear();
                }
            }
            if resp.drag_started() {
                let origin = ui.input(|i| i.pointer.press_origin()).unwrap_or(pos);
                if origin.x > lanes.left() {
                    self.ensure_anim(&anim.name);
                    self.key_marquee = Some(origin);
                }
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
                if self.curves {
                    return;
                }
                // Summary marks: all keys of the object at one time, picked and moved together.
                for t in times {
                    let keys: Vec<KeyRef> = anim
                        .tracks
                        .iter()
                        .filter(|tr| &tr.node == id && tr.keys.iter().any(|k| (k.time - t).abs() < 0.005))
                        .map(|tr| KeyRef { node: id.clone(), prop: tr.prop, time: *t })
                        .collect();
                    let shown = self.drag_time(&keys[0], *t);
                    let c = pos2(scale.x(shown), lanes.center().y);
                    let picked = animate && keys.iter().all(|k| self.is_key_picked(k));
                    theme::diamond(&p, c, if picked { 5.0 } else { 3.5 }, if picked { FG } else { MUTED }, Stroke::NONE);
                    let kresp = ui.interact(
                        Rect::from_center_size(c, vec2(12.0, ROW_H)),
                        ui.scope_id().with(("head-key", id, (t * 100.0).round() as i64)),
                        Sense::click_and_drag(),
                    );
                    let kresp = kresp.on_hover_text(format!("Все ключи объекта на {} с: {}", fmt_num(*t), keys.len()));
                    if ui.clip_rect().contains(c) {
                        marks.push((c, keys.clone()));
                    }
                    self.key_mark(&kresp, anim, &keys, scale);
                }
            }
            Row::Hint => {
                p.text(pos2(names.left() + 28.0, names.center().y), Align2::LEFT_CENTER, "—", theme::ui_font(12.0), FAINT);
                if !self.curves {
                    p.text(
                        pos2(scale.x0, lanes.center().y),
                        Align2::LEFT_CENTER,
                        "Режим Animate + изменение свойства = новый ключ",
                        theme::ui_font(11.5),
                        FAINT,
                    );
                }
            }
            Row::Prop { id, prop } => {
                let focused = self.curves && self.curve_focus.as_ref().is_some_and(|(n, p)| n == id && p == prop);
                if focused {
                    p.rect_filled(names.shrink2(vec2(2.0, 2.0)), 4.0, RAISE);
                }
                let numeric = anim.tracks.iter().find(|t| &t.node == id && t.prop == *prop).is_some_and(|t| is_numeric(&t));
                if self.curves && numeric {
                    p.line_segment(
                        [pos2(names.left() + 12.0, names.center().y), pos2(names.left() + 22.0, names.center().y)],
                        Stroke::new(2.0, curve_color(index).gamma_multiply(if focused { 1.0 } else { 0.6 })),
                    );
                }
                p.text(
                    pos2(names.left() + 28.0, names.center().y),
                    Align2::LEFT_CENTER,
                    prop_label(*prop),
                    theme::ui_font(12.0),
                    if focused { FG } else { MUTED },
                );
                if self.curves {
                    return;
                }
                let Some(track) = anim.tracks.iter().find(|t| &t.node == id && t.prop == *prop) else { return };
                let refs: Vec<KeyRef> = track.keys.iter().map(|k| KeyRef { node: id.clone(), prop: *prop, time: k.time }).collect();
                let times: Vec<f64> = refs.iter().map(|k| self.drag_time(k, k.time)).collect();
                // Segments between keys; dotted after a hold key.
                let seg = ACCENT.gamma_multiply(0.45);
                let mut order: Vec<usize> = (0..times.len()).collect();
                order.sort_by(|a, b| times[*a].total_cmp(&times[*b]));
                for w in order.windows(2) {
                    let (a, b) = (pos2(scale.x(times[w[0]]), lanes.center().y), pos2(scale.x(times[w[1]]), lanes.center().y));
                    if track.keys[w[0]].ease == Ease::Hold {
                        p.extend(Shape::dashed_line(&[a, b], Stroke::new(2.0, seg), 2.0, 3.0));
                    } else {
                        p.line_segment([a, b], Stroke::new(2.0, seg));
                    }
                }
                for (i, k) in track.keys.iter().enumerate() {
                    let c = pos2(scale.x(times[i]), lanes.center().y);
                    let r = Rect::from_center_size(c, vec2(14.0, ROW_H));
                    let kresp = ui.interact(r, ui.scope_id().with(("key", id, *prop, i)), Sense::click_and_drag());
                    let me = refs[i].clone();
                    let selected = animate && self.is_key_picked(&me);
                    if ui.clip_rect().contains(c) {
                        marks.push((c, vec![me.clone()]));
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
                    let kresp = kresp.on_hover_text(format!(
                        "{} · {:.2} с · {} · Shift — добавить к выбору, Alt+тянуть — растянуть, правый щелчок — кривая",
                        prop_label(*prop),
                        k.time,
                        ease_label(k.ease)
                    ));
                    // Right click: pick the curve straight from the key (for every picked key).
                    let mut pick = None;
                    kresp.context_menu(|ui| {
                        ui.label(egui::RichText::new("Кривая ключа").size(12.0).color(MUTED));
                        for e in Ease::BASIC {
                            if ui.selectable_label(k.ease == e, ease_label(e)).clicked() {
                                pick = Some(e);
                                ui.close();
                            }
                        }
                    });
                    self.key_mark(&kresp, anim, std::slice::from_ref(&me), scale);
                    if let Some(e) = pick {
                        self.set_ease(anim, e);
                    }
                }
            }
        }
    }

    /// Where a key is drawn: moved along while it is being dragged.
    fn drag_time(&self, k: &KeyRef, t: f64) -> f64 {
        match &self.key_drag {
            Some(d) if d.moves(k) => (d.map()(t) * 100.0).round() / 100.0,
            _ => t,
        }
    }
}

fn same_key(a: &KeyRef, b: &KeyRef) -> bool {
    a.node == b.node && a.prop == b.prop && (a.time - b.time).abs() < 0.005
}

fn key_of<'a>(anim: &'a Animation, k: &KeyRef) -> Option<&'a motif_core::Keyframe> {
    anim.tracks.iter().find(|t| t.node == k.node && t.prop == k.prop)?.keys.iter().find(|key| (key.time - k.time).abs() < 0.005)
}

impl App {
    /// The animation list: the states of the machine first (picking one opens its animation),
    /// then every animation.
    fn anim_picker(&mut self, ui: &mut Ui, anim: Option<&Animation>) {
        let machine = self.shown_machine().cloned();
        // The picked state when it plays the shown animation, otherwise the first state that does.
        let plays = |s: &&motif_core::State| anim.is_some_and(|a| a.name == s.anim);
        let state = machine.as_ref().and_then(|m| match self.machine_pick {
            Some(motif_script::MachineItem::State(i)) if m.states.get(i).is_some_and(|s| plays(&s)) => m.states.get(i),
            _ => m.states.iter().find(plays),
        });
        let text = match (anim, state) {
            (Some(a), Some(s)) if s.name != a.name => format!("{} · {}", s.name, a.name),
            (Some(a), _) => a.name.clone(),
            (None, _) => "нет анимаций".into(),
        };
        let mut pick: Option<(String, Option<usize>)> = None;
        let resp = egui::ComboBox::from_id_salt("anim-pick")
            .width(140.0)
            .height(420.0)
            .selected_text(egui::RichText::new(text).color(if self.anim.is_some() { FG } else { MUTED }))
            .show_ui(ui, |ui| {
                if let Some(m) = &machine
                    && !m.states.is_empty()
                {
                    ui.label(egui::RichText::new(format!("Состояния · {}", m.name)).size(11.0).color(FAINT));
                    for (i, s) in m.states.iter().enumerate() {
                        let on = state.is_some_and(|st| st.name == s.name);
                        let mut job = LayoutJob::default();
                        job.append(&s.name, 0.0, TextFormat { font_id: theme::ui_font(13.0), color: FG, ..Default::default() });
                        if s.anim != s.name {
                            job.append(&format!("  {}", s.anim), 0.0, TextFormat { font_id: theme::mono(11.0), color: MUTED, ..Default::default() });
                        }
                        if ui.selectable_label(on, job).clicked() {
                            pick = Some((s.anim.clone(), Some(i)));
                        }
                    }
                }
                // Animations no state plays.
                let used = |name: &str| machine.as_ref().is_some_and(|m| m.states.iter().any(|s| s.anim == name));
                let rest: Vec<String> = self.scene.doc.animations.iter().map(|a| a.name.clone()).filter(|n| !used(n)).collect();
                if !rest.is_empty() && machine.as_ref().is_some_and(|m| !m.states.is_empty()) {
                    ui.separator();
                    ui.label(egui::RichText::new("Другие анимации").size(11.0).color(FAINT));
                }
                for name in rest {
                    let on = anim.is_some_and(|x| x.name == name);
                    if ui.selectable_label(on, &name).clicked() {
                        pick = Some((name, None));
                    }
                }
            });
        resp.response.on_hover_text("Какую анимацию править: состояние машины или любую анимацию");
        if let Some((name, state)) = pick {
            self.select_anim(name);
            self.machine_pick = state.map(motif_script::MachineItem::State);
        }
    }
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

/// A key or a handle being dragged in the graph.
#[derive(Clone, Debug)]
pub struct CurveDrag {
    node: String,
    prop: Prop,
    /// The value range of the graph when the drag started: it stays put while dragging.
    range: (f64, f64),
    grab: CurveGrab,
}

#[derive(Clone, Debug)]
enum CurveGrab {
    /// The picked keys of the track move by a time and a value.
    Keys { times: Vec<f64>, start: (f64, f64), dt: f64, dv: f64 },
    /// One handle of the curve from key `seg` to the next.
    Handle { seg: usize, h: [f64; 4] },
}

const CURVE_COLORS: [Color32; 6] = [
    Color32::from_rgb(0xf4, 0xa6, 0x2a),
    Color32::from_rgb(0x5a, 0xa9, 0xff),
    Color32::from_rgb(0x4f, 0xd3, 0x8c),
    Color32::from_rgb(0xff, 0x6b, 0x9a),
    Color32::from_rgb(0xb3, 0x8c, 0xff),
    Color32::from_rgb(0x4f, 0xd6, 0xd6),
];

fn curve_color(row: usize) -> Color32 {
    CURVE_COLORS[row % CURVE_COLORS.len()]
}

fn is_numeric(t: &&motif_core::Track) -> bool {
    t.keys.iter().all(|k| k.value.as_num().is_some())
}

/// How many decimals a dragged value keeps, from the size of the graph's range.
fn value_decimals(range: f64) -> i32 {
    if range >= 100.0 {
        0
    } else if range >= 10.0 {
        1
    } else {
        2
    }
}

fn round_to(v: f64, decimals: i32) -> f64 {
    let f = 10f64.powi(decimals);
    (v * f).round() / f
}

/// The lowest and highest value a track takes, with its curves' overshoot.
fn value_range(track: &motif_core::Track, dur: f64) -> (f64, f64) {
    let mut lo = f64::INFINITY;
    let mut hi = f64::NEG_INFINITY;
    let mut take = |v: Option<motif_core::Value>| {
        if let Some(v) = v.and_then(|v| v.as_num()) {
            lo = lo.min(v);
            hi = hi.max(v);
        }
    };
    for k in &track.keys {
        take(Some(k.value));
    }
    for i in 0..=200 {
        take(track.sample(dur * i as f64 / 200.0));
    }
    if !lo.is_finite() {
        return (0.0, 1.0);
    }
    if hi - lo < 1e-6 {
        let pad = lo.abs().max(1.0) * 0.5;
        return (lo - pad, hi + pad);
    }
    let pad = (hi - lo) * 0.08;
    (lo - pad, hi + pad)
}

/// Maps values to y inside the graph and back.
#[derive(Clone, Copy)]
struct VScale {
    top: f32,
    bottom: f32,
    lo: f64,
    hi: f64,
}

impl VScale {
    fn y(self, v: f64) -> f32 {
        self.bottom - ((v - self.lo) / (self.hi - self.lo)) as f32 * (self.bottom - self.top)
    }

    fn v(self, y: f32) -> f64 {
        self.lo + ((self.bottom - y) / (self.bottom - self.top).max(1.0)) as f64 * (self.hi - self.lo)
    }
}

impl Scale {
    /// Time at `x`, not snapped and not clamped.
    fn t_raw(self, x: f32) -> f64 {
        ((x - self.x0) / (self.x1 - self.x0).max(1.0)) as f64 * self.dur
    }
}

impl App {
    /// The property the graph edits: the clicked one while it is shown, else the first numeric one.
    fn curve_focus(&self, rows: &[Row], anim: &Animation) -> Option<(String, Prop)> {
        let numeric = |id: &str, prop: Prop| anim.tracks.iter().find(|t| t.node == id && t.prop == prop).is_some_and(|t| is_numeric(&t));
        let shown: Vec<(String, Prop)> = rows
            .iter()
            .filter_map(|r| match r {
                Row::Prop { id, prop } if numeric(id, *prop) => Some((id.clone(), *prop)),
                _ => None,
            })
            .collect();
        match &self.curve_focus {
            Some(f) if shown.contains(f) => Some(f.clone()),
            _ => shown.into_iter().next(),
        }
    }

    /// The track as it looks while something of it is dragged in the graph.
    fn curve_preview(&self, track: &motif_core::Track) -> motif_core::Track {
        let mut t = track.clone();
        let Some(d) = &self.curve_drag else { return t };
        if d.node != track.node || d.prop != track.prop {
            return t;
        }
        match &d.grab {
            CurveGrab::Keys { times, dt, dv, .. } => {
                let decimals = value_decimals(d.range.1 - d.range.0);
                for k in &mut t.keys {
                    if times.iter().any(|x| (x - k.time).abs() < 0.005) {
                        k.time = ((k.time + dt).max(0.0) * 100.0).round() / 100.0;
                        if let Some(v) = k.value.as_num() {
                            k.value = motif_core::Value::Num(round_to(v + dv, decimals));
                        }
                    }
                }
                t.keys.sort_by(|a, b| a.time.total_cmp(&b.time));
            }
            CurveGrab::Handle { seg, h } => {
                if let Some(k) = t.keys.get_mut(*seg) {
                    k.ease = Ease::Cubic(*h);
                }
            }
        }
        t
    }

    /// The graph editor: every shown numeric property as a curve of value over time, each scaled
    /// to its own range. The focused one is drawn bright with its values, keys and handles.
    fn curve_graph(&mut self, ui: &mut Ui, area: Rect, rows: &[Row], anim: &Animation, scale: Scale, marks: &mut Vec<(Pos2, Vec<KeyRef>)>) {
        let animate = self.anim.as_deref() == Some(anim.name.as_str());
        let painter = ui.painter_at(area);
        painter.rect_filled(area, 0.0, theme::mix(theme::BG, PANEL, 0.5));
        let plot = Rect::from_min_max(pos2(area.left(), area.top() + 14.0), pos2(area.right(), area.bottom() - 14.0));
        // Empty graph: click sets the time, drag frames keys.
        let bg = ui.interact(area, ui.scope_id().with("curve-bg"), Sense::click_and_drag());
        if let Some(pos) = bg.interact_pointer_pos() {
            if bg.clicked() {
                self.ensure_anim(&anim.name);
                self.time = scale.t(pos.x);
                self.playing = false;
                if !ui.input(|i| i.modifiers.shift) {
                    self.sel_keys.clear();
                }
            }
            if bg.drag_started() {
                self.ensure_anim(&anim.name);
                self.key_marquee = Some(ui.input(|i| i.pointer.press_origin()).unwrap_or(pos));
            }
        }
        // Time grid from the ruler's marks.
        for i in 0..=10 {
            let x = scale.x(anim.duration * i as f64 / 10.0);
            painter.line_segment([pos2(x, area.top()), pos2(x, area.bottom())], Stroke::new(1.0, theme::mix(LINE, PANEL, 0.4)));
        }

        let focus = self.curve_focus.clone();
        let mut focused: Option<(usize, motif_core::Track)> = None;
        for (index, row) in rows.iter().enumerate() {
            let Row::Prop { id, prop } = row else { continue };
            let Some(track) = anim.tracks.iter().find(|t| &t.node == id && t.prop == *prop).filter(is_numeric) else { continue };
            let preview = self.curve_preview(track);
            if focus.as_ref().is_some_and(|(n, p)| n == id && p == prop) {
                focused = Some((index, preview));
                continue;
            }
            let (lo, hi) = value_range(&preview, anim.duration);
            let vs = VScale { top: plot.top(), bottom: plot.bottom(), lo, hi };
            paint_curve(&painter, &preview, scale, vs, plot, Stroke::new(1.2, curve_color(index).gamma_multiply(0.35)));
        }
        let Some((index, track)) = focused else {
            painter.text(area.center(), Align2::CENTER_CENTER, "Цветам и заливкам графики не нужны: у них нет числа", theme::ui_font(12.0), FAINT);
            return;
        };
        let (node, prop) = (track.node.clone(), track.prop);
        let range = match &self.curve_drag {
            Some(d) if d.node == node && d.prop == prop => d.range,
            _ => value_range(&track, anim.duration),
        };
        let vs = VScale { top: plot.top(), bottom: plot.bottom(), lo: range.0, hi: range.1 };
        let color = curve_color(index);
        // Value grid with labels of the focused property.
        let decimals = value_decimals(range.1 - range.0);
        for i in 0..=4 {
            let v = range.0 + (range.1 - range.0) * i as f64 / 4.0;
            let y = vs.y(v);
            painter.line_segment([pos2(area.left(), y), pos2(area.right(), y)], Stroke::new(1.0, theme::mix(LINE, PANEL, 0.6)));
            painter.text(pos2(area.left() + 4.0, y - 2.0), Align2::LEFT_BOTTOM, theme_num(round_to(v, decimals)), theme::mono(10.0), FAINT);
        }
        paint_curve(&painter, &track, scale, vs, plot, Stroke::new(2.0, color));
        painter.text(pos2(area.right() - 8.0, area.top() + 4.0), Align2::RIGHT_TOP, format!("{} · {}", node, prop_label(prop)), theme::ui_font(11.5), color);

        // Handles of the curves next to picked keys.
        let at = |t: f64, v: f64| pos2(scale.x(t), vs.y(v));
        let keyed: Vec<(f64, f64, Ease)> = track.keys.iter().filter_map(|k| Some((k.time, k.value.as_num()?, k.ease))).collect();
        for seg in 0..keyed.len().saturating_sub(1) {
            let (t0, v0, ease) = keyed[seg];
            let (t1, v1, _) = keyed[seg + 1];
            let picked = |t: f64| self.is_key_picked(&KeyRef { node: node.clone(), prop, time: t });
            let Some(h) = ease.handles() else { continue };
            if !animate || !(picked(t0) || picked(t1)) || t1 - t0 < 1e-6 {
                continue;
            }
            for which in 0..2 {
                let end = if which == 0 { at(t0, v0) } else { at(t1, v1) };
                let hp = at(t0 + h[which * 2] * (t1 - t0), v0 + h[which * 2 + 1] * (v1 - v0));
                painter.line_segment([end, hp], Stroke::new(1.0, MUTED));
                let resp = ui.interact(Rect::from_center_size(hp, vec2(14.0, 14.0)), ui.scope_id().with(("curve-handle", seg, which)), Sense::drag());
                painter.circle(hp, 4.5, if resp.hovered() || resp.dragged() { color } else { FG }, Stroke::new(1.5, theme::BG));
                let resp = resp.on_hover_text("Ручка кривой: тяните. Выше и ниже значения ключей — перелёт");
                if resp.drag_started() {
                    self.curve_drag = Some(CurveDrag { node: node.clone(), prop, range, grab: CurveGrab::Handle { seg, h } });
                }
                if resp.dragged()
                    && let Some(p) = resp.interact_pointer_pos()
                    && let Some(CurveDrag { grab: CurveGrab::Handle { h, .. }, .. }) = &mut self.curve_drag
                {
                    let x = ((scale.t_raw(p.x) - t0) / (t1 - t0)).clamp(0.0, 1.0);
                    h[which * 2] = (x * 100.0).round() / 100.0;
                    if (v1 - v0).abs() > 1e-9 {
                        let y = ((vs.v(p.y) - v0) / (v1 - v0)).clamp(-2.0, 3.0);
                        h[which * 2 + 1] = (y * 100.0).round() / 100.0;
                    }
                }
                if resp.drag_stopped()
                    && let Some(CurveDrag { grab: CurveGrab::Handle { seg, h }, .. }) = self.curve_drag.take()
                {
                    let key = KeyRef { node: node.clone(), prop, time: keyed[seg].0 };
                    let e = Ease::Cubic(h);
                    if self.apply("ease", set_keys_ease(&self.src, &self.scene, &anim.name, &[key], &e.script())) {
                        self.status = format!("Кривая {}.{} на {} с: {}", node, prop.path_name(), fmt_num(keyed[seg].0), e.script());
                    }
                }
            }
        }

        // Keys: click picks, Shift adds, drag moves time and value (Shift while dragging locks the
        // direction), Alt+drag in time stretches like in the lanes.
        for (i, &(t, v, ease)) in keyed.iter().enumerate() {
            let c = at(t, v);
            let me = KeyRef { node: node.clone(), prop, time: t };
            let picked = animate && self.is_key_picked(&me);
            marks.push((c, vec![me.clone()]));
            let resp = ui.interact(Rect::from_center_size(c, vec2(14.0, 14.0)), ui.scope_id().with(("curve-key", i)), Sense::click_and_drag());
            if picked {
                theme::diamond(&painter, c, 8.5, color.gamma_multiply(0.45), Stroke::NONE);
            }
            theme::diamond(
                &painter,
                c,
                5.5,
                if picked {
                    FG
                } else if ease == Ease::Hold {
                    PANEL
                } else {
                    color
                },
                Stroke::new(1.5, if picked { FG } else { color }),
            );
            let resp = resp.on_hover_text(format!("{} с · {} · {}", fmt_num(t), theme_num(round_to(v, decimals)), ease_label(ease)));
            if resp.hovered() || resp.dragged() {
                ui.ctx().set_cursor_icon(if resp.dragged() { egui::CursorIcon::Grabbing } else { egui::CursorIcon::Grab });
            }
            let shift = ui.input(|i| i.modifiers.shift);
            if resp.clicked() {
                self.ensure_anim(&anim.name);
                if shift && picked {
                    self.sel_keys.retain(|x| !same_key(x, &me));
                } else {
                    self.pick_keys(std::slice::from_ref(&me), shift);
                }
                self.time = t;
                self.playing = false;
            }
            if resp.drag_started()
                && let Some(p) = resp.interact_pointer_pos()
            {
                self.ensure_anim(&anim.name);
                if !self.is_key_picked(&me) {
                    self.pick_keys(std::slice::from_ref(&me), shift);
                }
                let times: Vec<f64> = self.sel_keys.iter().filter(|k| k.node == node && k.prop == prop).map(|k| k.time).collect();
                self.curve_drag = Some(CurveDrag {
                    node: node.clone(),
                    prop,
                    range,
                    grab: CurveGrab::Keys { times, start: (scale.t_raw(p.x), vs.v(p.y)), dt: 0.0, dv: 0.0 },
                });
                self.playing = false;
            }
            if resp.dragged()
                && let Some(p) = resp.interact_pointer_pos()
                && let Some(CurveDrag { grab: CurveGrab::Keys { start, dt, dv, .. }, .. }) = &mut self.curve_drag
            {
                *dt = scale.t_raw(p.x) - start.0;
                *dv = vs.v(p.y) - start.1;
                if shift {
                    // Lock to the direction moved more on screen.
                    let (dx, dy) = ((scale.x(start.0 + *dt) - scale.x(start.0)).abs(), (vs.y(start.1 + *dv) - vs.y(start.1)).abs());
                    if dx > dy {
                        *dv = 0.0;
                    } else {
                        *dt = 0.0;
                    }
                }
                self.time = ((t + *dt).max(0.0) * 100.0).round() / 100.0;
            }
            if resp.drag_stopped()
                && let Some(CurveDrag { grab: CurveGrab::Keys { times, dt, dv, .. }, range, .. }) = self.curve_drag.take()
            {
                self.commit_curve_keys(anim, &node, prop, &times, dt, dv, value_decimals(range.1 - range.0));
            }
        }
        // Design mode: only a preview.
        if !animate {
            self.curve_drag = None;
        }
    }

    /// Writes keys moved in the graph: their new times and values.
    #[allow(clippy::too_many_arguments)]
    fn commit_curve_keys(&mut self, anim: &Animation, node: &str, prop: Prop, times: &[f64], dt: f64, dv: f64, decimals: i32) {
        let Some(track) = anim.tracks.iter().find(|t| t.node == node && t.prop == prop) else { return };
        let all: Vec<KeyRef> = track.keys.iter().map(|k| KeyRef { node: node.to_string(), prop, time: k.time }).collect();
        let mut keys: Vec<motif_script::KeyText> = copy_keys(&self.src, &self.scene, &anim.name, &all).into_iter().map(|c| c.key).collect();
        let moved = |t: f64| times.iter().any(|x| (x - t).abs() < 0.005);
        for (k, src) in keys.iter_mut().zip(&track.keys) {
            if !moved(src.time) {
                continue;
            }
            k.time = ((k.time + dt).max(0.0) * 100.0).round() / 100.0;
            if dv != 0.0
                && let Some(v) = src.value.as_num()
            {
                k.value = theme_num(round_to(v + dv, decimals));
            }
        }
        if dt.abs() < 0.005 && dv == 0.0 {
            return;
        }
        let out = motif_script::write_tracks(&self.src, &self.scene, &anim.name, &[(node.to_string(), prop, keys.clone())]);
        if out.is_none() {
            self.status = "Ключи нельзя поставить так: у свойства уже есть ключ на этом времени".into();
            return;
        }
        if self.apply("curve-keys", out) {
            self.sel_keys = times.iter().map(|x| KeyRef { node: node.to_string(), prop, time: ((x + dt).max(0.0) * 100.0).round() / 100.0 }).collect();
            self.status = format!("Ключей изменено: {} ({}.{})", times.len(), node, prop.path_name());
        }
    }
}

/// A value as the script writes it.
fn theme_num(v: f64) -> String {
    motif_script::fmt_num_with(v, 3)
}

/// Draws a track's value over the whole timeline, sampling every two pixels.
fn paint_curve(p: &egui::Painter, track: &motif_core::Track, scale: Scale, vs: VScale, plot: Rect, stroke: Stroke) {
    let mut pts = Vec::new();
    let mut x = scale.x0;
    while x <= scale.x1 + 0.5 {
        let t = scale.t_raw(x).clamp(0.0, scale.dur);
        if let Some(v) = track.sample(t).and_then(|v| v.as_num()) {
            pts.push(pos2(x, vs.y(v).clamp(plot.top() - 12.0, plot.bottom() + 12.0)));
        }
        x += 2.0;
    }
    if pts.len() > 1 {
        p.add(Shape::line(pts, stroke));
    }
}
