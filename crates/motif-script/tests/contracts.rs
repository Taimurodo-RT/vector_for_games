//! Every example in `examples/` has its sample contract next to it (`NAME.contract.json`): the
//! contract a game binds to must not change by accident. After a deliberate change, rewrite the
//! samples with `MOTIF_BLESS=1 cargo test -p motif-script --test contracts`.

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
fn every_example_matches_its_sample_contract() {
    let bless = std::env::var_os("MOTIF_BLESS").is_some();
    let mut wrong = Vec::new();
    for msc in examples() {
        let src = std::fs::read_to_string(&msc).unwrap();
        let boards = motif_script::parse_boards(&src, msc.parent()).unwrap_or_else(|e| panic!("{}: {e:?}", msc.display()));
        let json = motif_script::contract_json(boards.iter().map(|s| &s.doc));
        let sample = msc.with_extension("contract.json");
        if bless {
            std::fs::write(&sample, &json).unwrap();
        } else if std::fs::read_to_string(&sample).ok().map(|s| s.replace("\r\n", "\n")) != Some(json) {
            wrong.push(sample.display().to_string());
        }
    }
    assert!(wrong.is_empty(), "договор не совпал с образцом (MOTIF_BLESS=1 перепишет образцы):\n{}", wrong.join("\n"));
}

#[test]
fn a_contract_reads_back_as_it_was_written() {
    let src = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/directions.msc")).unwrap();
    let boards = motif_script::parse_boards(&src, None).unwrap();
    let contract = motif_script::contract(boards.iter().map(|s| &s.doc));
    let back: motif_script::Contract = serde_json::from_str(&motif_script::contract_json(boards.iter().map(|s| &s.doc))).unwrap();
    assert_eq!(back, contract);
    let sides = contract.boards[0].sides.as_ref().unwrap();
    assert_eq!(sides.names.len(), 8);
}

#[test]
fn the_contract_names_what_a_game_binds_to() {
    let src = "artboard \"Кнопка\" 200x80\nrect btn at 100 40 size 160 48 fill #333\n\
        parameter mood \"Настроение\" range -1 1 default 0.5 value 0\n\
        anim Idle 1s loop {\n btn.x 0: 100 | 1: 100\n event blink 0.6\n event tick 0.2\n}\n\
        anim Hover 0.2s {\n btn.x 0: 100 | 0.2: 104\n}\n\
        machine Ui {\n input hover bool\n input clicks 2\n input press trigger\n\
        state Idle\n state Over anim Hover\n Idle -> Over when hover\n listen btn hover -> hover\n}\n";
    let scene = motif_script::parse(src).unwrap();
    let c = motif_script::contract([&scene.doc]);
    let b = &c.boards[0];
    assert_eq!((b.name.as_str(), b.width, b.height), ("Кнопка", 200.0, 80.0));
    let idle = &b.animations[0];
    assert!(idle.looping);
    assert_eq!(idle.events.iter().map(|e| (e.name.as_str(), e.time)).collect::<Vec<_>>(), [("tick", 0.2), ("blink", 0.6)]);
    assert_eq!(b.events, ["blink", "tick"]);
    let m = &b.machines[0];
    assert_eq!(
        m.inputs.iter().map(|i| (i.name.as_str(), i.kind.as_str(), i.default)).collect::<Vec<_>>(),
        [("hover", "bool", 0.0), ("clicks", "number", 2.0), ("press", "trigger", 0.0)]
    );
    assert_eq!((m.states[1].name.as_str(), m.states[1].animation.as_str()), ("Over", "Hover"));
    assert_eq!((m.pointer[0].node.as_str(), m.pointer[0].event.as_str(), m.pointer[0].input.as_str()), ("btn", "hover", "hover"));
    let p = &b.parameters[0];
    assert_eq!((p.id.as_str(), p.min, p.max, p.default), ("mood", -1.0, 1.0, 0.5));
    assert!(b.sides.is_none());
}
