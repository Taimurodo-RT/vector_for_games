//! Scenes drawn and compared pixel by pixel, for the tests that promise "the picture stays".
#![allow(dead_code)]

use motif_render::{CpuOptions, render_cpu};
use motif_script::parse;

pub const HEAD: &str = "artboard \"Команды\" 240x180 bg #f4efe6\n";

pub fn draw(src: &str) -> tiny_skia::Pixmap {
    let scene = parse(src).unwrap_or_else(|e| panic!("{e:?}\n{src}"));
    // A scene with a clip is drawn in the middle of it.
    let doc = match scene.doc.animations.first() {
        Some(clip) => clip.pose(&scene.doc, clip.duration / 2.0),
        None => scene.doc.clone(),
    };
    render_cpu(&doc, CpuOptions { scale: 2.0, ..Default::default() }).expect("a picture")
}

/// How many pixels differ by more than a faint shade, and how many by half the range or more.
pub fn difference(a: &tiny_skia::Pixmap, b: &tiny_skia::Pixmap) -> (usize, usize) {
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
pub fn same_picture(before: &str, after: &str, what: &str) {
    let (a, b) = (draw(before), draw(after));
    let (soft, hard) = difference(&a, &b);
    let edge = (a.width() * a.height()) as usize / 200;
    assert!(hard <= 8 && soft < edge, "{what}: {soft} pixels differ in shade, {hard} of them fully\n{after}");
}

/// The same picture pixel for pixel: the outline is written to a hundredth of a unit, which
/// may tip the smoothing of a pixel or two at a sharp corner, and nothing more.
pub fn exact_picture(before: &str, after: &str, what: &str) {
    let (soft, hard) = difference(&draw(before), &draw(after));
    assert!(hard == 0 && soft <= 4, "{what}: {soft} pixels differ in shade, {hard} of them fully\n{after}");
}
