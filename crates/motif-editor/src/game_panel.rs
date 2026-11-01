//! «Для игры»: everything the game (Forge) binds to in this file, board by board, as the
//! contract tells it, with what is likely to go wrong. A click leads to the thing.

use crate::app::App;
use crate::link_ui::Profile;
use crate::theme::{FAINT, FG, MUTED};
use egui::RichText;
use motif_core::{Document, EventValue, IdKind};
use std::collections::BTreeMap;

/// What a click in the panel leads to.
enum Go {
    Anim(usize, String),
    Node(usize, String),
    Machine(usize, String),
}

/// What is likely to go wrong for a game binding to these boards, each with its board's name.
pub(crate) fn warnings(docs: &[Document], profile: Option<&Profile>) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut sent: Vec<&str> = Vec::new();
    let mut inputs: Vec<(&str, motif_core::InputKind)> = Vec::new();
    for doc in docs {
        let board = doc.artboard.name.clone();
        let mut say = |s: String| out.push((board.clone(), s));
        // Names without their hidden id: a rename before the next save breaks the game.
        let mut bare = 0;
        let mut count = |kind, owner: &str, name: &str| bare += usize::from(doc.ids.get(kind, owner, name).is_none());
        for a in &doc.animations {
            count(IdKind::Animation, "", &a.name);
            for e in &a.events {
                count(IdKind::Event, "", &e.name);
            }
        }
        for m in &doc.machines {
            for i in &m.inputs {
                count(IdKind::Input, &m.name, &i.name);
            }
            for s in &m.states {
                count(IdKind::State, &m.name, &s.name);
            }
        }
        for p in &doc.parameters {
            count(IdKind::Parameter, "", &p.id);
        }
        if bare > 0 {
            say(format!("Без скрытого id: {bare}. Id появятся при сохранении (Ctrl+S); до этого игра узнаёт их только по имени"));
        }
        // One event name, other data in another animation: the game reads one set of keys.
        let mut keys: BTreeMap<&str, Vec<(&str, Vec<&str>)>> = BTreeMap::new();
        for a in &doc.animations {
            for e in &a.events {
                let mut k: Vec<&str> = e.data.iter().map(|(k, _)| k.as_str()).collect();
                k.sort();
                let seen = keys.entry(e.name.as_str()).or_default();
                if !seen.iter().any(|(_, o)| *o == k) {
                    seen.push((a.name.as_str(), k));
                }
                sent.push(&e.name);
            }
        }
        for (name, sets) in keys.iter().filter(|(_, s)| s.len() > 1) {
            let list = |k: &Vec<&str>| if k.is_empty() { "без данных".to_string() } else { k.join(", ") };
            let sets: Vec<String> = sets.iter().map(|(a, k)| format!("в {a}: {}", list(k))).collect();
            say(format!("У события «{name}» разные данные — {}", sets.join("; ")));
        }
        // An input nothing reads: the game sets it and nothing happens.
        for m in &doc.machines {
            for i in &m.inputs {
                inputs.push((&i.name, i.kind));
                let read = m.transitions.iter().any(|t| t.conds.iter().any(|c| c.input == i.name)) || m.listeners.iter().any(|l| l.input == i.name);
                if !read {
                    say(format!("Вход «{}» машины «{}» не влияет ни на один переход", i.name, m.name));
                }
            }
        }
        // Morphing shapes the ReyVec engine would show otherwise (strokes, colours, lines).
        for w in motif_script::reyvec_morph_warnings([doc]) {
            say(format!("Для ReyVec: {w}"));
        }
    }
    // What the game told about itself, against the file.
    if let Some(p) = profile {
        let all = String::from("вся сцена");
        let mut say = |s: String| out.push((all.clone(), s));
        for v in p.values.iter().filter(|v| !inputs.iter().any(|(n, _)| n == v)) {
            say(format!("Игра передаёт «{v}», а входа с таким именем нет"));
        }
        for a in p.actions.iter().filter(|a| !inputs.iter().any(|(n, _)| n == a)) {
            say(format!("Действие игры «{a}» не связано ни с одним входом"));
        }
        for e in p.events.iter().filter(|e| !sent.contains(&e.as_str())) {
            say(format!("Игра ждёт событие «{e}», а ни одна анимация его не шлёт"));
        }
        let mut extra: Vec<&str> = sent.iter().copied().filter(|e| !p.events.iter().any(|w| w == e)).collect();
        extra.sort();
        extra.dedup();
        for e in extra {
            say(format!("Событие «{e}» игра не слушает"));
        }
    }
    out
}

fn value(v: &EventValue) -> String {
    match v {
        EventValue::Number(n) => motif_script::fmt_num(*n),
        EventValue::Bool(b) => b.to_string(),
        EventValue::Text(t) => format!("\"{t}\""),
    }
}

impl App {
    /// Every board of the file as a game sees it.
    pub(crate) fn game_docs(&self) -> Vec<Document> {
        (0..self.board_count()).filter_map(|k| self.board_doc(k)).collect()
    }

    pub(crate) fn game_window(&mut self, ctx: &egui::Context) {
        if !self.show_game {
            return;
        }
        let docs = self.game_docs();
        let warned = warnings(&docs, self.link.profile.as_ref());
        let mut open = true;
        let mut go = None;
        egui::Window::new("Для игры").id(egui::Id::unique("game-panel")).open(&mut open).default_width(380.0).default_height(520.0).show(ctx, |ui| {
            ui.label(
                RichText::new("Всё, к чему привязывается игра: имена из договора сцены. Щелчок ведёт к объекту, анимации или машине.").size(11.5).color(FAINT),
            );
            egui::ScrollArea::vertical().show(ui, |ui| {
                if !warned.is_empty() {
                    ui.add_space(4.0);
                    ui.label(RichText::new(format!("Внимание · {}", warned.len())).size(12.5).strong().color(crate::theme::ACCENT));
                    for (board, w) in &warned {
                        let at = if docs.len() > 1 { format!("{board}: ") } else { String::new() };
                        ui.label(RichText::new(format!("• {at}{w}")).size(12.0).color(FG));
                    }
                }
                for (k, doc) in docs.iter().enumerate() {
                    ui.add_space(6.0);
                    ui.label(RichText::new(format!("{} · {}×{}", doc.artboard.name, doc.artboard.width, doc.artboard.height)).size(13.0).strong().color(FG));
                    let id = |kind, owner: &str, name: &str| doc.ids.get(kind, owner, name).map_or(" · без id".to_string(), |i| format!(" · id {i}"));
                    if !doc.animations.is_empty() {
                        ui.label(RichText::new("Анимации").size(11.5).color(MUTED));
                    }
                    for a in &doc.animations {
                        let text = format!(
                            "{} · {} с{}{}",
                            a.name,
                            motif_script::fmt_num(a.duration),
                            if a.looping { " · повтор" } else { "" },
                            id(IdKind::Animation, "", &a.name)
                        );
                        if ui.selectable_label(false, RichText::new(text).size(12.0)).clicked() {
                            go = Some(Go::Anim(k, a.name.clone()));
                        }
                        for e in &a.events {
                            let data: Vec<String> = e.data.iter().map(|(k, v)| format!("{k}={}", value(v))).collect();
                            let data = if data.is_empty() { String::new() } else { format!(" {}", data.join(" ")) };
                            ui.label(RichText::new(format!("    ⚑ {} на {} с{data}", e.name, motif_script::fmt_num(e.time))).size(11.5).color(MUTED));
                        }
                    }
                    for m in &doc.machines {
                        ui.label(RichText::new(format!("Машина {}", m.name)).size(11.5).color(MUTED));
                        let inputs: Vec<String> = m.inputs.iter().map(|i| i.name.clone()).collect();
                        let states: Vec<String> = m.states.iter().map(|s| s.name.clone()).collect();
                        let text = format!("входы: {} · состояния: {}", inputs.join(", "), states.join(", "));
                        if ui.selectable_label(false, RichText::new(text).size(12.0)).clicked() {
                            go = Some(Go::Machine(k, m.name.clone()));
                        }
                    }
                    if !doc.parameters.is_empty() {
                        ui.label(RichText::new("Параметры позы").size(11.5).color(MUTED));
                        let list: Vec<String> =
                            doc.parameters.iter().map(|p| format!("{} {}…{}", p.id, motif_script::fmt_num(p.min), motif_script::fmt_num(p.max))).collect();
                        ui.label(RichText::new(list.join(", ")).size(12.0).color(FG));
                    }
                    for (title, pick) in [("Точки крепления", true), ("Области", false)] {
                        let nodes: Vec<&motif_core::Node> = doc.nodes.iter().filter(|n| if pick { n.socket } else { n.area }).collect();
                        if nodes.is_empty() {
                            continue;
                        }
                        ui.label(RichText::new(title).size(11.5).color(MUTED));
                        ui.horizontal_wrapped(|ui| {
                            for n in nodes {
                                let text = n.name.as_ref().map_or(n.id.clone(), |name| format!("{} ({name})", n.id));
                                if ui.selectable_label(false, RichText::new(text).size(12.0)).clicked() {
                                    go = Some(Go::Node(k, n.id.clone()));
                                }
                            }
                        });
                    }
                    if let Some(d) = &doc.directions {
                        ui.label(
                            RichText::new(format!("Стороны: {}{}", d.names().join(", "), if d.mirror { " (левые — отражение)" } else { "" }))
                                .size(12.0)
                                .color(FG),
                        );
                    }
                }
            });
        });
        self.show_game = open;
        match go {
            Some(Go::Anim(k, name)) => {
                self.switch_board(k);
                self.set_workspace(crate::workspace::Workspace::Animate);
                self.select_anim(name);
            }
            Some(Go::Node(k, id)) => {
                self.switch_board(k);
                self.select_only(Some(id));
            }
            Some(Go::Machine(k, name)) => {
                self.switch_board(k);
                self.machine = Some(name);
                self.machine_pick = None;
                self.bottom_machine = true;
                if !self.panels.layout().timeline {
                    self.panels.set(crate::workspace::Panel::Timeline, true);
                }
            }
            None => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(src: &str) -> Document {
        motif_script::parse(src).unwrap().doc
    }

    #[test]
    fn the_panel_warns_of_what_breaks_a_game() {
        let src = "artboard \"a\" 100x80\nrect r size 10\nanim Attack 1s id=a1 {\n  r.x 0: 0 | 1: 10\n  event hit 0.3 damage=2 id=e1\n}\nanim Kick 1s id=a2 {\n  r.x 0: 0 | 1: 10\n  event hit 0.5 damage=1 knock=3\n}\nmachine M {\n  input speed 0 id=i1\n  input idle false id=i2\n  state Attack id=s1\n  state Kick id=s2\n  Attack -> Kick when speed > 1\n}\n";
        let w: Vec<String> = warnings(&[doc(src)], None).into_iter().map(|(_, w)| w).collect();
        assert!(w.iter().any(|w| w.contains("«hit»") && w.contains("в Attack: damage") && w.contains("в Kick: damage, knock")), "{w:?}");
        assert!(w.iter().any(|w| w.contains("«idle»")) && !w.iter().any(|w| w.contains("«speed»")), "{w:?}");
        assert!(!w.iter().any(|w| w.contains("Без скрытого id")), "every name has its id: {w:?}");
        // Unsaved names: counted, not listed one by one.
        let w = warnings(&[doc(&src.replace(" id=a2", "").replace(" id=s2", ""))], None);
        assert!(w.iter().any(|(_, w)| w.starts_with("Без скрытого id: 2.")), "{w:?}");
        // What the game told about itself.
        let profile = Profile { values: vec!["speed".into(), "crouch".into()], actions: vec!["jump".into()], events: vec!["hit".into(), "land".into()] };
        let w: Vec<String> = warnings(&[doc(src)], Some(&profile)).into_iter().map(|(_, w)| w).collect();
        assert!(w.iter().any(|w| w.contains("«crouch»")) && !w.iter().any(|w| w.contains("передаёт «speed»")), "{w:?}");
        assert!(w.iter().any(|w| w.contains("«jump»")) && w.iter().any(|w| w.contains("ждёт событие «land»")), "{w:?}");
        assert!(!w.iter().any(|w| w.contains("«hit» игра не слушает")), "{w:?}");
        // A clean scene: nothing to say.
        let clean = "artboard \"a\" 100x80\nrect r size 10\nanim Idle 1s id=a1 {\n  r.x 0: 0 | 1: 10\n  event step 0.5 id=e1\n}\n";
        assert!(warnings(&[doc(clean)], None).is_empty());
    }

    #[test]
    fn the_panel_says_what_reyvec_would_show_otherwise() {
        let src = "artboard \"a\" 100x80\nstar s hidden fill #ff0000\nellipse ball size 40 40 fill #00ff00 stroke #000 2 morph s\nellipse fine size 40 40 fill #ff0000 morph s\n";
        let w: Vec<String> = warnings(&[doc(src)], None).into_iter().map(|(_, w)| w).collect();
        assert!(w.iter().any(|w| w.starts_with("Для ReyVec: «ball»") && w.contains("обводка")), "{w:?}");
        assert!(!w.iter().any(|w| w.contains("«fine»")), "{w:?}");
    }
}
