//! Look of the editor: colours, fonts and the small widgets the panels are built from.
//! Mirrors the HTML prototype (tokens `--bg`, `--panel`, `--accent`…).

use egui::{
    Align2, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Pos2, Rect, Response, Sense, Shape, Stroke, StrokeKind, Ui, pos2, vec2,
};
use std::sync::Arc;

pub const BG: Color32 = Color32::from_rgb(0x0b, 0x0d, 0x12);
pub const PANEL: Color32 = Color32::from_rgb(0x15, 0x19, 0x22);
pub const PANEL_2: Color32 = Color32::from_rgb(0x1b, 0x20, 0x29);
pub const RAISE: Color32 = Color32::from_rgb(0x23, 0x29, 0x37);
pub const LINE: Color32 = Color32::from_rgb(0x26, 0x2c, 0x39);
pub const FG: Color32 = Color32::from_rgb(0xe6, 0xe9, 0xf0);
pub const MUTED: Color32 = Color32::from_rgb(0x8d, 0x95, 0xa7);
pub const FAINT: Color32 = Color32::from_rgb(0x59, 0x61, 0x74);
pub const ACCENT: Color32 = Color32::from_rgb(0xf4, 0xa6, 0x2a);
pub const ACCENT_INK: Color32 = Color32::from_rgb(0x1d, 0x14, 0x04);
pub const SEL: Color32 = Color32::from_rgb(0x5a, 0xa9, 0xff);
pub const DANGER: Color32 = Color32::from_rgb(0xff, 0x6b, 0x5b);
pub const GOOD: Color32 = Color32::from_rgb(0x4f, 0xd3, 0x8c);
pub const DOT: Color32 = Color32::from_rgb(0x1c, 0x21, 0x2c);

pub fn semibold(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name("semibold".into()))
}

pub fn ui_font(size: f32) -> FontId {
    FontId::proportional(size)
}

pub fn mono(size: f32) -> FontId {
    FontId::monospace(size)
}

/// Mixes `c` over `base` with weight `t` (like CSS `color-mix`).
pub fn mix(c: Color32, base: Color32, t: f32) -> Color32 {
    let l = |a: u8, b: u8| (a as f32 * t + b as f32 * (1.0 - t)).round() as u8;
    Color32::from_rgb(l(c.r(), base.r()), l(c.g(), base.g()), l(c.b(), base.b()))
}

/// Fonts (IBM Plex, as in the prototype) and colours.
pub fn install(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();
    let add = |fonts: &mut FontDefinitions, name: &str, bytes: &'static [u8]| {
        fonts.font_data.insert(name.into(), Arc::new(FontData::from_static(bytes)));
    };
    add(&mut fonts, "plex-sans", include_bytes!("../assets/fonts/IBMPlexSans-Regular.ttf"));
    add(&mut fonts, "plex-sans-semibold", include_bytes!("../assets/fonts/IBMPlexSans-SemiBold.ttf"));
    add(&mut fonts, "plex-mono", include_bytes!("../assets/fonts/IBMPlexMono-Regular.ttf"));
    // Plex first; egui's own fonts stay behind it for symbols Plex lacks.
    let fallback = fonts.families[&FontFamily::Proportional].clone();
    fonts.families.get_mut(&FontFamily::Proportional).unwrap().insert(0, "plex-sans".into());
    fonts.families.get_mut(&FontFamily::Monospace).unwrap().insert(0, "plex-mono".into());
    let mut semi = vec!["plex-sans-semibold".to_string()];
    semi.extend(fallback);
    fonts.families.insert(FontFamily::Name("semibold".into()), semi);
    ctx.set_fonts(fonts);

    ctx.global_style_mut(|style| {
        use egui::TextStyle::*;
        style.text_styles = [(Small, ui_font(11.0)), (Body, ui_font(13.0)), (Button, ui_font(13.0)), (Heading, semibold(14.0)), (Monospace, mono(12.5))].into();
        let s = &mut style.spacing;
        s.item_spacing = vec2(6.0, 6.0);
        s.button_padding = vec2(10.0, 4.0);
        s.interact_size.y = 26.0;
        s.window_margin = egui::Margin::same(12);
        s.menu_margin = egui::Margin::same(6);
        s.scroll.floating = true;

        let v = &mut style.visuals;
        *v = egui::Visuals::dark();
        v.override_text_color = None;
        v.panel_fill = PANEL;
        v.window_fill = PANEL;
        v.window_stroke = Stroke::new(1.0, LINE);
        v.window_corner_radius = CornerRadius::same(12);
        v.menu_corner_radius = CornerRadius::same(8);
        v.extreme_bg_color = BG;
        v.faint_bg_color = PANEL_2;
        v.code_bg_color = BG;
        v.hyperlink_color = SEL;
        v.selection.bg_fill = SEL.gamma_multiply(0.35);
        v.selection.stroke = Stroke::new(1.0, SEL);
        v.text_cursor.stroke = Stroke::new(2.0, SEL);
        v.window_shadow = egui::Shadow { offset: [0, 8], blur: 24, spread: 0, color: Color32::from_black_alpha(140) };
        v.popup_shadow = v.window_shadow;
        let w = &mut v.widgets;
        let radius = CornerRadius::same(6);
        for (state, bg, fg, border) in [
            (&mut w.noninteractive, PANEL, MUTED, LINE),
            (&mut w.inactive, PANEL_2, FG, LINE),
            (&mut w.hovered, RAISE, FG, LINE),
            (&mut w.active, RAISE, FG, SEL),
            (&mut w.open, RAISE, FG, LINE),
        ] {
            state.bg_fill = bg;
            state.weak_bg_fill = bg;
            state.bg_stroke = Stroke::new(1.0, border);
            state.fg_stroke = Stroke::new(1.0, fg);
            state.corner_radius = radius;
            state.expansion = 0.0;
        }
    });
}

/// Upper-case section caption: `ТРАНСФОРМАЦИЯ`, `СЛОИ`.
pub fn caption(ui: &mut Ui, text: &str) {
    ui.label(egui::RichText::new(text.to_uppercase()).font(semibold(11.0)).color(MUTED).extra_letter_spacing(0.9));
}

/// A thin horizontal rule in the line colour.
pub fn rule(ui: &mut Ui) {
    let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 1.0), Sense::hover());
    ui.painter().rect_filled(r, 0.0, LINE);
}

pub fn diamond(painter: &egui::Painter, c: Pos2, r: f32, fill: Color32, stroke: Stroke) {
    let pts = vec![c + vec2(0.0, -r), c + vec2(r, 0.0), c + vec2(0.0, r), c + vec2(-r, 0.0)];
    painter.add(Shape::convex_polygon(pts, fill, stroke));
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Icon {
    Select,
    Hand,
    Rect,
    Ellipse,
    Star,
    Polygon,
    Group,
    Artboard,
    Play,
    Pause,
    First,
    Plus,
    Pen,
    AlignLeft,
    AlignCenterX,
    AlignRight,
    AlignTop,
    AlignCenterY,
    AlignBottom,
    SpreadX,
    SpreadY,
    /// Chevron pointing left: collapse a panel.
    CollapseLeft,
    /// Chevron pointing right: expand a panel.
    ExpandRight,
    Help,
    /// Circular arrow, also drawn as the pointer while rotating.
    Rotate,
    /// A ruler: snapping steps.
    Step,
    /// A capital T: text.
    Text,
    /// Two joints and a thin diamond between them: a bone.
    Bone,
}

/// Line icons drawn as vectors, so they never depend on font coverage.
pub fn paint_icon(painter: &egui::Painter, rect: Rect, icon: Icon, color: Color32) {
    let c = rect.center();
    let s = rect.width().min(rect.height()) / 16.0;
    let p = |x: f32, y: f32| c + vec2(x * s, y * s);
    let st = Stroke::new(1.4, color);
    match icon {
        Icon::Select => {
            let pts = vec![p(-4.0, -6.0), p(5.0, 1.0), p(1.0, 1.8), p(3.4, 6.2), p(1.6, 7.0), p(-0.7, 2.6), p(-4.0, 5.2)];
            painter.add(Shape::closed_line(pts, st));
        }
        Icon::Hand => {
            for (x, top) in [(-3.0, -3.0), (-1.0, -6.0), (1.0, -6.0), (3.0, -4.0)] {
                painter.line_segment([p(x, top), p(x, 1.0)], st);
            }
            painter.add(Shape::line(vec![p(-3.0, 0.0), p(-5.0, -1.5), p(-6.0, 0.0), p(-3.0, 5.0), p(3.0, 6.0), p(5.0, 1.0), p(3.0, -4.0)], st));
        }
        Icon::Rect => {
            painter.rect_stroke(Rect::from_center_size(c, vec2(11.0 * s, 9.0 * s)), 1.5 * s, st, StrokeKind::Middle);
        }
        Icon::Ellipse => {
            painter.circle_stroke(c, 5.5 * s, st);
        }
        Icon::Star => {
            let pts: Vec<Pos2> = (0..10)
                .map(|i| {
                    let a = std::f32::consts::PI * (i as f32 / 5.0) - std::f32::consts::FRAC_PI_2;
                    let r = if i % 2 == 0 { 6.5 } else { 2.8 };
                    p(a.cos() * r, a.sin() * r + 0.5)
                })
                .collect();
            painter.add(Shape::closed_line(pts, st));
        }
        Icon::Polygon => {
            let pts: Vec<Pos2> =
                (0..6).map(|i| std::f32::consts::PI / 3.0 * i as f32 - std::f32::consts::FRAC_PI_2).map(|a| p(a.cos() * 6.0, a.sin() * 6.0)).collect();
            painter.add(Shape::closed_line(pts, st));
        }
        Icon::Group => {
            painter.rect_stroke(Rect::from_center_size(p(-1.0, -1.0), vec2(9.0 * s, 9.0 * s)), 1.5 * s, st, StrokeKind::Middle);
            painter.add(Shape::line(vec![p(5.0, -2.0), p(5.0, 5.0), p(-2.0, 5.0)], st));
        }
        Icon::Artboard => {
            for d in [-2.5, 2.5] {
                painter.line_segment([p(d, -6.0), p(d, 6.0)], st);
                painter.line_segment([p(-6.0, d), p(6.0, d)], st);
            }
        }
        Icon::Play => {
            painter.add(Shape::convex_polygon(vec![p(-3.5, -5.0), p(5.0, 0.0), p(-3.5, 5.0)], color, Stroke::NONE));
        }
        Icon::Pause => {
            for x in [-2.5, 2.5] {
                painter.rect_filled(Rect::from_center_size(p(x, 0.0), vec2(2.4 * s, 9.5 * s)), 0.5, color);
            }
        }
        Icon::First => {
            painter.rect_filled(Rect::from_center_size(p(-4.5, 0.0), vec2(1.8 * s, 9.0 * s)), 0.0, color);
            painter.add(Shape::convex_polygon(vec![p(4.5, -4.5), p(4.5, 4.5), p(-2.5, 0.0)], color, Stroke::NONE));
        }
        Icon::Text => {
            painter.line_segment([p(-6.0, -6.0), p(6.0, -6.0)], st);
            painter.line_segment([p(0.0, -6.0), p(0.0, 6.5)], st);
            painter.line_segment([p(-2.5, 6.5), p(2.5, 6.5)], st);
        }
        Icon::Bone => {
            let (a, b) = (p(-5.0, 5.0), p(5.0, -5.0));
            painter.add(Shape::closed_line(vec![a, p(-1.5, -0.5), b, p(0.5, 1.5)], st));
            painter.circle_stroke(a, 2.0 * s, st);
            painter.circle_filled(b, 1.2 * s, color);
        }
        Icon::Pen => {
            painter.add(Shape::closed_line(vec![p(-6.0, 6.5), p(-5.0, 2.0), p(2.5, -5.5), p(5.5, -2.5), p(-2.0, 5.0)], st));
            painter.line_segment([p(1.0, -4.0), p(4.0, -1.0)], st);
        }
        Icon::AlignLeft | Icon::AlignCenterX | Icon::AlignRight => {
            let x = match icon {
                Icon::AlignLeft => -6.0,
                Icon::AlignCenterX => 0.0,
                _ => 6.0,
            };
            painter.line_segment([p(x, -7.0), p(x, 7.0)], st);
            let (w1, w2) = (9.0, 5.0);
            let left = |w: f32| match icon {
                Icon::AlignLeft => -6.0,
                Icon::AlignCenterX => -w / 2.0,
                _ => 6.0 - w,
            };
            painter.rect_filled(Rect::from_min_size(p(left(w1), -4.5), vec2(w1 * s, 3.0 * s)), 0.5, color);
            painter.rect_filled(Rect::from_min_size(p(left(w2), 1.5), vec2(w2 * s, 3.0 * s)), 0.5, color);
        }
        Icon::AlignTop | Icon::AlignCenterY | Icon::AlignBottom => {
            let y = match icon {
                Icon::AlignTop => -6.0,
                Icon::AlignCenterY => 0.0,
                _ => 6.0,
            };
            painter.line_segment([p(-7.0, y), p(7.0, y)], st);
            let (h1, h2) = (9.0, 5.0);
            let top = |h: f32| match icon {
                Icon::AlignTop => -6.0,
                Icon::AlignCenterY => -h / 2.0,
                _ => 6.0 - h,
            };
            painter.rect_filled(Rect::from_min_size(p(-4.5, top(h1)), vec2(3.0 * s, h1 * s)), 0.5, color);
            painter.rect_filled(Rect::from_min_size(p(1.5, top(h2)), vec2(3.0 * s, h2 * s)), 0.5, color);
        }
        Icon::SpreadX => {
            for x in [-6.5, 6.5] {
                painter.line_segment([p(x, -7.0), p(x, 7.0)], st);
            }
            painter.rect_filled(Rect::from_center_size(c, vec2(3.0 * s, 8.0 * s)), 0.5, color);
        }
        Icon::SpreadY => {
            for y in [-6.5, 6.5] {
                painter.line_segment([p(-7.0, y), p(7.0, y)], st);
            }
            painter.rect_filled(Rect::from_center_size(c, vec2(8.0 * s, 3.0 * s)), 0.5, color);
        }
        Icon::Plus => {
            painter.line_segment([p(-5.0, 0.0), p(5.0, 0.0)], st);
            painter.line_segment([p(0.0, -5.0), p(0.0, 5.0)], st);
        }
        Icon::CollapseLeft => {
            painter.add(Shape::line(vec![p(1.5, -5.0), p(-3.5, 0.0), p(1.5, 5.0)], st));
            painter.line_segment([p(5.0, -6.0), p(5.0, 6.0)], st);
        }
        Icon::ExpandRight => {
            painter.add(Shape::line(vec![p(-1.5, -5.0), p(3.5, 0.0), p(-1.5, 5.0)], st));
            painter.line_segment([p(-5.0, -6.0), p(-5.0, 6.0)], st);
        }
        Icon::Help => {
            painter.circle_stroke(c, 6.5 * s, st);
            painter.add(Shape::line(vec![p(-2.2, -1.8), p(-1.6, -3.4), p(0.0, -4.0), p(1.8, -3.4), p(2.3, -1.8), p(1.4, -0.4), p(0.0, 0.6), p(0.0, 1.8)], st));
            painter.circle_filled(p(0.0, 4.0), 0.9 * s.max(1.0), color);
        }
        Icon::Rotate => {
            let pts: Vec<Pos2> = (0..=20)
                .map(|i| {
                    let a = (-150.0 + i as f32 * 12.0).to_radians();
                    p(5.5 * a.cos(), 5.5 * a.sin())
                })
                .collect();
            let end = *pts.last().unwrap();
            painter.add(Shape::line(pts, st));
            // Arrow head at the end of the arc (pointing down-left, clockwise).
            painter.add(Shape::line(vec![end + vec2(-3.4 * s, -0.6 * s), end, end + vec2(0.4 * s, -3.4 * s)], st));
        }
        Icon::Step => {
            painter.rect_stroke(Rect::from_center_size(c, vec2(14.0 * s, 6.0 * s)), 1.0, st, StrokeKind::Middle);
            for x in [-4.0, -1.0, 2.0, 5.0] {
                painter.line_segment([p(x - 0.5, -3.0), p(x - 0.5, if x == -1.0 { 1.0 } else { -0.5 })], st);
            }
        }
    }
}

pub fn shape_icon(kind: motif_core::ShapeKind) -> Icon {
    use motif_core::ShapeKind::*;
    match kind {
        Group => Icon::Group,
        Rect => Icon::Rect,
        Ellipse => Icon::Ellipse,
        Star => Icon::Star,
        Polygon => Icon::Polygon,
        Path => Icon::Pen,
        Text => Icon::Text,
        Bone => Icon::Bone,
    }
}

/// A square tool button with an icon and a hotkey hint in the corner (tool rail).
pub fn tool_button(ui: &mut Ui, icon: Icon, on: bool, hotkey: &str, tip: &str) -> Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(34.0, 34.0), Sense::click());
    let p = ui.painter();
    if on {
        p.rect(rect, 7.0, RAISE, Stroke::new(1.0, LINE), StrokeKind::Inside);
    } else if resp.hovered() {
        p.rect_filled(rect, 7.0, PANEL_2);
    }
    let color = if on || resp.hovered() { FG } else { MUTED };
    paint_icon(p, Rect::from_center_size(rect.center(), vec2(18.0, 18.0)), icon, color);
    p.text(rect.right_bottom() + vec2(-3.0, -1.0), Align2::RIGHT_BOTTOM, hotkey, mono(9.0), FAINT);
    resp.on_hover_text(tip)
}

/// A small bordered button with an icon only.
pub fn icon_button(ui: &mut Ui, icon: Icon, tip: &str) -> Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(28.0, 26.0), Sense::click());
    let bg = if resp.hovered() { RAISE } else { PANEL_2 };
    ui.painter().rect(rect, 6.0, bg, Stroke::new(1.0, LINE), StrokeKind::Inside);
    paint_icon(ui.painter(), Rect::from_center_size(rect.center(), vec2(14.0, 14.0)), icon, FG);
    resp.on_hover_text(tip)
}

#[derive(Clone, Copy, PartialEq)]
pub enum ButtonKind {
    Normal,
    /// Light, bold: the main action (Экспорт).
    Primary,
    /// Outlined in the accent colour: a toggle that is on (Повтор).
    On,
}

pub fn button(ui: &mut Ui, text: &str, kind: ButtonKind) -> Response {
    let font = if kind == ButtonKind::Primary { semibold(13.0) } else { ui_font(13.0) };
    let galley = ui.painter().layout_no_wrap(text.to_string(), font, Color32::PLACEHOLDER);
    let size = vec2(galley.size().x + 20.0, 26.0);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    let (bg, border, fg) = match kind {
        ButtonKind::Normal => (if resp.hovered() { RAISE } else { PANEL_2 }, LINE, FG),
        ButtonKind::Primary => (if resp.hovered() { Color32::WHITE } else { FG }, FG, BG),
        ButtonKind::On => (if resp.hovered() { RAISE } else { PANEL_2 }, ACCENT, ACCENT),
    };
    ui.painter().rect(rect, 6.0, bg, Stroke::new(1.0, border), StrokeKind::Inside);
    ui.painter().galley(rect.center() - galley.size() / 2.0, galley, fg);
    resp
}

/// A row of mutually exclusive choices in a sunken frame (`Design | Animate`). Returns the clicked index.
pub fn segmented(ui: &mut Ui, items: &[&str], selected: usize, accent_last: bool) -> Option<usize> {
    let font = ui_font(13.0);
    let widths: Vec<f32> = items.iter().map(|t| ui.painter().layout_no_wrap(t.to_string(), font.clone(), FG).size().x + 24.0).collect();
    let total = vec2(widths.iter().sum::<f32>() + 4.0, 30.0);
    let (rect, _) = ui.allocate_exact_size(total, Sense::hover());
    ui.painter().rect(rect, 7.0, BG, Stroke::new(1.0, LINE), StrokeKind::Inside);
    let mut x = rect.left() + 2.0;
    let mut clicked = None;
    for (i, (text, w)) in items.iter().zip(&widths).enumerate() {
        let r = Rect::from_min_size(pos2(x, rect.top() + 2.0), vec2(*w, rect.height() - 4.0));
        let resp = ui.interact(r, ui.scope_id().with(("seg", text)), Sense::click());
        let on = i == selected;
        let (bg, fg) = match (on, accent_last && i + 1 == items.len()) {
            (true, true) => (ACCENT, ACCENT_INK),
            (true, false) => (RAISE, FG),
            (false, _) => (Color32::TRANSPARENT, if resp.hovered() { FG } else { MUTED }),
        };
        ui.painter().rect_filled(r, 5.0, bg);
        ui.painter().text(r.center(), Align2::CENTER_CENTER, *text, if on { semibold(13.0) } else { font.clone() }, fg);
        if resp.clicked() {
            clicked = Some(i);
        }
        x += w;
    }
    clicked
}

/// Keyframe state of a property, shown by the diamond next to its field.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum KeyState {
    /// Not in animate mode: no diamond.
    Hidden,
    /// Animate mode, property not animated: hollow grey diamond.
    None,
    /// Animated, but no key at the current time: hollow accent diamond.
    Track,
    /// A key sits at the current time: filled accent diamond.
    Key,
}

pub struct FieldOut {
    pub changed: Option<f64>,
    pub diamond: bool,
}

/// A numeric field in a sunken box with an inline label and an optional key diamond:
/// `[X  150         ◇]`. Dragging the label or the number scrubs the value.
#[allow(clippy::too_many_arguments)]
pub fn num_field(
    ui: &mut Ui,
    width: f32,
    label: &str,
    value: f64,
    speed: f64,
    range: std::ops::RangeInclusive<f64>,
    decimals: usize,
    key: KeyState,
) -> FieldOut {
    let (rect, _) = ui.allocate_exact_size(vec2(width, 28.0), Sense::hover());
    let animated = matches!(key, KeyState::Track | KeyState::Key);
    let border = if animated { mix(ACCENT, LINE, 0.45) } else { LINE };
    let mut out = FieldOut { changed: None, diamond: false };
    let label_w = ui.painter().layout_no_wrap(label.to_string(), ui_font(11.5), MUTED).size().x + 14.0;
    let diamond_w = if key == KeyState::Hidden { 4.0 } else { 22.0 };
    let value_rect = Rect::from_min_max(pos2(rect.left() + label_w, rect.top() + 1.0), pos2(rect.right() - diamond_w, rect.bottom() - 1.0));
    let mut v = value;
    let resp = ui.place(
        value_rect,
        egui::DragValue::new(&mut v)
            .speed(speed)
            .range(range.clone())
            .max_decimals(decimals)
            .custom_formatter(move |n, _| motif_script::fmt_num_with(n, decimals)),
    );
    let focused = resp.has_focus();
    ui.painter().rect_stroke(rect, 6.0, Stroke::new(1.0, if focused { SEL } else { border }), StrokeKind::Inside);
    // The label also scrubs, like in the prototype.
    let label_rect = Rect::from_min_max(rect.min, pos2(rect.left() + label_w, rect.bottom()));
    let lresp = ui.interact(label_rect, ui.scope_id().with(("scrub", label, rect.min.x as i32, rect.min.y as i32)), Sense::drag());
    ui.painter().text(pos2(rect.left() + 8.0, rect.center().y), Align2::LEFT_CENTER, label, ui_font(11.5), MUTED);
    if lresp.hovered() || lresp.dragged() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
    }
    if lresp.dragged() {
        let dv = lresp.drag_delta().x as f64 * speed;
        v = (v + dv).clamp(*range.start(), *range.end());
    }
    if v != value {
        out.changed = Some(v);
    }
    if key != KeyState::Hidden {
        let d = Rect::from_min_max(pos2(rect.right() - 22.0, rect.top()), rect.max);
        let dresp = ui.interact(d, ui.scope_id().with(("kf", label, rect.min.x as i32, rect.min.y as i32)), Sense::click());
        let (fill, stroke) = match key {
            KeyState::Key => (ACCENT, ACCENT),
            KeyState::Track => (Color32::TRANSPARENT, ACCENT),
            _ => (Color32::TRANSPARENT, if dresp.hovered() { MUTED } else { FAINT }),
        };
        diamond(ui.painter(), d.center(), 4.5, fill, Stroke::new(1.5, stroke));
        let tip = match key {
            KeyState::Key => "Убрать ключ на этом времени",
            _ => "Поставить ключ на этом времени",
        };
        out.diamond = dresp.on_hover_text(tip).clicked();
    }
    out
}

/// The popup of a colour swatch in the properties panel. It opens to the left of the swatch, over
/// the canvas, and always there: with egui's default alternatives it could jump between below and
/// above the swatch on every frame when it fits neither well.
pub fn color_popup(resp: &Response) -> egui::Popup<'_> {
    egui::Popup::from_toggle_button_response(resp)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .align(egui::RectAlign::LEFT_START)
        .align_alternatives(&[])
        .gap(8.0)
}
