use crate::Element;
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum State {
    Base,
    Hover,
    Press,
    Disabled,
    Focus,
}
impl State {
    pub const ANIMATED: [Self; 4] = [Self::Hover, Self::Press, Self::Disabled, Self::Focus];
    pub fn clip(self) -> &'static str {
        match self {
            Self::Base => "Base",
            Self::Hover => "Hover",
            Self::Press => "Press",
            Self::Disabled => "Disabled",
            Self::Focus => "Focus",
        }
    }
    pub fn pseudo(self) -> &'static str {
        match self {
            Self::Base => "base",
            Self::Hover => "hover",
            Self::Press => "active",
            Self::Disabled => "disabled",
            Self::Focus => "focus",
        }
    }
}
#[derive(Clone)]
struct Simple {
    tag: Option<String>,
    id: Option<String>,
    classes: Vec<String>,
    states: Vec<State>,
    disabled: bool,
    root: bool,
}
#[derive(Clone)]
struct Selector {
    parts: Vec<(Simple, bool)>,
    specificity: (u32, u32, u32),
}
pub(crate) struct Rule {
    selector: Selector,
    decls: Vec<(String, String, bool)>,
}
impl Rule {
    pub fn stateful_for(&self, state: State) -> bool {
        self.selector.parts.iter().any(|(s, _)| s.states.contains(&state))
    }
}

pub(crate) fn parse(css: &str, warnings: &mut Vec<String>) -> Result<Vec<Rule>, String> {
    let mut clean = String::new();
    let mut rest = css;
    while let Some(at) = rest.find("/*") {
        clean.push_str(&rest[..at]);
        let Some(end) = rest[at + 2..].find("*/") else { return Err("CSS: незакрытый комментарий".into()) };
        rest = &rest[at + 2 + end + 2..];
    }
    clean.push_str(rest);
    let mut rules = Vec::new();
    let mut rest = clean.as_str();
    while !rest.trim().is_empty() {
        // Statement at-rules (commonly font @import) end at ';', not at a later selector block.
        let statement = split(rest, ';')[0];
        if rest.trim_start().starts_with('@') && statement.len() < rest.len() && rest.find('{').is_none_or(|open| statement.len() < open) {
            warnings.push(format!("CSS at-rule {} пропущен", statement.trim()));
            rest = &rest[statement.len() + 1..];
            continue;
        }
        let Some(open) = rest.find('{') else {
            warnings.push(format!("CSS вне блока пропущен: {}", rest.trim()));
            break;
        };
        let head = rest[..open].trim();
        let mut end = None;
        let mut depth = 1;
        let mut quote = None;
        let mut escape = false;
        for (at, c) in rest[open + 1..].char_indices() {
            if escape {
                escape = false;
                continue;
            }
            if c == '\\' {
                escape = true;
                continue;
            }
            if let Some(q) = quote {
                if c == q {
                    quote = None;
                }
                continue;
            }
            if c == '\'' || c == '"' {
                quote = Some(c);
                continue;
            }
            if c == '{' {
                depth += 1;
            }
            if c == '}' {
                depth -= 1;
                if depth == 0 {
                    end = Some(open + 1 + at);
                    break;
                }
            }
        }
        let Some(end) = end else { return Err("CSS: незакрытый блок".into()) };
        let body = &rest[open + 1..end];
        if head.starts_with('@') {
            warnings.push(format!("CSS at-rule {head} пропущен"));
        } else {
            let decls = declarations(body, warnings);
            for text in split(head, ',') {
                match selector(text.trim()) {
                    Some(selector) => rules.push(Rule { selector, decls: decls.clone() }),
                    None => warnings.push(format!("CSS-селектор не поддерживается: {}", text.trim())),
                }
            }
        }
        if rules.len() > 2048 {
            return Err("Не больше 2048 CSS-правил".into());
        }
        rest = &rest[end + 1..];
    }
    Ok(rules)
}

pub(crate) fn split(text: &str, separator: char) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut start = 0;
    let mut quote = None;
    let mut depth = 0;
    let mut escape = false;
    for (i, c) in text.char_indices() {
        if escape {
            escape = false;
            continue;
        }
        if c == '\\' {
            escape = true;
            continue;
        }
        if let Some(q) = quote {
            if c == q {
                quote = None;
            }
            continue;
        }
        if c == '\'' || c == '"' {
            quote = Some(c);
            continue;
        }
        if c == '(' {
            depth += 1;
        }
        if c == ')' {
            depth -= 1;
        }
        if (c == separator || separator == ' ' && c.is_whitespace()) && depth == 0 {
            parts.push(&text[start..i]);
            start = i + c.len_utf8();
        }
    }
    parts.push(&text[start..]);
    parts
}
pub(crate) fn words(text: &str) -> Vec<&str> {
    split(text, ' ').into_iter().filter(|s| !s.is_empty()).collect()
}

fn declarations(body: &str, warnings: &mut Vec<String>) -> Vec<(String, String, bool)> {
    let mut out = Vec::new();
    for decl in split(body, ';') {
        let decl = decl.trim();
        if decl.is_empty() {
            continue;
        }
        let Some((key, value)) = decl.split_once(':') else {
            warnings.push(format!("CSS: пропущена декларация {decl}"));
            continue;
        };
        let key = key.trim().to_lowercase();
        let value = value.trim();
        let important = value.ends_with("!important");
        let value = value.trim_end_matches("!important").trim();
        if ["padding", "margin"].contains(&key.as_str()) {
            let vals = words(value);
            if !(1..=4).contains(&vals.len()) {
                warnings.push(format!("CSS {key}: нужны 1–4 значения"));
                continue;
            }
            let four = match vals.as_slice() {
                [a] => [*a, *a, *a, *a],
                [a, b] => [*a, *b, *a, *b],
                [a, b, c] => [*a, *b, *c, *b],
                [a, b, c, d] => [*a, *b, *c, *d],
                _ => unreachable!(),
            };
            for (side, v) in ["top", "right", "bottom", "left"].iter().zip(four) {
                out.push((format!("{key}-{side}"), v.into(), important));
            }
        } else if key == "border" {
            let mut width = "1px";
            let mut color = "currentColor";
            let mut none = false;
            for word in words(value) {
                if ["none", "hidden"].contains(&word) {
                    none = true;
                } else if ["solid"].contains(&word) {
                } else if word.ends_with("px") || word == "0" {
                    width = word;
                } else if ["dashed", "dotted", "double", "groove", "ridge", "inset", "outset"].contains(&word) {
                    warnings.push(format!("CSS border {word}: импортирована сплошная обводка"));
                } else {
                    color = word;
                }
            }
            out.push(("border-width".into(), if none { "0".into() } else { width.into() }, important));
            out.push(("border-color".into(), if none { "transparent".into() } else { color.into() }, important));
        } else if key == "background" {
            // A background shorthand resets both components, even when it only names a color.
            out.push(("background-image".into(), "none".into(), important));
            out.push(("background-color".into(), "transparent".into(), important));
            out.push((if value.starts_with("linear-gradient(") { "background-image" } else { "background-color" }.into(), value.into(), important));
        } else {
            out.push((key, value.into(), important));
        }
    }
    out
}

fn selector(text: &str) -> Option<Selector> {
    if text.is_empty() || text.contains(['+', '~', '(', ')', '|']) {
        return None;
    }
    let spaced = text.replace('>', " > ");
    let mut parts = Vec::new();
    let mut direct = false;
    let mut specificity = (0, 0, 0);
    for word in spaced.split_whitespace() {
        if word == ">" {
            if parts.is_empty() || direct {
                return None;
            }
            direct = true;
            continue;
        }
        let mut s = Simple { tag: None, id: None, classes: vec![], states: vec![], disabled: false, root: false };
        let chars: Vec<char> = word.chars().collect();
        let mut i = 0;
        while i < chars.len() {
            let prefix = chars[i];
            if prefix == '*' {
                i += 1;
                continue;
            }
            if prefix == '[' {
                let tail: String = chars[i..].iter().collect();
                if tail != "[disabled]" {
                    return None;
                }
                s.disabled = true;
                specificity.1 += 1;
                break;
            }
            let kind = if ['#', '.', ':'].contains(&prefix) {
                i += 1;
                prefix
            } else {
                't'
            };
            let start = i;
            while i < chars.len() && (chars[i].is_alphanumeric() || ['_', '-'].contains(&chars[i])) {
                i += 1;
            }
            if start == i {
                return None;
            }
            let value: String = chars[start..i].iter().collect();
            match kind {
                '#' => {
                    s.id = Some(value);
                    specificity.0 += 1;
                }
                '.' => {
                    s.classes.push(value);
                    specificity.1 += 1;
                }
                ':' => {
                    if value == "root" {
                        s.root = true;
                        specificity.1 += 1;
                        continue;
                    }
                    s.states.push(match value.as_str() {
                        "hover" => State::Hover,
                        "active" => State::Press,
                        "disabled" => State::Disabled,
                        "focus" | "focus-visible" => State::Focus,
                        _ => return None,
                    });
                    specificity.1 += 1;
                }
                _ => {
                    if s.tag.is_some() {
                        return None;
                    }
                    s.tag = Some(value.to_lowercase());
                    specificity.2 += 1;
                }
            }
        }
        parts.push((s, direct));
        direct = false;
    }
    (!direct).then_some(Selector { parts, specificity })
}
fn simple_matches(s: &Simple, e: &Element, state: State) -> bool {
    if s.root && e.parent.is_some() {
        return false;
    }
    if s.tag.as_ref().is_some_and(|t| t != &e.tag) || s.id.as_ref().is_some_and(|id| e.attrs.get("id") != Some(id)) {
        return false;
    }
    if s.disabled && !e.attrs.contains_key("disabled") {
        return false;
    }
    let classes = e.attrs.get("class").map_or("", String::as_str);
    if !s.classes.iter().all(|c| classes.split_whitespace().any(|w| w == c)) {
        return false;
    }
    s.states.iter().all(|p| match p {
        State::Hover => matches!(state, State::Hover | State::Press) && !e.attrs.contains_key("disabled"),
        State::Disabled => state == State::Disabled || e.attrs.contains_key("disabled"),
        State::Press | State::Focus if e.attrs.contains_key("disabled") => false,
        _ => *p == state,
    })
}
fn matches(s: &Selector, nodes: &[Element], index: usize, state: State) -> bool {
    let mut at = index;
    for (pos, (part, _)) in s.parts.iter().enumerate().rev() {
        if !simple_matches(part, &nodes[at], state) {
            return false;
        }
        if pos > 0 {
            let (wanted, _) = &s.parts[pos - 1];
            let direct = s.parts[pos].1;
            let mut parent = nodes[at].parent;
            let mut found = None;
            while let Some(p) = parent {
                if simple_matches(wanted, &nodes[p], state) {
                    found = Some(p);
                    break;
                }
                if direct {
                    break;
                }
                parent = nodes[p].parent;
            }
            let Some(parent) = found else {
                return false;
            };
            at = parent;
        }
    }
    true
}
pub(crate) fn cascade(rules: &[Rule], nodes: &[Element], index: usize, state: State, warnings: &mut Vec<String>) -> BTreeMap<String, String> {
    // Character data inherits text styling; selectors apply to elements, never text nodes.
    if nodes[index].tag == "#text" {
        return BTreeMap::new();
    }
    type Rank = (bool, bool, (u32, u32, u32), usize, usize);
    let mut values: BTreeMap<String, (Rank, String)> = BTreeMap::new();
    let mut add = |decls: &[(String, String, bool)], inline: bool, specificity, order| {
        for (d, (key, value, important)) in decls.iter().enumerate() {
            let rank = (*important, inline, specificity, order, d);
            if values.get(key).is_none_or(|(old, _)| rank >= *old) {
                values.insert(key.clone(), (rank, value.clone()));
            }
        }
    };
    for (order, rule) in rules.iter().enumerate() {
        if matches(&rule.selector, nodes, index, state) {
            add(&rule.decls, false, rule.selector.specificity, order);
        }
    }
    if let Some(inline) = nodes[index].attrs.get("style") {
        add(&declarations(inline, warnings), true, (0, 0, 0), rules.len());
    }
    values.into_iter().map(|(k, (_, v))| (k, v)).collect()
}
