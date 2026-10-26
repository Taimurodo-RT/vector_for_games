//! Script edits for skeletons: new bones, IK, skins, and moving a shape into a bone so that it
//! follows it.

use crate::edit::{free_id, insert_declaration, whole_lines};
use crate::machine::apply;
use crate::{Scene, Span, fmt_num};
use motif_core::kurbo::{Affine, Point};
use motif_core::{Ik, Prop, ShapeKind, Skin, WeightStroke};

/// The spaces in front of the line that `at` is on.
fn indent_at(src: &str, at: usize) -> String {
    let start = src[..at].rfind('\n').map_or(0, |i| i + 1);
    src[start..].chars().take_while(|c| *c == ' ' || *c == '\t').collect()
}

/// Inserts `text` (whole lines, ending in a newline) as the last children of `parent`, opening
/// a block when it has none yet. `None` when the parent is made by a loop.
fn into_block(src: &str, scene: &Scene, parent: &str, text: &str) -> Option<(Span, String)> {
    let site = scene.sites.get(parent).filter(|s| !s.generated)?;
    let indent = indent_at(src, site.span.start);
    if src[..site.span.end].ends_with('}') {
        let close = site.span.end - 1;
        let line_start = src[..close].rfind('\n').map_or(0, |i| i + 1);
        return Some(if src[line_start..close].trim().is_empty() {
            (Span::new(line_start, line_start), text.to_string())
        } else {
            (Span::new(close, close), format!("\n{text}"))
        });
    }
    let at = site.span.end;
    Some((Span::new(at, at), format!(" {{\n{text}{indent}}}")))
}

/// `text` with its lines moved from `from` indentation to `to`.
fn reindent(text: &str, from: &str, to: &str) -> String {
    text.split_inclusive('\n').map(|l| format!("{to}{}", l.strip_prefix(from).unwrap_or(l.trim_start_matches([' ', '\t'])))).collect()
}

/// Returns the script with a new bone and its id. `at` is in the parent's space; with a parent
/// bone the new one is written inside its block, so it follows it.
pub fn add_bone(src: &str, scene: &Scene, parent: Option<&str>, at: (f64, f64), len: f64, rot: f64) -> Option<(String, String)> {
    let id = free_id(scene, "bone");
    let mut line = format!("bone {id} at {} {} len {}", fmt_num(at.0), fmt_num(at.1), fmt_num(len));
    if rot.abs() > 0.005 {
        line.push_str(&format!(" rot {}", fmt_num(rot)));
    }
    let Some(parent) = parent else {
        return Some((insert_declaration(src, scene, &format!("{line}\n")), id));
    };
    let site = scene.sites.get(parent)?;
    let indent = format!("{}  ", indent_at(src, site.span.start));
    let edit = into_block(src, scene, parent, &format!("{indent}{line}\n"))?;
    Some((apply(src, vec![edit]), id))
}

/// Writes the properties of `node` named by `items` at once: each key with its text, or `None`
/// to remove it. `None` when the node is made by a loop.
fn set_styles(src: &str, scene: &Scene, node: &str, items: &[(&str, Option<String>)]) -> Option<String> {
    let site = scene.sites.get(node).filter(|s| !s.generated)?;
    let mut edits = Vec::new();
    let mut added = String::new();
    for (key, text) in items {
        match (scene.prop_spans.get(&(node.to_string(), key.to_string())), text) {
            (Some(span), Some(t)) => edits.push((*span, t.clone())),
            (Some(span), None) => {
                let start = src[..span.start].trim_end_matches(' ').len();
                edits.push((Span::new(start, span.end), String::new()));
            }
            (None, Some(t)) => added.push_str(&format!(" {t}")),
            (None, None) => {}
        }
    }
    if !added.is_empty() {
        edits.push((Span::new(site.insert_at, site.insert_at), added));
    }
    Some(apply(src, edits))
}

/// Returns the script with the IK of `bone` set or removed.
pub fn set_ik(src: &str, scene: &Scene, bone: &str, ik: Option<&Ik>) -> Option<String> {
    if scene.doc.node(bone)?.kind != ShapeKind::Bone {
        return None;
    }
    let ik_text = ik.map(|ik| format!("ik {}", ik.target));
    let chain = ik.filter(|ik| ik.chain != 2).map(|ik| format!("chain {}", ik.chain));
    let flip = ik.filter(|ik| ik.flip).map(|_| "flip".to_string());
    set_styles(src, scene, bone, &[("ik", ik_text), ("chain", chain), ("flip", flip)])
}

/// Returns the script with a new empty group at the tip of `bone` to be its IK target, and the
/// group's id. The group is written at the top level: a target inside the bones would move with them.
pub fn add_ik_target(src: &str, scene: &Scene, bone: &str) -> Option<(String, String)> {
    let doc = &scene.doc;
    let i = doc.index_of(bone).filter(|&i| doc.nodes[i].kind == ShapeKind::Bone)?;
    let tip = doc.world_transform(i) * doc.nodes[i].bone_tip();
    let base = format!("{bone}_target");
    let id = if doc.node(&base).is_none() { base } else { free_id(scene, &format!("{base}_")) };
    let line = format!("group {id} \"Цель {bone}\" at {} {}\n", fmt_num((tip.x * 10.0).round() / 10.0), fmt_num((tip.y * 10.0).round() / 10.0));
    Some((insert_declaration(src, scene, &line), id))
}

/// Returns the script with the bones that bend `node` set, or the skin removed.
pub fn set_skin(src: &str, scene: &Scene, node: &str, skin: Option<&Skin>) -> Option<String> {
    if scene.doc.node(node)?.kind.is_container() {
        return None;
    }
    let bones = skin.filter(|s| !s.bones.is_empty());
    let list = bones.map(|s| format!("skin {}", s.bones.join(" ")));
    let sharp = bones.filter(|s| (s.sharp - Skin::SHARP).abs() > 1e-9).map(|s| format!("skin-sharp {}", fmt_num(s.sharp)));
    set_styles(src, scene, node, &[("skin", list), ("skin-sharp", sharp)])
}

/// The script line of one weight-brush stroke, without indent or line break.
pub fn fmt_stroke(stroke: &WeightStroke) -> String {
    let mut line = format!("{} {} {} at", stroke.bone, fmt_num(stroke.radius), fmt_num(stroke.amount));
    for p in &stroke.points {
        line.push_str(&format!(" {} {}", fmt_num((p.x * 10.0).round() / 10.0), fmt_num((p.y * 10.0).round() / 10.0)));
    }
    line
}

/// Returns the script with `stroke` painted over the weights of the skinned shape `node`: the
/// last line of its `weights` block, or a new block at the end of the script.
pub fn add_weight_stroke(src: &str, scene: &Scene, node: &str, stroke: &WeightStroke) -> Option<String> {
    let skin = scene.doc.node(node)?.skin.as_ref()?;
    if !skin.bones.contains(&stroke.bone) || stroke.points.is_empty() {
        return None;
    }
    let line = fmt_stroke(stroke);
    let Some(site) = scene.weights.get(node) else {
        let mut out = src.to_string();
        if !out.is_empty() && !out.ends_with('\n') {
            out.push('\n');
        }
        out.push_str(&format!("\nweights {node} {{\n  {line}\n}}\n"));
        return Some(out);
    };
    let line_start = src[..site.close].rfind('\n').map_or(0, |i| i + 1);
    Some(if src[line_start..site.close].trim().is_empty() {
        apply(src, vec![(Span::new(line_start, line_start), format!("  {line}\n"))])
    } else {
        apply(src, vec![(Span::new(site.close, site.close), format!("\n  {line}\n"))])
    })
}

/// Returns the script without the painted strokes of `bone` on `node`, or without all of them
/// (the whole `weights` block) when `bone` is `None`. `None` when there is nothing to remove.
pub fn clear_weights(src: &str, scene: &Scene, node: &str, bone: Option<&str>) -> Option<String> {
    let site = scene.weights.get(node)?;
    let skin = scene.doc.node(node)?.skin.as_ref()?;
    let spans: Vec<Span> = match bone {
        Some(b) if skin.paint.iter().any(|s| s.bone != b) => {
            skin.paint.iter().zip(&site.strokes).filter(|(s, _)| s.bone == b).map(|(_, sp)| whole_lines(src, *sp)).collect()
        }
        Some(b) if !skin.paint.iter().any(|s| s.bone == b) => return None,
        _ => vec![whole_lines(src, site.span)],
    };
    if spans.is_empty() {
        return None;
    }
    let mut out = crate::edit::remove_spans(src, spans);
    // A block removed from the end leaves the blank line written before it.
    while out.ends_with("\n\n") {
        out.pop();
    }
    Some(out)
}

/// Direction of the x axis of `a`, in degrees.
fn angle(a: Affine) -> f64 {
    let v = a * Point::new(1.0, 0.0) - a * Point::ZERO;
    v.y.atan2(v.x).to_degrees()
}

/// Returns the script with `node` moved into the block of `bone`, or out to the top level with
/// `None`, keeping where it is on the artboard. `None` when that is not possible: a node made by
/// a loop, a position written as an expression, or a bone inside the node itself.
pub fn attach_to_bone(src: &str, scene: &Scene, node: &str, bone: Option<&str>) -> Option<String> {
    let doc = &scene.doc;
    let i = doc.index_of(node)?;
    let site = scene.sites.get(node).filter(|s| !s.generated)?;
    let new_parent = match bone {
        Some(b) => {
            let b = doc.index_of(b).filter(|&b| doc.nodes[b].kind == ShapeKind::Bone)?;
            let mut up = Some(b);
            while let Some(k) = up {
                if k == i {
                    return None;
                }
                up = doc.nodes[k].parent;
            }
            Some(b)
        }
        None => None,
    };
    if doc.nodes[i].parent == new_parent {
        return Some(src.to_string());
    }
    // The same place on the artboard, in the new parent's space.
    let n = &doc.nodes[i];
    let parent_world = new_parent.map_or(Affine::IDENTITY, |p| doc.world_transform(p));
    let old_parent = n.parent.map_or(Affine::IDENTITY, |p| doc.world_transform(p));
    let pivot = Point::ZERO + n.pivot;
    let at = parent_world.inverse() * (doc.world_transform(i) * pivot) - n.pivot;
    let round = |v: f64, k: f64| (v * k).round() / k;
    let rot = round(n.rotation + angle(old_parent) - angle(parent_world), 100.0);
    let rot = (rot + 180.0).rem_euclid(360.0) - 180.0;

    let whole = whole_lines(src, site.span);
    let text = &src[whole.start..whole.end];
    let local = |s: Span| Span::new(s.start - whole.start, s.end - whole.start);
    let mut edits = Vec::new();
    let pos = (scene.sources.get(&(node.to_string(), Prop::X)), scene.sources.get(&(node.to_string(), Prop::Y)));
    let in_line = |s: &crate::Source| s.editable && !s.from_override && s.span.start >= whole.start && s.span.end <= whole.end;
    match pos {
        (Some(x), Some(y)) if in_line(x) && in_line(y) => {
            edits.push((local(x.span), fmt_num(round(at.x, 10.0))));
            edits.push((local(y.span), fmt_num(round(at.y, 10.0))));
        }
        (None, None) => {
            edits.push((local(Span::new(site.insert_at, site.insert_at)), format!(" at {} {}", fmt_num(round(at.x, 10.0)), fmt_num(round(at.y, 10.0)))))
        }
        _ => return None,
    }
    match scene.sources.get(&(node.to_string(), Prop::Rotation)) {
        Some(r) if in_line(r) => edits.push((local(r.span), fmt_num(rot))),
        Some(_) => return None,
        None if rot.abs() > 0.005 => edits.push((local(Span::new(site.insert_at, site.insert_at)), format!(" rot {}", fmt_num(rot)))),
        None => {}
    }
    let moved = apply(text, edits);
    let from = indent_at(src, site.span.start);
    let mut out_edits = vec![(whole, String::new())];
    match new_parent {
        Some(b) => {
            let bone_id = &doc.nodes[b].id;
            let to = format!("{}  ", indent_at(src, scene.sites.get(bone_id)?.span.start));
            out_edits.push(into_block(src, scene, bone_id, &reindent(&moved, &from, &to))?);
        }
        None => {
            let moved = reindent(&moved, &from, "");
            let at = scene.blocks_start.map_or(src.len(), |at| src[..at].rfind('\n').map_or(0, |i| i + 1));
            let sep = if at == src.len() && !src.ends_with('\n') { "\n" } else { "" };
            out_edits.push((Span::new(at, at), format!("{sep}{moved}")));
        }
    }
    Some(apply(src, out_edits))
}
