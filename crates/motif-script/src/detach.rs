//! «Отцепить копию»: a copy of a symbol becomes an ordinary group that holds what the copy
//! showed. The objects keep their ids (`coin1-body`), so the animation and the edits of the copy
//! stay; the symbol no longer changes the group.

use crate::edit::{fmt_value, splice, whole_lines, with_space_before};
use crate::parser::{NodeAst, Stmt, UseAst, ValueAst, ValueKind};
use crate::{Scene, Span, fmt_num};
use motif_core::Value;

/// The values the copy gives the parameters of its symbol, and the names that mean something
/// else at the place being read (a loop variable or a `let` of the same name).
struct Subst<'a> {
    src: &'a str,
    copy: &'a str,
    params: &'a [(String, Value)],
    shadowed: Vec<String>,
    /// The objects the copy's variant hides, and whether each was found.
    hide: Vec<(String, bool)>,
    /// The objects it shows though the symbol has them `hidden`, the same way.
    show: Vec<(String, bool)>,
    edits: Vec<(Span, String)>,
}

impl Subst<'_> {
    fn param(&self, name: &str) -> Option<Value> {
        if self.shadowed.iter().any(|s| s == name) {
            return None;
        }
        self.params.iter().find(|(p, _)| p == name).map(|(_, v)| *v)
    }

    /// An expression with the parameters replaced by their numbers.
    fn expr(&self, e: &str) -> String {
        let mut out = String::new();
        let mut chars = e.char_indices().peekable();
        while let Some((i, c)) = chars.next() {
            if c.is_alphabetic() || c == '_' {
                let mut end = i + c.len_utf8();
                while let Some(&(j, d)) = chars.peek() {
                    if d.is_alphanumeric() || d == '_' {
                        end = j + d.len_utf8();
                        chars.next();
                    } else {
                        break;
                    }
                }
                match self.param(&e[i..end]) {
                    Some(Value::Num(n)) if n < 0.0 => out.push_str(&format!("({})", fmt_num(n))),
                    Some(Value::Num(n)) => out.push_str(&fmt_num(n)),
                    _ => out.push_str(&e[i..end]),
                }
            } else {
                out.push(c);
            }
        }
        out
    }

    /// `{size * 0.72}` as the copy reads it: the number when it is written exactly, or the
    /// expression with the numbers of the parameters in it (so nothing moves by a rounding, and
    /// a loop variable stays a variable).
    fn braces(&self, e: &str) -> String {
        let with = self.expr(e);
        match crate::expr::eval(&with, &|_| None) {
            Ok(n) if fmt_num(n).parse() == Ok(n) => fmt_num(n),
            _ => format!("{{{}}}", with.trim()),
        }
    }

    fn value(&mut self, v: &ValueAst) {
        match &v.kind {
            ValueKind::Ref(name) => {
                if let Some(value) = self.param(name) {
                    self.edits.push((v.span, fmt_value(value)));
                }
            }
            ValueKind::Expr(e) => {
                let new = self.braces(e);
                if self.src[v.span.start..v.span.end] != new {
                    self.edits.push((v.span, new));
                }
            }
            _ => {}
        }
    }

    /// The line of an object: its id gets the id of the copy before it, the ids it names too,
    /// and the parameters in its values become what the copy gives them.
    fn node(&mut self, n: &NodeAst) {
        // `dot{i}`: what stands in braces is an expression too.
        let mut id = String::new();
        let mut rest = n.id.as_str();
        while let Some(open) = rest.find('{') {
            let Some(close) = rest[open..].find('}').map(|c| c + open) else { break };
            id.push_str(&rest[..open]);
            id.push_str(&self.braces(&rest[open + 1..close]));
            rest = &rest[close + 1..];
        }
        id.push_str(rest);
        self.edits.push((n.id_span, format!("{}-{id}", self.copy)));
        for p in &n.props {
            let names_objects = matches!(p.key.as_str(), "ik" | "skin" | "on" | "tie" | "hang" | "links" | "morph");
            for v in &p.values {
                match &v.kind {
                    ValueKind::Path(other) if names_objects && self.src[v.span.start..v.span.end] == *other => {
                        self.edits.push((v.span, format!("{}-{other}", self.copy)));
                    }
                    _ => self.value(v),
                }
            }
        }
        if let Some(found) = self.show.iter_mut().find(|(part, _)| *part == n.id) {
            // What the variant shows is an object like any other in the group.
            found.1 = true;
            for p in n.props.iter().filter(|p| p.key == "hidden") {
                self.edits.push((with_space_before(self.src, p.key_span), String::new()));
            }
        } else if let Some(found) = self.hide.iter_mut().find(|(part, _)| *part == n.id) {
            found.1 = true;
            if !n.props.iter().any(|p| p.key == "hidden") {
                self.edits.push((Span::new(n.insert_at, n.insert_at), " hidden".to_string()));
            }
        }
    }

    fn stmts(&mut self, stmts: &[Stmt]) {
        for s in stmts {
            match s {
                Stmt::Node(n) => {
                    self.node(n);
                    self.stmts(&n.children);
                }
                Stmt::Use(u) => {
                    self.node(&u.node);
                    for p in &u.params {
                        self.value(&p.value);
                    }
                }
                Stmt::For { var, from, to, body } => {
                    self.value(from);
                    self.value(to);
                    let before = self.shadowed.len();
                    self.shadowed.push(var.clone());
                    self.stmts(body);
                    self.shadowed.truncate(before);
                }
                Stmt::Let { name, value, .. } => {
                    self.value(value);
                    self.shadowed.push(name.clone());
                }
                // The variants stay with the symbol: the group is what its variant made it.
                Stmt::Variant { span, .. } => self.edits.push((whole_lines(self.src, *span), String::new())),
                _ => {}
            }
        }
    }
}

fn find_use(stmts: &[Stmt], id_span: Span) -> Option<&UseAst> {
    stmts.iter().find_map(|s| match s {
        Stmt::Use(u) if u.node.id_span == id_span => Some(u),
        Stmt::Node(n) => find_use(&n.children, id_span),
        Stmt::For { body, .. } => find_use(body, id_span),
        _ => None,
    })
}

/// `src[from..to]` with `edits` made in it (each inside the range, none overlapping).
fn edited(src: &str, from: usize, to: usize, mut edits: Vec<(Span, String)>) -> String {
    edits.retain(|(s, _)| s.start >= from && s.end <= to);
    // An insertion at the end of a replaced value comes after the value.
    edits.sort_by_key(|(s, _)| (s.start, s.end));
    let mut out = String::new();
    let mut at = from;
    for (span, text) in edits {
        if span.start < at {
            continue;
        }
        out.push_str(&src[at..span.start]);
        out.push_str(&text);
        at = span.end;
    }
    out.push_str(&src[at..to]);
    out
}

/// Returns the script with copy `copy` turned into an ordinary group: the lines of its symbol are
/// written inside it with the values this copy gives the parameters, the objects its variant hides
/// are `hidden`, and every object keeps its id (`copy-object`), so animation tracks and the edits
/// of the copy in `override { }` stay as they are. The picture does not change; the symbol and its
/// other copies are not touched. Fails with the reason when `copy` is not a copy written by hand
/// (a copy made by a loop or placed inside another symbol shares its line), or the group cannot
/// be written.
pub fn detach_copy(src: &str, scene: &Scene, copy: &str, base: Option<&std::path::Path>) -> Result<String, String> {
    let inst = scene.instances.get(copy).ok_or_else(|| format!("«{copy}» не копия символа"))?;
    let site = scene.sites.get(copy).filter(|s| !s.generated).ok_or_else(|| {
        format!("Копия «{copy}» создана циклом или стоит внутри другого символа: её строка общая для нескольких копий. Отцепите её в скрипте")
    })?;
    if scene.editing.is_some() {
        return Err("Сначала выйдите из правки символа (Esc)".into());
    }
    // The lines of a symbol of another file are not in this script.
    if let Some(site) = scene.symbols.iter().find(|s| s.name == inst.symbol).and_then(|s| scene.included(s.span)) {
        return Err(format!(
            "Символ «{}» записан в подключённом файле «{}»: его копию отцепить нельзя, строк символа в этой сцене нет",
            inst.symbol, site.path
        ));
    }
    let unreadable = || "не удалось прочитать скрипт".to_string();
    let toks = crate::lexer::lex(src).map_err(|_| unreadable())?;
    let stmts = crate::parser::Parser::new(src, toks).block(false);
    let (clip, body, sym_span) = stmts
        .iter()
        .find_map(|s| match s {
            Stmt::Symbol { name, clip, body, span, .. } if *name == inst.symbol => Some((*clip, body, *span)),
            _ => None,
        })
        .ok_or_else(unreadable)?;
    let u = find_use(&stmts, site.id_span).ok_or_else(unreadable)?;

    // The lines between the braces of the symbol.
    let open = src[sym_span.start..sym_span.end].find('\n').map(|n| sym_span.start + n + 1).ok_or_else(unreadable)?;
    let close = sym_span.end.saturating_sub(1);
    let end = src[..close].rfind('\n').map_or(open, |n| n + 1).max(open);

    let params: Vec<(String, Value)> = inst.params.iter().map(|p| (p.name.clone(), p.value)).collect();
    let taken =
        inst.variant.as_ref().and_then(|(name, _)| scene.symbols.iter().filter(|s| s.name == inst.symbol).flat_map(|s| &s.variants).find(|v| v.name == *name));
    let listed = |ids: &[(String, Span)]| -> Vec<(String, bool)> { ids.iter().map(|(id, _)| (id.clone(), false)).collect() };
    let (hide, show) = taken.map_or((Vec::new(), Vec::new()), |v| (listed(&v.hide), listed(&v.show)));
    let mut subst = Subst { src, copy, params: &params, shadowed: Vec::new(), hide, show, edits: Vec::new() };
    subst.stmts(body);
    if let Some((part, _)) = subst.hide.iter().chain(&subst.show).find(|(_, found)| !found) {
        return Err(format!(
            "Вариант скрывает или показывает «{part}», а такой строки в символе нет (объект создан циклом или стоит во вложенной копии). Отцепите копию в скрипте"
        ));
    }
    let inner = edited(src, open, end, subst.edits);

    // The lines of the symbol, indented as the lines of a group standing where the copy stands.
    let line_start = src[..site.span.start].rfind('\n').map_or(0, |n| n + 1);
    let indent: String = src[line_start..site.span.start].chars().take_while(|c| *c == ' ' || *c == '\t').collect();
    let width = |l: &str| l.len() - l.trim_start_matches([' ', '\t']).len();
    let least = inner.lines().filter(|l| !l.trim().is_empty()).map(width).min().unwrap_or(0);
    let mut lines = String::new();
    for l in inner.lines() {
        if l.trim().is_empty() {
            lines.push('\n');
        } else {
            lines.push_str(&format!("{indent}  {}\n", &l[least..]));
        }
    }

    // `use Coin c1 "Монета" at 40 60 size=30 variant big` → `group c1 "Монета" at 40 60 {`.
    let mut gone: Vec<(Span, String)> =
        u.params.iter().map(|p| (with_space_before(src, Span::new(p.name_span.start, p.value.span.end)), String::new())).collect();
    gone.extend(u.variant.iter().map(|v| (with_space_before(src, v.span), String::new())));
    let head = edited(src, u.node.id_span.start, u.node.span.end, gone);
    let clip = if clip && !u.node.props.iter().any(|p| p.key == "clip") { " clip" } else { "" };
    let out = splice(src, u.node.span, &format!("group {}{clip} {{\n{lines}{indent}}}", head.trim_end()));
    match crate::parse_in(&out, base) {
        Ok(_) => Ok(out),
        Err(errors) => Err(format!("Копию не удалось записать группой: {}", errors.first().map(ToString::to_string).unwrap_or_default())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse;

    const SCENE: &str = "\
artboard \"Набор\" 320x200 bg #ffffff
let accent = #ff5a36
symbol Coin size=24 color=#ffd23f {
  variant big size=40 hide shine
  ellipse body size {size} {size} fill color stroke #1b1d24 2
  rect shine at {-size / 6} -4 size 6 6 fill #ffffff
  for i in 0..2 {
    ellipse dot{i} at {i * size} 20 size 4 4 fill color
  }
}
symbol Ball clip r=20 {
  ellipse mask size {r * 2}
  rect band size {r * 3} 8 fill accent
}
use Coin c1 \"Монета\" at 40 60 rot 15
use Coin c2 at 120 60 variant big color=accent
group shelf at 200 100 {
  use Ball b1 at 0 0 r=30
}
rect floor at 160 190 size 320 10 fill #888888
anim Spin 1s loop {
  c1-shine.x 0: -4 | 1: 4
  c2-body.rot 0: 0 | 1: 90
}
override {
  c1-shine.fill = #ff0000
}
";

    #[test]
    fn a_detached_copy_is_a_group_that_looks_the_same() {
        let scene = parse(SCENE).unwrap();
        let out = detach_copy(SCENE, &scene, "c1", None).unwrap();
        assert!(
            out.contains(
                "group c1 \"Монета\" at 40 60 rot 15 {\n  ellipse c1-body size 24 24 fill #ffd23f stroke #1b1d24 2\n  rect c1-shine at -4 -4 size 6 6 fill #ffffff\n  for i in 0..2 {\n    ellipse c1-dot{i} at {i * 24} 20 size 4 4 fill #ffd23f\n  }\n}\nuse Coin c2"
            ),
            "{out}"
        );
        let after = parse(&out).unwrap();
        assert_eq!(after.doc, scene.doc, "the picture, the animation and the edit of the copy are the same");
        assert!(!after.instances.contains_key("c1") && after.instances.contains_key("c2"));
        // The lines of the group are its own now.
        assert!(!after.sites["c1-body"].generated);
        assert!(out.contains("  c1-shine.x 0: -4 | 1: 4\n") && out.contains("  c1-shine.fill = #ff0000\n"), "{out}");
        // The symbol is as it was.
        assert!(out.contains("symbol Coin size=24 color=#ffd23f {\n  variant big size=40 hide shine\n  ellipse body size {size} {size}"), "{out}");
    }

    #[test]
    fn the_values_and_the_hidden_objects_of_the_variant_stay_with_the_group() {
        let scene = parse(SCENE).unwrap();
        let out = detach_copy(SCENE, &scene, "c2", None).unwrap();
        assert!(
            out.contains(
                "group c2 at 120 60 {\n  ellipse c2-body size 40 40 fill #ff5a36 stroke #1b1d24 2\n  rect c2-shine at {-40 / 6} -4 size 6 6 fill #ffffff hidden\n  for i in 0..2 {\n    ellipse c2-dot{i} at {i * 40} 20 size 4 4 fill #ff5a36\n"
            ),
            "{out}"
        );
        let after = parse(&out).unwrap();
        assert!(!after.doc.node("c2-shine").unwrap().visible);
        assert_eq!(after.doc.node("c2-body").unwrap().width, 40.0);
        assert_eq!(after.doc, scene.doc, "nothing moves, not even by a rounding");
    }

    #[test]
    fn what_the_variant_shows_is_seen_in_the_group() {
        let src = "\
artboard \"Карточки\" 200x120
symbol Card {
  variant hit show ribbon
  rect sheet size 80 100 fill #ffffff
  rect ribbon at 0 -40 size 40 12 hidden fill #ff5a36
  rect lock size 20 20 fill #000000 hidden
}
use Card b at 100 60 variant hit
";
        let scene = parse(src).unwrap();
        let out = detach_copy(src, &scene, "b", None).unwrap();
        assert!(
            out.contains("group b at 100 60 {\n  rect b-sheet size 80 100 fill #ffffff\n  rect b-ribbon at 0 -40 size 40 12 fill #ff5a36\n  rect b-lock size 20 20 fill #000000 hidden\n}\n"),
            "{out}"
        );
        assert_eq!(parse(&out).unwrap().doc, scene.doc);
    }

    #[test]
    fn a_copy_inside_a_group_keeps_its_place_and_its_mask() {
        let scene = parse(SCENE).unwrap();
        let out = detach_copy(SCENE, &scene, "b1", None).unwrap();
        assert!(
            out.contains("group shelf at 200 100 {\n  group b1 at 0 0 clip {\n    ellipse b1-mask size 60\n    rect b1-band size 90 8 fill accent\n  }\n}\n"),
            "{out}"
        );
        assert_eq!(parse(&out).unwrap().doc, scene.doc);
    }

    #[test]
    fn a_copy_that_shares_its_line_is_not_detached() {
        let src = "artboard \"a\" 100x100\nsymbol Dot {\n  ellipse d size 4 4\n}\nfor i in 0..3 {\n  use Dot p{i} at {i * 10} 0\n}\nrect r size 4 4\n";
        let scene = parse(src).unwrap();
        assert!(detach_copy(src, &scene, "p1", None).unwrap_err().contains("создана циклом"));
        assert!(detach_copy(src, &scene, "r", None).unwrap_err().contains("не копия"));
    }
}
