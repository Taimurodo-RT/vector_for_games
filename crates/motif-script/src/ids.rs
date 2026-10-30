//! Hidden ids written into the script (`input speed 0 id=k7f2`). Motif gives one, on save, to
//! every input and state of a machine, every animation, event name and pose parameter that has
//! none yet; edits keep them when the thing is renamed. A game binds to the id, so renaming
//! `speed` to `скорость` in Motif does not break it.

use crate::lexer::{Tok, Token, lex};
use motif_core::IdKind;
use std::collections::{HashMap, HashSet};

/// `src` with an id added to every line that should have one and has none. A script that does
/// not lex comes back as it is.
pub fn stamp(src: &str) -> String {
    let Ok(toks) = lex(src) else { return src.to_string() };
    let lines = lines(&toks);
    let mut taken: HashSet<String> = HashSet::new();
    let mut events: HashMap<String, String> = HashMap::new();
    for l in &lines {
        if let Some(id) = &l.id {
            taken.insert(id.clone());
            if l.kind == IdKind::Event {
                events.entry(l.name.clone()).or_insert_with(|| id.clone());
            }
        }
    }
    let mut inserts: Vec<(usize, String)> = Vec::new();
    for l in lines.iter().filter(|l| l.id.is_none()) {
        let id = match (l.kind, events.get(&l.name)) {
            (IdKind::Event, Some(id)) => id.clone(),
            _ => {
                let id = fresh(l.kind, &l.owner, &l.name, &taken);
                taken.insert(id.clone());
                if l.kind == IdKind::Event {
                    events.insert(l.name.clone(), id.clone());
                }
                id
            }
        };
        inserts.push((l.at, format!(" id={id}")));
    }
    let mut out = src.to_string();
    for (at, text) in inserts.into_iter().rev() {
        out.insert_str(at, &text);
    }
    out
}

/// A line that takes an id.
struct Line {
    kind: IdKind,
    /// The machine of an input or a state.
    owner: String,
    name: String,
    id: Option<String>,
    /// Where ` id=…` goes: after the last word, before the `{` of an animation.
    at: usize,
}

fn lines(toks: &[Token]) -> Vec<Line> {
    #[derive(PartialEq)]
    enum Block {
        Machine(String),
        Anim,
        Other,
    }
    let mut blocks: Vec<Block> = Vec::new();
    let mut out = Vec::new();
    for line in toks.split(|t| t.tok == Tok::Newline).filter(|l| !l.is_empty()) {
        let word = |k: usize| match line.get(k).map(|t| &t.tok) {
            Some(Tok::Ident(w)) => Some(w.as_str()),
            _ => None,
        };
        let opens = line.last().is_some_and(|t| t.tok == Tok::BlockOpen);
        let kind = match (word(0), blocks.last()) {
            (Some("input"), Some(Block::Machine(_))) => Some(IdKind::Input),
            (Some("state"), Some(Block::Machine(_))) => Some(IdKind::State),
            (Some("event"), Some(Block::Anim)) if line.get(1).is_some_and(|t| t.tok != Tok::Dot) => Some(IdKind::Event),
            (Some("anim"), _) if opens => Some(IdKind::Animation),
            (Some("parameter"), _) => Some(IdKind::Parameter),
            _ => None,
        };
        if let (Some(kind), Some(name)) = (kind, word(1))
            && !name.contains('{')
            && line.len() >= 2
        {
            let owner = match blocks.last() {
                Some(Block::Machine(m)) if matches!(kind, IdKind::Input | IdKind::State) => m.clone(),
                _ => String::new(),
            };
            let id = line.windows(3).find_map(|w| match (&w[0].tok, &w[1].tok, &w[2].tok) {
                (Tok::Ident(k), Tok::Eq, Tok::Ident(v)) if k == "id" => Some(v.clone()),
                _ => None,
            });
            let last = if opens { line.len() - 2 } else { line.len() - 1 };
            out.push(Line { kind, owner, name: name.to_string(), id, at: line[last].span.end });
        }
        for t in line {
            match t.tok {
                Tok::BlockClose => _ = blocks.pop(),
                Tok::BlockOpen => blocks.push(match (word(0), word(1)) {
                    (Some("machine"), Some(name)) => Block::Machine(name.to_string()),
                    (Some("anim"), _) => Block::Anim,
                    _ => Block::Other,
                }),
                _ => {}
            }
        }
    }
    out
}

/// A short id (a letter and three letters or digits) not in `taken`, the same for the same
/// thing every time.
fn fresh(kind: IdKind, owner: &str, name: &str, taken: &HashSet<String>) -> String {
    const ALPHABET: &[u8] = b"abcdefghijkmnpqrstuvwxyz23456789";
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in format!("{kind:?}/{owner}/{name}").bytes() {
        h = (h ^ b as u64).wrapping_mul(0x100_0000_01b3);
    }
    (0u64..)
        .map(|salt| {
            let mut x = h ^ salt.wrapping_mul(0x9e37_79b9_7f4a_7c15);
            let mut id = String::new();
            id.push(ALPHABET[(x % 24) as usize] as char);
            for _ in 0..3 {
                x /= 24;
                id.push(ALPHABET[(x % 32) as usize] as char);
            }
            id
        })
        .find(|id| !taken.contains(id))
        .expect("ids are unbounded")
}

/// `new`, the rewritten text of a line, with the ` id=…` of its old text `old` kept.
pub(crate) fn keep(old: &str, new: String) -> String {
    let old_id = old.split_whitespace().find(|w| w.starts_with("id=") && w.len() > 3);
    match old_id {
        Some(id) if !new.split_whitespace().any(|w| w.starts_with("id=")) => format!("{new} {id}"),
        _ => new,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCENE: &str = "artboard \"a\" 100x80\nrect r size 10 fill #123\nparameter turn \"Поворот\" range -1 1 default 0 value 0\n\
        anim Idle 1s loop {\n  r.x 0: 0 | 1: 0\n  event step 0 0.5\n}\nanim Run 0.5s {\n  r.x 0: 0 | 0.5: 10\n  event step 0.25\n  event.x 0: 1\n}\n\
        machine Hero {\n  input speed 0\n  input jump trigger id=jmp1\n  state Idle\n  state Run at 10 20\n  Idle -> Run when speed > 0.5\n}\n";

    #[test]
    fn every_line_gets_an_id_once_and_events_share_theirs() {
        let src = SCENE.replace("  event.x 0: 1\n", "");
        let out = stamp(&src);
        assert_eq!(stamp(&out), out, "a second save adds nothing");
        let scene = crate::parse(&out).unwrap_or_else(|e| panic!("{e:?}\n{out}"));
        let ids = &scene.doc.ids;
        let get = |kind, owner, name| ids.get(kind, owner, name).unwrap_or_else(|| panic!("{kind:?} {name}\n{out}")).to_string();
        assert_eq!(get(IdKind::Input, "Hero", "jump"), "jmp1", "an id written stays");
        let all = [
            get(IdKind::Input, "Hero", "speed"),
            get(IdKind::State, "Hero", "Idle"),
            get(IdKind::State, "Hero", "Run"),
            get(IdKind::Animation, "", "Idle"),
            get(IdKind::Animation, "", "Run"),
            get(IdKind::Event, "", "step"),
            get(IdKind::Parameter, "", "turn"),
        ];
        let unique: HashSet<_> = all.iter().collect();
        assert_eq!(unique.len(), all.len(), "{all:?}");
        assert_eq!(out.matches(&format!("id={}", all[5])).count(), 2, "both lines of «step» have one id:\n{out}");
        assert!(out.contains(&format!("anim Idle 1s loop id={} {{", all[3])), "{out}");
        assert!(out.contains(&format!("state Run at 10 20 id={}", all[2])), "{out}");
        assert!(out.contains("Idle -> Run when speed > 0.5\n"), "transitions take none:\n{out}");
    }

    #[test]
    fn ids_are_checked() {
        let scene = SCENE.replace("  event.x 0: 1\n", "");
        let twice = scene.replace("input speed 0", "input speed 0 id=jmp1");
        assert!(crate::parse(&twice).unwrap_err()[0].message.contains("уже у «"), "two inputs, one id");
        let split = scene.replace("event step 0 0.5", "event step 0 0.5 id=e1").replace("event step 0.25", "event step 0.25 id=e2");
        assert!(crate::parse(&split).unwrap_err()[0].message.contains("id один"), "one event, two ids");
        assert!(crate::parse(&scene.replace("id=jmp1", "id=")).is_err());
        assert!(crate::parse(&scene).is_ok());
    }

    #[test]
    fn renames_in_motif_keep_the_id() {
        let src = stamp(&SCENE.replace("  event.x 0: 1\n", ""));
        let scene = crate::parse(&src).unwrap();
        let speed = scene.doc.ids.get(IdKind::Input, "Hero", "speed").unwrap().to_string();
        let idle = scene.doc.ids.get(IdKind::State, "Hero", "Idle").unwrap().to_string();
        let renamed = crate::edit_machine(&src, &scene, "Hero", |m| {
            m.inputs[0].name = "скорость".into();
            m.states[0].name = "Покой".into();
            m.transitions[0].from = Some(0);
            m.transitions[0].conds[0].input = "скорость".into();
        })
        .unwrap();
        let scene = crate::parse(&renamed).unwrap_or_else(|e| panic!("{e:?}\n{renamed}"));
        assert_eq!(scene.doc.ids.get(IdKind::Input, "Hero", "скорость"), Some(speed.as_str()), "{renamed}");
        assert_eq!(scene.doc.ids.get(IdKind::State, "Hero", "Покой"), Some(idle.as_str()));
        // An event renamed in one animation while another still sends the old name is a new event.
        let step = scene.doc.ids.get(IdKind::Event, "", "step").unwrap().to_string();
        let moved = crate::write_event(&renamed, &scene, "Idle", "step", "шаг", &[0.0, 0.4]).unwrap();
        let scene = crate::parse(&moved).unwrap_or_else(|e| panic!("{e:?}\n{moved}"));
        assert_eq!(scene.doc.ids.get(IdKind::Event, "", "step"), Some(step.as_str()));
        assert_eq!(scene.doc.ids.get(IdKind::Event, "", "шаг"), None, "{moved}");
        // Renamed in the last animation that sends it, the event keeps its id.
        let all = crate::write_event(&moved, &scene, "Run", "step", "шаг", &[0.25]).unwrap();
        let scene = crate::parse(&stamp(&all)).unwrap();
        assert_eq!(scene.doc.ids.get(IdKind::Event, "", "шаг"), Some(step.as_str()), "{all}");
    }

    #[test]
    fn a_rewritten_line_keeps_its_id() {
        assert_eq!(keep("input speed 0 id=k7f2", "input скорость 0".into()), "input скорость 0 id=k7f2");
        assert_eq!(keep("state Idle", "state Rest".into()), "state Rest");
        assert_eq!(keep("event step 0 id=e1", "event step 0 id=e1".into()), "event step 0 id=e1");
    }
}
