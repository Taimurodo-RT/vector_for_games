//! The live link with a game (Forge) on this computer: the game opens a file where it wants it
//! and plays the character with its own values; Motif tells it what was saved, what in the
//! contract changed and which object is selected. Off until switched on in «Файл», or until the
//! game starts Motif with `--link`.

use crate::app::{App, fold};
use crate::theme::{self, ButtonKind, FAINT, MUTED};
use egui::{RichText, Ui};
use motif_core::{InputKind, Machine};
use motif_link::{FromMotif, Server, ToMotif};
use motif_script::Contract;
use std::path::{Path, PathBuf};

/// What the game told about itself: names it has, for hints.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Profile {
    pub values: Vec<String>,
    pub actions: Vec<String>,
    pub events: Vec<String>,
}

#[derive(Default)]
pub(crate) struct Link {
    pub server: Option<Server>,
    /// The contract of the file as it was saved last, to tell the game what a save changed.
    saved: Option<(PathBuf, Contract)>,
    pub profile: Option<Profile>,
    /// The focus sent last, so it goes again only when it changes.
    focus: Option<(String, Option<String>)>,
    /// Game values without an input, told once until they change.
    unknown: Vec<String>,
}

/// The contract of a whole file, or `None` while its script has errors.
fn contract_of(text: &str, path: &Path) -> Option<Contract> {
    let scenes = motif_script::parse_boards(text, path.parent()).ok()?;
    Some(motif_script::contract(scenes.iter().map(|s| &s.doc)))
}

fn file_name(path: &Path) -> String {
    path.display().to_string()
}

impl App {
    /// Starts or stops listening, and remembers the choice.
    pub(crate) fn set_link(&mut self, on: bool) {
        self.panels.settings.link = on;
        if on {
            self.start_link(self.panels.settings.link_port);
        } else {
            self.link.server = None;
            self.status = "Связь с игрой выключена".into();
        }
    }

    /// Listens on `port`; the status tells where, or why not.
    pub(crate) fn start_link(&mut self, port: u16) {
        if self.link.server.as_ref().is_some_and(|s| s.port() == port) {
            return;
        }
        let ctx = self.ctx.clone();
        match Server::start(port, crate::app::VERSION, move || ctx.request_repaint()) {
            Ok(server) => {
                self.status = format!("Связь с игрой: ждём на 127.0.0.1:{}", server.port());
                self.link.server = Some(server);
                self.link.focus = None;
            }
            Err(e) => self.status = format!("Связь с игрой не включилась на порту {port}: {e}"),
        }
    }

    fn link_send(&self, message: &FromMotif) {
        if let Some(server) = &self.link.server {
            server.send(message);
        }
    }

    fn link_error(&mut self, message: String) {
        self.status = format!("Игра: {message}");
        self.link_send(&FromMotif::Error { message });
    }

    /// Every frame: does what the games asked and tells them the selection.
    pub(crate) fn poll_link(&mut self) {
        let Some(server) = &self.link.server else { return };
        for message in server.poll() {
            match message {
                Ok(ToMotif::Open { file, board, animation, state }) => self.link_open(&file, board, animation, state),
                Ok(ToMotif::GameValues { machine, values }) => self.link_values(machine, values),
                Ok(ToMotif::Profile { values, actions, events }) => {
                    let p = Profile { values, actions, events };
                    self.status = format!("Игра рассказала о себе: значений {}, действий {}, событий {}", p.values.len(), p.actions.len(), p.events.len());
                    self.link.profile = Some(p);
                }
                Err(e) => self.link_error(e),
            }
        }
        let Some(path) = &self.path else { return };
        let focus = (file_name(path), self.selected.clone());
        if self.link.focus.as_ref() != Some(&focus) {
            self.link_send(&FromMotif::Focus { file: focus.0.clone(), object: focus.1.clone() });
            self.link.focus = Some(focus);
        }
    }

    /// «Открыть в Motif»: the file (unless it is open already), then the board, the animation or
    /// the state of a machine.
    fn link_open(&mut self, file: &str, board: Option<String>, animation: Option<String>, state: Option<String>) {
        let path = std::path::absolute(file).unwrap_or_else(|_| PathBuf::from(file));
        if self.path.as_ref() != Some(&path) {
            if self.dirty() {
                return self.link_error(format!("в Motif есть несохранённые правки, «{file}» не открыт: сохраните их"));
            }
            if let Err(e) = self.open_path(path) {
                return self.link_error(e);
            }
        }
        self.ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
        if let Some(name) = board {
            match self.board_specs().iter().position(|(_, n)| *n == name) {
                Some(k) => self.switch_board(k),
                None => return self.link_error(format!("в «{file}» нет холста «{name}»")),
            }
        }
        if let Some(name) = animation {
            if self.scene.doc.animation(&name).is_none() {
                return self.link_error(format!("в «{file}» нет анимации «{name}»"));
            }
            self.run = None;
            self.select_anim(name);
        }
        if let Some(name) = state {
            let found = self.scene.doc.machines.iter().find_map(|m| m.state(&name).map(|i| (m.name.clone(), i)));
            let Some((machine, i)) = found else { return self.link_error(format!("в «{file}» нет состояния «{name}»")) };
            self.show_machine(&machine);
            if self.run.is_none() {
                self.toggle_run();
            }
            if let Some(run) = &mut self.run {
                run.player.jump(i);
            }
            self.status = format!("Игра открыла состояние «{name}» машины «{machine}»");
        }
    }

    /// Picks the machine the bottom panel shows; a running one stops when another is picked.
    fn show_machine(&mut self, name: &str) {
        if self.shown_machine().is_some_and(|m| m.name != name) {
            self.run = None;
            self.machine_pick = None;
        }
        self.machine = Some(name.to_string());
        // The game plays it: the machine is in sight.
        self.bottom_machine = true;
        if !self.panels.layout().timeline {
            self.panels.set(crate::workspace::Panel::Timeline, true);
        }
    }

    /// The game's values drive the machine: it starts if it is not running, and a recorded run
    /// takes them down.
    fn link_values(&mut self, machine: Option<String>, values: std::collections::BTreeMap<String, f64>) {
        if let Some(name) = &machine {
            if self.scene.doc.machine(name).is_none() {
                return self.link_error(format!("в сцене нет машины «{name}»"));
            }
            self.show_machine(name);
        }
        let Some(m) = self.shown_machine().cloned() else { return self.link_error("в сцене нет машины состояний".into()) };
        if machine.is_none() {
            self.show_machine(&m.name);
        }
        if self.run.is_none() {
            self.toggle_run();
        }
        let Some(run) = &mut self.run else { return };
        let mut unknown = Vec::new();
        for (name, v) in &values {
            if m.input(name).is_some() {
                run.player.set(&m, name, *v);
            } else {
                unknown.push(name.clone());
            }
        }
        if unknown != self.link.unknown {
            self.link.unknown = unknown.clone();
            if !unknown.is_empty() {
                self.link_error(format!("у машины «{}» нет входов {}", m.name, unknown.join(", ")));
            }
        }
    }

    /// After a save: the game reads the file again, and hears what its names became.
    pub(crate) fn link_saved(&mut self, path: &Path, before: Option<&str>) {
        if self.link.server.is_none() {
            return;
        }
        let file = file_name(path);
        let Some(now) = contract_of(&self.saved_src, path) else {
            return self.link_error(format!("«{file}» сохранён с ошибками в скрипте: договор для игры не обновлён"));
        };
        let was = match self.link.saved.take() {
            Some((p, c)) if p == path => Some(c),
            _ => before.and_then(|text| contract_of(text, path)),
        };
        self.link_send(&FromMotif::Saved { file: file.clone(), contract: now.clone() });
        if let Some(was) = was {
            let changes = motif_link::changes(&was, &now);
            if !changes.is_empty() {
                self.link_send(&FromMotif::ContractChanged { file, changes });
            }
        }
        self.link.saved = Some((path.to_path_buf(), now));
    }

    /// The «Файл» menu item: on or off, and how many games are connected.
    pub(crate) fn link_menu(&mut self, ui: &mut Ui) {
        let mut on = self.link.server.is_some();
        let text = match &self.link.server {
            Some(s) => format!("Связь с игрой · порт {} · игр {}", s.port(), s.clients()),
            None => "Связь с игрой".into(),
        };
        if ui
            .checkbox(&mut on, text)
            .on_hover_text(
                "Игра (Forge) на этом компьютере открывает файлы в Motif, играет персонажа своими значениями и узнаёт о сохранениях и переименованиях",
            )
            .changed()
        {
            self.set_link(on);
        }
    }

    /// Under the machine's inputs: what the game told about itself, against the scene.
    pub(crate) fn game_hints(&mut self, ui: &mut Ui, m: &Machine) {
        let Some(p) = self.link.profile.clone() else { return };
        let missing: Vec<&String> = p.values.iter().chain(&p.actions).filter(|n| m.input(n).is_none()).collect();
        let emitted: Vec<String> = self.scene.doc.animations.iter().flat_map(|a| a.events.iter().map(|e| e.name.clone())).collect();
        let waited: Vec<&String> = p.events.iter().filter(|e| !emitted.contains(e)).collect();
        let mut unused: Vec<&String> = emitted.iter().filter(|e| !p.events.contains(e)).collect();
        unused.dedup();
        let mut add = None;
        fold(ui, "Что знает игра", |ui| {
            if missing.is_empty() && waited.is_empty() && unused.is_empty() {
                ui.label(RichText::new("Входы и события совпадают с игрой.").size(11.5).color(FAINT));
            }
            for name in &missing {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(name.as_str()).size(12.0));
                    let action = p.actions.contains(name);
                    ui.label(RichText::new(if action { "действие игры" } else { "значение игры" }).size(11.0).color(FAINT));
                    if theme::button(ui, "+ Вход", ButtonKind::Normal).on_hover_text("Добавить вход с этим именем").clicked() {
                        add = Some(((*name).clone(), if action { InputKind::Trigger } else { InputKind::Number }));
                    }
                });
            }
            for name in &waited {
                ui.label(RichText::new(format!("{name} — игра ждёт, ни одна анимация его не шлёт")).size(11.5).color(MUTED));
            }
            for name in &unused {
                ui.label(RichText::new(format!("{name} — событие не используется игрой")).size(11.5).color(FAINT));
            }
        });
        if let Some((name, kind)) = add {
            self.machine_edit("machine-input", |m| m.inputs.push(motif_core::Input { name, kind, default: 0.0 }));
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::app::App;
    use crate::app::editing_tests::fixture;
    use motif_link::{Client, FromMotif, ToMotif};
    use std::time::{Duration, Instant};

    /// Polls until `done` holds, as the window does every frame.
    fn frames(app: &mut App, done: impl Fn(&App) -> bool) {
        let until = Instant::now() + Duration::from_secs(5);
        while !done(app) && Instant::now() < until {
            app.poll_link();
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(done(app), "the link did not deliver");
    }

    /// The next message other than `focus`.
    fn next(game: &mut Client) -> FromMotif {
        loop {
            match game.recv(Duration::from_secs(5)).unwrap().expect("a message from Motif") {
                FromMotif::Focus { .. } => {}
                m => return m,
            }
        }
    }

    #[test]
    fn a_game_opens_a_state_drives_the_machine_and_hears_a_rename() {
        let dir = std::env::temp_dir().join(format!("motif-link-editor-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("hero.msc");
        std::fs::write(&file, include_str!("../../../examples/machine.msc")).unwrap();
        let mut app = fixture("artboard \"Пусто\" 10x10\n");
        app.start_link(0);
        let port = app.link.server.as_ref().expect("listening").port();
        let mut game = Client::connect(port).unwrap();
        assert!(matches!(game.hello, FromMotif::Hello { protocol: motif_link::PROTOCOL, .. }));

        // Opened where the game wants it: the jump state plays.
        let open = ToMotif::Open { file: file.display().to_string(), board: None, animation: None, state: Some("Jump".into()) };
        game.send(&open).unwrap();
        frames(&mut app, |a| a.path.as_ref() == Some(&file));
        let jump = app.scene.doc.machine("Hero").unwrap().state("Jump").unwrap();
        assert_eq!(app.run.as_ref().map(|r| r.player.state), Some(jump));
        match game.recv(Duration::from_secs(5)).unwrap() {
            Some(FromMotif::Focus { file: f, object: None }) => assert_eq!(f, file.display().to_string()),
            other => panic!("focus expected, got {other:?}"),
        }

        // The game's values drive the inputs; a name the machine lacks is told back.
        let values = [("speed".to_string(), 2.0), ("crouch".to_string(), 1.0)].into();
        game.send(&ToMotif::GameValues { machine: Some("Hero".into()), values }).unwrap();
        frames(&mut app, |a| a.run.as_ref().is_some_and(|r| r.player.values[0] == 2.0));
        match next(&mut game) {
            FromMotif::Error { message } => assert!(message.contains("crouch") && !message.contains("speed"), "{message}"),
            other => panic!("error expected, got {other:?}"),
        }

        // What the game knows comes as hints.
        let profile =
            ToMotif::Profile { values: vec!["speed".into(), "crouch".into()], actions: vec!["jump".into()], events: vec!["land".into(), "boom".into()] };
        game.send(&profile).unwrap();
        frames(&mut app, |a| a.link.profile.is_some());

        // The selection is told as it changes.
        app.selected = Some("hero".into());
        app.poll_link();
        match game.recv(Duration::from_secs(5)).unwrap() {
            Some(FromMotif::Focus { object, .. }) => assert_eq!(object.as_deref(), Some("hero")),
            other => panic!("focus expected, got {other:?}"),
        }

        // A save: the game reads the file again and hears the state renamed by its id.
        app.src = app.src.replace("state Jump at", "state Leap anim Jump at").replace("-> Jump", "-> Leap").replace("Jump -> Idle", "Leap -> Idle");
        app.reparse();
        app.save(false);
        match next(&mut game) {
            FromMotif::Saved { file: f, contract } => {
                assert_eq!(f, file.display().to_string());
                assert!(contract.boards[0].machines[0].states.iter().any(|s| s.name == "Leap"));
            }
            other => panic!("saved expected, got {other:?}"),
        }
        match next(&mut game) {
            FromMotif::ContractChanged { changes, .. } => {
                assert_eq!(changes.len(), 1, "{changes:?}");
                let c = &changes[0];
                assert_eq!((c.kind.as_str(), c.id.as_deref(), c.was.as_deref(), c.now.as_deref()), ("state", Some("ekxq"), Some("Jump"), Some("Leap")));
            }
            other => panic!("contract change expected, got {other:?}"),
        }

        // Unsaved edits are never thrown away by the game.
        app.src.push_str("\n// правка\n");
        let other = dir.join("other.msc");
        std::fs::write(&other, "artboard \"Другой\" 10x10\n").unwrap();
        game.send(&ToMotif::Open { file: other.display().to_string(), board: None, animation: None, state: None }).unwrap();
        frames(&mut app, |a| a.status.contains("несохранённые"));
        assert_eq!(app.path.as_ref(), Some(&file));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_game_starts_motif_with_the_link_on() {
        let args = |a: &[&str]| crate::launch(a.iter().map(|s| s.to_string()));
        assert_eq!(args(&["--link", "a.msc"]), (Some("a.msc".into()), Some(motif_link::DEFAULT_PORT)));
        assert_eq!(args(&["a.msc", "--link=5000"]), (Some("a.msc".into()), Some(5000)));
    }
}
