//! A scene as one file, and a symbol taken out of its scene into a file of its own with all it
//! needs: the names of the palette, the styles, the symbols it uses, the lines the themes have
//! for them, and the font files it names. That file opens in any other project.

use crate::lexer::{Tok, Token};
use crate::parser::{Parser, Stmt};
use std::ops::Range;
use std::path::{Component, Path, PathBuf};

/// A scene written without `include` lines.
#[derive(Clone, Debug, PartialEq)]
pub struct Bundled {
    pub text: String,
    /// The font files the text names, as written: paths from the folder of the scene.
    pub fonts: Vec<String>,
    /// The files whose lines went into the text, as the `include` lines named them.
    pub files: Vec<String>,
}

/// A symbol in a file of its own.
#[derive(Clone, Debug, PartialEq)]
pub struct Extracted {
    pub text: String,
    /// The font files it names: the path from the folder of the scene, and the path the text
    /// has for it from the folder of the new file.
    pub fonts: Vec<(String, String)>,
    /// What went with the symbol, by name: colors, numbers and gradients of the palette, styles,
    /// the other symbols it uses, and the themes that have lines for them.
    pub palette: Vec<String>,
    pub styles: Vec<String>,
    pub symbols: Vec<String>,
    pub themes: Vec<String>,
}

/// The font files `toks` name: `font "my.ttf"`, not the built-in names.
fn fonts_of(toks: &[Token], out: &mut Vec<String>) {
    for pair in toks.windows(2) {
        if let (Tok::Ident(key), Tok::Str(font)) = (&pair[0].tok, &pair[1].tok)
            && key == "font"
            && !crate::fonts::builtin_name(font)
            && !out.contains(font)
        {
            out.push(font.clone());
        }
    }
}

/// Returns the scene `src`, saved in `base`, as one file: the lines its `include` files give
/// are written in their place.
pub fn bundle_scene(src: &str, base: Option<&Path>) -> Result<Bundled, String> {
    let text = crate::inline_includes(src, base)?;
    let mut files: Vec<String> = Vec::new();
    for site in crate::file_includes(src, base) {
        if !files.contains(&site.path) {
            files.push(site.path);
        }
    }
    let mut fonts = Vec::new();
    fonts_of(&crate::lexer::lex(&text).map_err(|e| e.to_string())?, &mut fonts);
    Ok(Bundled { text, fonts, files })
}

/// Copies the font files a new file names from the folder of the scene to the folder of the
/// new file: `fonts` are (path from `from`, path from `to`). Returns the names of the files
/// that were not copied: missing, or named by a path that leaves the folder.
pub fn copy_fonts(fonts: &[(String, String)], from: &Path, to: &Path) -> Vec<String> {
    let mut left = Vec::new();
    for (was, now) in fonts {
        let (source, target) = (from.join(was), to.join(now));
        let inside = Path::new(now).components().all(|c| matches!(c, Component::Normal(_) | Component::CurDir));
        let same = std::fs::canonicalize(&source).ok().is_some_and(|s| std::fs::canonicalize(&target).ok() == Some(s));
        if same {
            continue;
        }
        let made = inside && target.parent().is_none_or(|dir| std::fs::create_dir_all(dir).is_ok()) && std::fs::copy(&source, &target).is_ok();
        if !made {
            left.push(was.clone());
        }
    }
    left
}

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Let,
    Style,
    Symbol,
    Theme,
}

/// A top-level statement that can go with a symbol.
struct Item<'a> {
    kind: Kind,
    name: &'a str,
    toks: Range<usize>,
    stmt: &'a Stmt,
}

/// What the tokens `toks` name: names of the palette, styles (`style card`) and symbols
/// (`use Coin`), as indexes into `items`.
fn named(toks: &[Token], items: &[Item], out: &mut Vec<usize>) {
    for (k, t) in toks.iter().enumerate() {
        let Tok::Ident(word) = &t.tok else { continue };
        let before = k.checked_sub(1).and_then(|p| match &toks[p].tok {
            Tok::Ident(w) => Some(w.as_str()),
            _ => None,
        });
        for (i, item) in items.iter().enumerate() {
            let fits = match item.kind {
                Kind::Let => true,
                Kind::Style => before == Some("style"),
                Kind::Symbol => before == Some("use"),
                Kind::Theme => false,
            };
            if fits && item.name == word && !out.contains(&i) {
                out.push(i);
            }
        }
    }
}

/// The comment lines right above byte `at` of `text`: they go with the statement.
fn comment_above(text: &str, at: usize) -> usize {
    let mut start = text[..at].rfind('\n').map_or(0, |i| i + 1);
    while start > 0 {
        let prev = text[..start - 1].rfind('\n').map_or(0, |i| i + 1);
        if !text[prev..start - 1].trim_start().starts_with("//") {
            break;
        }
        start = prev;
    }
    start
}

/// Symbol `symbol` of the script `text` with what it needs. `None` when `text` has no such
/// symbol on its top level.
fn take(text: &str, base: Option<&Path>, symbol: &str) -> Option<Result<Extracted, String>> {
    let all = match crate::lexer::lex(text) {
        Ok(all) => all,
        Err(e) => return Some(Err(e.to_string())),
    };
    let mut parser = Parser::new(text, all.clone()).in_folder(base);
    let stmts = parser.block(false);
    if let Some(e) = parser.errors.first() {
        return Some(Err(e.to_string()));
    }
    let items: Vec<Item> = stmts
        .iter()
        .zip(&parser.top)
        .filter_map(|(stmt, toks)| {
            let (kind, name) = match stmt {
                Stmt::Let { name, .. } | Stmt::LetGradient { name, .. } => (Kind::Let, name),
                Stmt::Style { name, .. } => (Kind::Style, name),
                Stmt::Symbol { name, .. } => (Kind::Symbol, name),
                Stmt::Theme { name, .. } => (Kind::Theme, name),
                _ => return None,
            };
            Some(Item { kind, name, toks: toks.clone(), stmt })
        })
        .collect();
    let first = items.iter().position(|i| i.kind == Kind::Symbol && i.name == symbol)?;
    // The lines of a theme for what is kept bring the names they use themselves.
    let entry_toks = |span: crate::Span| -> Vec<Token> { all.iter().filter(|t| span.start <= t.span.start && t.span.end <= span.end).cloned().collect() };
    let kept_entries = |kept: &[usize]| -> Vec<(usize, crate::Span)> {
        let has = |kind: Kind, name: &str| kept.iter().any(|&k| items[k].kind == kind && items[k].name == name);
        let mut out = Vec::new();
        for (i, item) in items.iter().enumerate() {
            let Stmt::Theme { entries, .. } = item.stmt else { continue };
            for e in entries {
                let style = matches!(e.value, crate::parser::ThemeValueAst::Style { .. });
                if has(if style { Kind::Style } else { Kind::Let }, &e.name) {
                    out.push((i, e.span));
                }
            }
        }
        out
    };
    let mut kept = vec![first];
    loop {
        let before = kept.len();
        for k in kept.clone() {
            named(&all[items[k].toks.clone()], &items, &mut kept);
        }
        for (_, span) in kept_entries(&kept) {
            // The first word of a theme line is the name it sets, which is kept already.
            named(&entry_toks(span), &items, &mut kept);
        }
        if kept.len() == before {
            break;
        }
    }
    let entries = kept_entries(&kept);
    let (mut out, mut fonts, mut themes) = (String::new(), Vec::new(), Vec::new());
    for (i, item) in items.iter().enumerate() {
        let toks = &all[item.toks.clone()];
        if let Stmt::Theme { name, .. } = item.stmt {
            let lines: Vec<&str> = entries.iter().filter(|(t, _)| *t == i).map(|(_, span)| &text[span.start..span.end]).collect();
            if !lines.is_empty() {
                out.push_str(&format!("theme {name} {{\n"));
                for line in lines {
                    out.push_str(&format!("  {line}\n"));
                }
                out.push_str("}\n");
                if !themes.contains(name) {
                    themes.push(name.clone());
                }
            }
        } else if kept.contains(&i) {
            let start = comment_above(text, toks[0].span.start);
            out.push_str(&text[start..toks[toks.len() - 1].span.end]);
            out.push('\n');
            fonts_of(toks, &mut fonts);
        }
    }
    // A font beside the scene stays beside the new file; one from another folder moves in.
    let fonts: Vec<(String, String)> = fonts
        .into_iter()
        .map(|was| {
            let plain = Path::new(&was).components().all(|c| matches!(c, Component::Normal(_) | Component::CurDir));
            let now = if plain { was.clone() } else { Path::new(&was).file_name().map_or_else(|| was.clone(), |n| n.to_string_lossy().into_owned()) };
            (was, now)
        })
        .collect();
    let names = |kind: Kind| -> Vec<String> {
        let mut names: Vec<String> = Vec::new();
        for (i, item) in items.iter().enumerate() {
            if kept.contains(&i) && item.kind == kind && i != first && !names.iter().any(|n| n == item.name) {
                names.push(item.name.to_string());
            }
        }
        names
    };
    // A sheet with one copy, sized to it: the file shows the symbol when it is opened.
    let id = "sample";
    let sheet = |w: f64, h: f64, x: f64, y: f64| {
        let n = crate::fmt_num;
        format!("\nartboard \"{symbol}\" {}x{}\nuse {symbol} {id} at {} {}\n", n(w), n(h), n(x), n(y))
    };
    let told = |e: Vec<crate::ScriptError>| format!("символ «{symbol}» не читается отдельно от сцены, строка {}: {}", e[0].line, e[0].message);
    let probe = match crate::parse_in(&format!("{out}{}", sheet(64.0, 64.0, 0.0, 0.0)), base) {
        Ok(scene) => scene,
        Err(e) => return Some(Err(told(e))),
    };
    let bounds = probe.doc.index_of(id).and_then(|i| probe.doc.world_bounds(i));
    let pad = 24.0;
    let page = match bounds {
        Some(b) => sheet((b.width() + 2.0 * pad).ceil(), (b.height() + 2.0 * pad).ceil(), (pad - b.x0).round(), (pad - b.y0).round()),
        None => sheet(64.0, 64.0, 32.0, 32.0),
    };
    // Measured with the fonts where the scene has them; written with the paths of the new file.
    for (was, now) in &fonts {
        if was != now {
            out = out.replace(&format!("\"{was}\""), &format!("\"{now}\""));
        }
    }
    let text = crate::format::stamp(&format!("// Символ {symbol} со всем, что ему нужно: его берут строкой include или открывают как сцену.\n{out}{page}"));
    Some(Ok(Extracted { text, fonts, palette: names(Kind::Let), styles: names(Kind::Style), symbols: names(Kind::Symbol), themes }))
}

/// Returns symbol `symbol` of the scene `src`, saved in `base`, as a file of its own with the
/// names of the palette, the styles and the symbols it uses, wherever they are written: in the
/// scene or in the files it includes.
pub fn extract_symbol(src: &str, base: Option<&Path>, symbol: &str) -> Result<Extracted, String> {
    let whole = crate::inline_includes(src, base)?;
    let (head, ranges) = crate::boards(&whole);
    // A symbol of a file with several boards is in its head, or with one of the boards.
    let texts: Vec<String> = if ranges.len() > 1 {
        std::iter::once(whole[head.clone()].to_string()).chain(ranges.iter().map(|r| format!("{}{}", &whole[head.clone()], &whole[r.clone()]))).collect()
    } else {
        vec![whole.clone()]
    };
    texts.iter().find_map(|t| take(t, base, symbol)).unwrap_or_else(|| Err(format!("в сцене нет символа «{symbol}»")))
}

/// Where the file of a symbol or of a bundled scene is written, with a name made from `name`.
pub fn file_name_for(name: &str) -> PathBuf {
    let word: String = name.chars().flat_map(char::to_lowercase).filter(|c| c.is_alphanumeric() || *c == '_' || *c == '-').collect();
    PathBuf::from(format!("{}.msc", if word.is_empty() { "symbol" } else { &word }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse_in;

    fn folder_of(test: &str, files: &[(&str, &str)]) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("motif-extract-{}-{test}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        for (name, text) in files {
            let file = dir.join(name);
            std::fs::create_dir_all(file.parent().unwrap()).unwrap();
            std::fs::write(file, text).unwrap();
        }
        dir
    }

    const KIT: &str = "let ink = #2a2230
let paper = #f6ecd9
let accent = #e2573b
let unused = #00ff00
let glow = linear 90 accent paper
style caption fill ink font-size 13
style card fill paper stroke ink 3
theme night {
  ink = #f1e7d3
  unused = #ff00ff
  style caption fill ink font-size 13 opacity 0.8
  style card fill #000000
}
// Значок: кружок.
symbol Dot size=10 {
  ellipse c size {size} {size} gradient glow stroke ink 2
}
symbol Lonely {
  rect r size 4 4 fill unused
}
// Кнопка с подписью
// и значком.
symbol Button face=#e2573b {
  variant main face=accent
  rect body size 120 40 r 10 fill face stroke ink 3
  text label content \"Кнопка\" style caption
  use Dot mark at -44 0 size=12
}
";

    const SCREEN: &str = "motif 7
include \"kit.msc\"
let cold = #4b7bd1
// Строка меню:
// кнопка и черта.
symbol Row {
  use Button go at 0 0 variant main
  rect line at 0 30 size 120 2 fill cold
}
artboard \"Экран\" 300x200 bg paper
use Row r1 at 150 100
";

    #[test]
    fn a_scene_is_bundled_into_one_file() {
        let dir = folder_of("bundle", &[("kit.msc", KIT), ("screen.msc", SCREEN)]);
        let b = bundle_scene(SCREEN, Some(&dir)).unwrap();
        assert_eq!(b.files, ["kit.msc"]);
        assert!(!b.text.contains("include") && b.text.contains("symbol Button"), "{}", b.text);
        // It reads without the folder, and draws the same scene.
        let (one, two) = (parse_in(&b.text, None).unwrap(), parse_in(SCREEN, Some(&dir)).unwrap());
        assert_eq!(one.doc.nodes.len(), two.doc.nodes.len());
        assert_eq!(one.doc.node("r1-go-body").unwrap().fill, two.doc.node("r1-go-body").unwrap().fill);
        assert!(one.includes.is_empty() && one.doc.includes.is_empty());
        // A scene that includes nothing is bundled as it is.
        assert_eq!(bundle_scene(&b.text, None).unwrap().text, b.text);
    }

    #[test]
    fn a_symbol_leaves_with_what_it_needs_and_nothing_else() {
        let dir = folder_of("symbol", &[("kit.msc", KIT), ("screen.msc", SCREEN)]);
        let e = extract_symbol(SCREEN, Some(&dir), "Row").unwrap();
        assert_eq!(e.symbols, ["Dot", "Button"]);
        assert_eq!(e.palette, ["ink", "paper", "accent", "glow", "cold"]);
        assert_eq!((e.styles.clone(), e.themes.clone()), (vec!["caption".to_string()], vec!["night".to_string()]));
        // What nothing of it names stays behind: a color, a style, a symbol, their theme lines.
        for gone in ["unused", "Lonely", "style card", "include \"", "Экран"] {
            assert!(!e.text.contains(gone), "{gone}: {}", e.text);
        }
        // The theme keeps its lines for what went, and comments go with their statements.
        assert!(e.text.contains("theme night {\n  ink = #f1e7d3\n  style caption fill ink font-size 13 opacity 0.8\n}\n"), "{}", e.text);
        assert!(e.text.contains("// Строка меню:\n// кнопка и черта.\nsymbol Row {"), "{}", e.text);
        // The file reads on its own, shows one copy on a sheet sized to it, and gives the symbol.
        // Its version is the lowest that reads it: nothing in it is newer than variants.
        assert!(e.text.starts_with("motif 5\n"), "{}", e.text);
        let scene = parse_in(&e.text, None).unwrap();
        let (ab, b) = (&scene.doc.artboard, scene.doc.world_bounds(scene.doc.index_of("sample").unwrap()).unwrap());
        assert_eq!((ab.name.as_str(), ab.width, ab.height), ("Row", 168.0, 99.0));
        assert!(b.x0 >= 23.0 && b.y0 >= 23.0 && b.x1 <= ab.width - 23.0 && b.y1 <= ab.height - 23.0, "{b:?}");
        std::fs::write(dir.join("row.msc"), &e.text).unwrap();
        let lib = crate::library_file(&dir.join("row.msc"));
        assert_eq!(lib.symbols.iter().map(|s| s.name.as_str()).collect::<Vec<_>>(), ["Dot", "Button", "Row"]);
        // A symbol the scene does not have, and a symbol of the kit alone.
        assert!(extract_symbol(SCREEN, Some(&dir), "Nope").unwrap_err().contains("нет символа «Nope»"));
        let dot = extract_symbol(SCREEN, Some(&dir), "Dot").unwrap();
        assert_eq!(
            (dot.palette.clone(), dot.symbols.is_empty(), dot.styles.is_empty()),
            (vec!["ink".to_string(), "paper".into(), "accent".into(), "glow".into()], true, true)
        );
    }

    #[test]
    fn a_symbol_of_one_board_and_its_fonts_are_found() {
        let dir = folder_of("fonts", &[("fonts/pix.ttf", "not a font"), ("../motif-extract-shared.ttf", "not a font")]);
        let src = "motif 5\nlet ink = #111111\nartboard \"A\" 64x64\nrect a size 4 4\n\nartboard \"B\" 64x64\nsymbol Tag {\n  text t content \"x\" font \"fonts/pix.ttf\" fill ink\n  text u content \"y\" font \"../motif-extract-shared.ttf\"\n}\nuse Tag t1 at 10 10\n";
        // The fonts are not real: the symbol is taken by its lines, the sheet is not measured.
        let all = crate::lexer::lex(src).unwrap();
        let mut fonts = Vec::new();
        fonts_of(&all, &mut fonts);
        assert_eq!(fonts, ["fonts/pix.ttf", "../motif-extract-shared.ttf"]);
        let to = dir.join("out");
        let left = copy_fonts(
            &[
                ("fonts/pix.ttf".into(), "fonts/pix.ttf".into()),
                ("../motif-extract-shared.ttf".into(), "motif-extract-shared.ttf".into()),
                ("gone.ttf".into(), "gone.ttf".into()),
            ],
            &dir,
            &to,
        );
        assert_eq!(left, ["gone.ttf"]);
        assert!(to.join("fonts/pix.ttf").exists() && to.join("motif-extract-shared.ttf").exists());
        assert_eq!(file_name_for("Кнопка Go!"), PathBuf::from("кнопкаgo.msc"));
        // A font from another folder moves in beside the new file, and the text names it there.
        let mono = concat!(env!("CARGO_MANIFEST_DIR"), "/assets/fonts/LiberationMono-Regular.ttf");
        std::fs::create_dir_all(dir.join("proj")).unwrap();
        std::fs::create_dir_all(dir.join("shared")).unwrap();
        std::fs::copy(mono, dir.join("shared/mono.ttf")).unwrap();
        let scene = "symbol Label {\n  text t content \"x\" font \"../shared/mono.ttf\" font-size 12\n}\nartboard \"A\" 64x64\nuse Label l1 at 10 10\n";
        let e = extract_symbol(scene, Some(&dir.join("proj")), "Label").unwrap();
        assert_eq!(e.fonts, [("../shared/mono.ttf".to_string(), "mono.ttf".to_string())]);
        assert!(e.text.contains("font \"mono.ttf\"") && !e.text.contains("shared"), "{}", e.text);
        assert!(copy_fonts(&e.fonts, &dir.join("proj"), &dir.join("taken")).is_empty());
        assert!(parse_in(&e.text, Some(&dir.join("taken"))).is_ok());
        // A symbol written with one board of several is found there, with the head it uses.
        let boards = "motif 5\nlet ink = #111111\nlet other = #222222\nartboard \"A\" 64x64\nrect a size 4 4\n\nartboard \"B\" 64x64\nsymbol Tag {\n  rect t size 20 8 fill ink\n}\nuse Tag t1 at 10 10\n";
        let e = extract_symbol(boards, None, "Tag").unwrap();
        assert_eq!(e.palette, ["ink"]);
        assert!(parse_in(&e.text, None).is_ok() && !e.text.contains("other") && e.text.contains("artboard \"Tag\" 68x56"), "{}", e.text);
    }
}
