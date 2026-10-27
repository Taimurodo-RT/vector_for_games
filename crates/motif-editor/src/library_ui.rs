//! «Новое…»: a library of starting points in a window over the editor — an empty scene, the UI
//! kit and the usual screens of a game (menus, HUD, dialogue, inventory, results, loading).
//! Each card shows its first board as drawn; a template is an ordinary script that becomes the
//! new, unsaved scene.

use crate::app::{App, FileAction};
use crate::theme::{self, ACCENT, BG, DOT, FAINT, FG, LINE, MUTED, PANEL, PANEL_2, RAISE};
use egui::{Align2, Color32, CornerRadius, Key, Rect, RichText, Sense, Stroke, StrokeKind, TextureHandle, TextureOptions, vec2};
use motif_render::{CpuOptions, render_cpu};

/// One starting point of the library.
pub struct Template {
    pub name: &'static str,
    /// The shelf it stands on: an index into [`GROUPS`].
    pub group: usize,
    pub about: &'static str,
    pub src: &'static str,
}

/// The shelves of the library; «Все» shows every template.
pub const GROUPS: [&str; 6] = ["Все", "Начать", "Меню", "В игре", "Итоги", "Переходы"];

pub const TEMPLATES: [Template; 13] = [
    Template {
        name: "Пустая сцена", group: 1, about: "Холст 320×240 с прыгающим мячом — для персонажей и анимации", src: crate::app::NEW_SCENE
    },
    Template {
        name: "UI-набор", group: 1, about: "Кнопки, переключатели, поля, полосы и панели — тема для Godot", src: crate::app::NEW_UI_KIT
    },
    Template {
        name: "Главное меню",
        group: 2,
        about: "Название игры, столбик кнопок по центру и версия в углу",
        src: include_str!("../../../examples/screens/main-menu.msc"),
    },
    Template {
        name: "Пауза",
        group: 2,
        about: "Затемнённая игра, окно с кнопками и «закрыть» в углу",
        src: include_str!("../../../examples/screens/pause.msc"),
    },
    Template {
        name: "Настройки",
        group: 2,
        about: "Громкость, полный экран, качество и кнопки «Назад» и «Применить»",
        src: include_str!("../../../examples/screens/settings.msc"),
    },
    Template {
        name: "HUD",
        group: 3,
        about: "Здоровье и мана, таймер, монеты, пауза и панель предметов у краёв",
        src: include_str!("../../../examples/screens/hud.msc"),
    },
    Template {
        name: "Диалог",
        group: 3,
        about: "Окно реплики с портретом и именем, варианты ответа",
        src: include_str!("../../../examples/screens/dialogue.msc"),
    },
    Template {
        name: "Инвентарь",
        group: 3,
        about: "Сетка слотов 4×3 и описание выбранного предмета",
        src: include_str!("../../../examples/screens/inventory.msc"),
    },
    Template {
        name: "Магазин", group: 3, about: "Кошелёк и карточки товаров с ценой на кнопке", src: include_str!("../../../examples/screens/shop.msc")
    },
    Template {
        name: "Победа",
        group: 4,
        about: "Лучи, три звезды, счёт и кнопки «Ещё раз» и «Дальше»",
        src: include_str!("../../../examples/screens/victory.msc"),
    },
    Template {
        name: "Поражение",
        group: 4,
        about: "Красная виньетка, итог забега и кнопки «Заново» и «В меню»",
        src: include_str!("../../../examples/screens/game-over.msc"),
    },
    Template {
        name: "Выбор уровня",
        group: 5,
        about: "Сетка уровней со звёздами, текущий и закрытые на замок",
        src: include_str!("../../../examples/screens/level-select.msc"),
    },
    Template {
        name: "Загрузка", group: 5, about: "Крутящийся знак, полоса с процентами и совет", src: include_str!("../../../examples/screens/loading.msc")
    },
];

/// Width of a card's picture in points; the picture keeps its board's proportions inside 16:9.
const THUMB: f32 = 224.0;
const THUMB_H: f32 = THUMB * 9.0 / 16.0;
/// A card: the picture, the name and two lines about it.
const CARD_H: f32 = THUMB_H + 78.0;

/// The open library: the shelf shown, the chosen card and the pictures drawn so far.
pub struct Library {
    group: usize,
    chosen: usize,
    thumbs: Vec<Option<TextureHandle>>,
}

impl Library {
    pub fn new() -> Library {
        Library { group: 0, chosen: 0, thumbs: Vec::new() }
    }
}

/// The first board of a template drawn `width` pixels wide.
pub fn thumbnail(src: &str, width: f64) -> Option<tiny_skia::Pixmap> {
    let board = motif_script::parse_boards(src, None).ok()?.into_iter().next()?;
    let doc = board.doc;
    render_cpu(&doc, CpuOptions { scale: width / doc.artboard.width.max(1.0), antialias: true, background: true })
}

impl App {
    pub(crate) fn library_window(&mut self, ctx: &egui::Context) {
        let Some(mut lib) = self.library.take() else { return };
        if lib.thumbs.is_empty() {
            // Twice the card size, so the pictures stay sharp on a high-density screen.
            let px = (THUMB * ctx.pixels_per_point().max(1.0)) as f64;
            lib.thumbs = TEMPLATES
                .iter()
                .enumerate()
                .map(|(i, t)| {
                    let p = thumbnail(t.src, px)?;
                    let image = egui::ColorImage::from_rgba_premultiplied([p.width() as usize, p.height() as usize], p.data());
                    Some(ctx.load_texture(format!("library-{i}"), image, TextureOptions::LINEAR))
                })
                .collect();
        }
        let mut create = None;
        let mut close = false;
        let frame = egui::Frame::new().fill(PANEL).stroke(Stroke::new(1.0, LINE)).corner_radius(12).inner_margin(20);
        let modal = egui::Modal::new(egui::Id::unique("library")).frame(frame).backdrop_color(Color32::from_black_alpha(150)).show(ctx, |ui| {
            let cols = 3;
            let width = cols as f32 * (THUMB + 14.0) - 14.0 + 168.0;
            ui.set_width(width);
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.label(RichText::new("Новое").font(theme::semibold(18.0)).color(FG));
                    ui.label(
                        RichText::new("Пустая сцена, UI-набор или готовый экран игры. Шаблон — обычный скрипт: всё в нём можно менять, а «Экспорт → Экран для Godot» делает из него сцену.")
                            .size(12.0)
                            .color(MUTED),
                    );
                });
            });
            ui.add_space(14.0);
            ui.horizontal_top(|ui| {
                // Shelves.
                ui.vertical(|ui| {
                    ui.set_width(150.0);
                    for (g, name) in GROUPS.iter().enumerate() {
                        let count = if g == 0 { TEMPLATES.len() } else { TEMPLATES.iter().filter(|t| t.group == g).count() };
                        let (r, resp) = ui.allocate_exact_size(vec2(150.0, 30.0), Sense::click());
                        let on = lib.group == g;
                        let bg = if on {
                            RAISE
                        } else if resp.hovered() {
                            PANEL_2
                        } else {
                            Color32::TRANSPARENT
                        };
                        ui.painter().rect_filled(r, 6.0, bg);
                        let font = if on { theme::semibold(13.0) } else { theme::ui_font(13.0) };
                        ui.painter().text(r.left_center() + vec2(10.0, 0.0), Align2::LEFT_CENTER, *name, font, if on { FG } else { MUTED });
                        ui.painter().text(r.right_center() - vec2(10.0, 0.0), Align2::RIGHT_CENTER, count.to_string(), theme::ui_font(11.5), FAINT);
                        if resp.clicked() {
                            lib.group = g;
                        }
                    }
                });
                ui.add_space(18.0);
                // Cards.
                let shown: Vec<usize> = (0..TEMPLATES.len()).filter(|&i| lib.group == 0 || TEMPLATES[i].group == lib.group).collect();
                if !shown.contains(&lib.chosen) {
                    lib.chosen = shown[0];
                }
                egui::ScrollArea::vertical().max_height(470.0).auto_shrink([false, true]).show(ui, |ui| {
                    egui::Grid::new("library-cards").spacing(vec2(14.0, 14.0)).show(ui, |ui| {
                        for (n, &i) in shown.iter().enumerate() {
                            let t = &TEMPLATES[i];
                            let (r, resp) = ui.allocate_exact_size(vec2(THUMB + 12.0, CARD_H), Sense::click());
                            let on = lib.chosen == i;
                            let border = if on {
                                ACCENT
                            } else if resp.hovered() {
                                MUTED
                            } else {
                                LINE
                            };
                            ui.painter().rect(r, 10.0, if resp.hovered() || on { PANEL_2 } else { BG }, Stroke::new(if on { 2.0 } else { 1.0 }, border), StrokeKind::Inside);
                            let pic = Rect::from_min_size(r.min + vec2(6.0, 6.0), vec2(THUMB, THUMB_H));
                            ui.painter().rect_filled(pic, 6.0, DOT);
                            if let Some(tex) = lib.thumbs.get(i).and_then(Option::as_ref) {
                                let [w, h] = tex.size().map(|v| v as f32);
                                let k = (THUMB / w).min(THUMB_H / h);
                                let img = Rect::from_center_size(pic.center(), vec2(w * k, h * k));
                                egui::Image::new(tex).corner_radius(CornerRadius::same(6)).paint_at(ui, img);
                            }
                            let text = r.left_top() + vec2(10.0, THUMB_H + 14.0);
                            ui.painter().text(text, Align2::LEFT_TOP, t.name, theme::semibold(13.5), FG);
                            let about = ui.painter().layout(t.about.to_string(), theme::ui_font(11.5), MUTED, THUMB - 8.0);
                            ui.painter().galley(text + vec2(0.0, 19.0), about, MUTED);
                            if resp.clicked() {
                                lib.chosen = i;
                            }
                            if resp.double_clicked() {
                                create = Some(i);
                            }
                            if n % cols == cols - 1 {
                                ui.end_row();
                            }
                        }
                    });
                });
            });
            ui.add_space(12.0);
            ui.horizontal(|ui| {
                let t = &TEMPLATES[lib.chosen];
                ui.label(RichText::new(format!("«{}» откроется новой сценой без файла: сохраните её, чтобы работать дальше.", t.name)).size(12.0).color(FAINT));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if theme::button(ui, "Создать", theme::ButtonKind::Primary).clicked() {
                        create = Some(lib.chosen);
                    }
                    if theme::button(ui, "Отмена", theme::ButtonKind::Normal).clicked() {
                        close = true;
                    }
                });
            });
        });
        let (enter, escape) = ctx.input(|i| (i.key_pressed(Key::Enter), i.key_pressed(Key::Escape)));
        if enter {
            create = Some(lib.chosen);
        }
        if let Some(i) = create {
            self.file_action(FileAction::New(TEMPLATES[i].src));
        } else if !(close || escape || modal.backdrop_response.clicked()) {
            self.library = Some(lib);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{GROUPS, TEMPLATES, thumbnail};

    /// Every template parses, draws, stands on a shelf and makes Godot controls of its pieces.
    #[test]
    fn every_template_opens_and_draws() {
        for t in &TEMPLATES {
            assert!(t.group > 0 && t.group < GROUPS.len(), "{}", t.name);
            let boards = motif_script::parse_boards(t.src, None).unwrap_or_else(|e| panic!("{}: {e:?}", t.name));
            let px = thumbnail(t.src, 224.0).unwrap_or_else(|| panic!("{}: no picture", t.name));
            assert_eq!(px.width(), 224, "{}", t.name);
            // A screen template is interface: its board is in UI mode and has pieces for Godot.
            if t.group > 1 {
                let doc = &boards[0].doc;
                assert!(doc.nodes.iter().any(|n| n.ui), "{}: no ui pieces", t.name);
            }
        }
    }
}
