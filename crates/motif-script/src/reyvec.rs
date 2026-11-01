//! Morphing shapes for the ReyVec engine (`motif morph`): a plain JSON file, so that the engine
//! does not depend on Motif's code. It follows the engine's contract (`docs/motif-morph.md` in
//! ReyVec): each shape's key contours in order, its fill rule and colour, the artist's marks
//! (`match`), and the morph number of each animation, frame by frame.
//!
//! What the engine cannot show comes back as warnings, not in the file: strokes (only fills
//! morph there), colours that change between contours (the engine keeps one colour), gradients.

use motif_core::kurbo::{Affine, BezPath};
use motif_core::{Document, IdKind, MatchEnd, Prop, Value};
use serde::{Deserialize, Serialize};

/// The name of the format, the first field of the file.
pub const REYVEC_MORPH_FORMAT: &str = "reyvec-morph";
/// The version of the file's layout: it grows when a field changes its meaning.
pub const REYVEC_MORPH_VERSION: u32 = 1;
/// How many morph numbers a second of animation gets.
pub const REYVEC_MORPH_FPS: f64 = 60.0;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MorphFile {
    /// Always `reyvec-morph`.
    pub format: String,
    pub version: u32,
    /// Which way y points in the contours: `up`, as in the engine.
    pub y: String,
    /// Morph numbers per second in each animation.
    pub fps: f64,
    pub shapes: Vec<MorphShape>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MorphShape {
    /// The board (artboard) it is on.
    pub board: String,
    /// The shape's id in the scene.
    pub id: String,
    /// `nonzero` or `evenodd`, the same for every contour.
    pub fill_rule: String,
    /// sRGB `#rrggbb`.
    pub color: String,
    /// 0 transparent .. 1 solid.
    pub alpha: f64,
    /// The key contours in order, at least two, as SVG `d` in the shape's own units around its
    /// origin, y up. Contour 0 is the shape as drawn.
    pub contours: Vec<String>,
    /// The morph number when no animation plays: 0 the first contour, 1.5 between the second
    /// and the third.
    pub morph: f64,
    /// The artist's marks between neighbouring contours; empty when the matching is left to the
    /// engine.
    pub marks: Vec<Mark>,
    /// Every animation that moves this shape's morph number.
    pub animations: Vec<MorphTrack>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Mark {
    /// Between contour `step` and `step + 1`.
    pub step: usize,
    /// On contour `step`; missing: the piece on the other side grows from a point.
    pub from: Option<Place>,
    /// On contour `step + 1`; missing: the piece on this side shrinks into a point.
    pub to: Option<Place>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Place {
    /// The piece of the contour, from 0, one for each `M`.
    pub piece: usize,
    /// A share of the piece's length from its start, 0..1. Missing on a pair of whole pieces,
    /// and on a growing piece it means its middle.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub at: Option<f64>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MorphTrack {
    /// The hidden id (`id=…`) of the animation, missing until Motif saves the file.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    pub animation: String,
    /// Seconds.
    pub duration: f64,
    #[serde(rename = "loop")]
    pub looping: bool,
    /// The morph number at 0, 1/fps, 2/fps … up to the end, its easing already applied.
    pub morph: Vec<f64>,
}

/// The morphing shapes of these boards for ReyVec, and what the engine will show differently.
pub fn reyvec_morph<'a>(boards: impl IntoIterator<Item = &'a Document>) -> (MorphFile, Vec<String>) {
    let (shapes, warnings) = gather(boards, true);
    let file = MorphFile { format: REYVEC_MORPH_FORMAT.into(), version: REYVEC_MORPH_VERSION, y: "up".into(), fps: REYVEC_MORPH_FPS, shapes };
    (file, warnings)
}

/// Only what the engine would show differently: cheap enough to ask every frame.
pub fn reyvec_morph_warnings<'a>(boards: impl IntoIterator<Item = &'a Document>) -> Vec<String> {
    gather(boards, false).1
}

fn gather<'a>(boards: impl IntoIterator<Item = &'a Document>, frames: bool) -> (Vec<MorphShape>, Vec<String>) {
    let (mut shapes, mut warnings) = (Vec::new(), Vec::new());
    for doc in boards {
        for (i, n) in doc.nodes.iter().enumerate() {
            if n.morph.is_none() && n.forms.is_none() {
                continue;
            }
            let keys = doc.key_contours(i);
            let name = format!("«{}»", n.id);
            if keys.len() < 2 {
                warnings.push(format!("{name}: у перетекающей фигуры нужно хотя бы два контура — пропущена"));
                continue;
            }
            if keys.iter().all(|k| motif_core::kurbo::Shape::area(k).abs() < 1e-6) {
                warnings.push(format!("{name}: это линии без площади, а движок перетекает только заливки — пропущена"));
                continue;
            }
            let Some(fill) = n.fill.or_else(|| n.gradient.as_ref().and_then(|g| g.colors.first().copied())) else {
                warnings.push(format!("{name}: нет заливки, а движок перетекает только заливки — пропущена"));
                continue;
            };
            if n.gradient.is_some() {
                warnings.push(format!("{name}: градиент движку пока не передаётся — взят его первый цвет {}", hex(fill)));
            }
            if n.stroke.is_some() && n.stroke_width > 0.0 {
                warnings.push(format!("{name}: обводка в движке не перетекает и не будет видна — нарисуйте её отдельной заливкой-кольцом"));
            }
            if let Some(m) = &n.morph {
                if n.forms.is_some() {
                    warnings.push(format!("{name}: у фигуры и morph, и forms — движку уходят цели morph"));
                }
                for t in m.targets.iter().filter_map(|id| doc.node(id)) {
                    if t.fill.is_some_and(|c| c != fill) {
                        warnings.push(format!(
                            "{name}: у цели «{}» другой цвет — движок цвет не смешивает, останется {}; цвет можно вести оттенком",
                            t.id,
                            hex(fill)
                        ));
                    }
                }
            }
            let up = Affine::FLIP_Y;
            let contours = keys.iter().map(|k| crate::path_to_d(&rounded(&(up * k.clone())))).collect();
            let last = (keys.len() - 1) as f64;
            let prop = if n.morph.is_some() { Prop::Morph } else { Prop::Contour };
            let rest = n.morph.as_ref().map(|m| m.amount).or(n.forms.as_ref().map(|f| f.amount)).unwrap_or(0.0).clamp(0.0, last);
            let animations = doc
                .animations
                .iter()
                .filter(|a| frames && a.tracks.iter().any(|t| t.node == n.id && t.prop == prop))
                .map(|a| {
                    let frames = (a.duration * REYVEC_MORPH_FPS).round().max(0.0) as usize;
                    let morph = (0..=frames)
                        .map(|f| {
                            let t = (f as f64 / REYVEC_MORPH_FPS).min(a.duration);
                            let v = match a.keys_at(doc, t).node(&n.id).map(|n| n.get(prop)) {
                                Some(Value::Num(v)) => v,
                                _ => rest,
                            };
                            round(v.clamp(0.0, last))
                        })
                        .collect();
                    MorphTrack {
                        id: doc.ids.get(IdKind::Animation, "", &a.name).map(str::to_string),
                        animation: a.name.clone(),
                        duration: a.duration,
                        looping: a.looping,
                        morph,
                    }
                })
                .collect();
            let place = |e: Option<MatchEnd>| e.map(|e| Place { piece: e.piece, at: e.at });
            shapes.push(MorphShape {
                board: doc.artboard.name.clone(),
                id: n.id.clone(),
                fill_rule: if n.even_odd { "evenodd" } else { "nonzero" }.into(),
                color: format!("#{:02x}{:02x}{:02x}", fill.r, fill.g, fill.b),
                alpha: round(fill.a as f64 / 255.0),
                contours,
                morph: round(rest),
                marks: n.morph_links.iter().map(|l| Mark { step: l.key, from: place(l.from), to: place(l.to) }).collect(),
                animations,
            });
        }
    }
    (shapes, warnings)
}

/// The file as JSON text, and the warnings.
pub fn reyvec_morph_json<'a>(boards: impl IntoIterator<Item = &'a Document>) -> (String, Vec<String>) {
    let (file, warnings) = reyvec_morph(boards);
    (file.to_json(), warnings)
}

impl MorphFile {
    /// As JSON text, each list of numbers on one line.
    pub fn to_json(&self) -> String {
        compact_numbers(&serde_json::to_string_pretty(self).expect("a morph file is plain data"))
    }
}

/// Puts each list of plain numbers on one line, so that a long animation stays readable.
fn compact_numbers(pretty: &str) -> String {
    let lines: Vec<&str> = pretty.lines().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        if line.ends_with('[') {
            let end = (i + 1..lines.len()).find(|&j| !lines[j].trim().trim_end_matches(',').parse::<f64>().is_ok());
            if let Some(end) = end.filter(|&j| j > i + 1 && lines[j].trim().starts_with(']')) {
                let nums: Vec<&str> = lines[i + 1..end].iter().map(|l| l.trim().trim_end_matches(',')).collect();
                out.push_str(&format!("{line}{}{}\n", nums.join(", "), lines[end].trim()));
                i = end + 1;
                continue;
            }
        }
        out.push_str(line);
        out.push('\n');
        i += 1;
    }
    out
}

fn hex(c: motif_core::Color) -> String {
    format!("#{:02x}{:02x}{:02x}", c.r, c.g, c.b)
}

fn round(v: f64) -> f64 {
    let r = (v * 10000.0).round() / 10000.0;
    if r == 0.0 { 0.0 } else { r }
}

/// The path with every coordinate to four decimals, so the file stays readable and stable.
fn rounded(path: &BezPath) -> BezPath {
    use motif_core::kurbo::{PathEl, Point};
    let p = |q: Point| Point::new(round(q.x), round(q.y));
    BezPath::from_vec(
        path.elements()
            .iter()
            .map(|el| match *el {
                PathEl::MoveTo(a) => PathEl::MoveTo(p(a)),
                PathEl::LineTo(a) => PathEl::LineTo(p(a)),
                PathEl::QuadTo(a, b) => PathEl::QuadTo(p(a), p(b)),
                PathEl::CurveTo(a, b, c) => PathEl::CurveTo(p(a), p(b), p(c)),
                PathEl::ClosePath => PathEl::ClosePath,
            })
            .collect(),
    )
}
