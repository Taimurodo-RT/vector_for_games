//! Source authoring for vector forms and explicit object appearance keys.
use crate::{Scene, parse_in, path_to_d, set_key, set_key_ease, set_style};
use motif_core::kurbo::BezPath;
use motif_core::{Prop, Value};
use std::path::Path;

pub fn set_contour_key(src: &str, scene: &Scene, clip: &str, node: &str, time: f64, path: &BezPath, base: Option<&Path>) -> Option<String> {
    let n = scene.doc.node(node)?;
    let mut shapes = n.forms.as_ref().map(|f| f.shapes.clone()).unwrap_or_default();
    let current = scene
        .doc
        .animation(clip)
        .and_then(|a| a.tracks.iter().find(|t| t.node == node && t.prop == Prop::Contour))
        .and_then(|t| t.keys.iter().find(|k| (k.time - time).abs() < 0.005))
        .and_then(|k| k.value.as_num())
        .map(|v| v as usize);
    let reusable = current.filter(|index| {
        *index > 0
            && *index <= shapes.len()
            && n.get(Prop::Contour) != Value::Num(*index as f64)
            && !scene.doc.drivers.iter().any(|d| d.tracks.iter().any(|t| t.node == node && t.prop == Prop::Contour))
            && scene
                .doc
                .animations
                .iter()
                .flat_map(|a| &a.tracks)
                .filter(|t| t.node == node && t.prop == Prop::Contour)
                .flat_map(|t| &t.keys)
                .filter(|k| k.value == Value::Num(*index as f64))
                .count()
                == 1
    });
    let index = if path == &n.geometry {
        0
    } else if let Some(i) = shapes.iter().position(|p| p == path) {
        i + 1
    } else if let Some(i) = reusable {
        shapes[i - 1] = path.clone();
        i
    } else {
        shapes.push(path.clone());
        shapes.len()
    };
    if shapes.is_empty() {
        return Some(src.into());
    }
    let mut out = src.to_string();
    if !shapes.is_empty() {
        let text = format!("forms {}", shapes.iter().map(|p| format!("\"{}\"", path_to_d(p))).collect::<Vec<_>>().join(" "));
        out = set_style(&out, scene, node, "forms", Some(&text))?;
    }
    let parsed = parse_in(&out, base).ok()?;
    set_key(&out, &parsed, clip, node, Prop::Contour, time, Value::Num(index as f64))
}

pub fn key_appearance(src: &str, scene: &Scene, clip: &str, node: &str, time: f64, base: Option<&Path>) -> Option<String> {
    if time <= 0.0 {
        return Some(src.into());
    }
    let mut out = set_key(src, scene, clip, node, Prop::Visible, 0.0, Value::Num(0.0))?;
    let parsed = parse_in(&out, base).ok()?;
    out = set_key(&out, &parsed, clip, node, Prop::Visible, time, Value::Num(1.0))?;
    let parsed = parse_in(&out, base).ok()?;
    set_key_ease(&out, &parsed, clip, node, Prop::Visible, 0.0, "hold")
}
