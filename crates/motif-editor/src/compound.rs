//! Editable compound paths. Untouched parts retain their original quadratic/cubic commands.
use crate::pathedit::Outline;
use motif_core::kurbo::{BezPath, CubicBez, ParamCurveNearest, PathEl, Point};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Anchor {
    pub part: usize,
    pub vertex: usize,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Part {
    pub outline: Outline,
    original: Outline,
    path: BezPath,
}
impl Part {
    fn new(outline: Outline, path: BezPath) -> Self {
        Self { original: outline.clone(), outline, path }
    }
    fn edited(outline: Outline) -> Self {
        let path = outline.to_bez();
        Self::new(outline, path)
    }
    pub fn to_bez(&self) -> BezPath {
        if self.outline == self.original { self.path.clone() } else { self.outline.to_bez() }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Compound {
    pub parts: Vec<Part>,
}
impl Compound {
    pub fn from_bez(path: &BezPath) -> Option<Self> {
        let mut parts = Vec::new();
        let mut part = BezPath::new();
        let mut start = None;
        let flush = |part: &mut BezPath, parts: &mut Vec<Part>| -> Option<()> {
            if !part.elements().is_empty() {
                parts.push(Part::new(Outline::from_bez(part)?, std::mem::take(part)));
            }
            Some(())
        };
        for &el in path.elements() {
            if let PathEl::MoveTo(p) = el {
                flush(&mut part, &mut parts)?;
                start = Some(p);
            } else if part.elements().is_empty() {
                // SVG permits another segment after Z, starting at the closed subpath's origin.
                if matches!(el, PathEl::ClosePath) {
                    continue;
                }
                part.move_to(start?);
            }
            part.push(el);
            if matches!(el, PathEl::ClosePath) {
                flush(&mut part, &mut parts)?;
            }
        }
        flush(&mut part, &mut parts)?;
        (!parts.is_empty()).then_some(Self { parts })
    }
    pub fn to_bez(&self) -> BezPath {
        let mut out = BezPath::new();
        for part in &self.parts {
            out.extend(part.to_bez().iter());
        }
        out
    }
    pub fn contains(&self, a: Anchor) -> bool {
        self.parts.get(a.part).is_some_and(|p| a.vertex < p.outline.verts.len())
    }
    /// Hit distance is measured on the screen, including nonuniform scale and skew.
    pub fn nearest(&self, p: Point, to_screen: impl Fn(Point) -> Point) -> Option<(usize, usize, f64)> {
        let mut best = None;
        let mut distance = 36.0;
        for (part, entry) in self.parts.iter().enumerate() {
            let o = &entry.outline;
            for segment in 0..o.segment_count() {
                let c = o.segment(segment);
                let c = CubicBez::new(to_screen(c.p0), to_screen(c.p1), to_screen(c.p2), to_screen(c.p3));
                let n = c.nearest(p, 1e-3);
                if n.distance_sq <= distance && n.t > 0.02 && n.t < 0.98 {
                    distance = n.distance_sq;
                    best = Some((part, segment, n.t));
                }
            }
        }
        best
    }
    pub fn split(&mut self, part: usize, segment: usize, t: f64) -> Anchor {
        Anchor { part, vertex: self.parts[part].outline.split(segment, t) }
    }
    pub fn reverse(&mut self, part: usize) {
        let o = &mut self.parts[part].outline;
        o.verts.reverse();
        if o.closed {
            o.verts.rotate_right(1);
        }
        for v in &mut o.verts {
            std::mem::swap(&mut v.cin, &mut v.cout);
        }
    }
    /// A closed contour opens at this anchor. An open contour splits into two at an interior anchor.
    pub fn break_at(&mut self, at: Anchor) -> bool {
        if !self.contains(at) {
            return false;
        }
        let o = &mut self.parts[at.part].outline;
        if o.closed {
            o.verts.rotate_left(at.vertex);
            let mut end = o.verts[0];
            end.cout = None;
            o.verts[0].cin = None;
            o.verts.push(end);
            o.closed = false;
        } else {
            if at.vertex == 0 || at.vertex + 1 == o.verts.len() {
                return false;
            }
            let mut right = o.verts.split_off(at.vertex);
            o.verts.push(right[0]);
            o.verts.last_mut().unwrap().cout = None;
            right[0].cin = None;
            self.parts.insert(at.part + 1, Part::edited(Outline { verts: right, closed: false }));
        }
        true
    }
    pub fn endpoint(&self, at: Anchor) -> bool {
        self.contains(at) && !self.parts[at.part].outline.closed && (at.vertex == 0 || at.vertex + 1 == self.parts[at.part].outline.verts.len())
    }
    /// Join two open contours. Coincident endpoints merge; otherwise insert a straight bridge.
    pub fn join(&mut self, a: Anchor, b: Anchor) -> Option<Anchor> {
        if a.part == b.part || !self.endpoint(a) || !self.endpoint(b) {
            return None;
        }
        if a.vertex == 0 {
            self.reverse(a.part);
        }
        if b.vertex != 0 {
            self.reverse(b.part);
        }
        let right = self.parts[b.part].outline.clone();
        let left = &mut self.parts[a.part].outline;
        let vertex = left.verts.len() - 1;
        if left.verts[vertex].p == right.verts[0].p {
            left.verts[vertex].cout = right.verts[0].cout;
            left.verts.extend_from_slice(&right.verts[1..]);
        } else {
            left.verts[vertex].cout = None;
            let mut right = right.verts;
            right[0].cin = None;
            left.verts.extend(right);
        }
        self.parts.remove(b.part);
        Some(Anchor { part: a.part - usize::from(b.part < a.part), vertex })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use motif_core::kurbo::{Affine, ParamCurve, Shape, Vec2};

    fn path(d: &str) -> BezPath {
        BezPath::from_svg(d).unwrap()
    }
    const RING: &str = "M0 0 L100 0 L100 100 L0 100 Z M20.123456789 20 Q20 40 20 60 L60 60 L60 20 Z M120 10 C130 -5 140 50 150 10";

    #[test]
    fn mixed_contours_round_trip_exactly_with_hole_winding_and_precise_neighbours() {
        let before = path(RING);
        let mut shape = Compound::from_bez(&before).unwrap();
        assert_eq!(shape.parts.len(), 3);
        assert_eq!(shape.to_bez(), before);
        assert_eq!(shape.to_bez().winding(Point::new(40.0, 40.0)), 0);
        let untouched = shape.parts[1].to_bez();
        let at = shape.split(0, 0, 0.3);
        shape.parts[at.part].outline.verts[at.vertex].translate(Vec2::new(0.0, -5.0));
        assert_eq!(shape.parts[1].to_bez(), untouched);
        let src = format!("artboard \"Test\" 200x200\n// keep me\npath ring d \"{RING}\" fill #ff0000\nanim Idle 1s loop {{\n ring.x 0: 0 | 1: 5\n}}\n");
        let scene = motif_script::parse(&src).unwrap();
        let saved = motif_script::set_path(&src, &scene, "ring", &shape.to_bez()).unwrap();
        let reopened = motif_script::parse(&saved).unwrap();
        let round_trip = Compound::from_bez(&reopened.doc.node("ring").unwrap().geometry).unwrap();
        assert_eq!(round_trip.parts[1].to_bez(), untouched);
        assert_eq!(reopened.doc.animations.len(), scene.doc.animations.len());
        assert!(saved.contains("ring.x 0: 0 | 1: 5"));
        assert!(saved.contains("// keep me"));
    }

    #[test]
    fn split_cubic_preserves_samples_including_closing_curve() {
        let mut shape = Compound::from_bez(&path("M0 0 C0 20 20 20 20 0 C20 -20 0 -20 0 0 Z M50 0 L60 0")).unwrap();
        let old = shape.parts[0].outline.segment(1);
        shape.split(0, 1, 0.37);
        let l = shape.parts[0].outline.segment(1);
        let r = shape.parts[0].outline.segment(2);
        for i in 0..=100 {
            let t = i as f64 / 100.0;
            let actual = if t <= 0.37 { l.eval(t / 0.37) } else { r.eval((t - 0.37) / 0.63) };
            assert!(actual.distance(old.eval(t)) < 1e-10);
        }
    }

    #[test]
    fn split_does_not_change_rendered_holes() {
        let src = format!("artboard \"Ring\" 180x120\npath ring d \"{RING}\" fill #ff0000\n");
        let a = motif_script::parse(&src).unwrap();
        let mut shape = Compound::from_bez(&a.doc.node("ring").unwrap().geometry).unwrap();
        shape.split(1, 0, 0.43);
        let b = motif_script::parse(&motif_script::set_path(&src, &a, "ring", &shape.to_bez()).unwrap()).unwrap();
        let options = motif_render::CpuOptions::default();
        let before = motif_render::render_cpu(&a.doc, options).unwrap();
        let after = motif_render::render_cpu(&b.doc, options).unwrap();
        assert_eq!(before.data(), after.data());
    }

    #[test]
    fn reverse_is_reversible_and_break_join_keeps_curve_geometry() {
        let original = path("M0 0 C0 10 10 10 10 0 C10 -10 20 -10 20 0 M50 50 L60 60 Z");
        let mut shape = Compound::from_bez(&original).unwrap();
        shape.reverse(0);
        shape.reverse(0);
        assert_eq!(shape.to_bez(), original);
        assert!(shape.break_at(Anchor { part: 0, vertex: 1 }));
        assert_eq!(shape.parts.len(), 3);
        shape.join(Anchor { part: 0, vertex: 1 }, Anchor { part: 1, vertex: 0 }).unwrap();
        assert_eq!(shape.to_bez(), original);
        assert!(!shape.break_at(Anchor { part: 0, vertex: 0 }));
    }

    #[test]
    fn opening_closed_curve_keeps_the_closing_segment() {
        let p = path("M0 0 C0 10 10 10 10 0 C10 -10 0 -10 0 0 Z");
        let mut shape = Compound::from_bez(&p).unwrap();
        assert!(shape.break_at(Anchor { part: 0, vertex: 0 }));
        assert!(!shape.parts[0].outline.closed);
        assert_eq!(shape.to_bez().elements(), &p.elements()[..p.elements().len() - 1]);
    }

    #[test]
    fn hit_testing_uses_screen_distance_under_nonuniform_reflected_skew() {
        let shape = Compound::from_bez(&path("M0 0 L100 0 M0 20 L100 20")).unwrap();
        let transform = Affine::new([-20.0, 0.0, 3.0, 0.1, 0.0, 0.0]);
        assert_eq!(shape.nearest(Point::new(-1000.0, 6.0), |p| transform * p).unwrap().0, 1);
        assert!(shape.nearest(Point::new(-1000.0, 9.0), |p| transform * p).is_none());
    }

    #[test]
    fn isolated_moves_tiny_edges_and_commands_after_close_are_retained() {
        for d in ["M1 2 M4 5 L6 7", "M0 0 L10 0 L0.00000001 0 Z", "M0 0 C1 2 2 1 0 0 Z"] {
            let p = path(d);
            let shape = Compound::from_bez(&p).unwrap();
            assert_eq!(shape.to_bez(), p);
        }
        let p = path("M0 0 L10 0 Z L20 20");
        let shape = Compound::from_bez(&p).unwrap();
        assert_eq!(shape.parts.len(), 2);
        assert_eq!(shape.parts[1].outline.verts[0].p, Point::ZERO);
    }

    #[test]
    fn joining_any_endpoints_keeps_both_curves_and_untouched_holes() {
        let original = path("M0 0 C0 5 10 5 10 0 M20 0 C20 -5 30 -5 30 0 M50 50 L50 60 L60 60 L60 50 Z");
        for a_part in 0..2 {
            for a_vertex in 0..2 {
                for b_vertex in 0..2 {
                    let mut shape = Compound::from_bez(&original).unwrap();
                    let a = shape.parts[a_part].outline.segment(0);
                    let b = shape.parts[1 - a_part].outline.segment(0);
                    let hole = shape.parts[2].to_bez();
                    let joined = shape.join(Anchor { part: a_part, vertex: a_vertex }, Anchor { part: 1 - a_part, vertex: b_vertex }).unwrap();
                    assert_eq!(joined, Anchor { part: 0, vertex: 1 });
                    assert_eq!(shape.parts[1].to_bez(), hole);
                    let o = &shape.parts[0].outline;
                    assert_eq!(o.verts.len(), 4);
                    for i in 0..=20 {
                        let t = i as f64 / 20.0;
                        assert!(o.segment(0).eval(t).distance(a.eval(if a_vertex == 0 { 1.0 - t } else { t })) < 1e-10);
                        assert!(o.segment(2).eval(t).distance(b.eval(if b_vertex == 1 { 1.0 - t } else { t })) < 1e-10);
                    }
                }
            }
        }
    }
}
