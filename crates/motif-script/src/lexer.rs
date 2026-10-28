use crate::{ScriptError, Span};
use motif_core::Color;

#[derive(Clone, Debug, PartialEq)]
pub enum Tok {
    /// Identifier or keyword; may contain `{expr}` templates (`coin{i}`).
    Ident(String),
    Num(f64),
    /// Number with an `s` suffix: `1.2s`.
    Seconds(f64),
    /// `480x360`.
    Size(f64, f64),
    Color(Color),
    Str(String),
    /// Inline expression `{ i * 24 }`, without the braces.
    Expr(String),
    /// `{` that ends a line and opens a block.
    BlockOpen,
    BlockClose,
    Colon,
    Pipe,
    Dot,
    DotDot,
    Eq,
    /// `->` between two states of a machine.
    Arrow,
    /// A comparison in a transition condition: `>`, `>=`, `<`, `<=`, `==`, `!=`.
    Cmp(&'static str),
    Newline,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Token {
    pub tok: Tok,
    pub span: Span,
}

fn is_ident_start(c: char) -> bool {
    c.is_alphabetic() || c == '_'
}

fn is_ident_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_' || c == '-'
}

pub fn lex(src: &str) -> Result<Vec<Token>, ScriptError> {
    let chars: Vec<(usize, char)> = src.char_indices().collect();
    let at = |i: usize| chars.get(i).map(|c| c.1);
    let off = |i: usize| chars.get(i).map_or(src.len(), |c| c.0);
    let mut out = Vec::new();
    let mut i = 0;
    let mut line_start = true;

    while i < chars.len() {
        let c = chars[i].1;
        let start = off(i);
        if c == '\n' {
            out.push(Token { tok: Tok::Newline, span: Span::new(start, start + 1) });
            line_start = true;
            i += 1;
            continue;
        }
        if c.is_whitespace() {
            i += 1;
            continue;
        }
        // `#` as the first character of a line is a comment; elsewhere it starts a color.
        let comment = (c == '/' && at(i + 1) == Some('/')) || (c == '#' && line_start);
        if comment {
            while i < chars.len() && chars[i].1 != '\n' {
                i += 1;
            }
            continue;
        }
        line_start = false;

        let single = |t: Tok| Token { tok: t, span: Span::new(start, start + c.len_utf8()) };
        match c {
            ':' => {
                out.push(single(Tok::Colon));
                i += 1;
            }
            '|' => {
                out.push(single(Tok::Pipe));
                i += 1;
            }
            '-' if at(i + 1) == Some('>') => {
                out.push(Token { tok: Tok::Arrow, span: Span::new(start, start + 2) });
                i += 2;
            }
            '<' | '>' | '!' | '=' => {
                let two = at(i + 1) == Some('=');
                let op = match (c, two) {
                    ('<', false) => "<",
                    ('<', true) => "<=",
                    ('>', false) => ">",
                    ('>', true) => ">=",
                    ('=', true) => "==",
                    ('!', true) => "!=",
                    ('=', false) => {
                        out.push(single(Tok::Eq));
                        i += 1;
                        continue;
                    }
                    _ => return Err(ScriptError::at(src, start, "неожиданный символ «!»: сравнение «не равно» пишется как !=")),
                };
                let len = if two { 2 } else { 1 };
                out.push(Token { tok: Tok::Cmp(op), span: Span::new(start, start + len) });
                i += len;
            }
            '}' => {
                out.push(single(Tok::BlockClose));
                i += 1;
            }
            '.' if at(i + 1) == Some('.') => {
                out.push(Token { tok: Tok::DotDot, span: Span::new(start, start + 2) });
                i += 2;
            }
            '.' if !at(i + 1).is_some_and(|c| c.is_ascii_digit()) => {
                out.push(single(Tok::Dot));
                i += 1;
            }
            '{' => {
                // A brace followed only by blanks or a comment until the end of the line opens a block.
                let mut j = i + 1;
                while at(j).is_some_and(|c| c == ' ' || c == '\t' || c == '\r') {
                    j += 1;
                }
                let ends_line = match at(j) {
                    None | Some('\n') => true,
                    Some('/') => at(j + 1) == Some('/'),
                    _ => false,
                };
                if ends_line {
                    out.push(single(Tok::BlockOpen));
                    i += 1;
                } else {
                    let (body, next) = read_braced(&chars, i).ok_or_else(|| ScriptError::at(src, start, "не закрыта фигурная скобка выражения"))?;
                    out.push(Token { tok: Tok::Expr(body), span: Span::new(start, off(next)) });
                    i = next;
                }
            }
            '"' => {
                let mut j = i + 1;
                let mut s = String::new();
                while let Some(ch) = at(j) {
                    if ch == '"' || ch == '\n' {
                        break;
                    }
                    // `\"` is a quote inside the string: `hide "Red \"front\""`.
                    if ch == '\\' && at(j + 1) == Some('"') {
                        s.push('"');
                        j += 2;
                        continue;
                    }
                    s.push(ch);
                    j += 1;
                }
                if at(j) != Some('"') {
                    return Err(ScriptError::at(src, start, "не закрыта кавычка"));
                }
                out.push(Token { tok: Tok::Str(s), span: Span::new(start, off(j + 1)) });
                i = j + 1;
            }
            '#' => {
                let mut j = i + 1;
                while at(j).is_some_and(|c| c.is_ascii_alphanumeric()) {
                    j += 1;
                }
                let text = &src[start..off(j)];
                let color =
                    Color::from_hex(text).ok_or_else(|| ScriptError::at(src, start, format!("неверный цвет «{text}», нужен #rgb, #rrggbb или #rrggbbaa")))?;
                out.push(Token { tok: Tok::Color(color), span: Span::new(start, off(j)) });
                i = j;
            }
            c if c.is_ascii_digit() || (c == '-' && at(i + 1).is_some_and(|d| d.is_ascii_digit() || d == '.')) || c == '.' => {
                let mut j = read_number(&chars, i);
                let value = src[start..off(j)].parse::<f64>().map_err(|_| ScriptError::at(src, start, "неверное число"))?;
                if at(j) == Some('x') && at(j + 1).is_some_and(|d| d.is_ascii_digit()) {
                    let h_start = j + 1;
                    let k = read_number(&chars, h_start);
                    let h = src[off(h_start)..off(k)].parse::<f64>().map_err(|_| ScriptError::at(src, start, "неверный размер"))?;
                    out.push(Token { tok: Tok::Size(value, h), span: Span::new(start, off(k)) });
                    i = k;
                } else if at(j) == Some('s') && !at(j + 1).is_some_and(is_ident_char) {
                    j += 1;
                    out.push(Token { tok: Tok::Seconds(value), span: Span::new(start, off(j)) });
                    i = j;
                } else {
                    out.push(Token { tok: Tok::Num(value), span: Span::new(start, off(j)) });
                    i = j;
                }
            }
            c if is_ident_start(c) => {
                let mut j = i;
                let mut s = String::new();
                loop {
                    match at(j) {
                        // `Idle->Run`: the arrow is not part of the name.
                        Some('-') if at(j + 1) == Some('>') => break,
                        Some(ch) if is_ident_char(ch) => {
                            s.push(ch);
                            j += 1;
                        }
                        Some('{') => {
                            let (body, next) = read_braced(&chars, j).ok_or_else(|| ScriptError::at(src, off(j), "не закрыта фигурная скобка в имени"))?;
                            s.push('{');
                            s.push_str(&body);
                            s.push('}');
                            j = next;
                        }
                        _ => break,
                    }
                }
                out.push(Token { tok: Tok::Ident(s), span: Span::new(start, off(j)) });
                i = j;
            }
            other => return Err(ScriptError::at(src, start, format!("неожиданный символ «{other}»"))),
        }
    }
    Ok(out)
}

/// Reads digits with an optional sign and fraction; stops before `..`.
fn read_number(chars: &[(usize, char)], mut i: usize) -> usize {
    if chars.get(i).map(|c| c.1) == Some('-') {
        i += 1;
    }
    let mut seen_dot = false;
    while let Some(&(_, c)) = chars.get(i) {
        if c.is_ascii_digit() {
            i += 1;
        } else if c == '.' && !seen_dot && chars.get(i + 1).is_some_and(|n| n.1.is_ascii_digit()) {
            seen_dot = true;
            i += 1;
        } else {
            break;
        }
    }
    i
}

/// Reads a `{ ... }` group starting at `i` (which must be `{`); returns the inner text and the index after `}`.
fn read_braced(chars: &[(usize, char)], i: usize) -> Option<(String, usize)> {
    let mut depth = 0;
    let mut body = String::new();
    let mut j = i;
    while let Some(&(_, c)) = chars.get(j) {
        match c {
            '{' => {
                depth += 1;
                if depth > 1 {
                    body.push(c);
                }
            }
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some((body, j + 1));
                }
                body.push(c);
            }
            '\n' => return None,
            _ => body.push(c),
        }
        j += 1;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn toks(s: &str) -> Vec<Tok> {
        lex(s).unwrap().into_iter().map(|t| t.tok).collect()
    }

    #[test]
    fn lexes_mixed_line() {
        assert_eq!(
            toks("rect a at 10 -2.5 size 4x3 fill #fff // hi\n"),
            vec![
                Tok::Ident("rect".into()),
                Tok::Ident("a".into()),
                Tok::Ident("at".into()),
                Tok::Num(10.0),
                Tok::Num(-2.5),
                Tok::Ident("size".into()),
                Tok::Size(4.0, 3.0),
                Tok::Ident("fill".into()),
                Tok::Color(Color::WHITE),
                Tok::Newline,
            ]
        );
    }

    #[test]
    fn distinguishes_blocks_from_expressions() {
        assert_eq!(
            toks("for i in 0..5 {\n  ellipse c{i} at {i * 2} 0\n}"),
            vec![
                Tok::Ident("for".into()),
                Tok::Ident("i".into()),
                Tok::Ident("in".into()),
                Tok::Num(0.0),
                Tok::DotDot,
                Tok::Num(5.0),
                Tok::BlockOpen,
                Tok::Newline,
                Tok::Ident("ellipse".into()),
                Tok::Ident("c{i}".into()),
                Tok::Ident("at".into()),
                Tok::Expr("i * 2".into()),
                Tok::Num(0.0),
                Tok::Newline,
                Tok::BlockClose,
            ]
        );
    }

    #[test]
    fn hash_comment_only_at_line_start() {
        assert_eq!(toks("# note\nlet a = #000"), vec![Tok::Newline, Tok::Ident("let".into()), Tok::Ident("a".into()), Tok::Eq, Tok::Color(Color::BLACK)]);
    }

    #[test]
    fn arrows_and_comparisons() {
        assert_eq!(
            toks("Idle->Run when speed >= 0.5 and n != -1 a=1"),
            vec![
                Tok::Ident("Idle".into()),
                Tok::Arrow,
                Tok::Ident("Run".into()),
                Tok::Ident("when".into()),
                Tok::Ident("speed".into()),
                Tok::Cmp(">="),
                Tok::Num(0.5),
                Tok::Ident("and".into()),
                Tok::Ident("n".into()),
                Tok::Cmp("!="),
                Tok::Num(-1.0),
                Tok::Ident("a".into()),
                Tok::Eq,
                Tok::Num(1.0),
            ]
        );
    }

    #[test]
    fn seconds_and_paths() {
        assert_eq!(toks("1.2s ball.y 0.6:"), vec![Tok::Seconds(1.2), Tok::Ident("ball".into()), Tok::Dot, Tok::Ident("y".into()), Tok::Num(0.6), Tok::Colon]);
    }
}
