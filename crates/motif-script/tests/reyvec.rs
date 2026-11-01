//! Every example with morphing shapes has its sample file for the ReyVec engine next to it
//! (`NAME.reyvec.json`): the file the engine reads must not change by accident. After a
//! deliberate change, rewrite the samples with `MOTIF_BLESS=1 cargo test -p motif-script --test reyvec`.

use std::path::{Path, PathBuf};

fn examples() -> Vec<PathBuf> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples");
    let mut found = Vec::new();
    for dir in [root.clone(), root.join("screens")] {
        for entry in std::fs::read_dir(&dir).unwrap() {
            let path = entry.unwrap().path();
            if path.extension().is_some_and(|e| e == "msc") {
                found.push(path);
            }
        }
    }
    found.sort();
    found
}

#[test]
fn every_morphing_example_matches_its_sample_file() {
    let bless = std::env::var_os("MOTIF_BLESS").is_some();
    let (mut wrong, mut seen) = (Vec::new(), 0);
    for msc in examples() {
        let src = std::fs::read_to_string(&msc).unwrap();
        let boards = motif_script::parse_boards(&src, msc.parent()).unwrap_or_else(|e| panic!("{}: {e:?}", msc.display()));
        let (file, _) = motif_script::reyvec_morph(boards.iter().map(|s| &s.doc));
        let sample = msc.with_extension("reyvec.json");
        if file.shapes.is_empty() {
            assert!(!sample.exists(), "{} без перетекающих фигур, а образец есть", msc.display());
            continue;
        }
        seen += 1;
        let (json, _) = motif_script::reyvec_morph_json(boards.iter().map(|s| &s.doc));
        if bless {
            std::fs::write(&sample, &json).unwrap();
        } else if std::fs::read_to_string(&sample).ok().map(|s| s.replace("\r\n", "\n")) != Some(json) {
            wrong.push(sample.display().to_string());
        }
    }
    assert!(seen >= 1);
    assert!(wrong.is_empty(), "файл для ReyVec не совпал с образцом (MOTIF_BLESS=1 перепишет образцы):\n{}", wrong.join("\n"));
}

#[test]
fn the_file_gives_the_engine_what_its_contract_asks_for() {
    let src = "artboard \"Рот\" 200x100\n\
        path mouth at 100 30 d \"M -40 0 L 40 0 L 0 20 Z\" forms \"M -30 -10 L 30 -10 L 0 30 Z M -5 0 L 5 0 L 0 8 Z\" match \"0>1 0:0=0:0 0:0.5=0:0.5 *=1\" fill #9d3b5a80 evenodd\n\
        star s hidden fill #ff0000\n\
        ellipse ball size 40 40 fill #00ff00 stroke #000 2 morph s\n\
        ellipse glow at 20 20 size 40 20 gradient radial #ffffff #ffffff00 morph s\n\
        path bar at 50 50 d \"M -20 -10 L 20 -10 L 20 10 L -20 10 Z\" rot 90 gradient linear 0 #000 #fff #f00 forms \"M -20 -10 L 20 -10 L 20 10 L -20 10 Z\"\n\
        anim Talk 1s loop id=t4lk {\n  mouth.contour 0: 0 | 0.5: 1 | 1: 0\n}\n";
    let boards = motif_script::parse_boards(src, None).unwrap();
    let (json, warnings) = motif_script::reyvec_morph_json(boards.iter().map(|s| &s.doc));
    // The format and its version come first.
    assert!(json.starts_with("{\n  \"format\": \"reyvec-morph\",\n  \"version\": 1,\n  \"y\": \"up\",\n  \"units\": \"px\","), "{json}");
    let file: motif_script::MorphFile = serde_json::from_str(&json).unwrap();
    assert_eq!(file, motif_script::reyvec_morph(boards.iter().map(|s| &s.doc)).0, "reads back as written");
    let mouth = &file.shapes[0];
    assert_eq!((mouth.board.as_str(), mouth.id.as_str(), mouth.fill_rule.as_str()), ("Рот", "mouth", "evenodd"));
    assert_eq!((mouth.color.as_deref(), mouth.alpha, &mouth.gradient), (Some("#9d3b5a"), Some(0.502), &None));
    // Where it stands on the 200×100 board, y up from the bottom-left corner: 30 from the top
    // is 70 from the bottom.
    assert_eq!(mouth.transform, [1.0, 0.0, 0.0, 1.0, 100.0, 70.0]);
    // y points up: the tip drawn below (y 20) is above in the file.
    assert_eq!(mouth.contours, ["M -40 0 L 40 0 L 0 -20 Z", "M -30 10 L 30 10 L 0 -30 Z M -5 0 L 5 0 L 0 -8 Z"]);
    assert_eq!(mouth.marks.len(), 3);
    assert_eq!(mouth.marks[1].to.as_ref().unwrap().at, Some(0.5));
    assert!(mouth.marks[2].from.is_none() && mouth.marks[2].to == Some(motif_script::reyvec::Place { piece: 1, at: None }));
    // The morph number frame by frame, 60 a second, ends included.
    let talk = &mouth.animations[0];
    assert_eq!((talk.animation.as_str(), talk.id.as_deref(), talk.looping, talk.morph.len()), ("Talk", Some("t4lk"), true, 61));
    assert_eq!((talk.morph[0], talk.morph[30], talk.morph[15], talk.morph[60]), (0.0, 1.0, 0.5, 0.0));
    // A gradient goes in place of the colour, in the contours' coordinates (y up).
    let glow = file.shapes.iter().find(|s| s.id == "glow").unwrap();
    let g = glow.gradient.as_ref().unwrap();
    assert_eq!((glow.color.as_ref(), glow.alpha), (None, None));
    assert_eq!((g.kind.as_str(), g.center, g.radius, g.focus, g.start), ("radial", Some([0.0, 0.0]), Some(20.0), Some([0.0, 0.0]), None));
    assert_eq!((g.stops.len(), g.stops[1].at, g.stops[1].color.as_str(), g.stops[1].alpha), (2, 1.0, "#ffffff", 0.0));
    let bar = file.shapes.iter().find(|s| s.id == "bar").unwrap();
    let g = bar.gradient.as_ref().unwrap();
    assert_eq!((g.kind.as_str(), g.start, g.end, g.stops[1].at), ("linear", Some([-20.0, 0.0]), Some([20.0, 0.0]), 0.5));
    // Turned by 90° on the board (y down) is turned by −90° with y up.
    assert_eq!(bar.transform, [0.0, -1.0, 1.0, 0.0, 50.0, 50.0]);
    // What the engine would not show is said, not written.
    assert_eq!(file.shapes[1].id, "ball");
    assert!(file.shapes[1].animations.is_empty());
    assert!(warnings.iter().any(|w| w.contains("«ball»") && w.contains("обводка")), "{warnings:?}");
    assert!(warnings.iter().any(|w| w.contains("«ball»") && w.contains("другой цвет")), "{warnings:?}");
    assert!(!warnings.iter().any(|w| w.contains("«mouth»")), "{warnings:?}");
    // A gradient follows the shape in Motif but stays put in the engine.
    assert!(warnings.iter().any(|w| w.contains("«glow»") && w.contains("градиент")), "{warnings:?}");
    assert!(!warnings.iter().any(|w| w.contains("«bar»")), "{warnings:?}");
}
