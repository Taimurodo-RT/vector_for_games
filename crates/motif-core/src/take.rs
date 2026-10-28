//! A take: a state machine run recorded as its inputs changed over time, so it can be played
//! again frame by frame at any rate (a video of the run) exactly as it went. Triggers are kept
//! as the moments they fired, the other inputs as the values they held.

use crate::{Document, InputKind, Machine, MachinePlayer};

/// A run of a machine: the moments its inputs changed, and how long it went on.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Take {
    /// The machine the take was recorded on.
    pub machine: String,
    pub moments: Vec<Moment>,
    /// Seconds from the start of the run to its end.
    pub length: f64,
}

/// The inputs from a moment of a run on, in the machine's order, and a jump to a state made by
/// hand then.
#[derive(Clone, Debug, PartialEq)]
pub struct Moment {
    pub time: f64,
    pub values: Vec<f64>,
    pub jump: Option<usize>,
}

impl Take {
    pub fn new(m: &Machine) -> Take {
        Take { machine: m.name.clone(), moments: Vec::new(), length: 0.0 }
    }

    /// Notes the inputs at `time` (and a jump made then); nothing is kept while they stay the same.
    pub fn record(&mut self, time: f64, values: &[f64], jump: Option<usize>) {
        if jump.is_none() && self.moments.last().is_some_and(|m| m.values == values) {
            return;
        }
        self.moments.push(Moment { time, values: values.to_vec(), jump });
        self.length = self.length.max(time);
    }

    /// A take written by hand: `"1 speed=1, 2.5 jump, 4 speed=0"` — at 1 s `speed` becomes 1, at
    /// 2.5 s the trigger `jump` fires, at 4 s `speed` is 0 again. Flags take `name` (on) or
    /// `name=0`. The run lasts `length` seconds, or a second past the last moment.
    pub fn parse(m: &Machine, text: &str, length: Option<f64>) -> Result<Take, String> {
        let mut values: Vec<f64> = m.inputs.iter().map(|i| i.default).collect();
        let mut take = Take::new(m);
        take.record(0.0, &values, None);
        let mut last = 0.0f64;
        for item in text.split([',', ';']).map(str::trim).filter(|s| !s.is_empty()) {
            let (time, rest) = item.split_once(char::is_whitespace).ok_or_else(|| format!("«{item}»: нужно «время вход=значение», например «1 speed=1»"))?;
            let time: f64 = time.trim_end_matches('s').trim_end_matches('с').parse().map_err(|_| format!("«{item}»: время в секундах, например 1.5"))?;
            if time < last {
                return Err(format!("«{item}»: моменты идут по порядку времени"));
            }
            for set in rest.split_whitespace() {
                let (name, value) = match set.split_once('=') {
                    Some((n, v)) => (n, Some(v.parse::<f64>().map_err(|_| format!("«{set}»: значение — число"))?)),
                    None => (set, None),
                };
                let i = m.input(name).ok_or_else(|| {
                    let names: Vec<&str> = m.inputs.iter().map(|i| i.name.as_str()).collect();
                    format!("у машины «{}» нет входа «{name}»; входы: {}", m.name, names.join(", "))
                })?;
                values[i] = match (m.inputs[i].kind, value) {
                    (InputKind::Number, None) => return Err(format!("«{set}»: у числа нужно значение, {name}=1")),
                    (_, Some(v)) => v,
                    (_, None) => 1.0,
                };
            }
            take.record(time, &values, None);
            // A trigger fires once: right after its moment it is off again.
            for (k, input) in m.inputs.iter().enumerate() {
                if input.kind == InputKind::Trigger {
                    values[k] = 0.0;
                }
            }
            take.record(time + 1e-6, &values, None);
            last = time;
        }
        take.length = length.unwrap_or(last + 1.0).max(0.0);
        Ok(take)
    }

    /// The run played again at `fps` frames a second: the posed scene of every frame, from the
    /// start to the end of the take.
    pub fn poses(&self, m: &Machine, doc: &Document, fps: f64) -> Vec<Document> {
        let mut player = MachinePlayer::new(m);
        let count = ((self.length * fps).round() as usize).max(1);
        let dt = 1.0 / fps;
        let mut next = 0;
        let mut out = Vec::with_capacity(count);
        for i in 0..count {
            let t = i as f64 * dt;
            if i > 0 {
                // Triggers fired since the last frame are seen by this step, the rest hold
                // their latest values.
                while next < self.moments.len() && self.moments[next].time < t - 1e-9 {
                    self.apply(m, &mut player, next);
                    next += 1;
                }
                player.advance(m, doc, dt);
            }
            while next < self.moments.len() && self.moments[next].time <= t + 1e-9 {
                self.apply(m, &mut player, next);
                next += 1;
            }
            out.push(player.pose(m, doc));
        }
        out
    }

    fn apply(&self, m: &Machine, player: &mut MachinePlayer, k: usize) {
        let moment = &self.moments[k];
        if let Some(s) = moment.jump.filter(|&s| s < m.states.len()) {
            player.jump(s);
        }
        player.values.resize(m.inputs.len(), 0.0);
        for (i, input) in m.inputs.iter().enumerate() {
            let v = moment.values.get(i).copied().unwrap_or(input.default);
            // A trigger stays fired until a step uses it up.
            if input.kind != InputKind::Trigger || v != 0.0 {
                player.values[i] = v;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Animation, Artboard, Cond, CondOp, Ease, Input, Keyframe, Node, Prop, ShapeKind, State, Track, Transition, Value};

    fn scene() -> (Machine, Document) {
        let mut doc = Document::new(Artboard::default());
        doc.nodes.push(Node::new("ball", ShapeKind::Ellipse));
        let key = |t, v| Keyframe { time: t, value: Value::Num(v), ease: Ease::Linear };
        let anim = |name: &str, x: f64| Animation {
            name: name.into(),
            duration: 1.0,
            looping: true,
            tracks: vec![Track { node: "ball".into(), prop: Prop::X, keys: vec![key(0.0, x), key(1.0, x)] }],
            events: Vec::new(),
        };
        doc.animations = vec![anim("Idle", 0.0), anim("Run", 100.0), anim("Jump", 200.0)];
        let m = Machine {
            name: "Hero".into(),
            inputs: vec![
                Input { name: "speed".into(), kind: InputKind::Number, default: 0.0 },
                Input { name: "jump".into(), kind: InputKind::Trigger, default: 0.0 },
            ],
            listeners: Vec::new(),
            states: ["Idle", "Run", "Jump"].iter().map(|s| State { name: s.to_string(), anim: s.to_string(), pos: None }).collect(),
            transitions: vec![
                Transition { from: Some(0), to: 1, conds: vec![Cond { input: "speed".into(), op: CondOp::Gt, value: 0.5 }], at_end: false, blend: 0.0 },
                Transition { from: None, to: 2, conds: vec![Cond { input: "jump".into(), op: CondOp::Is, value: 0.0 }], at_end: false, blend: 0.0 },
            ],
            any_pos: None,
        };
        (m, doc)
    }

    fn xs(take: &Take, m: &Machine, doc: &Document) -> Vec<f64> {
        take.poses(m, doc, 10.0).iter().map(|d| d.node("ball").unwrap().x).collect()
    }

    #[test]
    fn a_written_take_plays_its_moments() {
        let (m, doc) = scene();
        let take = Take::parse(&m, "0.3 speed=1, 0.65 jump", Some(1.0)).unwrap();
        assert_eq!(take.length, 1.0);
        let x = xs(&take, &m, &doc);
        assert_eq!(x.len(), 10);
        // Idle until speed rises at 0.3 s, Run from the next step, Jump after the trigger at 0.65 s.
        assert_eq!(x, vec![0.0, 0.0, 0.0, 0.0, 100.0, 100.0, 100.0, 200.0, 200.0, 200.0]);
        assert!(Take::parse(&m, "1 fly=1", None).unwrap_err().contains("speed, jump"));
        assert!(Take::parse(&m, "1 speed", None).is_err(), "a number needs a value");
        assert!(Take::parse(&m, "2 jump, 1 jump", None).is_err(), "in order of time");
        assert_eq!(Take::parse(&m, "2.5 jump", None).unwrap().length, 3.5);
    }

    #[test]
    fn a_recorded_trigger_is_not_lost_between_frames() {
        let (m, doc) = scene();
        let mut take = Take::new(&m);
        // Recorded at 60 frames a second: the trigger is on for one of them only.
        for i in 0..60 {
            let t = i as f64 / 60.0;
            let jump = if i == 23 { 1.0 } else { 0.0 };
            take.record(t, &[0.0, jump], None);
            take.length = t + 1.0 / 60.0;
        }
        assert_eq!(take.moments.len(), 3, "only changes are kept");
        let x = xs(&take, &m, &doc);
        assert_eq!(x[3], 0.0);
        assert_eq!(x[4], 200.0, "fired at 0.383 s, seen by the step to 0.4 s");
    }

    #[test]
    fn a_jump_by_hand_is_kept() {
        let (m, doc) = scene();
        let mut take = Take::new(&m);
        take.record(0.0, &[0.0, 0.0], None);
        take.record(0.5, &[0.0, 0.0], Some(1));
        take.length = 1.0;
        let x = xs(&take, &m, &doc);
        assert_eq!((x[4], x[5]), (0.0, 100.0));
    }
}
