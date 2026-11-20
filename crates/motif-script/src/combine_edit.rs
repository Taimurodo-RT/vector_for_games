//! Editing live operations: selected shapes put into a `combine` group, the operation of a
//! group changed or taken off, and a group taken apart into the one outline it draws.

use crate::edit::{group_nodes, path_to_d, set_style, splice, whole_lines};
use crate::{Scene, Span};
use motif_core::kurbo::{Affine, Shape};
use motif_core::{CombineOp, ShapeKind};

/// What the outline left by taking a group apart takes over from the shape that drew it.
const PAINT_KEYS: [&str; 21] = [
    "fill",
    "gradient",
    "stroke",
    "stroke-gradient",
    "dash",
    "cap",
    "join",
    "outline",
    "shadow",
    "inset",
    "glow",
    "noise",
    "blur",
    "elevation",
    "blend",
    "rough",
    "boil",
    "hatch",
    "grain",
    "bands",
    "seed",
];

fn reparse(src: &str, base: Option<&std::path::Path>) -> Result<Scene, String> {
    crate::parse_in(src, base).map_err(|e| e.first().map_or_else(|| "скрипт не читается".to_string(), |e| e.message.clone()))
}

/// Returns the script with the shapes `ids` put into a new group that combines them with `op`,
/// and the group's id. Nothing moves; the bottom shape draws the result with its own paint and
/// the others only shape it. Errors explain what to change.
pub fn combine_nodes(src: &str, scene: &Scene, ids: &[String], op: CombineOp, base: Option<&std::path::Path>) -> Result<(String, String), String> {
    let doc = &scene.doc;
    let mut picked: Vec<usize> = ids.iter().filter_map(|id| doc.index_of(id)).collect();
    picked.sort_unstable();
    picked.dedup();
    let shapes = picked
        .iter()
        .flat_map(|&i| if doc.nodes[i].kind.is_container() { doc.combine_members(i) } else { vec![i] })
        .filter(|&i| doc.nodes[i].kind != ShapeKind::Bone && doc.is_visible(i))
        .count();
    if shapes < 2 {
        return Err("выделите хотя бы две фигуры".into());
    }
    if let Some(bone) = picked.iter().find(|&&i| doc.nodes[i].kind == ShapeKind::Bone) {
        return Err(format!("«{}» — кость: у неё нет контура", doc.nodes[*bone].id));
    }
    if let Some(&inside) = picked.iter().find(|&&i| doc.combined_in(i).is_some()) {
        let outer = doc.combined_in(inside).map_or("", |g| doc.nodes[g].id.as_str());
        if !picked.iter().all(|&i| doc.combined_in(i) == doc.combined_in(inside) && doc.nodes[i].parent == doc.nodes[inside].parent) {
            return Err(format!("«{}» уже часть операции «{outer}»: выделите фигуры одной группы", doc.nodes[inside].id));
        }
    }
    let (grouped, group) = group_nodes(src, scene, ids, base)?;
    let scene = reparse(&grouped, base)?;
    let out = set_style(&grouped, &scene, &group, "combine", Some(&format!("combine {}", op.keyword()))).ok_or("группа не меняется")?;
    // What cannot be in an operation (a soft body, particles) is said by the script itself.
    reparse(&out, base)?;
    Ok((out, group))
}

/// Returns the script with the operation of group `id` set to `op`, or taken off (`None`): the
/// group is then a plain one and every shape draws itself again. `None` for a group made by a
/// loop or for what is not a group.
pub fn set_combine(src: &str, scene: &Scene, id: &str, op: Option<CombineOp>) -> Option<String> {
    if scene.doc.node(id)?.kind != ShapeKind::Group {
        return None;
    }
    let text = op.map(|op| format!("combine {}", op.keyword()));
    set_style(src, scene, id, "combine", text.as_deref())
}

/// Returns the script with the `combine` group `id` replaced by one `path` holding the outline
/// it draws now, and that path's id — the group's own, so its keys go on playing. The path
/// stands where the group stood and takes the paint of the shape that drew the result. Refused,
/// in words, when taking apart would lose something: a shape that moves in a clip, keys of the
/// shapes inside.
pub fn flatten_combine(src: &str, scene: &Scene, id: &str, base: Option<&std::path::Path>) -> Result<(String, String), String> {
    let doc = &scene.doc;
    let g = doc.index_of(id).ok_or("нет такого объекта")?;
    let group = &doc.nodes[g];
    if group.combine.is_none() {
        return Err("у группы нет операции: разбирать нечего".into());
    }
    let site = scene.sites.get(id).filter(|s| !s.generated).ok_or("группа из цикла или символа: её правят в тексте")?;
    if scene.instances.contains_key(id) || !src[site.span.start..].starts_with("group") {
        return Err("это копия символа: операцию разбирают в самом символе".into());
    }
    if group.stack.is_some() || group.lattice.is_some() || group.ui || group.slice.is_some() {
        return Err("у группы раскладка, деформер или роль в интерфейсе: у контура их не бывает".into());
    }
    if let Some(mover) = doc.combine_moved_by(g) {
        return Err(format!("фигура «{mover}» двигается или меняет форму во время игры: после разбора контур застыл бы. Сначала уберите её ключи и связи"));
    }
    let inside: Vec<&str> = (0..doc.nodes.len()).filter(|&i| doc.is_inside(i, g)).map(|i| doc.nodes[i].id.as_str()).collect();
    let keyed = |node: &str| inside.contains(&node);
    if let Some(clip) = doc.animations.iter().find(|a| a.tracks.iter().any(|t| keyed(&t.node))) {
        return Err(format!("у фигур операции есть ключи в клипе «{}»: они пропали бы", clip.name));
    }
    if let Some(driver) = doc.drivers.iter().find(|d| d.tracks.iter().any(|t| keyed(&t.node))) {
        return Err(format!("фигуры операции связаны с параметром «{}»: связь пропала бы", driver.parameter));
    }
    if doc.correctives.iter().any(|c| c.tracks.iter().any(|t| keyed(&t.node))) {
        return Err("фигуры операции меняет коррекция: она пропала бы".into());
    }
    if let Some(other) = doc.nodes.iter().find(|n| !inside.contains(&n.id.as_str()) && n.morph.as_ref().is_some_and(|m| m.targets.iter().any(|t| keyed(t)))) {
        return Err(format!("в фигуру операции перетекает «{}»: цель пропала бы", other.id));
    }

    let bearer = doc.combine_bearer(g).ok_or("в группе нет видимых фигур")?;
    let drawn = &doc.nodes[bearer];
    let made = drawn.path().filter(|p| !drawn.absorbed && !p.elements().is_empty() && p.bounding_box().area() > 0.0);
    let made = made.ok_or("от фигур ничего не осталось: разбирать нечего")?;
    let to_group =
        if doc.world_transform(g).determinant().abs() < 1e-12 { Affine::IDENTITY } else { doc.world_transform(g).inverse() * doc.world_transform(bearer) };
    let outline = to_group * made;

    // The group's own line, as written, becomes the path's: its id, name, place and keys stay.
    let own = |key: &str| scene.prop_spans.get(&(id.to_string(), key.to_string())).copied();
    let theirs = |key: &str| scene.prop_spans.get(&(drawn.id.clone(), key.to_string())).copied();
    let combine = own("combine").ok_or("слово combine не найдено в строке группы")?;
    let head = Span::new(site.span.start + "group".len(), site.insert_at);
    let mut edits = vec![(Span::new(src[..combine.start].trim_end_matches([' ', '\t']).len().max(head.start), combine.end), String::new())];
    let mut tail = String::new();
    // Both see-through: the outline is as faint as the shape was inside its group.
    if (drawn.opacity - 1.0).abs() > 1e-9 {
        let faint = |o: f64| format!("opacity {}", crate::fmt_num((o.clamp(0.0, 1.0) * 1000.0).round() / 1000.0));
        match own("opacity") {
            Some(_) if doc.animations.iter().any(|a| a.tracks.iter().any(|t| t.node == id && t.prop == motif_core::Prop::Opacity)) => {
                return Err("прозрачность группы анимирована, а у нижней фигуры своя: уберите прозрачность фигуры".into());
            }
            Some(span) => edits.push((span, faint(drawn.opacity * group.opacity))),
            None => tail = format!(" {}", faint(drawn.opacity)),
        }
    }
    let mut header = src[head.start..head.end].to_string();
    edits.sort_by_key(|(span, _)| std::cmp::Reverse(span.start));
    for (span, text) in edits {
        header.replace_range(span.start - head.start..span.end.min(head.end) - head.start, &text);
    }
    let mut line = format!("path{header} d \"{}\"", path_to_d(&outline));
    let mut painted = false;
    for key in PAINT_KEYS {
        // What the group says of itself (a blur of the whole group) stays the group's.
        if own(key).is_some() {
            continue;
        }
        if let Some(span) = theirs(key) {
            line.push(' ');
            line.push_str(&src[span.start..span.end]);
            painted |= key == "fill" || key == "gradient";
        }
    }
    if !painted && drawn.fill.is_none() && drawn.gradient.is_none() {
        line.push_str(" fill none");
    }
    line.push_str(&tail);
    line.push_str(" evenodd");

    let block = whole_lines(src, site.span);
    let indent: String = src[block.start..].chars().take_while(|c| *c == ' ' || *c == '\t').collect();
    let ending = if src[block.start..block.end].ends_with('\n') { "\n" } else { "" };
    let out = splice(src, block, &format!("{indent}{line}{ending}"));
    // Whatever else pointed at a shape inside (a listener, a rope tied to it) is said here.
    reparse(&out, base).map_err(|e| format!("после разбора скрипт не читается: {e}"))?;
    Ok((out, id.to_string()))
}
