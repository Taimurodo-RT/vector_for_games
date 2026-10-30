//! The player on the drawing threads, and what a frame costs.

use crate::Player;
use motif_render::mesh::Cutter;

#[test]
fn a_frame_drawn_on_the_drawing_threads_is_the_frame_drawn_here() {
    let walker = std::fs::read_to_string("../../examples/directions.msc").unwrap();
    let mut w = Player::load(&walker, None, "", "", "").unwrap();
    w.set_side("left").unwrap();
    w.advance(0.3);
    let pending: Vec<_> = (0..4)
        .map(|_| {
            let f = w.frame();
            crate::pool::start(move || f.render(0.75, true))
        })
        .collect();
    let here = w.render(0.75, true).unwrap();
    for p in pending {
        assert_eq!(p.wait().flatten().unwrap().data(), here.data());
    }
}

/// How long a frame of the example characters takes, apart from Godot:
/// `cargo test --release -p motif-godot frame_cost -- --ignored --nocapture`.
#[test]
#[ignore]
fn frame_cost() {
    for f in ["directions", "character-platformer", "character-anime"] {
        let src = std::fs::read_to_string(format!("../../examples/{f}.msc")).unwrap();
        let mut p = Player::load(&src, Some(std::path::Path::new("../../examples")), "", "", "").unwrap();
        let n = 200;
        let t = std::time::Instant::now();
        for _ in 0..n {
            p.advance(1.0 / 60.0);
        }
        let advance = t.elapsed().as_secs_f64() * 1000.0 / n as f64;
        let t = std::time::Instant::now();
        for _ in 0..n {
            p.render(1.0, false);
        }
        let render = t.elapsed().as_secs_f64() * 1000.0 / n as f64;
        let mut cutter = Cutter::default();
        let t = std::time::Instant::now();
        for _ in 0..n {
            p.advance(1.0 / 60.0);
            p.frame().pieces(&mut cutter, 1.0);
        }
        let pieces = t.elapsed().as_secs_f64() * 1000.0 / n as f64 - advance;
        let (w, h) = p.size();
        println!("{f}: advance {advance:.3} ms, render {render:.3} ms, pieces {pieces:.3} ms ({w}x{h})");
    }
}
