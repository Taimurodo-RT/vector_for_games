//! Boolean operations: several shapes combined into one `path`, like Pathfinder in Illustrator.

use crate::edit::{free_id, node_spans, path_to_d, remove_spans, whole_lines};
use crate::{Scene, fmt_num};
use motif_core::ShapeKind;
use motif_core::kurbo::{Affine, BezPath, PathEl, Point, Shape};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BoolOp {
    /// Everything covered by any shape.
    Union,
    /// The bottom shape minus the ones above it.
    Subtract,
    /// Only where all shapes overlap.
    Intersect,
    /// Where an odd number of shapes overlap: the overlaps become holes.
    Exclude,
}

impl BoolOp {
    pub const ALL: [BoolOp; 4] = [BoolOp::Union, BoolOp::Subtract, BoolOp::Intersect, BoolOp::Exclude];

    pub fn name(self) -> &'static str {
        match self {
            BoolOp::Union => "Объединить",
            BoolOp::Subtract => "Вычесть",
            BoolOp::Intersect => "Пересечь",
            BoolOp::Exclude => "Исключить",
        }
    }
}

/// Style properties the result takes over from the bottom shape, as written there.
const STYLE_KEYS: [&str; 10] = ["fill", "stroke", "gradient", "opacity", "dash", "cap", "join", "shadow", "blend", "hidden"];

/// Closes every subpath, so open lines take part as the area they enclose.
fn closed(path: &BezPath) -> BezPath {
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

fn combine(a: &BezPath, b: &BezPath, op: linesweeper::BinaryOp) -> Result<BezPath, String> {
    let contours = linesweeper::binary_op(a, b, linesweeper::FillRule::NonZero, op).map_err(|e| e.to_string())?;
    let mut out = BezPath::new();
    for c in contours.contours() {
        out.extend(c.path.iter());
    }
    Ok(out)
}

/// Returns the script with the shapes `ids` replaced by one `path` holding the result of `op`, and
/// the new path's id. The path takes the place and style of the bottom shape (the first in paint
/// order) and is filled even-odd, so holes stay holes. Errors explain what to change.
pub fn boolean(src: &str, scene: &Scene, ids: &[String], op: BoolOp) -> Result<(String, String), String> {
    let doc = &scene.doc;
    let mut nodes: Vec<usize> = ids.iter().filter_map(|id| doc.index_of(id)).collect();
    nodes.sort_unstable();
    nodes.dedup();
    // A single text is turned into outlines: a union with nothing else.
    let single_text = nodes.len() == 1 && doc.nodes[nodes[0]].kind == ShapeKind::Text;
    if nodes.len() < 2 && !single_text {
        return Err("выделите хотя бы две фигуры".into());
    }
    for &i in &nodes {
        let n = &doc.nodes[i];
        if n.kind == ShapeKind::Group {
            return Err(format!("«{}» — группа: выделите фигуры внутри неё", n.id));
        }
        if scene.sites.get(&n.id).is_none_or(|s| s.generated) {
            return Err(format!("«{}» создан циклом: его правят в тексте", n.id));
        }
    }
    let world = |i: usize| doc.nodes[i].path().map(|p| closed(&(doc.world_transform(i) * p)));
    let mut result = world(nodes[0]).ok_or("у фигуры нет контура")?;
    if single_text {
        // Overlapping letters (outlines with overlaps in the font) merge into one clean outline.
        result = combine(&result, &BezPath::new(), linesweeper::BinaryOp::Union)?;
    }
    let rest: Vec<BezPath> = nodes[1..].iter().filter_map(|&i| world(i)).collect();
    result = match op {
        BoolOp::Union => rest.iter().try_fold(result, |acc, p| combine(&acc, p, linesweeper::BinaryOp::Union))?,
        BoolOp::Intersect => rest.iter().try_fold(result, |acc, p| combine(&acc, p, linesweeper::BinaryOp::Intersection))?,
        BoolOp::Exclude => rest.iter().try_fold(result, |acc, p| combine(&acc, p, linesweeper::BinaryOp::Xor))?,
        BoolOp::Subtract => rest.iter().try_fold(result, |acc, p| combine(&acc, p, linesweeper::BinaryOp::Difference))?,
    };
    if result.elements().is_empty() {
        return Err("в результате ничего не осталось: фигуры не пересекаются".into());
    }

    let base = &doc.nodes[nodes[0]];
    // Into the bottom shape's parent, centred on the result.
    let to_parent = base.parent.map_or(Affine::IDENTITY, |p| doc.world_transform(p).inverse());
    let local = to_parent * result;
    let c = local.bounding_box().center();
    let c = Point::new((c.x * 10.0).round() / 10.0, (c.y * 10.0).round() / 10.0);
    let local = Affine::translate(-c.to_vec2()) * local;
    let id = free_id(scene, "path");
    let mut line = format!("path {id}");
    if let Some(name) = &base.name {
        line.push_str(&format!(" \"{}\"", name.replace('"', "'")));
    }
    line.push_str(&format!(" at {} {} d \"{}\"", fmt_num(c.x), fmt_num(c.y), path_to_d(&local)));
    let mut styled = false;
    for key in STYLE_KEYS {
        if let Some(span) = scene.prop_spans.get(&(base.id.clone(), key.to_string())) {
            line.push(' ');
            line.push_str(&src[span.start..span.end]);
            styled |= key == "fill" || key == "gradient";
        }
    }
    if !styled && base.fill.is_none() && base.gradient.is_none() {
        line.push_str(" fill none");
    }
    line.push_str(" evenodd");

    // The new line goes where the bottom shape's line was, with its indentation.
    let site = scene.sites.get(&base.id).ok_or("нет строки объекта")?;
    let base_line = whole_lines(src, site.span);
    let indent: String = src[base_line.start..].chars().take_while(|c| *c == ' ' || *c == '\t').collect();
    let mut spans = Vec::new();
    for &i in &nodes {
        spans.extend(node_spans(src, scene, &doc.nodes[i].id).ok_or("нет строки объекта")?);
    }
    // Where the bottom line starts once everything before it is removed.
    let mut sorted = spans.clone();
    sorted.sort_by_key(|s| s.start);
    let mut removed = 0;
    let mut covered = 0;
    for s in &sorted {
        let (a, b) = (s.start.max(covered), s.end.min(base_line.start));
        if b > a {
            removed += b - a;
        }
        covered = covered.max(s.end);
    }
    let at = base_line.start - removed;
    let out = remove_spans(src, spans);
    let mut text = out[..at].to_string();
    text.push_str(&format!("{indent}{line}\n"));
    text.push_str(&out[at..]);
    Ok((text, id))
}
