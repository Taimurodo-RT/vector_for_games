//! The scene playing, apart from Godot: which board, the machine driving it and how far it got.
//! A lone animation plays as a machine of one state, so it gets the same events and simulation.

use motif_core::kurbo::Point;
use motif_core::{Document, Machine, MachinePlayer, State};
use motif_render::CpuOptions;
use tiny_skia::Pixmap;

#[derive(Debug)]
pub struct Player {
    doc: Document,
    machine: Machine,
    run: MachinePlayer,
    /// The pose shown now: what the pointer is tested against.
    shown: Document,
}

/// What happened in one [`Player::advance`].
#[derive(Debug, Default, PartialEq)]
pub struct Step {
    /// Animation events passed, in order.
    pub events: Vec<String>,
    /// The state entered, when the machine moved.
    pub state: Option<String>,
}

impl Player {
    /// Board `board` (a name or a number from 1; empty — the first) of scene `src`, played by
    /// machine `machine`, or by animation `animation` alone. With neither, the board's first
    /// machine, else its first animation, else the still board.
    pub fn load(src: &str, base: Option<&std::path::Path>, board: &str, machine: &str, animation: &str) -> Result<Player, String> {
        let boards = motif_script::parse_boards(src, base).map_err(|errs| errs.iter().map(|e| e.to_string()).collect::<Vec<_>>().join("\n"))?;
        let names = || boards.iter().map(|s| format!("«{}»", s.doc.artboard.name)).collect::<Vec<_>>().join(", ");
        let at = match board.parse::<usize>() {
            _ if board.is_empty() => Some(0),
            Ok(n) if (1..=boards.len()).contains(&n) => Some(n - 1),
            _ => boards.iter().position(|s| s.doc.artboard.name == board),
        };
        let doc = at.and_then(|i| boards.get(i)).map(|s| s.doc.clone()).ok_or_else(|| format!("нет холста «{board}»; холсты: {}", names()))?;
        let machine = if !animation.is_empty() {
            doc.animation(animation).ok_or_else(|| {
                let all: Vec<&str> = doc.animations.iter().map(|a| a.name.as_str()).collect();
                format!("нет анимации «{animation}»; анимации: {}", all.join(", "))
            })?;
            Self::alone(animation)
        } else if !machine.is_empty() {
            doc.machine(machine).cloned().ok_or_else(|| {
                let all: Vec<&str> = doc.machines.iter().map(|m| m.name.as_str()).collect();
                format!("нет машины «{machine}»; машины: {}", all.join(", "))
            })?
        } else if let Some(m) = doc.machines.first() {
            m.clone()
        } else {
            doc.animations.first().map_or_else(|| Machine { name: String::new(), ..Self::alone("") }, |a| Self::alone(&a.name))
        };
        let run = MachinePlayer::new(&machine);
        let shown = run.pose(&machine, &doc);
        Ok(Player { doc, machine, run, shown })
    }

    /// A machine of one state playing `anim`.
    fn alone(anim: &str) -> Machine {
        Machine {
            name: anim.to_string(),
            inputs: Vec::new(),
            listeners: Vec::new(),
            states: if anim.is_empty() { Vec::new() } else { vec![State { name: anim.to_string(), anim: anim.to_string(), pos: None }] },
            transitions: Vec::new(),
            any_pos: None,
        }
    }

    /// The artboard's width and height.
    pub fn size(&self) -> (f64, f64) {
        (self.doc.artboard.width, self.doc.artboard.height)
    }

    /// Passes the pointer (in artboard units) to the machine's `listen` lines.
    pub fn pointer(&mut self, at: Option<Point>, down: bool) {
        if !self.machine.listeners.is_empty() {
            self.run.pointer(&self.machine, &self.shown, at, down);
        }
    }

    /// Moves time on by `dt` seconds.
    pub fn advance(&mut self, dt: f64) -> Step {
        if self.machine.states.is_empty() {
            return Step::default();
        }
        let before = self.run.state;
        let taken = self.run.advance(&self.machine, &self.doc, dt.max(0.0));
        self.shown = self.run.pose(&self.machine, &self.doc);
        let state = (taken.is_some() || self.run.state != before).then(|| self.state());
        Step { events: self.run.events.clone(), state }
    }

    /// The state now, or the animation playing.
    pub fn state(&self) -> String {
        self.machine.states.get(self.run.state).map_or(String::new(), |s| s.name.clone())
    }

    pub fn inputs(&self) -> Vec<String> {
        self.machine.inputs.iter().map(|i| i.name.clone()).collect()
    }

    pub fn get(&self, input: &str) -> Option<f64> {
        self.machine.input(input).and_then(|i| self.run.values.get(i).copied())
    }

    pub fn set(&mut self, input: &str, value: f64) -> Result<(), String> {
        self.known(input)?;
        self.run.set(&self.machine, input, value);
        Ok(())
    }

    pub fn fire(&mut self, input: &str) -> Result<(), String> {
        self.known(input)?;
        self.run.fire(&self.machine, input);
        Ok(())
    }

    fn known(&self, input: &str) -> Result<(), String> {
        match self.machine.input(input) {
            Some(_) => Ok(()),
            None => Err(format!("у машины «{}» нет входа «{input}»; входы: {}", self.machine.name, self.inputs().join(", "))),
        }
    }

    /// Goes to `state` at once.
    pub fn jump(&mut self, state: &str) -> Result<(), String> {
        let k = self.machine.state(state).ok_or_else(|| {
            let all: Vec<&str> = self.machine.states.iter().map(|s| s.name.as_str()).collect();
            format!("у машины «{}» нет состояния «{state}»; состояния: {}", self.machine.name, all.join(", "))
        })?;
        self.run.jump(k);
        self.shown = self.run.pose(&self.machine, &self.doc);
        Ok(())
    }

    pub fn restart(&mut self) {
        self.run = MachinePlayer::new(&self.machine);
        self.shown = self.run.pose(&self.machine, &self.doc);
    }

    /// The pose now drawn at `scale` pixels a unit, premultiplied RGBA.
    pub fn render(&self, scale: f64, background: bool) -> Option<Pixmap> {
        motif_render::render_cpu(&self.shown, CpuOptions { scale, antialias: true, background })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCENE: &str = r#"
artboard "Капля" 100x80
ellipse ball at 50 40 size 20 20 fill #5ee0a0
anim Idle 1s loop {
  ball.x 0: 50 | 1: 50
  event blink 0.5
}
anim Jump 0.5s {
  ball.y 0: 40 | 0.5: 10
}
machine Hero {
  input jump trigger
  input speed 0
  state Idle
  state Jump
  any -> Jump when jump
  Jump -> Idle at end
}
"#;

    #[test]
    fn a_machine_plays_and_reports_states_and_events() {
        let mut p = Player::load(SCENE, None, "", "", "").unwrap();
        assert_eq!((p.size(), p.state()), ((100.0, 80.0), "Idle".to_string()));
        assert_eq!(p.inputs(), ["jump", "speed"]);
        let step = p.advance(0.6);
        assert_eq!(step, Step { events: vec!["blink".into()], state: None });
        p.fire("jump").unwrap();
        assert_eq!(p.advance(0.01).state.as_deref(), Some("Jump"));
        for _ in 0..60 {
            p.advance(0.01);
        }
        assert_eq!(p.state(), "Idle", "back at the end of Jump");
        p.set("speed", 2.0).unwrap();
        assert_eq!(p.get("speed"), Some(2.0));
        assert!(p.set("fly", 1.0).unwrap_err().contains("jump, speed"));
        p.jump("Jump").unwrap();
        assert_eq!(p.state(), "Jump");
        p.restart();
        assert_eq!((p.state(), p.get("speed")), ("Idle".to_string(), Some(0.0)));
        let px = p.render(2.0, false).unwrap();
        assert_eq!((px.width(), px.height()), (200, 160));
    }

    #[test]
    fn an_animation_plays_alone_and_mistakes_are_named() {
        let mut p = Player::load(SCENE, None, "1", "", "Jump").unwrap();
        assert_eq!(p.state(), "Jump");
        p.advance(0.5);
        assert!(p.inputs().is_empty());
        assert!(Player::load(SCENE, None, "Меню", "", "").unwrap_err().contains("«Капля»"));
        assert!(Player::load(SCENE, None, "", "Boss", "").unwrap_err().contains("Hero"));
        assert!(Player::load(SCENE, None, "", "", "Run").unwrap_err().contains("Idle, Jump"));
        assert!(Player::load("rect", None, "", "", "").is_err());
    }
}
