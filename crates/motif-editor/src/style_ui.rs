//! Panel rows for the CSS-like properties: corners, skew and pivot, dashes, caps and joins,
//! shadow, glow, inner shadow, grain and blend mode. Each row collects whole-property edits (`dash 6 4`) applied afterwards.

use crate::app::App;
use crate::theme::{self, ButtonKind, FG, KeyState, LINE, MUTED};
use egui::{Color32, RichText, Sense, Stroke, StrokeKind, vec2};
use motif_core::{Blend, Cap, Color, Glow, Join, Node, Noise, Shadow};
use motif_script::fmt_num;

/// A property to write: the key, and the whole text with its values, or `None` to remove it.
pub type StyleEdit = (&'static str, Option<String>);

fn nums(v: &[f64]) -> String {
    v.iter().map(|x| fmt_num(*x)).collect::<Vec<_>>().join(" ")
}

/// Two number fields side by side; returns the pair when one changed.
fn pair(ui: &mut egui::Ui, labels: [&str; 2], v: [f64; 2], speed: f64, range: std::ops::RangeInclusive<f64>) -> Option<[f64; 2]> {
    let half = (ui.available_width() - 6.0) / 2.0;
    let mut out = None;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        for k in 0..2 {
            if let Some(n) = theme::num_field(ui, half, labels[k], v[k], speed, range.clone(), 1, KeyState::Hidden).changed {
                let mut v2 = v;
                v2[k] = (n * 10.0).round() / 10.0;
                out = Some(v2);
            }
        }
    });
    out
}

/// Pivot (transform-origin) under the rotation and skew fields.
pub fn pivot_row(ui: &mut egui::Ui, node: &Node, edits: &mut Vec<StyleEdit>) {
    if let Some([x, y]) = pair(ui, ["Ось X", "Ось Y"], [node.pivot.x, node.pivot.y], 0.5, -100_000.0..=100_000.0) {
        edits.push(("pivot", (x != 0.0 || y != 0.0).then(|| format!("pivot {} {}", fmt_num(x), fmt_num(y)))));
    }
}

/// Rounded corners of a rectangle: one radius, or four when they differ.
/// Returns true when the single animatable radius field should be shown instead.
pub fn corners_row(ui: &mut egui::Ui, node: &Node, edits: &mut Vec<StyleEdit>) -> bool {
    let Some(r) = node.radii else {
        if theme::button(ui, "По углам", ButtonKind::Normal).on_hover_text("Свой радиус у каждого угла, как border-radius: 8 8 0 0").clicked()
        {
            edits.push(("r", Some(format!("r {}", nums(&[node.radius; 4])))));
        }
        return true;
    };
    let mut changed = None;
    for (labels, idx) in [(["↖", "↗"], [0, 1]), (["↙", "↘"], [3, 2])] {
        if let Some(v) = pair(ui, labels, [r[idx[0]], r[idx[1]]], 0.5, 0.0..=10_000.0) {
            let mut r2 = r;
            r2[idx[0]] = v[0];
            r2[idx[1]] = v[1];
            changed = Some(r2);
        }
    }
    if let Some(r2) = changed {
        edits.push(("r", Some(format!("r {}", nums(&r2)))));
    }
    if theme::button(ui, "Одинаковые углы", ButtonKind::Normal).clicked() {
        edits.push(("r", Some(format!("r {}", fmt_num(r[0])))));
    }
    false
}

/// Dashes, line ends and corners of the stroke.
pub fn line_rows(ui: &mut egui::Ui, node: &Node, edits: &mut Vec<StyleEdit>) {
    let dash = [node.dash.first().copied().unwrap_or(0.0), node.dash.get(1).copied().unwrap_or(0.0)];
    if node.dash.len() > 2 {
        ui.label(RichText::new(format!("Пунктир: {} (правка в скрипте)", nums(&node.dash))).size(12.0).color(MUTED));
    } else if let Some([a, b]) = pair(ui, ["Штрих", "Пробел"], dash, 0.5, 0.0..=1000.0) {
        edits.push(("dash", (a > 0.0).then(|| if b > 0.0 { format!("dash {} {}", fmt_num(a), fmt_num(b)) } else { format!("dash {}", fmt_num(a)) })));
    }
    let caps = Cap::ALL.map(|c| match c {
        Cap::Butt => "Срез",
        Cap::Round => "Кругл.",
        Cap::Square => "Квадр.",
    });
    let joins = Join::ALL.map(|j| match j {
        Join::Miter => "Острый",
        Join::Round => "Кругл.",
        Join::Bevel => "Скос",
    });
    ui.horizontal(|ui| {
        ui.label(RichText::new("Концы").size(12.0).color(MUTED));
        let cur = Cap::ALL.iter().position(|c| *c == node.cap).unwrap_or(1);
        if let Some(k) = ui.push_id("caps", |ui| theme::segmented(ui, &caps, cur, false)).inner {
            let c = Cap::ALL[k];
            edits.push(("cap", (c != Cap::default()).then(|| format!("cap {}", c.keyword()))));
        }
    });
    ui.horizontal(|ui| {
        ui.label(RichText::new("Углы  ").size(12.0).color(MUTED));
        let cur = Join::ALL.iter().position(|j| *j == node.join).unwrap_or(1);
        if let Some(k) = ui.push_id("joins", |ui| theme::segmented(ui, &joins, cur, false)).inner {
            let j = Join::ALL[k];
            edits.push(("join", (j != Join::default()).then(|| format!("join {}", j.keyword()))));
        }
    });
}

fn shadow_text(s: &Shadow) -> String {
    if s.blur > 0.0 {
        format!("shadow {} {} {} {}", fmt_num(s.dx), fmt_num(s.dy), fmt_num(s.blur), s.color.to_hex())
    } else {
        format!("shadow {} {} {}", fmt_num(s.dx), fmt_num(s.dy), s.color.to_hex())
    }
}

/// A colour square with alpha; returns the colour when it was picked.
fn alpha_swatch(ui: &mut egui::Ui, c: Color) -> Option<Color> {
    let (sw, resp) = ui.allocate_exact_size(vec2(28.0, 28.0), Sense::click());
    let mut c32 = Color32::from_rgba_unmultiplied(c.r, c.g, c.b, c.a);
    ui.painter().rect_filled(sw, 6.0, c32);
    ui.painter().rect_stroke(sw, 6.0, Stroke::new(1.0, LINE), StrokeKind::Inside);
    let mut out = None;
    theme::color_popup(&resp).show(|ui| {
        if egui::color_picker::color_picker_color32(ui, &mut c32, egui::color_picker::Alpha::OnlyBlend) {
            let [r, g, b, a] = c32.to_srgba_unmultiplied();
            out = Some(Color { r, g, b, a });
        }
    });
    out
}

fn inset_text(s: &Shadow) -> String {
    shadow_text(s).replacen("shadow", "inset", 1)
}

fn glow_text(g: &Glow) -> String {
    if g.spread > 0.0 {
        format!("glow {} {} {}", fmt_num(g.size), fmt_num(g.spread), g.color.to_hex())
    } else {
        format!("glow {} {}", fmt_num(g.size), g.color.to_hex())
    }
}

fn noise_text(n: &Noise) -> String {
    if n.size == Noise::DEFAULT.size { format!("noise {}", fmt_num(n.amount)) } else { format!("noise {} {}", fmt_num(n.amount), fmt_num(n.size)) }
}

/// Glow on or off, its colour and its spread. Returns whether the node glows, so the caller can
/// draw its size as a keyable field (a hover that lights the button up).
pub fn glow_rows(ui: &mut egui::Ui, node: &Node, edits: &mut Vec<StyleEdit>) -> bool {
    let mut on = node.glow.is_some();
    ui.horizontal(|ui| {
        if ui.checkbox(&mut on, RichText::new("Свечение").size(12.5).color(FG)).changed() {
            let base = node.stroke.or(node.fill).unwrap_or(Glow::DEFAULT.color);
            let g = Glow { size: 12.0, color: Color { a: 0xcc, ..base }, ..Glow::DEFAULT };
            edits.push(("glow", on.then(|| glow_text(&g))));
        }
        let Some(g) = node.glow.filter(|_| on) else { return };
        if let Some(color) = alpha_swatch(ui, g.color) {
            edits.push(("glow", Some(glow_text(&Glow { color, ..g }))));
        }
        let w = ui.available_width();
        if let Some(v) = theme::num_field(ui, w, "Разлив", g.spread, 0.2, 0.0..=200.0, 1, KeyState::Hidden).changed {
            edits.push(("glow", Some(glow_text(&Glow { spread: (v * 10.0).round() / 10.0, ..g }))));
        }
    });
    on && node.glow.is_some()
}

/// Inner shadow on or off, and its colour; its offset and blur are keyable fields of the caller.
pub fn inset_rows(ui: &mut egui::Ui, node: &Node, edits: &mut Vec<StyleEdit>) -> bool {
    let mut on = node.inset.is_some();
    ui.horizontal(|ui| {
        if ui.checkbox(&mut on, RichText::new("Внутренняя тень").size(12.5).color(FG)).changed() {
            let s = Shadow { dx: 0.0, dy: 3.0, blur: 6.0, color: Color { r: 0, g: 0, b: 0, a: 0x88 } };
            edits.push(("inset", on.then(|| inset_text(&s))));
        }
        let Some(s) = node.inset.filter(|_| on) else { return };
        if let Some(color) = alpha_swatch(ui, s.color) {
            edits.push(("inset", Some(inset_text(&Shadow { color, ..s }))));
        }
    });
    on && node.inset.is_some()
}

/// Grain on or off, and the size of a speck; its strength is a keyable field of the caller.
pub fn noise_rows(ui: &mut egui::Ui, node: &Node, edits: &mut Vec<StyleEdit>) -> bool {
    let mut on = node.noise.is_some();
    ui.horizontal(|ui| {
        if ui.checkbox(&mut on, RichText::new("Шум").size(12.5).color(FG)).changed() {
            edits.push(("noise", on.then(|| noise_text(&Noise { amount: 0.12, ..Noise::DEFAULT }))));
        }
        let Some(n) = node.noise.filter(|_| on) else { return };
        let w = ui.available_width();
        if let Some(v) = theme::num_field(ui, w, "Крупинка", n.size, 0.05, 0.1..=50.0, 1, KeyState::Hidden).changed {
            edits.push(("noise", Some(noise_text(&Noise { size: (v * 10.0).round() / 10.0, ..n }))));
        }
    });
    on && node.noise.is_some()
}

/// Shadow on or off, and its colour. Returns whether the node has a shadow, so the caller can
/// draw its offset and blur as keyable fields.
pub fn shadow_rows(ui: &mut egui::Ui, node: &Node, edits: &mut Vec<StyleEdit>) -> bool {
    let mut on = node.shadow.is_some();
    ui.horizontal(|ui| {
        if ui.checkbox(&mut on, RichText::new("Тень").size(12.5).color(FG)).changed() {
            let s = Shadow { dx: 0.0, dy: 4.0, blur: 8.0, ..Shadow::DEFAULT };
            edits.push(("shadow", on.then(|| shadow_text(&s))));
        }
        let Some(s) = node.shadow.filter(|_| on) else { return };
        // Colour swatch with alpha: shadows are usually translucent.
        if let Some(color) = alpha_swatch(ui, s.color) {
            edits.push(("shadow", Some(shadow_text(&Shadow { color, ..s }))));
        }
    });
    on && node.shadow.is_some()
}

/// Blend mode.
pub fn blend_row(ui: &mut egui::Ui, node: &Node, edits: &mut Vec<StyleEdit>) {
    ui.horizontal(|ui| {
        ui.label(RichText::new("Наложение").size(12.0).color(MUTED));
        let mut pick = None;
        egui::ComboBox::from_id_salt(("blend", &node.id)).width(ui.available_width() - 8.0).selected_text(node.blend.keyword()).show_ui(ui, |ui| {
            for b in Blend::ALL {
                if ui.selectable_label(b == node.blend, b.keyword()).clicked() {
                    pick = Some(b);
                }
            }
        });
        if let Some(b) = pick
            && b != node.blend
        {
            edits.push(("blend", (b != Blend::Normal).then(|| format!("blend {}", b.keyword()))));
        }
    });
}

impl App {
    /// Writes the collected whole-property edits to the script.
    pub(crate) fn apply_styles(&mut self, node: &str, edits: Vec<StyleEdit>) {
        for (key, text) in edits {
            if !self.can_edit() {
                return;
            }
            match motif_script::set_style(&self.src, &self.scene, node, key, text.as_deref()) {
                Some(out) => {
                    self.apply(&format!("{node}.{key}"), Some(out));
                }
                None => self.status = "Свойства объекта из цикла меняются в тексте цикла".into(),
            }
        }
    }
}
