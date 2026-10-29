use crate::{Element, css};
use motif_core::Color;
use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Style {
    pub width: Option<f64>,
    pub height: Option<f64>,
    pub padding: [f64; 4],
    pub margin: [f64; 4],
    pub gap: f64,
    pub row: bool,
    pub flex: bool,
    pub inline: bool,
    pub align_center: bool,
    pub justify_center: bool,
    pub hidden: bool,
    pub border_box: bool,
    pub font_size: f64,
    pub line_height: f64,
    pub letter_spacing: f64,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub strike: bool,
    /// Vertical offset in em relative to this element's font size. Not inherited.
    pub baseline_shift: f64,
    pub preserve_space: bool,
    pub tab_size: usize,
    pub color: String,
    pub background: String,
    pub border_color: String,
    pub border_width: f64,
    pub radius: f64,
    pub opacity: f64,
    pub gradient: Option<(f64, Vec<String>)>,
    pub shadow: Option<(f64, f64, f64, String)>,
    vars: BTreeMap<String, String>,
}
impl Default for Style {
    fn default() -> Self {
        Self {
            width: None,
            height: None,
            padding: [0.0; 4],
            margin: [0.0; 4],
            gap: 0.0,
            row: false,
            flex: false,
            inline: false,
            align_center: false,
            justify_center: false,
            hidden: false,
            border_box: false,
            font_size: 16.0,
            line_height: 1.2,
            letter_spacing: 0.0,
            bold: false,
            italic: false,
            underline: false,
            strike: false,
            baseline_shift: 0.0,
            preserve_space: false,
            tab_size: 8,
            color: "#202635".into(),
            background: "none".into(),
            border_color: "none".into(),
            border_width: 0.0,
            radius: 0.0,
            opacity: 1.0,
            gradient: None,
            shadow: None,
            vars: BTreeMap::new(),
        }
    }
}
impl Style {
    pub fn geometry_differs(&self, next: &Self) -> bool {
        self.width != next.width
            || self.height != next.height
            || self.padding != next.padding
            || self.margin != next.margin
            || self.row != next.row
            || self.flex != next.flex
            || self.inline != next.inline
            || self.gap != next.gap
            || self.align_center != next.align_center
            || self.justify_center != next.justify_center
            || self.hidden != next.hidden
            || self.border_width != next.border_width
            || self.italic != next.italic
            || self.underline != next.underline
            || self.strike != next.strike
            || self.baseline_shift != next.baseline_shift
            || self.preserve_space != next.preserve_space
            || self.tab_size != next.tab_size
    }
    pub fn compute(node: &Element, parent: Option<&Self>, declarations: &BTreeMap<String, String>, warnings: &mut Vec<String>) -> Self {
        let mut s = Self::default();
        if let Some(p) = parent {
            s.font_size = p.font_size;
            s.line_height = p.line_height;
            s.letter_spacing = p.letter_spacing;
            s.bold = p.bold;
            s.italic = p.italic;
            s.underline = p.underline;
            s.strike = p.strike;
            s.preserve_space = p.preserve_space;
            s.tab_size = p.tab_size;
            s.color = p.color.clone();
            s.hidden = p.hidden;
            s.vars = p.vars.clone();
        }
        if node.tag == "button" {
            s.padding = [8.0, 16.0, 8.0, 16.0];
            s.background = "#e9edf5".into();
            s.border_color = "#7b8496".into();
            s.border_width = 1.0;
            s.radius = 4.0;
            s.align_center = true;
            s.justify_center = true;
        }
        if [
            "#text", "span", "strong", "b", "em", "i", "u", "s", "del", "ins", "small", "sub", "sup", "code", "kbd", "samp", "mark", "button", "label",
            "input", "textarea", "select", "a",
        ]
        .contains(&node.tag.as_str())
        {
            s.inline = true;
            s.row = true;
        }
        if ["b", "strong", "h1", "h2", "h3", "h4", "h5", "h6"].contains(&node.tag.as_str()) {
            s.bold = true;
        }
        if ["h1", "h2", "h3", "h4", "h5", "h6"].contains(&node.tag.as_str()) {
            s.font_size = match node.tag.as_str() {
                "h1" => 32.0,
                "h2" => 24.0,
                "h3" => 20.0,
                "h4" => 16.0,
                "h5" => 13.28,
                _ => 10.72,
            };
        }
        if ["em", "i"].contains(&node.tag.as_str()) {
            s.italic = true;
        }
        if ["u", "ins"].contains(&node.tag.as_str()) {
            s.underline = true;
        }
        if ["s", "del"].contains(&node.tag.as_str()) {
            s.strike = true;
        }
        if node.tag == "a" && node.attrs.contains_key("href") {
            s.color = "#0066cc".into();
            s.underline = true;
        }
        if node.tag == "ul" || node.tag == "ol" {
            s.padding[3] = 24.0;
            s.gap = 4.0;
        }
        if ["small", "sub", "sup"].contains(&node.tag.as_str()) {
            s.font_size *= 0.8;
        }
        if node.tag == "sub" {
            s.baseline_shift = 0.2;
        }
        if node.tag == "sup" {
            s.baseline_shift = -0.35;
        }
        if node.tag == "pre" {
            s.preserve_space = true;
        }
        if ["pre", "code", "kbd", "samp"].contains(&node.tag.as_str()) {
            warnings.push(format!("HTML <{}>: использован встроенный IBM Plex Sans; моноширинный шрифт автоматически не подключается", node.tag));
        }
        if node.tag == "input" {
            let input_type = node.attrs.get("type").map_or("text", String::as_str).to_ascii_lowercase();
            match input_type.as_str() {
                "hidden" => s.hidden = true,
                "button" | "submit" | "reset" => {
                    s.padding = [8.0, 16.0, 8.0, 16.0];
                    s.background = "#e9edf5".into();
                    s.border_color = "#7b8496".into();
                    s.border_width = 1.0;
                    s.radius = 4.0;
                    s.align_center = true;
                    s.justify_center = true;
                }
                "checkbox" | "radio" => {
                    s.width = Some(18.0);
                    s.height = Some(18.0);
                    s.border_box = true;
                    s.border_width = 1.0;
                    s.border_color = "#7b8496".into();
                    s.background = "#ffffff".into();
                    s.radius = if input_type == "radio" { 9.0 } else { 3.0 };
                }
                "range" => {
                    s.width = Some(160.0);
                    s.height = Some(20.0);
                    s.border_box = true;
                }
                _ => {
                    s.width = Some(180.0);
                    s.padding = [6.0, 8.0, 6.0, 8.0];
                    s.border_box = true;
                    s.border_width = 1.0;
                    s.border_color = "#7b8496".into();
                    s.background = "#ffffff".into();
                    s.radius = 3.0;
                }
            }
        }
        if node.tag == "textarea" || node.tag == "select" {
            s.width = Some(if node.tag == "textarea" { 220.0 } else { 180.0 });
            s.height = Some(if node.tag == "textarea" { 72.0 } else { 34.0 });
            s.padding = [6.0, 8.0, 6.0, 8.0];
            s.border_box = true;
            s.border_width = 1.0;
            s.border_color = "#7b8496".into();
            s.background = "#ffffff".into();
            s.radius = 3.0;
        }
        if node.tag == "textarea" {
            s.preserve_space = true;
        }
        if node.tag == "hr" {
            s.width = Some(160.0);
            s.height = Some(1.0);
            s.background = "#7b8496".into();
            s.border_box = true;
        }
        for (key, value) in declarations {
            if key.starts_with("--") {
                s.vars.insert(key.clone(), value.clone());
            }
        }
        let values: BTreeMap<_, _> = declarations
            .iter()
            .map(|(key, v)| {
                let value = resolve(v, &s.vars);
                if value.is_none() {
                    warnings.push(format!("CSS {key}: переменная не найдена или циклична"));
                }
                (key.clone(), value.unwrap_or_else(|| v.clone()))
            })
            .collect();
        if let Some(size) = values.get("font-size") {
            if let Some(size) = length(size, parent.map_or(16.0, |p| p.font_size)) {
                s.font_size = size.clamp(1.0, 512.0);
            } else {
                bad("font-size", size, warnings);
            }
        }
        if let Some(value) = values.get("color")
            && let Some(value) = color(value, &s.color)
        {
            s.color = value;
        }
        for (key, value) in &values {
            let len = |v: &str| length(v, s.font_size);
            let positive = |v: &str| len(v).filter(|v| *v >= 0.0);
            match key.as_str() {
                key if key.starts_with("--") => {}
                "font-size" => {}
                "width" | "height" => {
                    let v = if value == "auto" { None } else { positive(value) };
                    if v.is_none() && value != "auto" {
                        bad(key, value, warnings);
                    }
                    if key == "width" {
                        s.width = v;
                    } else {
                        s.height = v;
                    }
                }
                "padding-top" | "padding-right" | "padding-bottom" | "padding-left" | "margin-top" | "margin-right" | "margin-bottom" | "margin-left" => {
                    if let Some(v) = len(value) {
                        let index = match key.rsplit('-').next().unwrap() {
                            "top" => 0,
                            "right" => 1,
                            "bottom" => 2,
                            _ => 3,
                        };
                        if key.starts_with("padding") {
                            s.padding[index] = v.max(0.0);
                        } else {
                            s.margin[index] = v;
                        }
                    } else if value == "auto" && key.starts_with("margin") {
                        warnings.push(format!("CSS {key}: auto заменён на 0; используйте flex для выравнивания"));
                    } else {
                        bad(key, value, warnings);
                    }
                }
                "gap" => {
                    if let Some(v) = positive(value) {
                        s.gap = v;
                    } else {
                        bad(key, value, warnings);
                    }
                }
                "display" => match value.as_str() {
                    "none" => s.hidden = true,
                    "flex" | "inline-flex" => {
                        s.flex = true;
                        s.inline = value == "inline-flex";
                        s.row = true;
                    }
                    "block" => {
                        s.flex = false;
                        s.inline = false;
                        s.row = false;
                    }
                    "inline" | "inline-block" => {
                        s.flex = false;
                        s.inline = true;
                        s.row = true;
                    }
                    _ => bad(key, value, warnings),
                },
                "flex-direction" => match value.as_str() {
                    "row" => s.row = true,
                    "column" => s.row = false,
                    _ => bad(key, value, warnings),
                },
                "align-items" => match value.as_str() {
                    "center" => s.align_center = true,
                    "start" | "flex-start" => s.align_center = false,
                    _ => bad(key, value, warnings),
                },
                "justify-content" => match value.as_str() {
                    "center" => s.justify_center = true,
                    "start" | "flex-start" => s.justify_center = false,
                    _ => bad(key, value, warnings),
                },
                "box-sizing" => match value.as_str() {
                    "border-box" => s.border_box = true,
                    "content-box" => s.border_box = false,
                    _ => bad(key, value, warnings),
                },
                "color" => {
                    if let Some(c) = color(value, &s.color) {
                        s.color = c;
                    } else {
                        bad(key, value, warnings)
                    }
                }
                "background-color" => {
                    if let Some(c) = color(value, &s.color) {
                        s.background = c;
                    } else {
                        bad(key, value, warnings)
                    }
                }
                "border-color" => {
                    if let Some(c) = color(value, &s.color) {
                        s.border_color = c;
                    } else {
                        bad(key, value, warnings)
                    }
                }
                "border-width" => {
                    if let Some(v) = positive(value) {
                        s.border_width = v;
                    } else {
                        bad(key, value, warnings)
                    }
                }
                "border-style" => match value.as_str() {
                    "none" | "hidden" => s.border_width = 0.0,
                    "solid" => {}
                    _ => bad(key, value, warnings),
                },
                "border-radius" => {
                    let words = css::words(value);
                    let first = words.first().copied().unwrap_or("");
                    if first == "50%" {
                        s.radius = 10000.0;
                        warnings.push("CSS border-radius 50%: использован равномерный радиус-пилюля".into());
                    } else if let Some(v) = positive(first) {
                        s.radius = v;
                        if words.iter().any(|w| *w != first) {
                            warnings.push("CSS: разные радиусы углов заменены первым радиусом".into());
                        }
                    } else {
                        bad(key, value, warnings);
                    }
                }
                "opacity" => {
                    if let Ok(v) = value.parse::<f64>() {
                        if v.is_finite() {
                            s.opacity = v.clamp(0.0, 1.0);
                        } else {
                            bad(key, value, warnings);
                        }
                    } else {
                        bad(key, value, warnings)
                    }
                }
                "font-weight" => match value.as_str() {
                    "bold" | "600" | "700" | "800" | "900" => s.bold = true,
                    "normal" | "100" | "200" | "300" | "400" | "500" => s.bold = false,
                    _ => bad(key, value, warnings),
                },
                "font-style" => match value.as_str() {
                    "normal" => s.italic = false,
                    "italic" | "oblique" => s.italic = true,
                    _ => bad(key, value, warnings),
                },
                "text-decoration" | "text-decoration-line" => {
                    let parts: Vec<_> = value.split_whitespace().collect();
                    if value == "none" {
                        s.underline = false;
                        s.strike = false;
                    } else if !parts.is_empty() && parts.iter().all(|p| matches!(*p, "underline" | "line-through")) {
                        s.underline = parts.contains(&"underline");
                        s.strike = parts.contains(&"line-through");
                    } else {
                        bad(key, value, warnings);
                    }
                }
                "white-space" => match value.as_str() {
                    "normal" | "nowrap" => s.preserve_space = false,
                    "pre" | "pre-wrap" | "break-spaces" => s.preserve_space = true,
                    _ => bad(key, value, warnings),
                },
                "tab-size" => match value.parse::<usize>() {
                    Ok(size) if (1..=16).contains(&size) => s.tab_size = size,
                    _ => bad(key, value, warnings),
                },
                "vertical-align" => match value.as_str() {
                    "baseline" => s.baseline_shift = 0.0,
                    "sub" => s.baseline_shift = 0.2,
                    "super" => s.baseline_shift = -0.35,
                    _ => bad(key, value, warnings),
                },
                "font-family" => {
                    warnings.push(format!("CSS font-family {value}: использован встроенный IBM Plex Sans; внешний шрифт не загружается"));
                }
                "line-height" => {
                    if value == "normal" {
                        s.line_height = 1.2;
                    } else if let Ok(v) = value.parse::<f64>() {
                        if v.is_finite() && v > 0.0 {
                            s.line_height = v.clamp(0.1, 10.0);
                        } else {
                            bad(key, value, warnings);
                        }
                    } else if let Some(v) = positive(value) {
                        s.line_height = (v / s.font_size).clamp(0.1, 10.0);
                    } else {
                        bad(key, value, warnings);
                    }
                }
                "letter-spacing" => {
                    if value == "normal" {
                        s.letter_spacing = 0.0;
                    } else if let Some(v) = len(value) {
                        s.letter_spacing = v;
                    } else {
                        bad(key, value, warnings)
                    }
                }
                "background-image" => {
                    if value == "none" {
                        s.gradient = None;
                    } else {
                        s.gradient = gradient(value, &s.color);
                        if s.gradient.is_none() {
                            bad(key, value, warnings);
                        }
                    }
                }
                "box-shadow" => {
                    if value == "none" {
                        s.shadow = None;
                    } else {
                        s.shadow = shadow(value, &s.color);
                        if s.shadow.is_none() {
                            bad(key, value, warnings);
                        }
                    }
                }
                "text-align" => {
                    if value != "center" && value != "left" {
                        bad(key, value, warnings);
                    } else if value == "center" {
                        s.align_center = true;
                    }
                }
                "cursor" | "user-select" | "-webkit-user-select" | "outline" | "outline-offset" => {
                    warnings.push(format!("CSS {key}: не имеет редактируемого графического эквивалента и пропущен"))
                }
                _ => warnings.push(format!("CSS-свойство не поддерживается: {key}: {value}")),
            }
        }
        s
    }
}
fn bad(key: &str, value: &str, warnings: &mut Vec<String>) {
    warnings.push(format!("CSS {key}: неподдерживаемое значение «{value}»; сохранено базовое значение"));
}
fn length(text: &str, font_size: f64) -> Option<f64> {
    let text = text.trim();
    let (v, factor) = if let Some(v) = text.strip_suffix("px") {
        (v, 1.0)
    } else if let Some(v) = text.strip_suffix("rem") {
        (v, 16.0)
    } else if let Some(v) = text.strip_suffix("em") {
        (v, font_size)
    } else if text == "0" {
        (text, 1.0)
    } else {
        return None;
    };
    let n = v.parse::<f64>().ok()? * factor;
    (n.is_finite() && n.abs() <= 8192.0).then_some(n)
}
fn resolve(value: &str, vars: &BTreeMap<String, String>) -> Option<String> {
    let mut value = value.to_string();
    for _ in 0..12 {
        let Some(start) = value.find("var(") else { return Some(value) };
        let end = start + value[start..].find(')')?;
        let inner = &value[start + 4..end];
        let (key, fallback) = inner.split_once(',').map_or((inner, None), |(a, b)| (a, Some(b.trim())));
        let replace = vars.get(key.trim()).map(String::as_str).or(fallback)?.to_string();
        value.replace_range(start..=end, &replace);
    }
    None
}
pub(crate) fn color(value: &str, current: &str) -> Option<String> {
    let lower = value.trim().to_lowercase();
    let named = match lower.as_str() {
        "none" | "transparent" => "#00000000",
        "currentcolor" => return Some(current.into()),
        "black" => "#000000",
        "white" => "#ffffff",
        "red" => "#ff0000",
        "green" => "#008000",
        "blue" => "#0000ff",
        "yellow" => "#ffff00",
        "gray" | "grey" => "#808080",
        "silver" => "#c0c0c0",
        "orange" => "#ffa500",
        "purple" => "#800080",
        "pink" => "#ffc0cb",
        "rebeccapurple" => "#663399",
        _ => lower.as_str(),
    };
    if let Some(c) = Color::from_hex(named) {
        return Some(c.to_hex());
    }
    let inner = lower.strip_prefix("rgba(").or_else(|| lower.strip_prefix("rgb("))?.strip_suffix(')')?;
    let normalized = inner.replace(['/', ','], " ");
    let values: Vec<_> = normalized.split_whitespace().collect();
    if !(3..=4).contains(&values.len()) {
        return None;
    }
    let channel = |s: &str| -> Option<u8> {
        let n = if let Some(s) = s.strip_suffix('%') { s.parse::<f64>().ok()? * 2.55 } else { s.parse::<f64>().ok()? };
        n.is_finite().then(|| n.round().clamp(0.0, 255.0) as u8)
    };
    let a = if let Some(a) = values.get(3) {
        let v = if let Some(a) = a.strip_suffix('%') { a.parse::<f64>().ok()? / 100.0 } else { a.parse::<f64>().ok()? };
        if !v.is_finite() {
            return None;
        }
        (v.clamp(0.0, 1.0) * 255.0).round() as u8
    } else {
        255
    };
    Some(Color { r: channel(values[0])?, g: channel(values[1])?, b: channel(values[2])?, a }.to_hex())
}
fn gradient(value: &str, current: &str) -> Option<(f64, Vec<String>)> {
    let inner = value.strip_prefix("linear-gradient(")?.strip_suffix(')')?;
    let mut parts = css::split(inner, ',');
    let mut angle = 90.0;
    if parts.first().is_some_and(|p| p.trim().ends_with("deg") || p.trim().starts_with("to ")) {
        let head = parts.remove(0).trim();
        angle = match head {
            "to right" => 0.0,
            "to bottom" => 90.0,
            "to left" => 180.0,
            "to top" => 270.0,
            _ => head.strip_suffix("deg")?.parse::<f64>().ok()? - 90.0,
        };
    }
    if !angle.is_finite() || !(2..=8).contains(&parts.len()) {
        return None;
    }
    let colors: Option<Vec<_>> = parts.iter().map(|p| color(p.trim(), current)).collect();
    Some((angle, colors?))
}
fn shadow(value: &str, current: &str) -> Option<(f64, f64, f64, String)> {
    let words = css::words(value);
    if value.contains("inset") || css::split(value, ',').len() > 1 || !(3..=5).contains(&words.len()) {
        return None;
    }
    let dx = length(words[0], 16.0)?;
    let dy = length(words[1], 16.0)?;
    let blur = length(words[2], 16.0)?.max(0.0);
    let color = if words.len() == 5 {
        if length(words[3], 16.0)? != 0.0 {
            return None;
        }
        words[4]
    } else {
        *words.get(3).unwrap_or(&"currentColor")
    };
    Some((dx, dy, blur, self::color(color, current)?))
}

#[cfg(test)]
mod style_tests {
    use super::*;
    fn element(tag: &str) -> Element {
        Element { tag: tag.into(), ..Default::default() }
    }
    fn declarations(items: &[(&str, &str)]) -> BTreeMap<String, String> {
        items.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
    }
    #[test]
    fn text_semantics_inherit_and_css_can_override_tag_defaults() {
        let mut warnings = Vec::new();
        let mut parent = Style::compute(
            &element("p"),
            None,
            &declarations(&[("color", "#aabbcc"), ("font-size", "20px"), ("font-style", "italic"), ("text-decoration", "underline line-through")]),
            &mut warnings,
        );
        parent.baseline_shift = 0.2;
        let child = Style::compute(
            &element("strong"),
            Some(&parent),
            &declarations(&[("font-weight", "normal"), ("font-style", "normal"), ("text-decoration-line", "none")]),
            &mut warnings,
        );
        assert_eq!(child.color, "#aabbcc");
        assert_eq!(child.font_size, 20.0);
        assert!(!child.bold && !child.italic && !child.underline && !child.strike);
        assert_eq!(child.baseline_shift, 0.0);
        let sub = Style::compute(&element("sub"), Some(&parent), &BTreeMap::new(), &mut warnings);
        let sup = Style::compute(&element("sup"), Some(&parent), &BTreeMap::new(), &mut warnings);
        assert_eq!(sub.font_size, 16.0);
        assert_eq!(sub.baseline_shift, 0.2);
        assert_eq!(sup.baseline_shift, -0.35);
        assert!(warnings.is_empty(), "{warnings:?}");
    }
    #[test]
    fn whitespace_and_display_are_inherited_or_local_as_required() {
        let mut warnings = Vec::new();
        let pre = Style::compute(&element("pre"), None, &declarations(&[("tab-size", "4")]), &mut warnings);
        assert!(pre.preserve_space && !pre.inline && !pre.flex);
        assert_eq!(pre.tab_size, 4);
        let span = Style::compute(
            &element("span"),
            Some(&pre),
            &declarations(&[("white-space", "normal"), ("display", "inline-flex"), ("flex-direction", "column")]),
            &mut warnings,
        );
        assert!(!span.preserve_space && span.inline && span.flex && !span.row);
        assert_eq!(span.tab_size, 4);
        let child = Style::compute(&element("span"), Some(&span), &BTreeMap::new(), &mut warnings);
        assert!(!child.flex && child.inline && !child.preserve_space);
        assert_eq!(child.tab_size, 4);
        assert!(warnings.iter().all(|w| w.contains("моноширинный")), "{warnings:?}");
    }
    #[test]
    fn simple_controls_lists_and_links_keep_overridable_defaults() {
        let mut warnings = Vec::new();
        let button = Style::compute(&element("button"), None, &BTreeMap::new(), &mut warnings);
        let mut input = element("input");
        input.attrs.insert("type".into(), "SUBMIT".into());
        let submit = Style::compute(&input, None, &BTreeMap::new(), &mut warnings);
        assert_eq!(submit, button);
        input.attrs.insert("type".into(), "checkbox".into());
        let check = Style::compute(&input, None, &declarations(&[("width", "24px"), ("background-color", "#123456")]), &mut warnings);
        assert_eq!(check.width, Some(24.0));
        assert_eq!(check.height, Some(18.0));
        assert_eq!(check.background, "#123456");
        let list = Style::compute(&element("ul"), None, &BTreeMap::new(), &mut warnings);
        assert_eq!(list.padding[3], 24.0);
        assert_eq!(list.gap, 4.0);
        let mut link = element("a");
        link.attrs.insert("href".into(), "/demo".into());
        let link = Style::compute(&link, None, &declarations(&[("color", "#abcdef"), ("text-decoration", "none")]), &mut warnings);
        assert_eq!(link.color, "#abcdef");
        assert!(!link.underline);
        assert!(warnings.is_empty(), "{warnings:?}");
    }
}
