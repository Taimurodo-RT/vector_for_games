//! Live boolean operations. A group with `combine subtract` draws one outline made of the
//! outlines of its shapes; the shapes stay in the file and can be moved, edited and animated,
//! and the outline follows them.
//!
//! The first shape of the group (the bottom one) draws the result with its own paint, exactly
//! as the one-time command of the editor leaves it; the other shapes are not drawn. An outline
//! that was already computed for the same shapes in the same places is taken from a cache, so
//! an operation whose shapes stand still costs nothing while the scene plays.

use crate::{Document, Prop, ShapeKind};
use kurbo::{BezPath, PathEl, Shape};
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::Mutex;

/// How the shapes of a `combine` group make one outline.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CombineOp {
    /// Everything covered by any shape.
    Union,
    /// The bottom shape minus the ones above it.
    Subtract,
    /// Only where all shapes overlap.
    Intersect,
    /// Where an odd number of shapes overlap: the overlaps become holes.
    Exclude,
}

impl CombineOp {
    pub const ALL: [CombineOp; 4] = [CombineOp::Union, CombineOp::Subtract, CombineOp::Intersect, CombineOp::Exclude];

    /// The word in a scene file: `combine subtract`.
    pub fn keyword(self) -> &'static str {
        match self {
            CombineOp::Union => "union",
            CombineOp::Subtract => "subtract",
            CombineOp::Intersect => "intersect",
            CombineOp::Exclude => "exclude",
        }
    }

    pub fn from_keyword(word: &str) -> Option<CombineOp> {
        Self::ALL.into_iter().find(|op| op.keyword() == word)
    }

    fn sweep(self) -> linesweeper::BinaryOp {
        match self {
            CombineOp::Union => linesweeper::BinaryOp::Union,
            CombineOp::Subtract => linesweeper::BinaryOp::Difference,
            CombineOp::Intersect => linesweeper::BinaryOp::Intersection,
            CombineOp::Exclude => linesweeper::BinaryOp::Xor,
        }
    }
}

/// Which insides of an outline count: every part it winds around, or only the parts it winds
/// around an odd number of times.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Inside {
    NonZero,
    EvenOdd,
}

/// Closes every subpath, so open lines take part as the area they enclose.
pub fn closed(path: &BezPath) -> BezPath {
    let mut out = BezPath::new();
    let mut open = false;
    for el in path.elements() {
        match el {
            PathEl::MoveTo(_) if open => {
                out.close_path();
                out.push(*el);
            }
            PathEl::MoveTo(_) => {
                out.push(*el);
                open = true;
            }
            PathEl::ClosePath => {
                out.close_path();
                open = false;
            }
            _ => out.push(*el),
        }
    }
    if open {
        out.close_path();
    }
    out
}

/// A strip thinner than this, in units, is a leftover of two edges that run together, not a
/// shape: no screen shows it, but a stroke around it would.
const SLIVER: f64 = 0.005;

/// The outline of `a op b`, the insides of both read by `inside`. The result reads the same by
/// either rule: its parts do not overlap and holes turn the other way.
pub fn combine_two(a: &BezPath, b: &BezPath, op: CombineOp, inside: Inside) -> Result<BezPath, String> {
    let rule = match inside {
        Inside::NonZero => linesweeper::FillRule::NonZero,
        Inside::EvenOdd => linesweeper::FillRule::EvenOdd,
    };
    let (op, a, b) = (op.sweep(), closed(a), closed(b));
    if !a.is_finite() || !b.is_finite() {
        return Err("у контура есть точка без места (NaN или бесконечность)".to_string());
    }
    // The sweep gives up on some degenerate outlines by panicking: that is an answer, not a crash.
    let swept = std::panic::catch_unwind(|| linesweeper::binary_op(&a, &b, rule, op));
    let contours = swept.map_err(|_| "контуры слишком вырождены для расчёта".to_string())?.map_err(|e| e.to_string())?;
    let mut out = BezPath::new();
    for c in contours.contours() {
        // Edges that coincide (a copy cut from itself, a cutter flush with a side) leave hairs
        // of no width between them.
        if c.path.area().abs() >= SLIVER / 2.0 * c.path.perimeter(0.1) {
            out.extend(c.path.iter());
        }
    }
    Ok(out)
}

/// The outline of an operation over `shapes`, bottom first, all in one space.
pub fn combine_shapes(shapes: &[BezPath], op: CombineOp) -> Result<BezPath, String> {
    let Some((first, rest)) = shapes.split_first() else { return Ok(BezPath::new()) };
    rest.iter().try_fold(closed(first), |acc, shape| combine_two(&acc, shape, op, Inside::NonZero))
}

/// Outlines computed lately, by what they were computed from.
struct Cache {
    made: HashMap<u64, (u64, Option<BezPath>)>,
    tick: u64,
}

/// More than any scene shows at once; an operation that changes every frame pushes old frames out.
const CACHE_SIZE: usize = 512;

static CACHE: Mutex<Option<Cache>> = Mutex::new(None);

fn key(shapes: &[BezPath], op: CombineOp) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    op.hash(&mut h);
    let point = |h: &mut std::collections::hash_map::DefaultHasher, p: kurbo::Point| {
        p.x.to_bits().hash(h);
        p.y.to_bits().hash(h);
    };
    for shape in shapes {
        shape.elements().len().hash(&mut h);
        for el in shape.elements() {
            match *el {
                PathEl::MoveTo(p) => {
                    0u8.hash(&mut h);
                    point(&mut h, p);
                }
                PathEl::LineTo(p) => {
                    1u8.hash(&mut h);
                    point(&mut h, p);
                }
                PathEl::QuadTo(a, p) => {
                    2u8.hash(&mut h);
                    point(&mut h, a);
                    point(&mut h, p);
                }
                PathEl::CurveTo(a, b, p) => {
                    3u8.hash(&mut h);
                    point(&mut h, a);
                    point(&mut h, b);
                    point(&mut h, p);
                }
                PathEl::ClosePath => 4u8.hash(&mut h),
            }
        }
    }
    h.finish()
}

/// [`combine_shapes`] that remembers its answers. `None` when the outline cannot be computed.
fn combine_cached(shapes: &[BezPath], op: CombineOp) -> Option<BezPath> {
    let key = key(shapes, op);
    {
        let mut guard = CACHE.lock().unwrap_or_else(|e| e.into_inner());
        let cache = guard.get_or_insert_with(|| Cache { made: HashMap::new(), tick: 0 });
        cache.tick += 1;
        let tick = cache.tick;
        if let Some((used, made)) = cache.made.get_mut(&key) {
            *used = tick;
            return made.clone();
        }
    }
    MISSES.with(|m| m.set(m.get() + 1));
    let made = combine_shapes(shapes, op).ok();
    let mut guard = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    let cache = guard.get_or_insert_with(|| Cache { made: HashMap::new(), tick: 0 });
    if cache.made.len() >= CACHE_SIZE {
        // Drop the half that was asked for longest ago.
        let mut ticks: Vec<u64> = cache.made.values().map(|(used, _)| *used).collect();
        ticks.sort_unstable();
        let old = ticks[ticks.len() / 2];
        cache.made.retain(|_, (used, _)| *used > old);
    }
    cache.made.insert(key, (cache.tick, made.clone()));
    made
}

/// How many outlines of live operations this thread has computed, not taken from the cache.
/// A scene whose operations stand still stops adding to it after the first frame.
pub fn combine_misses() -> u64 {
    MISSES.with(|m| m.get())
}

thread_local! {
    static MISSES: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

impl Document {
    /// The shapes a `combine` group is made of, in paint order: the first one draws the result.
    /// Hidden shapes, areas, bones and the shapes an inner operation has already taken in are
    /// left out.
    pub fn combine_sources(&self, group: usize) -> Vec<usize> {
        (group + 1..self.nodes.len())
            .take_while(|&i| self.is_inside(i, group))
            .filter(|&i| {
                let n = &self.nodes[i];
                !n.kind.is_container() && n.kind != ShapeKind::Bone && self.is_drawn(i)
            })
            .collect()
    }

    /// The group whose live operation takes node `i` in, if any: the nearest one above it.
    pub fn combined_in(&self, i: usize) -> Option<usize> {
        let mut cur = self.nodes.get(i)?.parent;
        while let Some(p) = cur {
            if self.nodes[p].combine.is_some() {
                return Some(p);
            }
            cur = self.nodes[p].parent;
        }
        None
    }

    /// The first shape of the group whose outline or place in the group changes while the scene
    /// plays: keys of a clip, a character parameter, bones, a morph, a spring. With such a shape
    /// the operation is computed anew on every frame it moves; without one, once.
    pub fn combine_moved_by(&self, group: usize) -> Option<&str> {
        // Paint does not touch an outline.
        let paint = |prop: Prop| {
            use Prop::*;
            matches!(
                prop,
                Opacity
                    | Fill
                    | StrokeColor
                    | StrokeWidth
                    | Blur
                    | ShadowX
                    | ShadowY
                    | ShadowBlur
                    | ShadowColor
                    | Glow
                    | InsetX
                    | InsetY
                    | InsetBlur
                    | InsetColor
                    | Noise
                    | Elevation
                    | Rough
            )
        };
        let keyed = |id: &str| {
            let on = |node: &str, prop: Prop| node == id && !paint(prop);
            self.animations.iter().any(|a| a.tracks.iter().any(|t| on(&t.node, t.prop)))
                || self.drivers.iter().any(|d| d.tracks.iter().any(|t| on(&t.node, t.prop)))
                || self.correctives.iter().any(|c| c.tracks.iter().any(|t| on(&t.node, t.prop)))
        };
        let lively = |i: usize| {
            let n = &self.nodes[i];
            n.skin.is_some()
                || n.morph.is_some()
                || n.forms.is_some()
                || n.spring.is_some()
                || n.jiggle.is_some()
                || n.squash.is_some()
                || n.on.is_some()
                || n.hang.is_some()
                || n.lattice.is_some()
                || keyed(&n.id)
        };
        (group + 1..self.nodes.len())
            .take_while(|&i| self.is_inside(i, group))
            .filter(|&i| !self.nodes[i].kind.is_container() && self.nodes[i].kind != ShapeKind::Bone)
            .find(|&i| {
                // The shape itself, or a group between it and the operation.
                let mut cur = Some(i);
                while let Some(k) = cur.filter(|&k| k != group) {
                    if lively(k) {
                        return true;
                    }
                    cur = self.nodes[k].parent;
                }
                false
            })
            .map(|i| self.nodes[i].id.as_str())
    }

    /// Computes every live operation: the first shape of a `combine` group gets the outline of
    /// the operation in place of its own, the other shapes of the group are marked as taken in
    /// and are not drawn. Runs after bones, morphs and simulations have shaped the outlines.
    pub fn combine_all(&mut self) {
        for node in &mut self.nodes {
            node.absorbed = false;
            node.combined = None;
        }
        if !self.nodes.iter().any(|n| n.combine.is_some()) {
            return;
        }
        // Children come after their parents, so going backwards an inner operation is ready
        // before the outer one asks for it.
        for group in (0..self.nodes.len()).rev() {
            let Some(op) = self.nodes[group].combine else { continue };
            if !self.is_visible(group) {
                continue;
            }
            let sources = self.combine_sources(group);
            let Some((&bearer, rest)) = sources.split_first() else { continue };
            if rest.is_empty() {
                continue;
            }
            let home = self.world_transform(bearer);
            if home.determinant().abs() < 1e-12 {
                continue;
            }
            let back = home.inverse();
            let shapes: Vec<BezPath> = sources
                .iter()
                .filter_map(|&i| {
                    let path = self.nodes[i].unbent_path()?;
                    Some(if i == bearer { path } else { back * self.world_transform(i) * path })
                })
                .collect();
            for &i in rest {
                self.nodes[i].absorbed = true;
            }
            match combine_cached(&shapes, op) {
                // Nothing is left: nothing is drawn.
                Some(made) if made.elements().is_empty() || made.bounding_box().area() <= 0.0 => self.nodes[bearer].absorbed = true,
                Some(made) => self.nodes[bearer].combined = Some(made),
                // The outline could not be computed: the bottom shape stays as it is.
                None => {}
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Artboard, Color, Node};

    fn doc(op: CombineOp) -> Document {
        let mut d = Document::new(Artboard { name: "a".into(), width: 200.0, height: 200.0, background: Color::WHITE });
        let mut g = Node::new("g", ShapeKind::Group);
        (g.x, g.y, g.combine) = (100.0, 100.0, Some(op));
        let mut a = Node::new("a", ShapeKind::Rect);
        (a.width, a.height, a.parent) = (80.0, 80.0, Some(0));
        let mut b = Node::new("b", ShapeKind::Ellipse);
        (b.x, b.y, b.width, b.height, b.parent) = (40.0, 0.0, 40.0, 40.0, Some(0));
        d.nodes = vec![g, a, b];
        d
    }

    #[test]
    fn the_bottom_shape_draws_the_result_and_the_rest_are_taken_in() {
        let mut d = doc(CombineOp::Subtract);
        d.rig();
        assert!(d.is_drawn(1) && !d.is_drawn(2));
        let made = d.nodes[1].path().unwrap();
        let area = made.area().abs();
        // A square of 80 minus half a circle of 40.
        let expect = 80.0 * 80.0 - std::f64::consts::PI * 20.0 * 20.0 / 2.0;
        assert!((area - expect).abs() < 2.0, "{area} {expect}");
        // The source itself is untouched: a second pass gives the same.
        assert_eq!(d.nodes[1].shape_path().unwrap().area().abs(), 6400.0);
        let once = d.nodes[1].path();
        d.rig();
        assert_eq!(d.nodes[1].path(), once);
    }

    #[test]
    fn posing_again_starts_from_the_shapes_not_from_the_last_result() {
        // Exclude twice would give the square back; a moved cutter would cut twice.
        let mut d = doc(CombineOp::Exclude);
        d.rig();
        let once = d.nodes[1].path().unwrap();
        d.rig();
        assert_eq!(d.nodes[1].path().unwrap(), once);
        d.nodes[2].x = -40.0;
        d.rig();
        let moved = d.nodes[1].path().unwrap();
        assert!((moved.area().abs() - once.area().abs()).abs() < 0.5, "one bite on the other side, not two: {} {}", moved.area(), once.area());
        assert!(moved.bounding_box().x0 < once.bounding_box().x0 - 15.0, "{:?} {:?}", moved.bounding_box(), once.bounding_box());
    }

    #[test]
    fn each_operation_covers_its_own_area() {
        let half = std::f64::consts::PI * 20.0 * 20.0 / 2.0;
        for (op, expect) in
            [(CombineOp::Union, 6400.0 + half), (CombineOp::Subtract, 6400.0 - half), (CombineOp::Intersect, half), (CombineOp::Exclude, 6400.0)]
        {
            let mut d = doc(op);
            d.rig();
            let made = d.nodes[1].path().unwrap();
            // Holes turn the other way, so the signed area is the covered area.
            assert!((made.area().abs() - expect).abs() < 2.0, "{op:?}: {} {expect}", made.area());
        }
    }

    #[test]
    fn a_moved_or_hidden_shape_changes_the_result_and_nothing_left_draws_nothing() {
        let mut d = doc(CombineOp::Subtract);
        d.nodes[2].x = 200.0;
        d.rig();
        assert!((d.nodes[1].path().unwrap().area().abs() - 6400.0).abs() < 1e-6, "the cutter is away");
        // A hidden shape takes no part: one shape alone is drawn as it is.
        let mut d = doc(CombineOp::Subtract);
        d.nodes[2].visible = false;
        d.rig();
        assert!(d.is_drawn(1) && d.nodes[1].combined.is_none());
        // The cutter covers the whole shape.
        let mut d = doc(CombineOp::Subtract);
        (d.nodes[2].x, d.nodes[2].width, d.nodes[2].height) = (0.0, 400.0, 400.0);
        d.rig();
        assert!(!d.is_drawn(1) && !d.is_drawn(2));
    }

    #[test]
    fn shapes_take_part_where_their_own_transforms_put_them() {
        let mut d = doc(CombineOp::Intersect);
        // The bottom shape is turned and scaled; the result is in its own space and so drawn
        // through the same transform.
        (d.nodes[1].rotation, d.nodes[1].scale_x) = (90.0, 0.5);
        d.rig();
        let world = d.world_transform(1) * d.nodes[1].path().unwrap();
        let b = world.bounding_box();
        // A square 80 turned and squeezed is 80 wide and 40 tall around (100, 100); the circle of
        // 40 at (140, 100) is cut by its right edge.
        assert!((b.x0 - 120.0).abs() < 0.1 && (b.x1 - 140.0).abs() < 0.1 && (b.y0 - 80.0).abs() < 0.1 && (b.y1 - 120.0).abs() < 0.1, "{b:?}");
    }

    #[test]
    fn an_inner_operation_is_one_shape_of_the_outer_one() {
        let mut d = doc(CombineOp::Union);
        let mut inner = Node::new("inner", ShapeKind::Group);
        (inner.parent, inner.combine) = (Some(0), Some(CombineOp::Subtract));
        let mut c = Node::new("c", ShapeKind::Rect);
        (c.x, c.width, c.height, c.parent) = (100.0, 40.0, 40.0, Some(3));
        let mut e = Node::new("e", ShapeKind::Rect);
        (e.x, e.width, e.height, e.parent) = (100.0, 20.0, 20.0, Some(3));
        d.nodes.extend([inner, c, e]);
        d.rig();
        assert!(d.is_drawn(1) && [2, 4, 5].iter().all(|&i| !d.is_drawn(i)));
        let half = std::f64::consts::PI * 20.0 * 20.0 / 2.0;
        let expect = 6400.0 + half + (1600.0 - 400.0);
        let area = d.nodes[1].path().unwrap().area().abs();
        assert!((area - expect).abs() < 2.0, "{area} {expect}");
    }

    #[test]
    fn an_operation_knows_whether_its_shapes_move() {
        use crate::{Animation, Ease, Keyframe, Track, Value};
        let clip = |node: &str, prop| Animation {
            name: "Go".into(),
            duration: 1.0,
            looping: false,
            events: vec![],
            tracks: vec![Track {
                node: node.into(),
                prop,
                keys: vec![
                    Keyframe { time: 0.0, value: Value::Num(0.0), ease: Ease::Linear },
                    Keyframe { time: 1.0, value: Value::Num(1.0), ease: Ease::Linear },
                ],
            }],
        };
        let mut d = doc(CombineOp::Subtract);
        assert_eq!(d.combine_moved_by(0), None);
        // The whole operation moves: the shapes stay where they are in it.
        d.animations = vec![clip("g", Prop::X)];
        assert_eq!(d.combine_moved_by(0), None);
        // Paint changes no outline.
        d.animations = vec![clip("b", Prop::Opacity)];
        assert_eq!(d.combine_moved_by(0), None);
        d.animations = vec![clip("b", Prop::Rotation)];
        assert_eq!(d.combine_moved_by(0), Some("b"));
        d.animations.clear();
        d.nodes[1].spring = Some(crate::Spring::default());
        assert_eq!(d.combine_moved_by(0), Some("a"));
    }

    #[test]
    fn shapes_that_stand_still_are_computed_once() {
        let mut d = doc(CombineOp::Exclude);
        // Unlike the shapes of any other test, so nobody has computed this one yet.
        d.nodes[2].x = 33.25;
        let before = combine_misses();
        d.rig();
        assert_eq!(combine_misses(), before + 1);
        let first = d.nodes[1].path();
        for _ in 0..20 {
            let mut again = d.clone();
            again.rig();
            assert_eq!(again.nodes[1].path(), first);
        }
        assert_eq!(combine_misses(), before + 1, "twenty more frames of the same shapes compute nothing");
        // A shape that moves is computed for each new place, and only for it.
        for step in 0..5 {
            let mut moved = d.clone();
            moved.nodes[2].y = 0.125 + step as f64;
            moved.rig();
        }
        assert_eq!(combine_misses(), before + 6);
    }

    #[test]
    fn the_cache_stays_small_and_keeps_what_is_still_asked_for() {
        let mut still = doc(CombineOp::Union);
        still.nodes[2].x = 17.375;
        still.rig();
        let before = combine_misses();
        // An operation that changes every frame, for longer than the cache is large.
        for frame in 0..CACHE_SIZE * 3 {
            let mut moving = doc(CombineOp::Union);
            moving.nodes[2].y = 1000.0 + frame as f64 * 0.01;
            moving.rig();
            let mut again = still.clone();
            again.rig();
        }
        assert_eq!(combine_misses(), before + CACHE_SIZE as u64 * 3, "the still operation was never computed again");
        assert!(CACHE.lock().unwrap().as_ref().unwrap().made.len() <= CACHE_SIZE);
    }

    #[test]
    fn edges_that_touch_or_coincide_neither_fail_nor_leave_hairs() {
        use kurbo::{Circle, Rect};
        let rect = |x: f64, y: f64, w: f64, h: f64| Rect::new(x, y, x + w, y + h).to_path(0.1);
        let circle = |x: f64, y: f64, r: f64| Circle::new((x, y), r).to_path(0.01);
        let plate = rect(0.0, 0.0, 40.0, 30.0);
        let disc = circle(0.0, 0.0, 20.0);
        let round = std::f64::consts::PI * 400.0;
        let mut line = BezPath::new();
        line.move_to((0.0, 10.0));
        line.line_to((40.0, 10.0));
        // What is cut and with what, then the area of union, subtract, intersect, exclude.
        let cases: [(&str, &BezPath, BezPath, [f64; 4]); 12] = [
            ("the same rect", &plate, plate.clone(), [1200.0, 0.0, 1200.0, 0.0]),
            ("a shared side", &plate, rect(40.0, 0.0, 40.0, 30.0), [2400.0, 1200.0, 0.0, 2400.0]),
            ("a part of a side", &plate, rect(40.0, 10.0, 40.0, 30.0), [2400.0, 1200.0, 0.0, 2400.0]),
            ("flush inside", &plate, rect(0.0, 0.0, 20.0, 30.0), [1200.0, 600.0, 600.0, 600.0]),
            ("a shared corner", &plate, rect(40.0, 30.0, 40.0, 30.0), [2400.0, 1200.0, 0.0, 2400.0]),
            ("the same circle", &disc, disc.clone(), [round, 0.0, round, 0.0]),
            ("circles that touch", &disc, circle(40.0, 0.0, 20.0), [round * 2.0, round, 0.0, round * 2.0]),
            ("a circle touching from inside", &disc, circle(10.0, 0.0, 10.0), [round, round * 0.75, round * 0.25, round * 0.75]),
            ("an open line", &plate, line, [1200.0, 1200.0, 0.0, 1200.0]),
            ("nothing", &plate, BezPath::new(), [1200.0, 1200.0, 0.0, 1200.0]),
            ("a point", &plate, rect(5.0, 5.0, 0.0, 0.0), [1200.0, 1200.0, 0.0, 1200.0]),
            ("no width", &plate, rect(5.0, 5.0, 0.0, 10.0), [1200.0, 1200.0, 0.0, 1200.0]),
        ];
        for (name, a, b, areas) in &cases {
            for (op, want) in CombineOp::ALL.into_iter().zip(areas) {
                let made = combine_two(a, b, op, Inside::NonZero).unwrap_or_else(|e| panic!("{name}, {}: {e}", op.keyword()));
                // Circles are drawn with curves a hair off the true circle.
                assert!((made.area().abs() - want).abs() < 0.5, "{name}, {}: area {} for {want}", op.keyword(), made.area());
                if *want == 0.0 {
                    assert!(made.elements().is_empty(), "{name}, {}: a hair is left: {}", op.keyword(), made.to_svg());
                }
            }
        }
        // A point nowhere is refused in words, not by a crash.
        for bad in [f64::NAN, f64::INFINITY] {
            assert!(combine_two(&plate, &rect(bad, 1.0, 1.0, 1.0), CombineOp::Union, Inside::NonZero).is_err());
        }
    }

    #[test]
    fn a_shape_sliding_over_another_never_blinks() {
        use kurbo::{Affine, Circle, Rect, RoundedRect};
        let mut star = BezPath::new();
        for i in 0..14 {
            let (a, r) = (std::f64::consts::PI * i as f64 / 7.0, if i % 2 == 0 { 30.0 } else { 15.0 });
            let at = (r * a.sin(), -r * a.cos());
            if i == 0 { star.move_to(at) } else { star.line_to(at) }
        }
        star.close_path();
        let plate = Rect::new(-30.0, -20.0, 30.0, 20.0).to_path(0.1);
        let shapes = [plate.clone(), Circle::new((0.0, 0.0), 25.0).to_path(0.01), RoundedRect::new(-30.0, -20.0, 30.0, 20.0, 8.0).to_path(0.01), star];
        // Whole steps bring sides exactly together, odd ones do not; a slide along a side keeps
        // two edges on one line all the way.
        for (k, shape) in shapes.iter().enumerate() {
            for (dir, step, turn) in [((1.0, 0.0), 0.5, 0.0), ((0.6, 0.8), 0.37, 0.4), ((0.0, 1.0), 1.0, 0.0)] {
                let op = CombineOp::ALL[k];
                // A shape moved by a step uncovers no more than the step times its outline.
                let most = step * 2.0 * shape.perimeter(0.1) + 1.0;
                let mut last: Option<f64> = None;
                for i in 0..=120 {
                    let t = -60.0 + i as f64 * step * 2.0;
                    let cutter = Affine::translate((dir.0 * t, dir.1 * t)) * Affine::rotate(turn) * shape.clone();
                    let made = combine_two(&plate, &cutter, op, Inside::NonZero).unwrap_or_else(|e| panic!("{} at {t}: {e}", op.keyword()));
                    let area = made.area().abs();
                    assert!(last.is_none_or(|l| (area - l).abs() < most), "{} with shape {k} at {t}: the area jumps from {last:?} to {area}", op.keyword());
                    last = Some(area);
                }
            }
        }
    }
}
