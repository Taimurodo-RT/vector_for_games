//! Repeatable CPU-side editor measurements; native GPU/presentation FPS is checked by the user.
use super::editing_tests::fixture;
use super::*;
use std::hint::black_box;

fn measure(mut run: impl FnMut(), count: usize) -> serde_json::Value {
    for _ in 0..3 {
        run();
    }
    let mut samples = Vec::new();
    for _ in 0..count {
        let start = Instant::now();
        run();
        samples.push(start.elapsed().as_secs_f64() * 1000.0);
    }
    samples.sort_by(f64::total_cmp);
    serde_json::json!({"median_ms":samples[count/2],"mean_ms":samples.iter().sum::<f64>()/count as f64,"p95_ms":samples[(count*95/100).min(count-1)],"samples":count})
}

fn editor_frame(app: &mut App) {
    let ctx = app.ctx.clone();
    ctx.run_ui(egui::RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1440.0, 900.0))), ..Default::default() }, |ui| {
        egui::Panel::right("performance-props").exact_size(320.0).show(ui, |ui| app.props_panel(ui));
        app.canvas_ui(ui, None);
    })
    .drop_without_applying_deltas();
}

#[test]
#[ignore = "release measurement: set MOTIF_PERF_OUTPUT to a local laboratory report"]
fn anime_editor_cpu_measurement() {
    let scene = include_str!("../../../examples/character-anime.msc");
    let mut app = fixture(scene);
    app.select_only(Some("portrait".into()));
    app.select_anim("Combinations".into());
    app.time = 1.0;
    let mut frame = 0;
    let pose = measure(
        || {
            app.time = 1.0 + (frame % 17) as f64 * 0.013;
            frame += 1;
            black_box(app.display_doc());
        },
        40,
    );
    let mut edit_frame = 0;
    let editing = measure(
        || {
            app.edit("turn", Prop::Parameter, Value::Num(-0.95 + (edit_frame % 39) as f64 * 0.05));
            edit_frame += 1;
        },
        40,
    );
    let idle = measure(|| editor_frame(&mut app), 25);
    let interactive = measure(
        || {
            app.edit("turn", Prop::Parameter, Value::Num(-0.95 + (edit_frame % 39) as f64 * 0.05));
            edit_frame += 1;
            editor_frame(&mut app);
        },
        25,
    );
    let report = serde_json::json!({"scene":"character-anime","nodes":app.scene.doc.nodes.len(),"display_doc":pose,"author_parameter":editing,"editor_idle_frame":idle,"editor_parameter_frame":interactive,"scope":"release CPU; excludes native GPU/presentation and OS clipboard"});
    if let Ok(path) = std::env::var("MOTIF_PERF_OUTPUT") {
        std::fs::write(path, serde_json::to_vec_pretty(&report).unwrap()).unwrap();
    }
    println!("{report}");
}

#[test]
fn displayed_pose_matches_export_pose_without_extra_rigging() {
    for source in
        [include_str!("../../../examples/character-anime.msc"), include_str!("../../../examples/bounce.msc"), include_str!("../../../examples/ropes.msc")]
    {
        let mut app = fixture(source);
        let name = app.scene.doc.animations[0].name.clone();
        app.select_anim(name);
        for t in [0.0, 0.3, 0.7] {
            app.time = t;
            assert_eq!(app.display_doc(), app.pose_anim(app.current_anim().unwrap(), t));
        }
    }
}

#[test]
fn visual_parse_failure_preserves_the_last_scene_and_history() {
    let mut app = fixture(include_str!("../../../examples/character-anime.msc"));
    let before = app.src.clone();
    let doc = app.scene.doc.clone();
    assert!(!app.apply_once("invalid", Some("artboard 200x200\nrect broken unknown 1\n".into())));
    assert_eq!(app.src, before);
    assert_eq!(app.scene.doc, doc);
    assert!(app.undo.is_empty() && app.errors.is_empty());
    app.edit("turn", Prop::Parameter, Value::Num(0.5));
    assert!(app.src != before);
    assert_eq!(app.undo.len(), 1);
    app.undo();
    assert_eq!(app.src, before);
    assert_eq!(app.scene.doc, doc);
}
