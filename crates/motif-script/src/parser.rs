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
    Node(NodeAst),
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
        "size" | "scale" | "stroke" => (1, 2),
        "r" | "rot" | "opacity" | "points" | "inner" | "fill" => (1, 1),
        "hidden" | "clip" => (0, 0),
        "d" => (1, 1),
        "gradient" => (2, 10),
        _ => return None,
    })
}

pub struct Parser<'a> {
    src: &'a str,
    toks: Vec<Token>,
    pos: usize,
    pub errors: Vec<ScriptError>,
}

impl<'a> Parser<'a> {
    pub fn new(src: &'a str, toks: Vec<Token>) -> Self {
        Parser { src, toks, pos: 0, errors: Vec::new() }
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
            Some(Tok::Ident(s)) => prop_arity(s).is_none(),
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
        let (word, span) = self.ident("команда: artboard, let, rect, ellipse, star, polygon, path, group, for, anim или override")?;
        match word.as_str() {
            "artboard" => self.artboard(),
            "let" => {
                let (name, _) = self.ident("имя переменной")?;
                self.expect(Tok::Eq, "«=»")?;
                let value = self.value()?;
                Ok(Stmt::Let { name, value })
            }
            "for" => {
                let (var, _) = self.ident("имя переменной цикла")?;
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
            let (key, key_span) = self.ident("свойство артборда bg")?;
            if key != "bg" {
                return Err(self.err(key_span, format!("у артборда нет свойства «{key}»")));
            }
            props.push(PropAst { key, key_span, values: vec![self.value()?] });
        }
        Ok(Stmt::Artboard { name, width, height, props })
    }

    fn node(&mut self, kind: ShapeKind, start: usize) -> Result<Stmt, ScriptError> {
        let (id, id_span) = self.ident("id объекта")?;
        if prop_arity(&id).is_some() {
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
            let (key, key_span) = self.ident("свойство")?;
            let Some((min, max)) = prop_arity(&key) else {
                return Err(self.err(key_span, format!("неизвестное свойство «{key}»")));
            };
            let mut values = Vec::new();
            if key == "d" {
                match self.next() {
                    Some(Token { tok: Tok::Str(s), span }) => values.push(ValueAst { kind: ValueKind::Path(s), span }),
                    _ => return Err(self.err(key_span, "после d нужен контур в кавычках, например d \"M 0 0 L 40 0 L 20 30 Z\"")),
                }
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
            if kind != ShapeKind::Group {
                return Err(self.err(self.peek_span(), "вложенные объекты бывают только у group"));
            }
            self.pos += 1;
            children = self.block(true);
            end = self.close_block()? + 1;
        }
        Ok(Stmt::Node(NodeAst { kind, id, id_span, name, props, insert_at, children, span: Span::new(start, end) }))
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
                        None => return Err(self.err(span, format!("неизвестная кривая «{e}»: linear, ease-in, ease-out, ease-in-out, hold, spring, cubic"))),
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
