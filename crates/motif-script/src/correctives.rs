//! Two-parameter residual warp forms. Tables contain parameter coordinates, never seconds.
use crate::parser::{Stmt, ValueKind};
use crate::{Scene, ScriptError, Span};
use motif_core::{CorrectiveKey, CorrectiveTrack, PoseCorrective, Prop};
use std::collections::HashSet;

#[derive(Clone, Debug, PartialEq)]
pub struct CorrectiveKeySite {
    pub x: f64,
    pub y: f64,
    pub value: Span,
}
#[derive(Clone, Debug, PartialEq)]
pub struct CorrectiveTrackSite {
    pub node: String,
    pub prop: Prop,
    pub span: Span,
    pub keys: Vec<CorrectiveKeySite>,
}
#[derive(Clone, Debug, PartialEq)]
pub struct CorrectiveSite {
    pub span: Span,
    pub close: usize,
    pub tracks: Vec<CorrectiveTrackSite>,
}

pub(crate) fn evaluate(src: &str, scene: &mut Scene, stmts: &[Stmt]) -> Vec<ScriptError> {
    let mut errors = Vec::new();
    let mut error = |span: Span, text: &str| errors.push(ScriptError::at(src, span.start, text));
    for stmt in stmts {
        let Stmt::Corrective { id, parameters, tracks, span, close } = stmt else { continue };
        if span.start < crate::head_end(src) {
            error(*span, "corrective принадлежит одному холсту и пишется после artboard");
            continue;
        }
        scene.blocks_start = Some(scene.blocks_start.map_or(span.start, |s| s.min(span.start)));
        if scene.corrective_sites.contains_key(id) {
            error(*span, "повторяется имя corrective");
            continue;
        }
        if parameters[0] == parameters[1] {
            error(*span, "corrective требует два разных параметра");
            continue;
        }
        let (Some(x), Some(y)) = (scene.doc.parameter(&parameters[0]).cloned(), scene.doc.parameter(&parameters[1]).cloned()) else {
            error(*span, "corrective ссылается на неизвестный параметр");
            continue;
        };
        let mut result = PoseCorrective { id: id.clone(), parameters: parameters.clone(), tracks: Vec::new() };
        let mut site = CorrectiveSite { span: *span, close: *close, tracks: Vec::new() };
        let mut targets = HashSet::new();
        for track in tracks {
            let Some(prop) = Prop::from_path_name(&track.prop).filter(|p| matches!(p, Prop::WarpX(_) | Prop::WarpY(_))) else {
                error(track.span, "corrective изменяет только смещения сетки warp-x0…8 и warp-y0…8");
                continue;
            };
            if scene.doc.node(&track.node).is_none_or(|n| n.lattice.is_none()) {
                error(track.span, "цель corrective должна иметь сетку deform");
                continue;
            }
            if !targets.insert((&track.node, prop)) {
                error(track.span, "повторяется свойство в corrective");
                continue;
            }
            if track.keys.len() > 4096 {
                error(track.span, "слишком большая таблица corrective (максимум 4096 опор)");
                continue;
            }
            let mut keys = Vec::new();
            let mut key_sites = Vec::new();
            for key in &track.keys {
                let ValueKind::Num(value) = key.value.kind else {
                    error(key.value.span, "смещение опоры corrective должно быть обычным числом");
                    continue;
                };
                if ![key.x, key.y, value].into_iter().all(f64::is_finite)
                    || value.abs() > 1e6
                    || !(x.min..=x.max).contains(&key.x)
                    || !(y.min..=y.max).contains(&key.y)
                {
                    error(key.span, "опоры corrective должны находиться в диапазонах параметров; смещение должно быть конечным и не больше 1000000");
                    continue;
                }
                keys.push(CorrectiveKey { x: key.x, y: key.y, value });
                key_sites.push(CorrectiveKeySite { x: key.x, y: key.y, value: key.value.span });
            }
            keys.sort_by(|a, b| a.x.total_cmp(&b.x).then(a.y.total_cmp(&b.y)));
            let parsed = CorrectiveTrack { node: track.node.clone(), prop, keys };
            let (xs, ys) = parsed.axes();
            if xs.len() < 2
                || ys.len() < 2
                || xs.first() != Some(&x.min)
                || xs.last() != Some(&x.max)
                || ys.first() != Some(&y.min)
                || ys.last() != Some(&y.max)
                || parsed.keys.len() != xs.len() * ys.len()
                || parsed.keys.windows(2).any(|w| w[0].x == w[1].x && w[0].y == w[1].y)
            {
                error(track.span, "таблица corrective должна содержать все пересечения прямоугольной сетки без повторов, включая границы обоих параметров");
            }
            result.tracks.push(parsed);
            site.tracks.push(CorrectiveTrackSite { node: track.node.clone(), prop, span: track.span, keys: key_sites });
        }
        scene.doc.correctives.push(result);
        scene.corrective_sites.insert(id.clone(), site);
    }
    errors
}

fn word(id: &str) -> bool {
    id.chars().next().is_some_and(|c| c.is_alphabetic() || c == '_') && id.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '-')
}

/// Add all 18 residual warp tracks for a group's min/default/max combinations.
/// New forms are neutral; editing them does not rewrite the ordinary parameter drivers.
pub fn add_lattice(src: &str, scene: &Scene, id: &str, node: &str, parameters: [&str; 2]) -> Option<String> {
    if !word(id) || scene.corrective_sites.contains_key(id) || parameters[0] == parameters[1] || scene.doc.node(node)?.lattice.is_none() {
        return None;
    }
    let axis = |id: &str| {
        let p = scene.doc.parameter(id)?;
        let mut a = vec![p.min, p.default, p.max];
        a.sort_by(f64::total_cmp);
        a.dedup();
        Some(a)
    };
    let (xs, ys) = (axis(parameters[0])?, axis(parameters[1])?);
    let mut out = format!("{}\ncorrective {id} parameters {} {} {{\n", src.trim_end(), parameters[0], parameters[1]);
    for i in 0..9 {
        for prop in [Prop::WarpX(i), Prop::WarpY(i)] {
            let keys = xs.iter().flat_map(|x| ys.iter().map(move |y| format!("{} {}: 0", x, y))).collect::<Vec<_>>().join(" | ");
            out.push_str(&format!("  {node}.{} {keys}\n", prop.path_name()));
        }
    }
    out.push_str("}\n");
    Some(out)
}

/// Set a residual at parameter coordinates, refining the track's grid when needed.
/// New intersections sample the old bilinear response before the requested anchor is changed.
/// A missing warp coordinate starts neutral on the target's grid (or the first grid for a new
/// target). Other tracks and the source's animation clips are left intact.
#[allow(clippy::too_many_arguments)]
pub fn set_value(src: &str, scene: &Scene, id: &str, node: &str, prop: Prop, x: f64, y: f64, value: f64) -> Option<String> {
    if !value.is_finite() || value.abs() > 1e6 || !matches!(prop,Prop::WarpX(i) | Prop::WarpY(i) if i<9) || scene.doc.node(node)?.lattice.is_none() {
        return None;
    }
    let site = scene.corrective_sites.get(id)?;
    let corrective = scene.doc.correctives.iter().find(|c| c.id == id)?;
    let px = scene.doc.parameter(&corrective.parameters[0])?;
    let py = scene.doc.parameter(&corrective.parameters[1])?;
    if !(px.min..=px.max).contains(&x) || !(py.min..=py.max).contains(&y) {
        return None;
    }
    let existing_site = site.tracks.iter().find(|t| t.node == node && t.prop == prop);
    if let Some(key) = existing_site.and_then(|t| t.keys.iter().find(|k| k.x == x && k.y == y)) {
        return Some(crate::edit::splice(src, key.value, &crate::fmt_num(value)));
    }
    let existing = corrective.tracks.iter().find(|t| t.node == node && t.prop == prop);
    let template = existing.or_else(|| corrective.tracks.iter().find(|t| t.node == node)).or_else(|| corrective.tracks.first())?;
    let (mut xs, mut ys) = template.axes();
    xs.push(x);
    ys.push(y);
    xs.sort_by(f64::total_cmp);
    ys.sort_by(f64::total_cmp);
    xs.dedup();
    ys.dedup();
    if xs.len() * ys.len() > 4096 {
        return None;
    }
    let keys = xs
        .iter()
        .flat_map(|gx| {
            ys.iter().map(move |gy| {
                let v = if *gx == x && *gy == y { crate::fmt_num(value) } else { existing.map_or(0.0, |t| t.sample(*gx, *gy)).to_string() };
                format!("{gx} {gy}: {v}")
            })
        })
        .collect::<Vec<_>>()
        .join(" | ");
    let line = format!("{node}.{} {keys}", prop.path_name());
    if let Some(track) = existing_site {
        return Some(crate::edit::splice(src, track.span, &line));
    }
    let line = format!("  {line}\n");
    let start = src[..site.close].rfind('\n').map_or(0, |i| i + 1);
    let (at, line) = if src[start..site.close].trim().is_empty() { (start, line) } else { (site.close, format!("\n{line}")) };
    Some(crate::edit::splice(src, Span::new(at, at), &line))
}

pub fn remove(src: &str, scene: &Scene, id: &str) -> Option<String> {
    let span = scene.corrective_sites.get(id)?.span;
    Some(crate::edit::splice(src, span, ""))
}

#[cfg(test)]
mod tests;
