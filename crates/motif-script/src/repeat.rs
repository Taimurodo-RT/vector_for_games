//! A live repeat: a group that shows what is inside it several times.
//!
//! `group rivets at 30 30 repeat grid 6 1 step 28 0 { … }` draws its objects, then the same
//! objects again in every other cell. The objects written in the block are the first of the
//! repeats and stay ordinary lines of the script: what is changed in them changes in every
//! repeat. The others are made when the script is read, as the objects of a loop are: a group
//! `rivets-2-1` for the second column of the first row, with `rivets-2-1-dot` inside.
//!
//! The kinds: `grid COLS ROWS` with `step DX DY`; `circle COUNT` around the pivot of the group
//! (its own point until `pivot` moves it), with `arc DEGREES` for a part of the circle;
//! `along ID COUNT` evenly along the outline of another object, with `offset D` inwards.
//! `upright` keeps the repeats of a circle or of an outline unturned.

use motif_core::kurbo::{BezPath, PathEl, Point, Vec2};

use crate::{Scene, Span};

/// The most repeats one group makes.
pub const MAX: u32 = 2000;

/// How a group repeats what is inside it.
#[derive(Clone, Debug, PartialEq)]
pub enum RepeatKind {
    /// Columns to the right and rows down, `step` apart.
    Grid { cols: u32, rows: u32, step: (f64, f64) },
    /// Around the pivot of the group, over `arc` degrees (360 is the whole circle).
    Circle { count: u32, arc: f64, upright: bool },
    /// Evenly along the outline of the object `path`, `offset` inwards from it.
    Along { path: String, count: u32, offset: f64, upright: bool },
}

/// A group with `repeat` on its line.
#[derive(Clone, Debug, PartialEq)]
pub struct RepeatSite {
    pub kind: RepeatKind,
    /// The groups made for every repeat but the first, in the order they are drawn.
    pub clones: Vec<String>,
    /// The line is written by hand with plain numbers: the panel and the handles can rewrite it.
    pub editable: bool,
}

impl RepeatKind {
    /// How many times the content is shown, the written one included.
    pub fn count(&self) -> u32 {
        match *self {
            RepeatKind::Grid { cols, rows, .. } => cols * rows,
            RepeatKind::Circle { count, .. } | RepeatKind::Along { count, .. } => count,
        }
    }

    /// What follows the id of the group in the id of each repeat but the first: `2-1` (column,
    /// row) in a grid, the number of the repeat otherwise. A cell keeps its id when columns or
    /// rows are added.
    pub fn suffixes(&self) -> Vec<String> {
        match *self {
            RepeatKind::Grid { cols, rows, .. } => {
                (1..=rows).flat_map(|r| (1..=cols).map(move |c| (c, r))).filter(|&cell| cell != (1, 1)).map(|(c, r)| format!("{c}-{r}")).collect()
            }
            RepeatKind::Circle { count, .. } | RepeatKind::Along { count, .. } => (2..=count).map(|k| k.to_string()).collect(),
        }
    }

    /// The words of the group's line that say this.
    pub fn words(&self) -> String {
        let n = crate::fmt_num;
        match self {
            RepeatKind::Grid { cols, rows, step } => format!("repeat grid {cols} {rows} step {} {}", n(step.0), n(step.1)),
            RepeatKind::Circle { count, arc, upright } => {
                let arc = if *arc == 360.0 { String::new() } else { format!(" arc {}", n(*arc)) };
                format!("repeat circle {count}{arc}{}", if *upright { " upright" } else { "" })
            }
            RepeatKind::Along { path, count, offset, upright } => {
                let offset = if *offset == 0.0 { String::new() } else { format!(" offset {}", n(*offset)) };
                format!("repeat along {path} {count}{offset}{}", if *upright { " upright" } else { "" })
            }
        }
    }

    /// The angle of every repeat of a circle, the first one at 0.
    pub fn angles(count: u32, arc: f64) -> Vec<f64> {
        let whole = arc.abs() >= 360.0;
        let step = match (whole, count) {
            (_, 0 | 1) => 0.0,
            (true, n) => arc.signum() * 360.0 / f64::from(n),
            (false, n) => arc / f64::from(n - 1),
        };
        (0..count).map(|k| f64::from(k) * step).collect()
    }

    /// Where each repeat but the first stands inside the group: x, y and the turn in degrees.
    /// A repeat of a circle turns around `pivot`, the pivot of the group. `anchor` is the middle
    /// of the written content: an upright repeat carries it around the pivot without turning.
    /// The repeats along an outline are placed when the outline is known ([`along`]).
    pub fn places(&self, anchor: Vec2, pivot: Vec2) -> Vec<(f64, f64, f64)> {
        match *self {
            RepeatKind::Grid { cols, rows, step } => {
                (0..rows).flat_map(|r| (0..cols).map(move |c| (c, r))).skip(1).map(|(c, r)| (f64::from(c) * step.0, f64::from(r) * step.1, 0.0)).collect()
            }
            RepeatKind::Circle { count, arc, upright } => Self::angles(count, arc)
                .into_iter()
                .skip(1)
                .map(|deg| {
                    if upright {
                        let (sin, cos) = deg.to_radians().sin_cos();
                        let arm = anchor - pivot;
                        let turned = pivot + Vec2::new(arm.x * cos - arm.y * sin, arm.x * sin + arm.y * cos);
                        (turned.x - anchor.x, turned.y - anchor.y, 0.0)
                    } else {
                        (0.0, 0.0, deg)
                    }
                })
                .collect(),
            RepeatKind::Along { count, .. } => vec![(0.0, 0.0, 0.0); count.saturating_sub(1) as usize],
        }
    }
}

/// Where two straight pieces cross, when they do.
fn crossing(a: Point, b: Point, c: Point, d: Point) -> Option<Point> {
    let (r, s) = (b - a, d - c);
    let den = r.cross(s);
    if den.abs() < 1e-12 {
        return None;
    }
    let (t, u) = ((c - a).cross(s) / den, (c - a).cross(r) / den);
    let within = |v: f64| (-1e-9..=1.0 + 1e-9).contains(&v);
    (within(t) && within(u)).then(|| a + r * t)
}

/// `count` points evenly along the first outline of `path`, moved `offset` inwards (to the left
/// of an open line), each with the direction of the outline there in degrees. A closed outline
/// is divided into `count` equal parts; an open one has a point at each end.
pub fn along(path: &BezPath, count: u32, offset: f64) -> Vec<(Point, f64)> {
    // The first outline, as a chain of straight pieces fine enough for placing. What follows a
    // second `M` is left alone.
    let end = path.elements().iter().skip(1).position(|el| matches!(el, PathEl::MoveTo(_))).map_or(path.elements().len(), |at| at + 1);
    let mut drawn: Vec<Point> = Vec::new();
    let mut closed = false;
    motif_core::kurbo::flatten(path.elements()[..end].iter().copied(), 0.005, |el| match el {
        PathEl::MoveTo(p) | PathEl::LineTo(p) => {
            if drawn.last().is_none_or(|last| last.distance(p) > 1e-9) {
                drawn.push(p);
            }
        }
        PathEl::ClosePath => closed = true,
        _ => {}
    });
    if closed && drawn.len() > 1 && drawn[0].distance(drawn[drawn.len() - 1]) < 1e-9 {
        drawn.pop();
    }
    if count == 0 || drawn.is_empty() {
        return Vec::new();
    }
    if drawn.len() == 1 {
        return vec![(drawn[0], 0.0); count as usize];
    }
    let n = drawn.len();
    let pieces = if closed { n } else { n - 1 };
    // Piece `i` goes from point `i` to the next one.
    let dir = |i: usize| (drawn[(i + 1) % n] - drawn[i]).normalize();
    let before = |i: usize| {
        if i > 0 {
            Some(i - 1)
        } else if closed {
            Some(pieces - 1)
        } else {
            None
        }
    };
    let after = |i: usize| {
        if i + 1 < pieces {
            Some(i + 1)
        } else if closed {
            Some(0)
        } else {
            None
        }
    };
    // Two pieces of one curve, not the two sides of a corner.
    let smooth = |a: usize, b: usize| dir(a).dot(dir(b)) > 0.9;
    // The direction of a curve where two of its pieces meet: the longer one says more.
    let through = |a: usize, b: usize| (drawn[(b + 1) % n] - drawn[a]).normalize();

    let mut points = drawn.clone();
    if offset != 0.0 {
        // Inwards: the side the outline goes around.
        let area: f64 = (0..n).map(|i| drawn[i].to_vec2().cross(drawn[(i + 1) % n].to_vec2())).sum();
        let side = if closed && area < 0.0 { -1.0 } else { 1.0 };
        let normal = |d: Vec2| Vec2::new(-d.y, d.x) * side;
        for i in 0..n {
            let shift = match (before(i), (i < pieces).then_some(i)) {
                (Some(a), Some(b)) if smooth(a, b) => normal(through(a, b)),
                // A corner: the point where the two moved sides meet, not further than four offsets away.
                (Some(a), Some(b)) => {
                    let (a, b) = (normal(dir(a)), normal(dir(b)));
                    let miter = (a + b) / (1.0 + a.dot(b)).max(0.125);
                    if miter.hypot() > 4.0 { miter.normalize() * 4.0 } else { miter }
                }
                (Some(p), None) | (None, Some(p)) => normal(dir(p)),
                (None, None) => Vec2::ZERO,
            };
            points[i] = drawn[i] + shift * offset;
        }
        // Moved inwards past the radius of a round corner, the pieces of the corner turn back
        // and the outline crosses itself there. The loop is cut off: the outline goes straight
        // from one side of the crossing to the other, and the corner becomes a sharp one.
        let turned = |points: &[Point], i: usize| (points[(i + 1) % n] - points[i]).dot(dir(i)) < -1e-12;
        let mut seen = vec![false; pieces];
        for start in 0..pieces {
            if seen[start] || !turned(&points, start) {
                continue;
            }
            let (mut first, mut last) = (start, start);
            for _ in 0..pieces {
                match before(first) {
                    Some(p) if p != last && turned(&points, p) => first = p,
                    _ => break,
                }
            }
            for _ in 0..pieces {
                match after(last) {
                    Some(q) if q != first && turned(&points, q) => last = q,
                    _ => break,
                }
            }
            let mut k = first;
            loop {
                seen[k] = true;
                if k == last {
                    break;
                }
                k = (k + 1) % pieces;
            }
            // The nearest pieces on the two sides of the loop that cross.
            let step = |from: usize, by: usize, back: bool| (0..by).try_fold(from, |at, _| if back { before(at) } else { after(at) });
            'search: for total in 0..96usize {
                for back in 0..=total {
                    let (Some(a), Some(b)) = (step(first, back + 1, true), step(last, total - back + 1, false)) else { continue };
                    if a == b {
                        break 'search;
                    }
                    if let Some(x) = crossing(points[a], points[(a + 1) % n], points[b], points[(b + 1) % n]) {
                        let mut k = (a + 1) % n;
                        loop {
                            points[k] = x;
                            if k == b {
                                break;
                            }
                            k = (k + 1) % n;
                        }
                        break 'search;
                    }
                }
            }
        }
    }
    // A piece that is still turned back, or closed up into a point (to a fiftieth of what it
    // was: a round corner moved in by its own radius), takes no room.
    let lengths: Vec<f64> = (0..pieces)
        .map(|i| {
            let moved = points[(i + 1) % n] - points[i];
            let was = drawn[i].distance(drawn[(i + 1) % n]);
            if moved.dot(dir(i)) > 0.0 && moved.hypot() > 1e-9 && moved.hypot() > was * 0.02 { moved.hypot() } else { 0.0 }
        })
        .collect();
    let total: f64 = lengths.iter().sum();
    let parts = if closed { count } else { count.saturating_sub(1).max(1) };
    let mut out = Vec::with_capacity(count as usize);
    let (mut piece, mut passed) = (0usize, 0.0f64);
    for k in 0..count {
        let target = if count == 1 { 0.0 } else { total * f64::from(k) / f64::from(parts) };
        while piece + 1 < pieces && passed + lengths[piece] < target - 1e-9 {
            passed += lengths[piece];
            piece += 1;
        }
        let (a, b) = (points[piece], points[(piece + 1) % n]);
        let t = if lengths[piece] > 0.0 { ((target - passed) / lengths[piece]).clamp(0.0, 1.0) } else { 0.0 };
        // Within a hundredth of the end of a piece is at its end.
        let (at_start, at_end) = (t * lengths[piece] < 0.01, (1.0 - t) * lengths[piece] < 0.01);
        // The direction of the outline there. Along a curve it turns smoothly from one straight
        // piece to the next. On a side it is the direction of the side, and exactly at a corner
        // the direction between the two sides.
        let own = dir(piece);
        let joint = |a: Option<usize>, b: Option<usize>, exact: bool| match (a, b) {
            (Some(a), Some(b)) if smooth(a, b) => through(a, b),
            (Some(a), Some(b)) if exact && (dir(a) + dir(b)).hypot() > 1e-6 => (dir(a) + dir(b)).normalize(),
            _ => own,
        };
        let mut way = joint(before(piece), Some(piece), at_start).lerp(joint(Some(piece), after(piece), at_end), t);
        // A round corner that the move inwards closed up into a point is a corner too: a repeat
        // on it stands between its sides.
        let (mut first, mut last) = (piece, piece);
        let mut folded = lengths[piece] == 0.0;
        for _ in 0..pieces {
            match before(first).filter(|_| at_start) {
                Some(p) if lengths[p] == 0.0 && p != last => (first, folded) = (p, true),
                _ => break,
            }
        }
        for _ in 0..pieces {
            match after(last).filter(|_| at_end) {
                Some(q) if lengths[q] == 0.0 && q != first => (last, folded) = (q, true),
                _ => break,
            }
        }
        if folded {
            // From the side the corner starts on to the side it ends on.
            let from = if lengths[first] == 0.0 { before(first).unwrap_or(first) } else { first };
            let to = if lengths[last] == 0.0 { after(last).unwrap_or(last) } else { last };
            if (dir(from) + dir(to)).hypot() > 1e-6 {
                way = dir(from) + dir(to);
            }
        }
        out.push((a.lerp(b, t), way.y.atan2(way.x).to_degrees()));
    }
    out
}

/// The properties a repeat along an outline takes from the outline: they are not written on its line.
pub const PLACED: [&str; 5] = ["at", "rot", "scale", "skew", "pivot"];

/// Rewrites how the group `id` repeats its content; `None` makes it an ordinary group again.
/// What an animation, `override` or a link says about a repeat that is no longer made goes
/// with it. Returns `None` when the group is not written by hand, when its repeat is written
/// with names or formulas (the script is the place to change those), or when nothing would change.
pub fn set_repeat(src: &str, scene: &Scene, id: &str, kind: Option<&RepeatKind>) -> Option<String> {
    rewrite(src, scene, id, kind, None)
}

/// [`set_repeat`] for a group that starts to repeat around `pivot`, a point in its own space:
/// the pivot is written with the repeat, unless the line of the group already has one.
pub fn set_repeat_around(src: &str, scene: &Scene, id: &str, kind: &RepeatKind, pivot: Vec2) -> Option<String> {
    rewrite(src, scene, id, Some(kind), Some(pivot))
}

fn rewrite(src: &str, scene: &Scene, id: &str, kind: Option<&RepeatKind>, pivot: Option<Vec2>) -> Option<String> {
    let site = scene.sites.get(id).filter(|s| !s.generated)?;
    let now = scene.repeats.get(id);
    if now.map(|r| &r.kind) == kind || (kind.is_some() && now.is_some_and(|r| !r.editable)) {
        return None;
    }
    // The groups that are made now and will not be: everything said about them is removed.
    let stays: Vec<String> = kind.map(|k| k.suffixes().into_iter().map(|s| format!("{id}-{s}")).collect()).unwrap_or_default();
    let gone: Vec<String> = now.map(|r| r.clones.iter().filter(|c| !stays.contains(c)).cloned().collect()).unwrap_or_default();
    let inside: Vec<&str> = gone
        .iter()
        .filter_map(|c| scene.doc.index_of(c))
        .flat_map(|i| std::iter::once(i).chain((i + 1..scene.doc.nodes.len()).take_while(move |&k| scene.doc.is_inside(k, i))))
        .map(|i| scene.doc.nodes[i].id.as_str())
        .collect();
    let mut said = crate::edit::reference_spans(src, scene, &inside);
    said.sort_by_key(|span| span.start);
    let mut edits: Vec<(Span, String)> = Vec::new();
    for span in said {
        match edits.last_mut() {
            Some((last, _)) if span.start < last.end => last.end = last.end.max(span.end),
            _ => edits.push((span, String::new())),
        }
    }
    // The new words stand where the old ones stood, or at the end of the line.
    let written = scene.prop_spans.get(&(id.to_string(), "repeat".to_string())).copied();
    let pivot = match pivot.filter(|_| !scene.prop_spans.contains_key(&(id.to_string(), "pivot".to_string()))) {
        Some(p) => format!(" pivot {} {}", crate::fmt_num(p.x), crate::fmt_num(p.y)),
        None => String::new(),
    };
    match (written, kind) {
        (Some(span), Some(kind)) => edits.push((span, format!("{}{pivot}", kind.words()))),
        (Some(span), None) => edits.push((crate::edit::with_space_before(src, span), String::new())),
        (None, Some(kind)) => edits.push((Span::new(site.insert_at, site.insert_at), format!("{pivot} {}", kind.words()))),
        (None, None) => return None,
    }
    // A repeat along an outline takes its place from the outline.
    if matches!(kind, Some(RepeatKind::Along { .. })) {
        for word in PLACED {
            if let Some(span) = scene.prop_spans.get(&(id.to_string(), word.to_string())) {
                edits.push((crate::edit::with_space_before(src, *span), String::new()));
            }
        }
    }
    Some(crate::edit::splice_all(src, edits))
}

/// Returns the script with `ids` put into a new group that repeats them, and the id of the
/// group. Everything written stays where it was, except along an outline, which gives the
/// group its place. A circle goes around `centre`, a point measured from the middle of the
/// objects: it becomes the pivot of the group.
pub fn repeat_nodes(
    src: &str,
    scene: &Scene,
    ids: &[String],
    kind: &RepeatKind,
    centre: Vec2,
    base: Option<&std::path::Path>,
) -> Result<(String, String), String> {
    let (grouped, group) = crate::edit::group_nodes(src, scene, ids, base)?;
    let scene = crate::edit::reparse(&grouped, base, scene)?;
    let site = scene.sites.get(&group).ok_or("группа не создалась")?;
    let pivot = match kind {
        RepeatKind::Circle { .. } if centre != Vec2::ZERO => format!(" pivot {} {}", crate::fmt_num(centre.x), crate::fmt_num(centre.y)),
        _ => String::new(),
    };
    let with_pivot = crate::edit::splice_all(&grouped, vec![(Span::new(site.insert_at, site.insert_at), pivot)]);
    let scene = crate::edit::reparse(&with_pivot, base, &scene)?;
    let out = set_repeat(&with_pivot, &scene, &group, Some(kind)).ok_or("повтор не записался")?;
    crate::edit::reparse(&out, base, &scene)?;
    Ok((out, group))
}

/// The objects `ids` and the same objects in every repeat of the groups they are written in:
/// what is said about `dot` is said about `rivets-2-1-dot` too. A repeat is read from the same
/// lines as the written content, so its objects come in the same order.
pub(crate) fn in_every_repeat(scene: &Scene, mut ids: Vec<String>) -> Vec<String> {
    let doc = &scene.doc;
    // A repeat inside a repeat: its objects are repeated by the outer one in turn.
    for _ in 0..4 {
        let before = ids.len();
        for (group, site) in &scene.repeats {
            let Some(g) = doc.index_of(group) else { continue };
            let clones: Vec<usize> = site.clones.iter().filter_map(|c| doc.index_of(c)).collect();
            let in_clone = |i: usize| clones.iter().any(|&c| i == c || doc.is_inside(i, c));
            let written: Vec<usize> = (g + 1..doc.nodes.len()).take_while(|&i| doc.is_inside(i, g)).filter(|&i| !in_clone(i)).collect();
            for &c in &clones {
                let made = (c + 1..doc.nodes.len()).take_while(|&i| doc.is_inside(i, c));
                for (&w, m) in written.iter().zip(made) {
                    if ids.contains(&doc.nodes[w].id) && !ids.contains(&doc.nodes[m].id) {
                        ids.push(doc.nodes[m].id.clone());
                    }
                }
            }
        }
        if ids.len() == before {
            break;
        }
    }
    ids
}

#[cfg(test)]
mod tests {
    use super::*;

    fn near(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-6
    }

    #[test]
    fn a_grid_names_its_cells_by_column_and_row() {
        let grid = RepeatKind::Grid { cols: 3, rows: 2, step: (40.0, 30.0) };
        assert_eq!(grid.count(), 6);
        assert_eq!(grid.suffixes(), ["2-1", "3-1", "1-2", "2-2", "3-2"]);
        assert_eq!(grid.places(Vec2::ZERO, Vec2::ZERO), [(40.0, 0.0, 0.0), (80.0, 0.0, 0.0), (0.0, 30.0, 0.0), (40.0, 30.0, 0.0), (80.0, 30.0, 0.0)]);
        assert_eq!(grid.words(), "repeat grid 3 2 step 40 30");
    }

    #[test]
    fn a_circle_turns_its_repeats_or_carries_them_upright() {
        assert_eq!(RepeatKind::angles(4, 360.0), [0.0, 90.0, 180.0, 270.0]);
        // A part of the circle has a repeat at each end.
        assert_eq!(RepeatKind::angles(3, 90.0), [0.0, 45.0, 90.0]);
        assert_eq!(RepeatKind::angles(1, 90.0), [0.0]);
        let turned = RepeatKind::Circle { count: 4, arc: 360.0, upright: false };
        assert_eq!(turned.suffixes(), ["2", "3", "4"]);
        assert_eq!(turned.places(Vec2::new(0.0, -40.0), Vec2::ZERO), [(0.0, 0.0, 90.0), (0.0, 0.0, 180.0), (0.0, 0.0, 270.0)]);
        assert_eq!(turned.words(), "repeat circle 4");
        // Upright: the middle of the content (0, -40) goes to (40, 0), and nothing turns.
        let upright = RepeatKind::Circle { count: 4, arc: 360.0, upright: true };
        let (x, y, rot) = upright.places(Vec2::new(0.0, -40.0), Vec2::ZERO)[0];
        assert!(near(x, 40.0) && near(y, 40.0) && rot == 0.0, "{x} {y} {rot}");
        // Around a pivot 10 below the content: a quarter turn puts it 10 to the right of the pivot.
        let (x, y, _) = upright.places(Vec2::new(0.0, -40.0), Vec2::new(0.0, -30.0))[0];
        assert!(near(x, 10.0) && near(y, 10.0), "{x} {y}");
        assert_eq!(RepeatKind::Circle { count: 5, arc: 60.0, upright: true }.words(), "repeat circle 5 arc 60 upright");
    }

    #[test]
    fn points_go_evenly_along_an_outline_and_inwards_from_it() {
        // A square 100 by 100, clockwise on the screen from the left top corner.
        let square = BezPath::from_svg("M 0 0 L 100 0 L 100 100 L 0 100 Z").unwrap();
        let on: Vec<(Point, f64)> = along(&square, 8, 0.0);
        let want = [(0.0, 0.0), (50.0, 0.0), (100.0, 0.0), (100.0, 50.0), (100.0, 100.0), (50.0, 100.0), (0.0, 100.0), (0.0, 50.0)];
        for ((p, _), (x, y)) in on.iter().zip(want) {
            assert!(near(p.x, x) && near(p.y, y), "{p:?}, wanted {x} {y}");
        }
        // The direction of the outline: to the right along the top, down along the right side,
        // and between the two at the corner.
        assert!(near(on[1].1, 0.0) && near(on[2].1, 45.0) && near(on[3].1, 90.0), "{on:?}");
        // Ten inwards: a square 80 by 80 with its corners at (10, 10) … The same way round the other way.
        for d in ["M 0 0 L 100 0 L 100 100 L 0 100 Z", "M 0 0 L 0 100 L 100 100 L 100 0 Z"] {
            let inner = along(&BezPath::from_svg(d).unwrap(), 4, 10.0);
            let mut corners: Vec<(i64, i64)> = inner.iter().map(|(p, _)| (p.x.round() as i64, p.y.round() as i64)).collect();
            corners.sort_unstable();
            assert_eq!(corners, [(10, 10), (10, 90), (90, 10), (90, 90)], "{d}");
        }
        // An open line has a point at each end; a second outline is not followed.
        let line = BezPath::from_svg("M 0 0 L 90 0 M 500 500 L 600 600").unwrap();
        let on: Vec<f64> = along(&line, 4, 0.0).iter().map(|(p, _)| p.x).collect();
        assert!(on.iter().zip([0.0, 30.0, 60.0, 90.0]).all(|(a, b)| near(*a, b)), "{on:?}");
        assert!(along(&line, 4, 0.0).iter().all(|(p, _)| near(p.y, 0.0)));
        // One repeat stands at the start.
        assert_eq!(along(&line, 1, 0.0).len(), 1);
    }
    fn scene(src: &str) -> Scene {
        crate::parse(src).unwrap_or_else(|e| panic!("{e:#?}\n{src}"))
    }

    /// Where the object stands on the board and how it is turned, to a tenth.
    fn stands(scene: &Scene, id: &str) -> (f64, f64, f64) {
        let t = scene.doc.world_transform(scene.doc.index_of(id).unwrap_or_else(|| panic!("no {id}")));
        let c = t.as_coeffs();
        let round = |v: f64| (v * 10.0).round() / 10.0 + 0.0;
        (round(c[4]), round(c[5]), round(c[1].atan2(c[0]).to_degrees()))
    }

    const BOARD: &str = "artboard \"a\" 400x300\n";

    #[test]
    fn a_group_shows_what_is_in_it_in_every_cell_of_a_grid() {
        let src = format!("{BOARD}group dots at 100 50 repeat grid 3 2 step 20 30 {{\n  ellipse dot size 8 8\n  rect tick at 0 6 size 2 4\n}}\n");
        let scene = scene(&src);
        let ids: Vec<&str> = scene.doc.nodes.iter().map(|n| n.id.as_str()).collect();
        assert_eq!(&ids[..6], ["dots", "dot", "tick", "dots-2-1", "dots-2-1-dot", "dots-2-1-tick"]);
        assert_eq!(ids.len(), 3 + 5 * 3);
        assert_eq!(stands(&scene, "dot"), (100.0, 50.0, 0.0));
        assert_eq!(stands(&scene, "dots-3-1-dot"), (140.0, 50.0, 0.0));
        assert_eq!(stands(&scene, "dots-1-2-dot"), (100.0, 80.0, 0.0));
        assert_eq!(stands(&scene, "dots-3-2-tick"), (140.0, 86.0, 0.0));
        // The written objects are lines of the script; the others are made, as in a loop.
        assert!(!scene.sites["dot"].generated && !scene.sites["dots"].generated);
        assert!(scene.sites["dots-2-1"].generated && scene.sites["dots-2-1-dot"].generated);
        let site = &scene.repeats["dots"];
        assert_eq!(site.kind, RepeatKind::Grid { cols: 3, rows: 2, step: (20.0, 30.0) });
        assert_eq!(site.clones, ["dots-2-1", "dots-3-1", "dots-1-2", "dots-2-2", "dots-3-2"]);
        assert!(site.editable);
        // One number after `step` is the step both ways; a name or a formula is not for the panel.
        let named = format!("{BOARD}let n = 2\ngroup dots repeat grid n {{n + 1}} step 10 {{\n  ellipse dot size 4 4\n}}\n");
        let scene = crate::parse(&named).unwrap();
        assert_eq!(scene.repeats["dots"].kind, RepeatKind::Grid { cols: 2, rows: 3, step: (10.0, 10.0) });
        assert!(!scene.repeats["dots"].editable);
        // The words of a repeat are words only in it: objects may be called so.
        let words = format!("{BOARD}rect step size 4 4\nrect arc size 4 4\nrect offset size 4 4\nrect upright size 4 4\n");
        assert_eq!(crate::parse(&words).unwrap().doc.nodes.len(), 4);
    }

    #[test]
    fn repeats_go_around_the_point_of_the_group_turned_or_upright() {
        let src = format!("{BOARD}group spokes at 200 150 repeat circle 4 {{\n  rect spoke at 0 -40 size 4 20\n}}\n");
        let scene = scene(&src);
        assert_eq!(stands(&scene, "spoke"), (200.0, 110.0, 0.0));
        assert_eq!(stands(&scene, "spokes-2-spoke"), (240.0, 150.0, 90.0));
        assert_eq!(stands(&scene, "spokes-3-spoke"), (200.0, 190.0, 180.0));
        assert_eq!(stands(&scene, "spokes-4-spoke"), (160.0, 150.0, -90.0));
        // Upright: the same places, nothing turned.
        let scene = crate::parse(&src.replace("circle 4", "circle 4 upright")).unwrap();
        assert_eq!(stands(&scene, "spokes-2-spoke"), (240.0, 150.0, 0.0));
        assert_eq!(stands(&scene, "spokes-4-spoke"), (160.0, 150.0, 0.0));
        // A part of the circle has a repeat at each of its ends.
        let scene = crate::parse(&src.replace("circle 4", "circle 3 arc 180")).unwrap();
        assert_eq!(stands(&scene, "spokes-2-spoke"), (240.0, 150.0, 90.0));
        assert_eq!(stands(&scene, "spokes-3-spoke"), (200.0, 190.0, 180.0));
        assert_eq!(scene.repeats["spokes"].kind.words(), "repeat circle 3 arc 180");
        // The pivot of the group is the centre of the circle, for turned and for upright repeats.
        let off = src.replace("at 200 150 repeat circle 4", "at 200 150 pivot 0 -30 repeat circle 4");
        let scene = crate::parse(&off).unwrap();
        assert_eq!(stands(&scene, "spoke"), (200.0, 110.0, 0.0));
        assert_eq!(stands(&scene, "spokes-2-spoke"), (210.0, 120.0, 90.0));
        assert_eq!(stands(&scene, "spokes-3-spoke"), (200.0, 130.0, 180.0));
        let scene = crate::parse(&off.replace("circle 4", "circle 4 upright")).unwrap();
        assert_eq!(stands(&scene, "spokes-2-spoke"), (210.0, 120.0, 0.0));
    }

    #[test]
    fn chosen_objects_go_into_a_new_group_that_repeats_them() {
        let src = format!(
            "{BOARD}rect panel at 200 150 size 200 100
ellipse rivet at 60 40 size 8 8
anim Blink 1s {{
  rivet.x 0: 60 | 1: 64
}}
"
        );
        let before = scene(&src);
        let ids = ["rivet".to_string()];
        // A row: the rivet stays where it is, the others stand to the right of it.
        let grid = RepeatKind::Grid { cols: 3, rows: 1, step: (10.0, 10.0) };
        let (out, group) = repeat_nodes(&src, &before, &ids, &grid, Vec2::ZERO, None).unwrap();
        assert!(out.contains("group group1 at 60 40 repeat grid 3 1 step 10 10 {"), "{out}");
        let after = scene(&out);
        assert_eq!((group.as_str(), stands(&after, "rivet"), stands(&after, "group1-3-1-rivet")), ("group1", (60.0, 40.0, 0.0), (80.0, 40.0, 0.0)));
        // A ring around a point 20 below the rivet: that point is the pivot of the group.
        let circle = RepeatKind::Circle { count: 4, arc: 360.0, upright: false };
        let (out, _) = repeat_nodes(&src, &before, &ids, &circle, Vec2::new(0.0, 20.0), None).unwrap();
        assert!(out.contains("group group1 at 60 40 pivot 0 20 repeat circle 4 {"), "{out}");
        let after = scene(&out);
        assert_eq!((stands(&after, "rivet"), stands(&after, "group1-3-rivet")), ((60.0, 40.0, 0.0), (60.0, 80.0, 180.0)));
        // Along the panel: the group takes its place from the outline, so it has no `at`.
        let along = RepeatKind::Along { path: "panel".into(), count: 4, offset: 0.0, upright: true };
        let (out, _) = repeat_nodes(&src, &before, &ids, &along, Vec2::ZERO, None).unwrap();
        assert!(out.contains("group group1 repeat along panel 4 upright {"), "{out}");
        let after = scene(&out);
        assert_eq!((stands(&after, "rivet"), stands(&after, "group1-2-rivet")), ((100.0, 100.0, 0.0), (250.0, 100.0, 0.0)));
    }

    #[test]
    fn repeats_follow_the_outline_of_another_object_and_move_with_it() {
        let src = format!("{BOARD}group rivets repeat along frame 8 offset 10 {{\n  rect rivet size 6 2\n}}\nrect frame at 200 150 size 120 120\n");
        let scene = scene(&src);
        // The inner square 100 by 100 from (150, 100): a rivet at every corner and between them,
        // each turned the way the outline goes there, and at a corner between its two sides.
        let want = [(150.0, 100.0, -45.0), (200.0, 100.0, 0.0), (250.0, 100.0, 45.0), (250.0, 150.0, 90.0), (250.0, 200.0, 135.0), (200.0, 200.0, 180.0)];
        let ids = ["rivet", "rivets-2-rivet", "rivets-3-rivet", "rivets-4-rivet", "rivets-5-rivet", "rivets-6-rivet"];
        for (id, want) in ids.iter().zip(want) {
            assert_eq!(stands(&scene, id), want, "{id}");
        }
        assert_eq!(scene.repeats["rivets"].clones.len(), 7);
        // The panel grows: the rivets stand along its new edge.
        let wide = crate::parse(&src.replace("size 120 120", "size 220 120")).unwrap();
        assert_eq!(stands(&wide, "rivet"), (100.0, 100.0, -45.0));
        assert_eq!(stands(&wide, "rivets-2-rivet"), (175.0, 100.0, 0.0));
        // Upright rivets are not turned.
        let upright = crate::parse(&src.replace("offset 10", "offset 10 upright")).unwrap();
        assert_eq!(stands(&upright, "rivets-3-rivet"), (250.0, 100.0, 0.0));
        // The outline is seen where it stands on the board, whatever groups it and the repeat are in.
        let nested = format!(
            "{BOARD}group panel at 200 150 rot 90 {{\n  rect frame size 120 120\n}}\ngroup holder at 30 40 {{\n  group rivets repeat along frame 4 {{\n    rect rivet size 6 2\n  }}\n}}\n"
        );
        let nested = crate::parse(&nested).unwrap();
        assert_eq!(stands(&nested, "rivet"), (260.0, 90.0, 45.0));
        assert_eq!(stands(&nested, "rivets-2-rivet"), (260.0, 210.0, 135.0));
        // What `override` put by hand stays where it was put.
        let moved = format!("{src}override {{\n  rivets-2.x = 7\n}}\n");
        let moved = crate::parse(&moved).unwrap();
        assert_eq!(moved.doc.node("rivets-2").unwrap().x, 7.0);
        assert_eq!(stands(&moved, "rivets-3-rivet"), (250.0, 100.0, 45.0));
    }

    #[test]
    fn a_round_corner_does_not_turn_the_repeats_back() {
        // Moved inwards further than the corner is round, the outline of the corner folds over
        // itself. The repeats still go one way round and stand as the sides go.
        let src = format!("{BOARD}rect frame at 200 150 size 300 180 r 12\ngroup rivets repeat along frame 14 offset 14 {{\n  rect rivet size 6 2\n}}\n");
        let scene = scene(&src);
        for (id, turn) in [("rivets-3", 0.0), ("rivets-5", 0.0), ("rivets-7", 90.0), ("rivets-10", 180.0), ("rivets-14", -90.0)] {
            let (x, y, rot) = stands(&scene, id);
            assert_eq!(rot, turn, "{id} at {x} {y}");
            let on_edge = (x - 64.0).abs() < 0.2 || (x - 336.0).abs() < 0.2 || (y - 74.0).abs() < 0.2 || (y - 226.0).abs() < 0.2;
            assert!(on_edge, "{id} at {x} {y}");
        }
        // The first one stands where the outline starts, in the left top corner, which the move
        // inwards made sharp: between the left side and the top one.
        let (x, y, rot) = stands(&scene, "rivets");
        assert!((x - 64.0).abs() < 0.2 && (y - 74.0).abs() < 0.2 && (rot + 45.0).abs() < 0.2, "{x} {y} {rot}");
        // Moved in by exactly the radius of the corners, the outline is a plain rectangle:
        // every corner has a repeat turned between its sides.
        let exact = format!("{BOARD}rect frame at 200 150 size 208 148 r 14\ngroup rivets repeat along frame 10 offset 14 {{\n  rect rivet size 6 2\n}}\n");
        let scene = crate::parse(&exact).unwrap();
        let corners =
            [("rivets", (110.0, 90.0, -45.0)), ("rivets-4", (290.0, 90.0, 45.0)), ("rivets-6", (290.0, 210.0, 135.0)), ("rivets-9", (110.0, 210.0, -135.0))];
        for (id, want) in corners {
            assert_eq!(stands(&scene, id), want, "{id}");
        }
        assert_eq!(stands(&scene, "rivets-2"), (170.0, 90.0, 0.0));
    }

    #[test]
    fn a_repeat_works_inside_a_symbol_and_inside_another_repeat() {
        let src = format!(
            "{BOARD}symbol Row n=3 {{\n  group cells repeat grid 2 1 step 10 0 {{\n    rect cell size 4 4\n  }}\n}}\nuse Row a at 10 10\nuse Row b at 10 30\ngroup outer repeat grid 1 2 step 0 50 {{\n  group inner repeat circle 2 {{\n    rect leaf at 0 -10 size 2 2\n  }}\n}}\n"
        );
        let scene = scene(&src);
        for id in [
            "a-cells",
            "a-cell",
            "a-cells-2-1",
            "a-cells-2-1-cell",
            "b-cells-2-1-cell",
            "inner-2-leaf",
            "outer-1-2-inner",
            "outer-1-2-leaf",
            "outer-1-2-inner-2-leaf",
        ] {
            assert!(scene.doc.index_of(id).is_some(), "{id}");
        }
        assert_eq!(stands(&scene, "b-cells-2-1-cell"), (20.0, 30.0, 0.0));
        assert_eq!(stands(&scene, "outer-1-2-inner-2-leaf"), (0.0, 60.0, 180.0));
        assert_eq!(scene.repeats["outer-1-2-inner"].clones, ["outer-1-2-inner-2"]);
    }

    #[test]
    fn a_wrong_repeat_says_what_is_wrong() {
        let group =
            |line: &str| format!("{BOARD}rect frame at 100 100 size 50 50\ngroup inside {{\n  rect own size 4 4\n}}\n{line} {{\n  rect own2 size 4 4\n}}\n");
        for (src, want) in [
            (group("group g repeat"), "после repeat нужен вид повтора"),
            (group("group g repeat row 3"), "после repeat нужен вид повтора"),
            (group("group g repeat grid 3"), "сетке нужны число столбцов и число строк"),
            (group("group g repeat grid 3 2"), "сетке нужен шаг"),
            (group("group g repeat grid 3 2 step"), "после step нужен шаг сетки"),
            (group("group g repeat grid 0 2 step 4"), "число повторов — целое от 1 до 2000"),
            (group("group g repeat grid 2.5 2 step 4"), "число повторов — целое от 1 до 2000"),
            (group("group g repeat grid 100 100 step 4"), "выходит 10000 повторов"),
            (group("group g repeat grid 2 2 step 4 upright"), "в сетке они и так не поворачиваются"),
            (group("group g repeat circle 4 step 3"), "step — шаг сетки"),
            (group("group g repeat circle 4 arc 0"), "от -360 до 360 и не 0"),
            (group("group g repeat circle 4 offset 3"), "offset — отступ от контура"),
            (group("group g repeat along frame 4 arc 90"), "arc — часть круга"),
            (group("group g repeat along 4"), "после repeat along нужен id контура"),
            (group("group g repeat along nothing 4"), "нет объекта «nothing»"),
            (group("group g repeat along inside 4"), "у «inside» нет контура"),
            (group("group g repeat along own2 4"), "лежит в самом повторе"),
            (group("group g at 10 10 repeat along frame 4"), "задаёт сам контур: уберите «at»"),
            (group("group g clip repeat circle 4"), "repeat не бывает вместе с clip"),
            (group("group g stack row repeat circle 4"), "repeat не бывает вместе с stack"),
            (group("group g combine union repeat circle 4"), "repeat не бывает вместе с combine"),
            (format!("{BOARD}rect g size 4 4 repeat circle 4\n"), "repeat бывает у group"),
            (format!("{BOARD}symbol Coin {{\n  rect body size 4 4\n}}\nuse Coin c repeat circle 4\n"), "копия символа сама не повторяется"),
            (format!("{BOARD}rect g-2 size 4 4\ngroup g repeat circle 2 {{\n  rect own size 4 4\n}}\n"), "id «g-2» уже занят"),
        ] {
            let told = crate::parse(&src).err().unwrap_or_else(|| panic!("no error, wanted «{want}»:\n{src}"));
            assert!(told.iter().any(|e| e.message.contains(want)), "wanted «{want}», got {told:#?}\n{src}");
        }
    }

    #[test]
    fn the_repeat_of_a_group_is_rewritten_with_what_was_said_about_the_repeats_that_go() {
        let src = format!(
            "{BOARD}group dots at 100 50 repeat grid 3 2 step 20 30 {{\n  ellipse dot size 8 8\n}}\nanim Blink 1s {{\n  dots-3-1-dot.opacity 0: 1 | 1: 0\n  dots-2-2-dot.opacity 0: 1 | 1: 0\n  dot.opacity 0: 1 | 1: 0\n}}\noverride {{\n  dots-3-2.x = 44\n  dots-2-1.y = 3\n}}\n"
        );
        let before = scene(&src);
        // A column less: the third column goes with its track and its `override` line.
        let narrow = RepeatKind::Grid { cols: 2, rows: 2, step: (20.0, 30.0) };
        let out = set_repeat(&src, &before, "dots", Some(&narrow)).unwrap();
        assert!(out.contains("group dots at 100 50 repeat grid 2 2 step 20 30 {"), "{out}");
        assert!(!out.contains("dots-3-1-dot") && !out.contains("dots-3-2"), "{out}");
        assert!(out.contains("dots-2-2-dot.opacity") && out.contains("dots-2-1.y = 3") && out.contains("  dot.opacity"), "{out}");
        let after = scene(&out);
        assert_eq!(after.repeats["dots"].kind, narrow);
        // Another kind keeps the place of the words; nothing changes when nothing is new.
        let circle = RepeatKind::Circle { count: 5, arc: 360.0, upright: false };
        let round = set_repeat(&out, &after, "dots", Some(&circle)).unwrap();
        assert!(round.contains("group dots at 100 50 repeat circle 5 {"), "{round}");
        // `dots-2-1` of the grid is not a repeat of the circle (they are `dots-2` … `dots-5`).
        assert!(!round.contains("dots-2-1") && !round.contains("dots-2-2"), "{round}");
        assert_eq!(set_repeat(&out, &after, "dots", Some(&narrow)), None);
        // No repeat: an ordinary group again.
        let plain = set_repeat(&out, &after, "dots", None).unwrap();
        assert!(plain.contains("group dots at 100 50 {") && !plain.contains("dots-2"), "{plain}");
        let plain_scene = scene(&plain);
        assert!(plain_scene.repeats.is_empty());
        // And back: the words go to the end of the line. Along an outline the place comes from
        // the outline, so `at` leaves the line.
        let again = set_repeat(&plain, &plain_scene, "dots", Some(&narrow)).unwrap();
        assert!(again.contains("group dots at 100 50 repeat grid 2 2 step 20 30 {"), "{again}");
        // A circle to start with brings its centre; a pivot that is written stays as it is.
        let ring = set_repeat_around(&plain, &plain_scene, "dots", &circle, Vec2::new(0.0, 12.0)).unwrap();
        assert!(ring.contains("group dots at 100 50 pivot 0 12 repeat circle 5 {"), "{ring}");
        let ring = set_repeat_around(&out, &after, "dots", &circle, Vec2::new(0.0, 12.0)).unwrap();
        assert!(ring.contains("group dots at 100 50 repeat circle 5 pivot 0 12 {"), "{ring}");
        let own = scene(&ring);
        let grid = set_repeat(&ring, &own, "dots", Some(&narrow)).unwrap();
        let ring = set_repeat_around(&grid, &scene(&grid), "dots", &circle, Vec2::new(5.0, 5.0)).unwrap();
        assert!(ring.contains("repeat circle 5 pivot 0 12 {") && !ring.contains("pivot 5 5"), "{ring}");
        let framed = format!("{plain}rect frame at 200 150 size 100 100\n");
        let along = RepeatKind::Along { path: "frame".into(), count: 4, offset: 6.0, upright: true };
        let out = set_repeat(&framed, &scene(&framed), "dots", Some(&along)).unwrap();
        assert!(out.contains("group dots repeat along frame 4 offset 6 upright {"), "{out}");
        assert_eq!(scene(&out).repeats["dots"].kind, along);
        // A repeat written with a name is changed in the script, and a made group is not a line.
        let named = format!("{BOARD}let n = 3\ngroup dots repeat circle n {{\n  ellipse dot size 8 8\n}}\n");
        assert_eq!(set_repeat(&named, &scene(&named), "dots", Some(&circle)), None);
        assert_eq!(set_repeat(&src, &before, "dots-2-1", Some(&circle)), None);
    }

    #[test]
    fn deleting_a_written_object_takes_what_was_said_about_it_in_every_repeat() {
        let src = format!(
            "{BOARD}group dots repeat grid 2 1 step 20 0 {{\n  ellipse dot size 8 8\n  rect tick size 2 2\n}}\nanim Blink 1s {{\n  dots-2-1-tick.opacity 0: 1 | 1: 0\n  dots-2-1-dot.opacity 0: 1 | 1: 0\n}}\n"
        );
        let (out, deleted, _) = crate::delete_nodes(&src, &scene(&src), &["tick".to_string()], None).unwrap();
        assert_eq!(deleted, 1);
        assert!(!out.contains("tick") && out.contains("dots-2-1-dot.opacity"), "{out}");
    }
}
