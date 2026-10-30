//! Files of every past format must always open: `tests/old-formats/` keeps them, one folder per
//! format. Each opens, plays its animations, and saved in the current format (the version line
//! first) gives the very same scenes.

use std::path::{Path, PathBuf};

fn files(dir: &Path, out: &mut Vec<PathBuf>) {
    let mut entries: Vec<PathBuf> = std::fs::read_dir(dir).unwrap().map(|e| e.unwrap().path()).collect();
    entries.sort();
    for p in entries {
        if p.is_dir() {
            files(&p, out);
        } else if p.extension().is_some_and(|x| x == "msc") {
            out.push(p);
        }
    }
}

#[test]
fn every_old_format_file_opens_plays_and_saves_the_same() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/old-formats");
    let mut all = Vec::new();
    files(&root, &mut all);
    assert!(all.len() >= 40, "the folder of old files is there: {}", all.len());
    // Fonts the examples use lie next to them.
    let fonts = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples");
    for path in all {
        let src = std::fs::read_to_string(&path).unwrap();
        let old = motif_script::parse_boards(&src, Some(&fonts)).unwrap_or_else(|e| panic!("{}: {e:?}", path.display()));
        for scene in &old {
            for a in &scene.doc.animations {
                for t in [0.0, a.duration * 0.5, a.duration] {
                    let pose = a.pose(&scene.doc, t);
                    assert!(pose.nodes.iter().all(|n| n.local_transform().as_coeffs().iter().all(|c| c.is_finite())), "{}: {} at {t}", path.display(), a.name);
                }
            }
        }
        let saved = motif_script::format::stamp(&src);
        assert_eq!(motif_script::format::declared(&saved), Some(motif_script::format::VERSION));
        let new = motif_script::parse_boards(&saved, Some(&fonts)).unwrap_or_else(|e| panic!("{} saved: {e:?}", path.display()));
        assert_eq!(new.len(), old.len(), "{}", path.display());
        for (a, b) in old.iter().zip(&new) {
            assert_eq!(a.doc, b.doc, "{}: board {}", path.display(), a.doc.artboard.name);
        }
    }
}

#[test]
fn the_version_line_is_checked() {
    let board = "artboard \"a\" 10x10\nrect r size 4\n";
    assert!(motif_script::parse(&format!("motif 1\n{board}")).is_ok());
    let newer = motif_script::parse(&format!("motif 2\n{board}")).unwrap_err();
    assert!(newer[0].message.contains("более новой версией"), "{}", newer[0].message);
    assert_eq!(newer[0].line, 1);
    assert!(motif_script::parse(&format!("motif 1.5\n{board}")).is_err());
    assert!(motif_script::parse(&format!("motif 0\n{board}")).is_err());
    let late = motif_script::parse(&format!("{board}motif 1\n")).unwrap_err();
    assert!(late[0].message.contains("первой строкой"), "{}", late[0].message);
    // Every board of a file with several reads the version from the shared head.
    let two = format!("motif 1\nlet c = #ff0000\n{board}artboard \"b\" 10x10\nrect q size 4 fill c\n");
    assert_eq!(motif_script::parse_boards(&two, None).unwrap().len(), 2);
    // A palette color added to a versioned file goes after the version line.
    let src = format!("motif 1\n{board}");
    let scene = motif_script::parse(&src).unwrap();
    let out = motif_script::add_swatch(&src, &scene, "ink", motif_core::Color::BLACK);
    assert!(out.starts_with("motif 1\nlet ink = #000000\n"), "{out}");
    assert!(motif_script::parse(&out).is_ok());
}
