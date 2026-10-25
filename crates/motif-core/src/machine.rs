//! State machines: animations as states, switched by transitions whose conditions read inputs
//! that the game sets (a flag, a number, a one-shot trigger).

use crate::{Document, Prop};

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

#[derive(Clone, Debug, PartialEq)]
pub struct Machine {
    pub name: String,
    pub inputs: Vec<Input>,
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
}

impl MachinePlayer {
    pub fn new(m: &Machine) -> Self {
        MachinePlayer { state: 0, time: 0.0, prev: None, blend_time: 0.0, blend: 0.0, values: m.inputs.iter().map(|i| i.default).collect() }
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

    /// Jumps straight to `state`, without a blend.
    pub fn jump(&mut self, state: usize) {
        self.state = state;
        self.time = 0.0;
        self.prev = None;
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
        self.time += dt;
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
        }
        for (v, input) in self.values.iter_mut().zip(&m.inputs) {
            if input.kind == InputKind::Trigger {
                *v = 0.0;
            }
        }
        taken
    }

    /// How far the blend into the current state has got, from 0 to 1 (1 when there is none).
    pub fn blend_amount(&self) -> f64 {
        if self.prev.is_some() && self.blend > 0.0 { (self.blend_time / self.blend).clamp(0.0, 1.0) } else { 1.0 }
    }

    /// The document posed by the current state, mixed with the previous one during a blend.
    pub fn pose(&self, m: &Machine, doc: &Document) -> Document {
        let pose_of = |state: usize, t: f64| match m.states.get(state).and_then(|s| doc.animation(&s.anim)) {
            Some(a) => a.keys_at(doc, t),
            None => doc.clone(),
        };
        let mut out = pose_of(self.state, self.time);
        let w = self.blend_amount();
        let Some((prev, prev_t)) = self.prev.filter(|_| w < 1.0) else {
            out.rig();
            return out;
        };
        let old = pose_of(prev, prev_t);
        let mut props: Vec<(&str, Prop)> = Vec::new();
        for s in [prev, self.state] {
            if let Some(a) = m.states.get(s).and_then(|s| doc.animation(&s.anim)) {
                props.extend(a.tracks.iter().map(|t| (t.node.as_str(), t.prop)));
            }
        }
        props.sort_unstable();
        props.dedup();
        for (node, prop) in props {
            if let Some(i) = out.index_of(node) {
                let v = old.nodes[i].get(prop).lerp(out.nodes[i].get(prop), w);
                out.nodes[i].set(prop, v);
            }
        }
        out.rig();
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
}
