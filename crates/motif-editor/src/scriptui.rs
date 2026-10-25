//! The collapsed script strip and the syntax cheat sheet that opens over the interface.

use crate::app::App;
use crate::theme::{self, FAINT, FG, Icon, LINE, MUTED};
use egui::{Align2, Rect, RichText, Sense, Stroke, pos2, vec2};

/// The cheat sheet: a heading, then example lines with what they do.
const SYNTAX: &[(&str, &[(&str, &str)])] = &[
    (
        "Сцена",
        &[
            ("artboard \"Меню\" 480x360 bg #fff4e4", "артборд: имя, размер, фон"),
            ("backdrop \"эскиз.png\" at 0 0 scale 1 opacity 0.4", "подложка-референс, только в редакторе"),
            ("let ink = #1b1d24", "переменная: число или цвет"),
            ("// комментарий", "до конца строки"),
        ],
    ),
    (
        "Объекты",
        &[
            ("rect btn \"Кнопка\" at 240 300 size 150 44", "тип, id, имя, центр, размер"),
            ("ellipse · star · polygon · path · group", "типы фигур"),
            ("path p d \"M 0 0 L 40 0 C …  Z\"", "контур в SVG-записи вокруг at"),
            ("group g at 40 40 { … }", "вложенные объекты"),
            ("for i in 0..5 { ellipse c{i} at {i*28} 0 }", "цикл, выражения в {}"),
        ],
    ),
    (
        "Свойства",
        &[
            ("r 8 · r 8 8 0 0", "скругление, по углам"),
            ("points 5 · inner 0.5", "лучи звезды, внутренний радиус"),
            ("rot 15 · scale 1.2 [0.8] · skew 10 [0]", "поворот, масштаб, наклон"),
            ("pivot 0 -20", "точка вращения (transform-origin)"),
            ("opacity 0.5 · hidden", "прозрачность, скрыть"),
            ("fill #ff5a36 · fill none · evenodd", "заливка; evenodd — дырки по чётности"),
            ("gradient linear 90 #a #b · gradient radial #a #b", "градиент"),
            ("stroke #000 2 · dash 6 4 · cap round · join round", "обводка, пунктир, концы, углы"),
            ("shadow 0 4 8 #00000066", "тень: сдвиг x y, размытие, цвет"),
            ("blend multiply", "режим наложения"),
            ("group g clip { маска … }", "первый объект группы — маска"),
        ],
    ),
    (
        "Анимация",
        &[
            ("anim Idle 1.2s loop {", "имя, длительность, повтор"),
            ("  ball.y 0: 120 ease-in | 0.6: 278", "время: значение кривая | …"),
            ("}", ""),
            ("x y w h rot sx sy opacity fill stroke stroke_w", "что можно анимировать"),
            ("override { coin2.y = 12 }", "правки поверх формул и циклов"),
        ],
    ),
    (
        "Кривые",
        &[
            ("linear · ease · ease-in · ease-out · ease-in-out", "базовые (по умолчанию ease-in-out)"),
            ("ease-in-sine … ease-in-out-bounce", "30 функций easings.net: sine quad cubic quart quint expo circ back elastic bounce"),
            ("hold · spring · steps 4", "ступенька, пружина, N шагов"),
            ("cubic 0.3 0 0.6 1.2", "своя кривая: ручки Безье"),
        ],
    ),
];

impl App {
    /// The narrow strip shown instead of the script panel when it is collapsed.
    pub(crate) fn script_strip(&mut self, ui: &mut egui::Ui) {
        let r = ui.max_rect();
        let resp = ui.interact(r, ui.scope_id().with("strip"), Sense::click()).on_hover_text("Развернуть скрипт (Ctrl+E)");
        if resp.hovered() {
            ui.painter().rect_filled(r, 0.0, theme::PANEL_2);
        }
        let color = if resp.hovered() { FG } else { MUTED };
        theme::paint_icon(ui.painter(), Rect::from_center_size(pos2(r.center().x, r.top() + 16.0), vec2(16.0, 16.0)), Icon::ExpandRight, color);
        // The title written top to bottom, one letter per line.
        for (k, ch) in "СКРИПТ".chars().enumerate() {
            ui.painter().text(pos2(r.center().x, r.top() + 44.0 + k as f32 * 14.0), Align2::CENTER_CENTER, ch, theme::semibold(11.0), color);
        }
        if !self.errors.is_empty() {
            ui.painter().circle_filled(pos2(r.center().x, r.top() + 44.0 + 6.0 * 14.0 + 8.0), 4.0, theme::DANGER);
        }
        if resp.clicked() {
            self.show_script = true;
        }
    }

    /// The syntax cheat sheet: a floating window over the interface, closed with × or F1.
    pub(crate) fn syntax_window(&mut self, ctx: &egui::Context) {
        if !self.show_syntax {
            return;
        }
        let mut open = true;
        egui::Window::new("Синтаксис сцены")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_pos(pos2(320.0, 70.0))
            .default_width(560.0)
            .max_height(ctx.content_rect().height() - 120.0)
            .default_height(ctx.content_rect().height() - 140.0)
            .vscroll(true)
            .show(ctx, |ui| {
                ui.label(
                    RichText::new("Одна строка — один объект. Свойства пишутся словом и значениями, порядок любой. F1 — закрыть.").size(12.0).color(MUTED),
                );
                for (title, rows) in SYNTAX {
                    ui.add_space(8.0);
                    theme::caption(ui, title);
                    egui::Grid::new(("syntax", *title)).num_columns(2).spacing([14.0, 4.0]).min_col_width(300.0).show(ui, |ui| {
                        for (code, what) in rows.iter() {
                            let job = crate::highlight::highlight(code, &[], None);
                            ui.label(job);
                            ui.label(RichText::new(*what).size(12.0).color(FAINT));
                            ui.end_row();
                        }
                    });
                }
                ui.add_space(6.0);
                let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 1.0), Sense::hover());
                ui.painter().line_segment([r.left_center(), r.right_center()], Stroke::new(1.0, LINE));
                ui.label(RichText::new("Полное описание — README в папке программы.").size(11.5).color(FAINT));
            });
        if !open {
            self.show_syntax = false;
        }
    }
}
