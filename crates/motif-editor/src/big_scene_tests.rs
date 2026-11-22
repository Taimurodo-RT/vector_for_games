//! Big scenes: what the editor does every frame and on every action, measured on 100, 1000 and
//! 10 000 objects.
//!
//! `cargo test --release -p motif-editor big_scenes_measured -- --ignored --nocapture` prints the
//! table and holds the scene of 1000 objects to the limits below. The picture itself is left
//! out: in the window it is drawn by the GPU.
//!
//! The ordinary test of this module needs no fast machine: it compares a scene with one ten
//! times bigger, and fails when some step grows as the square of the number of objects.

use super::editing_tests::fixture;
use super::snap_tests::button;
use super::*;
use std::hint::black_box;

/// A scene of `n` objects in groups of ten, like a screen of cards or an imported illustration:
/// each group holds shapes with fills and strokes, a curve and a nested pair.
pub(super) fn big_scene(n: usize) -> String {
    let groups = n / 10;
    let cols = (groups as f64).sqrt().ceil().max(1.0) as usize;
    let size = cols as f64 * 100.0 + 40.0;
    let mut s = format!("artboard \"Big\" {size}x{size} bg #f4f1ea\n");
    for g in 0..groups {
        let (x, y) = ((g % cols) as f64 * 100.0 + 70.0, (g / cols) as f64 * 100.0 + 70.0);
        let hue = ["#ff5a36", "#2a6fdb", "#4fd38c", "#ffd23f", "#c58cff"][g % 5];
        s.push_str(&format!(
            "group g{g} at {x} {y} {{
  rect g{g}plate size 88 88 r 10 fill #ffffff stroke #1b1d24 2
  rect g{g}bar at -26 0 size 14 60 r 4 fill {hue}
  ellipse g{g}dot at 14 -16 size 30 30 fill {hue} stroke #1b1d24 1.5
  star g{g}mark at 14 -16 size 16 16 points 5 inner 0.45 fill #ffffff
  path g{g}wave at 12 24 d \"M -22 0 C -12 -10 -6 10 0 0 C 6 -10 12 10 22 0\" fill none stroke #1b1d24 2
  polygon g{g}hex at 30 30 size 14 14 fill {hue}
  group g{g}pair at -26 -36 {{
    rect g{g}pa at -4 0 size 6 6 fill #1b1d24
    rect g{g}pb at 4 0 size 6 6 fill #1b1d24
  }}
}}
"
        ));
    }
    s
}

fn editor_frame(app: &mut App, events: Vec<egui::Event>) {
    let ctx = app.ctx.clone();
    ctx.run_ui(egui::RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1440.0, 900.0))), events, ..Default::default() }, |ui| {
        egui::Panel::left("big-layers").exact_size(260.0).show(ui, |ui| app.layers_panel(ui));
        egui::Panel::right("big-props").exact_size(320.0).show(ui, |ui| app.props_panel(ui));
        app.canvas_ui(ui, None);
    })
    .drop_without_applying_deltas();
}

/// Median time of `run`, in milliseconds.
fn median(count: usize, mut run: impl FnMut()) -> f64 {
    run();
    let mut samples: Vec<f64> = (0..count)
        .map(|_| {
            let start = Instant::now();
            run();
            start.elapsed().as_secs_f64() * 1000.0
        })
        .collect();
    samples.sort_by(f64::total_cmp);
    samples[count / 2]
}

/// What was measured on a scene of `n` objects: the name of each row and its time in ms.
pub(super) fn measure(n: usize, samples: usize) -> Vec<(&'static str, f64)> {
    let src = big_scene(n);
    let mut rows = Vec::new();
    rows.push(("разбор скрипта", median(samples.min(5), || drop(black_box(motif_script::parse(&src).unwrap())))));
    let start = Instant::now();
    let mut app = fixture(&src);
    app.canvas.no_raster = true;
    editor_frame(&mut app, vec![]);
    rows.push(("открытие до первого кадра", start.elapsed().as_secs_f64() * 1000.0));
    assert_eq!(app.scene.doc.nodes.len(), n / 10 * 10);

    rows.push(("поза для холста (display_doc)", median(samples, || drop(black_box(app.display_doc())))));
    let doc = app.display_doc();
    rows.push((
        "сцена для GPU (build_scene)",
        median(samples, || {
            let mut scene = motif_render::vello::Scene::new();
            motif_render::build_scene(&mut scene, &doc, motif_core::kurbo::Affine::IDENTITY, None);
            black_box(scene);
        }),
    ));
    let (w, h) = (doc.artboard.width, doc.artboard.height);
    rows.push((
        "попадание мышью, по объекту",
        median(samples, || {
            let _ = black_box(doc.hit_test(Point::new(w / 2.0 + 14.0, h / 2.0)));
        }),
    ));
    rows.push((
        "попадание мышью, мимо всех",
        median(samples, || {
            let _ = black_box(doc.hit_test(Point::new(w - 1.0, h - 1.0)));
        }),
    ));

    rows.push(("кадр редактора, ничего не выбрано", median(samples, || editor_frame(&mut app, vec![]))));
    // The pointer moves over the objects: every frame looks for what is under it.
    let middle = {
        let view = app.view.unwrap();
        view.to_screen(app.canvas_bounds, Point::new(w / 2.0 + 14.0, h / 2.0))
    };
    let mut step = 0.0;
    rows.push((
        "кадр с указателем над объектами",
        median(samples, || {
            step += 1.0;
            editor_frame(&mut app, vec![egui::Event::PointerMoved(middle + vec2(step % 7.0, 0.0))]);
        }),
    ));
    let one = format!("g{}bar", n / 20);
    app.select_only(Some(one.clone()));
    rows.push(("кадр с одним выбранным объектом", median(samples, || editor_frame(&mut app, vec![]))));

    // Dragging the selected object: the press, then every frame of the move.
    let at = {
        let view = app.view.unwrap();
        let doc = &app.scene.doc;
        view.to_screen(app.canvas_bounds, doc.world_transform(doc.index_of(&one).unwrap()) * Point::ZERO)
    };
    let before = app.src.clone();
    let start = Instant::now();
    editor_frame(&mut app, vec![egui::Event::PointerMoved(at), button(at, true, Modifiers::NONE)]);
    editor_frame(&mut app, vec![egui::Event::PointerMoved(at + vec2(12.0, 0.0))]);
    rows.push(("начало перетаскивания", start.elapsed().as_secs_f64() * 1000.0));
    assert!(app.drag.is_some(), "the object is being dragged");
    let mut k = 0.0;
    rows.push((
        "кадр перетаскивания",
        median(samples, || {
            k += 1.0;
            editor_frame(&mut app, vec![egui::Event::PointerMoved(at + vec2(12.0 + k % 9.0, k % 5.0))]);
        }),
    ));
    let start = Instant::now();
    editor_frame(&mut app, vec![egui::Event::PointerMoved(at + vec2(16.0, 4.0)), button(at + vec2(16.0, 4.0), false, Modifiers::NONE)]);
    rows.push(("конец перетаскивания (запись в скрипт)", start.elapsed().as_secs_f64() * 1000.0));
    assert!(app.src != before, "the drag is written into the script");

    let mut v = 0.0;
    rows.push((
        "правка числа в свойствах",
        median(samples.min(9), || {
            v += 1.0;
            app.edit(&one, Prop::Opacity, Value::Num(0.5 + (v % 5.0) / 10.0));
        }),
    ));
    let start = Instant::now();
    app.undo();
    rows.push(("отмена", start.elapsed().as_secs_f64() * 1000.0));

    app.select_all();
    rows.push(("кадр, выбрано всё (Ctrl+A)", median(samples.min(9), || editor_frame(&mut app, vec![]))));
    app.select_only(None);
    rows
}

/// What a scene of 1000 objects may take in a release build, in ms: a frame keeps 60 frames a
/// second, an action is felt as instant, the file opens at once.
const FRAME_MS: f64 = 16.0;
const ACTION_MS: f64 = 100.0;
const OPEN_MS: f64 = 500.0;

fn limit(row: &str) -> f64 {
    match row {
        "открытие до первого кадра" => OPEN_MS,
        "разбор скрипта" | "начало перетаскивания" | "конец перетаскивания (запись в скрипт)" | "правка числа в свойствах" | "отмена" => {
            ACTION_MS
        }
        _ => FRAME_MS,
    }
}

#[test]
#[ignore = "release measurement: prints the table of big scenes"]
fn big_scenes_measured() {
    let sizes = [100, 1000, 10_000];
    let tables: Vec<_> = sizes.iter().map(|&n| measure(n, if n > 1000 { 7 } else { 21 })).collect();
    println!("| что | {} |", sizes.map(|n| n.to_string()).join(" | "));
    println!("|---|{}", "---:|".repeat(sizes.len()));
    for (row, (name, _)) in tables[0].iter().enumerate() {
        let cells: Vec<String> = tables.iter().map(|t| format!("{:.2}", t[row].1)).collect();
        println!("| {name} | {} |", cells.join(" | "));
    }
    if cfg!(not(debug_assertions)) {
        for &(name, ms) in &tables[1] {
            assert!(ms <= limit(name), "1000 objects, «{name}»: {ms:.1} ms, the limit is {} ms", limit(name));
        }
    }
}

#[test]
fn ten_times_the_objects_take_about_ten_times_the_time() {
    // A step that looks through the whole scene for every object takes 100 times longer on a
    // scene 10 times bigger; one pass takes 10 times. The limit lies between, far from both, so
    // the test does not depend on the speed of the machine.
    //
    // The other tests run at the same time and take the processor away at random moments, so one
    // measurement can come out several times too long. The shortest time of a step is the one
    // nothing interfered with: it is taken over up to three rounds. A step that really grows as
    // the square stays over the limit in every round.
    const LIMIT: f64 = 35.0;
    let mut best: Vec<(&str, f64, f64)> = Vec::new();
    for _ in 0..3 {
        let (small, big) = (measure(200, 5), measure(2000, 5));
        if best.is_empty() {
            best = small.iter().map(|&(name, _)| (name, f64::INFINITY, f64::INFINITY)).collect();
        }
        for (row, (&(_, a), &(_, b))) in best.iter_mut().zip(small.iter().zip(&big)) {
            (row.1, row.2) = (row.1.min(a), row.2.min(b));
        }
        if best.iter().all(|&(_, a, b)| growth(a, b) < LIMIT) {
            break;
        }
    }
    for &(name, a, b) in &best {
        let times = growth(a, b);
        println!("{name}: {a:.2} -> {b:.2} ms, {times:.1} times");
        assert!(times < LIMIT, "«{name}»: {a:.2} ms on 200 objects, {b:.2} ms on 2000: {times:.0} times longer");
    }
}

/// How many times longer the big scene took. Below a tenth of a millisecond the clock is too
/// coarse to divide by.
fn growth(small: f64, big: f64) -> f64 {
    big / small.max(0.1)
}
