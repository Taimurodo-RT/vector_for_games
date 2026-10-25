//! Edits of `machine` blocks. Every input, state and transition is one line, so an edit rewrites
//! the lines whose meaning changed and leaves the rest of the block as it was written.

use crate::edit::splice;
use crate::{Scene, Span, fmt_num};
use motif_core::{Cond, CondOp, Input, InputKind, Machine, State, Transition};

/// One line of a machine, by its place in the machine's lists.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MachineItem {
    Input(usize),
    State(usize),
    Transition(usize),
}

pub fn fmt_input(i: &Input) -> String {
    let value = match i.kind {
        InputKind::Number => fmt_num(i.default),
        InputKind::Bool if i.default != 0.0 => "true".into(),
        InputKind::Bool => "false".into(),
        InputKind::Trigger => "trigger".into(),
    };
    format!("input {} {value}", i.name)
}

pub fn fmt_state(s: &State) -> String {
    let mut out = format!("state {}", s.name);
    if s.anim != s.name {
        out += &format!(" anim {}", s.anim);
    }
    if let Some((x, y)) = s.pos {
        out += &format!(" at {} {}", fmt_num(x), fmt_num(y));
    }
    out
}

pub fn fmt_any(pos: (f64, f64)) -> String {
    format!("any at {} {}", fmt_num(pos.0), fmt_num(pos.1))
}

pub fn fmt_cond(c: &Cond) -> String {
    match c.op {
        CondOp::Is => c.input.clone(),
        CondOp::Not => format!("not {}", c.input),
        op => format!("{} {} {}", c.input, op.symbol(), fmt_num(c.value)),
    }
}

pub fn fmt_transition(m: &Machine, t: &Transition) -> String {
    let name = |i: usize| m.states.get(i).map_or("?", |s| s.name.as_str());
    let mut out = format!("{} -> {}", t.from.map_or("any", name), name(t.to));
    if !t.conds.is_empty() {
        out += " when ";
        out += &t.conds.iter().map(fmt_cond).collect::<Vec<_>>().join(" and ");
    }
    if t.at_end {
        out += " at end";
    }
    if t.blend > 0.0 {
        out += &format!(" blend {}s", fmt_num(t.blend));
    }
    out
}

/// The whole `machine` block, for a new machine.
pub fn fmt_machine(m: &Machine) -> String {
    let mut out = format!("machine {} {{\n", m.name);
    for i in &m.inputs {
        out += &format!("  {}\n", fmt_input(i));
    }
    if let Some(p) = m.any_pos {
        out += &format!("  {}\n", fmt_any(p));
    }
    for s in &m.states {
        out += &format!("  {}\n", fmt_state(s));
    }
    for t in &m.transitions {
        out += &format!("  {}\n", fmt_transition(m, t));
    }
    out + "}\n"
}

/// A name of the form `base`, `base2`, … for which `taken` is false.
pub fn free_name(base: &str, taken: impl Fn(&str) -> bool) -> String {
    if !taken(base) {
        return base.to_string();
    }
    (2..).map(|k| format!("{base}{k}")).find(|n| !taken(n)).expect("names are unbounded")
}

/// Returns the script with a new machine at its end, with a state for every animation, and its name.
pub fn add_machine(src: &str, scene: &Scene) -> (String, String) {
    let name = free_name("Machine", |n| scene.doc.machine(n).is_some());
    let states = scene
        .doc
        .animations
        .iter()
        .enumerate()
        .map(|(i, a)| State { name: a.name.clone(), anim: a.name.clone(), pos: Some((40.0 + 180.0 * i as f64, 80.0)) })
        .collect();
    let m = Machine { name: name.clone(), inputs: Vec::new(), states, transitions: Vec::new(), any_pos: None };
    let mut out = src.trim_end().to_string();
    out += if out.is_empty() { "" } else { "\n\n" };
    out += &fmt_machine(&m);
    (out, name)
}

/// The whole line `span` is on, with its line break.
fn full_line(src: &str, span: Span) -> Span {
    let start = src[..span.start].rfind('\n').map_or(0, |n| n + 1);
    let end = src[span.end..].find('\n').map_or(src.len(), |n| span.end + n + 1);
    Span::new(start, end)
}

pub(crate) fn apply(src: &str, mut edits: Vec<(Span, String)>) -> String {
    edits.sort_by(|a, b| b.0.start.cmp(&a.0.start).then(b.0.end.cmp(&a.0.end)));
    let mut out = src.to_string();
    for (span, text) in edits {
        out = splice(&out, span, &text);
    }
    out
}

/// Changes machine `name` with `f` and writes the result back: lines that mean something new
/// are rewritten, and inputs, states and transitions added to the ends of the lists get new lines.
/// `None` when there is no such machine or `f` removed something (see [`delete_machine_item`]).
pub fn edit_machine(src: &str, scene: &Scene, name: &str, f: impl FnOnce(&mut Machine)) -> Option<String> {
    let old = scene.doc.machine(name)?;
    let site = scene.machines.get(name)?;
    let mut new = old.clone();
    f(&mut new);
    if new.name != old.name || new.inputs.len() < old.inputs.len() || new.states.len() < old.states.len() || new.transitions.len() < old.transitions.len() {
        return None;
    }
    let mut edits = Vec::new();
    let mut added = String::new();
    match (site.any, new.any_pos) {
        (Some(span), Some(p)) if old.any_pos != Some(p) => edits.push((span, fmt_any(p))),
        (Some(span), None) => edits.push((full_line(src, span), String::new())),
        (None, Some(p)) => added += &format!("  {}\n", fmt_any(p)),
        _ => {}
    }
    for (i, b) in new.inputs.iter().enumerate() {
        match old.inputs.get(i) {
            Some(a) if fmt_input(a) == fmt_input(b) => {}
            Some(_) => edits.push((site.inputs[i], fmt_input(b))),
            None => added += &format!("  {}\n", fmt_input(b)),
        }
    }
    for (i, b) in new.states.iter().enumerate() {
        match old.states.get(i) {
            Some(a) if fmt_state(a) == fmt_state(b) => {}
            Some(_) => edits.push((site.states[i], fmt_state(b))),
            None => added += &format!("  {}\n", fmt_state(b)),
        }
    }
    for (i, b) in new.transitions.iter().enumerate() {
        let text = fmt_transition(&new, b);
        match old.transitions.get(i) {
            Some(a) if fmt_transition(old, a) == text => {}
            Some(_) => edits.push((site.transitions[i], text)),
            None => added += &format!("  {text}\n"),
        }
    }
    if !added.is_empty() {
        let at = src[..site.close].rfind('\n').map_or(site.close, |n| n + 1);
        if at == site.close && !src[..at].ends_with('\n') {
            added.insert(0, '\n');
        }
        edits.push((Span::new(at, at), added));
    }
    Some(apply(src, edits))
}

/// Removes one line of machine `name`. A state takes its transitions with it; an input takes the
/// conditions that read it.
pub fn delete_machine_item(src: &str, scene: &Scene, name: &str, item: MachineItem) -> Option<String> {
    let m = scene.doc.machine(name)?;
    let site = scene.machines.get(name)?;
    let mut edits = Vec::new();
    match item {
        MachineItem::Transition(i) => edits.push((full_line(src, *site.transitions.get(i)?), String::new())),
        MachineItem::State(i) => {
            edits.push((full_line(src, *site.states.get(i)?), String::new()));
            for (k, t) in m.transitions.iter().enumerate() {
                if t.from == Some(i) || t.to == i {
                    edits.push((full_line(src, site.transitions[k]), String::new()));
                }
            }
        }
        MachineItem::Input(i) => {
            let input = &m.inputs.get(i)?.name;
            edits.push((full_line(src, site.inputs[i]), String::new()));
            for (k, t) in m.transitions.iter().enumerate() {
                if t.conds.iter().any(|c| &c.input == input) {
                    let mut t = t.clone();
                    t.conds.retain(|c| &c.input != input);
                    edits.push((site.transitions[k], fmt_transition(m, &t)));
                }
            }
        }
    }
    Some(apply(src, edits))
}

/// Moves state `i` to the top of the states, which makes it the one the machine starts in.
pub fn make_entry(src: &str, scene: &Scene, name: &str, i: usize) -> Option<String> {
    let site = scene.machines.get(name)?;
    if i == 0 {
        return None;
    }
    let line = full_line(src, *site.states.get(i)?);
    let first = full_line(src, site.states[0]);
    let mut text = src[line.start..line.end].to_string();
    if !text.ends_with('\n') {
        text.push('\n');
    }
    Some(apply(src, vec![(line, String::new()), (Span::new(first.start, first.start), text)]))
}

/// Removes the whole `machine` block.
pub fn delete_machine(src: &str, scene: &Scene, name: &str) -> Option<String> {
    let site = scene.machines.get(name)?;
    let end = site.span.end + usize::from(src[site.span.end..].starts_with('\n'));
    let start = site.span.start - usize::from(src[..site.span.start].ends_with("\n\n"));
    Some(apply(src, vec![(Span::new(start, end), String::new())]))
}
