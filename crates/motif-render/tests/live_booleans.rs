//! A live boolean operation (`group … combine subtract`) is drawn exactly as the one-time
//! command of the editor leaves the same shapes, and follows its shapes when they move.

mod common;

use common::{HEAD, difference, draw, exact_picture};
use motif_script::{BoolOp, boolean, parse};

/// The shapes of an operation, bottom first, as lines of a scene.
const SHAPES: [&str; 3] = [
    "rect base at 0 0 size 110 80 r 12 fill #ff5a36 stroke #1b1d24 3",
    "ellipse hole at 40 -20 size 70 70 fill #6f93ff rot 20",
    "star spike at 15 5 size 70 70 points 5 inner 0.45 fill #ffd34d scale 1.2 0.8",
];

fn live(place: &str, op: &str) -> String {
    let shapes: String = SHAPES.iter().map(|s| format!("  {s}\n")).collect();
    format!("{HEAD}group icon {place} combine {op} {{\n{shapes}}}\n")
}

fn plain(place: &str) -> String {
    let shapes: String = SHAPES.iter().map(|s| format!("  {s}\n")).collect();
    format!("{HEAD}group icon {place} {{\n{shapes}}}\n")
}

#[test]
fn a_live_operation_is_drawn_as_the_one_time_command_leaves_it() {
    let ids: Vec<String> = ["base", "hole", "spike"].map(String::from).to_vec();
    for place in ["at 120 90", "at 120 90 rot 30 scale 0.9 1.1", "at 110 95 skew 15 scale -1 1"] {
        for (word, op) in [("union", BoolOp::Union), ("subtract", BoolOp::Subtract), ("intersect", BoolOp::Intersect), ("exclude", BoolOp::Exclude)] {
            let before = plain(place);
            let scene = parse(&before).unwrap();
            let (cut, _) = boolean(&before, &scene, &ids, op).unwrap_or_else(|e| panic!("{e}\n{before}"));
            exact_picture(&cut, &live(place, word), &format!("{word} {place}"));
            // The operation does change the picture: the test is not comparing two plain groups.
            let (soft, _) = difference(&draw(&before), &draw(&cut));
            assert!(soft > 100, "{word}: {soft}");
        }
    }
}

#[test]
fn the_result_follows_shapes_that_a_clip_moves() {
    let moving = format!("{}anim Slide 2s {{\n  hole.x 0: 40 | 2: -30\n  spike.rot 0: 0 | 2: 90\n}}\n", live("at 120 90", "subtract"));
    // `draw` shows the middle of the clip: the same as a still scene with the shapes put there.
    let still = live("at 120 90", "subtract").replace("ellipse hole at 40 -20", "ellipse hole at 5 -20").replace("scale 1.2 0.8", "scale 1.2 0.8 rot 45");
    exact_picture(&moving, &still, "the middle of the clip");
    let (soft, _) = difference(&draw(&moving), &draw(&live("at 120 90", "subtract")));
    assert!(soft > 100, "the clip moves the cut: {soft}");
}

#[test]
fn bones_and_inner_operations_shape_the_result() {
    // A bent shape cuts with its bent outline.
    let bent = format!(
        "{HEAD}bone arm at 60 90 len 60 {{\n  bone hand at 60 0 len 60 rot 70\n}}\ngroup icon at 0 0 combine subtract {{\n  rect base at 120 90 size 200 120 fill #ff5a36\n  rect bar at 120 90 size 140 24 fill #000000 skin arm hand\n}}\n"
    );
    let apart = bent.replace(" combine subtract", "").replace("fill #000000", "fill #f4efe6");
    exact_picture(&apart, &bent, "a bent cutter");
    // An inner operation is one shape of the outer one.
    let nested = format!(
        "{HEAD}group icon at 120 90 combine subtract {{\n  rect base size 160 110 fill #ff5a36\n  group ring combine subtract {{\n    ellipse outer size 90 90 fill #000000\n    ellipse inner size 50 50 fill #000000\n  }}\n}}\n"
    );
    let drawn = format!(
        "{HEAD}rect base at 120 90 size 160 110 fill #ff5a36\nellipse outer at 120 90 size 90 90 fill #f4efe6\nellipse inner at 120 90 size 50 50 fill #ff5a36\n"
    );
    exact_picture(&drawn, &nested, "a ring cut out");
}

#[test]
fn hidden_and_empty_cases_draw_what_is_left() {
    // A hidden cutter takes no part.
    let hidden = live("at 120 90", "subtract").replace("fill #6f93ff rot 20", "fill #6f93ff rot 20 hidden").replace("scale 1.2 0.8", "scale 1.2 0.8 hidden");
    let alone = format!("{HEAD}group icon at 120 90 {{\n  {}\n}}\n", SHAPES[0]);
    exact_picture(&alone, &hidden, "hidden cutters");
    // Nothing overlaps: an intersection draws nothing at all, not even a stroke.
    let apart = format!(
        "{HEAD}group icon at 120 90 combine intersect {{\n  rect a at -60 0 size 40 40 fill #ff5a36 stroke #1b1d24 3\n  rect b at 60 0 size 40 40 fill #000000\n}}\n"
    );
    exact_picture(HEAD, &apart, "nothing in common");
    // A hidden group draws nothing and takes nothing.
    let off = live("at 120 90 hidden", "union");
    exact_picture(HEAD, &off, "a hidden operation");
}
