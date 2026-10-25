//! Expands the statement tree into a `Document` and records where each property value came from.

use crate::parser::{MachineLine, MachineLineAst, NodeAst, ParamAst, Stmt, UseAst, ValueAst, ValueKind};
use crate::{AnimSite, Backdrop, Instance, KeySite, MachineSite, NodeSite, ParamSite, Scene, ScriptError, Source, Span, SymbolSite, TrackSite};
use motif_core::kurbo::Shape;
use motif_core::{Animation, Artboard, Color, Document, Ease, Keyframe, Node, Prop, ShapeKind, Track, Value};
use std::collections::HashMap;

#[derive(Clone, Copy, Debug)]
enum Var {
    Num(f64),
    Color(Color),
}

pub struct Evaluator<'a> {
    src: &'a str,
    /// Folder that font files named in the script are relative to.
    base: Option<std::path::PathBuf>,
    vars: HashMap<String, Var>,
    /// `symbol` blocks by name: their parameters and objects.
    symbols: HashMap<String, (bool, Vec<ParamAst>, Vec<Stmt>)>,
    /// Put before every id while a symbol is being copied: `coin1-`.
    prefix: String,
    /// How many symbols are being copied one inside another (to stop a symbol using itself).
    depth: usize,
    /// Ids named by `ik` and `skin`, checked once every object exists: node, id, where it is written.
    rig_refs: Vec<(usize, String, Span)>,
    /// Balls named by `on`, checked once every object exists.
    ball_refs: Vec<(usize, String, Span)>,
    /// Objects named by `tie` (any object) and ropes named by `hang` and `links` (`true`).
    rope_refs: Vec<(usize, String, Span, bool)>,
    morph_refs: Vec<(usize, String, Span)>,
    pub scene: Scene,
    pub errors: Vec<ScriptError>,
}

impl<'a> Evaluator<'a> {
    pub fn new(src: &'a str, base: Option<&std::path::Path>) -> Self {
        Evaluator {
            src,
            base: base.map(|b| b.to_path_buf()),
            vars: HashMap::new(),
            symbols: HashMap::new(),
            prefix: String::new(),
            depth: 0,
            rig_refs: Vec::new(),
            ball_refs: Vec::new(),
            rope_refs: Vec::new(),
            morph_refs: Vec::new(),
            scene: Scene {
                doc: Document::new(Artboard::default()),
                sources: HashMap::new(),
                sites: HashMap::new(),
                override_close: None,
                anims: HashMap::new(),
                blocks_start: None,
                override_entries: Vec::new(),
                path_sources: HashMap::new(),
                prop_spans: HashMap::new(),
                backdrop: None,
                symbols: Vec::new(),
                instances: HashMap::new(),
                machines: HashMap::new(),
                world: None,
            },
            errors: Vec::new(),
        }
    }

    fn err(&mut self, span: Span, msg: impl Into<String>) {
        self.errors.push(ScriptError::at(self.src, span.start, msg));
    }

    pub fn run(&mut self, stmts: &[Stmt]) {
        // Symbols can be used above the place they are written.
        for s in stmts {
            if let Stmt::Symbol { name, name_span, clip, params, body, span } = s {
                if self.symbols.contains_key(name) {
                    self.err(*name_span, format!("символ «{name}» уже есть"));
                    continue;
                }
                let mut defaults = Vec::new();
                for p in params {
                    if defaults.iter().any(|(n, _)| n == &p.name) {
                        self.err(p.name_span, format!("параметр «{}» уже есть", p.name));
                    }
                    if let Some(v) = self.param_value(&p.value) {
                        defaults.push((p.name.clone(), v));
                    }
                }
                self.symbols.insert(name.clone(), (*clip, params.clone(), body.clone()));
                self.scene.symbols.push(SymbolSite { name: name.clone(), clip: *clip, params: defaults, span: *span });
            }
        }
        // Animations and overrides refer to nodes, so they run after the whole tree is built.
        self.stmts(stmts, None, false);
        for s in stmts {
            if let Stmt::Override { entries, close, start } = s {
                self.note_block(*start);
                self.scene.override_close = Some(*close);
                for e in entries {
                    let Some(prop) = self.prop_path(&e.prop, e.span) else { continue };
                    let Some(index) = self.scene.doc.index_of(&e.node) else {
                        self.err(e.span, format!("нет объекта «{}»", e.node));
                        continue;
                    };
                    self.scene.override_entries.push((e.node.clone(), Span::new(e.span.start, e.value.span.end)));
                    let Some(value) = self.value(&e.value) else { continue };
                    self.scene.doc.nodes[index].set(prop, value);
                    self.scene
                        .sources
                        .insert((e.node.clone(), prop), Source { span: e.value.span, editable: e.value.is_literal(), paired: None, from_override: true });
                }
            }
        }
        self.check_rig();
        // The setup pose is the one skinned shapes were drawn in.
        self.scene.doc.bind_skins();
        self.scene.doc.rig();
        for s in stmts {
            if let Stmt::Anim { name, duration, looping, tracks, span, close, duration_span, loop_span } = s {
                self.note_block(span.start);
                if self.scene.doc.animation(name).is_some() {
                    self.err(*span, format!("анимация «{name}» уже есть"));
                    continue;
                }
                let mut anim = Animation { name: name.clone(), duration: *duration, looping: *looping, tracks: Vec::new() };
                let mut site = AnimSite { close: *close, duration: *duration_span, loop_word: *loop_span, tracks: Vec::new() };
                for t in tracks {
                    let world = t.node == "world" && self.scene.doc.index_of(&t.node).is_none();
                    if !world && self.scene.doc.index_of(&t.node).is_none() {
                        self.err(t.span, format!("нет объекта «{}»", t.node));
                        continue;
                    }
                    let Some(prop) = self.prop_path(&t.prop, t.span) else { continue };
                    if world != prop.is_world() {
                        let msg = if world {
                            "у мира ключи gravity-x, gravity-y, wind-x, wind-y и gust: world.wind-x 0: 0 | 2: 300".to_string()
                        } else {
                            format!("«{}» — свойство мира: world.{}", t.prop, t.prop)
                        };
                        self.err(t.span, msg);
                        continue;
                    }
                    let mut keys = Vec::new();
                    for k in &t.keys {
                        let ease = k.ease.unwrap_or(Ease::EaseInOut);
                        if let Some(value) = self.value(&k.value) {
                            keys.push(Keyframe { time: k.time, value, ease });
                        }
                    }
                    keys.sort_by(|a, b| a.time.total_cmp(&b.time));
                    anim.tracks.push(Track { node: t.node.clone(), prop, keys });
                    let keys: Vec<KeySite> =
                        t.keys.iter().map(|k| KeySite { time: k.time, span: k.span, value: k.value.span, time_span: k.time_span, ease: k.ease_span }).collect();
                    let end = keys.last().map_or(t.span.end, |k| k.span.end);
                    site.tracks.push(TrackSite { node: t.node.clone(), prop, span: Span::new(t.span.start, end), keys });
                }
                self.scene.doc.animations.push(anim);
                self.scene.anims.insert(name.clone(), site);
            }
        }
        // Machines play animations, so they come last.
        for s in stmts {
            if let Stmt::Machine { name, name_span, lines, span, close } = s {
                self.note_block(span.start);
                if self.scene.doc.machine(name).is_some() {
                    self.err(*name_span, format!("машина «{name}» уже есть"));
                    continue;
                }
                self.machine(name, lines, *span, *close);
            }
        }
    }

    /// IK targets and skin bones must exist; a target must not hang from the chain it moves.
    fn check_rig(&mut self) {
        let refs = std::mem::take(&mut self.rig_refs);
        let doc = &self.scene.doc;
        let mut errors = Vec::new();
        for (i, id, span) in refs {
            let Some(t) = doc.index_of(&id) else {
                errors.push((span, format!("нет объекта «{id}»")));
                continue;
            };
            let node = &doc.nodes[i];
            match &node.ik {
                Some(ik) if ik.target == id => {
                    let mut root = i;
                    for _ in 1..ik.chain.max(1) {
                        match doc.nodes[root].parent {
                            Some(p) if doc.nodes[p].kind == ShapeKind::Bone => root = p,
                            _ => break,
                        }
                    }
                    let mut up = Some(t);
                    while let Some(k) = up {
                        if k == root {
                            errors.push((span, format!("цель «{id}» внутри цепочки костей, которую она двигает: вынесите её из костей")));
                            break;
                        }
                        up = doc.nodes[k].parent;
                    }
                }
                _ if doc.nodes[t].kind != ShapeKind::Bone => errors.push((span, format!("«{id}» — не кость: skin перечисляет кости"))),
                _ => {}
            }
        }
        for (i, id, span) in std::mem::take(&mut self.ball_refs) {
            match doc.index_of(&id) {
                None => errors.push((span, format!("нет объекта «{id}»"))),
                Some(b) if b == i => errors.push((span, "объект не может лежать на самом себе".to_string())),
                Some(b) if matches!(doc.nodes[b].kind, ShapeKind::Group | ShapeKind::Bone) => {
                    errors.push((span, format!("«{id}» — не фигура: шаром для on служит ellipse или другая фигура")))
                }
                Some(_) => {}
            }
        }
        for (i, id, span, rope) in std::mem::take(&mut self.rope_refs) {
            match doc.index_of(&id) {
                None => errors.push((span, format!("нет объекта «{id}»"))),
                Some(r) if r == i => errors.push((span, "объект не может держаться сам за себя".to_string())),
                Some(r) if rope && doc.nodes[r].rope.as_ref().is_none_or(|r| r.cloth) => {
                    errors.push((span, format!("«{id}» — не верёвка: сделайте её верёвкой, rope")))
                }
                Some(_) => {}
            }
        }
        for (i, id, span) in std::mem::take(&mut self.morph_refs) {
            match doc.index_of(&id) {
                None => errors.push((span, format!("нет объекта «{id}»"))),
                Some(t) if t == i => errors.push((span, "фигура не может перетекать сама в себя".to_string())),
                Some(t) if matches!(doc.nodes[t].kind, ShapeKind::Group | ShapeKind::Bone) => {
                    errors.push((span, format!("«{id}» — не фигура: морфинг перетекает в контур другой фигуры")))
                }
                Some(_) => {}
            }
        }
        for (span, msg) in errors {
            self.err(span, msg);
        }
    }

    fn machine(&mut self, name: &str, lines: &[MachineLineAst], span: Span, close: usize) {
        use motif_core::{Cond, CondOp, Input, InputKind, Machine, State, Transition};
        let mut m = Machine { name: name.to_string(), inputs: Vec::new(), states: Vec::new(), transitions: Vec::new(), any_pos: None };
        let mut site = MachineSite { span, close, inputs: Vec::new(), states: Vec::new(), transitions: Vec::new(), any: None };
        // Inputs and states first: transitions may name them before they are written.
        for l in lines {
            match &l.line {
                MachineLine::Input { name, name_span, kind, default } => {
                    if m.input(name).is_some() {
                        self.err(*name_span, format!("вход «{name}» уже есть"));
                        continue;
                    }
                    m.inputs.push(Input { name: name.clone(), kind: *kind, default: *default });
                    site.inputs.push(l.span);
                }
                MachineLine::State { name, name_span, anim, at } => {
                    if m.state(name).is_some() || name == "any" {
                        self.err(*name_span, format!("состояние «{name}» уже есть"));
                        continue;
                    }
                    let (anim, anim_span) = anim.clone().unwrap_or((name.clone(), *name_span));
                    if self.scene.doc.animation(&anim).is_none() {
                        let hint = if anim == *name { format!(": напишите state {name} anim ИмяАнимации") } else { String::new() };
                        self.err(anim_span, format!("нет анимации «{anim}»{hint}"));
                        continue;
                    }
                    m.states.push(State { name: name.clone(), anim, pos: *at });
                    site.states.push(l.span);
                }
                MachineLine::AnyAt(x, y) => {
                    m.any_pos = Some((*x, *y));
                    site.any = Some(l.span);
                }
                MachineLine::Transition { .. } => {}
            }
        }
        for l in lines {
            let MachineLine::Transition { from, from_span, to, to_span, conds, at_end, blend } = &l.line else { continue };
            let from = match from {
                None => None,
                Some(f) => match m.state(f) {
                    Some(i) => Some(i),
                    None => {
                        self.err(*from_span, format!("нет состояния «{f}»"));
                        continue;
                    }
                },
            };
            let Some(to) = m.state(to) else {
                self.err(*to_span, format!("нет состояния «{to}»"));
                continue;
            };
            let mut ok = true;
            let mut out = Vec::new();
            for c in conds {
                let Some(input) = m.input(&c.input).map(|i| &m.inputs[i]) else {
                    self.err(c.span, format!("нет входа «{}»: добавьте строку input {} false", c.input, c.input));
                    ok = false;
                    continue;
                };
                let bad = match (input.kind, c.op) {
                    (InputKind::Number, CondOp::Is | CondOp::Not) => Some(format!("«{}» — число: сравните его, например {} > 0", c.input, c.input)),
                    (InputKind::Trigger, op) if op != CondOp::Is => Some(format!("«{}» — триггер: пишется просто when {}", c.input, c.input)),
                    _ => None,
                };
                if let Some(msg) = bad {
                    self.err(c.span, msg);
                    ok = false;
                }
                out.push(Cond { input: c.input.clone(), op: c.op, value: c.value });
            }
            if ok {
                m.transitions.push(Transition { from, to, conds: out, at_end: *at_end, blend: *blend });
                site.transitions.push(l.span);
            }
        }
        self.scene.doc.machines.push(m);
        self.scene.machines.insert(name.to_string(), site);
    }

    fn note_block(&mut self, start: usize) {
        self.scene.blocks_start = Some(self.scene.blocks_start.map_or(start, |s| s.min(start)));
    }

    fn prop_path(&mut self, name: &str, span: Span) -> Option<Prop> {
        let p = Prop::from_path_name(name);
        if p.is_none() {
            self.err(span, format!("неизвестное свойство «{name}»: x, y, w, h, rot, sx, sy, opacity, r, points, inner, fill, stroke, stroke_w"));
        }
        p
    }

    fn stmts(&mut self, stmts: &[Stmt], parent: Option<usize>, generated: bool) {
        for s in stmts {
            match s {
                Stmt::Artboard { name, width, height, props } => {
                    let ab = &mut self.scene.doc.artboard;
                    if let Some(n) = name {
                        ab.name = n.clone();
                    }
                    ab.width = *width;
                    ab.height = *height;
                    for p in props {
                        match p.key.as_str() {
                            "ui" => self.scene.doc.purpose = motif_core::Purpose::Ui,
                            "game" => self.scene.doc.purpose = motif_core::Purpose::Game,
                            _ => {}
                        }
                        let Some(v) = p.values.first() else { continue };
                        match (p.key.as_str(), self.value(v)) {
                            ("bg", Some(Value::Color(c))) => self.scene.doc.artboard.background = c,
                            ("light", Some(Value::Num(a))) => self.scene.doc.light = a,
                            ("light", _) => self.err(p.values[0].span, "light: угол в градусах, куда падают тени (90 — вниз)"),
                            _ => {}
                        }
                    }
                }
                Stmt::Let { name, value } => {
                    if let Some(v) = self.value(value) {
                        match v {
                            Value::Num(n) => self.vars.insert(name.clone(), Var::Num(n)),
                            Value::Color(c) => self.vars.insert(name.clone(), Var::Color(c)),
                            Value::None => None,
                        };
                    }
                }
                Stmt::World { props, span } => {
                    if parent.is_some() || self.scene.world.is_some() {
                        self.err(*span, "мир один на сцену и пишется на верхнем уровне: world gravity 0 900 wind 200 0");
                        continue;
                    }
                    self.scene.world = Some(*span);
                    let mut bad = Vec::new();
                    for p in props {
                        let nums: Vec<Option<f64>> = p
                            .values
                            .iter()
                            .map(|v| match self.value(v) {
                                Some(Value::Num(n)) => Some(n),
                                _ => None,
                            })
                            .collect();
                        let w = &mut self.scene.doc.world;
                        match (p.key.as_str(), nums.as_slice()) {
                            ("gravity", [Some(x), Some(y)]) => w.gravity = motif_core::kurbo::Vec2::new(*x, *y),
                            ("wind", [Some(x), Some(y)]) => w.wind = motif_core::kurbo::Vec2::new(*x, *y),
                            ("gust", [Some(g)]) if (0.0..=1.0).contains(g) => w.gust = *g,
                            ("gust", _) => bad.push((p.key_span, "gust — порывистость ветра от 0 (ровный) до 1: gust 0.4")),
                            _ => bad.push((p.key_span, "gravity и wind — два числа, X и Y: gravity 0 900, wind 200 0")),
                        }
                    }
                    for (span, msg) in bad {
                        self.err(span, msg);
                    }
                }
                Stmt::Backdrop { file, props, span } => {
                    if parent.is_some() || self.scene.backdrop.is_some() {
                        self.err(*span, "подложка одна на сцену и пишется на верхнем уровне");
                        continue;
                    }
                    let mut b = Backdrop { file: file.clone(), x: 0.0, y: 0.0, scale: 1.0, opacity: 0.5, hidden: false, span: *span };
                    for p in props {
                        let nums: Vec<f64> = p
                            .values
                            .iter()
                            .filter_map(|v| match self.value(v) {
                                Some(Value::Num(n)) => Some(n),
                                _ => None,
                            })
                            .collect();
                        if nums.len() != p.values.len() {
                            self.err(p.key_span, format!("«{}» у подложки ждёт числа", p.key));
                            continue;
                        }
                        match p.key.as_str() {
                            "at" => (b.x, b.y) = (nums[0], nums[1]),
                            "scale" => b.scale = nums[0].max(0.001),
                            "opacity" => b.opacity = nums[0].clamp(0.0, 1.0),
                            _ => b.hidden = true,
                        }
                    }
                    self.scene.backdrop = Some(b);
                }
                Stmt::Node(n) => {
                    self.node(n, parent, generated);
                }
                Stmt::Symbol { name_span, .. } => {
                    if parent.is_some() {
                        self.err(*name_span, "symbol пишется на верхнем уровне, не внутри group, for или другого символа");
                    }
                }
                Stmt::Use(u) => self.use_symbol(u, parent, generated),
                Stmt::For { var, from, to, body } => {
                    let (Some(Value::Num(a)), Some(Value::Num(b))) = (self.value(from), self.value(to)) else {
                        self.err(from.span, "границы цикла должны быть числами");
                        continue;
                    };
                    let (a, b) = (a.round() as i64, b.round() as i64);
                    if b - a > 10_000 {
                        self.err(from.span, "слишком длинный цикл (больше 10 000 повторов)");
                        continue;
                    }
                    let saved = self.vars.get(var).copied();
                    for i in a..b {
                        self.vars.insert(var.clone(), Var::Num(i as f64));
                        self.stmts(body, parent, true);
                    }
                    match saved {
                        Some(v) => self.vars.insert(var.clone(), v),
                        None => self.vars.remove(var),
                    };
                }
                Stmt::Anim { .. } | Stmt::Override { .. } | Stmt::Machine { .. } => {
                    if parent.is_some() {
                        self.err(Span::new(0, 0), "anim, machine и override пишутся на верхнем уровне");
                    }
                }
            }
        }
    }

    /// A number or colour for a symbol parameter.
    fn param_value(&mut self, v: &ValueAst) -> Option<Value> {
        match self.value(v)? {
            Value::None => {
                self.err(v.span, "параметр символа — число или цвет");
                None
            }
            v => Some(v),
        }
    }

    fn use_symbol(&mut self, u: &UseAst, parent: Option<usize>, generated: bool) {
        let Some((clip, params, body)) = self.symbols.get(&u.symbol).cloned() else {
            let known: Vec<&str> = self.scene.symbols.iter().map(|s| s.name.as_str()).collect();
            let hint = if known.is_empty() { "в сцене нет ни одного symbol".to_string() } else { format!("есть: {}", known.join(", ")) };
            self.err(u.symbol_span, format!("нет символа «{}»: {hint}", u.symbol));
            return;
        };
        if self.depth >= 16 {
            self.err(u.symbol_span, format!("символ «{}» вставлен сам в себя", u.symbol));
            return;
        }
        // Defaults, then the values written on the `use` line.
        let mut sites: Vec<ParamSite> = Vec::new();
        for p in &params {
            if let Some(value) = self.param_value(&p.value) {
                sites.push(ParamSite { name: p.name.clone(), value, span: None, editable: false });
            }
        }
        for p in &u.params {
            let Some(site) = sites.iter().position(|s| s.name == p.name) else {
                let names: Vec<&str> = params.iter().map(|p| p.name.as_str()).collect();
                let hint = if names.is_empty() { "у него нет параметров".to_string() } else { format!("есть: {}", names.join(", ")) };
                self.err(p.name_span, format!("у символа «{}» нет параметра «{}»: {hint}", u.symbol, p.name));
                continue;
            };
            if let Some(value) = self.param_value(&p.value) {
                sites[site] = ParamSite { name: p.name.clone(), value, span: Some(p.value.span), editable: p.value.is_literal() && !generated };
            }
        }
        let Some(index) = self.node(&u.node, parent, generated) else { return };
        let id = self.scene.doc.nodes[index].id.clone();
        self.scene.doc.nodes[index].clip |= clip;
        let saved_vars = self.vars.clone();
        let saved_prefix = std::mem::replace(&mut self.prefix, format!("{id}-"));
        for p in &sites {
            let var = match p.value {
                Value::Num(n) => Var::Num(n),
                Value::Color(c) => Var::Color(c),
                Value::None => continue,
            };
            self.vars.insert(p.name.clone(), var);
        }
        self.scene.instances.insert(id, Instance { symbol: u.symbol.clone(), params: sites });
        self.depth += 1;
        self.stmts(&body, Some(index), true);
        self.depth -= 1;
        self.prefix = saved_prefix;
        self.vars = saved_vars;
    }

    fn node(&mut self, n: &NodeAst, parent: Option<usize>, generated: bool) -> Option<usize> {
        let id = self.template(&n.id, n.id_span)?;
        let id = format!("{}{id}", self.prefix);
        if self.scene.doc.index_of(&id).is_some() {
            self.err(n.id_span, format!("id «{id}» уже занят"));
            return None;
        }
        let mut node = Node::new(id.clone(), n.kind);
        node.name = n.name.clone();
        node.parent = parent;
        let record = |scene: &mut Scene, prop: Prop, v: &ValueAst, paired: Option<Prop>| {
            scene.sources.insert((id.clone(), prop), Source { span: v.span, editable: v.is_literal() && !generated, paired, from_override: false });
        };
        let mut content = String::new();
        let mut font: Option<String> = None;
        let mut ik: Option<(String, Span)> = None;
        let (mut chain, mut flip) = (2, false);
        let mut skin: Option<Vec<(String, Span)>> = None;
        let mut sharp = motif_core::Skin::SHARP;
        let mut soft: Option<motif_core::Soft> = None;
        let mut on: Option<(String, Span)> = None;
        let mut gravity: Option<f64> = None;
        let mut gravity_span: Option<Span> = None;
        let mut range: Option<(f64, Span)> = None;
        let mut cone: Option<(f64, Span)> = None;
        let mut emit: Option<motif_core::Emitter> = None;
        let (mut has_emit, mut emit_only) = (false, None);
        let mut rope: Option<motif_core::Rope> = None;
        let (mut pins, mut wind, mut rope_only): (Option<Vec<motif_core::Pin>>, Option<f64>, Option<Span>) = (None, None, None);
        let mut ties: Vec<(String, Span)> = Vec::new();
        let mut hang: Option<(String, f64, Span)> = None;
        let mut links: Option<(String, Span)> = None;
        let mut morph: Option<Vec<(String, Span)>> = None;
        let mut fluid: Option<motif_core::Fluid> = None;
        let mut flow: Option<(f64, Span)> = None;
        let mut roll: Option<(motif_core::Roll, Span)> = None;
        for p in &n.props {
            if !generated {
                let end = p.values.last().map_or(p.key_span.end, |v| v.span.end);
                self.scene.prop_spans.insert((id.clone(), p.key.clone()), Span::new(p.key_span.start, end));
            }
            let vals: Vec<Option<Value>> = match p.key.as_str() {
                "d" | "content" | "font" | "align" | "ik" | "skin" | "on" | "pin" | "tie" | "hang" | "links" | "morph" => Vec::new(),
                // The first value of a gradient is its kind word, not a name.
                "gradient" => std::iter::once(None).chain(p.values.iter().skip(1).map(|v| self.value(v))).collect(),
                // Keywords, not names: `cap round`, `blend multiply`.
                "cap" | "join" | "blend" => Vec::new(),
                // `cross` is a keyword of the hatching.
                "hatch" => p.values.iter().map(|v| if matches!(&v.kind, ValueKind::Ref(t) if t == "cross") { None } else { self.value(v) }).collect(),
                _ => p.values.iter().map(|v| self.value(v)).collect(),
            };
            let num = |i: usize| vals.get(i).copied().flatten().and_then(Value::as_num);
            let bad = |ev: &mut Self| ev.err(p.key_span, format!("неверное значение для «{}»", p.key));
            match p.key.as_str() {
                "at" => match (num(0), num(1)) {
                    (Some(x), Some(y)) => {
                        node.x = x;
                        node.y = y;
                        record(&mut self.scene, Prop::X, &p.values[0], None);
                        record(&mut self.scene, Prop::Y, &p.values[1], None);
                    }
                    _ => bad(self),
                },
                "r" if p.values.len() > 1 => {
                    let r: Vec<f64> = (0..p.values.len()).filter_map(num).collect();
                    if r.len() != p.values.len() {
                        bad(self);
                        continue;
                    }
                    // CSS border-radius order: top left, top right, bottom right, bottom left.
                    let radii = match r[..] {
                        [a, b] => [a, b, a, b],
                        [a, b, c] => [a, b, c, b],
                        [a, b, c, d, ..] => [a, b, c, d],
                        _ => [r[0]; 4],
                    };
                    node.radius = radii[0];
                    node.radii = Some(radii);
                }
                "skew" => match (num(0), num(1)) {
                    (Some(a), b) if p.values.len() == 1 || b.is_some() => {
                        node.skew_x = a;
                        node.skew_y = b.unwrap_or(0.0);
                        if p.values.len() == 2 {
                            record(&mut self.scene, Prop::SkewX, &p.values[0], None);
                            record(&mut self.scene, Prop::SkewY, &p.values[1], None);
                        } else {
                            // One value is skewX; editing skewY rewrites both.
                            record(&mut self.scene, Prop::SkewX, &p.values[0], Some(Prop::SkewY));
                            record(&mut self.scene, Prop::SkewY, &p.values[0], Some(Prop::SkewX));
                        }
                    }
                    _ => bad(self),
                },
                "pivot" => match (num(0), num(1)) {
                    (Some(x), Some(y)) => node.pivot = motif_core::kurbo::Vec2::new(x, y),
                    _ => bad(self),
                },
                "dash" => {
                    let d: Vec<f64> = (0..p.values.len()).filter_map(num).collect();
                    if d.len() != p.values.len() || d.iter().any(|v| *v < 0.0) {
                        bad(self);
                        continue;
                    }
                    // `dash 0` is a solid line.
                    node.dash = if d.iter().all(|v| *v == 0.0) { Vec::new() } else { d };
                }
                "cap" | "join" | "blend" => {
                    let word = match &p.values[0].kind {
                        ValueKind::Ref(w) => w.as_str(),
                        ValueKind::None => "none",
                        _ => "",
                    };
                    let ok = match p.key.as_str() {
                        "cap" => motif_core::Cap::from_keyword(word).map(|c| node.cap = c).is_some(),
                        "join" => motif_core::Join::from_keyword(word).map(|j| node.join = j).is_some(),
                        _ => motif_core::Blend::from_keyword(word).map(|b| node.blend = b).is_some(),
                    };
                    if !ok {
                        let choices = match p.key.as_str() {
                            "cap" => "butt, round, square".to_string(),
                            "join" => "miter, round, bevel".to_string(),
                            _ => motif_core::Blend::ALL.map(|b| b.keyword()).join(", "),
                        };
                        self.err(p.values[0].span, format!("«{}»: ожидалось одно из: {choices}", p.key));
                    }
                }
                "shadow" => {
                    let color = vals.last().copied().flatten().and_then(|v| if let Value::Color(c) = v { Some(c) } else { None });
                    let nums: Vec<f64> = (0..p.values.len() - 1).filter_map(num).collect();
                    match (color, nums.len() == p.values.len() - 1) {
                        (Some(color), true) => {
                            node.shadow = Some(motif_core::Shadow { dx: nums[0], dy: nums[1], blur: nums.get(2).copied().unwrap_or(0.0).max(0.0), color });
                            record(&mut self.scene, Prop::ShadowX, &p.values[0], None);
                            record(&mut self.scene, Prop::ShadowY, &p.values[1], None);
                            if nums.len() == 3 {
                                record(&mut self.scene, Prop::ShadowBlur, &p.values[2], None);
                            }
                        }
                        _ => self.err(p.key_span, "тень: shadow x y [размытие] #цвет, например shadow 0 4 8 #00000055"),
                    }
                }
                "size" | "scale" => {
                    let (pa, pb) = if p.key == "size" { (Prop::Width, Prop::Height) } else { (Prop::ScaleX, Prop::ScaleY) };
                    match (num(0), num(1)) {
                        (Some(a), Some(b)) => {
                            node.set(pa, Value::Num(a));
                            node.set(pb, Value::Num(b));
                            record(&mut self.scene, pa, &p.values[0], None);
                            record(&mut self.scene, pb, &p.values[1], None);
                        }
                        (Some(a), None) if p.values.len() == 1 => {
                            node.set(pa, Value::Num(a));
                            node.set(pb, Value::Num(a));
                            record(&mut self.scene, pa, &p.values[0], Some(pb));
                            record(&mut self.scene, pb, &p.values[0], Some(pa));
                        }
                        _ => bad(self),
                    }
                }
                "stroke" => {
                    match vals[0] {
                        Some(Value::Color(c)) => node.stroke = Some(c),
                        Some(Value::None) => node.stroke = None,
                        _ => {
                            bad(self);
                            continue;
                        }
                    }
                    record(&mut self.scene, Prop::StrokeColor, &p.values[0], None);
                    node.stroke_width = 2.0;
                    if p.values.len() > 1 {
                        match num(1) {
                            Some(w) => {
                                node.stroke_width = w;
                                record(&mut self.scene, Prop::StrokeWidth, &p.values[1], None);
                            }
                            None => bad(self),
                        }
                    }
                }
                "fill" => match vals[0] {
                    Some(v @ (Value::Color(_) | Value::None)) => {
                        node.set(Prop::Fill, v);
                        record(&mut self.scene, Prop::Fill, &p.values[0], None);
                    }
                    _ => bad(self),
                },
                "len" | "ik" | "chain" | "flip" if n.kind != ShapeKind::Bone => {
                    self.err(p.key_span, format!("«{}» бывает только у bone", p.key));
                }
                "len" => match num(0) {
                    Some(v) => {
                        node.width = v;
                        record(&mut self.scene, Prop::Width, &p.values[0], None);
                    }
                    None => bad(self),
                },
                "ik" => {
                    if let ValueKind::Path(t) = &p.values[0].kind {
                        ik = Some((t.clone(), p.values[0].span));
                    }
                }
                "chain" => match num(0) {
                    Some(v) if v >= 1.0 && v.fract() == 0.0 => chain = v as usize,
                    _ => self.err(p.key_span, "chain — сколько костей гнёт IK, целое от 1: chain 2"),
                },
                "flip" => flip = true,
                "skin" | "skin-sharp" if n.kind.is_container() => {
                    self.err(p.key_span, "кости гнут фигуры: skin бывает у rect, ellipse, path и других фигур, но не у group и bone");
                }
                "skin" => {
                    skin = Some(p.values.iter().filter_map(|v| if let ValueKind::Path(b) = &v.kind { Some((b.clone(), v.span)) } else { None }).collect())
                }
                "skin-sharp" => match num(0) {
                    Some(v) if v > 0.0 => sharp = v,
                    _ => bad(self),
                },
                "gravity" => match num(0) {
                    Some(v) => {
                        gravity = Some(v);
                        gravity_span = Some(p.key_span);
                    }
                    None => self.err(p.key_span, "«gravity»: gravity — ускорение вниз в единицах артборда за секунду²: gravity 900"),
                },
                "emit" | "burst" | "life" | "speed" | "angle" | "spread" | "drag" | "fade" | "grow" | "spin" if n.kind.is_container() => {
                    self.err(p.key_span, format!("«{}» бывает у фигур: источником частиц становится фигура", p.key));
                }
                "emit" | "burst" | "life" | "speed" | "angle" | "spread" | "drag" | "fade" | "grow" | "spin" => {
                    let e = emit.get_or_insert_with(motif_core::Emitter::default);
                    if p.key != "emit" {
                        emit_only = emit_only.or(Some(p.key_span));
                    }
                    let v = if p.key == "emit" && p.values.is_empty() { Some(e.rate) } else { num(0) };
                    let ok = match (p.key.as_str(), v) {
                        ("emit", Some(v)) if v >= 0.0 => {
                            e.rate = v;
                            has_emit = true;
                            true
                        }
                        ("burst", Some(v)) if v >= 0.0 => {
                            e.burst = v;
                            true
                        }
                        ("life", Some(v)) if v > 0.0 => {
                            e.life = v;
                            true
                        }
                        ("speed", Some(v)) => {
                            e.speed = v;
                            true
                        }
                        ("angle", Some(v)) => {
                            e.angle = v;
                            true
                        }
                        ("spread", Some(v)) if v >= 0.0 => {
                            e.spread = v;
                            true
                        }
                        ("drag", Some(v)) if (0.0..=1.0).contains(&v) => {
                            e.drag = v;
                            true
                        }
                        ("fade", Some(v)) if (0.0..=1.0).contains(&v) => {
                            e.fade = v;
                            true
                        }
                        ("grow", Some(v)) if v >= 0.0 => {
                            e.grow = v;
                            true
                        }
                        ("spin", Some(v)) => {
                            e.spin = v;
                            true
                        }
                        _ => false,
                    };
                    if !ok {
                        let hint = match p.key.as_str() {
                            "emit" => "emit — частиц в секунду, 0 и больше: emit 30",
                            "burst" => "burst — сколько частиц сразу при старте: burst 20",
                            "life" => "life — сколько секунд живёт частица, больше 0: life 1.2",
                            "spread" => "spread — ширина конуса вылета в градусах: spread 40",
                            "drag" | "fade" => "значение от 0 до 1",
                            "grow" => "grow — размер в конце жизни относительно начала, 0 и больше: grow 0.2",
                            _ => "нужно число",
                        };
                        self.err(p.key_span, format!("«{}»: {hint}", p.key));
                    }
                }
                "soft" | "pressure" | "friction" | "damp" | "drive" if n.kind.is_container() => {
                    self.err(p.key_span, format!("«{}» бывает у фигур: мягким телом становится контур", p.key));
                }
                "soft" | "pressure" | "friction" | "damp" | "drive" => {
                    let s = soft.get_or_insert_with(motif_core::Soft::default);
                    let v = num(0);
                    let unit = |v: Option<f64>| v.filter(|v| (0.0..=1.0).contains(v));
                    let ok = match p.key.as_str() {
                        "soft" if p.values.is_empty() => true,
                        "soft" => unit(v).map(|v| s.stiff = v).is_some(),
                        "friction" => unit(v).map(|v| s.friction = v).is_some(),
                        "damp" => unit(v).map(|v| s.damp = v).is_some(),
                        "drive" => unit(v).map(|v| s.drive = v).is_some(),
                        "pressure" => v.filter(|v| *v >= 0.0).map(|v| s.pressure = v).is_some(),
                        _ => false,
                    };
                    if !ok {
                        let hint = match p.key.as_str() {
                            "pressure" => "pressure — объём относительно нарисованного, 0 и больше: pressure 1",
                            _ => "значение от 0 до 1",
                        };
                        self.err(p.key_span, format!("«{}»: {hint}", p.key));
                    }
                }
                "jiggle" if n.kind != ShapeKind::Bone => {
                    self.err(p.key_span, "«jiggle» бывает только у bone: кость покачивается вслед за родительской");
                }
                "spring" | "jiggle" => {
                    let d = motif_core::Spring::default();
                    let freq = if p.values.is_empty() { Some(d.freq) } else { num(0) };
                    let damp = if p.values.len() < 2 { Some(d.damp) } else { num(1) };
                    match (freq, damp) {
                        (Some(freq), Some(damp)) if freq > 0.0 && freq <= motif_core::Spring::MAX_FREQ && damp >= 0.0 => {
                            let s = Some(motif_core::Spring { freq, damp });
                            if p.key == "spring" { node.spring = s } else { node.jiggle = s }
                        }
                        _ => self.err(
                            p.key_span,
                            format!("«{}» — частота в колебаниях за секунду (больше 0, до 30) и затухание от 0 (1 — без перелёта): {} 3 0.3", p.key, p.key),
                        ),
                    }
                }
                "squash" => {
                    let d = motif_core::Squash::default();
                    let at = |i: usize, def: f64| if p.values.len() <= i { Some(def) } else { num(i) };
                    match (at(0, d.amount), at(1, d.spring.freq), at(2, d.spring.damp)) {
                        (Some(amount), Some(freq), Some(damp)) if amount >= 0.0 && freq > 0.0 && freq <= motif_core::Spring::MAX_FREQ && damp >= 0.0 => {
                            node.squash = Some(motif_core::Squash { amount, spring: motif_core::Spring { freq, damp } });
                        }
                        _ => {
                            self.err(p.key_span, "«squash» — сила (0 и больше, 1 — вдвое длиннее на 1000 ед/с), частота (до 30) и затухание: squash 0.5 4 0.3")
                        }
                    }
                }
                "on" => match &p.values[0].kind {
                    ValueKind::Path(t) => on = Some((t.clone(), p.values[0].span)),
                    _ => self.err(p.key_span, "on — на каком шаре лежит объект: on planet"),
                },
                "wrap" => node.wrap = true,
                "rope" | "cloth" if n.kind.is_container() => self.err(p.key_span, format!("«{}» бывает у фигур: верёвкой или тканью становится контур", p.key)),
                "rope" | "cloth" => {
                    let mut r = if p.key == "cloth" { motif_core::Rope::cloth() } else { motif_core::Rope::default() };
                    let at = |i: usize, def: f64| if p.values.len() <= i { Some(def) } else { num(i) };
                    match (at(0, r.stiff), at(1, r.damp)) {
                        (Some(stiff), Some(damp)) if (0.0..=1.0).contains(&stiff) && (0.0..=1.0).contains(&damp) => {
                            (r.stiff, r.damp) = (stiff, damp);
                            rope = Some(r);
                        }
                        _ => self.err(p.key_span, format!("«{}» — жёсткость от 0 до 1 (1 не тянется) и затухание от 0 до 1: {} 1 0.02", p.key, p.key)),
                    }
                }
                "pin" => {
                    rope_only = rope_only.or(Some(p.key_span));
                    let mut list = Vec::new();
                    for v in &p.values {
                        match &v.kind {
                            ValueKind::None => {}
                            ValueKind::Ref(w) => match motif_core::Pin::from_word(w) {
                                Some(pin) => list.push(pin),
                                None => self.err(
                                    v.span,
                                    "pin: у верёвки start, end; у ткани top, bottom, left, right, top-left, top-right, bottom-left, bottom-right; none — ничего",
                                ),
                            },
                            _ => self.err(v.span, "pin — где закреплено: pin start end, pin top, pin none"),
                        }
                    }
                    pins = Some(list);
                }
                "wind" => {
                    rope_only = rope_only.or(Some(p.key_span));
                    match num(0) {
                        Some(v) => wind = Some(v),
                        None => self.err(p.key_span, "«wind»: сила ветра вправо (меньше нуля — влево): wind 300"),
                    }
                }
                "tie" => {
                    rope_only = rope_only.or(Some(p.key_span));
                    ties = p.values.iter().filter_map(|v| if let ValueKind::Path(t) = &v.kind { Some((t.clone(), v.span)) } else { None }).collect();
                }
                "links" if n.kind.is_container() => self.err(p.key_span, "«links» бывает у фигур: звеном цепи служит фигура"),
                "hang" => {
                    let f = match p.values.get(1) {
                        None => Some(1.0),
                        Some(v) => self.value(v).and_then(|v| v.as_num()).filter(|f| (0.0..=1.0).contains(f)),
                    };
                    match (&p.values[0].kind, f) {
                        (ValueKind::Path(t), Some(f)) => hang = Some((t.clone(), f, p.values[0].span)),
                        _ => self.err(p.key_span, "hang — на какой верёвке висит объект и где: 0 начало, 1 конец: hang rope 1"),
                    }
                }
                "links" => {
                    if let ValueKind::Path(t) = &p.values[0].kind {
                        links = Some((t.clone(), p.values[0].span));
                    }
                }
                "extrude" if n.kind.is_container() => self.err(p.key_span, "«extrude» бывает у фигур: в глубину выдавливается контур"),
                "extrude" => match (num(0), if p.values.len() > 1 { num(1) } else { Some(1.0) }) {
                    (Some(d), Some(t)) if d != 0.0 && t >= 0.0 => {
                        node.body = Some(motif_core::Body { depth: d, taper: t });
                        record(&mut self.scene, Prop::Depth, &p.values[0], None);
                        if let Some(v) = p.values.get(1) {
                            record(&mut self.scene, Prop::Taper, v, None);
                        }
                    }
                    _ => self.err(p.key_span, "extrude — глубина и сужение задней грани (1 — призма, 0 — конус): extrude 60, extrude 120 0"),
                },
                "morph" if n.kind.is_container() => self.err(p.key_span, "«morph» бывает у фигур: перетекает контур"),
                "morph" => {
                    let ids: Vec<_> = p.values.iter().filter_map(|v| if let ValueKind::Path(t) = &v.kind { Some((t.clone(), v.span)) } else { None }).collect();
                    let amount = p.values.get(ids.len());
                    match amount.map(|v| self.value(v).and_then(|v| v.as_num())) {
                        Some(None) => self.err(p.key_span, "morph — в какие фигуры перетекает и насколько: morph star heart 0.5"),
                        Some(Some(a)) if a < 0.0 => self.err(p.key_span, "«morph»: насколько перетекла фигура, от 0 — morph star 0.5"),
                        a => {
                            let mut m = motif_core::Morph::new(Vec::new());
                            m.amount = a.flatten().unwrap_or(0.0);
                            node.morph = Some(m);
                            if let Some(v) = amount {
                                record(&mut self.scene, Prop::Morph, v, None);
                            }
                            morph = Some(ids);
                        }
                    }
                }
                "fluid" | "flow" if n.kind.is_container() => self.err(p.key_span, format!("«{}» бывает у фигур: жидкость заполняет контур", p.key)),
                "fluid" => {
                    let mut f = motif_core::Fluid::default();
                    match if p.values.is_empty() { Some(f.visc) } else { num(0) } {
                        Some(v) if (0.0..=1.0).contains(&v) => {
                            f.visc = v;
                            fluid = Some(f);
                        }
                        _ => self.err(p.key_span, "«fluid» — вязкость от 0 (вода) до 1 (почти не течёт): fluid 0.1"),
                    }
                }
                "flow" => match num(0) {
                    Some(v) if v > 0.0 => flow = Some((v, p.key_span)),
                    _ => self.err(p.key_span, "«flow» — капель в секунду из фигуры, больше 0: flow 60"),
                },
                "roll" => {
                    let d = motif_core::Roll::default();
                    let at = |i: usize, def: f64| if p.values.len() <= i { Some(def) } else { num(i) };
                    match (at(0, d.friction), at(1, d.damp)) {
                        (Some(friction), Some(damp)) if (0.0..=1.0).contains(&friction) && (0.0..=1.0).contains(&damp) => {
                            roll = Some((motif_core::Roll { friction, damp, ..d }, p.key_span));
                        }
                        _ => self.err(p.key_span, "«roll» — сцепление с шаром от 0 до 1 (1 — вращается вместе с ним) и затухание от 0 до 1: roll 0.3 0.05"),
                    }
                }
                "pull" | "fan" => match num(0) {
                    Some(v) => {
                        let f = if p.key == "pull" { &mut node.pull } else { &mut node.fan };
                        let d = if p.key == "pull" { motif_core::Field::pull() } else { motif_core::Field::fan() };
                        f.get_or_insert(d).strength = v;
                    }
                    None => self.err(p.key_span, format!("«{}» — сила, ускорение в единицах артборда за секунду²: {} 2000", p.key, p.key)),
                },
                "range" => match num(0) {
                    Some(v) if v > 0.0 => range = Some((v, p.key_span)),
                    _ => self.err(p.key_span, "«range» — докуда достаёт притяжение или вентилятор, больше 0: range 300"),
                },
                "cone" => match num(0) {
                    Some(v) if v > 0.0 && v <= 360.0 => cone = Some((v, p.key_span)),
                    _ => self.err(p.key_span, "«cone» — ширина струи вентилятора в градусах, до 360: cone 40"),
                },
                "weight" => match num(0) {
                    Some(v) => node.weight = Some(v),
                    None => self.err(p.key_span, "«weight» — вес для гравитации сцены: 1 обычный, 0 невесомый, 2 вдвое тяжелее"),
                },
                "sail" => match num(0) {
                    Some(v) if v >= 0.0 => node.sail = Some(v),
                    _ => self.err(p.key_span, "«sail» — сколько ветра ловит объект: 1 обычно, 0 не замечает ветра"),
                },
                "solid" => node.solid = true,
                "ui" => node.ui = true,
                "locked" => node.locked = true,
                "rough" | "boil" | "hatch" | "grain" | "bands" | "seed" if n.kind.is_container() => {
                    self.err(p.key_span, format!("«{}» бывает у фигур: кисть рисует контур", p.key))
                }
                "rough" => match (num(0), if p.values.len() > 1 { num(1) } else { Some(0.0) }) {
                    (Some(a), Some(w)) if a >= 0.0 && w >= 0.0 => {
                        node.brush.rough = a;
                        node.brush.wave = w;
                        record(&mut self.scene, Prop::Rough, &p.values[0], None);
                    }
                    _ => self.err(p.key_span, "rough — насколько контур гуляет от руки, и длина одной волны: rough 3, rough 3 40"),
                },
                "boil" => match num(0) {
                    Some(f) if f >= 0.0 => node.brush.boil = f,
                    _ => self.err(p.key_span, "boil — сколько раз в секунду линия перерисовывается заново: boil 8"),
                },
                "seed" => match num(0) {
                    Some(s) if s >= 0.0 && s.fract() == 0.0 => node.brush.seed = Some(s as u32),
                    _ => self.err(p.key_span, "seed — целое число от 0: другой seed — другой рисунок той же кисти"),
                },
                "bands" => match num(0) {
                    Some(b) if (1.0..=64.0).contains(&b) && b.fract() == 0.0 => node.brush.bands = b as u32,
                    _ => self.err(p.key_span, "bands — сколько ступеней у света и градиентов, от 1 до 64: bands 3"),
                },
                "hatch" => {
                    let cross = p.values.iter().any(|v| matches!(&v.kind, ValueKind::Ref(t) if t == "cross"));
                    let color = vals.iter().flatten().find_map(|v| if let Value::Color(c) = v { Some(*c) } else { None });
                    let nums: Vec<f64> = vals.iter().flatten().filter_map(|v| v.as_num()).collect();
                    let rest = p.values.len() - usize::from(cross) - usize::from(color.is_some());
                    match nums[..] {
                        [angle, gap, ..] if rest == nums.len() && nums.len() <= 3 && gap > 0.0 && nums.get(2).is_none_or(|w| *w > 0.0) => {
                            node.brush.hatch = Some(motif_core::Hatch { angle, gap, width: nums.get(2).copied().unwrap_or(1.0), color, cross });
                        }
                        _ => self.err(p.key_span, "hatch — штриховка: угол, шаг, толщина, цвет и cross для второго слоя: hatch 45 6, hatch 30 5 1.5 #2a2e39 cross"),
                    }
                }
                "grain" => {
                    let color = vals.iter().flatten().find_map(|v| if let Value::Color(c) = v { Some(*c) } else { None });
                    let nums: Vec<f64> = vals.iter().flatten().filter_map(|v| v.as_num()).collect();
                    match nums[..] {
                        [d, ..] if nums.len() + usize::from(color.is_some()) == p.values.len() && nums.len() <= 2 && d > 0.0 && d <= 1.0 && nums.get(1).is_none_or(|s| *s > 0.0) => {
                            node.brush.grain = Some(motif_core::Grain { density: d, size: nums.get(1).copied().unwrap_or(2.0), color });
                        }
                        _ => self.err(p.key_span, "grain — зерно: доля точек от 0 до 1, размер точки, цвет: grain 0.2, grain 0.3 3 #2a2e39"),
                    }
                }
                "slice" => match (p.values.len(), num(0), num(1), num(2), num(3)) {
                    (1, Some(a), ..) if a >= 0.0 => node.slice = Some([a; 4]),
                    (2, Some(x), Some(y), ..) if x >= 0.0 && y >= 0.0 => node.slice = Some([x, y, x, y]),
                    (4, Some(l), Some(t), Some(r), Some(b)) if [l, t, r, b].iter().all(|v| *v >= 0.0) => node.slice = Some([l, t, r, b]),
                    _ => self.err(p.key_span, "slice — поля 9-slice, которые не растягиваются: slice 16, slice 16 12 (по бокам, сверху-снизу), slice 16 12 16 20 (слева, сверху, справа, снизу)"),
                },
                "hidden" => node.visible = false,
                "evenodd" => node.even_odd = true,
                "clip" => {
                    if n.kind != ShapeKind::Group {
                        self.err(p.key_span, "маска clip бывает только у group: первый объект группы обрезает остальные");
                        continue;
                    }
                    node.clip = true;
                }
                "gradient" => {
                    let radial = match &p.values[0].kind {
                        ValueKind::Ref(k) if k == "linear" => false,
                        ValueKind::Ref(k) if k == "radial" => true,
                        _ => {
                            self.err(p.values[0].span, "после gradient укажите linear или radial");
                            continue;
                        }
                    };
                    let mut angle = 0.0;
                    let mut colors = Vec::new();
                    let mut ok = true;
                    for (i, v) in vals.iter().enumerate().skip(1) {
                        match v {
                            Some(Value::Num(a)) if i == 1 && !radial => angle = *a,
                            Some(Value::Color(c)) => colors.push(*c),
                            _ => ok = false,
                        }
                    }
                    if !ok || colors.len() < 2 {
                        self.err(p.key_span, "градиент: gradient linear 90 #цвет #цвет или gradient radial #цвет #цвет (от двух цветов)");
                        continue;
                    }
                    node.gradient = Some(motif_core::Gradient { radial, angle, colors });
                }
                "d" => {
                    let ValueKind::Path(data) = &p.values[0].kind else { continue };
                    if n.kind != ShapeKind::Path {
                        self.err(p.key_span, "контур d бывает только у path");
                        continue;
                    }
                    match motif_core::kurbo::BezPath::from_svg(data) {
                        Ok(path) => {
                            let bb = path.bounding_box();
                            node.width = bb.width();
                            node.height = bb.height();
                            node.geometry = path;
                            if !generated {
                                self.scene.path_sources.insert(id.clone(), p.values[0].span);
                            }
                        }
                        Err(e) => self.err(p.values[0].span, format!("не удалось прочитать контур: {e}")),
                    }
                }
                "content" | "font" | "align" | "line-height" if n.kind != ShapeKind::Text => {
                    self.err(p.key_span, format!("«{}» бывает только у text", p.key));
                }
                "content" => {
                    if let ValueKind::Path(s) = &p.values[0].kind {
                        content = s.replace("\\n", "\n");
                    }
                }
                "font" => {
                    font = match &p.values[0].kind {
                        ValueKind::Path(file) => Some(file.clone()),
                        ValueKind::Ref(w) if w == "bold" || w == "regular" => Some(w.clone()),
                        _ => {
                            self.err(p.values[0].span, "шрифт: font \"файл.ttf\", font bold или font regular");
                            continue;
                        }
                    }
                }
                "align" => match &p.values[0].kind {
                    ValueKind::Ref(w) if motif_core::TextAlign::from_keyword(w).is_some() => {
                        node.text_style.align = motif_core::TextAlign::from_keyword(w).unwrap_or_default();
                    }
                    _ => self.err(p.values[0].span, "выравнивание: align left, center или right"),
                },
                "line-height" => match num(0) {
                    Some(v) if v > 0.0 => node.text_style.line_height = v,
                    _ => bad(self),
                },
                "outline" => {
                    let mut out = Vec::new();
                    for pair in vals.chunks(2) {
                        match pair {
                            [Some(Value::Color(c)), Some(Value::Num(w))] if *w > 0.0 => out.push((*c, *w)),
                            _ => {
                                out.clear();
                                break;
                            }
                        }
                    }
                    if out.is_empty() {
                        self.err(p.key_span, "обводки слоями: outline #цвет толщина [#цвет толщина …], например outline #1b1d24 12 #ffffff 6");
                        continue;
                    }
                    out.sort_by(|a, b| b.1.total_cmp(&a.1));
                    node.outlines = out;
                }
                key => {
                    let prop = match key {
                        "font-size" => Prop::FontSize,
                        "letter-spacing" => Prop::LetterSpacing,
                        "reveal" => Prop::Reveal,
                        "wave" => Prop::Wave,
                        "phase" => Prop::Phase,
                        "blur" => Prop::Blur,
                        "elevation" => Prop::Elevation,
                        "lat" => Prop::Lat,
                        "lon" => Prop::Lon,
                        "alt" => Prop::Alt,
                        "yaw" => Prop::Yaw,
                        "pitch" => Prop::Pitch,
                        "r" => Prop::Radius,
                        "rot" => Prop::Rotation,
                        "opacity" => Prop::Opacity,
                        "points" => Prop::Points,
                        _ => Prop::Inner,
                    };
                    match num(0) {
                        Some(v) => {
                            node.set(prop, Value::Num(v));
                            record(&mut self.scene, prop, &p.values[0], None);
                        }
                        None => bad(self),
                    }
                }
            }
        }
        if n.kind == ShapeKind::Text {
            match crate::fonts::load(font.as_deref(), self.base.as_deref()).and_then(|bytes| motif_core::TextShape::new(&content, &bytes)) {
                Ok(shape) => {
                    let b = shape.bounds(&node.text_style);
                    (node.width, node.height) = (b.width(), b.height());
                    node.text = Some(std::sync::Arc::new(shape));
                }
                Err(e) => self.err(n.id_span, e),
            }
        }
        let index = self.scene.doc.nodes.len();
        if let Some((target, span)) = ik {
            node.ik = Some(motif_core::Ik { target: format!("{}{target}", self.prefix), chain, flip });
            self.rig_refs.push((index, format!("{}{target}", self.prefix), span));
        }
        if let Some(span) = emit_only.filter(|_| !has_emit) {
            self.err(span, "это настройка частиц: сначала сделайте фигуру источником, emit 30");
        }
        if let Some(span) = rope_only.filter(|_| rope.is_none()) {
            self.err(span, "это настройка верёвки или ткани: сначала сделайте фигуру верёвкой (rope) или тканью (cloth)");
        }
        if let Some(r) = rope.as_mut() {
            if let Some(pins) = pins {
                if let Some(bad) = pins.iter().find(|p| p.of_rope() == r.cloth) {
                    let what = if r.cloth {
                        "у ткани закрепляют края и углы: top, bottom, left, right, top-left…"
                    } else {
                        "у верёвки закрепляют концы: start, end"
                    };
                    self.err(n.id_span, format!("pin {}: {what}", bad.word()));
                }
                r.pins = pins;
            }
            if let Some(w) = wind {
                r.wind = w;
            }
            if r.cloth && !ties.is_empty() {
                self.err(n.id_span, "tie держит концы верёвки, у ткани их нет: закрепите край, pin top");
            }
        }
        node.tie = ties.iter().map(|(t, _)| format!("{}{t}", self.prefix)).collect();
        self.rope_refs.extend(ties.into_iter().map(|(t, span)| (index, format!("{}{t}", self.prefix), span, false)));
        if let Some((r, f, span)) = hang {
            node.hang = Some((format!("{}{r}", self.prefix), f));
            self.rope_refs.push((index, format!("{}{r}", self.prefix), span, true));
        }
        if let Some((r, span)) = links {
            node.links = Some(format!("{}{r}", self.prefix));
            self.rope_refs.push((index, format!("{}{r}", self.prefix), span, true));
        }
        if let Some((r, span)) = range {
            if node.pull.is_none() && node.fan.is_none() {
                self.err(span, "«range» бывает у притяжения (pull) и вентилятора (fan)");
            }
            for f in [&mut node.pull, &mut node.fan].into_iter().flatten() {
                f.range = r;
            }
        }
        if let Some((c, span)) = cone {
            match &mut node.fan {
                Some(f) => f.cone = c,
                None => self.err(span, "«cone» бывает у вентилятора: fan 600 cone 40"),
            }
        }
        if let Some(span) = gravity_span.filter(|_| n.kind.is_container() && roll.is_none()) {
            self.err(span, "«gravity» бывает у фигур: мягких тел, верёвок, жидкостей, источников частиц — и у всего, что катится по шару (roll)");
        }
        if let Some((v, span)) = flow {
            match fluid.as_mut() {
                Some(f) => f.flow = v,
                None => self.err(span, "flow льёт жидкость: сначала сделайте фигуру жидкостью, fluid"),
            }
        }
        if has_emit {
            if let Some(g) = gravity {
                emit.as_mut().unwrap().gravity = g;
            }
            node.emit = emit;
        } else if let (Some(g), Some(r)) = (gravity, rope.as_mut()) {
            r.gravity = g;
        } else if let (Some(g), Some(f)) = (gravity, fluid.as_mut()) {
            f.gravity = g;
        } else if let (Some(g), Some((r, _))) = (gravity, roll.as_mut()) {
            r.gravity = g;
        } else if let Some(g) = gravity {
            soft.get_or_insert_with(motif_core::Soft::default).gravity = g;
        }
        node.soft = soft;
        node.rope = rope;
        node.fluid = fluid;
        if let Some((r, span)) = roll {
            if on.is_none() {
                self.err(span, "roll катит объект по шару: нужен ещё on шар");
            }
            node.roll = Some(r);
        }
        if let Some(ids) = morph
            && let Some(m) = &mut node.morph
        {
            m.targets = ids.iter().map(|(t, _)| format!("{}{t}", self.prefix)).collect();
            self.morph_refs.extend(ids.into_iter().map(|(t, span)| (index, format!("{}{t}", self.prefix), span)));
        }
        if let Some((ball, span)) = on {
            node.on = Some(format!("{}{ball}", self.prefix));
            self.ball_refs.push((index, format!("{}{ball}", self.prefix), span));
        } else if node.wrap {
            self.err(n.id_span, "wrap кладёт фигуру на шар: нужен ещё on шар");
        }
        if let Some(bones) = skin {
            let ids = bones.iter().map(|(b, _)| format!("{}{b}", self.prefix)).collect();
            node.skin = Some(motif_core::Skin { sharp, ..motif_core::Skin::new(ids) });
            self.rig_refs.extend(bones.into_iter().map(|(b, span)| (index, format!("{}{b}", self.prefix), span)));
        }
        self.scene.doc.nodes.push(node);
        self.scene.sites.insert(
            id,
            NodeSite {
                insert_at: n.insert_at,
                generated,
                line: crate::line_of(self.src, n.id_span.start),
                span: n.span,
                id_span: n.id_span,
                name_span: n.name_span,
            },
        );
        self.stmts(&n.children, Some(index), generated);
        Some(index)
    }

    /// Expands `{expr}` parts of an id.
    fn template(&mut self, raw: &str, span: Span) -> Option<String> {
        let mut out = String::new();
        let mut rest = raw;
        while let Some(open) = rest.find('{') {
            out.push_str(&rest[..open]);
            let close = rest[open..].find('}')? + open;
            let v = match crate::expr::eval(&rest[open + 1..close], &|name| self.num_var(name)) {
                Ok(v) => v,
                Err(e) => {
                    self.err(span, e);
                    return None;
                }
            };
            out.push_str(&crate::fmt_num(v));
            rest = &rest[close + 1..];
        }
        out.push_str(rest);
        Some(out)
    }

    fn num_var(&self, name: &str) -> Option<f64> {
        match self.vars.get(name) {
            Some(Var::Num(v)) => Some(*v),
            _ => None,
        }
    }

    fn value(&mut self, v: &ValueAst) -> Option<Value> {
        match &v.kind {
            ValueKind::Num(n) => Some(Value::Num(*n)),
            ValueKind::Color(c) => Some(Value::Color(*c)),
            ValueKind::None => Some(Value::None),
            ValueKind::Ref(name) => match self.vars.get(name) {
                Some(Var::Num(n)) => Some(Value::Num(*n)),
                Some(Var::Color(c)) => Some(Value::Color(*c)),
                None => {
                    self.err(v.span, format!("неизвестное имя «{name}»"));
                    None
                }
            },
            ValueKind::Path(_) => {
                self.err(v.span, "текст в кавычках пишется только после d, content и font");
                None
            }
            ValueKind::Expr(e) => match crate::expr::eval(e, &|name| self.num_var(name)) {
                Ok(n) => Some(Value::Num(n)),
                Err(msg) => {
                    self.err(v.span, msg);
                    None
                }
            },
        }
    }
}
