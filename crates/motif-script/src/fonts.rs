//! Font files for `text`, read once and kept while they do not change on disk.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::SystemTime;

type Cache = HashMap<PathBuf, (Option<SystemTime>, Arc<Vec<u8>>)>;

fn cache() -> &'static Mutex<Cache> {
    static CACHE: OnceLock<Mutex<Cache>> = OnceLock::new();
    CACHE.get_or_init(Default::default)
}

/// Reads files where `std::fs` cannot: a game engine's packed resources (`res://` in Godot).
pub type Reader = fn(&Path) -> Option<Vec<u8>>;

static READER: OnceLock<Reader> = OnceLock::new();

/// Portable font aliases used by the HTML importer. They are embedded resources,
/// not filesystem paths, so an imported scene also opens on another computer.
pub const CSS_SANS_REGULAR: &str = "builtin:css-sans-regular";
pub const CSS_SANS_BOLD: &str = "builtin:css-sans-bold";
pub const CSS_SERIF_REGULAR: &str = "builtin:css-serif-regular";
pub const CSS_SERIF_BOLD: &str = "builtin:css-serif-bold";
pub const CSS_MONO_REGULAR: &str = "builtin:css-mono-regular";
pub const CSS_MONO_BOLD: &str = "builtin:css-mono-bold";

/// Liberation Sans is an OFL-licensed, metrically compatible substitute for Arial.
pub fn css_sans_data(bold: bool) -> &'static [u8] {
    if bold { include_bytes!("../assets/fonts/LiberationSans-Bold.ttf") } else { include_bytes!("../assets/fonts/LiberationSans-Regular.ttf") }
}

/// Portable Times New Roman-compatible faces for CSS serif text.
pub fn css_serif_data(bold: bool) -> &'static [u8] {
    if bold { include_bytes!("../assets/fonts/LiberationSerif-Bold.ttf") } else { include_bytes!("../assets/fonts/LiberationSerif-Regular.ttf") }
}

/// Portable Courier New-compatible faces for CSS fixed-width text.
pub fn css_mono_data(bold: bool) -> &'static [u8] {
    if bold { include_bytes!("../assets/fonts/LiberationMono-Bold.ttf") } else { include_bytes!("../assets/fonts/LiberationMono-Regular.ttf") }
}

/// Notices for the exact portable faces, independent of an exported file's name.
pub fn css_font_notices(data: &[u8]) -> Option<[(&'static str, &'static str); 2]> {
    if let Some(notices) = css_sans_notices(data) {
        return Some(notices);
    }
    [css_serif_data(false), css_serif_data(true), css_mono_data(false), css_mono_data(true)].contains(&data).then_some([
        ("LiberationFonts-LICENSE.txt", include_str!("../assets/fonts/LiberationFonts-LICENSE.txt")),
        ("LiberationFonts-AUTHORS.txt", include_str!("../assets/fonts/LiberationFonts-AUTHORS.txt")),
    ])
}

/// Notices for these exact embedded faces, not merely a file with the same name.
pub fn css_sans_notices(data: &[u8]) -> Option<[(&'static str, &'static str); 2]> {
    (data == css_sans_data(false) || data == css_sans_data(true)).then_some([
        ("LiberationSans-LICENSE.txt", include_str!("../assets/fonts/LiberationSans-LICENSE.txt")),
        ("LiberationSans-AUTHORS.txt", include_str!("../assets/fonts/LiberationSans-AUTHORS.txt")),
    ])
}

fn builtin_name(name: &str) -> bool {
    matches!(name, "" | "regular" | "bold" | CSS_SANS_REGULAR | CSS_SANS_BOLD | CSS_SERIF_REGULAR | CSS_SERIF_BOLD | CSS_MONO_REGULAR | CSS_MONO_BOLD)
}

/// Makes fonts load through `reader` first, falling back to the disk. Set once, at start.
pub fn set_reader(reader: Reader) {
    let _ = READER.set(reader);
}

/// The bytes of a font: the built-in one (`None`, `regular`, `bold`) or a file, relative to `base`.
pub fn load(font: Option<&str>, base: Option<&Path>) -> Result<Arc<Vec<u8>>, String> {
    let file = match font {
        None | Some("regular") => return Ok(builtin(false)),
        Some("bold") => return Ok(builtin(true)),
        Some(CSS_SANS_REGULAR) => return Ok(css_sans(false)),
        Some(CSS_SANS_BOLD) => return Ok(css_sans(true)),
        Some(CSS_SERIF_REGULAR) => return Ok(css_serif(false)),
        Some(CSS_SERIF_BOLD) => return Ok(css_serif(true)),
        Some(CSS_MONO_REGULAR) => return Ok(css_mono(false)),
        Some(CSS_MONO_BOLD) => return Ok(css_mono(true)),
        Some(f) => f,
    };
    let path = match base {
        Some(b) if Path::new(file).is_relative() => b.join(file),
        _ => PathBuf::from(file),
    };
    if let Some(bytes) = READER.get().and_then(|read| read(&path)) {
        return Ok(Arc::new(bytes));
    }
    let mtime = std::fs::metadata(&path).and_then(|m| m.modified()).ok();
    let mut cache = cache().lock().map_err(|_| "кэш шрифтов недоступен".to_string())?;
    if let Some((t, bytes)) = cache.get(&path)
        && *t == mtime
    {
        return Ok(bytes.clone());
    }
    let bytes = Arc::new(std::fs::read(&path).map_err(|e| format!("шрифт {}: {e}", path.display()))?);
    cache.insert(path, (mtime, bytes.clone()));
    Ok(bytes)
}

/// The font files a script names (`font "fonts/Inter.ttf"`), as written, each once; the built-in
/// `regular` and `bold` are left out. Read from the script's words, so a comment or a text that
/// mentions a font does not count.
pub fn named(src: &str) -> Result<Vec<String>, String> {
    use crate::lexer::Tok;
    let toks = crate::lexer::lex(src).map_err(|e| e.to_string())?;
    let mut out: Vec<String> = Vec::new();
    for pair in toks.windows(2) {
        if let (Tok::Ident(key), Tok::Str(file)) = (&pair[0].tok, &pair[1].tok)
            && key == "font"
            && !builtin_name(file)
            && !out.contains(file)
        {
            out.push(file.clone());
        }
    }
    Ok(out)
}

/// The file name a font is saved under next to an exported theme.
pub fn file_name(font: Option<&str>) -> String {
    match font {
        None | Some("regular") => "IBMPlexSans-Regular.ttf".into(),
        Some("bold") => "IBMPlexSans-SemiBold.ttf".into(),
        Some(CSS_SANS_REGULAR) => "LiberationSans-Regular.ttf".into(),
        Some(CSS_SANS_BOLD) => "LiberationSans-Bold.ttf".into(),
        Some(CSS_SERIF_REGULAR) => "LiberationSerif-Regular.ttf".into(),
        Some(CSS_SERIF_BOLD) => "LiberationSerif-Bold.ttf".into(),
        Some(CSS_MONO_REGULAR) => "LiberationMono-Regular.ttf".into(),
        Some(CSS_MONO_BOLD) => "LiberationMono-Bold.ttf".into(),
        Some(f) => Path::new(f).file_name().map_or_else(|| f.to_string(), |n| n.to_string_lossy().into_owned()),
    }
}

fn builtin(bold: bool) -> Arc<Vec<u8>> {
    static REGULAR: OnceLock<Arc<Vec<u8>>> = OnceLock::new();
    static BOLD: OnceLock<Arc<Vec<u8>>> = OnceLock::new();
    if bold {
        BOLD.get_or_init(|| Arc::new(motif_core::text::BOLD_FONT.to_vec())).clone()
    } else {
        REGULAR.get_or_init(|| Arc::new(motif_core::text::DEFAULT_FONT.to_vec())).clone()
    }
}

fn css_sans(bold: bool) -> Arc<Vec<u8>> {
    static REGULAR: OnceLock<Arc<Vec<u8>>> = OnceLock::new();
    static BOLD: OnceLock<Arc<Vec<u8>>> = OnceLock::new();
    let cache = if bold { &BOLD } else { &REGULAR };
    cache.get_or_init(|| Arc::new(css_sans_data(bold).to_vec())).clone()
}

fn css_serif(bold: bool) -> Arc<Vec<u8>> {
    static REGULAR: OnceLock<Arc<Vec<u8>>> = OnceLock::new();
    static BOLD: OnceLock<Arc<Vec<u8>>> = OnceLock::new();
    let cache = if bold { &BOLD } else { &REGULAR };
    cache.get_or_init(|| Arc::new(css_serif_data(bold).to_vec())).clone()
}

fn css_mono(bold: bool) -> Arc<Vec<u8>> {
    static REGULAR: OnceLock<Arc<Vec<u8>>> = OnceLock::new();
    static BOLD: OnceLock<Arc<Vec<u8>>> = OnceLock::new();
    let cache = if bold { &BOLD } else { &REGULAR };
    cache.get_or_init(|| Arc::new(css_mono_data(bold).to_vec())).clone()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn portable_css_fonts_load_without_reading_a_file_and_keep_original_builtins() {
        for (name, bold, file) in [(CSS_SANS_REGULAR, false, "LiberationSans-Regular.ttf"), (CSS_SANS_BOLD, true, "LiberationSans-Bold.ttf")] {
            let bytes = load(Some(name), Some(Path::new("not-an-existing-font-folder"))).unwrap();
            assert_eq!(bytes.as_slice(), css_sans_data(bold));
            assert_eq!(file_name(Some(name)), file);
            let second = load(Some(name), None).unwrap();
            assert!(Arc::ptr_eq(&bytes, &second));
            let source = format!("artboard 200x100\ntext label content \"Play — Играть\" font \"{name}\"\n");
            let scene = crate::parse(&source).unwrap();
            let text = scene.doc.nodes[0].text.as_ref().unwrap();
            assert_eq!(text.font.as_ref().unwrap().name, file);
            assert_eq!(text.font.as_ref().unwrap().data.as_slice(), bytes.as_slice());
            assert!(!scene.doc.nodes[0].path().unwrap().elements().is_empty());
        }
        assert_eq!(load(None, None).unwrap().as_slice(), motif_core::text::DEFAULT_FONT);
        assert_eq!(load(Some("bold"), None).unwrap().as_slice(), motif_core::text::BOLD_FONT);
        assert_eq!(file_name(None), "IBMPlexSans-Regular.ttf");
    }

    #[test]
    fn builtin_css_aliases_are_not_reported_as_files_to_copy() {
        let source = format!(
            "text a font \"{CSS_SANS_REGULAR}\"\ntext b font \"{CSS_SANS_BOLD}\"\ntext c font \"fonts/custom.ttf\"\ntext d font bold\ntext e font \"{CSS_SERIF_REGULAR}\"\ntext f font \"{CSS_SERIF_BOLD}\"\ntext g font \"{CSS_MONO_REGULAR}\"\ntext h font \"{CSS_MONO_BOLD}\"\n"
        );
        assert_eq!(named(&source).unwrap(), vec!["fonts/custom.ttf"]);
    }

    #[test]
    fn serif_and_monospace_are_portable_and_include_only_their_own_notices() {
        for (name, data, file) in [
            (CSS_SERIF_REGULAR, css_serif_data(false), "LiberationSerif-Regular.ttf"),
            (CSS_SERIF_BOLD, css_serif_data(true), "LiberationSerif-Bold.ttf"),
            (CSS_MONO_REGULAR, css_mono_data(false), "LiberationMono-Regular.ttf"),
            (CSS_MONO_BOLD, css_mono_data(true), "LiberationMono-Bold.ttf"),
        ] {
            let bytes = load(Some(name), Some(Path::new("no-font-folder"))).unwrap();
            assert_eq!(bytes.as_slice(), data);
            assert!(Arc::ptr_eq(&bytes, &load(Some(name), None).unwrap()));
            assert_eq!(file_name(Some(name)), file);
            let scene = crate::parse(&format!("artboard 200x100\ntext t content \"Menu — Меню\" font \"{name}\"\n")).unwrap();
            let text = scene.doc.nodes[0].text.as_ref().unwrap();
            assert_eq!(text.font.as_ref().unwrap().name, file);
            assert!(!scene.doc.nodes[0].path().unwrap().elements().is_empty());
            assert!(css_font_notices(bytes.as_slice()).unwrap()[0].0.starts_with("LiberationFonts-"));
        }
        assert!(css_font_notices(motif_core::text::DEFAULT_FONT).is_none());
        let mono = motif_core::TextShape::new("iii", css_mono_data(false)).unwrap();
        let wide = motif_core::TextShape::new("WWW", css_mono_data(false)).unwrap();
        let style =
            motif_core::TextStyle { size: 13.0, line_height: 1.2, spacing: 0.0, align: motif_core::TextAlign::Left, reveal: 1.0, wave: 0.0, phase: 0.0 };
        assert_eq!(mono.bounds(&style).width(), wide.bounds(&style).width());
    }
}
