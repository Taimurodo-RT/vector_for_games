//! Syntax colouring for the script panel.

use crate::theme::{self, ACCENT, DANGER, FAINT, FG, GOOD, MUTED, SEL};
use egui::Color32;
use egui::text::{LayoutJob, TextFormat};

const KEYWORDS: &[&str] = &[
    "artboard", "backdrop", "let", "group", "for", "in", "anim", "override", "loop", "symbol", "use", "machine", "input", "state", "any", "when", "and", "not",
    "rect", "ellipse", "star", "polygon", "path", "text",
];
const PROPS: &[&str] = &[
    "at",
    "size",
    "r",
    "points",
    "inner",
    "rot",
    "scale",
    "opacity",
    "fill",
    "stroke",
    "hidden",
    "bg",
    "none",
    "d",
    "gradient",
    "clip",
    "skew",
    "pivot",
    "dash",
    "cap",
    "join",
    "shadow",
    "blend",
    "evenodd",
    "outline",
    "content",
    "font",
    "font-size",
    "letter-spacing",
    "line-height",
    "align",
    "reveal",
    "wave",
    "phase",
    "blur",
    "elevation",
    "light",
    "end",
    "trigger",
    "true",
    "false",
];

/// Curve names, including the easings.net library and the words that take numbers.
fn is_ease(word: &str) -> bool {
    matches!(word, "cubic" | "steps") || motif_core::Ease::from_keyword(word).is_some()
}
const NUMBER: Color32 = Color32::from_rgb(0xff, 0xb8, 0x6b);
const PROP: Color32 = Color32::from_rgb(0xc7, 0x9b, 0xff);

/// Lays out the script with colours. `error_lines` are 1-based; `selected` is the byte span of the
/// selected node's declaration, tinted so that the object is easy to find in the text.
pub fn highlight(text: &str, error_lines: &[usize], selected: Option<(usize, usize)>) -> LayoutJob {
    let mut job = LayoutJob::default();
    job.wrap.max_width = f32::INFINITY;
    let font = theme::mono(13.0);
    let mut line_start = 0;
    for (line_no, line) in (1..).zip(text.split_inclusive('\n')) {
        let err = error_lines.contains(&line_no);
        for (s, e, color) in tokens(line) {
            let (a, b) = (line_start + s, line_start + e);
            // Split the token where the selection starts or ends so that the tint is exact.
            let mut cuts = vec![a, b];
            if let Some((ss, se)) = selected {
                for c in [ss, se] {
                    if c > a && c < b {
                        cuts.push(c);
                    }
                }
            }
            cuts.sort_unstable();
            for w in cuts.windows(2) {
                let in_sel = selected.is_some_and(|(ss, se)| w[0] >= ss && w[1] <= se);
                let background = if err {
                    DANGER.gamma_multiply(0.16)
                } else if in_sel {
                    SEL.gamma_multiply(0.12)
                } else {
                    Color32::TRANSPARENT
                };
                job.append(&text[w[0]..w[1]], 0.0, TextFormat { font_id: font.clone(), color, background, ..Default::default() });
            }
        }
        line_start += line.len();
    }
    job
}

/// Splits one line into (start, end, colour) runs that cover it completely.
fn tokens(line: &str) -> Vec<(usize, usize, Color32)> {
    let b = line.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    let first = line.len() - line.trim_start().len();
    while i < b.len() {
        let c = b[i];
        let start = i;
        let color = if c == b'/' && b.get(i + 1) == Some(&b'/') || (c == b'#' && i == first && !is_hex_color(&line[i..])) {
            i = b.len();
            FAINT
        } else if c == b'"' {
            i += 1;
            while i < b.len() && b[i] != b'"' && b[i] != b'\n' {
                i += 1;
            }
            i = (i + 1).min(b.len());
            GOOD
        } else if c == b'#' {
            i += 1;
            while i < b.len() && b[i].is_ascii_hexdigit() {
                i += 1;
            }
            ACCENT
        } else if c.is_ascii_digit() || (c == b'-' && b.get(i + 1).is_some_and(u8::is_ascii_digit) && !prev_is_word(b, i)) {
            i += 1;
            while i < b.len() && (b[i].is_ascii_digit() || b[i] == b'.' && b.get(i + 1).is_some_and(u8::is_ascii_digit)) {
                i += 1;
            }
            // Units and sizes: 1.2s, 480x360.
            if i < b.len() && (b[i] == b's' || b[i] == b'x') && !b.get(i + 1).is_some_and(|n| n.is_ascii_alphabetic()) {
                i += 1;
            }
            NUMBER
        } else if c.is_ascii_alphabetic() || c == b'_' {
            while i < b.len() && (b[i].is_ascii_alphanumeric() || b[i] == b'_' || b[i] == b'-' && b.get(i + 1).is_some_and(u8::is_ascii_alphabetic)) {
                i += 1;
            }
            let word = &line[start..i];
            if KEYWORDS.contains(&word) {
                SEL
            } else if is_ease(word) {
                GOOD.gamma_multiply(0.8)
            } else if PROPS.contains(&word) {
                PROP
            } else {
                FG
            }
        } else if c.is_ascii() {
            i += 1;
            if matches!(c, b'{' | b'}' | b'|' | b':' | b'=' | b'.' | b'(' | b')') { MUTED } else { FG }
        } else {
            // Non-ASCII text (names in Cyrillic): copy the whole character.
            i += line[i..].chars().next().map_or(1, char::len_utf8);
            FG
        };
        out.push((start, i, color));
    }
    out
}

fn prev_is_word(b: &[u8], i: usize) -> bool {
    i > 0 && (b[i - 1].is_ascii_alphanumeric() || b[i - 1] == b'_')
}

fn is_hex_color(s: &str) -> bool {
    let hex = s[1..].bytes().take_while(u8::is_ascii_hexdigit).count();
    matches!(hex, 3 | 4 | 6 | 8)
}
