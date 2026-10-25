//! The state machine in the bottom panel: a graph of states joined by transitions on the left,
//! the inputs and the selected state or transition on the right, and a run mode that plays the
//! machine on the canvas while the inputs are switched by hand.

use crate::app::{App, section};
use crate::theme::{self, ACCENT, ButtonKind, FAINT, FG, GOOD, Icon, LINE, MUTED, PANEL, PANEL_2, RAISE, SEL};
use egui::{Align, Align2, Color32, Layout, Pos2, Rect, RichText, Sense, Shape, Stroke, StrokeKind, Ui, UiBuilder, Vec2, pos2, vec2};
use motif_core::{Cond, CondOp, Input, InputKind, Machine, MachinePlayer, State, Transition};
use motif_script::{MachineItem, add_machine, delete_machine, delete_machine_item, edit_machine, free_name, make_entry};

const BOX: Vec2 = vec2(150.0, 46.0);
const ANY_BOX: Vec2 = vec2(96.0, 28.0);
/// Where the `any` box sits in the graph.
const ANY_AT: Pos2 = pos2(40.0, 12.0);
const INSPECTOR_W: f32 = 330.0;
/// Words the script reads as part of a machine line, so they cannot name an input or a state.
const RESERVED: [&str; 15] = ["any", "not", "and", "when", "at", "end", "blend", "input", "state", "anim", "true", "false", "trigger", "number", "bool"];

/// The graph view: scroll offset and the drag in progress.
#[derive(Default)]
pub struct Graph {
    pan: Vec2,
    drag: Option<GraphDrag>,
    /// The pointer is over the graph (Delete then removes the selected state or transition).
    pub hovered: bool,
}

enum GraphDrag {
    /// A state box (`None`: the `any` box) being moved, and where it is now.
    Move { state: Option<usize>, at: Pos2 },
    /// A new transition being pulled from a state (`None`: from `any`) to the pointer.
    Link { from: Option<usize>, to: Pos2 },
}

/// The machine running on the canvas.
pub struct Run {
    pub player: MachinePlayer,
    /// The transition taken last and how long it stays lit.
    flash: Option<(usize, f32)>,
}

fn valid_name(s: &str) -> bool {
    let mut chars = s.chars();
    chars.next().is_some_and(|c| c.is_alphabetic() || c == '_') && chars.all(|c| c.is_alphanumeric() || c == '_') && !RESERVED.contains(&s)
}

/// Where state `i` sits in the graph: its `at`, or a place in a grid below the others.
fn state_at(m: &Machine, i: usize) -> Pos2 {
    match m.states[i].pos {
        Some((x, y)) => pos2(x as f32, y as f32),
        None => pos2(40.0 + 180.0 * (i % 4) as f32, 80.0 + 80.0 * (i / 4) as f32),
    }
}

fn kind_label(k: InputKind) -> &'static str {
    match k {
        InputKind::Bool => "флаг",
        InputKind::Number => "число",
        InputKind::Trigger => "триггер",
    }
}

fn op_label(op: CondOp) -> &'static str {
    match op {
        CondOp::Is => "включён",
        CondOp::Not => "выключен",
        op => op.symbol(),
    }
}

/// The point where the line from the centre of `r` towards `to` leaves `r`.
fn border_point(r: Rect, to: Pos2) -> Pos2 {
    let d = to - r.center();
    if d.length() < 1e-3 {
        return r.center();
    }
    let t = (r.width() / 2.0 / d.x.abs().max(1e-6)).min(r.height() / 2.0 / d.y.abs().max(1e-6));
    r.center() + d * t.min(1.0)
}

fn dist_to_segment(p: Pos2, a: Pos2, b: Pos2) -> f32 {
    let ab = b - a;
    let t = ((p - a).dot(ab) / ab.length_sq().max(1e-6)).clamp(0.0, 1.0);
    (a + ab * t - p).length()
}

fn arrow_head(painter: &egui::Painter, tip: Pos2, dir: Vec2, color: Color32) {
    let d = dir.normalized();
    let n = d.rot90();
    painter.add(Shape::convex_polygon(vec![tip + d * 5.0, tip - d * 5.0 + n * 5.0, tip - d * 5.0 - n * 5.0], color, Stroke::NONE));
}

/// A one-line name field that hands back the new text when editing ends with a different, valid name.
fn name_edit(ui: &mut Ui, salt: impl std::hash::Hash + std::fmt::Debug, current: &str, width: f32) -> Option<String> {
    let id = ui.make_persistent_id(salt);
    let edit_id = id.with("edit");
    let focused = ui.memory(|m| m.has_focus(edit_id));
    let mut text = if focused { ui.data(|d| d.get_temp::<String>(id)).unwrap_or_else(|| current.to_string()) } else { current.to_string() };
    let resp = ui.add(egui::TextEdit::singleline(&mut text).id(edit_id).desired_width(width).font(theme::mono(12.5)));
    if resp.changed() {
        ui.data_mut(|d| d.insert_temp(id, text.clone()));
    }
    if resp.lost_focus() {
        ui.data_mut(|d| d.remove::<String>(id));
        if text != current {
            return Some(text);
        }
    }
    None
}

impl App {
    /// The machine the bottom panel shows: the one picked, or the first.
    pub(crate) fn shown_machine(&self) -> Option<&Machine> {
        let doc = &self.scene.doc;
        self.machine.as_deref().and_then(|n| doc.machine(n)).or(doc.machines.first())
    }

    /// The running machine and its player, when the canvas plays one.
    pub(crate) fn running(&self) -> Option<(&Machine, &MachinePlayer)> {
        Some((self.shown_machine()?, &self.run.as_ref()?.player))
    }

    pub(crate) fn toggle_run(&mut self) {
        if self.run.is_some() {
            self.run = None;
            self.status = "Машина остановлена".into();
        } else if let Some(m) = self.shown_machine() {
            if m.states.is_empty() {
                self.status = "В машине нет состояний: добавьте их кнопкой «+ Состояние»".into();
                return;
            }
            let player = MachinePlayer::new(m);
            self.status = format!("Машина «{}» играет: переключайте входы справа", m.name);
            self.set_mode_animate(false);
            self.run = Some(Run { player, flash: None });
        }
    }

    /// Moves the running machine on by `dt` seconds.
    pub(crate) fn advance_run(&mut self, dt: f32) {
        let Some(m) = self.shown_machine().cloned() else {
            self.run = None;
            return;
        };
        let Some(run) = &mut self.run else { return };
        if let Some(k) = run.player.advance(&m, &self.scene.doc, dt as f64) {
            run.flash = Some((k, 0.6));
        }
        if let Some((_, t)) = &mut run.flash {
            *t -= dt;
            if *t <= 0.0 {
                run.flash = None;
            }
        }
    }

    /// Keeps the picked machine, state and transition valid after the script changed.
    pub(crate) fn prune_machine(&mut self) {
        if let Some(n) = &self.machine
            && self.scene.doc.machine(n).is_none()
        {
            self.machine = None;
            self.machine_pick = None;
        }
        let ok = match (self.shown_machine(), self.machine_pick) {
            (Some(m), Some(MachineItem::State(i))) => i < m.states.len(),
            (Some(m), Some(MachineItem::Transition(i))) => i < m.transitions.len(),
            (Some(m), Some(MachineItem::Input(i))) => i < m.inputs.len(),
            (_, None) => true,
            (None, _) => false,
        };
        if !ok {
            self.machine_pick = None;
        }
        if self.shown_machine().is_none() {
            self.run = None;
        }
    }

    fn machine_edit(&mut self, kind: &str, f: impl FnOnce(&mut Machine)) -> bool {
        let Some(name) = self.shown_machine().map(|m| m.name.clone()) else { return false };
        if !self.can_edit() {
            return false;
        }
        let out = edit_machine(&self.src, &self.scene, &name, f);
        self.apply(kind, out)
    }

    /// Deletes the selected state or transition.
    pub(crate) fn delete_machine_pick(&mut self) {
        let (Some(name), Some(item)) = (self.shown_machine().map(|m| m.name.clone()), self.machine_pick) else { return };
        if !self.can_edit() {
            return;
        }
        let out = delete_machine_item(&self.src, &self.scene, &name, item);
        if self.apply("machine-delete", out) {
            self.machine_pick = None;
            self.status = "Удалено из машины".into();
        }
    }

    /// Opens state `i` on the timeline to edit its animation.
    fn open_state(&mut self, i: usize) {
        let Some(anim) = self.shown_machine().and_then(|m| m.states.get(i)).map(|s| s.anim.clone()) else { return };
        self.run = None;
        self.machine_pick = Some(MachineItem::State(i));
        self.bottom_machine = false;
        self.select_anim(anim);
    }

    /// The bar of the machine view: which machine, which state, run and reset.
    pub(crate) fn machine_bar(&mut self, ui: &mut Ui) {
        let names: Vec<String> = self.scene.doc.machines.iter().map(|m| m.name.clone()).collect();
        let shown = self.shown_machine().map(|m| m.name.clone());
        ui.label(RichText::new("Машина").size(12.0).color(MUTED));
        let mut pick = None;
        let mut create = false;
        egui::ComboBox::from_id_salt("machine-pick").width(130.0).selected_text(shown.clone().unwrap_or_else(|| "нет".into())).show_ui(ui, |ui| {
            for n in &names {
                if ui.selectable_label(shown.as_deref() == Some(n.as_str()), n).clicked() {
                    pick = Some(n.clone());
                }
            }
            ui.separator();
            if ui.selectable_label(false, "+ Новая машина").clicked() {
                create = true;
            }
        });
        if let Some(n) = pick {
            self.machine = Some(n);
            self.machine_pick = None;
            self.run = None;
        }
        if (create || (names.is_empty() && theme::button(ui, "Создать машину", ButtonKind::Primary).clicked())) && self.can_edit() {
            let (out, name) = add_machine(&self.src, &self.scene);
            if self.apply("machine-add", Some(out)) {
                self.status = format!("Машина «{name}»: по состоянию на каждую анимацию. Соедините их стрелками");
                self.machine = Some(name);
                self.machine_pick = None;
                self.run = None;
            }
        }
        let Some(m) = self.shown_machine().cloned() else { return };
        vsep(ui);

        // The state: picks it in the graph, and while running jumps straight to it.
        ui.label(RichText::new("Состояние").size(12.0).color(MUTED));
        let current = match (&self.run, self.machine_pick) {
            (Some(r), _) => Some(r.player.state),
            (None, Some(MachineItem::State(i))) => Some(i),
            _ => None,
        };
        let text = current.and_then(|i| m.states.get(i)).map_or("—".to_string(), |s| s.name.clone());
        let mut chosen = None;
        egui::ComboBox::from_id_salt("state-pick").width(130.0).selected_text(text).show_ui(ui, |ui| {
            for (i, s) in m.states.iter().enumerate() {
                let mut label = RichText::new(&s.name);
                if i == 0 {
                    label = label.color(GOOD);
                }
                let resp = ui.selectable_label(current == Some(i), label);
                if resp.on_hover_text(format!("Анимация {}{}", s.anim, if i == 0 { " · начальное состояние" } else { "" })).clicked()
                {
                    chosen = Some(i);
                }
            }
        });
        if let Some(i) = chosen {
            self.machine_pick = Some(MachineItem::State(i));
            if let Some(run) = &mut self.run {
                run.player.jump(i);
            }
        }
        let add = theme::button(ui, "+ Состояние", ButtonKind::Normal).on_hover_text("Новое состояние с одной из анимаций");
        let mut add_anim = None;
        egui::Popup::from_toggle_button_response(&add).show(|ui| {
            ui.label(RichText::new("Какую анимацию играет").size(11.5).color(FAINT));
            for a in &self.scene.doc.animations {
                if ui.selectable_label(false, &a.name).clicked() {
                    add_anim = Some(a.name.clone());
                }
            }
        });
        if let Some(anim) = add_anim {
            let n = m.states.len();
            let name = free_name(&anim, |x| m.state(x).is_some());
            let at = m.states.last().map_or((40.0, 80.0), |s| s.pos.map_or((40.0, 80.0), |(x, y)| (x + 180.0, y)));
            let at = if n % 4 == 0 && n > 0 { (40.0, at.1 + 90.0) } else { at };
            if self.machine_edit("machine-state", |m| m.states.push(State { name: name.clone(), anim, pos: Some(at) })) {
                self.machine_pick = Some(MachineItem::State(n));
            }
        }
        vsep(ui);

        let running = self.run.is_some();
        let icon = if running { Icon::Pause } else { Icon::Play };
        if theme::icon_button(ui, icon, "Запустить машину на холсте (пробел)").clicked() {
            self.toggle_run();
        }
        if theme::button(ui, "Сброс", ButtonKind::Normal).on_hover_text("С начального состояния, входы по умолчанию").clicked() && running
        {
            self.run = Some(Run { player: MachinePlayer::new(&m), flash: None });
        }
        let hint = if running {
            "Играет: переключайте входы справа"
        } else {
            "Переход: тяните от точки на краю состояния"
        };
        ui.label(RichText::new(hint).size(11.5).color(if running { ACCENT } else { FAINT }));
    }

    pub(crate) fn machine_body(&mut self, ui: &mut Ui, body: Rect) {
        let Some(m) = self.shown_machine().cloned() else {
            self.graph.hovered = false;
            ui.painter().text(
                body.left_top() + vec2(12.0, 16.0),
                Align2::LEFT_TOP,
                "Машин пока нет. «Создать машину» сделает состояние для каждой анимации; потом соедините их переходами.",
                theme::ui_font(12.5),
                FAINT,
            );
            return;
        };
        let split = (body.right() - INSPECTOR_W).max(body.left() + 200.0);
        let graph = Rect::from_min_max(body.min, pos2(split, body.bottom()));
        let side = Rect::from_min_max(pos2(split + 1.0, body.top()), body.max);
        ui.painter().line_segment([pos2(split, body.top()), pos2(split, body.bottom())], Stroke::new(1.0, LINE));
        self.machine_graph(ui, graph, &m);
        let mut child = ui.new_child(UiBuilder::new().max_rect(side).layout(Layout::top_down(Align::Min)));
        egui::ScrollArea::vertical().id_salt("machine-side").auto_shrink([false, false]).show(&mut child, |ui| self.machine_inspector(ui, &m));
    }

    fn machine_graph(&mut self, ui: &mut Ui, rect: Rect, m: &Machine) {
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 0.0, theme::BG);
        let origin = rect.min.to_vec2() + self.graph.pan;
        // Dots every 20 px, moving with the view.
        let step = 20.0;
        let off = vec2(self.graph.pan.x.rem_euclid(step), self.graph.pan.y.rem_euclid(step));
        let mut y = rect.top() + off.y;
        while y < rect.bottom() {
            let mut x = rect.left() + off.x;
            while x < rect.right() {
                painter.circle_filled(pos2(x, y), 0.8, theme::DOT);
                x += step;
            }
            y += step;
        }

        let bg = ui.interact(rect, ui.scope_id().with("graph-bg"), Sense::click_and_drag());
        self.graph.hovered = bg.hovered() || bg.contains_pointer();
        if bg.dragged() {
            self.graph.pan += bg.drag_delta();
        }
        if self.graph.hovered {
            let scroll = ui.input(|i| i.smooth_scroll_delta);
            self.graph.pan += scroll;
        }

        // Boxes, with the one being moved at its new place.
        let boxes: Vec<Rect> = (0..m.states.len())
            .map(|i| {
                let at = match &self.graph.drag {
                    Some(GraphDrag::Move { state, at }) if *state == Some(i) => *at,
                    _ => state_at(m, i),
                };
                Rect::from_min_size(at + origin, BOX)
            })
            .collect();
        let any_at = match &self.graph.drag {
            Some(GraphDrag::Move { state: None, at }) => *at,
            _ => m.any_pos.map_or(ANY_AT, |(x, y)| pos2(x as f32, y as f32)),
        };
        let any_box = Rect::from_min_size(any_at + origin, ANY_BOX);
        let box_of = |from: Option<usize>, boxes: &[Rect]| from.map_or(any_box, |i| boxes[i]);

        // Transitions: straight arrows, side by side when several join the same two boxes.
        let run_state = self.run.as_ref().map(|r| r.player.state);
        let flash = self.run.as_ref().and_then(|r| r.flash.map(|f| f.0));
        let mut segments: Vec<(usize, Pos2, Pos2)> = Vec::new();
        for (k, t) in m.transitions.iter().enumerate() {
            let a = box_of(t.from, &boxes);
            let b = boxes[t.to];
            let key = |f: Option<usize>| f.unwrap_or(usize::MAX);
            let (lo, hi) = (key(t.from).min(t.to), key(t.from).max(t.to));
            let same: Vec<usize> =
                m.transitions.iter().enumerate().filter(|(_, u)| (key(u.from).min(u.to), key(u.from).max(u.to)) == (lo, hi)).map(|(j, _)| j).collect();
            let slot = same.iter().position(|&j| j == k).unwrap_or(0) as f32 - (same.len() as f32 - 1.0) / 2.0;
            let picked = self.machine_pick == Some(MachineItem::Transition(k));
            let lit = flash == Some(k);
            let color = if picked {
                SEL
            } else if lit {
                ACCENT
            } else if t.from.is_none() {
                MUTED
            } else {
                theme::mix(FG, theme::BG, 0.65)
            };
            if t.from == Some(t.to) {
                // A transition back into the same state: a small loop over the box.
                let c = pos2(b.right() - 18.0, b.top() - 8.0);
                painter.circle_stroke(c, 9.0, Stroke::new(1.5, color));
                arrow_head(&painter, pos2(c.x + 9.0, c.y + 2.0), vec2(0.0, 1.0), color);
                segments.push((k, c, c));
                continue;
            }
            // The offset follows the low → high direction, so arrows both ways sit apart.
            let (lo_box, hi_box) = if key(t.from) <= t.to { (a, b) } else { (b, a) };
            let normal = (hi_box.center() - lo_box.center()).normalized().rot90() * slot * 16.0;
            let (ca, cb) = (a.center() + normal, b.center() + normal);
            let (pa, pb) = (border_point(a.translate(normal), cb), border_point(b.translate(normal), ca));
            let width = if picked || lit { 2.5 } else { 1.5 };
            painter.line_segment([pa, pb], Stroke::new(width, color));
            arrow_head(&painter, pb, pb - pa, color);
            arrow_head(&painter, pa + (pb - pa) * 0.5, pb - pa, color);
            segments.push((k, pa, pb));
        }

        // The `any` box: transitions from it leave whatever state is playing.
        let resp = ui.interact(any_box, ui.scope_id().with("any"), Sense::drag()).on_hover_text("Переходы отсюда срабатывают из любого состояния");
        painter.rect(any_box, 14.0, if resp.hovered() { RAISE } else { PANEL }, Stroke::new(1.0, LINE), StrokeKind::Inside);
        painter.text(any_box.center(), Align2::CENTER_CENTER, "Любое", theme::ui_font(12.0), MUTED);
        if resp.dragged() {
            self.graph.drag = Some(GraphDrag::Move { state: None, at: any_at + resp.drag_delta() });
        }
        if resp.drag_stopped() {
            self.finish_move();
        }

        // State boxes.
        let mut hovered_box = None;
        for (i, s) in m.states.iter().enumerate() {
            let r = boxes[i];
            let resp = ui.interact(r, ui.scope_id().with(("state", i)), Sense::click_and_drag());
            if resp.hovered() {
                hovered_box = Some(i);
            }
            let picked = self.machine_pick == Some(MachineItem::State(i));
            let active = run_state == Some(i);
            let blending_out = self.run.as_ref().and_then(|r| r.player.prev).is_some_and(|(p, _)| p == i);
            let fill = if active {
                theme::mix(ACCENT, PANEL_2, 0.2)
            } else if resp.hovered() {
                RAISE
            } else {
                PANEL_2
            };
            let border = if picked {
                Stroke::new(2.0, SEL)
            } else if active {
                Stroke::new(2.0, ACCENT)
            } else if blending_out {
                Stroke::new(1.5, ACCENT.gamma_multiply(0.5))
            } else {
                Stroke::new(1.0, LINE)
            };
            painter.rect(r, 7.0, fill, border, StrokeKind::Inside);
            if i == 0 {
                painter.rect_filled(Rect::from_min_size(r.min + vec2(1.0, 6.0), vec2(3.0, r.height() - 12.0)), 1.5, GOOD);
                painter.text(r.left_top() + vec2(2.0, -3.0), Align2::LEFT_BOTTOM, "СТАРТ", theme::semibold(9.5), GOOD);
            }
            painter.text(r.left_top() + vec2(12.0, 8.0), Align2::LEFT_TOP, &s.name, theme::semibold(13.0), FG);
            let anim = self.scene.doc.animation(&s.anim);
            let detail = match anim {
                Some(a) => format!("{} · {} с{}", s.anim, motif_script::fmt_num(a.duration), if a.looping { " · повтор" } else { "" }),
                None => s.anim.clone(),
            };
            painter.text(r.left_bottom() + vec2(12.0, -8.0), Align2::LEFT_BOTTOM, detail, theme::mono(10.5), MUTED);
            if active
                && let (Some(a), Some(run)) = (anim, &self.run)
                && a.duration > 0.0
            {
                let u = (a.local_time(run.player.time) / a.duration) as f32;
                let bar = Rect::from_min_size(r.left_bottom() + vec2(6.0, -4.0), vec2((r.width() - 12.0) * u, 2.0));
                painter.rect_filled(bar, 1.0, ACCENT);
            }
            if resp.clicked() {
                self.machine_pick = Some(MachineItem::State(i));
                if let Some(run) = &mut self.run {
                    run.player.jump(i);
                }
            }
            if resp.double_clicked() {
                self.open_state(i);
                return;
            }
            if resp.dragged() {
                self.graph.drag = Some(GraphDrag::Move { state: Some(i), at: r.min - origin + resp.drag_delta() });
                self.machine_pick = Some(MachineItem::State(i));
            }
            if resp.drag_stopped() {
                self.finish_move();
            }
        }

        // Ports: pull from one to another box to add a transition.
        let ports: Vec<(Option<usize>, Rect)> = std::iter::once((None, any_box)).chain(boxes.iter().enumerate().map(|(i, r)| (Some(i), *r))).collect();
        for (from, r) in &ports {
            let c = pos2(r.right(), r.center().y);
            let resp = ui.interact(Rect::from_center_size(c, vec2(14.0, 14.0)), ui.scope_id().with(("port", from.map_or(-1, |i| i as i64))), Sense::drag());
            let show =
                resp.hovered() || resp.dragged() || hovered_box == *from && from.is_some() || r.contains(ui.ctx().pointer_hover_pos().unwrap_or(Pos2::ZERO));
            if show {
                painter.circle(c, 5.0, if resp.hovered() || resp.dragged() { ACCENT } else { PANEL }, Stroke::new(1.5, ACCENT));
            }
            if resp.hovered() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::Crosshair);
            }
            if resp.dragged()
                && let Some(p) = resp.interact_pointer_pos()
            {
                self.graph.drag = Some(GraphDrag::Link { from: *from, to: p });
            }
            if resp.drag_stopped()
                && let Some(GraphDrag::Link { from, to }) = self.graph.drag.take()
            {
                let target = boxes.iter().position(|b| b.contains(to));
                match target {
                    Some(to) if Some(to) != from => {
                        let n = m.transitions.len();
                        if self.machine_edit("machine-link", |m| m.transitions.push(Transition { from, to, conds: Vec::new(), at_end: false, blend: 0.0 })) {
                            self.machine_pick = Some(MachineItem::Transition(n));
                            self.status = "Переход добавлен: задайте ему условие справа".into();
                        }
                    }
                    _ => self.status = "Отпустите стрелку над другим состоянием".into(),
                }
            }
        }
        if let Some(GraphDrag::Link { from, to }) = &self.graph.drag {
            let a = box_of(*from, &boxes);
            let start = pos2(a.right(), a.center().y);
            painter.line_segment([start, *to], Stroke::new(2.0, ACCENT));
            arrow_head(&painter, *to, *to - start, ACCENT);
        }

        // A click on empty space picks the nearest arrow, or clears the pick.
        if bg.clicked()
            && let Some(p) = bg.interact_pointer_pos()
        {
            let near = segments
                .iter()
                .map(|(k, a, b)| (*k, if a == b { ((p - *a).length() - 9.0).abs() } else { dist_to_segment(p, *a, *b) }))
                .filter(|(_, d)| *d < 7.0)
                .min_by(|a, b| a.1.total_cmp(&b.1));
            self.machine_pick = near.map(|(k, _)| MachineItem::Transition(k));
        }
        if bg.hovered()
            && let Some(p) = ui.ctx().pointer_hover_pos()
            && segments.iter().any(|(_, a, b)| a != b && dist_to_segment(p, *a, *b) < 7.0)
        {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
    }

    /// Writes the place of the box that was being dragged, snapped to 10 px.
    fn finish_move(&mut self) {
        let Some(GraphDrag::Move { state, at }) = self.graph.drag.take() else { return };
        let p = (((at.x / 10.0).round() * 10.0) as f64, ((at.y / 10.0).round() * 10.0) as f64);
        self.machine_edit("machine-move", |m| match state {
            Some(i) => m.states[i].pos = Some(p),
            None => m.any_pos = Some(p),
        });
    }

    fn machine_inspector(&mut self, ui: &mut Ui, m: &Machine) {
        match self.machine_pick {
            Some(MachineItem::State(i)) if i < m.states.len() => {
                self.state_section(ui, m, i);
                self.inputs_section(ui, m);
            }
            Some(MachineItem::Transition(k)) if k < m.transitions.len() => {
                self.transition_section(ui, m, k);
                self.inputs_section(ui, m);
            }
            _ => {
                self.inputs_section(ui, m);
                section(ui, |ui| {
                    theme::caption(ui, &format!("Машина {}", m.name));
                    ui.label(
                        RichText::new(
                            "Состояние играет свою анимацию. Переход срабатывает, когда выполнены его условия. Щёлкните состояние или стрелку, чтобы их настроить; двойной щелчок открывает анимацию состояния на таймлайне.",
                        )
                        .size(11.5)
                        .color(FAINT),
                    );
                    if theme::button(ui, "Удалить машину", ButtonKind::Normal).clicked() && self.can_edit() {
                        let out = delete_machine(&self.src, &self.scene, &m.name);
                        if self.apply("machine-remove", out) {
                            self.machine = None;
                            self.run = None;
                        }
                    }
                });
            }
        }
    }

    fn inputs_section(&mut self, ui: &mut Ui, m: &Machine) {
        let mut add = None;
        let mut remove = None;
        let mut rename = None;
        let mut default = None;
        section(ui, |ui| {
            theme::caption(ui, if self.run.is_some() { "Входы · сейчас" } else { "Входы" });
            if m.inputs.is_empty() {
                ui.label(RichText::new("Входы задаёт игра: флаг (наведён ли курсор), число (скорость), триггер (прыжок).").size(11.5).color(FAINT));
            }
            for (i, input) in m.inputs.iter().enumerate() {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;
                    if let Some(n) = name_edit(ui, ("input-name", &m.name, i), &input.name, 96.0) {
                        rename = Some((i, n));
                    }
                    ui.label(RichText::new(kind_label(input.kind)).size(11.0).color(FAINT));
                    let run = self.run.as_mut();
                    match (input.kind, run) {
                        (InputKind::Bool, Some(r)) => {
                            let mut on = r.player.values.get(i).is_some_and(|v| *v != 0.0);
                            if ui.checkbox(&mut on, "").changed() {
                                r.player.values[i] = f64::from(u8::from(on));
                            }
                        }
                        (InputKind::Number, Some(r)) => {
                            let mut v = r.player.values.get(i).copied().unwrap_or(0.0);
                            if ui.add(egui::DragValue::new(&mut v).speed(0.05).max_decimals(2)).changed() {
                                r.player.values[i] = v;
                            }
                        }
                        (InputKind::Trigger, Some(r)) => {
                            if theme::button(ui, "Сработать", ButtonKind::On).clicked() {
                                r.player.values[i] = 1.0;
                            }
                        }
                        (InputKind::Bool, None) => {
                            let mut on = input.default != 0.0;
                            if ui.checkbox(&mut on, "").on_hover_text("Значение в начале").changed() {
                                default = Some((i, f64::from(u8::from(on))));
                            }
                        }
                        (InputKind::Number, None) => {
                            let mut v = input.default;
                            if ui.add(egui::DragValue::new(&mut v).speed(0.05).max_decimals(2)).on_hover_text("Значение в начале").changed() {
                                default = Some((i, (v * 100.0).round() / 100.0));
                            }
                        }
                        (InputKind::Trigger, None) => {}
                    }
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui.add(egui::Button::new(RichText::new("×").color(MUTED)).frame(false)).on_hover_text("Удалить вход и условия с ним").clicked()
                        {
                            remove = Some(i);
                        }
                    });
                });
            }
            ui.horizontal(|ui| {
                for (kind, text) in [(InputKind::Bool, "+ Флаг"), (InputKind::Number, "+ Число"), (InputKind::Trigger, "+ Триггер")] {
                    if theme::button(ui, text, ButtonKind::Normal).clicked() {
                        add = Some(kind);
                    }
                }
            });
        });
        if let Some(kind) = add {
            let base = match kind {
                InputKind::Bool => "flag",
                InputKind::Number => "value",
                InputKind::Trigger => "fire",
            };
            let name = free_name(base, |n| m.input(n).is_some());
            self.machine_edit("machine-input", |m| m.inputs.push(Input { name, kind, default: 0.0 }));
        }
        if let Some(i) = remove {
            self.machine_pick = Some(MachineItem::Input(i));
            self.delete_machine_pick();
        }
        if let Some((i, v)) = default {
            self.machine_edit("machine-default", |m| m.inputs[i].default = v);
        }
        if let Some((i, new)) = rename {
            if !valid_name(&new) || m.input(&new).is_some() {
                self.status = format!("«{new}» не подходит: латиница или кириллица, цифры и _, без пробелов, не занятое имя");
            } else {
                let old = m.inputs[i].name.clone();
                self.machine_edit("machine-rename", |m| {
                    m.inputs[i].name = new.clone();
                    for c in m.transitions.iter_mut().flat_map(|t| t.conds.iter_mut()) {
                        if c.input == old {
                            c.input = new.clone();
                        }
                    }
                });
            }
        }
    }

    fn state_section(&mut self, ui: &mut Ui, m: &Machine, i: usize) {
        let s = &m.states[i];
        let mut rename = None;
        let mut anim = None;
        let mut entry = false;
        let mut open = false;
        let mut link = None;
        let mut delete = false;
        section(ui, |ui| {
            theme::caption(ui, if i == 0 { "Состояние · начальное" } else { "Состояние" });
            ui.horizontal(|ui| {
                ui.label(RichText::new("Имя").size(12.0).color(MUTED));
                rename = name_edit(ui, ("state-name", &m.name, i), &s.name, 150.0);
            });
            ui.horizontal(|ui| {
                ui.label(RichText::new("Анимация").size(12.0).color(MUTED));
                egui::ComboBox::from_id_salt(("state-anim", i)).width(150.0).selected_text(&s.anim).show_ui(ui, |ui| {
                    for a in &self.scene.doc.animations {
                        if ui.selectable_label(a.name == s.anim, &a.name).clicked() {
                            anim = Some(a.name.clone());
                        }
                    }
                });
            });
            ui.horizontal_wrapped(|ui| {
                open = theme::button(ui, "Открыть на таймлайне", ButtonKind::Normal).on_hover_text("Двойной щелчок по состоянию делает то же").clicked();
                if i != 0 {
                    entry = theme::button(ui, "Сделать начальным", ButtonKind::Normal).clicked();
                }
                let resp = theme::button(ui, "+ Переход в…", ButtonKind::Normal);
                egui::Popup::from_toggle_button_response(&resp).show(|ui| {
                    for (j, other) in m.states.iter().enumerate().filter(|(j, _)| *j != i) {
                        if ui.selectable_label(false, &other.name).clicked() {
                            link = Some(j);
                        }
                    }
                });
                delete = theme::button(ui, "Удалить", ButtonKind::Normal).on_hover_text("Вместе с его переходами (Delete над графом)").clicked();
            });
            let out: Vec<String> = m.transitions.iter().filter(|t| t.from == Some(i)).map(|t| format!("→ {}", m.states[t.to].name)).collect();
            if !out.is_empty() {
                ui.label(RichText::new(format!("Переходы: {}", out.join(", "))).size(11.5).color(FAINT));
            }
        });
        if let Some(new) = rename {
            if !valid_name(&new) || m.state(&new).is_some() {
                self.status = format!("«{new}» не подходит: буквы, цифры и _, без пробелов, не занятое имя");
            } else {
                self.machine_edit("machine-rename", |m| m.states[i].name = new);
            }
        }
        if let Some(a) = anim {
            self.machine_edit("machine-anim", |m| m.states[i].anim = a);
        }
        if entry && self.can_edit() {
            let out = make_entry(&self.src, &self.scene, &m.name, i);
            if self.apply("machine-entry", out) {
                self.machine_pick = Some(MachineItem::State(0));
            }
        }
        if let Some(to) = link {
            let n = m.transitions.len();
            if self.machine_edit("machine-link", |m| m.transitions.push(Transition { from: Some(i), to, conds: Vec::new(), at_end: false, blend: 0.0 })) {
                self.machine_pick = Some(MachineItem::Transition(n));
            }
        }
        if delete {
            self.delete_machine_pick();
        }
        if open {
            self.open_state(i);
        }
    }

    fn transition_section(&mut self, ui: &mut Ui, m: &Machine, k: usize) {
        let t = &m.transitions[k];
        let mut new = t.clone();
        let mut delete = false;
        section(ui, |ui| {
            theme::caption(ui, "Переход");
            ui.horizontal(|ui| {
                ui.label(RichText::new("Из").size(12.0).color(MUTED));
                let name = |f: Option<usize>| f.map_or("Любое".to_string(), |i| m.states[i].name.clone());
                egui::ComboBox::from_id_salt(("tr-from", k)).width(96.0).selected_text(name(new.from)).show_ui(ui, |ui| {
                    for f in std::iter::once(None).chain((0..m.states.len()).map(Some)) {
                        if ui.selectable_label(new.from == f, name(f)).clicked() {
                            new.from = f;
                        }
                    }
                });
                ui.label(RichText::new("в").size(12.0).color(MUTED));
                egui::ComboBox::from_id_salt(("tr-to", k)).width(96.0).selected_text(&m.states[new.to].name).show_ui(ui, |ui| {
                    for (j, s) in m.states.iter().enumerate() {
                        if ui.selectable_label(new.to == j, &s.name).clicked() {
                            new.to = j;
                        }
                    }
                });
            });
            ui.label(RichText::new("Условия — все сразу").size(12.0).color(MUTED));
            let mut remove = None;
            for (ci, c) in new.conds.iter_mut().enumerate() {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;
                    egui::ComboBox::from_id_salt(("cond-in", k, ci)).width(84.0).selected_text(&c.input).show_ui(ui, |ui| {
                        for input in &m.inputs {
                            if ui.selectable_label(input.name == c.input, &input.name).clicked() && input.name != c.input {
                                *c = default_cond(input);
                            }
                        }
                    });
                    match m.input(&c.input).map(|i| m.inputs[i].kind) {
                        Some(InputKind::Bool) => {
                            egui::ComboBox::from_id_salt(("cond-op", k, ci)).width(84.0).selected_text(op_label(c.op)).show_ui(ui, |ui| {
                                for op in [CondOp::Is, CondOp::Not] {
                                    ui.selectable_value(&mut c.op, op, op_label(op));
                                }
                            });
                        }
                        Some(InputKind::Number) => {
                            egui::ComboBox::from_id_salt(("cond-op", k, ci)).width(44.0).selected_text(op_label(c.op)).show_ui(ui, |ui| {
                                for op in CondOp::COMPARE {
                                    ui.selectable_value(&mut c.op, op, op_label(op));
                                }
                            });
                            ui.add(egui::DragValue::new(&mut c.value).speed(0.05).max_decimals(2));
                            c.value = (c.value * 100.0).round() / 100.0;
                        }
                        _ => {
                            ui.label(RichText::new("сработал").size(12.0).color(FG));
                        }
                    }
                    if ui.add(egui::Button::new(RichText::new("×").color(MUTED)).frame(false)).on_hover_text("Убрать условие").clicked() {
                        remove = Some(ci);
                    }
                });
            }
            if let Some(ci) = remove {
                new.conds.remove(ci);
            }
            ui.horizontal(|ui| {
                ui.add_enabled_ui(!m.inputs.is_empty(), |ui| {
                    if theme::button(ui, "+ Условие", ButtonKind::Normal).on_disabled_hover_text("Сначала добавьте вход выше").clicked()
                    {
                        new.conds.push(default_cond(&m.inputs[0]));
                    }
                });
            });
            ui.checkbox(&mut new.at_end, RichText::new("Дождаться конца анимации").size(12.5).color(FG));
            ui.horizontal(|ui| {
                ui.label(RichText::new("Смешивание").size(12.0).color(MUTED));
                ui.add(egui::DragValue::new(&mut new.blend).speed(0.01).range(0.0..=10.0).max_decimals(2).suffix(" с"));
                new.blend = (new.blend * 100.0).round() / 100.0;
            });
            let note = if new.conds.is_empty() && !new.at_end {
                "Без условий переход сработает сразу же."
            } else {
                "Смешивание плавно перетекает из старой анимации в новую."
            };
            ui.label(RichText::new(note).size(11.5).color(FAINT));
            delete = theme::button(ui, "Удалить переход", ButtonKind::Normal).clicked();
        });
        if delete {
            self.delete_machine_pick();
        } else if new != *t {
            self.machine_edit(&format!("machine-tr-{k}"), |m| m.transitions[k] = new);
        }
    }
}

fn default_cond(input: &Input) -> Cond {
    let op = if input.kind == InputKind::Number { CondOp::Gt } else { CondOp::Is };
    Cond { input: input.name.clone(), op, value: 0.0 }
}

pub(crate) fn vsep(ui: &mut Ui) {
    let (r, _) = ui.allocate_exact_size(vec2(1.0, 20.0), Sense::hover());
    ui.painter().rect_filled(r, 0.0, LINE);
}
