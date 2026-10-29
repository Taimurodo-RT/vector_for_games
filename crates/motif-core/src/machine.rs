//! State machines: animations as states, switched by transitions whose conditions read inputs
//! that the game sets (a flag, a number, a one-shot trigger).

use crate::{Document, Prop};
use kurbo::Point;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InputKind {
    Bool,
    Number,
    /// Fires once: true for the step it is fired in, then false again.
    Trigger,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Input {
    pub name: String,
    pub kind: InputKind,
    /// The starting value; 0 or 1 for a flag, 0 for a trigger.
    pub default: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct State {
    pub name: String,
    /// The animation the state plays.
    pub anim: String,
    /// Where the state's box sits in the editor graph.
    pub pos: Option<(f64, f64)>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CondOp {
    /// A flag is on, or a trigger fired.
    Is,
    /// A flag is off.
    Not,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
}

impl CondOp {
    pub const COMPARE: [CondOp; 6] = [CondOp::Gt, CondOp::Ge, CondOp::Lt, CondOp::Le, CondOp::Eq, CondOp::Ne];

    pub fn symbol(self) -> &'static str {
        match self {
            CondOp::Is => "",
            CondOp::Not => "not",
            CondOp::Eq => "==",
            CondOp::Ne => "!=",
            CondOp::Lt => "<",
            CondOp::Le => "<=",
            CondOp::Gt => ">",
            CondOp::Ge => ">=",
        }
    }

    pub fn from_symbol(s: &str) -> Option<Self> {
        Self::COMPARE.into_iter().find(|o| o.symbol() == s)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Cond {
    pub input: String,
    pub op: CondOp,
    /// What a number is compared with (unused for `Is` and `Not`).
    pub value: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Transition {
    /// The state it leaves, or `None` for `any`: from whatever state is playing.
    pub from: Option<usize>,
    pub to: usize,
    /// All must hold.
    pub conds: Vec<Cond>,
    /// Wait until the animation of the state has played to its end.
    pub at_end: bool,
    /// Seconds over which the old animation fades into the new one.
    pub blend: f64,
}

/// What a pointer listener watches on its object.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PointerEvent {
    /// A flag that is on while the pointer is over the object.
    Hover,
    /// A flag that is on while the button, pressed on the object, is held.
    Press,
    /// Released on the object it was pressed on: fires a trigger, flips a flag, adds 1 to a number.
    Click,
    /// Where the pointer is across the object, 0 at the left edge and 1 at the right.
    X,
    /// Where the pointer is down the object, 0 at the top and 1 at the bottom.
    Y,
}

impl PointerEvent {
    pub const ALL: [PointerEvent; 5] = [PointerEvent::Hover, PointerEvent::Press, PointerEvent::Click, PointerEvent::X, PointerEvent::Y];

    pub fn keyword(self) -> &'static str {
        match self {
            PointerEvent::Hover => "hover",
            PointerEvent::Press => "press",
            PointerEvent::Click => "click",
            PointerEvent::X => "x",
            PointerEvent::Y => "y",
        }
    }

    pub fn from_keyword(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|e| e.keyword() == s)
    }

    /// The kinds of input the event can drive.
    pub fn fits(self, kind: InputKind) -> bool {
        match self {
            PointerEvent::Hover | PointerEvent::Press => kind == InputKind::Bool,
            PointerEvent::Click => true,
            PointerEvent::X | PointerEvent::Y => kind == InputKind::Number,
        }
    }
}

/// `listen btn hover -> hover`: the pointer on an object drives an input, so a button reacts by
/// itself in the editor preview and in a game that passes the pointer to the machine.
#[derive(Clone, Debug, PartialEq)]
pub struct Listener {
    pub node: String,
    pub event: PointerEvent,
    pub input: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Machine {
    pub name: String,
    pub inputs: Vec<Input>,
    pub listeners: Vec<Listener>,
    /// The first state is where the machine starts.
    pub states: Vec<State>,
    pub transitions: Vec<Transition>,
    /// Where the `any` box sits in the editor graph.
    pub any_pos: Option<(f64, f64)>,
}

impl Machine {
    pub fn input(&self, name: &str) -> Option<usize> {
        self.inputs.iter().position(|i| i.name == name)
    }

    pub fn state(&self, name: &str) -> Option<usize> {
        self.states.iter().position(|s| s.name == name)
    }
}

/// A running copy of a machine: what state it is in, how far its animation has played, and the
/// values of the inputs.
#[derive(Clone, Debug, PartialEq)]
pub struct MachinePlayer {
    pub state: usize,
    pub time: f64,
    /// The state being blended out, and its time.
    pub prev: Option<(usize, f64)>,
    pub blend_time: f64,
    pub blend: f64,
    pub values: Vec<f64>,
    /// Springs, jiggling bones and soft bodies, stepped as the machine plays.
    pub sim: Option<crate::SimState>,
    /// Whether the pointer button was down at the last [`pointer`](Self::pointer) call.
    pub down: bool,
    /// The listened objects the held button was pressed on.
    pub grab: Vec<String>,
    /// The animation events passed in the last [`advance`](Self::advance), in order.
    pub events: Vec<String>,
    /// The current state has just begun: its events at 0 are still to come.
    fresh: bool,
}

impl MachinePlayer {
    pub fn new(m: &Machine) -> Self {
        MachinePlayer {
            state: 0,
            time: 0.0,
            prev: None,
            blend_time: 0.0,
            blend: 0.0,
            values: m.inputs.iter().map(|i| i.default).collect(),
            sim: None,
            down: false,
            grab: Vec::new(),
            events: Vec::new(),
            fresh: true,
        }
    }

    pub fn set(&mut self, m: &Machine, input: &str, value: f64) {
        if let Some(i) = m.input(input) {
            self.values[i] = value;
        }
    }

    /// Fires a trigger (or sets a flag): the next [`advance`](Self::advance) sees it.
    pub fn fire(&mut self, m: &Machine, input: &str) {
        self.set(m, input, 1.0);
    }

    /// Passes the pointer to the machine's listeners: `at` is where it is in artboard space
    /// (`None` off the artboard view), `down` whether the main button is held, `doc` the document
    /// as it is shown now. Call it before [`advance`](Self::advance) every frame.
    pub fn pointer(&mut self, m: &Machine, doc: &Document, at: Option<Point>, down: bool) {
        self.values.resize(m.inputs.len(), 0.0);
        let over = |node: &str| at.is_some_and(|p| doc.index_of(node).is_some_and(|i| doc.hits_tree(i, p)));
        let pressed = down && !self.down;
        let released = !down && self.down;
        if pressed {
            self.grab = m.listeners.iter().filter(|l| over(&l.node)).map(|l| l.node.clone()).collect();
            self.grab.dedup();
        }
        for l in &m.listeners {
            let Some(i) = m.input(&l.input) else { continue };
            let held = self.grab.contains(&l.node);
            match l.event {
                PointerEvent::Hover => self.values[i] = f64::from(u8::from(over(&l.node))),
                PointerEvent::Press => self.values[i] = f64::from(u8::from(down && held)),
                PointerEvent::Click => {
                    if released && held && over(&l.node) {
                        self.values[i] = match m.inputs[i].kind {
                            InputKind::Trigger => 1.0,
                            InputKind::Bool => f64::from(u8::from(self.values[i] == 0.0)),
                            InputKind::Number => self.values[i] + 1.0,
                        };
                    }
                }
                PointerEvent::X | PointerEvent::Y => {
                    // Follows the pointer over the object, and keeps following a drag that began on it.
                    let (Some(p), Some(b)) = (at, doc.index_of(&l.node).and_then(|n| doc.world_bounds(n))) else { continue };
                    if over(&l.node) || (down && held) {
                        let u = if l.event == PointerEvent::X { (p.x - b.x0) / b.width().max(1e-9) } else { (p.y - b.y0) / b.height().max(1e-9) };
                        self.values[i] = (u.clamp(0.0, 1.0) * 1000.0).round() / 1000.0;
                    }
                }
            }
        }
        if released {
            self.grab.clear();
        }
        self.down = down;
    }

    /// Jumps straight to `state`, without a blend.
    pub fn jump(&mut self, state: usize) {
        self.state = state;
        self.time = 0.0;
        self.prev = None;
        self.fresh = true;
    }

    fn holds(&self, m: &Machine, c: &Cond) -> bool {
        let Some(i) = m.input(&c.input) else { return false };
        let v = self.values.get(i).copied().unwrap_or(0.0);
        match c.op {
            CondOp::Is => v != 0.0,
            CondOp::Not => v == 0.0,
            CondOp::Eq => (v - c.value).abs() < 1e-9,
            CondOp::Ne => (v - c.value).abs() >= 1e-9,
            CondOp::Lt => v < c.value,
            CondOp::Le => v <= c.value,
            CondOp::Gt => v > c.value,
            CondOp::Ge => v >= c.value,
        }
    }

    /// The transition that would be taken now: those of the current state first, then `any`.
    pub fn ready(&self, m: &Machine, doc: &Document) -> Option<usize> {
        let ended = m.states.get(self.state).and_then(|s| doc.animation(&s.anim)).is_none_or(|a| self.time >= a.duration - 1e-9);
        let ok = |t: &Transition| (!t.at_end || ended) && t.conds.iter().all(|c| self.holds(m, c));
        let own = m.transitions.iter().position(|t| t.from == Some(self.state) && ok(t));
        own.or_else(|| m.transitions.iter().position(|t| t.from.is_none() && t.to != self.state && ok(t)))
    }

    /// Moves time on by `dt` seconds and takes at most one transition. Triggers are used up.
    /// Returns the transition taken.
    pub fn advance(&mut self, m: &Machine, doc: &Document, dt: f64) -> Option<usize> {
        if m.states.is_empty() {
            return None;
        }
        self.state = self.state.min(m.states.len() - 1);
        self.values.resize(m.inputs.len(), 0.0);
        let from = if self.fresh { -1e-9 } else { self.time };
        self.fresh = false;
        self.time += dt;
        self.events = doc
            .animation(&m.states[self.state].anim)
            .map_or_else(Vec::new, |a| a.events_between(from, self.time).into_iter().map(|e| e.name.clone()).collect());
        if let Some((_, t)) = &mut self.prev {
            *t += dt;
        }
        self.blend_time += dt;
        if self.blend_time >= self.blend {
            self.prev = None;
        }
        let taken = self.ready(m, doc);
        if let Some(k) = taken {
            let t = &m.transitions[k];
            self.prev = (t.blend > 0.0).then_some((self.state, self.time));
            self.blend = t.blend;
            self.blend_time = 0.0;
            self.state = t.to;
            self.time = 0.0;
            self.fresh = true;
        }
        for (v, input) in self.values.iter_mut().zip(&m.inputs) {
            if input.kind == InputKind::Trigger {
                *v = 0.0;
            }
        }
        if doc.has_sim() {
            let keys = self.keys(m, doc);
            match &mut self.sim {
                Some(s) if s.fits(doc) => s.advance(&keys, dt),
                sim => *sim = Some(crate::SimState::new(&keys)),
            }
        } else {
            self.sim = None;
        }
        taken
    }

    /// How far the blend into the current state has got, from 0 to 1 (1 when there is none).
    pub fn blend_amount(&self) -> f64 {
        if self.prev.is_some() && self.blend > 0.0 { (self.blend_time / self.blend).clamp(0.0, 1.0) } else { 1.0 }
    }

    /// The document posed by the current state, mixed with the previous one during a blend,
    /// with bones solved, shapes bent and the simulation applied.
    pub fn pose(&self, m: &Machine, doc: &Document) -> Document {
        let keys = self.keys(m, doc);
        match &self.sim {
            Some(s) if s.fits(doc) => s.apply(&keys),
            _ => {
                let mut out = keys;
                out.rig();
                out
            }
        }
    }

    /// The keys of the current state, mixed with the previous one during a blend, before `rig`.
    fn keys(&self, m: &Machine, doc: &Document) -> Document {
        let pose_of = |state: usize, t: f64| match m.states.get(state).and_then(|s| doc.animation(&s.anim)) {
            Some(a) => a.keys_at(doc, t),
            None => doc.clone(),
        };
        let mut out = pose_of(self.state, self.time);
        let w = self.blend_amount();
        let Some((prev, prev_t)) = self.prev.filter(|_| w < 1.0) else {
            return out;
        };
        let mut old = pose_of(prev, prev_t);
        let mut props: Vec<(&str, Prop)> = Vec::new();
        for s in [prev, self.state] {
            if let Some(a) = m.states.get(s).and_then(|s| doc.animation(&s.anim)) {
                props.extend(a.tracks.iter().map(|t| (t.node.as_str(), t.prop)));
            }
        }
        props.sort_unstable();
        props.dedup();
        let mut target = out.clone();
        old.forms_all();
        target.forms_all();
        for &(node, prop) in props.iter().filter(|(_, p)| *p == Prop::Parameter) {
            if let (Some(a), Some(b)) = (old.animated_value(node, prop), out.animated_value(node, prop)) {
                out.set_animated_value(node, prop, a.lerp(b, w));
            }
        }
        out.apply_parameters();
        for (node, prop) in props.into_iter().filter(|(_, p)| *p != Prop::Parameter) {
            if prop == Prop::Contour {
                if let (Some(a), Some(b), Some(i)) = (old.node(node), target.node(node), out.index_of(node)) {
                    let path = crate::forms::blend_forms(&a.editable_path(), &b.editable_path(), w);
                    if let Some(forms) = &mut out.nodes[i].forms {
                        forms.shapes.push(path);
                        forms.amount = forms.shapes.len() as f64;
                        forms.mix = None;
                    }
                }
            } else if let (Some(a), Some(b)) = (old.animated_value(node, prop), target.animated_value(node, prop)) {
                let value = if prop == Prop::Visible { if w < 0.5 { a } else { b } } else { a.lerp(b, w) };
                out.set_animated_value(node, prop, value);
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Animation, Artboard, Ease, Keyframe, Node, ShapeKind, Track, Value};

    fn doc() -> Document {
        let mut d = Document::new(Artboard::default());
        d.nodes.push(Node::new("b", ShapeKind::Rect));
        let anim = |name: &str, x: f64, looping: bool| Animation {
            name: name.into(),
            duration: 1.0,
            looping,
            tracks: vec![Track {
                node: "b".into(),
                prop: Prop::X,
                keys: vec![Keyframe { time: 0.0, value: Value::Num(x), ease: Ease::Linear }, Keyframe { time: 1.0, value: Value::Num(x), ease: Ease::Linear }],
            }],
            events: Vec::new(),
        };
        d.animations = vec![anim("Idle", 0.0, true), anim("Run", 100.0, true), anim("Jump", 50.0, false)];
        d
    }

    fn machine() -> Machine {
        let st = |n: &str| State { name: n.into(), anim: n.into(), pos: None };
        Machine {
            name: "M".into(),
            inputs: vec![
                Input { name: "speed".into(), kind: InputKind::Number, default: 0.0 },
                Input { name: "jump".into(), kind: InputKind::Trigger, default: 0.0 },
            ],
            listeners: Vec::new(),
            states: vec![st("Idle"), st("Run"), st("Jump")],
            transitions: vec![
                Transition { from: Some(0), to: 1, conds: vec![Cond { input: "speed".into(), op: CondOp::Gt, value: 0.5 }], at_end: false, blend: 0.5 },
                Transition { from: Some(1), to: 0, conds: vec![Cond { input: "speed".into(), op: CondOp::Le, value: 0.5 }], at_end: false, blend: 0.0 },
                Transition { from: None, to: 2, conds: vec![Cond { input: "jump".into(), op: CondOp::Is, value: 0.0 }], at_end: false, blend: 0.0 },
                Transition { from: Some(2), to: 0, conds: vec![], at_end: true, blend: 0.0 },
            ],
            any_pos: None,
        }
    }

    #[test]
    fn events_fire_as_states_play() {
        let mut d = doc();
        let ev = |name: &str, time| crate::AnimEvent { name: name.into(), time };
        d.animations[1].events = vec![ev("step", 0.0), ev("step", 0.5)];
        d.animations[2].events = vec![ev("land", 1.0)];
        let m = machine();
        let mut p = MachinePlayer::new(&m);
        p.set(&m, "speed", 1.0);
        p.advance(&m, &d, 0.1);
        assert!(p.events.is_empty(), "Idle has none; Run only begins");
        p.advance(&m, &d, 0.1);
        assert_eq!(p.events, ["step"], "Run's event at 0 fires on the first step in it");
        p.advance(&m, &d, 0.3);
        assert!(p.events.is_empty());
        p.advance(&m, &d, 0.2);
        assert_eq!(p.events, ["step"], "0.5");
        p.advance(&m, &d, 0.5);
        assert_eq!(p.events, ["step"], "1.0 is the next pass's 0");
        p.fire(&m, "jump");
        p.advance(&m, &d, 0.1);
        p.advance(&m, &d, 0.95);
        assert!(p.events.is_empty());
        p.advance(&m, &d, 0.1);
        assert_eq!(p.events, ["land"]);
    }

    #[test]
    fn switches_blends_and_uses_up_triggers() {
        let (d, m) = (doc(), machine());
        let mut p = MachinePlayer::new(&m);
        assert_eq!(p.advance(&m, &d, 0.1), None);
        p.set(&m, "speed", 1.0);
        assert_eq!(p.advance(&m, &d, 0.1), Some(0));
        assert_eq!(p.state, 1);
        p.advance(&m, &d, 0.25);
        let x = p.pose(&m, &d).nodes[0].x;
        assert!((x - 50.0).abs() < 1e-6, "half way through the blend: {x}");
        p.advance(&m, &d, 0.3);
        assert_eq!(p.pose(&m, &d).nodes[0].x, 100.0);

        p.fire(&m, "jump");
        assert_eq!(p.advance(&m, &d, 0.1), Some(2));
        assert_eq!(p.values[1], 0.0, "the trigger is used up");
        p.advance(&m, &d, 0.5);
        assert_eq!(p.state, 2, "waits for the end of Jump");
        p.advance(&m, &d, 0.6);
        assert_eq!(p.state, 0);
        // `any` never restarts the state that is already playing.
        p.set(&m, "speed", 0.0);
        p.fire(&m, "jump");
        p.advance(&m, &d, 0.1);
        p.fire(&m, "jump");
        p.advance(&m, &d, 0.1);
        assert_eq!((p.state, p.time > 0.05), (2, true));
    }

    #[test]
    fn springs_smooth_a_switch_while_the_machine_plays() {
        let (mut d, m) = (doc(), machine());
        d.nodes[0].spring = Some(crate::Spring { freq: 2.0, damp: 0.3 });
        let mut p = MachinePlayer::new(&m);
        p.advance(&m, &d, 0.0);
        p.set(&m, "speed", 1.0);
        p.advance(&m, &d, 0.0);
        // Run snaps x to 100 after its blend, but the spring follows over time.
        let mut xs = Vec::new();
        for _ in 0..120 {
            p.advance(&m, &d, 1.0 / 60.0);
            xs.push(p.pose(&m, &d).nodes[0].x);
        }
        assert!(xs[5] < 20.0, "{}", xs[5]);
        assert!(xs.iter().cloned().fold(f64::MIN, f64::max) > 101.0, "overshoots");
    }

    #[test]
    fn pointer_listeners_drive_inputs() {
        let mut d = doc();
        let b = &mut d.nodes[0];
        (b.x, b.y, b.width, b.height) = (100.0, 50.0, 100.0, 40.0);
        let flag = |n: &str| Input { name: n.into(), kind: InputKind::Bool, default: 0.0 };
        let listen = |event, input: &str| Listener { node: "b".into(), event, input: input.into() };
        let mut m = machine();
        m.inputs.extend([flag("hover"), flag("press"), flag("on")]);
        m.listeners = vec![
            listen(PointerEvent::Hover, "hover"),
            listen(PointerEvent::Press, "press"),
            listen(PointerEvent::Click, "jump"),
            listen(PointerEvent::Click, "on"),
            listen(PointerEvent::X, "speed"),
        ];
        let mut p = MachinePlayer::new(&m);
        let v = |p: &MachinePlayer, n: &str| p.values[m.input(n).unwrap()];
        p.pointer(&m, &d, Some(Point::new(10.0, 10.0)), false);
        assert_eq!((v(&p, "hover"), v(&p, "speed")), (0.0, 0.0));
        // Over the right quarter of the rect (it spans 50..150).
        p.pointer(&m, &d, Some(Point::new(125.0, 50.0)), false);
        assert_eq!((v(&p, "hover"), v(&p, "press"), v(&p, "speed")), (1.0, 0.0, 0.75));
        p.pointer(&m, &d, Some(Point::new(125.0, 50.0)), true);
        assert_eq!(v(&p, "press"), 1.0);
        // Dragged off: still pressed, x follows the drag and stops at the edge.
        p.pointer(&m, &d, Some(Point::new(300.0, 50.0)), true);
        assert_eq!((v(&p, "hover"), v(&p, "press"), v(&p, "speed")), (0.0, 1.0, 1.0));
        // Released off the object: no click.
        p.pointer(&m, &d, Some(Point::new(300.0, 50.0)), false);
        assert_eq!((v(&p, "press"), v(&p, "jump"), v(&p, "on")), (0.0, 0.0, 0.0));
        // A click: the trigger fires and the flag flips; a second click flips it back.
        for _ in 0..2 {
            p.pointer(&m, &d, Some(Point::new(100.0, 50.0)), true);
            p.pointer(&m, &d, Some(Point::new(100.0, 50.0)), false);
        }
        assert_eq!((v(&p, "jump"), v(&p, "on")), (1.0, 0.0));
        assert_eq!(p.advance(&m, &d, 0.1), Some(2), "the click jumps");
        // A press that starts outside does not count.
        p.pointer(&m, &d, Some(Point::new(10.0, 10.0)), true);
        p.pointer(&m, &d, Some(Point::new(100.0, 50.0)), true);
        assert_eq!(v(&p, "press"), 0.0);
    }
}
