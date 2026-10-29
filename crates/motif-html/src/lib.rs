//! Converts a deliberately bounded HTML/CSS subset into ordinary editable MSC nodes.
//! No browser, script evaluation, network, or external resource loading is involved.
mod css;
mod style;

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
}

/// Imports UTF-8 markup and an additional stylesheet. Embedded styles precede `css`.
pub fn import(html: &str, css: &str) -> Result<ImportResult, String> {
    if html.len() > 200_000 || css.len() > 200_000 {
        return Err("Фрагмент HTML/CSS должен быть не больше 200 КБ на поле".into());
    }
    let mut warnings = Vec::new();
    let (nodes, embedded) = parse_html(html, &mut warnings)?;
    let rules = css::parse(&format!("{embedded}\n{css}"), &mut warnings)?;
    let styles = computed(&nodes, &rules, css::State::Base, &mut warnings);
    let mut boxes = vec![BoxNode::default(); nodes.len()];
    measure(0, &nodes, &styles, &mut boxes);
    position(0, 24.0, 24.0, &nodes, &styles, &mut boxes);
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
        if ["world", "artboard", "anim", "group", "rect", "text", "path", "let", "parameter", "driver", "corrective"].contains(&id.as_str()) {
            id = format!("html_{id}");
        }
        let base = id.clone();
        let mut suffix = 2;
        while [&id, &format!("{id}_bg"), &format!("{id}_text")].iter().any(|candidate| used.contains(*candidate)) {
            id = format!("{base}_{suffix}");
            suffix += 1;
        }
        used.extend([id.clone(), format!("{id}_bg"), format!("{id}_text")]);
        ids.push(id);
    }
    let mut source =
        format!("// Imported HTML/CSS subset: editable groups, backgrounds and text.\nartboard \"HTML UI\" {}x{} bg #f5f6fa ui\n", num(width), num(height));
    for &child in &nodes[0].children {
        emit(child, 0, &nodes, &styles, &boxes, &ids, &mut source);
    }
    for state in css::State::ANIMATED {
        if !rules.iter().any(|r| r.stateful_for(state)) {
            continue;
        }
        let styled = computed(&nodes, &rules, state, &mut warnings);
        let mut tracks = String::new();
        for i in 1..nodes.len() {
            if styles[i].hidden || nodes[i].tag == "#text" && nodes[i].text.is_empty() {
                continue;
            }
            let (base, next) = (&styles[i], &styled[i]);
            let id = &ids[i];
            let mut key = |node: &str, prop: &str, a: String, b: String| {
                if a != b {
                    tracks.push_str(&format!("  {node}.{prop} 0: {a} | 0.2: {b}\n"));
                }
            };
            if nodes[i].tag == "#text" {
                key(&format!("{id}_text"), "fill", base.color.clone(), next.color.clone());
                key(&format!("{id}_text"), "font-size", num(base.font_size), num(next.font_size));
            } else {
                if base.gradient == next.gradient {
                    key(&format!("{id}_bg"), "fill", base.background.clone(), next.background.clone());
                }
                key(&format!("{id}_bg"), "r", num(surface_radius(base, &boxes[i])), num(surface_radius(next, &boxes[i])));
                key(&format!("{id}_bg"), "stroke", base.border_color.clone(), next.border_color.clone());
                key(&format!("{id}_bg"), "stroke_w", num(base.border_width), num(next.border_width));
                key(id, "opacity", num(base.opacity), num(next.opacity));
            }
            if base.geometry_differs(next) {
                warnings.push(format!(
                    "{}: изменение размеров/раскладки в :{} не импортировано; состояние сохраняет базовую геометрию",
                    nodes[i].tag,
                    state.pseudo()
                ));
            }
            if base.gradient != next.gradient || base.shadow != next.shadow {
                warnings.push(format!("{}: изменение градиента или тени в :{} не импортировано", nodes[i].tag, state.pseudo()));
            }
        }
        if !tracks.is_empty() {
            source.push_str(&format!("\nanim {} 0.2s {{\n{tracks}}}\n", state.clip()));
        }
    }
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
                if ["link", "img", "video", "audio", "source", "input", "canvas", "svg"].contains(&name.as_str()) {
                    warnings.push(format!("<{name}> не поддерживается; внешние ресурсы не загружаются"));
                    if name == "svg" || name == "canvas" {
                        skipped = Some(name);
                    }
                    continue;
                }
                if ["meta", "title", "head"].contains(&name.as_str()) {
                    if name == "title" {
                        skipped = Some(name);
                    }
                    continue;
                }
                if name == "br" {
                    let p = *stack.last().unwrap();
                    add_text(&mut nodes, p, "\n");
                    warnings.push("HTML <br>: перенос строки заменён пробелом; в этой версии задайте строки отдельными блоками".into());
                    continue;
                }
                if ![
                    "html", "body", "main", "section", "article", "div", "button", "span", "p", "label", "h1", "h2", "h3", "a", "strong", "b", "em", "i",
                    "small",
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
                if name == "button" && attrs.contains_key("disabled") {
                    warnings.push("HTML disabled: импортирован базовый вид; недоступность поведения задайте в проверке UI или в игровом контроле".into());
                }
                let p = *stack.last().unwrap();
                let i = nodes.len();
                nodes.push(Element { tag: name, attrs, parent: Some(p), ..Default::default() });
                nodes[p].children.push(i);
                if nodes.len() > 2048 {
                    return Err("Не больше 2048 узлов в HTML-фрагменте".into());
                }
                if !tag.self_closing {
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
    let collapsed: Vec<_> = nodes
        .iter()
        .enumerate()
        .map(|(i, n)| {
            if n.tag != "#text" {
                return n.text.clone();
            }
            let text = n.text.split_ascii_whitespace().collect::<Vec<_>>().join(" ");
            if text.is_empty() {
                return text;
            }
            let siblings = &nodes[n.parent.unwrap()].children;
            let prefix = siblings.first() != Some(&i) && n.text.starts_with(|c: char| c.is_ascii_whitespace());
            let suffix = siblings.last() != Some(&i) && n.text.ends_with(|c: char| c.is_ascii_whitespace());
            format!("{}{text}{}", if prefix { " " } else { "" }, if suffix { " " } else { "" })
        })
        .collect();
    for (n, text) in nodes.iter_mut().zip(collapsed) {
        n.text = text;
    }
    // Retain only meaningful text, keeping Unicode/entity-decoded content.
    let blank: BTreeSet<usize> = nodes.iter().enumerate().filter(|(_, n)| n.tag == "#text" && n.text.is_empty()).map(|(i, _)| i).collect();
    for n in &mut nodes {
        n.children.retain(|i| !blank.contains(i));
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
        styles.push(style::Style::compute(node, parent, &declarations, warnings));
    }
    styles
}

#[derive(Clone, Default)]
struct BoxNode {
    x: f64,
    y: f64,
    w: f64,
    h: f64,
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
fn measure(i: usize, nodes: &[Element], styles: &[style::Style], boxes: &mut [BoxNode]) {
    let n = &nodes[i];
    let s = &styles[i];
    if s.hidden {
        return;
    }
    for &c in &n.children {
        measure(c, nodes, styles, boxes);
    }
    if n.tag == "#text" {
        let (w, h) = text_size(&n.text, s);
        boxes[i].w = w;
        boxes[i].h = h;
        return;
    }
    let children: Vec<_> = n.children.iter().copied().filter(|c| !styles[*c].hidden).collect();
    let (mut w, mut h) = (0.0_f64, 0.0_f64);
    for &c in &children {
        let b = &boxes[c];
        let margin = styles[c].margin;
        if s.row {
            w += b.w + margin[1] + margin[3];
            h = h.max(b.h + margin[0] + margin[2]);
        } else {
            w = w.max(b.w + margin[1] + margin[3]);
            h += b.h + margin[0] + margin[2];
        }
    }
    let gaps = s.gap * children.len().saturating_sub(1) as f64;
    if s.row {
        w += gaps;
    } else {
        h += gaps;
    }
    let px = s.padding[1] + s.padding[3] + 2.0 * s.border_width;
    let py = s.padding[0] + s.padding[2] + 2.0 * s.border_width;
    let width = s.width.map(|v| if s.border_box { v } else { v + px }).unwrap_or(w + px);
    let height = s.height.map(|v| if s.border_box { v } else { v + py }).unwrap_or(h + py);
    boxes[i].w = width.clamp(0.0, 8192.0);
    boxes[i].h = height.clamp(0.0, 8192.0);
}
fn position(i: usize, x: f64, y: f64, nodes: &[Element], styles: &[style::Style], boxes: &mut [BoxNode]) {
    boxes[i].x = x;
    boxes[i].y = y;
    let s = &styles[i];
    let b = boxes[i].clone();
    let children: Vec<_> = nodes[i].children.iter().copied().filter(|c| !styles[*c].hidden).collect();
    let iw = (b.w - s.padding[1] - s.padding[3] - 2.0 * s.border_width).max(0.0);
    let ih = (b.h - s.padding[0] - s.padding[2] - 2.0 * s.border_width).max(0.0);
    let extent: f64 = children
        .iter()
        .map(|c| {
            let m = styles[*c].margin;
            if s.row { boxes[*c].w + m[1] + m[3] } else { boxes[*c].h + m[0] + m[2] }
        })
        .sum::<f64>()
        + s.gap * children.len().saturating_sub(1) as f64;
    let mut at = if s.justify_center { ((if s.row { iw } else { ih }) - extent) / 2.0 } else { 0.0 };
    for c in children {
        let m = styles[c].margin;
        let cw = boxes[c].w;
        let ch = boxes[c].h;
        let (cx, cy) = if s.row {
            (at + m[3], if s.align_center { (ih - ch) / 2.0 } else { m[0] })
        } else {
            (if s.align_center { (iw - cw) / 2.0 } else { m[3] }, at + m[0])
        };
        position(c, x + s.padding[3] + s.border_width + cx, y + s.padding[0] + s.border_width + cy, nodes, styles, boxes);
        at += if s.row { cw + m[1] + m[3] } else { ch + m[0] + m[2] };
        at += s.gap;
    }
}
fn emit(i: usize, depth: usize, nodes: &[Element], styles: &[style::Style], boxes: &[BoxNode], ids: &[String], out: &mut String) {
    let n = &nodes[i];
    let s = &styles[i];
    if s.hidden {
        return;
    }
    let b = &boxes[i];
    let parent = n.parent.filter(|p| *p != 0).map(|p| &boxes[p]);
    let x = b.x + b.w / 2.0 - parent.map_or(0.0, |p| p.x + p.w / 2.0);
    let y = b.y + b.h / 2.0 - parent.map_or(0.0, |p| p.y + p.h / 2.0);
    let ind = "  ".repeat(depth);
    let id = &ids[i];
    if n.tag == "#text" {
        if !n.text.is_empty() {
            let mut owner = n.parent;
            while let Some(p) = owner {
                if nodes[p].tag == "button" || nodes[p].tag == "label" {
                    break;
                }
                owner = nodes[p].parent;
            }
            let part = if owner.is_some() { " part text" } else { "" };
            out.push_str(&format!(
                "{ind}text {id}_text at {} {} content {} font-size {} line-height {} letter-spacing {} fill {}{}{part}\n",
                num(x),
                num(y),
                quote(&n.text),
                num(s.font_size),
                num(s.line_height),
                num(s.letter_spacing),
                s.color,
                if s.bold { " font bold" } else { "" }
            ));
        }
        return;
    }
    let role = if n.tag == "button" {
        " ui button"
    } else if n.tag == "label" {
        " ui label"
    } else {
        ""
    };
    out.push_str(&format!(
        "{ind}group {id} {} at {} {}{role} opacity {} {{\n",
        quote(n.attrs.get("aria-label").map_or(n.tag.as_str(), String::as_str)),
        num(x),
        num(y),
        num(s.opacity)
    ));
    let radius = surface_radius(s, b);
    let attrs = s.gradient.as_ref().map_or(String::new(), |g| format!(" gradient linear {} {}", num(g.0), g.1.join(" ")));
    let shadow = s.shadow.as_ref().map_or(String::new(), |v| format!(" shadow {} {} {} {}", num(v.0), num(v.1), num(v.2), v.3));
    out.push_str(&format!(
        "{ind}  rect {id}_bg {} size {} {} r {} fill {} stroke {} {}{attrs}{shadow}\n",
        quote("Фон"),
        num((b.w - s.border_width).max(0.0)),
        num((b.h - s.border_width).max(0.0)),
        num(radius),
        s.background,
        s.border_color,
        num(s.border_width)
    ));
    for &c in &n.children {
        emit(c, depth + 1, nodes, styles, boxes, ids, out);
    }
    out.push_str(&format!("{ind}}}\n"));
}
fn num(v: f64) -> String {
    motif_script::fmt_num(v)
}
fn surface_radius(s: &style::Style, b: &BoxNode) -> f64 {
    (s.radius - s.border_width / 2.0).max(0.0).min((b.w.min(b.h) - s.border_width).max(0.0) / 2.0)
}
fn quote(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\"").replace('\n', "\\n").replace('\r', ""))
}

#[cfg(test)]
mod tests;
