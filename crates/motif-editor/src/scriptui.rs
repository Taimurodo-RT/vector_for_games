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
            ("artboard \"Кнопка\" 240x100 cell 1 0", "ещё холст: до следующего artboard; cell — место в сетке"),
            ("let ink = #10131d  // выше первого холста", "общее для всех холстов файла"),
            ("backdrop \"эскиз.png\" at 0 0 scale 1 opacity 0.4", "подложка-референс, только в редакторе"),
            ("let ink = #1b1d24", "переменная: число или цвет"),
            ("fill ink · stroke ink 4", "цвет по имени: палитра, раздел «Палитра»"),
            ("// комментарий", "до конца строки"),
        ],
    ),
    (
        "Объекты",
        &[
            ("rect btn \"Кнопка\" at 240 300 size 150 44", "тип, id, имя, центр, размер"),
            ("ellipse · star · polygon · path · text · group", "типы фигур"),
            ("text t at 240 60 content \"Привет\" font-size 48", "текст: буквы-контуры, \\n — новая строка"),
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
            ("locked", "заблокировать: холст не выбирает объект"),
            ("fill #ff5a36 · fill none · evenodd", "заливка; evenodd — дырки по чётности"),
            ("gradient linear 90 #a #b · gradient radial #a #b", "градиент"),
            ("stroke #000 2 · dash 6 4 · cap round · join round", "обводка, пунктир, концы, углы"),
            ("shadow 0 4 8 #00000066", "тень: сдвиг x y, размытие, цвет"),
            ("inset 0 4 6 #000000aa", "внутренняя тень: вдавленное поле, ямка"),
            ("glow 12 #ffd34d · glow 16 4 #6f93ff", "свечение: размер, [разлив], цвет"),
            ("stroke-gradient linear 90 #a #b", "обводка градиентом"),
            ("noise 0.15 · noise 0.2 2", "шум: сила 0…1, [крупинка]"),
            ("blur 6", "размытие объекта или целой группы"),
            ("elevation 12 · artboard … light 120", "высота над артбордом; куда падают тени"),
            ("artboard … ui · artboard … game", "режим: интерфейс или игровая графика"),
            ("blend multiply", "режим наложения"),
            ("outline #1b1d24 12 #fff 6", "обводки слоями под фигурой"),
            ("font bold · font \"мой.ttf\" · align left", "шрифт текста, выравнивание"),
            ("letter-spacing 2 · line-height 1.2", "межбуквенный и межстрочный интервал"),
            ("reveal 0..1 · wave 6 · phase 0..1", "анимация букв: появление, волна"),
            ("group g clip { маска … }", "первый объект группы — маска"),
            ("group m stack column center gap 8 pad 12 {", "стек: ряд или столбик, как flex"),
            ("  rect bg fit · anchor right bottom", "фон во весь стек; at от края артборда"),
        ],
    ),
    (
        "Символы",
        &[
            ("symbol Coin size=16 color=#ffd23f {", "символ с параметрами по умолчанию"),
            ("  ellipse body size {size} fill color", "параметры — как переменные"),
            ("}", ""),
            ("use Coin c1 at 40 40 size=24", "копия: свой id, место, параметры"),
            ("c1-body.rot", "объект копии: id копии - id в символе"),
        ],
    ),
    (
        "Анимация",
        &[
            ("anim Idle 1.2s loop {", "имя, длительность, повтор"),
            ("  ball.y 0: 120 ease-in | 0.6: 278", "время: значение кривая | …"),
            ("  ball.w 0: 70 cubic 0.2 0 0.4 1.3 | 1: 90", "кривая из ручек «Графиков»; > 1 — перелёт"),
            ("  event step 0.1 0.3", "событие для игры в эти секунды"),
            ("}", "Shift, рамка, Alt+тянуть, Ctrl+C/V — несколько ключей"),
            ("«Путь» на таймлайне", "траектория: тяните кружок ключа, клик по линии — время"),
            ("x y w h rot sx sy opacity fill stroke stroke_w", "что можно анимировать"),
            ("blur elevation shadow-x shadow-y shadow-blur glow inset-x inset-y inset-blur noise", "и эффекты тоже"),
            ("ellipse blob … morph star heart", "перетекает в контуры целей"),
            ("blob.morph 0: 0 | 1: 1 | 2: 2", "0 — сама, 1 — первая цель, 2 — вторая"),
            ("override { coin2.y = 12 }", "правки поверх формул и циклов"),
        ],
    ),
    (
        "Кости",
        &[
            ("bone arm at 100 80 len 60 rot 30 {", "кость: сустав, длина, угол"),
            ("  bone hand at 60 0 len 40 ik target flip", "дочерняя на кончике; IK к target"),
            ("  ellipse fist at 40 0 size 20", "объект внутри едет с костью"),
            ("}", "chain N — сколько костей гнёт IK"),
            ("rect sleeve … skin arm hand", "фигура гнётся костями"),
            ("skin-sharp 2", "мягкий изгиб; 4 — сустав"),
            ("weights sleeve { arm 18 1 at 0 0 20 0 }", "мазки кисти весов: кость, радиус, сила"),
        ],
    ),
    (
        "2.5D и 3D",
        &[
            ("ellipse eye … on head lat 12 lon -22", "объект на шаре"),
            ("alt 50", "высота над шаром"),
            ("wrap", "изгибается по поверхности"),
            ("head.yaw 0: 0 | 1: 50", "поворот шара: yaw, pitch"),
            ("roll 0.3 0.05", "катится по шару: сцепление, затухание"),
            ("rect cube … extrude 90 yaw 30 pitch 25", "3D-тело: глубина, поворот в пространстве"),
            ("ellipse cone … extrude 120 0", "сужение задней грани: 0 — конус"),
        ],
    ),
    (
        "Кисть",
        &[
            ("rough 3 40", "контур «от руки»: сила, длина волны"),
            ("boil 8", "линия перерисовывается 8 раз в секунду"),
            ("hatch 45 6 1.5 #2a2230 cross", "штриховка: угол, шаг, толщина, цвет"),
            ("grain 0.15 2 #ffffff", "зерно: доля, размер точки, цвет"),
            ("bands 3", "градиенты и свет 3D-тел ступенями"),
            ("seed 2", "другой рисунок той же кисти"),
        ],
    ),
    (
        "Интерфейс",
        &[
            ("group button … ui { … }", "деталь UI-набора для Godot"),
            ("ui button · panel · progress · check · switch · slider · scroll · label · input · option", "чем деталь станет в Godot"),
            ("part fill · track · grabber · checked · arrow · text", "часть такой детали; part text рисует Godot"),
            ("ui check radio difficulty", "радио-кнопка: в группе включена одна"),
            ("slice 20 · slice 16 12 · slice 22 46 22 22", "поля 9-slice, которые не тянутся"),
            ("anim Hover · Press · Disabled · Focus", "состояния: последний кадр анимации"),
            ("▶ Проверить", "детали работают мышью, как в Godot; значения справа"),
            ("ui slider + anim Value", "слайдер-рисунок: значение 0..1 проигрывает Value"),
            ("dial across · up · turn 270 · value 0.4", "как его тянуть и где он стоит"),
        ],
    ),
    (
        "Физика",
        &[
            ("spring 3 0.3", "ключи на пружине: частота, затухание"),
            ("bone tail … jiggle 2.5 0.4", "кость покачивается вслед"),
            ("squash 0.6 6 0.3", "тянется на скорости, сплющивается при ударе"),
            ("emit 40 life 1 speed 160", "частицы: в секунду, жизнь, скорость"),
            ("angle -90 spread 50 burst 24", "направление, конус, залп"),
            ("drag 0.5 fade 0 grow 0.2 spin 360", "торможение, угасание, размер, вращение"),
            ("path cord … rope 1 0.02", "верёвка: жёсткость, затухание"),
            ("pin start end · wind 200", "закреплённые концы, свой ветер"),
            ("tie hand · hang cord 0.5", "конец держит объект · объект висит на верёвке"),
            ("ellipse link … links chain", "звенья цепи вдоль верёвки"),
            ("rect flag … cloth pin left", "ткань, закреплённая краем"),
            ("rect water … fluid 0.1", "жидкость, вязкость 0–1"),
            ("rect tap … fluid flow 60", "кран: капель в секунду"),
            ("ellipse ball … soft 0.2", "мягкое тело, упругость 0–1"),
            ("pressure 1 friction 0.3 damp 0.05", "объём, трение, затухание"),
            ("drive 0.4", "следование ключам"),
            ("rect wall … solid", "препятствие"),
            ("world gravity 0 900 wind 200 0 gust 0.4", "мир: гравитация, ветер, порывы"),
            ("weight 0.5 sail 2", "вес и парусность объекта"),
            ("pull 2600 range 200", "притяжение (с минусом — отталкивает), радиус"),
            ("fan 600 range 300 cone 40", "вентилятор: сила, радиус, ширина струи"),
            ("world.wind-x 0: 0 | 2: 600", "ключи мира: gravity-x/y, wind-x/y, gust"),
        ],
    ),
    (
        "Машина состояний",
        &[
            ("machine Player {", "состояния, переходы, входы"),
            ("  input speed 0 · input hover false · input jump trigger", "вход: число, флаг, триггер"),
            ("  state Idle at 40 80", "состояние играет анимацию Idle"),
            ("  state Walk anim Run", "или другую анимацию"),
            ("  Idle -> Walk when speed > 0.5 blend 0.2s", "переход: условие и смешивание"),
            ("  any -> Jump when jump · Jump -> Idle at end", "из любого; в конце анимации"),
            ("  … when hover and not dead", "несколько условий; not — флаг выключен"),
            ("  listen btn hover -> hover", "мышь: hover press click x y на объекте"),
            ("}", "первое состояние — начальное"),
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
