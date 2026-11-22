//! `include "kit.msc"`: a scene takes the colors, gradients, styles, themes and symbols of
//! another file. The file is a scene like any other: what it gives is what all of its boards
//! share, the lines of its top level (of its head, when it has several boards). Its boards and
//! their objects stay with it.
//!
//! The lines are read where the `include` line is, and every place in them is the place of that
//! line: the scene has nowhere else to show, and nothing of the other file can be changed from
//! here ([`touched`] is how an editor tells that an edit tried).

use crate::lexer::{Tok, Token};
use crate::parser::{Parser, Stmt};
use crate::{Scene, Span};
use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock, Mutex};

/// An `include` line of a scene.
#[derive(Clone, Debug, PartialEq)]
pub struct IncludeSite {
    /// The file as it is written: a path from the folder of the scene.
    pub path: String,
    /// The file it names.
    pub file: PathBuf,
    /// From `include` to the closing quote. Everything the file gives is written "here".
    pub span: Span,
    /// The quoted path.
    pub path_span: Span,
    /// Every file read for the line: the file itself and the files it includes in turn.
    pub files: Vec<PathBuf>,
    /// The file has mistakes of its own: the scene says so at the line.
    pub broken: bool,
}

impl Scene {
    /// The `include` line that gives what is written at `span`; `None` for what the scene
    /// writes itself. Nothing of the scene's own is written inside an `include` line, and a
    /// place of the other file is that line or a part of it (a block ends one byte in).
    pub fn included(&self, span: Span) -> Option<&IncludeSite> {
        self.includes.iter().find(|i| i.span.start <= span.start && span.end <= i.span.end)
    }
}

/// A file read for an `include`.
#[derive(Debug)]
pub struct Loaded {
    pub file: PathBuf,
    /// The real path of the file: one file is one file however it is named.
    key: PathBuf,
    text: String,
    /// What it gives, in the order written: its own lines and the files it includes.
    parts: Vec<Part>,
    /// The first mistake of the file read on its own.
    pub error: Option<String>,
    /// The file and every file it includes, each once.
    pub files: Vec<PathBuf>,
    /// The symbols written in the file itself, by name.
    pub symbols: Vec<String>,
}

#[derive(Debug)]
enum Part {
    /// Lines of the file itself: as the parser reads them and as they are written.
    Lines { toks: Vec<Token>, text: Vec<Piece> },
    /// A file it includes, and the folder of that file from this one (`kits/`, or nothing).
    File { lib: Arc<Loaded>, dir: String },
}

/// The text of lines, cut at each font file they name: its path is written from the folder of
/// the file, and is another path from another folder.
#[derive(Debug)]
enum Piece {
    Text(String),
    Font(String),
}

impl Loaded {
    /// Adds the lines the file gives to `out`. A file in `given` gave its lines already: two
    /// files that include the same third one bring it once.
    pub(crate) fn toks(&self, given: &mut Vec<PathBuf>, out: &mut Vec<Token>) {
        if given.contains(&self.key) {
            return;
        }
        given.push(self.key.clone());
        for p in &self.parts {
            match p {
                Part::Lines { toks, .. } => out.extend(toks.iter().cloned()),
                Part::File { lib, .. } => lib.toks(given, out),
            }
        }
    }

    /// The same lines as text, for a scene whose folder sees this file's folder as `dir`.
    fn text(&self, given: &mut Vec<PathBuf>, dir: &str, out: &mut String) {
        if given.contains(&self.key) {
            return;
        }
        given.push(self.key.clone());
        for p in &self.parts {
            match p {
                Part::Lines { text, .. } => {
                    for piece in text {
                        match piece {
                            Piece::Text(t) => out.push_str(t),
                            Piece::Font(f) if Path::new(f).is_absolute() => out.push_str(&format!("\"{}\"", f.replace('"', "\\\""))),
                            Piece::Font(f) => out.push_str(&format!("\"{dir}{}\"", f.replace('"', "\\\""))),
                        }
                    }
                }
                Part::File { lib, dir: inner } => lib.text(given, &format!("{dir}{inner}"), out),
            }
        }
    }
}

/// The folder of the file `written` names, as a start of paths: `kits/` for `kits/ui.msc`.
fn folder(written: &str) -> String {
    let path = written.trim().replace('\\', "/");
    path.rfind('/').map_or(String::new(), |i| path[..=i].to_string())
}

/// The file `written` names for a scene saved in `base`.
pub(crate) fn resolve(base: &Path, written: &str) -> Result<PathBuf, String> {
    let path = written.trim().replace('\\', "/");
    if path.is_empty() {
        return Err("после include — имя файла в кавычках: include \"kit.msc\"".to_string());
    }
    let drive = path.as_bytes().get(1) == Some(&b':');
    if path.starts_with('/') || drive {
        return Err(format!(
            "include \"{written}\": путь пишется от папки сцены (kit.msc, kits/ui.msc, ../kit.msc), а не от корня диска: так проект открывается и на другом компьютере"
        ));
    }
    Ok(base.join(path))
}

/// Files read before, by their real paths. One is read again when its text or the text of a
/// file it includes changes.
static READ: LazyLock<Mutex<HashMap<PathBuf, Arc<Loaded>>>> = LazyLock::new(Default::default);

thread_local! {
    /// The files being read right now, one inside another.
    static READING: RefCell<Vec<PathBuf>> = const { RefCell::new(Vec::new()) };
}

fn name(file: &Path) -> String {
    file.file_name().map_or_else(|| file.display().to_string(), |n| n.to_string_lossy().into_owned())
}

/// Reads `file` for an `include`. `Err` when there is no such file, or when the file is being
/// read already: it includes itself, straight or through another file.
pub(crate) fn load(file: &Path) -> Result<Arc<Loaded>, String> {
    let key = std::fs::canonicalize(file).map_err(|_| format!("не найден файл «{}»", file.display()))?;
    let round = READING.with(|r| {
        let r = r.borrow();
        r.iter().position(|p| *p == key).map(|i| r[i..].iter().chain([&key]).map(|p| name(p)).collect::<Vec<_>>().join(" → "))
    });
    if let Some(round) = round {
        return Err(format!("файлы подключают друг друга по кругу: {round}"));
    }
    let text = std::fs::read_to_string(&key).map_err(|e| format!("файл «{}» не читается: {e}", file.display()))?;
    let text = text.strip_prefix('\u{feff}').map(str::to_string).unwrap_or(text);
    READING.with(|r| r.borrow_mut().push(key.clone()));
    let known = READ.lock().unwrap_or_else(|e| e.into_inner()).get(&key).cloned();
    let same = |k: &Arc<Loaded>| {
        k.parts.iter().all(|p| match p {
            Part::File { lib, .. } => load(&lib.file).is_ok_and(|now| Arc::ptr_eq(&now, lib)),
            Part::Lines { .. } => true,
        })
    };
    let fresh = known.filter(|k| k.text == text && same(k));
    let loaded = fresh.unwrap_or_else(|| {
        let (loaded, whole) = read(file, &key, text);
        let loaded = Arc::new(loaded);
        let mut known = READ.lock().unwrap_or_else(|e| e.into_inner());
        // What was read while a file it names was missing is read again the next time.
        if whole {
            known.insert(key.clone(), loaded.clone());
        } else {
            known.remove(&key);
        }
        loaded
    });
    READING.with(|r| r.borrow_mut().pop());
    Ok(loaded)
}

/// The lines `file` gives and its first mistake; `false` when a file it includes was not read.
fn read(file: &Path, key: &Path, text: String) -> (Loaded, bool) {
    let dir = file.parent();
    // The file on its own, every board of it: a scene takes nothing from a file with mistakes.
    let error = crate::parse_boards(&text, dir).err().map(|errors| {
        let first = &errors[0];
        let more = if errors.len() > 1 { format!(" (и ещё ошибок: {})", errors.len() - 1) } else { String::new() };
        format!("строка {}: {}{more}", first.line, first.message)
    });
    let (head, ranges) = crate::boards(&text);
    let shared = if ranges.len() > 1 { &text[head] } else { &text[..] };
    let (mut parts, mut files, mut whole) = (Vec::new(), vec![file.to_path_buf()], true);
    let mut symbols = Vec::new();
    if let Ok(all) = crate::lexer::lex(shared) {
        let mut parser = Parser::new(shared, all.clone()).in_folder(dir);
        let stmts = parser.block(false);
        let mut included = parser.included.iter();
        for (s, range) in stmts.iter().zip(&parser.top) {
            match s {
                Stmt::Let { .. } | Stmt::LetGradient { .. } | Stmt::Style { .. } | Stmt::Theme { .. } | Stmt::Symbol { .. } => {
                    if let Stmt::Symbol { name, .. } = s {
                        symbols.push(name.clone());
                    }
                    if !matches!(parts.last(), Some(Part::Lines { .. })) {
                        parts.push(Part::Lines { toks: Vec::new(), text: Vec::new() });
                    }
                    let Some(Part::Lines { toks, text }) = parts.last_mut() else { continue };
                    let line = &all[range.clone()];
                    toks.extend(line.iter().cloned());
                    toks.push(Token { tok: Tok::Newline, span: Span::new(0, 0) });
                    let mut at = line[0].span.start;
                    for pair in line.windows(2) {
                        if let (Tok::Ident(key), Tok::Str(font)) = (&pair[0].tok, &pair[1].tok)
                            && key == "font"
                            && !crate::fonts::builtin_name(font)
                        {
                            text.push(Piece::Text(shared[at..pair[1].span.start].to_string()));
                            text.push(Piece::Font(font.clone()));
                            at = pair[1].span.end;
                        }
                    }
                    text.push(Piece::Text(format!("{}\n", &shared[at..line[line.len() - 1].span.end])));
                }
                Stmt::Include { path, .. } => {
                    let Some(lib) = included.next() else { continue };
                    for f in &lib.files {
                        if !files.contains(f) {
                            files.push(f.clone());
                        }
                    }
                    parts.push(Part::File { lib: lib.clone(), dir: folder(path) });
                }
                _ => {}
            }
        }
        whole = !parser.missing;
    }
    (Loaded { file: file.to_path_buf(), key: key.to_path_buf(), text, parts, error, files, symbols }, whole)
}

/// The sites of the `include` lines among `stmts`, with the lines of each put in its place.
pub(crate) fn spread(stmts: Vec<Stmt>, included: &[Arc<Loaded>]) -> (Vec<Stmt>, Vec<IncludeSite>) {
    let (mut out, mut sites) = (Vec::new(), Vec::new());
    let mut included = included.iter();
    for s in stmts {
        match s {
            Stmt::Include { path, path_span, span, body } => {
                if let Some(lib) = included.next() {
                    sites.push(IncludeSite { path, file: lib.file.clone(), span, path_span, files: lib.files.clone(), broken: lib.error.is_some() });
                }
                out.extend(body);
            }
            s => out.push(s),
        }
    }
    (out, sites)
}

/// The `include` line an edit reached: `new` differs from `old` inside the line, or adds words
/// to it. A new line right after it or right before it is not the line itself. This is how an
/// editor refuses an edit that tried to change what an included file gives: everything of that
/// file is written at its `include` line.
pub fn touched<'a>(old: &str, new: &str, includes: &'a [IncludeSite]) -> Option<&'a IncludeSite> {
    if old == new {
        return None;
    }
    let (a, b) = (old.as_bytes(), new.as_bytes());
    let head = a.iter().zip(b).take_while(|(x, y)| x == y).count();
    let tail = a[head..].iter().rev().zip(b[head..].iter().rev()).take_while(|(x, y)| x == y).count();
    // What is gone from the old text and what stands there now.
    let (from, to) = (head, a.len() - tail);
    let added = &b[head..b.len() - tail];
    includes.iter().find(|i| {
        let (start, end) = (i.span.start, i.span.end);
        if from < to {
            return from < end && to > start;
        }
        // Words put in: inside the line, or on it at either end.
        (start < from && from < end) || (from == end && !added.starts_with(b"\n") && !added.starts_with(b"\r")) || (from == start && !added.ends_with(b"\n"))
    })
}

/// Returns the script with every `include` line replaced by the lines its file gives: one text
/// that reads the same with no other file beside it. The paths of font files that those lines
/// name are written from the folder of the scene. Fails with the reason when the script or a
/// file it includes cannot be read.
pub fn inline_includes(src: &str, base: Option<&Path>) -> Result<String, String> {
    // The `include` lines of a file with several boards are in its head.
    let (head, ranges) = crate::boards(src);
    let own = if ranges.len() > 1 { &src[head] } else { src };
    let toks = crate::lexer::lex(own).map_err(|e| e.to_string())?;
    let mut parser = Parser::new(own, toks).in_folder(base);
    let stmts = parser.block(false);
    if let Some(e) = parser.errors.first() {
        return Err(e.to_string());
    }
    let mut out = String::new();
    let (mut at, mut given, mut included) = (0, Vec::new(), parser.included.iter());
    for s in &stmts {
        let Stmt::Include { path, span, .. } = s else { continue };
        let Some(lib) = included.next() else { continue };
        out.push_str(&own[at..span.start]);
        let mut lines = String::new();
        lib.text(&mut given, &folder(path), &mut lines);
        out.push_str(&format!("// из файла «{path}»\n{}", lines.trim_end_matches('\n')));
        at = span.end;
    }
    if at == 0 {
        return Ok(src.to_string());
    }
    out.push_str(&src[at..]);
    Ok(if crate::format::declared(src).is_some() { crate::format::stamp(&out) } else { out })
}

/// The `include` lines of every board of `file`, each once, with their places in the file. Empty
/// when a board has errors.
pub fn file_includes(file: &str, base: Option<&Path>) -> Vec<IncludeSite> {
    let mut out: Vec<IncludeSite> = Vec::new();
    for v in crate::boards::board_views(file, base).unwrap_or_default() {
        for i in &v.scene.includes {
            let span = v.in_file(i.span);
            if !out.iter().any(|o| o.span == span) {
                out.push(IncludeSite { span, path_span: v.in_file(i.path_span), ..i.clone() });
            }
        }
    }
    out
}

/// How `file` is written in an `include` line of a scene saved in `folder`: the path between
/// them, with `/`. `None` when there is no such path (another disk) or a file is missing.
pub fn include_path(folder: &Path, file: &Path) -> Option<String> {
    let (from, to) = (std::fs::canonicalize(folder).ok()?, std::fs::canonicalize(file).ok()?);
    let (from, to): (Vec<_>, Vec<_>) = (from.components().collect(), to.components().collect());
    let same = from.iter().zip(&to).take_while(|(a, b)| a == b).count();
    // Nothing in common: the files are on different disks.
    if same == 0 || matches!(to.get(same), Some(std::path::Component::Prefix(_) | std::path::Component::RootDir)) {
        return None;
    }
    let up = std::iter::repeat_n("..".to_string(), from.len() - same);
    let down = to[same..].iter().map(|c| c.as_os_str().to_string_lossy().into_owned());
    Some(up.chain(down).collect::<Vec<_>>().join("/"))
}

/// Returns the script with the line `include "path"`: after the `include` lines it has, or at
/// the top of the file (after the format version and the comments that open it), in the head
/// that every board shares. What the file gives is known from that line down.
pub fn add_include(src: &str, scene: &Scene, path: &str) -> String {
    let line = format!("include \"{}\"\n", path.replace('"', ""));
    match scene.includes.iter().map(|i| i.span.end).filter(|end| *end <= crate::head_end(src) || crate::head_end(src) == 0).max() {
        Some(end) => {
            let at = src[end..].find('\n').map_or(src.len(), |i| end + i + 1);
            if at == src.len() && !src.ends_with('\n') {
                return format!("{src}\n{line}");
            }
            crate::edit::splice(src, Span::new(at, at), &line)
        }
        None => {
            let at = crate::palette::top_of_file(src);
            crate::edit::splice(src, Span::new(at, at), &line)
        }
    }
}

/// Returns the script without its `include` line number `index`.
pub fn remove_include(src: &str, scene: &Scene, index: usize) -> Option<String> {
    let site = scene.includes.get(index)?;
    Some(crate::edit::splice(src, crate::themes::whole_line(src, site.span), ""))
}

/// `file` followed by lines that write the names the included files give `scene`: for the
/// functions that look for a free name in the text of a file alone.
pub fn with_included_names(file: &str, scene: &Scene) -> String {
    let mut out = file.to_string();
    if !out.ends_with('\n') {
        out.push('\n');
    }
    let given = |span: Span| scene.included(span).is_some();
    for s in scene.palette.iter().filter(|s| given(s.decl)) {
        out.push_str(&format!("let {} = #000000\n", s.name));
    }
    for g in scene.gradients.iter().filter(|g| given(g.decl)) {
        out.push_str(&format!("let {} = linear 0 #000000 #ffffff\n", g.name));
    }
    for s in scene.styles.iter().filter(|s| given(s.decl)) {
        out.push_str(&format!("style {} fill #000000\n", s.name));
    }
    for t in scene.themes.iter().filter(|t| given(t.span)) {
        out.push_str(&format!("theme {} {{\n}}\n", t.name));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Reading, parse_as, parse_boards, parse_in};

    /// A folder of its own for a test, with `files` written into it.
    fn folder_of(test: &str, files: &[(&str, &str)]) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("motif-include-{}-{test}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        for (name, text) in files {
            let file = dir.join(name);
            std::fs::create_dir_all(file.parent().unwrap()).unwrap();
            std::fs::write(file, text).unwrap();
        }
        dir
    }

    fn messages(src: &str, dir: &Path) -> Vec<String> {
        parse_in(src, Some(dir)).err().unwrap_or_default().iter().map(|e| format!("{}: {}", e.line, e.message)).collect()
    }

    const KIT: &str = "motif 7
// Набор кнопок
let ink = #1b1d24
let paper = #fff4e4
let gap = 8
let sky = linear 90 #6fb8ff paper
style card fill paper stroke ink 2 r 10
theme night {
  paper = #20222b
  ink = #f2f2f2
}
symbol Button face=#ff5a36 w=120 {
  variant quiet face=#8a8f9c
  rect body size {w} 40 fill face stroke ink 2 r 8
  ellipse dot at {w / 2 - 16} 0 size 8 8 fill ink
}
artboard \"Набор\" 300x200 bg paper
use Button sample at 150 100
rect only_here at 40 40 size 20 20 style card
";

    const SCREEN: &str = "motif 7
include \"kit.msc\"
artboard \"Экран\" 400x300 bg paper
rect panel at 200 150 size 300 200 style card
rect bar at 200 60 size {300 - gap * 2} 20 gradient sky
use Button ok at 200 220
use Button no at 200 260 variant quiet w=90
";

    #[test]
    fn a_scene_takes_the_colors_styles_themes_and_symbols_of_another_file() {
        let dir = folder_of("takes", &[("kit.msc", KIT)]);
        let scene = parse_in(SCREEN, Some(&dir)).unwrap();
        let ids: Vec<&str> = scene.doc.nodes.iter().map(|n| n.id.as_str()).collect();
        // The objects of the kit's own board stay with the kit.
        assert_eq!(ids, ["panel", "bar", "ok", "ok-body", "ok-dot", "no", "no-body", "no-dot"]);
        assert_eq!(scene.doc.artboard.name, "Экран");
        let node = |id: &str| scene.doc.nodes.iter().find(|n| n.id == id).unwrap();
        let hex = |c: Option<motif_core::Color>| c.map(|c| c.to_hex());
        assert_eq!(hex(node("panel").fill).as_deref(), Some("#fff4e4"));
        assert_eq!(node("panel").radius, 10.0);
        assert_eq!(node("bar").width, 284.0);
        assert!(node("bar").gradient.is_some());
        assert_eq!(hex(node("ok-body").fill).as_deref(), Some("#ff5a36"));
        assert_eq!((hex(node("no-body").fill).as_deref(), node("no-body").width), (Some("#8a8f9c"), 90.0));
        // What came from the file is told apart by its place: the `include` line.
        let line = scene.includes[0].clone();
        assert_eq!((line.path.as_str(), &SCREEN[line.span.start..line.span.end], line.broken), ("kit.msc", "include \"kit.msc\"", false));
        assert_eq!(line.files, [dir.join("kit.msc")]);
        assert!(scene.palette.iter().all(|s| scene.included(s.span).is_some()));
        assert_eq!(scene.palette.iter().map(|s| s.name.as_str()).collect::<Vec<_>>(), ["ink", "paper"]);
        assert!(scene.included(scene.styles[0].span).is_some() && scene.included(scene.symbols[0].span).is_some());
        assert!(scene.included(scene.gradients[0].span).is_some() && scene.included(scene.themes[0].span).is_some());
        // The value of each variant is known by its names, though every place is one place.
        let quiet = &scene.symbols[0].variants[0];
        assert_eq!(quiet.values[0].value, Some(motif_core::Value::Color(motif_core::Color::from_hex("#8a8f9c").unwrap())));
        // The colors and styles written in the other file are not places of this script.
        assert!(scene.colors.iter().all(|c| scene.included(c.span).is_none()), "{:?}", scene.colors);
        assert!(scene.style_uses.iter().all(|u| scene.included(u.span).is_none()));
        assert_eq!(scene.style_uses.len(), 1);
        // Its theme shows the scene too.
        let night = parse_as(SCREEN, Some(&dir), Reading { theme: Some("night"), ..Default::default() }).unwrap();
        assert_eq!(night.theme.as_deref(), Some("night"));
        assert_eq!(hex(night.doc.nodes[0].fill).as_deref(), Some("#20222b"));
        assert_eq!(night.doc.artboard.background.to_hex(), "#20222b");
        // A symbol of the other file is changed there: it is not edited through its copy here,
        // and the copy is not turned into a group of this scene.
        let edited = parse_as(SCREEN, Some(&dir), Reading { symbol_of: Some("ok"), ..Default::default() }).unwrap();
        assert_eq!(edited.editing, None);
        let refused = crate::detach_copy(SCREEN, &scene, "ok", Some(&dir)).unwrap_err();
        assert!(refused.contains("в подключённом файле «kit.msc»"), "{refused}");
    }

    #[test]
    fn what_is_wrong_with_an_include_is_told_at_its_line() {
        let dir = folder_of("wrong", &[("kit.msc", KIT), ("bad.msc", "let ink = #1b1d24\nartboard \"a\" 10x10\nrect a fill nope\nrect b at\n")]);
        let scene = |line: &str| format!("{line}\nartboard \"a\" 100x100\n");
        let told = |line: &str| messages(&scene(line), &dir);
        assert_eq!(told("include \"none.msc\""), [format!("1: не найден файл «{}»", dir.join("none.msc").display())]);
        assert!(told("include kit.msc")[0].contains("имя файла в кавычках"));
        assert!(told("include \"kit.msc\" all")[0].contains("только имя файла"));
        assert!(told("include \"/kit.msc\"")[0].contains("от папки сцены"));
        assert!(told("include \"C:\\kits\\kit.msc\"")[0].contains("от папки сцены"));
        assert!(told("group g {\n  include \"kit.msc\"\n}")[0].contains("на верхнем уровне файла"));
        // A file with mistakes: the first of them, with its line there.
        let bad = told("include \"bad.msc\"");
        assert_eq!(bad.len(), 1, "{bad:?}");
        assert!(bad[0].starts_with("1: в подключённом файле «bad.msc» ошибка, строка 3: ") && bad[0].ends_with("(и ещё ошибок: 2)"), "{bad:?}");
        // A scene that is not a file has no folder to look in.
        let none = crate::parse(&scene("include \"kit.msc\"")).unwrap_err();
        assert!(none[0].message.contains("нет папки"), "{none:?}");
        // The same name here and there.
        let twice = messages("include \"kit.msc\"\nartboard \"a\" 100x100\nsymbol Button {\n  rect b size 4 4\n}\nstyle card fill #000000\n", &dir);
        assert_eq!(twice, ["3: символ «Button» уже есть", "6: стиль «card» уже есть"]);
        let twice = messages("symbol Button {\n  rect b size 4 4\n}\ninclude \"kit.msc\"\nartboard \"a\" 100x100\n", &dir);
        assert_eq!(twice, ["4: в подключённом файле «kit.msc»: символ «Button» уже есть"]);
    }

    #[test]
    fn files_include_files_each_once_and_never_in_a_circle() {
        let dir = folder_of(
            "nested",
            &[
                ("palette.msc", "let ink = #1b1d24\nstyle line stroke ink 2\n"),
                ("kits/buttons.msc", "include \"../palette.msc\"\nsymbol Button {\n  rect body size 80 30 style line\n}\n"),
                ("kits/panels.msc", "include \"../palette.msc\"\nsymbol Panel {\n  rect body size 200 120 style line\n}\n"),
                ("a.msc", "include \"b.msc\"\nlet a = 1\n"),
                ("b.msc", "include \"a.msc\"\nlet b = 2\n"),
                ("self.msc", "include \"self.msc\"\n"),
            ],
        );
        // Two kits bring the palette they share once: its style is not written twice.
        let src = "include \"kits/buttons.msc\"\ninclude \"kits\\panels.msc\"\nartboard \"a\" 300x200\nuse Button b1 at 50 50\nuse Panel p1 at 150 100\n";
        let scene = parse_in(src, Some(&dir)).unwrap();
        assert_eq!(scene.styles.len(), 1);
        assert_eq!(scene.includes[0].files, [dir.join("kits/buttons.msc"), dir.join("kits/../palette.msc")]);
        assert_eq!(scene.doc.nodes.iter().find(|n| n.id == "p1-body").unwrap().stroke_width, 2.0);
        // A circle is a mistake of the files in it, told with the way round.
        let round = messages("include \"a.msc\"\nartboard \"a\" 10x10\n", &dir);
        assert_eq!(round.len(), 1);
        assert!(round[0].contains("файлы подключают друг друга по кругу: a.msc → b.msc → a.msc"), "{round:?}");
        let own = messages("include \"self.msc\"\nartboard \"a\" 10x10\n", &dir);
        assert!(own[0].contains("по кругу: self.msc → self.msc"), "{own:?}");
        // Mended, the files read: nothing of the circle is remembered.
        std::fs::write(dir.join("b.msc"), "let b = 2\n").unwrap();
        assert_eq!(messages("include \"a.msc\"\nartboard \"a\" 10x10\nrect r size {a + b} 4\n", &dir), Vec::<String>::new());
    }

    #[test]
    fn a_file_of_several_boards_gives_its_head_and_takes_for_every_board() {
        let kit = "let ink = #1b1d24\nsymbol Dot {\n  ellipse d size 6 6 fill ink\n}\nartboard \"Кнопки\" 100x100\nlet local = #ff0000\nsymbol Local {\n  rect r size 4 4\n}\nartboard \"Панели\" 100x100\nuse Dot d1\n";
        let dir = folder_of("boards", &[("kit.msc", kit)]);
        // What one board of the kit writes for itself stays with that board.
        let missing = messages("include \"kit.msc\"\nartboard \"a\" 10x10\nuse Local l1\n", &dir);
        assert!(missing[0].contains("нет символа «Local»: есть: Dot"), "{missing:?}");
        // An `include` in the head of a scene serves each of its boards.
        let src = "include \"kit.msc\"\nartboard \"Один\" 50x50\nuse Dot a\nartboard \"Два\" 60x60\nuse Dot b\nrect r size 4 4 fill ink\n";
        let boards = parse_boards(src, Some(&dir)).unwrap();
        assert_eq!(boards.len(), 2);
        assert!(boards.iter().all(|b| b.includes.len() == 1 && b.symbols.len() == 1));
        assert_eq!(boards[1].doc.nodes.iter().map(|n| n.id.as_str()).collect::<Vec<_>>(), ["b", "b-d", "r"]);
    }

    #[test]
    fn a_changed_file_is_read_again() {
        let dir = folder_of("changed", &[("colors.msc", "let ink = #111111\n"), ("kit.msc", "include \"colors.msc\"\nlet w = 10\n")]);
        let src = "include \"kit.msc\"\nartboard \"a\" 100x100\nrect r size {w} 4 fill ink\n";
        let look = || {
            let scene = parse_in(src, Some(&dir)).unwrap();
            (scene.doc.nodes[0].width, scene.doc.nodes[0].fill.unwrap().to_hex())
        };
        assert_eq!(look(), (10.0, "#111111".to_string()));
        assert_eq!(look(), (10.0, "#111111".to_string()));
        // The same length and, on a coarse clock, the same time: the text is what is compared.
        std::fs::write(dir.join("kit.msc"), "include \"colors.msc\"\nlet w = 20\n").unwrap();
        assert_eq!(look(), (20.0, "#111111".to_string()));
        std::fs::write(dir.join("colors.msc"), "let ink = #222222\n").unwrap();
        assert_eq!(look(), (20.0, "#222222".to_string()));
        // A file that got a mistake stops the scene, and reads again once it is mended.
        std::fs::write(dir.join("colors.msc"), "let ink = #22\n").unwrap();
        assert!(messages(src, &dir)[0].contains("в подключённом файле «kit.msc» ошибка, строка 1: в подключённом файле «colors.msc» ошибка, строка 1: "));
        std::fs::write(dir.join("colors.msc"), "let ink = #333333\n").unwrap();
        assert_eq!(look(), (20.0, "#333333".to_string()));
        std::fs::remove_file(dir.join("colors.msc")).unwrap();
        assert!(messages(src, &dir)[0].contains("не найден файл"));
        std::fs::write(dir.join("colors.msc"), "let ink = #444444\n").unwrap();
        assert_eq!(look(), (20.0, "#444444".to_string()));
    }

    #[test]
    fn the_names_of_a_file_read_there_as_they_read_here() {
        // `r` is a property; a scene that calls a number `r` above the `include` line does not
        // change how the lines of the kit are read, and the names of the kit are values below.
        let kit = "let skin = #5ee0a0\nsymbol Tile {\n  rect t size 10 10 r 2 fill skin\n}\n";
        let dir = folder_of("names", &[("kit.msc", kit)]);
        let src = "let r = 4\ninclude \"kit.msc\"\nartboard \"a\" 100x100\nuse Tile t1\nrect own size 8 8 fill skin\n";
        let scene = parse_in(src, Some(&dir)).unwrap();
        let node = |id: &str| scene.doc.nodes.iter().find(|n| n.id == id).unwrap();
        assert_eq!(node("t1-t").radius, 2.0);
        assert_eq!(node("own").fill.unwrap().to_hex(), "#5ee0a0");
        // The same color name written below the line is this scene's own from there on.
        let own = parse_in(&src.replace("use Tile", "let skin = #ff0000\nuse Tile"), Some(&dir)).unwrap();
        assert_eq!(own.palette.len(), 1);
        assert!(own.included(own.palette[0].span).is_none());
        assert_eq!(own.doc.nodes.iter().find(|n| n.id == "t1-t").unwrap().fill.unwrap().to_hex(), "#ff0000");
    }

    #[test]
    fn an_edit_that_reaches_an_include_line_is_told() {
        let dir = folder_of("touched", &[("kit.msc", KIT)]);
        let scene = parse_in(SCREEN, Some(&dir)).unwrap();
        let (start, end) = (scene.includes[0].span.start, scene.includes[0].span.end);
        let with = |at: usize, to: usize, text: &str| format!("{}{text}{}", &SCREEN[..at], &SCREEN[to..]);
        let touched = |new: String| super::touched(SCREEN, &new, &scene.includes).is_some();
        // What a file gives is written at the line: a value replaced, a word added, the line gone.
        assert!(touched(with(start, end, "#ff0000")));
        assert!(touched(with(end, end, " fill #ff0000")));
        assert!(touched(with(start, start, "fill #ff0000 ")));
        assert!(touched(with(start + 3, start + 3, "x")));
        assert!(touched(with(start, end + 1, "")));
        // A line of the scene's own written after it or before it, and any edit elsewhere.
        assert!(!touched(with(end, end, "\nlet own = #ff0000")));
        assert!(!touched(with(start, start, "let own = #ff0000\n")));
        assert!(!touched(SCREEN.replace("size 300 200", "size 320 200")));
        assert!(!touched(SCREEN.to_string()));
        // The edits of the scene that would write into the kit come out as such.
        let swatch = scene.palette.iter().find(|s| s.name == "ink").unwrap();
        assert!(touched(crate::edit::splice(SCREEN, swatch.span, "#000000")));
    }

    #[test]
    fn a_scene_is_written_as_one_text_with_the_lines_of_its_files() {
        let dir = folder_of(
            "inline",
            &[
                ("kits/kit.msc", KIT),
                ("kits/words.msc", "include \"kit.msc\"\nsymbol Title {\n  text t content \"Привет\" font-size 20 font \"fonts/own.ttf\" fill ink\n}\n"),
            ],
        );
        let src = SCREEN.replace("include \"kit.msc\"", "include \"kits/kit.msc\"\ninclude \"kits/words.msc\"");
        let one = inline_includes(&src, Some(&dir)).unwrap();
        assert!(!one.lines().any(|l| l.starts_with("include")), "{one}");
        assert!(one.contains("// из файла «kits/kit.msc»\nlet ink = #1b1d24\n"), "{one}");
        // The kit both lines bring is written once, and the board of the kit is not written.
        assert_eq!(one.matches("symbol Button").count(), 1);
        assert!(!one.contains("only_here") && !one.contains("Набор"));
        // The font of the other folder is named from the folder of the scene.
        assert!(one.contains("font \"kits/fonts/own.ttf\""), "{one}");
        // Without the font line the text reads with no file beside it, as the scene reads.
        let plain = one.lines().filter(|l| !l.contains("own.ttf")).collect::<Vec<_>>().join("\n").replace("symbol Title {\n}", "");
        let (whole, alone) = (parse_in(SCREEN, Some(&dir.join("kits"))).unwrap(), crate::parse(&plain).unwrap());
        assert_eq!(whole.doc.nodes, alone.doc.nodes);
        // A scene with no `include` is returned as it is.
        assert_eq!(inline_includes(KIT, None).unwrap(), KIT);
    }
    #[test]
    fn an_include_line_is_written_and_taken_away() {
        let dir = folder_of("lines", &[("kit.msc", KIT), ("sub/screen.msc", ""), ("more/other.msc", "let extra = 4\n")]);
        // The path between a scene and a file, whichever is deeper.
        assert_eq!(include_path(&dir, &dir.join("kit.msc")).as_deref(), Some("kit.msc"));
        assert_eq!(include_path(&dir.join("sub"), &dir.join("kit.msc")).as_deref(), Some("../kit.msc"));
        assert_eq!(include_path(&dir.join("sub"), &dir.join("more/other.msc")).as_deref(), Some("../more/other.msc"));
        assert_eq!(include_path(&dir, &dir.join("none.msc")), None);
        // The first line goes under the version and the comments that open the file, above the
        // scene's own names; the next one goes under it.
        let src = "motif 1\n// Экран\nlet own = #ff0000\nartboard \"a\" 100x100\nrect r size 4 4 fill own\n";
        let scene = parse_in(src, Some(&dir)).unwrap();
        let one = add_include(src, &scene, "kit.msc");
        assert!(one.starts_with("motif 1\n// Экран\ninclude \"kit.msc\"\nlet own = #ff0000\n"), "{one}");
        let scene = parse_in(&one, Some(&dir)).unwrap();
        assert!(touched(src, &one, &scene.includes).is_none());
        let two = add_include(&one, &scene, "more/other.msc");
        assert!(two.contains("include \"kit.msc\"\ninclude \"more/other.msc\"\nlet own"), "{two}");
        let scene = parse_in(&two, Some(&dir)).unwrap();
        assert_eq!(scene.doc.includes, ["kit.msc", "more/other.msc"]);
        assert_eq!(file_includes(&two, Some(&dir)).len(), 2);
        assert_eq!(remove_include(&two, &scene, 1).as_deref(), Some(one.as_str()));
        assert_eq!(remove_include(&one, &parse_in(&one, Some(&dir)).unwrap(), 0).as_deref(), Some(src));
        // A new name of the scene is looked for among the names of the files too.
        let known = with_included_names(&one, &parse_in(&one, Some(&dir)).unwrap());
        assert!(known.contains("let ink = ") && known.contains("let sky = linear") && known.contains("style card ") && known.contains("theme night {"));
        assert!(!known.contains("let own = #000000"));
        // In a file of several boards the line is in the head they share.
        let boards = "let own = #ff0000\nartboard \"a\" 10x10\nartboard \"b\" 10x10\n";
        let head = add_include(&boards[..35], &crate::parse(&boards[..35]).unwrap(), "kit.msc");
        assert!(head.starts_with("include \"kit.msc\"\nlet own"), "{head}");
    }
}
