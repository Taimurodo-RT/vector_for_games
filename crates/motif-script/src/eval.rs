//! Expands the statement tree into a `Document` and records where each property value came from.

use crate::parser::{NodeAst, Stmt, ValueAst, ValueKind};
use crate::{AnimSite, KeySite, NodeSite, Scene, ScriptError, Source, Span, TrackSite};
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
    vars: HashMap<String, Var>,
    pub scene: Scene,
    pub errors: Vec<ScriptError>,
}

impl<'a> Evaluator<'a> {
    pub fn new(src: &'a str) -> Self {
        Evaluator {
            src,
            vars: HashMap::new(),
            scene: Scene {
                doc: Document::new(Artboard::default()),
                sources: HashMap::new(),
                sites: HashMap::new(),
                override_close: None,
                anims: HashMap::new(),
                blocks_start: None,
                override_entries: Vec::new(),
                path_sources: HashMap::new(),
                gradient_sources: HashMap::new(),
                clip_sources: HashMap::new(),
            },
            errors: Vec::new(),
        }
    }

    fn err(&mut self, span: Span, msg: impl Into<String>) {
        self.errors.push(ScriptError::at(self.src, span.start, msg));
    }

    pub fn run(&mut self, stmts: &[Stmt]) {
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
                    if self.scene.doc.index_of(&t.node).is_none() {
                        self.err(t.span, format!("нет объекта «{}»", t.node));
                        continue;
                    }
                    let Some(prop) = self.prop_path(&t.prop, t.span) else { continue };
                    let mut keys = Vec::new();
                    for k in &t.keys {
                        let ease = match &k.ease {
                            None => Ease::EaseInOut,
                            Some(e) => match Ease::from_keyword(e) {
                                Some(e) => e,
                                None => {
                                    self.err(k.value.span, format!("неизвестная кривая «{e}»: linear, ease-in, ease-out, ease-in-out, hold, spring"));
                                    Ease::Linear
                                }
                            },
                        };
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
                        if let Some(Value::Color(c)) = self.value(&p.values[0]) {
                            self.scene.doc.artboard.background = c;
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
                Stmt::Node(n) => self.node(n, parent, generated),
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
                Stmt::Anim { .. } | Stmt::Override { .. } => {
                    if parent.is_some() {
                        self.err(Span::new(0, 0), "anim и override пишутся на верхнем уровне");
                    }
                }
            }
        }
    }

    fn node(&mut self, n: &NodeAst, parent: Option<usize>, generated: bool) {
        let Some(id) = self.template(&n.id, n.id_span) else { return };
        if self.scene.doc.index_of(&id).is_some() {
            self.err(n.id_span, format!("id «{id}» уже занят"));
            return;
        }
        let mut node = Node::new(id.clone(), n.kind);
        node.name = n.name.clone();
        node.parent = parent;
        let record = |scene: &mut Scene, prop: Prop, v: &ValueAst, paired: Option<Prop>| {
            scene.sources.insert((id.clone(), prop), Source { span: v.span, editable: v.is_literal() && !generated, paired, from_override: false });
        };
        for p in &n.props {
            let vals: Vec<Option<Value>> = match p.key.as_str() {
                "d" => Vec::new(),
                // The first value of a gradient is its kind word, not a name.
                "gradient" => std::iter::once(None).chain(p.values.iter().skip(1).map(|v| self.value(v))).collect(),
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
                "hidden" => node.visible = false,
                "clip" => {
                    if n.kind != ShapeKind::Group {
                        self.err(p.key_span, "маска clip бывает только у group: первый объект группы обрезает остальные");
                        continue;
                    }
                    node.clip = true;
                    if !generated {
                        self.scene.clip_sources.insert(id.clone(), p.key_span);
                    }
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
                    if !generated {
                        let end = p.values.last().map_or(p.key_span.end, |v| v.span.end);
                        self.scene.gradient_sources.insert(id.clone(), Span::new(p.key_span.start, end));
                    }
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
                key => {
                    let prop = match key {
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
        let index = self.scene.doc.nodes.len();
        self.scene.doc.nodes.push(node);
        self.scene
            .sites
            .insert(id, NodeSite { insert_at: n.insert_at, generated, line: crate::line_of(self.src, n.id_span.start), span: n.span, id_span: n.id_span });
        self.stmts(&n.children, Some(index), generated);
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
                self.err(v.span, "контур в кавычках пишется только после d");
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
