//! The outline commands must leave the picture as it was: a shape turned into a path, and a
//! stroke turned into a filled outline, are drawn before and after and compared pixel by pixel.

use motif_render::{CpuOptions, render_cpu};
use motif_script::{offset_path, parse, shape_to_path, stroke_to_path};

const HEAD: &str = "artboard \"Команды\" 240x180 bg #f4efe6\n";

fn draw(src: &str) -> tiny_skia::Pixmap {
    let scene = parse(src).unwrap_or_else(|e| panic!("{e:?}\n{src}"));
    // A scene with a clip is drawn in the middle of it.
    let doc = match scene.doc.animations.first() {
        Some(clip) => clip.pose(&scene.doc, clip.duration / 2.0),
        None => scene.doc.clone(),
    };
    render_cpu(&doc, CpuOptions { scale: 2.0, ..Default::default() }).expect("a picture")
}

/// How many pixels differ by more than a faint shade, and how many by half the range or more.
fn difference(a: &tiny_skia::Pixmap, b: &tiny_skia::Pixmap) -> (usize, usize) {
    assert_eq!((a.width(), a.height()), (b.width(), b.height()));
    let (mut soft, mut hard) = (0, 0);
    for (p, q) in a.data().chunks(4).zip(b.data().chunks(4)) {
        let d = p.iter().zip(q).map(|(x, y)| x.abs_diff(*y)).max().unwrap_or(0);
        soft += usize::from(d > 24);
        hard += usize::from(d >= 128);
    }
    (soft, hard)
}

/// The same picture to the eye. The export renderer lays a curved stroke and the ends of dashes
/// on a curve a fraction of a pixel off, while the outline is exact (see the ring test below),
/// so pixels on an edge may differ in shade. A wrong cap, corner, dash or place would change
/// whole pixels by the hundred.
fn same_picture(before: &str, after: &str, what: &str) {
    let (a, b) = (draw(before), draw(after));
    let (soft, hard) = difference(&a, &b);
    let edge = (a.width() * a.height()) as usize / 200;
    assert!(hard <= 8 && soft < edge, "{what}: {soft} pixels differ in shade, {hard} of them fully\n{after}");
}

/// The same picture pixel for pixel: the outline is written to a hundredth of a unit, which
/// may tip the smoothing of a pixel or two at a sharp corner, and nothing more.
fn exact_picture(before: &str, after: &str, what: &str) {
    let (soft, hard) = difference(&draw(before), &draw(after));
    assert!(hard == 0 && soft <= 4, "{what}: {soft} pixels differ in shade, {hard} of them fully\n{after}");
}

#[test]
fn a_shape_turned_into_a_path_is_drawn_the_same() {
    for line in [
        "rect box at 120 90 size 120 70 fill #ff5a36 stroke #1b1d24 4",
        "rect box at 120 90 size 120 70 r 18 fill #ff5a36 stroke #1b1d24 4 rot 20",
        "rect box at 120 90 size 120 70 r 30 4 0 12 fill #ff5a36 scale 1.2 0.8 skew 12",
        "ellipse box at 120 90 size 130 80 gradient linear 90 #ff5a36 #ffd34d stroke #1b1d24 3 dash 8 5",
        "star box at 120 90 size 130 130 points 7 inner 0.45 fill #6f93ff stroke #1b1d24 3 join round rot -12",
        "polygon box at 120 90 size 120 120 points 6 fill #7bd389 pivot 20 -10 rot 33 opacity 0.6",
    ] {
        let before = format!("{HEAD}{line}\n");
        let scene = parse(&before).unwrap();
        let after = shape_to_path(&before, &scene, "box").unwrap_or_else(|e| panic!("{e}\n{line}"));
        assert!(after.contains("path box "), "{after}");
        // The same outline goes through the same renderer: nothing may move.
        exact_picture(&before, &after, line);
    }
}

#[test]
fn a_stroke_turned_into_an_outline_is_drawn_the_same() {
    for line in [
        // No fill: the shape itself becomes the outline of its stroke.
        "rect box at 120 90 size 120 70 fill none stroke #1b1d24 10",
        "ellipse box at 120 90 size 130 80 fill none stroke #1b1d24 8 dash 14 9 cap round",
        "path box at 120 90 d \"M -80 30 C -40 -70 30 60 80 -30\" fill none stroke #c2410c 12 cap square",
        "path box at 120 90 d \"M -80 30 L -20 -40 L 30 40 L 80 -30\" fill none stroke #c2410c 12 join bevel cap round rot 8",
        "path box at 120 90 d \"M -80 30 L -20 -40 L 30 40 L 80 -30\" fill none stroke #c2410c 9 dash 20 6 4 6 cap butt",
        "star box at 120 90 size 130 130 points 5 inner 0.4 fill none stroke-gradient linear 0 #ff5a36 #6f93ff stroke #000 7 join round",
        // A fill stays where it was and the outline of the stroke lies on top of it.
        "rect box at 120 90 size 120 70 r 14 fill #ffd34d stroke #1b1d24 8 rot -15",
        "ellipse box at 120 90 size 130 80 gradient radial #ffffff #6f93ff stroke #1b1d24 6 dash 10 6 scale 1.1",
    ] {
        let before = format!("{HEAD}{line}\n");
        let scene = parse(&before).unwrap();
        let (after, id, note) = stroke_to_path(&before, &scene, "box").unwrap_or_else(|e| panic!("{e}\n{line}"));
        assert_eq!(note, None);
        let made = parse(&after).unwrap_or_else(|e| panic!("{e:?}\n{after}"));
        let outline = made.doc.node(&id).expect("the outline");
        assert!(outline.stroke.is_none() && outline.stroke_gradient.is_none(), "the outline has no stroke of its own\n{after}");
        same_picture(&before, &after, line);
    }
}

#[test]
fn an_offset_outline_stands_where_a_stroke_of_that_width_would_reach() {
    // Outwards by 10 with round corners covers what a round stroke of 20 covers.
    let shape = "rect box at 120 90 size 100 60 fill #1b1d24";
    let before = format!("{HEAD}{shape}\n");
    let scene = parse(&before).unwrap();
    let (after, id, _) = offset_path(&before, &scene, "box", 10.0, false).unwrap();
    assert_eq!(id, "box_offset");
    let stroked = format!("{HEAD}{shape} stroke #1b1d24 20 join round\n");
    same_picture(&stroked, &after, "outwards");
    // Inwards by 10 leaves the shape smaller by 10 on every side.
    let (after, _, _) = offset_path(&before, &scene, "box", -10.0, true).unwrap();
    let only = after.replace(&format!("{shape}\n"), "");
    let smaller = format!("{HEAD}rect box at 120 90 size 80 40 fill #1b1d24\n");
    exact_picture(&smaller, &only, "inwards");
}

#[test]
fn the_outline_of_a_round_stroke_is_exact() {
    // A circle of 80 with a stroke of 8 covers the ring between circles of 88 and 72.
    let before = format!("{HEAD}ellipse box at 120 90 size 80 80 fill none stroke #1b1d24 8\n");
    let ring = format!("{HEAD}ellipse outer at 120 90 size 88 88 fill #1b1d24\nellipse inner at 120 90 size 72 72 fill #f4efe6\n");
    let scene = parse(&before).unwrap();
    let (after, id, _) = stroke_to_path(&before, &scene, "box").unwrap();
    assert_eq!(id, "box");
    exact_picture(&ring, &after, "ring");
}

#[test]
fn a_shape_turned_into_a_path_still_bends_and_morphs_the_same() {
    for body in [
        // Bones bend the outline itself, whatever kind of object holds it.
        "bone arm at 60 90 len 60 {\n  bone hand at 60 0 len 60\n}\nrect box at 120 90 size 140 30 r 8 fill #ff5a36 stroke #1b1d24 3 skin arm hand\nanim Bend 1s {\n  hand.rot 0: 0 | 1: 80\n}\n",
        // A morph blends the outline with the outlines of other objects.
        "star spike at 120 90 size 120 120 points 5 inner 0.4 fill #000000 hidden\nellipse box at 120 90 size 100 100 fill #6f93ff stroke #1b1d24 3 morph spike\nanim Turn 1s {\n  box.morph 0: 0 | 1: 1\n}\n",
        // Keys of place, turn and scale stay on the object.
        "rect box at 120 90 size 100 60 fill #7bd389 stroke #1b1d24 3\nanim Spin 1s {\n  box.rot 0: 0 | 1: 90\n  box.sx 0: 1 | 1: 1.5\n  box.x 0: 100 | 1: 140\n}\n",
    ] {
        let before = format!("{HEAD}{body}");
        let scene = parse(&before).unwrap_or_else(|e| panic!("{e:?}\n{before}"));
        let after = shape_to_path(&before, &scene, "box").unwrap_or_else(|e| panic!("{e}\n{body}"));
        exact_picture(&before, &after, body);
        // The middle of the clip is not the picture at rest: the test does compare a moved shape.
        let rest = before[..before.find("anim ").unwrap()].to_string();
        let (soft, _) = difference(&draw(&before), &draw(&rest));
        assert!(soft > 200, "the clip changes the picture: {soft}\n{body}");
    }
}
