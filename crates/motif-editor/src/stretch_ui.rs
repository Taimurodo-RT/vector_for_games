//! «Растяжение 9-slice»: a piece of the UI kit drawn the way Godot draws it, from the pictures
//! the kit export makes. Pull the corner to stretch it; the mouse hovers and presses it, so the
//! states show as in the game.

use crate::app::App;
use crate::theme::{self, FAINT, FG, MUTED};
use egui::{Align2, Color32, CornerRadius, FontId, Mesh, Rect, RichText, Sense, Shape, Stroke, StrokeKind, TextureHandle, TextureOptions, Vec2, pos2, vec2};
use motif_pixel::kit::{KitFont, KitItem, KitPicture, Slot};
use motif_render::CpuOptions;
use std::hash::{Hash, Hasher};

/// The preview window's state: which piece, how big, and what it was rendered from.
pub struct Stretch {
    pub id: String,
    /// The control's size in picture pixels; set from the drawing when first rendered.
    size: Option<Vec2>,
    /// Screen pixels per picture pixel.
    zoom: usize,
    /// Progress, slider or scroll position, 0..1.
    value: f32,
    disabled: bool,
    checked: bool,
    key: u64,
    item: Option<Result<KitItem, String>>,
    textures: Vec<TextureHandle>,
}

impl Stretch {
    pub fn new(id: &str) -> Stretch {
        Stretch { id: id.to_string(), size: None, zoom: 1, value: 0.6, disabled: false, checked: true, key: 0, item: None, textures: Vec::new() }
    }
}

fn style(item: &KitItem, prop: &str) -> Option<(Option<usize>, Option<[u32; 4]>)> {
    item.slots.iter().find_map(|s| match s {
        Slot::Style { prop: p, picture, content } if *p == prop => Some((*picture, *content)),
        _ => None,
    })
}

fn icon(item: &KitItem, prop: &str) -> Option<usize> {
    item.slots.iter().find_map(|s| match s {
        Slot::Icon { prop: p, picture } if *p == prop => Some(*picture),
        _ => None,
    })
}

fn constant(item: &KitItem, prop: &str) -> Option<i64> {
    item.slots.iter().find_map(|s| match s {
        Slot::Constant { prop: p, value } if *p == prop => Some(*value),
        _ => None,
    })
}

/// A picture's own size without what reaches past the control (shadows).
fn box_size(p: &KitPicture) -> Vec2 {
    vec2((p.px.width() - p.expand[0] - p.expand[2]) as f32, (p.px.height() - p.expand[1] - p.expand[3]) as f32)
}

/// The smallest a stretched picture gets: its corners.
fn min_size(p: &KitPicture) -> Vec2 {
    p.margins.map_or(Vec2::ZERO, |m| vec2((m[0] + m[2]) as f32, (m[1] + m[3]) as f32))
}

/// Lays a nine-slice picture over `target` (the control's rect on screen) at `k` screen points
/// per picture pixel, like Godot's `StyleBoxTexture`: corners as drawn, the rest stretched.
fn nine(mesh: &mut Mesh, p: &KitPicture, target: Rect, k: f32) {
    let e = p.expand.map(|v| v as f32 * k);
    let drawn = Rect::from_min_max(target.min - vec2(e[0], e[1]), target.max + vec2(e[2], e[3]));
    let (w, h) = (p.px.width() as f32, p.px.height() as f32);
    let m = p.margins.unwrap_or([0; 4]).map(|v| v as f32);
    // Corners bigger than the rect shrink together, as in Godot.
    let fit = |a: f32, b: f32, room: f32| {
        let f = if (a + b) * k > room && a + b > 0.0 { room / ((a + b) * k) } else { 1.0 };
        (a * k * f, b * k * f)
    };
    let (l, r) = fit(m[0], m[2], drawn.width());
    let (t, b) = fit(m[1], m[3], drawn.height());
    let xs = [(0.0, drawn.left()), (m[0], drawn.left() + l), (w - m[2], drawn.right() - r), (w, drawn.right())];
    let ys = [(0.0, drawn.top()), (m[1], drawn.top() + t), (h - m[3], drawn.bottom() - b), (h, drawn.bottom())];
    for i in 0..3 {
        for j in 0..3 {
            let (u0, x0) = xs[i];
            let (u1, x1) = xs[i + 1];
            let (v0, y0) = ys[j];
            let (v1, y1) = ys[j + 1];
            if u1 <= u0 || v1 <= v0 || x1 <= x0 || y1 <= y0 {
                continue;
            }
            mesh.add_rect_with_uv(
                Rect::from_min_max(pos2(x0, y0), pos2(x1, y1)),
                Rect::from_min_max(pos2(u0 / w, v0 / h), pos2(u1 / w, v1 / h)),
                Color32::WHITE,
            );
        }
    }
}

/// A sample caption in the font's color for `state`, centred in `rect`.
fn caption(painter: &egui::Painter, font: &KitFont, state: &str, rect: Rect, k: f32, align: Align2) {
    let prop = match state {
        "hover" => "font_hover_color",
        "pressed" => "font_pressed_color",
        "disabled" => "font_disabled_color",
        _ => "font_color",
    };
    let c = font.colors.iter().find(|c| c.0 == prop).or(font.colors.first()).map_or(motif_core::Color::WHITE, |c| c.1);
    let color = Color32::from_rgba_unmultiplied(c.r, c.g, c.b, c.a);
    let at = match align {
        Align2::LEFT_CENTER => rect.left_center(),
        _ => rect.center(),
    };
    painter.text(at, align, "Текст", FontId::proportional(font.size as f32 * k), color);
}

impl App {
    pub(crate) fn stretch_window(&mut self, ctx: &egui::Context) {
        let Some(mut st) = self.stretch.take() else { return };
        let scale = self.export.scale;
        let antialias = self.export.antialias;
        let mut hasher = std::hash::DefaultHasher::new();
        (&self.src, &st.id, scale.to_bits(), antialias, self.boards.active).hash(&mut hasher);
        let key = hasher.finish();
        if st.key != key || st.item.is_none() {
            let item = motif_pixel::kit::render_piece(&self.scene.doc, &st.id, CpuOptions { scale, antialias, background: false });
            st.textures = match &item {
                Ok(it) => it
                    .pictures
                    .iter()
                    .enumerate()
                    .map(|(i, p)| {
                        let image = egui::ColorImage::from_rgba_premultiplied([p.px.width() as usize, p.px.height() as usize], p.px.data());
                        ctx.load_texture(format!("stretch-{i}"), image, TextureOptions::NEAREST)
                    })
                    .collect(),
                Err(_) => Vec::new(),
            };
            if st.size.is_none()
                && let Ok(it) = &item
            {
                st.size = Some(natural_size(it));
            }
            st.item = Some(item);
            st.key = key;
        }
        let mut open = true;
        let bg = self.scene.doc.artboard.background;
        egui::Window::new("Растяжение 9-slice").open(&mut open).collapsible(false).default_width(460.0).show(ctx, |ui| {
            let item = match &st.item {
                Some(Ok(it)) => it,
                Some(Err(e)) => {
                    ui.label(RichText::new(e).color(theme::DANGER));
                    return;
                }
                None => return,
            };
            ui.horizontal(|ui| {
                ui.label(RichText::new(format!("{} — {}", st.id, item.base)).size(13.0).color(FG));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if let Some(z) = theme::segmented(ui, &["1x", "2x", "3x"], st.zoom - 1, false) {
                        st.zoom = z + 1;
                    }
                });
            });
            ui.horizontal(|ui| {
                let has_disabled = item.pictures.iter().any(|p| p.name.ends_with("disabled")) || item.font.as_ref().is_some_and(|f| f.colors.iter().any(|c| c.0 == "font_disabled_color"));
                if has_disabled {
                    ui.checkbox(&mut st.disabled, "Выключена");
                }
                if item.base == "CheckBox" {
                    ui.checkbox(&mut st.checked, "Включён");
                }
                if matches!(item.base, "ProgressBar" | "HSlider" | "VSlider" | "HScrollBar" | "VScrollBar") {
                    ui.add(egui::Slider::new(&mut st.value, 0.0..=1.0).show_value(false).text("значение"));
                }
            });
            let size = st.size.unwrap_or(vec2(64.0, 32.0));
            let least = least_size(item);
            ui.label(
                RichText::new(format!(
                    "В игре {}×{} px · меньше {}×{} не сожмётся · масштаб экспорта ×{}",
                    size.x.round(),
                    size.y.round(),
                    least.x.round(),
                    least.y.round(),
                    motif_script::fmt_num(scale)
                ))
                .size(11.5)
                .color(MUTED),
            );
            // One screen pixel per picture pixel at 1x, whatever the display's scaling.
            let k = st.zoom as f32 / ctx.pixels_per_point();
            let pad = 28.0;
            let area = vec2(ui.available_width().max(300.0), (size.y * k + pad * 2.0).clamp(140.0, 520.0));
            let (rect, _) = ui.allocate_exact_size(area, Sense::hover());
            let painter = ui.painter_at(rect);
            painter.rect_filled(rect, 6.0, Color32::from_rgb(bg.r, bg.g, bg.b));
            let control = Rect::from_min_size(rect.min + vec2(pad, pad), size * k);
            let resp = ui.interact(control, ui.scope_id().with("stretch-control"), Sense::click_and_drag());
            let hovered = resp.hovered() || resp.dragged();
            let pressed = resp.is_pointer_button_down_on();
            let state = if st.disabled {
                "disabled"
            } else if pressed {
                "pressed"
            } else if hovered {
                "hover"
            } else {
                "normal"
            };
            let mesh = |pic: usize, target: Rect| {
                let (Some(p), Some(tex)) = (item.pictures.get(pic), st.textures.get(pic)) else { return };
                let mut m = Mesh::with_texture(tex.id());
                nine(&mut m, p, target, k);
                painter.add(Shape::mesh(m));
            };
            let horizontal = !item.base.starts_with('V');
            match item.base {
                "ProgressBar" => {
                    if let Some((Some(p), _)) = style(item, "background") {
                        mesh(p, control);
                    }
                    if let Some((Some(p), _)) = style(item, "fill")
                        && st.value > 0.0
                    {
                        let least = min_size(&item.pictures[p]);
                        let w = least.x + (size.x - least.x).max(0.0) * st.value;
                        mesh(p, Rect::from_min_size(control.min, vec2(w * k, control.height())));
                    }
                }
                "HSlider" | "VSlider" => {
                    if resp.is_pointer_button_down_on()
                        && let Some(at) = resp.interact_pointer_pos()
                    {
                        st.value = if horizontal { (at.x - control.left()) / control.width() } else { (control.bottom() - at.y) / control.height() }.clamp(0.0, 1.0);
                    }
                    let thick = style(item, "slider").and_then(|s| s.1).map_or(0.0, |c| if horizontal { (c[1] + c[3]) as f32 } else { (c[0] + c[2]) as f32 });
                    let grabber = icon(item, if st.disabled { "grabber_disabled" } else if hovered { "grabber_highlight" } else { "grabber" });
                    let g = grabber.and_then(|g| item.pictures.get(g)).map_or(Vec2::ZERO, |p| vec2(p.px.width() as f32, p.px.height() as f32));
                    let track = if horizontal {
                        Rect::from_min_size(pos2(control.left(), control.center().y - thick * k / 2.0), vec2(control.width(), thick * k))
                    } else {
                        Rect::from_min_size(pos2(control.center().x - thick * k / 2.0, control.top()), vec2(thick * k, control.height()))
                    };
                    if let Some((Some(p), _)) = style(item, "slider") {
                        mesh(p, track);
                    }
                    let area = if horizontal { size.x - g.x } else { size.y - g.y };
                    if let Some((Some(p), _)) = style(item, if hovered { "grabber_area_highlight" } else { "grabber_area" }) {
                        let len = (area * st.value + if horizontal { g.x } else { g.y } / 2.0) * k;
                        let r = if horizontal {
                            Rect::from_min_size(track.min, vec2(len, track.height()))
                        } else {
                            Rect::from_min_max(pos2(track.left(), track.bottom() - len), track.max)
                        };
                        mesh(p, r);
                    }
                    if let Some(tex) = grabber.and_then(|g| st.textures.get(g)) {
                        let at = if horizontal {
                            pos2(control.left() + area * st.value * k, control.center().y - g.y * k / 2.0)
                        } else {
                            pos2(control.center().x - g.x * k / 2.0, control.bottom() - (area * st.value + g.y) * k)
                        };
                        painter.image(tex.id(), Rect::from_min_size(at, g * k), Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
                    }
                }
                "HScrollBar" | "VScrollBar" => {
                    if let Some((Some(p), _)) = style(item, "scroll") {
                        mesh(p, control);
                    }
                    let prop = if pressed { "grabber_pressed" } else if hovered { "grabber_highlight" } else { "grabber" };
                    if let Some((Some(p), _)) = style(item, prop) {
                        // A page a third of the content long.
                        let least = min_size(&item.pictures[p]);
                        let (len, room) = if horizontal { (size.x / 3.0, size.x) } else { (size.y / 3.0, size.y) };
                        let len = len.max(if horizontal { least.x } else { least.y });
                        let ofs = (room - len) * st.value;
                        let r = if horizontal {
                            Rect::from_min_size(control.min + vec2(ofs * k, 0.0), vec2(len * k, control.height()))
                        } else {
                            Rect::from_min_size(control.min + vec2(0.0, ofs * k), vec2(control.width(), len * k))
                        };
                        mesh(p, r);
                    }
                }
                "CheckBox" => {
                    if resp.clicked() && !st.disabled {
                        st.checked = !st.checked;
                    }
                    let name = match (st.checked, st.disabled) {
                        (true, false) => "checked",
                        (true, true) => "checked_disabled",
                        (false, false) => "unchecked",
                        (false, true) => "unchecked_disabled",
                    };
                    if let Some((Some(p), _)) = style(item, state).or(style(item, "normal")) {
                        mesh(p, control);
                    }
                    let left = style(item, "normal").and_then(|s| s.1).map_or(0.0, |c| c[0] as f32);
                    if let Some(i) = icon(item, name)
                        && let (Some(p), Some(tex)) = (item.pictures.get(i), st.textures.get(i))
                    {
                        let s = vec2(p.px.width() as f32, p.px.height() as f32) * k;
                        let at = pos2(control.left() + left * k, control.center().y - s.y / 2.0);
                        painter.image(tex.id(), Rect::from_min_size(at, s), Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
                        if let Some(f) = &item.font {
                            let gap = constant(item, "h_separation").unwrap_or(4) as f32;
                            let text = Rect::from_min_max(pos2(at.x + s.x + gap * k, control.top()), control.max);
                            caption(&painter, f, state, text, k, Align2::LEFT_CENTER);
                        }
                    }
                }
                _ => {
                    // Buttons, panels and labels: the picture of the state, the caption in its content rect.
                    let slot = if item.base == "PanelContainer" { style(item, "panel") } else { style(item, state).or(style(item, "normal")) };
                    if let Some((Some(p), _)) = slot {
                        mesh(p, control);
                    }
                    if let Some(f) = &item.font {
                        let c = slot.and_then(|s| s.1).map_or([0.0; 4], |c| c.map(|v| v as f32 * k));
                        let inner = Rect::from_min_max(control.min + vec2(c[0], c[1]), control.max - vec2(c[2], c[3]));
                        caption(&painter, f, state, inner, k, Align2::CENTER_CENTER);
                    }
                }
            }
            // The corner handle stretches the control, in whole pixels.
            let handle = Rect::from_center_size(control.max, vec2(12.0, 12.0));
            let h = ui.interact(handle, ui.scope_id().with("stretch-handle"), Sense::drag());
            painter.rect(handle, CornerRadius::same(2), theme::ACCENT, Stroke::new(1.0, theme::ACCENT_INK), StrokeKind::Inside);
            painter.rect_stroke(control, 0.0, Stroke::new(1.0, Color32::from_white_alpha(40)), StrokeKind::Outside);
            if h.dragged() {
                let to = (size + h.drag_delta() / k).max(least.max(vec2(1.0, 1.0)));
                st.size = Some(to);
            }
            if h.drag_stopped() {
                st.size = st.size.map(|s| s.round());
            }
            if h.hovered() || h.dragged() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeNwSe);
            }
            ui.horizontal(|ui| {
                if theme::button(ui, "Как нарисовано", theme::ButtonKind::Normal).clicked() {
                    st.size = Some(natural_size(item));
                }
                if theme::button(ui, "Вдвое шире", theme::ButtonKind::Normal).clicked() {
                    st.size = Some(vec2(size.x * 2.0, size.y));
                }
            });
            ui.label(
                RichText::new(
                    "Тяните оранжевый уголок — так Godot растянет деталь. Наведите и нажмите мышью: покажутся Hover и Press. Подпись — образец цвета и размера шрифтом редактора.",
                )
                .size(11.5)
                .color(FAINT),
            );
        });
        if open {
            self.stretch = Some(st);
        }
    }
}

/// The size the piece is drawn at.
fn natural_size(item: &KitItem) -> Vec2 {
    match item.base {
        "CheckBox" => {
            let icon = icon(item, "unchecked").and_then(|i| item.pictures.get(i)).map_or(vec2(24.0, 24.0), |p| vec2(p.px.width() as f32, p.px.height() as f32));
            let text = item.font.as_ref().map_or(0.0, |f| f.size as f32 * 3.0);
            vec2(icon.x + constant(item, "h_separation").unwrap_or(4) as f32 + text, icon.y.max(item.font.as_ref().map_or(0.0, |f| f.size as f32 * 1.4)))
        }
        "HSlider" | "VSlider" => {
            let track = style(item, "slider").and_then(|s| s.0).and_then(|p| item.pictures.get(p)).map_or(vec2(120.0, 8.0), box_size);
            let g = icon(item, "grabber").and_then(|p| item.pictures.get(p)).map_or(Vec2::ZERO, |p| vec2(p.px.width() as f32, p.px.height() as f32));
            track.max(g)
        }
        "Label" if item.pictures.is_empty() => {
            vec2(item.font.as_ref().map_or(80.0, |f| f.size as f32 * 4.0), item.font.as_ref().map_or(24.0, |f| f.size as f32 * 1.4))
        }
        _ => item.pictures.first().map_or(vec2(64.0, 32.0), box_size),
    }
}

/// How small the control can get: the corners of its main picture.
fn least_size(item: &KitItem) -> Vec2 {
    let main = match item.base {
        "ProgressBar" => style(item, "background"),
        "HSlider" | "VSlider" => style(item, "slider"),
        "HScrollBar" | "VScrollBar" => style(item, "scroll"),
        "PanelContainer" => style(item, "panel"),
        _ => style(item, "normal"),
    };
    main.and_then(|s| s.0).and_then(|p| item.pictures.get(p)).map_or(Vec2::ZERO, |p| {
        let m = min_size(p);
        vec2(m.x - (p.expand[0] + p.expand[2]) as f32, m.y - (p.expand[1] + p.expand[3]) as f32).max(Vec2::ZERO)
    })
}
