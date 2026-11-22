//! The library of a project: the symbols that the `.msc` files of a folder give to a scene that
//! includes them. A symbol is found by its name, by the words of its `tags` line and by the name
//! of its file; placing a copy writes the `include` line when the scene does not have it yet.

use crate::edit::{splice, whole_lines};
use crate::{Scene, Span};
use std::path::{Path, PathBuf};

/// A symbol a file gives.
#[derive(Clone, Debug, PartialEq)]
pub struct LibrarySymbol {
    pub name: String,
    /// The words of its `tags` line.
    pub tags: Vec<String>,
    /// The names of its parameters and of its variants, in the order written.
    pub params: Vec<String>,
    pub variants: Vec<String>,
}

/// What a file of the project gives to the library.
#[derive(Clone, Debug, PartialEq)]
pub struct LibraryFile {
    pub file: PathBuf,
    /// The symbols written in the file itself: those of the files it includes stand with them.
    pub symbols: Vec<LibrarySymbol>,
    /// Why nothing is taken from the file: it is not read, or has a mistake.
    pub error: Option<String>,
}

/// How deep the folders of a project are looked into, and how many files are taken.
const DEPTH: usize = 4;
const FILES: usize = 400;

/// The `.msc` files of `folder` and of the folders in it, in the order of their paths. Hidden
/// folders and the folders of build tools are passed by.
pub fn library_files(folder: &Path) -> Vec<PathBuf> {
    fn walk(dir: &Path, depth: usize, out: &mut Vec<PathBuf>) {
        let Ok(entries) = std::fs::read_dir(dir) else { return };
        let mut entries: Vec<PathBuf> = entries.flatten().map(|e| e.path()).collect();
        entries.sort();
        for p in entries {
            if out.len() >= FILES {
                return;
            }
            let name = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            if p.is_dir() {
                if depth < DEPTH && !name.starts_with('.') && !matches!(name.as_str(), "target" | "node_modules") {
                    walk(&p, depth + 1, out);
                }
            } else if p.extension().is_some_and(|e| e.eq_ignore_ascii_case("msc")) {
                out.push(p);
            }
        }
    }
    let mut out = Vec::new();
    walk(folder, 0, &mut out);
    out
}

/// The line that includes `file` from a scene beside it.
fn beside(file: &Path) -> Option<(String, &Path)> {
    let name = file.file_name()?.to_str()?;
    (!name.contains('"') && !name.contains('\n')).then(|| (format!("include \"{name}\"\n"), file.parent().unwrap_or(Path::new("."))))
}

/// The symbols `file` gives: the library reads the file as a scene that includes it would. A
/// file with no `symbol` line gives nothing and is not checked for mistakes.
pub fn library_file(file: &Path) -> LibraryFile {
    let told = |error: String| LibraryFile { file: file.to_path_buf(), symbols: Vec::new(), error: Some(error) };
    // Most files of a project are screens with no symbol of their own: those are not read as
    // scenes at all, so a folder of many files is looked through quickly.
    if std::fs::read_to_string(file).is_ok_and(|text| !text.lines().any(|l| l.trim_start().strip_prefix("symbol").is_some_and(|r| r.starts_with([' ', '\t']))))
    {
        return LibraryFile { file: file.to_path_buf(), symbols: Vec::new(), error: None };
    }
    let loaded = match crate::include::load(file) {
        Ok(l) => l,
        Err(e) => return told(e),
    };
    if let Some(e) = &loaded.error {
        return told(e.clone());
    }
    let mut out = LibraryFile { file: file.to_path_buf(), symbols: Vec::new(), error: None };
    if loaded.symbols.is_empty() {
        return out;
    }
    let Some((line, dir)) = beside(file) else { return out };
    match crate::parse_in(&format!("{line}artboard \"library\" 64x64\n"), Some(dir)) {
        Ok(scene) => {
            out.symbols = loaded
                .symbols
                .iter()
                .filter_map(|name| scene.symbols.iter().find(|s| &s.name == name))
                .map(|s| LibrarySymbol {
                    name: s.name.clone(),
                    tags: s.tags.clone(),
                    params: s.params.iter().map(|(n, _)| n.clone()).collect(),
                    variants: s.variants.iter().map(|v| v.name.clone()).collect(),
                })
                .collect();
        }
        Err(e) => out.error = e.first().map(|e| e.message.clone()),
    }
    out
}

/// A scene with one copy of `symbol` of `file` at the origin, and the id of the copy: what the
/// library draws the picture of the symbol from.
pub fn library_preview(file: &Path, symbol: &str) -> Option<(Scene, String)> {
    let (line, dir) = beside(file)?;
    let id = "library-copy";
    let scene = crate::parse_in(&format!("{line}artboard \"library\" 64x64\nuse {symbol} {id} at 0 0\n"), Some(dir)).ok()?;
    scene.doc.index_of(id).map(|_| (scene, id.to_string()))
}

/// The scene of `src` with one copy of each of its `symbols` at the origin, and the ids of the
/// copies in that order: what the library draws the pictures of the scene's own symbols from.
pub fn library_own_preview(src: &str, scene: &Scene, base: Option<&Path>, symbols: &[String]) -> Option<(Scene, Vec<String>)> {
    let mut lines = String::new();
    let mut ids = Vec::new();
    for (k, symbol) in symbols.iter().enumerate() {
        let id = (0..).map(|n| format!("library-copy-{k}-{n}")).find(|id| scene.doc.index_of(id).is_none())?;
        lines.push_str(&format!("use {symbol} {id} at 0 0\n"));
        ids.push(id);
    }
    let read = crate::parse_in(&crate::edit::insert_declaration(src, scene, &lines), base).ok()?;
    ids.iter().all(|id| read.doc.index_of(id).is_some()).then_some((read, ids))
}

/// Whether a symbol answers what is typed into the search of the library: every word of `query`
/// is found in its name, in one of its tags or in the name of its file. Letter case is not told
/// apart.
pub fn library_match(query: &str, name: &str, tags: &[String], file: &str) -> bool {
    let hay: Vec<String> = [name, file].into_iter().map(str::to_lowercase).chain(tags.iter().map(|t| t.to_lowercase())).collect();
    query.split_whitespace().all(|word| {
        let word = word.to_lowercase();
        hay.iter().any(|h| h.contains(&word))
    })
}

/// Returns the script with a copy of `symbol` of `file` centred at `center`, the id of the copy,
/// and whether the line `include` was written for it. The scene is saved in `folder`. An error
/// says why the copy is not placed: the scene has another symbol of that name, the file is on
/// another disk, or the scene does not read with the file.
pub fn add_library_use(src: &str, scene: &Scene, folder: &Path, file: &Path, symbol: &str, center: (f64, f64)) -> Result<(String, String, bool), String> {
    let real = std::fs::canonicalize(file).map_err(|_| format!("не найден файл «{}»", file.display()))?;
    if let Some(known) = scene.symbols.iter().find(|s| s.name == symbol) {
        let from = scene.included(known.span);
        if from.is_some_and(|site| site.files.iter().any(|f| std::fs::canonicalize(f).is_ok_and(|f| f == real))) {
            let (out, id) = crate::add_use(src, scene, symbol, center);
            return Ok((out, id, false));
        }
        let whose = from.map_or_else(|| "записанный в ней самой".to_string(), |site| format!("из файла «{}»", site.path));
        return Err(format!("в сцене уже есть символ «{symbol}», {whose}: два символа с одним именем сцена не берёт"));
    }
    let path = crate::include_path(folder, file).ok_or_else(|| format!("файл {} на другом диске: путь к нему от сцены не записать", file.display()))?;
    let with_file = crate::add_include(src, scene, &path);
    let read = crate::parse_in(&with_file, Some(folder)).map_err(|e| {
        let first = &e[0];
        format!("с файлом «{path}» сцена не читается, строка {}: {}", first.line, first.message)
    })?;
    if !read.symbols.iter().any(|s| s.name == symbol) {
        return Err(format!("файл «{path}» не даёт символ «{symbol}»"));
    }
    let (out, id) = crate::add_use(&with_file, &read, symbol, center);
    Ok((out, id, true))
}

/// How a tag is written in the `tags` line: a word as it is, anything else in quotes.
fn written(tag: &str) -> String {
    let mut chars = tag.chars();
    let word = chars.next().is_some_and(|c| c.is_alphabetic() || c == '_') && chars.all(|c| c.is_alphanumeric() || c == '_' || c == '-');
    if word { tag.to_string() } else { format!("\"{}\"", tag.replace('"', "")) }
}

/// Returns the script with `tags` as the words of symbol `symbol`: its `tags` line is rewritten,
/// written under the `symbol` line, or taken away when there are no words. `None` when the scene
/// has no such symbol or an included file writes it.
pub fn set_tags(src: &str, scene: &Scene, symbol: &str, tags: &[String]) -> Option<String> {
    let site = scene.symbols.iter().find(|s| s.name == symbol)?;
    if scene.included(site.span).is_some() {
        return None;
    }
    let mut words: Vec<String> = Vec::new();
    for t in tags.iter().map(|t| t.trim()).filter(|t| !t.is_empty()) {
        if !words.iter().any(|w| w.to_lowercase() == t.to_lowercase()) {
            words.push(t.to_string());
        }
    }
    let line = format!("tags {}", words.iter().map(|w| written(w)).collect::<Vec<_>>().join(" "));
    Some(match (site.tags_span, words.is_empty()) {
        (Some(span), true) => splice(src, whole_lines(src, span), ""),
        (Some(span), false) => splice(src, span, &line),
        (None, true) => src.to_string(),
        (None, false) => {
            let at = src[site.span.start..].find('\n').map_or(src.len(), |i| site.span.start + i + 1);
            splice(src, Span::new(at, at), &format!("  {line}\n"))
        }
    })
}

/// Returns the script of a board with `tags` as the words a game finds it by: its `tags` line is
/// rewritten, written under the `artboard` line, or taken away when there are no words.
pub fn set_board_tags(src: &str, scene: &Scene, tags: &[String]) -> String {
    let mut words: Vec<String> = Vec::new();
    for t in tags.iter().map(|t| t.trim()).filter(|t| !t.is_empty()) {
        if !words.iter().any(|w| w.to_lowercase() == t.to_lowercase()) {
            words.push(t.to_string());
        }
    }
    let line = format!("tags {}", words.iter().map(|w| written(w)).collect::<Vec<_>>().join(" "));
    match (scene.board_tags, words.is_empty()) {
        (Some(span), true) => splice(src, whole_lines(src, span), ""),
        (Some(span), false) => splice(src, span, &line),
        (None, true) => src.to_string(),
        (None, false) => {
            // Under the `artboard` line; a scene without one gets it on top, under the version.
            let board = src.match_indices("artboard").find(|(i, _)| *i == 0 || src.as_bytes()[i - 1] == b'\n').map(|(i, _)| i);
            let from = board.or_else(|| crate::format::declared(src).map(|_| 0));
            let at = match from {
                Some(from) => src[from..].find('\n').map_or(src.len(), |i| from + i + 1),
                None => 0,
            };
            let before = if at > 0 && !src[..at].ends_with('\n') { "\n" } else { "" };
            splice(src, Span::new(at, at), &format!("{before}{line}\n"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse_in;

    /// A folder of its own for a test, with `files` written into it.
    fn folder_of(test: &str, files: &[(&str, &str)]) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("motif-library-{}-{test}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        for (name, text) in files {
            let file = dir.join(name);
            std::fs::create_dir_all(file.parent().unwrap()).unwrap();
            std::fs::write(file, text).unwrap();
        }
        dir
    }

    const KIT: &str = "let ink = #2a2230\nsymbol Button face=#e2573b {\n  tags кнопка меню \"главный экран\"\n  variant quiet face=#b9ab95\n  rect body size 120 40 r 10 fill face stroke ink 3\n}\nsymbol Coin {\n  ellipse body size 20 20 fill #ffd23f\n}\n";

    #[test]
    fn the_files_of_a_folder_give_their_own_symbols_with_tags() {
        let dir = folder_of(
            "scan",
            &[
                ("kit.msc", KIT),
                ("hud/bars.msc", "include \"../kit.msc\"\nsymbol Bar {\n  tags полоса\n  rect track size 100 10 fill ink\n}\n"),
                ("broken.msc", "symbol Bad {\n  frobnicate a\n}\n"),
                (".git/x.msc", KIT),
                ("notes.txt", "symbol No {\n}\n"),
            ],
        );
        let files = library_files(&dir);
        let names: Vec<String> = files.iter().map(|f| f.strip_prefix(&dir).unwrap().to_string_lossy().replace('\\', "/")).collect();
        assert_eq!(names, ["broken.msc", "hud/bars.msc", "kit.msc"]);

        let kit = library_file(&dir.join("kit.msc"));
        assert_eq!(kit.error, None);
        assert_eq!(kit.symbols.iter().map(|s| s.name.as_str()).collect::<Vec<_>>(), ["Button", "Coin"]);
        assert_eq!(kit.symbols[0].tags, ["кнопка", "меню", "главный экран"]);
        assert_eq!((kit.symbols[0].params.clone(), kit.symbols[0].variants.clone()), (vec!["face".to_string()], vec!["quiet".to_string()]));
        assert!(kit.symbols[1].tags.is_empty());

        // A file that includes the kit gives its own symbol only: the kit's stand with the kit.
        let bars = library_file(&dir.join("hud/bars.msc"));
        assert_eq!(bars.symbols.iter().map(|s| s.name.as_str()).collect::<Vec<_>>(), ["Bar"]);
        // A file with a mistake gives nothing and says why.
        let broken = library_file(&dir.join("broken.msc"));
        assert!(broken.symbols.is_empty() && broken.error.as_deref().is_some_and(|e| e.starts_with("строка 2")), "{broken:?}");
    }

    #[test]
    fn a_symbol_is_found_by_name_tag_and_file() {
        let tags = ["кнопка".to_string(), "Главный экран".to_string()];
        assert!(library_match("", "Button", &tags, "kit.msc"));
        assert!(library_match("butt", "Button", &tags, "kit.msc"));
        assert!(library_match("КНОП kit", "Button", &tags, "kit.msc"));
        assert!(library_match("главный", "Button", &tags, "kit.msc"));
        assert!(!library_match("кнопка hud", "Button", &tags, "kit.msc"));
    }

    #[test]
    fn the_tags_line_reads_and_says_what_is_wrong() {
        let read = |src: &str| parse_in(src, None).map(|s| s.symbols[0].tags.clone()).map_err(|e| e[0].message.clone());
        assert_eq!(read("symbol A {\n  tags a_b \"два слова\" A_B\n  rect r size 4 4\n}\n").unwrap(), ["a_b", "два слова"]);
        assert!(read("symbol A {\n  tags\n  rect r size 4 4\n}\n").unwrap_err().contains("после tags"));
        assert!(read("symbol A {\n  tags a\n  tags b\n  rect r size 4 4\n}\n").unwrap_err().contains("уже есть строка tags"));
        // Outside a symbol the line is the words of the board, not of a symbol; in a group it is neither.
        assert!(read("symbol A {\n  rect r size 4 4\n}\ntags a\n").unwrap().is_empty());
        assert_eq!(parse_in("symbol A {\n  rect r size 4 4\n}\ntags a\n", None).unwrap().doc.tags, ["a"]);
        assert!(read("symbol A {\n  rect r size 4 4\n}\ngroup g {\n  tags a\n}\n").unwrap_err().contains("внутри symbol"));
        assert_eq!(crate::format::needed("symbol A {\n  tags a\n}\n"), 7);
    }

    #[test]
    fn tags_are_written_rewritten_and_taken_away() {
        let src = "symbol A {\n  rect r size 4 4\n}\nartboard \"x\" 64x64\nuse A a1 at 0 0\n";
        let scene = parse_in(src, None).unwrap();
        let one = set_tags(src, &scene, "A", &["кнопка".into(), "два слова".into(), "Кнопка".into(), " ".into()]).unwrap();
        assert!(one.starts_with("symbol A {\n  tags кнопка \"два слова\"\n  rect r"), "{one}");
        let scene = parse_in(&one, None).unwrap();
        assert_eq!(scene.symbols[0].tags, ["кнопка", "два слова"]);
        let two = set_tags(&one, &scene, "A", &["hud".into()]).unwrap();
        assert!(two.starts_with("symbol A {\n  tags hud\n  rect r"), "{two}");
        let none = set_tags(&two, &parse_in(&two, None).unwrap(), "A", &[]).unwrap();
        assert_eq!(none, src);
        assert_eq!(set_tags(src, &parse_in(src, None).unwrap(), "Nope", &["a".into()]), None);
    }

    #[test]
    fn a_copy_from_the_library_writes_the_include_line_once() {
        let dir = folder_of("place", &[("kits/kit.msc", KIT), ("other.msc", "symbol Button {\n  rect b size 8 8\n}\n")]);
        let kit = dir.join("kits/kit.msc");
        let src = "motif 7\nartboard \"x\" 200x100\nrect bg at 100 50 size 200 100 fill #ffffff\n";
        let scene = parse_in(src, Some(&dir)).unwrap();
        let (one, id, included) = add_library_use(src, &scene, &dir, &kit, "Button", (40.0, 30.0)).unwrap();
        assert!(included && id == "button1", "{id}");
        assert!(one.contains("include \"kits/kit.msc\"\n") && one.contains("use Button button1 at 40 30\n"), "{one}");
        // The second copy finds the file included.
        let scene = parse_in(&one, Some(&dir)).unwrap();
        let (two, id, included) = add_library_use(&one, &scene, &dir, &kit, "Coin", (10.0, 10.0)).unwrap();
        assert!(!included && id == "coin1" && two.matches("include").count() == 1, "{two}");
        // A symbol of the same name from another file is not taken: the reason is said.
        let scene = parse_in(&two, Some(&dir)).unwrap();
        let e = add_library_use(&two, &scene, &dir, &dir.join("other.msc"), "Button", (0.0, 0.0)).unwrap_err();
        assert!(e.contains("уже есть символ «Button», из файла «kits/kit.msc»"), "{e}");
        // Nor a file whose other names stand in the way.
        let own = "motif 7\nsymbol Coin {\n  rect c size 4 4\n}\nartboard \"x\" 200x100\n";
        let e = add_library_use(own, &parse_in(own, Some(&dir)).unwrap(), &dir, &kit, "Button", (0.0, 0.0)).unwrap_err();
        assert!(e.contains("с файлом «kits/kit.msc» сцена не читается") && e.contains("Coin"), "{e}");
    }

    #[test]
    fn the_picture_of_a_symbol_is_a_scene_with_one_copy() {
        let dir = folder_of("preview", &[("kit.msc", KIT)]);
        let (scene, id) = library_preview(&dir.join("kit.msc"), "Button").unwrap();
        let i = scene.doc.index_of(&id).unwrap();
        let b = scene.doc.world_bounds(i).unwrap();
        assert!(b.width() >= 120.0 && b.height() >= 40.0, "{b:?}");
        assert!(library_preview(&dir.join("kit.msc"), "Nope").is_none());
        // The symbols of the scene itself, all in one reading.
        let src = "symbol A {\n  rect r size 30 10\n}\nsymbol B {\n  ellipse e size 8 8\n}\nartboard \"x\" 64x64\nuse A a1 at 5 5\n";
        let own = parse_in(src, None).unwrap();
        let (scene, ids) = library_own_preview(src, &own, None, &["A".to_string(), "B".to_string()]).unwrap();
        let widths: Vec<f64> = ids.iter().map(|id| scene.doc.world_bounds(scene.doc.index_of(id).unwrap()).unwrap().width()).collect();
        assert_eq!(widths, [30.0, 8.0]);
    }
}
