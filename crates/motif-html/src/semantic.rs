use crate::{BoxNode, Element, num, style::Style};

pub(crate) fn input_type(n: &Element) -> &str {
    n.attrs.get("type").map(String::as_str).unwrap_or("text")
}

pub(crate) fn role(n: &Element) -> &'static str {
    match n.tag.as_str() {
        "button" => "button",
        "label" => "label",
        "textarea" => "input",
        "select" => "option",
        "input" => match input_type(n) {
            "button" | "submit" | "reset" => "button",
            "checkbox" | "radio" => "check",
            "range" => "slider",
            _ => "input",
        },
        _ => "",
    }
}

pub(crate) fn reserved_ids(id: &str, lines: usize) -> Vec<String> {
    let mut ids = vec![id.to_owned()];
    ids.extend(["bg", "text", "checked", "checked_bounds", "unchecked", "mark", "track", "fill", "grabber", "arrow"].map(|suffix| format!("{id}_{suffix}")));
    for line in 0..lines {
        ids.push(format!("{id}_underline_{line}"));
        ids.push(format!("{id}_strike_{line}"));
    }
    ids
}

fn caption(nodes: &[Element], i: usize) -> String {
    if nodes[i].tag == "#text" {
        return nodes[i].text.clone();
    }
    nodes[i].children.iter().map(|c| caption(nodes, *c)).collect()
}

fn synthetic_text(nodes: &mut Vec<Element>, parent: usize, text: String, placeholder: bool) -> usize {
    let i = nodes.len();
    nodes.push(Element { tag: "#text".into(), parent: Some(parent), text, placeholder, text_part: true, ..Default::default() });
    nodes[parent].children.push(i);
    i
}

/// Turn form values and list markers into the same editable text nodes as ordinary markup.
pub(crate) fn prepare(nodes: &mut Vec<Element>, warnings: &mut Vec<String>) -> Result<(), String> {
    let original = nodes.len();
    for i in 1..original {
        if let Some(kind) = nodes[i].attrs.get_mut("type") {
            *kind = kind.to_ascii_lowercase();
        }
        match nodes[i].tag.as_str() {
            "pre" | "textarea" => {
                // HTML ignores exactly one immediately following LF in these elements.
                if let Some(c) = nodes[i].children.first().copied()
                    && nodes[c].tag == "#text"
                    && nodes[c].text.starts_with('\n')
                {
                    nodes[c].text.remove(0);
                }
                if nodes[i].tag == "textarea" {
                    for child in nodes[i].children.clone() {
                        nodes[child].text_part = nodes[child].tag == "#text";
                    }
                    if nodes[i].children.is_empty() {
                        let text = nodes[i].attrs.get("placeholder").cloned().unwrap_or_default();
                        synthetic_text(nodes, i, text, true);
                    }
                    warnings.push(
                        "HTML textarea: многострочный рисунок сохранён; роль Motif Input не переносит браузерное редактирование, scroll и ограничения ввода"
                            .into(),
                    );
                }
            }
            "input" => {
                let kind = input_type(&nodes[i]).to_owned();
                if kind == "hidden" {
                    nodes[i].suppressed = true;
                    continue;
                }
                if !["text", "search", "password", "email", "url", "tel", "number", "checkbox", "radio", "range", "button", "submit", "reset"]
                    .contains(&kind.as_str())
                {
                    warnings.push(format!("HTML input type={kind}: показан как текстовое поле; специальный браузерный контроль не переносится"));
                }
                if !["checkbox", "radio", "range"].contains(&kind.as_str()) {
                    let value = nodes[i].attrs.get("value").filter(|v| !v.is_empty()).cloned();
                    let placeholder = value.is_none() && nodes[i].attrs.contains_key("placeholder");
                    let mut text = value.or_else(|| nodes[i].attrs.get("placeholder").cloned()).unwrap_or_else(|| match kind.as_str() {
                        "submit" => "Submit".into(),
                        "reset" => "Reset".into(),
                        _ => String::new(),
                    });
                    if kind == "password" && !placeholder {
                        text = "•".repeat(text.chars().count());
                    }
                    synthetic_text(nodes, i, text, placeholder);
                }
                warnings.push("HTML input: перенесены рисунок, начальное значение и роль UI; min/max/step, валидацию, password, disabled и отправку формы настройте в игровом контроле".into());
            }
            "select" => {
                let options: Vec<_> = nodes[i].children.iter().copied().filter(|c| nodes[*c].tag == "option").collect();
                let selected = options.iter().find(|c| nodes[**c].attrs.contains_key("selected")).or(options.first()).copied();
                let text = selected.map(|c| nodes[c].attrs.get("label").cloned().unwrap_or_else(|| caption(nodes, c))).unwrap_or_default();
                for c in nodes[i].children.clone() {
                    nodes[c].suppressed = true;
                }
                synthetic_text(nodes, i, text.split_ascii_whitespace().collect::<Vec<_>>().join(" "), false);
                warnings.push("HTML select: выбранная подпись и стрелка импортированы с ролью Motif Option; список вариантов, multiple/size и браузерное раскрытие не переносятся".into());
            }
            "ul" | "ol" => {
                let ordered = nodes[i].tag == "ol";
                let reversed = nodes[i].attrs.contains_key("reversed");
                let items: Vec<_> = nodes[i].children.iter().copied().filter(|c| nodes[*c].tag == "li").collect();
                let mut number = nodes[i].attrs.get("start").and_then(|v| v.parse::<i64>().ok()).unwrap_or(if reversed { items.len() as i64 } else { 1 });
                if nodes[i].attrs.contains_key("type") {
                    warnings.push("HTML ol/ul type: использованы десятичные номера или круглые маркеры".into());
                }
                for c in items {
                    if ordered {
                        number = nodes[c].attrs.get("value").and_then(|v| v.parse::<i64>().ok()).unwrap_or(number);
                    }
                    let marker = synthetic_text(nodes, c, if ordered { format!("{number}. ") } else { "• ".into() }, false);
                    nodes[c].children.pop();
                    nodes[c].children.insert(0, marker);
                    number = number.saturating_add(if reversed { -1 } else { 1 });
                }
            }
            "form" => warnings.push("HTML form: сохранена группа; отправка данных и обработчики не создаются".into()),
            "label" if nodes[i].attrs.contains_key("for") => {
                warnings.push("HTML label for: подпись сохранена, связь с контролом по ID задайте в игровом интерфейсе".into())
            }
            _ => {}
        }
    }
    if nodes.len() > 2048 {
        return Err("Не больше 2048 узлов в HTML-фрагменте, включая подписи контролов и маркеры списков".into());
    }
    Ok(())
}

fn expand_tabs(text: &str, tab_size: usize) -> String {
    let mut out = String::new();
    let mut column = 0;
    for ch in text.chars() {
        match ch {
            '\t' => {
                let n = tab_size - column % tab_size;
                out.extend(std::iter::repeat_n(' ', n));
                column += n;
            }
            '\n' => {
                out.push(ch);
                column = 0;
            }
            _ => {
                out.push(ch);
                column += 1;
            }
        }
    }
    out
}

#[derive(Default)]
struct Space {
    last: Option<usize>,
    started: bool,
    blank: bool,
}

fn boundary(nodes: &mut [Element], space: &mut Space) {
    if let Some(last) = space.last {
        nodes[last].text = nodes[last].text.trim_end_matches(' ').to_owned();
    }
    *space = Space::default();
}

fn normalize(i: usize, nodes: &mut [Element], styles: &[Style], space: &mut Space) {
    if styles[i].hidden {
        return;
    }
    if nodes[i].tag == "br" {
        boundary(nodes, space);
        return;
    }
    if nodes[i].tag == "#text" {
        if styles[i].preserve_space {
            nodes[i].text = expand_tabs(&nodes[i].text, styles[i].tab_size);
            space.started = !nodes[i].text.is_empty();
            space.blank = nodes[i].text.ends_with(' ');
            space.last = Some(i);
            return;
        }
        let mut text = String::new();
        for c in nodes[i].text.chars() {
            if c.is_ascii_whitespace() {
                if space.started && !space.blank {
                    text.push(' ');
                    space.blank = true;
                }
            } else {
                text.push(c);
                space.started = true;
                space.blank = false;
            }
        }
        nodes[i].text = text;
        if !nodes[i].text.is_empty() {
            space.last = Some(i);
        }
        return;
    }
    let separate = !styles[i].inline || styles[i].flex;
    if separate {
        boundary(nodes, space);
    }
    for child in nodes[i].children.clone() {
        if styles[i].flex {
            // CSS flex does not create anonymous items for indentation-only text.
            if nodes[child].tag == "#text" && nodes[child].text.chars().all(|c| c.is_ascii_whitespace()) {
                nodes[child].text.clear();
                continue;
            }
            boundary(nodes, space);
        }
        normalize(child, nodes, styles, space);
        if styles[i].flex {
            if styles[child].preserve_space {
                *space = Space::default();
            } else {
                boundary(nodes, space);
            }
        }
    }
    if separate {
        // Preserved trailing spaces are part of the drawing, unlike ordinary flow whitespace.
        if styles[i].preserve_space {
            *space = Space::default();
        } else {
            boundary(nodes, space);
        }
    }
}

pub(crate) fn normalize_text(nodes: &mut [Element], styles: &[Style]) {
    normalize(0, nodes, styles, &mut Space::default());
}

pub(crate) fn warn_inline_limits(nodes: &[Element], styles: &[Style], warnings: &mut Vec<String>) {
    fn multiline(i: usize, nodes: &[Element], styles: &[Style]) -> bool {
        !styles[i].hidden && (nodes[i].tag == "br" || nodes[i].text.contains('\n') || nodes[i].children.iter().any(|c| multiline(*c, nodes, styles)))
    }
    fn caption_parts(i: usize, nodes: &[Element], styles: &[Style]) -> usize {
        nodes[i]
            .children
            .iter()
            .filter(|c| !styles[**c].hidden)
            .map(|c| {
                if nodes[*c].tag == "#text" {
                    usize::from(!nodes[*c].text.trim().is_empty())
                } else if !role(&nodes[*c]).is_empty() {
                    0
                } else {
                    caption_parts(*c, nodes, styles)
                }
            })
            .sum()
    }
    for (i, n) in nodes.iter().enumerate().skip(1) {
        if styles[i].hidden {
            continue;
        }
        if ["button", "label"].contains(&n.tag.as_str()) && caption_parts(i, nodes, styles) > 1 {
            warnings.push(format!("HTML <{}>: составная подпись сохранена отдельными векторными текстами; штатный экспорт screen/kit использует первый текстовый part, полную подпись и оформление настройте в игровом UI", n.tag));
        }
        if styles[i].inline
            && !matches!(n.tag.as_str(), "#text" | "br")
            && multiline(i, nodes, styles)
            && n.parent.is_some_and(|p| nodes[p].children.iter().filter(|c| !styles[**c].hidden).count() > 1)
        {
            warnings.push(format!(
                "HTML <{}>: многострочный вложенный inline размещён отдельным боксом; продолжение строки после него может потребовать ручного выравнивания",
                n.tag
            ));
        }
        if styles[i].preserve_space && n.children.iter().any(|c| nodes[*c].tag != "#text") {
            warnings.push(
                "HTML pre/white-space: вложенные элементы сохраняются отдельными боксами; табуляция считается отдельно в каждом текстовом фрагменте".into(),
            );
        }
    }
}

pub(crate) fn range_value(n: &Element) -> f64 {
    let finite = |key: &str, default: f64| n.attrs.get(key).and_then(|v| v.parse::<f64>().ok()).filter(|v| v.is_finite()).unwrap_or(default);
    let min = finite("min", 0.0);
    let max = finite("max", 100.0).max(min);
    if max == min { 0.0 } else { ((finite("value", min * 0.5 + max * 0.5) * 0.5 - min * 0.5) / (max * 0.5 - min * 0.5)).clamp(0.0, 1.0) }
}

pub(crate) fn emit_parts(n: &Element, s: &Style, b: &BoxNode, id: &str, ind: &str, out: &mut String) {
    match role(n) {
        "check" => {
            let checked = n.attrs.contains_key("checked");
            out.push_str(&format!(
                "{ind}rect {id}_unchecked size {} {} fill none part unchecked{}\n",
                num(b.w),
                num(b.h),
                if checked { " hidden" } else { "" }
            ));
            out.push_str(&format!("{ind}group {id}_checked part checked{} {{\n", if checked { "" } else { " hidden" }));
            out.push_str(&format!("{ind}  rect {id}_checked_bounds size {} {} fill none\n", num(b.w), num(b.h)));
            if input_type(n) == "radio" {
                out.push_str(&format!("{ind}  ellipse {id}_mark size {} {} fill {}\n", num(b.w * 0.5), num(b.h * 0.5), s.color));
            } else {
                out.push_str(&format!(
                    "{ind}  path {id}_mark d \"M {} 0 L 0 {} L {} {}\" fill none stroke {} {} cap round join round\n",
                    num(-b.w * 0.28),
                    num(b.h * 0.25),
                    num(b.w * 0.3),
                    num(-b.h * 0.28),
                    s.color,
                    num((b.w.min(b.h) / 8.0).max(1.0))
                ));
            }
            out.push_str(&format!("{ind}}}\n"));
        }
        "slider" => {
            let value = range_value(n);
            let knob = b.h.min(18.0).min(b.w);
            let travel = (b.w - knob).max(0.0);
            let x = -travel / 2.0 + travel * value;
            let filled = b.w / 2.0 + x;
            let track_color = if s.border_color == "none" { "#7b8496" } else { &s.border_color };
            out.push_str(&format!("{ind}rect {id}_track size {} 4 r 2 fill {track_color} part track\n", num(b.w)));
            out.push_str(&format!("{ind}rect {id}_fill at {} 0 size {} 4 r 2 fill {} part fill\n", num(-b.w / 2.0 + filled / 2.0), num(filled), s.color));
            out.push_str(&format!(
                "{ind}ellipse {id}_grabber at {} 0 size {} {} fill #ffffff stroke {} 1 part grabber\n",
                num(x),
                num(knob),
                num(knob),
                s.color
            ));
        }
        "option" => {
            let x = b.w / 2.0 - (s.padding[1] - crate::browser::SELECT_ARROW_SPACE).max(0.0) - s.border_width - 5.0;
            out.push_str(&format!(
                "{ind}path {id}_arrow at {} 0 d \"M -4 -2 L 0 2 L 4 -2\" fill none stroke {} 1.5 cap round join round part arrow\n",
                num(x),
                s.color
            ));
        }
        _ => {}
    }
}
