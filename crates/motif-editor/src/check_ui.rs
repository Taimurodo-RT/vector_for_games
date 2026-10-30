//! The interface check: «▶ Проверить» makes the `ui` pieces of the board work under the mouse
//! as their Godot controls will, and the right panel lists them with their state and value.

use crate::app::{App, section};
use crate::theme::{FAINT, FG, MUTED};
use egui::{RichText, Ui};
use motif_core::{UiCheck, UiRole};

/// What a piece works as, in words.
fn role_label(r: UiRole) -> &'static str {
    match r {
        UiRole::Auto | UiRole::Button => "кнопка",
        UiRole::Panel => "панель",
        UiRole::Progress => "полоса",
        UiRole::Check => "флажок",
        UiRole::Slider => "слайдер",
        UiRole::Scroll => "прокрутка",
        UiRole::Label => "надпись",
        UiRole::Switch => "тумблер",
        UiRole::Input => "поле ввода",
        UiRole::Option => "список",
    }
}

impl App {
    /// Whether the board has pieces the check can work.
    pub(crate) fn has_ui(&self) -> bool {
        self.scene.doc.nodes.iter().any(|n| n.ui && n.part.is_none())
    }

    /// Starts or stops the check, and with it the machine that listens to the pointer.
    pub(crate) fn toggle_check(&mut self) {
        if self.check.is_some() || self.run.is_some() {
            self.check = None;
            self.run = None;
            // Back where the check started: the clip and frame in the workspace that showed them.
            if let Some(space) = self.check_from.take() {
                self.set_workspace(space);
            }
            self.status = "Проверка остановлена".into();
            return;
        }
        self.clear_draft_pose();
        if self.has_ui() {
            let from = self.workspace();
            self.set_mode_animate(false);
            self.check_from = Some(from);
        }
        if self.shown_machine().is_some_and(|m| !m.listeners.is_empty()) {
            self.toggle_run();
        }
        if self.has_ui() {
            self.check = Some(UiCheck::new(&self.scene.doc));
            self.status = "Проверка: наводите, нажимайте и тяните мышью детали на холсте — так они будут работать в Godot".into();
        }
    }

    /// The check after the script changed: the same values on the pieces that are still there.
    pub(crate) fn refresh_check(&mut self) {
        let Some(old) = self.check.take() else { return };
        let mut new = UiCheck::new(&self.scene.doc);
        for c in &mut new.controls {
            if let Some(o) = old.control(&c.node).filter(|o| o.role == c.role) {
                c.checked = o.checked;
                c.value = o.value;
                c.disabled = o.disabled;
                c.clicks = o.clicks;
            }
        }
        self.check = Some(new);
    }

    /// Passes the pointer on the canvas to the check.
    pub(crate) fn check_pointer(&mut self, ui: &Ui, resp: &egui::Response, doc: &motif_core::Document, at: Option<motif_core::kurbo::Point>) {
        let Some(check) = &mut self.check else { return };
        let on_canvas = resp.hovered() || resp.dragged() || resp.is_pointer_button_down_on();
        let at = at.filter(|_| on_canvas);
        let (down, quick) = ui.input(|i| (i.pointer.primary_down(), i.pointer.primary_pressed() && i.pointer.primary_released()));
        let down = down && (resp.is_pointer_button_down_on() || resp.dragged());
        // A click quicker than a frame: the press is passed before the release.
        let mut said = if quick && !down { check.pointer(doc, at, true) } else { None };
        said = check.pointer(doc, at, down).or(said);
        if let Some((node, said)) = said {
            let name = self.scene.doc.node(&node).map_or(node.clone(), |n| n.display_name().to_string());
            self.status = format!("{name}: {said}");
        }
        if at.is_some_and(|p| check.over_control(doc, p)) {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
    }

    /// The right panel while the check runs: every piece, what it shows, and its value by hand.
    pub(crate) fn check_panel(&mut self, ui: &mut Ui) {
        let Some(check) = &mut self.check else { return };
        section(ui, |ui| {
            ui.label(RichText::new("Проверка интерфейса").font(crate::theme::semibold(14.0)).color(FG));
            ui.label(
                RichText::new("Детали работают как в Godot: кнопки наводятся и нажимаются, флажки щёлкаются, слайдеры и прокрутка тянутся за ручку. Значения можно задать и здесь.")
                    .size(11.5)
                    .color(MUTED),
            );
        });
        if check.controls.is_empty() {
            section(ui, |ui| {
                ui.label(RichText::new("На холсте нет деталей ui").size(12.0).color(FAINT));
            });
        }
        let names: Vec<String> = check.controls.iter().map(|c| self.scene.doc.node(&c.node).map_or(c.node.clone(), |n| n.display_name().to_string())).collect();
        for (c, name) in check.controls.iter_mut().zip(names) {
            section(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(name).font(crate::theme::semibold(12.5)).color(FG));
                    ui.label(RichText::new(if c.radio.is_some() { "радио" } else { role_label(c.role) }).size(11.5).color(FAINT));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let state = if c.interactive() { c.state_word() } else { "" };
                        ui.label(RichText::new(state).size(11.5).color(MUTED));
                    });
                });
                if let Some(on) = &mut c.checked {
                    ui.checkbox(on, "галочка");
                }
                if let Some(v) = &mut c.value {
                    ui.add(egui::Slider::new(v, 0.0..=1.0).custom_formatter(|v, _| format!("{:.0}%", v * 100.0)).text("значение"));
                }
                if c.interactive() {
                    ui.horizontal(|ui| {
                        ui.checkbox(&mut c.disabled, "недоступно");
                        if c.clicks > 0 {
                            ui.label(RichText::new(format!("нажатий: {}", c.clicks)).size(11.5).color(FAINT));
                        }
                    });
                }
            });
        }
    }
}
