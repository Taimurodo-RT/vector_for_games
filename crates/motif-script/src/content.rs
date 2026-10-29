//! Lossless text content strings. Other quoted MSC strings keep their path semantics.

/// Quote a `content` value for MSC without changing its text, quotes or whitespace.
/// This is intentionally separate from quoting names and file paths: only `content`
/// interprets `\\`, `\n`, `\t` and `\r` when a scene is evaluated.
pub fn quoted_content(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for ch in text.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            other => out.push(other),
        }
    }
    out.push('"');
    out
}

pub(crate) fn decode_content(encoded: &str) -> String {
    let mut out = String::with_capacity(encoded.len());
    let mut chars = encoded.chars();
    while let Some(ch) = chars.next() {
        if ch != '\\' {
            out.push(ch);
            continue;
        }
        match chars.next() {
            Some('\\') => out.push('\\'),
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('r') => out.push('\r'),
            Some(other) => {
                out.push('\\');
                out.push(other);
            }
            None => out.push('\\'),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        lexer::{Tok, lex},
        parse,
    };

    fn read_content(text: &str) -> String {
        let src = format!("artboard 200x200\ntext label content {}\n", quoted_content(text));
        let scene = parse(&src).unwrap_or_else(|errors| panic!("{src}: {errors:?}"));
        scene.doc.node("label").unwrap().text.as_ref().unwrap().content.clone()
    }

    #[test]
    fn content_round_trip_preserves_literal_escapes_whitespace_quotes_and_trailing_slashes() {
        for text in [
            "",
            r"literal \n and \t and \r",
            "real\nnewline\ttab\rreturn",
            r#"say "quoted" and backslash \"#,
            "ending \\",
            "ending \\\\",
            "quote after slash \\\"",
            r"unknown \q and C:\new\tabs\font.ttf",
            "日本語 — Кириллица\n\t<&> \"\"",
        ] {
            assert_eq!(read_content(text), text, "{text:?}");
        }
        assert_ne!(read_content(r"literal \n"), read_content("literal \n"));
    }

    #[test]
    fn legacy_newline_and_unknown_escapes_keep_their_meaning() {
        let src = r#"artboard 200x200
text label content "line one\n\tline two\r\q"
"#;
        let scene = parse(src).unwrap();
        assert_eq!(scene.doc.node("label").unwrap().text.as_ref().unwrap().content, "line one\n\tline two\r\\q");
    }

    #[test]
    fn file_paths_and_other_quoted_strings_are_not_content_decoded() {
        let source = r#"font "C:\new\tabs\font.ttf"
backdrop "\\server\new\reference.png"
artboard "C:\new\tabs"
"#;
        let strings: Vec<_> = lex(source)
            .unwrap()
            .into_iter()
            .filter_map(|t| match t.tok {
                Tok::Str(s) => Some(s),
                _ => None,
            })
            .collect();
        assert_eq!(strings, vec![r"C:\new\tabs\font.ttf", r"\\server\new\reference.png", r"C:\new\tabs"]);
        assert_eq!(crate::fonts::named(source).unwrap(), vec![r"C:\new\tabs\font.ttf"]);
        let scene = parse(
            r#"artboard "C:\new\tabs" 200x200
backdrop "\\server\new\reference.png"
"#,
        )
        .unwrap();
        assert_eq!(scene.doc.artboard.name, r"C:\new\tabs");
        assert_eq!(scene.backdrop.unwrap().file, r"\\server\new\reference.png");
    }

    #[test]
    fn a_real_font_file_path_with_new_and_tabs_directories_still_loads() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        static SEQUENCE: AtomicUsize = AtomicUsize::new(0);
        let temp = std::env::temp_dir();
        let root = temp.join(format!("motif-content-font-{}-{}", std::process::id(), SEQUENCE.fetch_add(1, Ordering::Relaxed)));
        let parent = root.join("new").join("tabs");
        std::fs::create_dir_all(&parent).unwrap();
        let file = parent.join("regular.ttf");
        std::fs::write(&file, motif_core::text::DEFAULT_FONT).unwrap();
        let path = file.to_string_lossy();
        let source = format!("artboard 200x200\ntext label content {} font \"{path}\"\n", quoted_content("font sample"));
        let parsed = parse(&source).unwrap();
        assert_eq!(crate::fonts::named(&source).unwrap(), vec![path.as_ref()]);
        assert_eq!(parsed.doc.node("label").unwrap().text.as_ref().unwrap().content, "font sample");
        let resolved = root.canonicalize().unwrap();
        let temp = temp.canonicalize().unwrap();
        assert!(resolved.starts_with(&temp) && resolved != temp);
        std::fs::remove_dir_all(resolved).unwrap();
    }
}
