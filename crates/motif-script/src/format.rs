//! The version of the scene format. A file says it on its first line, `motif 1`; a file without
//! that line was written before versions and reads as version 1. When the format changes in a
//! way an older Motif cannot read, [`VERSION`] grows and Motif rewrites files to it on save.
//! Old files that must always open live in `tests/old-formats/`.

/// The newest format this Motif reads and the one it writes.
pub const VERSION: u32 = 1;

/// The version line Motif writes first.
pub fn line() -> String {
    format!("motif {VERSION}")
}

/// The version `src` says on its first line, if it says one (`motif 1`).
pub fn declared(src: &str) -> Option<u32> {
    let first = src.lines().next()?.trim();
    let mut words = first.split_whitespace();
    (words.next()? == "motif").then_some(())?;
    let v = words.next()?.parse().ok()?;
    words.next().is_none().then_some(v)
}

/// `src` as Motif saves it: the version line first, whatever version it was opened as.
pub fn stamp(src: &str) -> String {
    let body = match declared(src) {
        Some(_) => src.split_once('\n').map_or("", |(_, rest)| rest),
        None => src,
    };
    // Keep the file's line endings.
    let eol = if src.contains("\r\n") { "\r\n" } else { "\n" };
    format!("{}{eol}{body}", line())
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
}
