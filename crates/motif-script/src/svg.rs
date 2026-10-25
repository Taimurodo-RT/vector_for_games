//! SVG import: every visible shape of a file becomes a `path` line inside one new group.

use crate::edit::{insert_declaration, path_to_d};
use crate::{Scene, fmt_num};
use motif_core::kurbo::{Affine, BezPath, Point, Rect, Shape};
use std::collections::HashSet;

/// One shape read from the SVG, in artboard-independent SVG user space.
struct Piece {
    id: Option<String>,
    path: BezPath,
    fill: Option<String>,
    stroke: Option<String>,
    opacity: f64,
    even_odd: bool,
}

fn color(c: usvg::Color, alpha: f64) -> String {
    motif_core::Color { r: c.red, g: c.green, b: c.blue, a: (alpha * 255.0).round().clamp(0.0, 255.0) as u8 }.to_hex()
}

fn affine(t: usvg::Transform) -> Affine {
    Affine::new([t.sx, t.ky, t.kx, t.sy, t.tx, t.ty].map(f64::from))
}

fn bez(path: &usvg::tiny_skia_path::Path, t: Affine) -> BezPath {
    use usvg::tiny_skia_path::PathSegment as S;
    let p = |q: usvg::tiny_skia_path::Point| t * Point::new(q.x as f64, q.y as f64);
    let mut out = BezPath::new();
    for seg in path.segments() {
        match seg {
            S::MoveTo(a) => out.move_to(p(a)),
            S::LineTo(a) => out.line_to(p(a)),
            S::QuadTo(a, b) => out.quad_to(p(a), p(b)),
            S::CubicTo(a, b, c) => out.curve_to(p(a), p(b), p(c)),
            S::Close => out.close_path(),
        }
    }
    out
}

/// A paint as script values: a colour, or a `gradient …` property for gradients.
fn paint(p: &usvg::Paint, opacity: f64, t: Affine) -> (String, Option<String>) {
    let stops = |s: &[usvg::Stop]| -> Vec<String> {
        // The script spreads colours evenly; up to eight are kept.
        let n = s.len().min(8);
        (0..n).map(|k| &s[if n < 2 { 0 } else { k * (s.len() - 1) / (n - 1) }]).map(|s| color(s.color(), s.opacity().get() as f64 * opacity)).collect()
    };
    match p {
        usvg::Paint::Color(c) => (color(*c, opacity), None),
        usvg::Paint::LinearGradient(g) if g.stops().len() >= 2 => {
            let gt = t * affine(g.transform());
            let a = gt * Point::new(g.x1() as f64, g.y1() as f64);
            let b = gt * Point::new(g.x2() as f64, g.y2() as f64);
            let angle = (b - a).atan2().to_degrees().round();
            let first = stops(g.stops())[0].clone();
            (first, Some(format!("gradient linear {} {}", fmt_num(angle), stops(g.stops()).join(" "))))
        }
        usvg::Paint::RadialGradient(g) if g.stops().len() >= 2 => {
            let first = stops(g.stops())[0].clone();
            (first, Some(format!("gradient radial {}", stops(g.stops()).join(" "))))
        }
        usvg::Paint::LinearGradient(g) => (stops(g.stops()).first().cloned().unwrap_or_else(|| "#000000".into()), None),
        usvg::Paint::RadialGradient(g) => (stops(g.stops()).first().cloned().unwrap_or_else(|| "#000000".into()), None),
        // Patterns have no counterpart: a neutral grey keeps the shape visible.
        usvg::Paint::Pattern(_) => (color(usvg::Color::new_rgb(128, 128, 128), opacity), None),
    }
}

fn collect(group: &usvg::Group, opacity: f64, out: &mut Vec<Piece>) {
    let opacity = opacity * group.opacity().get() as f64;
    for node in group.children() {
        match node {
            usvg::Node::Group(g) => collect(g, opacity, out),
            usvg::Node::Path(p) if p.is_visible() => {
                let t = affine(p.abs_transform());
                let path = bez(p.data(), t);
                if path.elements().is_empty() {
                    continue;
                }
                let mut fill = None;
                let mut even_odd = false;
                if let Some(f) = p.fill() {
                    let (c, g) = paint(f.paint(), f.opacity().get() as f64, t);
                    fill = Some(g.unwrap_or_else(|| format!("fill {c}")));
                    even_odd = f.rule() == usvg::FillRule::EvenOdd;
                }
                let stroke = p.stroke().map(|s| {
                    let k = t.as_coeffs();
                    let scale = (k[0] * k[3] - k[1] * k[2]).abs().sqrt();
                    let (c, _) = paint(s.paint(), s.opacity().get() as f64, t);
                    let mut text = format!("stroke {c} {}", fmt_num(s.width().get() as f64 * scale));
                    if let Some(d) = s.dasharray() {
                        let d: Vec<String> = d.iter().take(8).map(|v| fmt_num(*v as f64 * scale)).collect();
                        text.push_str(&format!(" dash {}", d.join(" ")));
                    }
                    text.push_str(match s.linecap() {
                        usvg::LineCap::Butt => " cap butt",
                        usvg::LineCap::Round => "",
                        usvg::LineCap::Square => " cap square",
                    });
                    text.push_str(match s.linejoin() {
                        usvg::LineJoin::Miter | usvg::LineJoin::MiterClip => " join miter",
                        usvg::LineJoin::Round => "",
                        usvg::LineJoin::Bevel => " join bevel",
                    });
                    text
                });
                if fill.is_none() && stroke.is_none() {
                    continue;
                }
                out.push(Piece { id: (!p.id().is_empty()).then(|| p.id().to_string()), path, fill, stroke, opacity, even_odd });
            }
            _ => {}
        }
    }
}

/// An id made of letters, digits, `_` and `-` that the script reads back as the same word.
fn clean_id(s: &str) -> Option<String> {
    let id: String = s.chars().map(|c| if c.is_alphanumeric() || c == '_' || c == '-' { c } else { '_' }).collect();
    let first = id.chars().next()?;
    (first.is_alphabetic() || first == '_').then_some(id)
}

/// Words that cannot be an id: commands and flag properties.
fn reserved(id: &str) -> bool {
    matches!(id, "artboard" | "backdrop" | "let" | "for" | "in" | "anim" | "override" | "none" | "hidden" | "clip" | "evenodd")
        || motif_core::ShapeKind::from_keyword(id).is_some()
}

/// Returns the script with the shapes of an SVG file added as one group, and the group's id.
///
/// The group is placed at the centre of the artboard, scaled down when the drawing is larger than
/// the artboard. Each shape becomes a `path` with its fill, gradient and stroke. Text must be
/// converted to outlines in the drawing program first; embedded pictures are skipped.
pub fn import_svg(src: &str, scene: &Scene, svg: &[u8], name: &str) -> Result<(String, String), String> {
    let tree = usvg::Tree::from_data(svg, &usvg::Options::default()).map_err(|e| format!("не удалось прочитать SVG: {e}"))?;
    let mut pieces = Vec::new();
    collect(tree.root(), 1.0, &mut pieces);
    if pieces.is_empty() {
        return Err("в SVG нет фигур (текст нужно сначала перевести в кривые)".into());
    }
    let bounds = pieces.iter().map(|p| p.path.bounding_box()).reduce(|a, b| a.union(b)).unwrap_or(Rect::ZERO);
    let ab = &scene.doc.artboard;
    let fit = (ab.width * 0.9 / bounds.width().max(1e-9)).min(ab.height * 0.9 / bounds.height().max(1e-9)).min(1.0);
    let to_group = Affine::scale(fit) * Affine::translate(-bounds.center().to_vec2());

    let mut taken: HashSet<String> = scene.doc.nodes.iter().map(|n| n.id.clone()).collect();
    let mut fresh = |want: Option<String>, base: &str| -> String {
        let id = want.and_then(|w| clean_id(&w)).filter(|w| !reserved(w) && !taken.contains(w));
        let id = id.unwrap_or_else(|| (1..).map(|i| format!("{base}{i}")).find(|c| !taken.contains(c)).expect("ids are unbounded"));
        taken.insert(id.clone());
        id
    };
    let group = fresh(clean_id(name), "svg");
    let mut text = format!("group {group} \"{}\" at {} {} {{\n", name.replace('"', "'"), fmt_num((ab.width / 2.0).round()), fmt_num((ab.height / 2.0).round()));
    for p in pieces {
        let id = fresh(p.id, "shape");
        let outline = to_group * p.path;
        let c = outline.bounding_box().center();
        let c = Point::new((c.x * 10.0).round() / 10.0, (c.y * 10.0).round() / 10.0);
        let local = Affine::translate(-c.to_vec2()) * outline;
        let mut line = format!("  path {id} at {} {} d \"{}\"", fmt_num(c.x), fmt_num(c.y), path_to_d(&local));
        match p.fill {
            Some(f) => line.push_str(&format!(" {f}")),
            None => line.push_str(" fill none"),
        }
        if let Some(s) = p.stroke {
            // Stroke widths and dashes shrink with the drawing.
            let s = if fit < 1.0 { rescale_stroke(&s, fit) } else { s };
            line.push_str(&format!(" {s}"));
        }
        if p.even_odd {
            line.push_str(" evenodd");
        }
        if p.opacity < 0.999 {
            line.push_str(&format!(" opacity {}", fmt_num(p.opacity)));
        }
        text.push_str(&line);
        text.push('\n');
    }
    text.push_str("}\n");
    Ok((insert_declaration(src, scene, &text), group))
}

/// Multiplies the width and dash numbers of a `stroke #c w [dash …]` text by `k`.
fn rescale_stroke(s: &str, k: f64) -> String {
    let mut out = Vec::new();
    for (i, w) in s.split(' ').enumerate() {
        match w.parse::<f64>() {
            Ok(v) if i >= 2 => out.push(fmt_num(v * k)),
            _ => out.push(w.to_string()),
        }
    }
    out.join(" ")
}
