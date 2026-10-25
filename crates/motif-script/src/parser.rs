//! Turns tokens into a statement tree that still knows where every value lives in the source.

use crate::lexer::{Tok, Token};
use crate::{ScriptError, Span};
use motif_core::{Color, ShapeKind};

#[derive(Clone, Debug, PartialEq)]
pub enum ValueKind {
    Num(f64),
    Color(Color),
    /// `none` paint.
    None,
    /// Reference to a `let` or loop variable.
    Ref(String),
    /// Inline expression `{...}`.
    Expr(String),
    /// SVG path data of `d "M 0 0 L 10 0 Z"`.
    Path(String),
}

#[derive(Clone, Debug, PartialEq)]
pub struct ValueAst {
    pub kind: ValueKind,
    pub span: Span,
}

impl ValueAst {
    pub fn is_literal(&self) -> bool {
        matches!(self.kind, ValueKind::Num(_) | ValueKind::Color(_) | ValueKind::None)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct PropAst {
    pub key: String,
    pub key_span: Span,
    pub values: Vec<ValueAst>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct NodeAst {
    pub kind: ShapeKind,
    /// Id, possibly templated (`coin{i}`).
    pub id: String,
    pub id_span: Span,
    pub name: Option<String>,
    pub props: Vec<PropAst>,
    /// Byte offset where a new property can be appended on the node's line.
    pub insert_at: usize,
    pub children: Vec<Stmt>,
    /// The whole statement, from the shape keyword to the end of the line or the group's `}`.
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq)]
pub struct KeyAst {
    pub time: f64,
    pub value: ValueAst,
    pub ease: Option<motif_core::Ease>,
    /// From the time to the end of the value or curve: the whole `0.6: 278 ease-out`.
    pub span: Span,
    pub time_span: Span,
    pub ease_span: Option<Span>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TrackAst {
    pub node: String,
    pub prop: String,
    pub span: Span,
    pub keys: Vec<KeyAst>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct OverrideAst {
    pub node: String,
    pub prop: String,
    pub span: Span,
    pub value: ValueAst,
}

/// `name=value` on a `symbol` or `use` line.
#[derive(Clone, Debug, PartialEq)]
pub struct ParamAst {
    pub name: String,
    pub name_span: Span,
    pub value: ValueAst,
}

/// `use Coin coin1 "Монета" at 40 40 size=20`: one copy of a symbol. `node` is the group that
/// holds the copy (its id, name and own properties); its children come from the symbol.
#[derive(Clone, Debug, PartialEq)]
pub struct UseAst {
    pub symbol: String,
    pub symbol_span: Span,
    pub node: NodeAst,
    pub params: Vec<ParamAst>,
}

/// `speed > 0.5`, `hover`, `not hover` in a transition.
#[derive(Clone, Debug, PartialEq)]
pub struct CondAst {
    pub input: String,
    pub span: Span,
    pub op: motif_core::CondOp,
    pub value: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub enum MachineLine {
    /// `input speed 0`, `input hover false`, `input jump trigger`.
    Input { name: String, name_span: Span, kind: motif_core::InputKind, default: f64 },
    /// `any at 40 12`: where the `any` box sits in the editor graph.
    AnyAt(f64, f64),
    /// `state Run anim Running at 220 40`.
    State { name: String, name_span: Span, anim: Option<(String, Span)>, at: Option<(f64, f64)> },
    /// `Idle -> Run when speed > 0.5 at end blend 0.2s`; `from` is `None` for `any`.
    Transition { from: Option<String>, from_span: Span, to: String, to_span: Span, conds: Vec<CondAst>, at_end: bool, blend: f64 },
}

#[derive(Clone, Debug, PartialEq)]
pub struct MachineLineAst {
    pub line: MachineLine,
    /// The whole line, without its line break.
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Stmt {
    Artboard {
        name: Option<String>,
        width: f64,
        height: f64,
        props: Vec<PropAst>,
    },
    Let {
        name: String,
        value: ValueAst,
    },
    /// `backdrop "ref.png" at 0 0 scale 1 opacity 0.5 hidden`: a reference picture for the editor.
    Backdrop {
        file: String,
        props: Vec<PropAst>,
        span: Span,
    },
    Node(NodeAst),
    /// `symbol Coin size=16 color=#ffd23f { … }`: a reusable piece, drawn only through `use`.
    Symbol {
        name: String,
        name_span: Span,
        /// `clip`: the first object of every copy is a mask for the others.
        clip: bool,
        params: Vec<ParamAst>,
        body: Vec<Stmt>,
        /// From `symbol` to the closing brace.
        span: Span,
    },
    Use(UseAst),
    For {
        var: String,
        from: ValueAst,
        to: ValueAst,
        body: Vec<Stmt>,
    },
    Anim {
        name: String,
        duration: f64,
        looping: bool,
        tracks: Vec<TrackAst>,
        span: Span,
        /// Offset of the block's closing brace (where new tracks are inserted).
        close: usize,
        duration_span: Span,
        /// The `loop` word, when present.
        loop_span: Option<Span>,
    },
    /// `machine Player { … }`: states, the transitions between them and the inputs they read.
    Machine {
        name: String,
        name_span: Span,
        lines: Vec<MachineLineAst>,
        span: Span,
        /// Offset of the block's closing brace.
        close: usize,
    },
    /// `close` is the offset of the block's closing brace (where new entries are inserted).
    Override {
        entries: Vec<OverrideAst>,
        close: usize,
        /// Offset of the `override` keyword.
        start: usize,
    },
}

/// Properties a node line accepts, with their minimum and maximum number of values.
pub fn prop_arity(key: &str) -> Option<(usize, usize)> {
    Some(match key {
        "at" => (2, 2),
        "size" | "scale" | "stroke" | "skew" => (1, 2),
        "rot" | "opacity" | "points" | "inner" | "fill" | "cap" | "join" | "blend" => (1, 1),
        "r" => (1, 4),
        "pivot" => (2, 2),
        "dash" => (1, 8),
        "shadow" => (3, 4),
        "hidden" | "clip" | "evenodd" => (0, 0),
        "d" | "content" => (1, 1),
        "font" | "font-size" | "letter-spacing" | "line-height" | "align" | "reveal" | "wave" | "phase" => (1, 1),
        "blur" | "elevation" => (1, 1),
        "outline" => (2, 8),
        "gradient" => (2, 10),
        "len" | "chain" | "skin-sharp" | "ik" => (1, 1),
        "flip" | "solid" => (0, 0),
        "soft" => (0, 1),
        "spring" | "jiggle" => (0, 2),
        "squash" => (0, 3),
        "pressure" | "friction" | "damp" | "gravity" | "drive" => (1, 1),
        "skin" => (1, 16),
        _ => return None,
    })
}

pub struct Parser<'a> {
    src: &'a str,
    toks: Vec<Token>,
    pos: usize,
    pub errors: Vec<ScriptError>,
    /// Names from `let` and `for`: such a name is a value even where it is also a property
    /// (`let skin = #5ee0a0` then `fill skin`).
    vars: std::collections::HashSet<String>,
}

impl<'a> Parser<'a> {
    pub fn new(src: &'a str, toks: Vec<Token>) -> Self {
        Parser { src, toks, pos: 0, errors: Vec::new(), vars: Default::default() }
    }

    fn peek(&self) -> Option<&Tok> {
        self.toks.get(self.pos).map(|t| &t.tok)
    }

    fn peek_span(&self) -> Span {
        self.toks.get(self.pos).map_or(Span::new(self.src.len(), self.src.len()), |t| t.span)
    }

    fn next(&mut self) -> Option<Token> {
        let t = self.toks.get(self.pos).cloned();
        self.pos += 1;
        t
    }

    fn err(&self, span: Span, msg: impl Into<String>) -> ScriptError {
        ScriptError::at(self.src, span.start, msg)
    }

    fn at_line_end(&self) -> bool {
        matches!(self.peek(), None | Some(Tok::Newline))
    }

    /// Skips the rest of the current line (used to recover after an error).
    fn skip_line(&mut self) {
        let mut depth = 0i32;
        while let Some(t) = self.peek() {
            match t {
                Tok::BlockOpen => depth += 1,
                Tok::BlockClose if depth > 0 => depth -= 1,
                Tok::Newline if depth == 0 => break,
                _ => {}
            }
            self.pos += 1;
        }
    }

    fn ident(&mut self, what: &str) -> Result<(String, Span), ScriptError> {
        match self.next() {
            Some(Token { tok: Tok::Ident(s), span }) => Ok((s, span)),
            Some(t) => Err(self.err(t.span, format!("ожидалось {what}"))),
            None => Err(self.err(self.peek_span(), format!("ожидалось {what}"))),
        }
    }

    fn expect(&mut self, tok: Tok, what: &str) -> Result<Span, ScriptError> {
        match self.next() {
            Some(t) if t.tok == tok => Ok(t.span),
            Some(t) => Err(self.err(t.span, format!("ожидалось {what}"))),
            None => Err(self.err(self.peek_span(), format!("ожидалось {what}"))),
        }
    }

    fn is_value_start(&self) -> bool {
        match self.peek() {
            Some(Tok::Num(_) | Tok::Color(_) | Tok::Expr(_) | Tok::Seconds(_)) => true,
            Some(Tok::Ident(s)) => prop_arity(s).is_none() || self.vars.contains(s),
            _ => false,
        }
    }

    fn value(&mut self) -> Result<ValueAst, ScriptError> {
        let t = self.next().ok_or_else(|| self.err(self.peek_span(), "ожидалось значение"))?;
        let kind = match t.tok {
            Tok::Num(v) | Tok::Seconds(v) => ValueKind::Num(v),
            Tok::Color(c) => ValueKind::Color(c),
            Tok::Expr(e) => ValueKind::Expr(e),
            Tok::Ident(s) if s == "none" => ValueKind::None,
            Tok::Ident(s) => ValueKind::Ref(s),
            _ => return Err(self.err(t.span, "ожидалось значение: число, цвет, имя или {выражение}")),
        };
        Ok(ValueAst { kind, span: t.span })
    }

    /// Parses statements until the end of input or a closing brace (for blocks).
    pub fn block(&mut self, nested: bool) -> Vec<Stmt> {
        let mut out = Vec::new();
        loop {
            match self.peek() {
                None => {
                    if nested {
                        self.errors.push(self.err(self.peek_span(), "не хватает закрывающей «}»"));
                    }
                    return out;
                }
                Some(Tok::Newline) => {
                    self.pos += 1;
                }
                Some(Tok::BlockClose) => {
                    if nested {
                        return out;
                    }
                    let span = self.peek_span();
                    self.errors.push(self.err(span, "лишняя «}»"));
                    self.pos += 1;
                }
                Some(_) => match self.statement() {
                    Ok(s) => out.push(s),
                    Err(e) => {
                        self.errors.push(e);
                        self.skip_line();
                    }
                },
            }
        }
    }

    fn close_block(&mut self) -> Result<usize, ScriptError> {
        let span = self.peek_span();
        self.expect(Tok::BlockClose, "«}»")?;
        Ok(span.start)
    }

    fn statement(&mut self) -> Result<Stmt, ScriptError> {
        let (word, span) =
            self.ident("команда: artboard, backdrop, let, rect, ellipse, star, polygon, path, group, symbol, use, for, anim, machine или override")?;
        match word.as_str() {
            "artboard" => self.artboard(),
            "backdrop" => self.backdrop(span.start),
            "let" => {
                let (name, _) = self.ident("имя переменной")?;
                self.vars.insert(name.clone());
                self.expect(Tok::Eq, "«=»")?;
                let value = self.value()?;
                Ok(Stmt::Let { name, value })
            }
            "for" => {
                let (var, _) = self.ident("имя переменной цикла")?;
                self.vars.insert(var.clone());
                let (kw, kw_span) = self.ident("«in»")?;
                if kw != "in" {
                    return Err(self.err(kw_span, "ожидалось «in»"));
                }
                let from = self.value()?;
                self.expect(Tok::DotDot, "«..»")?;
                let to = self.value()?;
                self.expect(Tok::BlockOpen, "«{» в конце строки")?;
                let body = self.block(true);
                self.close_block()?;
                Ok(Stmt::For { var, from, to, body })
            }
            "anim" => self.anim(span),
            "machine" => self.machine(span),
            "symbol" => {
                let (name, name_span) = self.ident("имя символа")?;
                let mut params = Vec::new();
                let mut clip = false;
                while !self.at_line_end() && self.peek() != Some(&Tok::BlockOpen) {
                    if matches!(self.peek(), Some(Tok::Ident(w)) if w == "clip") && !matches!(self.toks.get(self.pos + 1).map(|t| &t.tok), Some(Tok::Eq)) {
                        self.pos += 1;
                        clip = true;
                        continue;
                    }
                    params.push(self.param()?);
                }
                self.expect(Tok::BlockOpen, "«{» в конце строки")?;
                let body = self.block(true);
                let close = self.close_block()?;
                Ok(Stmt::Symbol { name, name_span, clip, params, body, span: Span::new(span.start, close + 1) })
            }
            "use" => {
                let (symbol, symbol_span) = self.ident("имя символа после use")?;
                let mut params = Vec::new();
                let node = self.node_line(ShapeKind::Group, span.start, Some(&mut params))?;
                Ok(Stmt::Use(UseAst { symbol, symbol_span, node, params }))
            }
            "override" => {
                self.expect(Tok::BlockOpen, "«{» в конце строки")?;
                let mut entries = Vec::new();
                loop {
                    match self.peek() {
                        Some(Tok::Newline) => self.pos += 1,
                        Some(Tok::BlockClose) | None => break,
                        Some(_) => match self.override_entry() {
                            Ok(e) => entries.push(e),
                            Err(e) => {
                                self.errors.push(e);
                                self.skip_line();
                            }
                        },
                    }
                }
                let close = self.close_block()?;
                Ok(Stmt::Override { entries, close, start: span.start })
            }
            kw => match ShapeKind::from_keyword(kw) {
                Some(kind) => self.node(kind, span.start),
                None => Err(self.err(span, format!("неизвестная команда «{kw}»"))),
            },
        }
    }

    fn artboard(&mut self) -> Result<Stmt, ScriptError> {
        let name = match self.peek() {
            Some(Tok::Str(_)) => match self.next().map(|t| t.tok) {
                Some(Tok::Str(s)) => Some(s),
                _ => None,
            },
            _ => None,
        };
        let (width, height) = match self.next() {
            Some(Token { tok: Tok::Size(w, h), .. }) => (w, h),
            Some(t) => return Err(self.err(t.span, "ожидался размер артборда, например 480x360")),
            None => return Err(self.err(self.peek_span(), "ожидался размер артборда, например 480x360")),
        };
        let mut props = Vec::new();
        while !self.at_line_end() {
            let (key, key_span) = self.ident("свойство артборда bg или light")?;
            if key != "bg" && key != "light" {
                return Err(self.err(key_span, format!("у артборда нет свойства «{key}»")));
            }
            props.push(PropAst { key, key_span, values: vec![self.value()?] });
        }
        Ok(Stmt::Artboard { name, width, height, props })
    }

    fn backdrop(&mut self, start: usize) -> Result<Stmt, ScriptError> {
        let file = match self.next() {
            Some(Token { tok: Tok::Str(s), .. }) => s,
            Some(t) => return Err(self.err(t.span, "после backdrop нужен файл в кавычках, например backdrop \"эскиз.png\"")),
            None => return Err(self.err(self.peek_span(), "после backdrop нужен файл в кавычках")),
        };
        let mut end = self.toks[self.pos - 1].span.end;
        let mut props = Vec::new();
        while !self.at_line_end() {
            let (key, key_span) = self.ident("свойство подложки: at, scale, opacity или hidden")?;
            let n = match key.as_str() {
                "at" => 2,
                "scale" | "opacity" => 1,
                "hidden" => 0,
                _ => return Err(self.err(key_span, format!("у подложки нет свойства «{key}»: at, scale, opacity, hidden"))),
            };
            let mut values = Vec::new();
            for _ in 0..n {
                values.push(self.value()?);
            }
            end = values.last().map_or(key_span.end, |v| v.span.end);
            props.push(PropAst { key, key_span, values });
        }
        Ok(Stmt::Backdrop { file, props, span: Span::new(start, end) })
    }

    /// `name=value`, with a value of its own: number, colour, name or `{expression}`.
    fn param(&mut self) -> Result<ParamAst, ScriptError> {
        let (name, name_span) = self.ident("параметр вида имя=значение")?;
        self.expect(Tok::Eq, &format!("«=» после «{name}»: параметр пишется как {name}=значение"))?;
        Ok(ParamAst { name, name_span, value: self.value()? })
    }

    fn node(&mut self, kind: ShapeKind, start: usize) -> Result<Stmt, ScriptError> {
        Ok(Stmt::Node(self.node_line(kind, start, None)?))
    }

    /// A node line and, for a group, its block. A `use` line passes `params` and may have
    /// `name=value` parameters among its properties, but no block.
    fn node_line(&mut self, kind: ShapeKind, start: usize, mut params: Option<&mut Vec<ParamAst>>) -> Result<NodeAst, ScriptError> {
        let (id, id_span) = self.ident("id объекта")?;
        // A property name is fine as an id (`ellipse shadow "Тень"`) unless values follow it,
        // which means the id was left out (`rect at 10 20`).
        if prop_arity(&id).is_some() && (self.is_value_start() || prop_arity(&id) == Some((0, 0))) {
            return Err(self.err(id_span, format!("«{id}» — это свойство; сначала укажите id объекта")));
        }
        let mut name = None;
        let mut insert_at = id_span.end;
        if let Some(Tok::Str(_)) = self.peek() {
            let t = self.next().unwrap();
            if let Tok::Str(s) = t.tok {
                name = Some(s);
            }
            insert_at = t.span.end;
        }
        let mut props = Vec::new();
        while !self.at_line_end() && self.peek() != Some(&Tok::BlockOpen) {
            if let Some(params) = params.as_deref_mut()
                && matches!(self.toks.get(self.pos + 1).map(|t| &t.tok), Some(Tok::Eq))
            {
                let p = self.param()?;
                insert_at = p.value.span.end;
                params.push(p);
                continue;
            }
            let (key, key_span) = self.ident("свойство")?;
            let Some((min, max)) = prop_arity(&key) else {
                return Err(self.err(key_span, format!("неизвестное свойство «{key}»")));
            };
            let mut values = Vec::new();
            if key == "d" || key == "content" {
                match self.next() {
                    Some(Token { tok: Tok::Str(s), span }) => values.push(ValueAst { kind: ValueKind::Path(s), span }),
                    _ if key == "content" => return Err(self.err(key_span, "после content нужен текст в кавычках, например content \"Привет\"")),
                    _ => return Err(self.err(key_span, "после d нужен контур в кавычках, например d \"M 0 0 L 40 0 L 20 30 Z\"")),
                }
            }
            // Ids of objects: the IK target, the bones of a skin.
            if key == "ik" || key == "skin" {
                while values.len() < max
                    && let Some(Tok::Ident(s)) = self.peek()
                    && (values.is_empty() || prop_arity(s).is_none())
                {
                    let t = self.next().unwrap();
                    if let Tok::Ident(s) = t.tok {
                        values.push(ValueAst { kind: ValueKind::Path(s), span: t.span });
                    }
                }
                if values.is_empty() {
                    let hint = if key == "ik" {
                        "после ik нужен id цели, например ik hand_target"
                    } else {
                        "после skin нужны id костей, например skin arm forearm"
                    };
                    return Err(self.err(key_span, hint));
                }
            }
            // A font file in quotes, or `font bold`.
            if key == "font"
                && let Some(Tok::Str(_)) = self.peek()
                && let Some(Token { tok: Tok::Str(s), span }) = self.next()
            {
                values.push(ValueAst { kind: ValueKind::Path(s), span });
            }
            while values.len() < max && self.is_value_start() {
                values.push(self.value()?);
            }
            if values.len() < min {
                return Err(self.err(key_span, format!("свойству «{key}» нужно значений: {min}")));
            }
            insert_at = values.last().map_or(key_span.end, |v| v.span.end);
            props.push(PropAst { key, key_span, values });
        }
        let mut children = Vec::new();
        let mut end = insert_at;
        if self.peek() == Some(&Tok::BlockOpen) {
            if params.is_some() {
                return Err(self.err(self.peek_span(), "у use нет своего блока: содержимое берётся из символа"));
            }
            if !kind.is_container() {
                return Err(self.err(self.peek_span(), "вложенные объекты бывают только у group и bone"));
            }
            self.pos += 1;
            children = self.block(true);
            end = self.close_block()? + 1;
        }
        Ok(NodeAst { kind, id, id_span, name, props, insert_at, children, span: Span::new(start, end) })
    }

    fn path(&mut self) -> Result<(String, String, Span), ScriptError> {
        let (node, a) = self.ident("путь вида объект.свойство")?;
        self.expect(Tok::Dot, "«.» между объектом и свойством")?;
        let (prop, b) = self.ident("свойство")?;
        Ok((node, prop, Span::new(a.start, b.end)))
    }

    fn override_entry(&mut self) -> Result<OverrideAst, ScriptError> {
        let (node, prop, span) = self.path()?;
        self.expect(Tok::Eq, "«=»")?;
        let value = self.value()?;
        Ok(OverrideAst { node, prop, span, value })
    }

    fn anim(&mut self, span: Span) -> Result<Stmt, ScriptError> {
        let (name, _) = self.ident("имя анимации")?;
        let duration_span = self.peek_span();
        let duration = match self.next() {
            Some(Token { tok: Tok::Seconds(v) | Tok::Num(v), .. }) => v,
            Some(t) => return Err(self.err(t.span, "ожидалась длительность, например 1.2s")),
            None => return Err(self.err(self.peek_span(), "ожидалась длительность, например 1.2s")),
        };
        let mut looping = false;
        let mut loop_span = None;
        if let Some(Tok::Ident(s)) = self.peek()
            && s == "loop"
        {
            looping = true;
            loop_span = Some(self.peek_span());
            self.pos += 1;
        }
        self.expect(Tok::BlockOpen, "«{» в конце строки")?;
        let mut tracks = Vec::new();
        loop {
            match self.peek() {
                Some(Tok::Newline) => self.pos += 1,
                Some(Tok::BlockClose) | None => break,
                Some(_) => match self.track() {
                    Ok(t) => tracks.push(t),
                    Err(e) => {
                        self.errors.push(e);
                        self.skip_line();
                    }
                },
            }
        }
        let close = self.close_block()?;
        Ok(Stmt::Anim { name, duration, looping, tracks, span, close, duration_span, loop_span })
    }

    fn machine(&mut self, span: Span) -> Result<Stmt, ScriptError> {
        let (name, name_span) = self.ident("имя машины состояний")?;
        self.expect(Tok::BlockOpen, "«{» в конце строки")?;
        let mut lines = Vec::new();
        loop {
            match self.peek() {
                Some(Tok::Newline) => self.pos += 1,
                Some(Tok::BlockClose) | None => break,
                Some(_) => match self.machine_line() {
                    Ok(l) => lines.push(l),
                    Err(e) => {
                        self.errors.push(e);
                        self.skip_line();
                    }
                },
            }
        }
        let close = self.close_block()?;
        Ok(Stmt::Machine { name, name_span, lines, span: Span::new(span.start, close + 1), close })
    }

    fn num(&mut self, what: &str) -> Result<(f64, Span), ScriptError> {
        match self.next() {
            Some(Token { tok: Tok::Num(v) | Tok::Seconds(v), span }) => Ok((v, span)),
            Some(t) => Err(self.err(t.span, format!("ожидалось {what}"))),
            None => Err(self.err(self.peek_span(), format!("ожидалось {what}"))),
        }
    }

    fn word_is(&self, word: &str) -> bool {
        matches!(self.peek(), Some(Tok::Ident(w)) if w == word)
    }

    fn machine_line(&mut self) -> Result<MachineLineAst, ScriptError> {
        use motif_core::{CondOp, InputKind};
        let start = self.peek_span().start;
        let (word, word_span) = self.ident("строка машины: input, state или переход вида Idle -> Run")?;
        let line = match word.as_str() {
            "input" => {
                let (name, name_span) = self.ident("имя входа")?;
                let hint = "после имени входа: число (input speed 0), false/true (флаг) или trigger";
                let (kind, default) = match self.next() {
                    Some(Token { tok: Tok::Num(v), .. }) => (InputKind::Number, v),
                    Some(Token { tok: Tok::Ident(w), span }) => match w.as_str() {
                        "number" => match self.peek() {
                            Some(Tok::Num(v)) => {
                                let v = *v;
                                self.pos += 1;
                                (InputKind::Number, v)
                            }
                            _ => (InputKind::Number, 0.0),
                        },
                        "bool" | "false" => (InputKind::Bool, 0.0),
                        "true" => (InputKind::Bool, 1.0),
                        "trigger" => (InputKind::Trigger, 0.0),
                        _ => return Err(self.err(span, hint)),
                    },
                    Some(t) => return Err(self.err(t.span, hint)),
                    None => return Err(self.err(self.peek_span(), hint)),
                };
                MachineLine::Input { name, name_span, kind, default }
            }
            "state" => {
                let (name, name_span) = self.ident("имя состояния")?;
                let (mut anim, mut at) = (None, None);
                while !self.at_line_end() {
                    let (key, key_span) = self.ident("anim или at")?;
                    match key.as_str() {
                        "anim" => anim = Some(self.ident("имя анимации")?),
                        "at" => at = Some((self.num("число")?.0, self.num("число")?.0)),
                        _ => return Err(self.err(key_span, format!("у состояния нет «{key}»: anim Имя, at x y"))),
                    }
                }
                MachineLine::State { name, name_span, anim, at }
            }
            "any" if self.word_is("at") => {
                self.pos += 1;
                MachineLine::AnyAt(self.num("число")?.0, self.num("число")?.0)
            }
            _ => {
                if self.peek() != Some(&Tok::Arrow) {
                    return Err(self.err(word_span, format!("неизвестная строка «{word}»: input, state или переход вида {word} -> Другое")));
                }
                self.pos += 1;
                let (to, to_span) = self.ident("состояние после «->»")?;
                let (mut conds, mut at_end, mut blend) = (Vec::new(), false, 0.0);
                while !self.at_line_end() {
                    let (key, key_span) = self.ident("when, at end или blend")?;
                    match key.as_str() {
                        "when" => loop {
                            let negate = self.word_is("not");
                            if negate {
                                self.pos += 1;
                            }
                            let (input, span) = self.ident("имя входа")?;
                            let (op, value) = match self.peek() {
                                Some(Tok::Cmp(op)) if !negate => {
                                    let op = CondOp::from_symbol(op).expect("the lexer makes only known comparisons");
                                    self.pos += 1;
                                    let value = match self.next() {
                                        Some(Token { tok: Tok::Num(v), .. }) => v,
                                        Some(Token { tok: Tok::Ident(w), .. }) if w == "true" => 1.0,
                                        Some(Token { tok: Tok::Ident(w), .. }) if w == "false" => 0.0,
                                        _ => return Err(self.err(span, "после сравнения нужно число")),
                                    };
                                    (op, value)
                                }
                                _ => (if negate { CondOp::Not } else { CondOp::Is }, 0.0),
                            };
                            conds.push(CondAst { input, span, op, value });
                            if !self.word_is("and") {
                                break;
                            }
                            self.pos += 1;
                        },
                        "at" => {
                            let (w, span) = self.ident("end")?;
                            if w != "end" {
                                return Err(self.err(span, "ожидалось «at end»: переход в конце анимации"));
                            }
                            at_end = true;
                        }
                        "blend" => blend = self.num("время смешивания, например 0.2s")?.0.max(0.0),
                        _ => return Err(self.err(key_span, format!("у перехода нет «{key}»: when условие, at end, blend 0.2s"))),
                    }
                }
                let from = (word != "any").then_some(word);
                MachineLine::Transition { from, from_span: word_span, to, to_span, conds, at_end, blend }
            }
        };
        if !self.at_line_end() {
            return Err(self.err(self.peek_span(), "лишнее в конце строки"));
        }
        let end = self.toks[self.pos - 1].span.end;
        Ok(MachineLineAst { line, span: Span::new(start, end) })
    }

    fn track(&mut self) -> Result<TrackAst, ScriptError> {
        let (node, prop, span) = self.path()?;
        let mut keys = Vec::new();
        loop {
            let start = self.peek_span().start;
            let time = match self.next() {
                Some(Token { tok: Tok::Num(v) | Tok::Seconds(v), .. }) => v,
                Some(t) => return Err(self.err(t.span, "ожидалось время ключа, например 0.6")),
                None => return Err(self.err(self.peek_span(), "ожидалось время ключа")),
            };
            self.expect(Tok::Colon, "«:» после времени ключа")?;
            let time_span = Span::new(start, self.toks[self.pos - 2].span.end);
            let value = self.value()?;
            let mut end = value.span.end;
            let mut ease_span = None;
            let ease = match self.peek() {
                Some(Tok::Ident(_)) => {
                    let (e, span) = self.ident("кривая")?;
                    end = span.end;
                    let ease = match motif_core::Ease::from_keyword(&e) {
                        Some(ease) => ease,
                        None if e == "cubic" => {
                            // cubic x1 y1 x2 y2: the two handles of a custom curve.
                            let mut c = [0.0; 4];
                            for v in &mut c {
                                match self.next() {
                                    Some(Token { tok: Tok::Num(n), span }) => {
                                        *v = n;
                                        end = span.end;
                                    }
                                    _ => return Err(self.err(span, "после cubic нужны четыре числа: cubic 0.4 0 0.2 1")),
                                }
                            }
                            if !(0.0..=1.0).contains(&c[0]) || !(0.0..=1.0).contains(&c[2]) {
                                return Err(self.err(span, "у cubic первое и третье число (время ручек) от 0 до 1"));
                            }
                            motif_core::Ease::Cubic(c)
                        }
                        None if e == "steps" => match self.next() {
                            Some(Token { tok: Tok::Num(n), span }) if n >= 1.0 && n.fract() == 0.0 && n <= 1000.0 => {
                                end = span.end;
                                motif_core::Ease::Steps(n as u32)
                            }
                            _ => return Err(self.err(span, "после steps нужно целое число шагов, например steps 4")),
                        },
                        None => {
                            return Err(self.err(
                                span,
                                format!("неизвестная кривая «{e}»: linear, ease, ease-in, ease-out, ease-in-out, hold, spring, ease-out-bounce и другие из easings.net, cubic, steps"),
                            ));
                        }
                    };
                    ease_span = Some(Span::new(span.start, end));
                    Some(ease)
                }
                _ => None,
            };
            keys.push(KeyAst { time, value, ease, span: Span::new(start, end), time_span, ease_span });
            if self.peek() == Some(&Tok::Pipe) {
                self.pos += 1;
            } else {
                break;
            }
        }
        if !self.at_line_end() {
            return Err(self.err(self.peek_span(), "ожидалось «|» между ключами или конец строки"));
        }
        Ok(TrackAst { node, prop, span, keys })
    }
}
