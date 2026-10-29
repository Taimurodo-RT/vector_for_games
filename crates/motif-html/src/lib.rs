//! Converts a deliberately bounded HTML/CSS subset into ordinary editable MSC nodes.
//! No browser, script evaluation, network, or external resource loading is involved.
mod animation;
mod canvas;
mod css;
mod interpolate;
mod layout;
mod semantic;
mod style;
mod timeline;

use html5gum::{Token, Tokenizer};
use motif_core::text::{BOLD_FONT, DEFAULT_FONT};
use motif_core::{TextShape, TextStyle};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, PartialEq)]
pub struct ImportResult {
    pub source: String,
    pub warnings: Vec<String>,
    pub elements: usize,
}

#[derive(Default, Clone, Debug)]
struct Element {
    tag: String,
    attrs: BTreeMap<String, String>,
    parent: Option<usize>,
    children: Vec<usize>,
    text: String,
    suppressed: bool,
    placeholder: bool,
    text_part: bool,
}

/// Imports UTF-8 markup and an additional stylesheet. Embedded styles precede `css`.
pub fn import(html: &str, css: &str) -> Result<ImportResult, String> {
    if html.len() > 200_000 || css.len() > 200_000 {
        return Err("Фрагмент HTML/CSS должен быть не больше 200 КБ на поле".into());
    }
    let mut warnings = Vec::new();
    let (mut nodes, embedded) = parse_html(html, &mut warnings)?;
    semantic::prepare(&mut nodes, &mut warnings)?;
    let sheet = css::parse_stylesheet(&format!("{embedded}\n{css}"), &mut warnings)?;
    let rules = &sheet.rules;
    let styles = computed(&nodes, rules, css::State::Base, &mut warnings);
    semantic::normalize_text(&mut nodes, &styles);
    semantic::warn_inline_limits(&nodes, &styles, &mut warnings);
    let mut boxes = vec![BoxNode::default(); nodes.len()];
    layout::measure(0, &nodes, &styles, &mut boxes);
    layout::position(0, 24.0, 24.0, &nodes, &styles, &mut boxes);
    let width = (boxes[0].w + 48.0).clamp(64.0, 8192.0);
    let height = (boxes[0].h + 48.0).clamp(64.0, 8192.0);
    let mut ids = Vec::new();
    let mut used = BTreeSet::new();
    for (i, n) in nodes.iter().enumerate() {
        let name = n.attrs.get("id").map_or_else(|| format!("{}_{}", n.tag.replace('#', ""), i), Clone::clone);
        let mut id: String = name.chars().map(|c| if c.is_ascii_alphanumeric() || c == '_' { c } else { '_' }).collect();
        if id.is_empty() || !id.starts_with(|c: char| c.is_ascii_alphabetic() || c == '_') {
            id = format!("html_{id}");
        }
        if motif_script::prop_arity(&id).is_some_and(|(_, max)| max == 0)
            || ["world", "artboard", "anim", "group", "rect", "text", "path", "let", "parameter", "driver", "corrective"].contains(&id.as_str())
        {
            id = format!("html_{id}");
        }
        let base = id.clone();
        let mut suffix = 2;
        let lines = n.text.split('\n').count();
        while reserved_ids(&id, lines).iter().any(|candidate| used.contains(candidate)) {
            id = format!("{base}_{suffix}");
            suffix += 1;
        }
        used.extend(reserved_ids(&id, lines));
        ids.push(id);
    }
    let mut source =
        format!("// Imported HTML/CSS subset: editable groups, backgrounds and text.\nartboard \"HTML UI\" {}x{} bg #f5f6fa ui\n", num(width), num(height));
    for &child in &nodes[0].children {
        emit(child, 0, &nodes, &styles, &boxes, &ids, &mut source);
    }
    timeline::emit_clips(&nodes, &sheet, &ids, &styles, &mut source, &mut warnings)?;
    canvas::fit(&mut source, &mut warnings)?;
    // Validate the result before returning it to a visual editor or generator.
    motif_script::parse(&source).map_err(|e| format!("Не удалось построить сцену: {}", e.iter().map(ToString::to_string).collect::<Vec<_>>().join("; ")))?;
    let mut seen = BTreeSet::new();
    warnings.retain(|w| seen.insert(w.clone()));
    let elements = nodes.iter().skip(1).filter(|n| n.tag != "#text").count();
    Ok(ImportResult { source, warnings, elements })
}

fn parse_html(html: &str, warnings: &mut Vec<String>) -> Result<(Vec<Element>, String), String> {
    let mut nodes = vec![Element { tag: "root".into(), ..Default::default() }];
    let mut stack = vec![0];
    let mut embedded = String::new();
    let mut skipped: Option<String> = None;
    let mut style = false;
    let mut emitter = html5gum::emitters::default::DefaultEmitter::default();
    emitter.naively_switch_states(true);
    let tokenizer = Tokenizer::new_with_emitter(html, emitter);
    for result in tokenizer {
        let token = result.map_err(|e| format!("HTML: {e}"))?;
        match token {
            Token::StartTag(tag) => {
                let name = String::from_utf8_lossy(&tag.name).to_lowercase();
                if skipped.is_some() {
                    continue;
                }
                if name == "style" {
                    style = true;
                    continue;
                }
                if ["script", "iframe", "object", "template"].contains(&name.as_str()) {
                    warnings.push(format!("<{name}> пропущен; код и внешние ресурсы не выполняются"));
                    skipped = Some(name);
                    continue;
                }
                if ["link", "img", "video", "audio", "source", "canvas", "svg"].contains(&name.as_str()) {
                    warnings.push(format!("<{name}> не поддерживается; внешние ресурсы не загружаются"));
                    if name == "svg" || name == "canvas" {
                        skipped = Some(name);
                    }
                    continue;
                }
                if ["meta", "title", "head", "base"].contains(&name.as_str()) {
                    if name == "title" {
                        skipped = Some(name);
                    }
                    continue;
                }
                if ![
                    "html", "body", "main", "section", "article", "div", "button", "span", "p", "label", "h1", "h2", "h3", "a", "strong", "b", "em", "i", "h4",
                    "h5", "h6", "small", "u", "s", "sub", "sup", "code", "pre", "br", "hr", "ul", "ol", "li", "input", "textarea", "select", "option", "form",
                    "ins", "del", "header", "footer", "nav", "aside", "kbd", "samp", "mark",
                ]
                .contains(&name.as_str())
                {
                    warnings.push(format!("<{name}> импортируется как обычная группа"));
                }
                let attrs: BTreeMap<String, String> =
                    tag.attributes.into_iter().map(|(k, v)| (String::from_utf8_lossy(&k).to_string(), String::from_utf8_lossy(&v).to_string())).collect();
                for key in attrs.keys() {
                    if key.starts_with("on") {
                        warnings.push(format!("Атрибут {key} пропущен: обработчики событий не импортируются"));
                    }
                }
                if name == "a" && attrs.contains_key("href") {
                    warnings.push("Ссылка импортирована как рисунок; переход по href не создаётся".into());
                }
                if ["button", "input", "select", "textarea"].contains(&name.as_str()) && attrs.contains_key("disabled") {
                    warnings.push("HTML disabled: импортирован базовый вид; недоступность поведения задайте в проверке UI или в игровом контроле".into());
                }
                let p = *stack.last().unwrap();
                let i = nodes.len();
                let void = ["br", "hr", "input"].contains(&name.as_str());
                nodes.push(Element { tag: name, attrs, parent: Some(p), ..Default::default() });
                nodes[p].children.push(i);
                if nodes.len() > 2048 {
                    return Err("Не больше 2048 узлов в HTML-фрагменте".into());
                }
                if !tag.self_closing && !void {
                    stack.push(i);
                    if stack.len() > 64 {
                        return Err("Глубина HTML не больше 64".into());
                    }
                }
            }
            Token::EndTag(tag) => {
                let name = String::from_utf8_lossy(&tag.name).to_lowercase();
                if skipped.as_deref() == Some(&name) {
                    skipped = None;
                    continue;
                }
                if skipped.is_some() {
                    continue;
                }
                if name == "style" {
                    style = false;
                    continue;
                }
                if let Some(at) = stack.iter().rposition(|i| nodes[*i].tag == name) {
                    stack.truncate(at.max(1));
                }
            }
            Token::String(text) => {
                if skipped.is_some() {
                    continue;
                }
                let text = String::from_utf8_lossy(&text);
                if style {
                    embedded.push_str(&text);
                } else {
                    let p = *stack.last().unwrap();
                    add_text(&mut nodes, p, &text);
                }
            }
            Token::Error(_) | Token::Comment(_) | Token::Doctype(_) => {}
        }
    }
    if stack.len() > 1 {
        warnings.push("Незакрытые HTML-теги завершены в конце фрагмента".into());
    }
    Ok((nodes, embedded))
}

fn add_text(nodes: &mut Vec<Element>, parent: usize, text: &str) {
    if let Some(&last) = nodes[parent].children.last()
        && nodes[last].tag == "#text"
    {
        nodes[last].text.push_str(text);
        return;
    }
    let i = nodes.len();
    nodes.push(Element { tag: "#text".into(), parent: Some(parent), text: text.into(), ..Default::default() });
    nodes[parent].children.push(i);
}

fn computed(nodes: &[Element], rules: &[css::Rule], state: css::State, warnings: &mut Vec<String>) -> Vec<style::Style> {
    let mut styles = Vec::new();
    for (i, node) in nodes.iter().enumerate() {
        let parent = node.parent.map(|p| &styles[p]);
        let declarations = css::cascade(rules, nodes, i, state, warnings);
        let mut style = style::Style::compute(node, parent, &declarations, warnings);
        style.hidden |= node.suppressed;
        styles.push(style);
    }
    styles
}

#[derive(Clone, Default)]
struct BoxNode {
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    baseline: f64,
}

fn text_size(text: &str, s: &style::Style) -> (f64, f64) {
    let t = TextShape::new(text, if s.bold { BOLD_FONT } else { DEFAULT_FONT }).unwrap();
    let style = TextStyle {
        size: s.font_size,
        line_height: s.line_height,
        spacing: s.letter_spacing,
        align: motif_core::TextAlign::Center,
        reveal: 1.0,
        wave: 0.0,
        phase: 0.0,
    };
    let b = t.bounds(&style);
    (b.width(), b.height())
}
fn reserved_ids(id: &str, lines: usize) -> Vec<String> {
    let mut all = semantic::reserved_ids(id, lines);
    all.extend(["polygon", "polygon_mask", "overflow", "overflow_mask", "border_outer", "border_inner"].map(|suffix| format!("{id}__{suffix}")));
    all
}

/// QR decomposition into Motif's rotation * skewX * scale convention.
fn affine_parts(affine: motif_core::kurbo::Affine) -> [f64; 6] {
    let [a, b, c, d, x, y] = affine.as_coeffs();
    let sx = a.hypot(b);
    if sx < 1e-12 {
        return [x, y, (-c).atan2(d).to_degrees(), 0.0, c.hypot(d), 0.0];
    }
    let sy = (a * d - b * c) / sx;
    let shear = if sy.abs() < 1e-12 { 0.0 } else { ((a * c + b * d) / (sx * sy)).atan().to_degrees() };
    [x, y, b.atan2(a).to_degrees(), sx, sy, shear]
}

fn emit(i: usize, depth: usize, nodes: &[Element], styles: &[style::Style], boxes: &[BoxNode], ids: &[String], out: &mut String) {
    let n = &nodes[i];
    let s = &styles[i];
    if s.hidden || n.tag == "br" {
        return;
    }
    let b = &boxes[i];
    let parent = n.parent.filter(|p| *p != 0).map(|p| &boxes[p]);
    let x = b.x + b.w / 2.0 - parent.map_or(0.0, |p| p.x + p.w / 2.0);
    let y = b.y + b.h / 2.0 - parent.map_or(0.0, |p| p.y + p.h / 2.0);
    let ind = "  ".repeat(depth);
    let id = &ids[i];
    if n.tag == "#text" {
        if !n.text.is_empty() || n.text_part {
            let mut owner = n.parent;
            while let Some(p) = owner {
                if !semantic::role(&nodes[p]).is_empty() {
                    break;
                }
                owner = nodes[p].parent;
            }
            let part = if owner.is_some() { " part text" } else { "" };
            out.push_str(&format!(
                "{ind}text {id}_text at {} {} content {} font-size {} line-height {} letter-spacing {} fill {}{}{}{} align left{part}\n",
                num(x - b.w / 2.0),
                num(y),
                motif_script::quoted_content(&n.text),
                num(s.font_size),
                num(s.line_height),
                num(s.letter_spacing),
                s.color,
                if s.bold { " font bold" } else { "" },
                if s.italic { " skew -12 0" } else { "" },
                if n.placeholder { " opacity 0.45" } else { "" },
            ));
            let lh = s.font_size * s.line_height;
            for (line, text) in n.text.split('\n').enumerate() {
                let width = text_size(text, s).0;
                for (suffix, active, offset) in [("underline", s.underline, 0.36), ("strike", s.strike, -0.04)] {
                    if active {
                        out.push_str(&format!(
                            "{ind}rect {id}_{suffix}_{line} at {} {} size {} {} fill {}\n",
                            num(x - b.w / 2.0 + width / 2.0),
                            num(y - b.h / 2.0 + lh * (line as f64 + 0.5) + s.font_size * offset),
                            num(width),
                            num((s.font_size / 16.0).max(1.0)),
                            s.color
                        ));
                    }
                }
            }
        }
        return;
    }
    let kind = semantic::role(n);
    let mut role = if kind.is_empty() { String::new() } else { format!(" ui {kind}") };
    if n.tag == "input" && semantic::input_type(n) == "radio" {
        let name: String = n.attrs.get("name").unwrap_or(id).chars().map(|c| if c.is_ascii_alphanumeric() || c == '_' { c } else { '_' }).collect();
        role.push_str(&format!(" radio html_radio_{name}"));
    }
    if kind == "slider" {
        role.push_str(&format!(" dial across value {}", num(semantic::range_value(n))));
    }
    let [tx, ty, rotation, sx, sy, skew] = affine_parts(s.transform_matrix(b.w, b.h));
    out.push_str(&format!(
        "{ind}group {id} {} at {} {}{role} opacity {} rot {} scale {} {} skew {} 0 {{\n",
        quote(n.attrs.get("aria-label").map_or(n.tag.as_str(), String::as_str)),
        num(x + tx),
        num(y + ty),
        num(s.opacity),
        num(rotation),
        num(sx),
        num(sy),
        num(skew)
    ));
    let mut inner_depth = depth + 1;
    if let Some(polygon) = &s.clip_path {
        let child = "  ".repeat(inner_depth);
        let mut path = String::new();
        for (at, point) in polygon.iter().enumerate() {
            path.push_str(&format!(
                "{} {} {} ",
                if at == 0 { "M" } else { "L" },
                num(point[0].resolve(b.w) - b.w / 2.0),
                num(point[1].resolve(b.h) - b.h / 2.0)
            ));
        }
        path.push('Z');
        out.push_str(&format!("{child}group {id}__polygon clip {{\n{child}  path {id}__polygon_mask d {} fill #ffffff\n", quote(&path)));
        inner_depth += 1;
    }
    let child = "  ".repeat(inner_depth);
    let radius = surface_radius(s, b);
    let attrs = s.gradient.as_ref().map_or(String::new(), |g| format!(" gradient linear {} {}", num(g.0), g.1.join(" ")));
    let shadow = s.shadow.as_ref().map_or(String::new(), |v| format!(" shadow {} {} {} {}", num(v.0), num(v.1), num(v.2), v.3));
    let inset = s.inset_shadow.as_ref().map_or(String::new(), |v| format!(" inset {} {} {} {}", num(v.0), num(v.1), num(v.2), v.3));
    let double = s.border_double && s.border_width >= 3.0;
    out.push_str(&format!(
        "{child}rect {id}_bg {} size {} {} r {} fill {} stroke {} {}{attrs}{shadow}{inset}\n",
        quote("Фон"),
        num((b.w - if double { 0.0 } else { s.border_width }).max(0.0)),
        num((b.h - if double { 0.0 } else { s.border_width }).max(0.0)),
        num(radius),
        s.background,
        if double { "none" } else { &s.border_color },
        num(if double { 0.0 } else { s.border_width })
    ));
    if double {
        let band = s.border_width / 3.0;
        for (suffix, inset) in [("outer", band / 2.0), ("inner", s.border_width - band / 2.0)] {
            out.push_str(&format!(
                "{child}rect {id}__border_{suffix} size {} {} r {} fill none stroke {} {}\n",
                num((b.w - 2.0 * inset).max(0.0)),
                num((b.h - 2.0 * inset).max(0.0)),
                num((s.radius - inset).max(0.0)),
                s.border_color,
                num(band)
            ));
        }
    }
    semantic::emit_parts(n, s, b, id, &child, out);
    if s.overflow_hidden {
        out.push_str(&format!(
            "{child}group {id}__overflow clip {{\n{child}  rect {id}__overflow_mask size {} {} r {} fill #ffffff\n",
            num((b.w - 2.0 * s.border_width).max(0.0)),
            num((b.h - 2.0 * s.border_width).max(0.0)),
            num((s.radius - s.border_width).max(0.0))
        ));
        inner_depth += 1;
    }
    for &c in &n.children {
        emit(c, inner_depth, nodes, styles, boxes, ids, out);
    }
    if s.overflow_hidden {
        out.push_str(&format!("{child}}}\n"));
    }
    if s.clip_path.is_some() {
        out.push_str(&format!("{ind}  }}\n"));
    }
    out.push_str(&format!("{ind}}}\n"));
}
fn num(v: f64) -> String {
    motif_script::fmt_num_with(v, 6)
}
fn surface_radius(s: &style::Style, b: &BoxNode) -> f64 {
    (s.radius - s.border_width / 2.0).max(0.0).min((b.w.min(b.h) - s.border_width).max(0.0) / 2.0)
}
fn quote(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\"").replace('\n', "\\n").replace('\r', ""))
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod complex_tests;
