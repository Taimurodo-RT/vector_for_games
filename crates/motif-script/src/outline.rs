//! Commands that leave a plain outline: a shape turned into a `path`, the stroke of a shape
//! turned into a filled outline, and an outline set off from a shape outwards or inwards.
//!
//! All three write the outline in the object's own space, to a hundredth of a unit, so the
//! object keeps its place, turn, scale and pivot, and the picture stays what it was.

use crate::edit::{path_to_d, splice_all, whole_lines};
use crate::{Scene, Span};
use motif_core::kurbo::{self, BezPath, ParamCurveArclen, PathEl, Point, Shape};
use motif_core::{Cap, Join, Node, Prop, ShapeKind};
use motif_core::{CombineOp, Inside, closed, combine_two};

/// What makes a shape a `rect`, `ellipse`, `star` or `polygon`: gone once it is a path.
const SHAPE_KEYS: [&str; 4] = ["size", "r", "points", "inner"];
/// The stroke as written on a line.
const STROKE_KEYS: [&str; 5] = ["stroke", "stroke-gradient", "dash", "cap", "join"];
/// The most dashes a stroke may have to be turned into an outline.
const MAX_DASHES: f64 = 2000.0;
/// What a new object next to the source takes over so that it stands in the same place.
const PLACE_KEYS: [&str; 7] = ["at", "rot", "scale", "skew", "pivot", "opacity", "blend"];
/// The paint an offset outline takes over from its source.
const PAINT_KEYS: [&str; 7] = ["fill", "gradient", "stroke", "stroke-gradient", "dash", "cap", "join"];

fn cents(path: &BezPath) -> BezPath {
    let r = |p: Point| Point::new((p.x * 100.0).round() / 100.0 + 0.0, (p.y * 100.0).round() / 100.0 + 0.0);
    path.elements()
        .iter()
        .map(|el| match *el {
            PathEl::MoveTo(p) => PathEl::MoveTo(r(p)),
            PathEl::LineTo(p) => PathEl::LineTo(r(p)),
            PathEl::QuadTo(a, p) => PathEl::QuadTo(r(a), r(p)),
            PathEl::CurveTo(a, b, p) => PathEl::CurveTo(r(a), r(b), r(p)),
            PathEl::ClosePath => PathEl::ClosePath,
        })
        .collect()
}

/// The object as the commands need it: written by hand, not a group or a bone, with an outline.
fn source<'a>(scene: &'a Scene, id: &str) -> Result<(&'a Node, BezPath), String> {
    let node = scene.doc.node(id).ok_or("объект не найден")?;
    if node.kind.is_container() || node.kind == ShapeKind::Bone {
        return Err(format!("«{id}» — не фигура: выберите фигуру, контур или текст"));
    }
    if scene.sites.get(id).is_none_or(|s| s.generated) {
        return Err(format!("«{id}» создан циклом или символом: его правят в тексте"));
    }
    let path = node.shape_path().filter(|p| !p.elements().is_empty()).ok_or("у объекта нет контура")?;
    Ok((node, path))
}

/// Where one of `props` of the object is animated: in a clip or by a character parameter.
fn animated(scene: &Scene, id: &str, props: &[Prop]) -> Option<String> {
    let doc = &scene.doc;
    if let Some(clip) = doc.animations.iter().find(|a| a.tracks.iter().any(|t| t.node == id && props.contains(&t.prop))) {
        return Some(format!("в клипе «{}»", clip.name));
    }
    if let Some(driver) = doc.drivers.iter().find(|d| d.tracks.iter().any(|t| t.node == id && props.contains(&t.prop))) {
        return Some(format!("параметром «{}»", driver.parameter));
    }
    doc.correctives.iter().find(|c| c.tracks.iter().any(|t| t.node == id && props.contains(&t.prop))).map(|c| format!("коррекцией «{}»", c.id))
}

/// The keys that change the size or the kind of outline of a shape: a path has none of them.
const FORM_PROPS: [Prop; 5] = [Prop::Width, Prop::Height, Prop::Radius, Prop::Points, Prop::Inner];

/// What keeps reshaping the outline while the scene plays, so that one frozen outline cannot
/// stand for it.
fn reshaped(node: &Node) -> Option<&'static str> {
    Some(if node.skin.is_some() {
        "его гнут кости"
    } else if node.forms.is_some() {
        "у него опорные формы контура"
    } else if node.morph.is_some() {
        "у него морфинг"
    } else if node.soft.is_some() {
        "это мягкое тело"
    } else if node.rope.is_some() {
        "это верёвка или ткань"
    } else if node.fluid.is_some() {
        "это жидкость"
    } else if node.emit.is_some() {
        "это источник частиц"
    } else if node.body.is_some() {
        "это 3D-тело"
    } else if node.wrap {
        "он обёрнут вокруг шара"
    } else {
        return None;
    })
}

/// What needs the object to stay a shape with a size, which a path does not have.
fn sized(scene: &Scene, node: &Node) -> Option<&'static str> {
    Some(if node.fit || node.slice.is_some() || node.part.is_some() || node.ui {
        "это деталь интерфейса, она тянется по размеру"
    } else if node.fluid.is_some() {
        "это жидкость, её объём задан размером"
    } else if node.roll.is_some() || scene.doc.nodes.iter().any(|n| n.on.as_deref() == Some(node.id.as_str())) {
        "это шар: его радиус задан размером"
    } else {
        return None;
    })
}

/// Why a new object next to this one would not stand where it stands: something other than its
/// own place puts it there.
fn placed_by(scene: &Scene, node: &Node) -> Option<&'static str> {
    Some(if node.on.is_some() {
        "он стоит на шаре"
    } else if node.hang.is_some() {
        "он висит на верёвке"
    } else if node.fit || node.anchor.is_some() || node.parent.is_some_and(|p| scene.doc.nodes[p].stack.is_some()) {
        "его ставит раскладка, новый объект сдвинул бы соседей"
    } else {
        return None;
    })
}

fn span_of(scene: &Scene, id: &str, key: &str) -> Option<Span> {
    scene.prop_spans.get(&(id.to_string(), key.to_string())).copied()
}

/// The span of a property with the spaces in front of it, so removing it leaves no double space.
fn with_space(src: &str, span: Span) -> Span {
    Span::new(src[..span.start].trim_end_matches([' ', '\t']).len(), span.end)
}

/// Edits that make the line of `id` a `path` line with the outline `d`: the kind word, the
/// properties only a shape has, and the outline itself.
fn as_path_edits(src: &str, scene: &Scene, node: &Node, d: &BezPath) -> Result<Vec<(Span, String)>, String> {
    let site = scene.sites.get(&node.id).ok_or("нет строки объекта")?;
    let text = format!("d \"{}\"", path_to_d(&cents(d)));
    let mut edits = Vec::new();
    if node.kind == ShapeKind::Path {
        match scene.path_sources.get(&node.id) {
            Some(span) => edits.push((*span, format!("\"{}\"", path_to_d(&cents(d))))),
            None => edits.push((Span::new(site.insert_at, site.insert_at), format!(" {text}"))),
        }
        return Ok(edits);
    }
    let word = &src[site.span.start..site.id_span.start];
    if word.trim() != node.kind.keyword() {
        return Err(format!("«{}» записан необычно: превратите его в контур в тексте", node.id));
    }
    edits.push((Span::new(site.span.start, site.span.start + node.kind.keyword().len()), "path".into()));
    // The outline takes the place of the size; a shape written without one gets it at the end.
    let mut placed = false;
    for key in SHAPE_KEYS {
        if let Some(span) = span_of(scene, &node.id, key) {
            if placed {
                edits.push((with_space(src, span), String::new()));
            } else {
                edits.push((span, text.clone()));
                placed = true;
            }
        }
    }
    if !placed {
        edits.push((Span::new(site.insert_at, site.insert_at), format!(" {text}")));
    }
    Ok(edits)
}

/// Returns the script with the shape `id` (`rect`, `ellipse`, `star`, `polygon`) written as a
/// `path` of the same outline. The id, the place, the paint and every other property stay, so
/// keys and links to the object keep working.
pub fn shape_to_path(src: &str, scene: &Scene, id: &str) -> Result<String, String> {
    let (node, path) = source(scene, id)?;
    match node.kind {
        ShapeKind::Path => return Err("это уже контур: его точки правятся по Enter".into()),
        ShapeKind::Text => return Err("текст превращает в контур кнопка «В кривые»".into()),
        _ => {}
    }
    if let Some(why) = sized(scene, node) {
        return Err(format!("«{id}» должен остаться фигурой: {why}"));
    }
    if let Some(place) = animated(scene, id, &FORM_PROPS) {
        return Err(format!("размер или форма «{id}» анимированы {place}: у контура таких ключей нет. Уберите их или анимируйте масштаб"));
    }
    Ok(splice_all(src, as_path_edits(src, scene, node, &path)?))
}

fn stroke_style(width: f64, cap: Cap, join: Join, dash: &[f64]) -> kurbo::Stroke {
    let cap = match cap {
        Cap::Butt => kurbo::Cap::Butt,
        Cap::Round => kurbo::Cap::Round,
        Cap::Square => kurbo::Cap::Square,
    };
    let join = match join {
        Join::Miter => kurbo::Join::Miter,
        Join::Round => kurbo::Join::Round,
        Join::Bevel => kurbo::Join::Bevel,
    };
    let mut dash = dash.to_vec();
    if dash.len() % 2 == 1 {
        dash.extend(dash.clone());
    }
    // The same line the renderer draws.
    kurbo::Stroke { width, join, miter_limit: 4.0, start_cap: cap, end_cap: cap, ..kurbo::Stroke::default() }.with_dashes(0.0, dash)
}

/// The area a stroke covers, as one clean outline: overlaps of the raw outline are merged.
/// The flag tells whether the merge worked; without it the outline must be filled non-zero.
fn stroke_area(path: &BezPath, style: &kurbo::Stroke) -> (BezPath, bool) {
    let raw = kurbo::stroke(path.iter(), style, &kurbo::StrokeOpts::default(), 0.01);
    match combine_two(&raw, &BezPath::new(), CombineOp::Union, Inside::NonZero) {
        Ok(clean) if !clean.elements().is_empty() => (clean, true),
        _ => (raw, false),
    }
}

/// How the object moves on its own, which a new object next to it would not do.
fn moved(scene: &Scene, node: &Node) -> Option<String> {
    if node.spring.is_some() || node.jiggle.is_some() || node.squash.is_some() {
        return Some("пружиной".into());
    }
    let moves = [Prop::X, Prop::Y, Prop::Rotation, Prop::ScaleX, Prop::ScaleY, Prop::Opacity, Prop::Width, Prop::Height];
    animated(scene, &node.id, &moves)
}

/// The note for the user when the object moves and the new one made next to it does not.
fn left_behind(scene: &Scene, node: &Node, new_id: &str) -> Option<String> {
    let how = moved(scene, node)?;
    Some(format!("«{}» двигается {how}, а новый контур «{new_id}» — нет: сгруппируйте их и перенесите движение на группу", node.id))
}

/// The new id for an object made from `id`: `id_suffix`, with a number when that is taken.
fn derived_id(scene: &Scene, id: &str, suffix: &str) -> String {
    let free = |name: &str| scene.doc.node(name).is_none() && scene.doc.parameter(name).is_none();
    let base = format!("{id}_{suffix}");
    if free(&base) {
        return base;
    }
    (2..).map(|i| format!("{base}{i}")).find(|name| free(name)).expect("ids are unbounded")
}

/// The line of a new path next to `node`: same place, the outline `d` and the paint `paint`.
fn sibling_line(src: &str, scene: &Scene, node: &Node, id: &str, d: &BezPath, paint: &str) -> String {
    let mut line = format!("path {id}");
    for key in PLACE_KEYS {
        if let Some(span) = span_of(scene, &node.id, key) {
            line.push(' ');
            line.push_str(&src[span.start..span.end]);
        }
    }
    line.push_str(&format!(" d \"{}\" {paint}", path_to_d(&cents(d))));
    line
}

/// Where a new line next to the object goes (before or after its lines), and its indentation.
fn beside(src: &str, scene: &Scene, id: &str, above: bool) -> Option<(usize, String)> {
    let lines = whole_lines(src, scene.sites.get(id)?.span);
    let indent = src[lines.start..].chars().take_while(|c| *c == ' ' || *c == '\t').collect();
    // Later in the file is higher in the picture.
    Some((if above { lines.end } else { lines.start }, indent))
}

fn new_line(src: &str, at: usize, indent: &str, line: &str) -> (Span, String) {
    // The last line of a file may have no line break of its own.
    let lead = if at > 0 && !src[..at].ends_with('\n') { "\n" } else { "" };
    (Span::new(at, at), format!("{lead}{indent}{line}\n"))
}

/// Returns the script with the stroke of `id` turned into a filled outline, the id of the object
/// that holds it, and a note for the user when something needs their eye.
///
/// A shape without a fill becomes that outline itself and keeps its id. A filled shape keeps its
/// fill and loses the stroke; the outline is a new path right above it, in the same place. Dashes,
/// caps and joins become part of the outline.
pub fn stroke_to_path(src: &str, scene: &Scene, id: &str) -> Result<(String, String, Option<String>), String> {
    let (node, path) = source(scene, id)?;
    if (node.stroke.is_none() && node.stroke_gradient.is_none()) || node.stroke_width <= 0.0 {
        return Err(format!("у «{id}» нет обводки"));
    }
    if let Some(place) = animated(scene, id, &[Prop::StrokeColor, Prop::StrokeWidth]) {
        return Err(format!("обводка «{id}» анимирована {place}: контур получился бы только для одного кадра"));
    }
    if let Some(why) = reshaped(node) {
        return Err(format!("«{id}» меняет форму, пока сцена играет ({why}): обводка идёт за ним, а контур остался бы один"));
    }
    // Every dash becomes a part of the outline: a dust of dashes would be a file nobody can edit.
    let period: f64 = node.dash.iter().sum();
    if !node.dash.is_empty() {
        let length: f64 = path.segments().map(|s| s.arclen(0.1)).sum();
        if period <= 0.0 || length / period > MAX_DASHES {
            return Err(format!("пунктир «{id}» слишком мелкий: вышло бы больше {MAX_DASHES} штрихов. Сделайте штрихи длиннее или уберите пунктир"));
        }
    }
    let (area, clean) = stroke_area(&path, &stroke_style(node.stroke_width, node.cap, node.join, &node.dash));
    if area.elements().is_empty() {
        return Err("обводка не даёт площади".into());
    }
    // The paint of the stroke becomes the fill of its outline.
    let paint = match span_of(scene, id, "stroke-gradient") {
        Some(span) => src[span.start..span.end].trim_start_matches("stroke-").to_string(),
        None => format!("fill {}", node.stroke.map_or("#000000".into(), |c| c.to_hex())),
    };
    let rule = if clean { " evenodd" } else { "" };
    let unstroke = |edits: &mut Vec<(Span, String)>| {
        for key in STROKE_KEYS {
            if let Some(span) = span_of(scene, id, key) {
                edits.push((with_space(src, span), String::new()));
            }
        }
    };
    let filled = node.fill.is_some() || node.gradient.is_some();
    if !filled {
        if node.kind == ShapeKind::Text {
            return Err("у текста без заливки сначала нажмите «В кривые»".into());
        }
        if node.kind != ShapeKind::Path {
            if let Some(why) = sized(scene, node) {
                return Err(format!("«{id}» должен остаться фигурой: {why}"));
            }
            if let Some(place) = animated(scene, id, &FORM_PROPS) {
                return Err(format!("размер или форма «{id}» анимированы {place}: у контура таких ключей нет"));
            }
        }
        let site = scene.sites.get(id).ok_or("нет строки объекта")?;
        let mut edits = as_path_edits(src, scene, node, &area)?;
        unstroke(&mut edits);
        for key in ["fill", "gradient", "evenodd"] {
            if let Some(span) = span_of(scene, id, key) {
                edits.push((with_space(src, span), String::new()));
            }
        }
        edits.push((Span::new(site.insert_at, site.insert_at), format!(" {paint}{rule}")));
        return Ok((splice_all(src, edits), id.to_string(), None));
    }
    if let Some(why) = placed_by(scene, node) {
        return Err(format!("контур обводки стал бы отдельным объектом, а «{id}» так не продолжить: {why}"));
    }
    if let Some(place) = animated(scene, id, &FORM_PROPS) {
        return Err(format!("размер или форма «{id}» анимированы {place}: контур обводки не менялся бы вместе с ним"));
    }
    let new_id = derived_id(scene, id, "stroke");
    let line = sibling_line(src, scene, node, &new_id, &area, &format!("{paint}{rule}"));
    let (at, indent) = beside(src, scene, id, true).ok_or("нет строки объекта")?;
    let mut edits = vec![new_line(src, at, &indent, &line)];
    unstroke(&mut edits);
    let note = left_behind(scene, node, &new_id);
    Ok((splice_all(src, edits), new_id, note))
}

/// Returns the script with a new path set off from the outline of `id` by `distance` (outwards
/// when positive, inwards when negative), its id, and a note when the source moves without it. The source stays as it is; the new path
/// takes its place and paint and stands right under it (outwards) or right above it (inwards).
/// `sharp` keeps corners sharp, otherwise they are rounded.
pub fn offset_path(src: &str, scene: &Scene, id: &str, distance: f64, sharp: bool) -> Result<(String, String, Option<String>), String> {
    let (node, path) = source(scene, id)?;
    if !distance.is_finite() || distance.abs() < 0.005 {
        return Err("задайте смещение, отличное от нуля".into());
    }
    if let Some(why) = reshaped(node) {
        return Err(format!("«{id}» меняет форму, пока сцена играет ({why}): новый контур не менялся бы вместе с ним"));
    }
    if let Some(why) = placed_by(scene, node) {
        return Err(format!("новый контур не встал бы рядом с «{id}»: {why}"));
    }
    let band_style = stroke_style(distance.abs() * 2.0, Cap::Round, if sharp { Join::Miter } else { Join::Round }, &[]);
    let band = kurbo::stroke(path.iter(), &band_style, &kurbo::StrokeOpts::default(), 0.01);
    let has_area = path.elements().iter().any(|el| matches!(el, PathEl::ClosePath)) || node.fill.is_some() || node.gradient.is_some();
    let rule = if node.even_odd { Inside::EvenOdd } else { Inside::NonZero };
    let nonzero = Inside::NonZero;
    let fail = |e: String| format!("не удалось построить контур: {e}");
    let result = if !has_area {
        // An open line has no inside: its offset is the band around it.
        if distance < 0.0 {
            return Err("у незамкнутой линии нет внутренней стороны: задайте смещение наружу".into());
        }
        combine_two(&band, &BezPath::new(), CombineOp::Union, nonzero).map_err(fail)?
    } else {
        // The area of the shape by its own fill rule, then the band added to it or cut out of it.
        let area = combine_two(&closed(&path), &BezPath::new(), CombineOp::Union, rule).map_err(fail)?;
        let band = combine_two(&band, &BezPath::new(), CombineOp::Union, nonzero).map_err(fail)?;
        let op = if distance > 0.0 { CombineOp::Union } else { CombineOp::Subtract };
        combine_two(&area, &band, op, nonzero).map_err(fail)?
    };
    if result.elements().is_empty() || result.bounding_box().area() < 1e-6 {
        return Err("от фигуры ничего не осталось: смещение внутрь больше половины её толщины".into());
    }
    let new_id = derived_id(scene, id, "offset");
    let mut paint = String::new();
    for key in PAINT_KEYS {
        if let Some(span) = span_of(scene, id, key) {
            paint.push_str(&src[span.start..span.end]);
            paint.push(' ');
        }
    }
    if node.fill.is_none() && node.gradient.is_none() && span_of(scene, id, "fill").is_none() {
        paint.push_str("fill none ");
    }
    paint.push_str("evenodd");
    let line = sibling_line(src, scene, node, &new_id, &result, &paint);
    let (at, indent) = beside(src, scene, id, distance < 0.0).ok_or("нет строки объекта")?;
    let note = left_behind(scene, node, &new_id);
    Ok((splice_all(src, vec![new_line(src, at, &indent, &line)]), new_id, note))
}
