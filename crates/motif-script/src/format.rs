//! The version of the scene format. A file says it on its first line, `motif 1`; a file without
//! that line was written before versions and reads as version 1. When the format gains something
//! an older Motif cannot read, [`VERSION`] grows. A file is saved with the lowest version that
//! reads it ([`needed`]): a scene that uses nothing new stays `motif 1`, so a game built with an
//! older Motif keeps opening it. Old files that must always open live in `tests/old-formats/`.
//!
//! - 2: `guide x 120` — guide lines of the editor.
//! - 3: `group … combine subtract` — live boolean operations.
//! - 4: `style card fill #ffffff` — named styles; `let sky = linear 90 #a #b` — gradients of
//!   the palette; `theme dark { … }` — themes.
//! - 5: `variant main face=#ff5a36 hide icon show badge` inside a symbol and `variant main` on a `use`
//!   line — variants of a symbol; `title.content = "Купить"` in `override` — words of a text
//!   for one copy.
//! - 6: `group … repeat grid 4 2 step 40 30` — live repeats of what is in a group.
//! - 7: `include "kit.msc"` — the colors, styles, themes and symbols of another file.
//! - 8: `artboard "Трава" 64x64 tile` — an artboard that repeats as a tile of a field.

/// The newest format this Motif reads.
pub const VERSION: u32 = 8;

/// The lowest format version that reads `src`: the first one that has everything it uses.
pub fn needed(src: &str) -> u32 {
    // A statement starts its line; `guide` came with version 2.
    let guide = src.lines().any(|l| l.split_whitespace().next() == Some("guide"));
    // A property of a group, somewhere on its line after the id; `combine` came with version 3.
    let combine = src.lines().any(|l| {
        // Names are quoted and comments end the line: neither is a property.
        let code: String = l.split("//").next().unwrap_or("").split('"').step_by(2).collect::<Vec<_>>().join(" ");
        let mut words = code.split_whitespace();
        words.next() == Some("group") && words.skip(1).any(|w| w == "combine")
    });
    // A statement starts its line; `style` came with version 4.
    let style = src.lines().any(|l| l.split_whitespace().next() == Some("style"));
    // A gradient of the palette (`let sky = linear 90 #a #b`) came with version 4 too.
    let gradient = src.lines().any(|l| {
        l.split_whitespace().next() == Some("let")
            && l.split_once('=').is_some_and(|(_, value)| matches!(value.split_whitespace().next(), Some("linear" | "radial")))
    });
    // A theme (`theme dark {`) came with version 4 as well; its lines of styles are found above.
    let theme = src.lines().any(|l| l.split_whitespace().next() == Some("theme"));
    // A variant of a symbol came with version 5: its line inside the symbol, and the word on
    // the line of a copy after the name of the symbol and the id of the copy.
    let variant = src.lines().any(|l| {
        let code: String = l.split("//").next().unwrap_or("").split('"').step_by(2).collect::<Vec<_>>().join(" ");
        let mut words = code.split_whitespace();
        match words.next() {
            Some("variant") => true,
            Some("use") => words.skip(2).any(|w| w == "variant"),
            _ => false,
        }
    });
    // The words of a text in `override` came with version 5 too: `title.content = "…"`.
    let own_text =
        src.lines().any(|l| l.split_once('=').is_some_and(|(path, _)| path.trim().ends_with(".content") && !path.trim().contains(char::is_whitespace)));
    // A live repeat came with version 6: a property of a group, as `combine` is.
    let repeat = src.lines().any(|l| {
        let code: String = l.split("//").next().unwrap_or("").split('"').step_by(2).collect::<Vec<_>>().join(" ");
        let mut words = code.split_whitespace();
        words.next() == Some("group") && words.skip(1).any(|w| w == "repeat")
    });
    // A statement starts its line; `include` came with version 7.
    // The words the library finds a symbol by (`tags кнопка меню`) came with version 7 too.
    let include = src.lines().any(|l| matches!(l.split_whitespace().next(), Some("include" | "tags")));
    // An artboard that repeats as a tile (`artboard … tile`) came with version 8.
    let tile = src.lines().any(|l| {
        let code: String = l.split("//").next().unwrap_or("").split('"').step_by(2).collect::<Vec<_>>().join(" ");
        let mut words = code.split_whitespace();
        words.next() == Some("artboard") && words.any(|w| matches!(w, "tile" | "tile-x" | "tile-y"))
    });
    if tile {
        8
    } else if include {
        7
    } else if repeat {
        6
    } else if variant || own_text {
        5
    } else if style || gradient || theme {
        4
    } else if combine {
        3
    } else if guide {
        2
    } else {
        1
    }
}

/// The version `src` says on its first line, if it says one (`motif 1`).
pub fn declared(src: &str) -> Option<u32> {
    let first = src.lines().next()?.trim();
    let mut words = first.split_whitespace();
    (words.next()? == "motif").then_some(())?;
    let v = words.next()?.parse().ok()?;
    words.next().is_none().then_some(v)
}

/// `src` as Motif saves it: the version it needs on the first line, whatever it said before.
pub fn stamp(src: &str) -> String {
    stamp_as(src, needed(src))
}

/// `src` with `version` on its first line. For a script that is one part of a file (the head and
/// one board of several): the version is the one the whole file needs.
pub fn stamp_as(src: &str, version: u32) -> String {
    let body = match declared(src) {
        Some(_) => src.split_once('\n').map_or("", |(_, rest)| rest),
        None => src,
    };
    // Keep the file's line endings.
    let eol = if src.contains("\r\n") { "\r\n" } else { "\n" };
    format!("motif {version}{eol}{body}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_version_line_is_read_and_written_first() {
        assert_eq!(declared("motif 1\nartboard \"a\" 10x10\n"), Some(1));
        assert_eq!(declared("artboard \"a\" 10x10\n"), None);
        assert_eq!(declared("motif one\n"), None);
        assert_eq!(stamp("artboard \"a\" 10x10\n"), "motif 1\nartboard \"a\" 10x10\n");
        assert_eq!(stamp("motif 1\nartboard \"a\" 10x10\n"), "motif 1\nartboard \"a\" 10x10\n");
        assert_eq!(stamp("// x\r\nlet a = 1\r\n"), "motif 1\r\n// x\r\nlet a = 1\r\n");
        assert_eq!(stamp(""), "motif 1\n");
    }

    #[test]
    fn a_file_is_saved_with_the_lowest_version_that_reads_it() {
        // Nothing new in it: an older Motif, and a game built with one, keeps reading the file.
        assert_eq!(needed("artboard \"a\" 10x10\nrect guide at 0 0 size 4 4\n// guide x 5\n"), 1);
        // Guide lines came with version 2, and going back needs them gone.
        let with = "motif 1\nartboard \"a\" 10x10\nguide x 5\n";
        assert_eq!(stamp(with), "motif 2\nartboard \"a\" 10x10\nguide x 5\n");
        assert_eq!(stamp("motif 2\nartboard \"a\" 10x10\n"), "motif 1\nartboard \"a\" 10x10\n");
        // Live boolean operations came with version 3.
        let live = "artboard \"a\" 10x10\ngroup g combine subtract {\n  rect a size 4 4\n  rect b size 2 2\n}\n";
        assert_eq!(needed(live), 3);
        assert_eq!(needed(&format!("{live}guide x 5\n")), 3);
        // Named styles came with version 4.
        let styled = "artboard \"a\" 10x10\nstyle card fill #ffffff\nrect a size 4 4 style card\n";
        assert_eq!(needed(styled), 4);
        assert_eq!(stamp(styled), format!("motif 4\n{styled}"));
        // Variants of a symbol came with version 5: the line in the symbol or the word at a copy.
        let coin = "artboard \"a\" 10x10\nsymbol Coin size=4 {\n  rect body size {size}\n}\n";
        assert_eq!(needed(&format!("{coin}use Coin c1\n")), 1);
        assert_eq!(needed(&coin.replace("  rect", "  variant big size=8\n  rect")), 5);
        let big = coin.replace("  rect", "  variant big size=8\n  rect");
        assert_eq!(stamp(&format!("{big}use Coin c1 variant big\n")), format!("motif 5\n{big}use Coin c1 variant big\n"));
        // Live repeats came with version 6.
        let rivets = "artboard \"a\" 10x10\ngroup rivets repeat grid 4 1 step 8 0 {\n  ellipse rivet size 2 2\n}\n";
        assert_eq!(needed(rivets), 6);
        assert_eq!(stamp(rivets), format!("motif 6\n{rivets}"));
        // The lines of another file came with version 7.
        let screen = "include \"kit.msc\"\nartboard \"a\" 10x10\n";
        assert_eq!(needed(screen), 7);
        assert_eq!(stamp(&format!("motif 1\n{screen}")), format!("motif 7\n{screen}"));
        // An object called `include` and a comment with the word are not the line.
        assert_eq!(needed("artboard \"a\" 10x10\nrect include at 0 0 size 4 4 // include \"kit.msc\"\n"), 1);
        // A group called `repeat`, a name and a comment with the word are not a repeat.
        assert_eq!(needed("artboard \"a\" 10x10\ngroup repeat \"we repeat it\" { // repeat\n}\n"), 1);
        // A copy called `variant`, a name and a comment with the word are not a variant.
        assert_eq!(needed(&format!("{coin}use Coin variant \"variant big\" at 1 1 // variant big\n")), 1);
        // An object called `style` and a comment with the word are not a style.
        assert_eq!(needed("artboard \"a\" 10x10\nrect style at 0 0 size 4 4 // style card\n"), 1);
        // A group called `combine`, a name and a comment with the word are not the property.
        assert_eq!(needed("artboard \"a\" 10x10\ngroup combine \"we combine it\" { // combine\n}\n"), 1);
    }
}
