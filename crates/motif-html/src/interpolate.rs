//! Interpolate a single supported CSS property without replacing unrelated declarations.
use crate::style::{Length, Style, TransformOp};
use motif_core::{Color, Value};

pub(crate) const PROPERTIES: &[&str] = &[
    "color",
    "background-color",
    "border-color",
    "opacity",
    "width",
    "height",
    "top",
    "right",
    "bottom",
    "left",
    "transform",
    "transform-origin",
    "border-width",
    "border-radius",
    "font-size",
    "line-height",
    "letter-spacing",
    "gap",
    "padding",
    "padding-top",
    "padding-right",
    "padding-bottom",
    "padding-left",
    "margin",
    "margin-top",
    "margin-right",
    "margin-bottom",
    "margin-left",
    "box-shadow",
];

fn number(a: f64, b: f64, k: f64) -> f64 {
    a + (b - a) * k
}
fn length(a: Length, b: Length, k: f64) -> Length {
    Length { px: number(a.px, b.px, k), percent: number(a.percent, b.percent, k) }
}
fn paint(a: &str, b: &str, k: f64) -> Option<String> {
    if k == 0.0 {
        return Some(a.to_owned());
    }
    if k == 1.0 {
        return Some(b.to_owned());
    }
    let value = |s: &str| {
        if s == "none" { Some(Value::None) } else { Color::from_hex(s).map(|c| if c.a == 0 { Value::None } else { Value::Color(c) }) }
    };
    match value(a)?.lerp(value(b)?, k) {
        Value::None => Some("none".into()),
        Value::Color(c) => Some(c.to_hex()),
        _ => None,
    }
}
fn option_length(a: Option<Length>, b: Option<Length>, k: f64) -> Option<Option<Length>> {
    match (a, b) {
        (Some(a), Some(b)) => Some(Some(length(a, b, k))),
        (None, None) => Some(None),
        _ => None, // CSS auto is discrete, not an invented zero offset.
    }
}
fn size(a: (Option<f64>, Option<f64>), b: (Option<f64>, Option<f64>), k: f64) -> Option<(Option<f64>, Option<f64>)> {
    match (a, b) {
        ((Some(a), None), (Some(b), None)) => Some((Some(number(a, b, k)), None)),
        ((None, Some(a)), (None, Some(b))) => Some((None, Some(number(a, b, k)))),
        ((None, None), (None, None)) => Some((None, None)),
        _ => None, // Mixed px/% sizes need CSS calc, which Style does not represent.
    }
}
type Shadow = (f64, f64, f64, String);
fn shadow(a: &Option<Shadow>, b: &Option<Shadow>, k: f64) -> Option<Option<Shadow>> {
    if k == 0.0 {
        return Some(a.clone());
    }
    if k == 1.0 {
        return Some(b.clone());
    }
    if a.is_none() && b.is_none() {
        return Some(None);
    }
    let transparent = (0.0, 0.0, 0.0, "none".to_owned());
    let a = a.as_ref().unwrap_or(&transparent);
    let b = b.as_ref().unwrap_or(&transparent);
    Some(Some((number(a.0, b.0, k), number(a.1, b.1, k), number(a.2, b.2, k), paint(&a.3, &b.3, k)?)))
}

fn identity(op: &TransformOp) -> TransformOp {
    match op {
        TransformOp::Translate(_, _) => TransformOp::Translate(Length::default(), Length::default()),
        TransformOp::Scale(_, _) => TransformOp::Scale(1.0, 1.0),
        TransformOp::Rotate(_) => TransformOp::Rotate(0.0),
        TransformOp::Skew(_, _) => TransformOp::Skew(0.0, 0.0),
        TransformOp::Matrix(_) => TransformOp::Matrix([1.0, 0.0, 0.0, 1.0, 0.0, 0.0]),
    }
}
fn same_operation(a: &TransformOp, b: &TransformOp, k: f64) -> Option<TransformOp> {
    Some(match (a, b) {
        (TransformOp::Translate(ax, ay), TransformOp::Translate(bx, by)) => TransformOp::Translate(length(*ax, *bx, k), length(*ay, *by, k)),
        (TransformOp::Scale(ax, ay), TransformOp::Scale(bx, by)) => TransformOp::Scale(number(*ax, *bx, k), number(*ay, *by, k)),
        (TransformOp::Rotate(a), TransformOp::Rotate(b)) => TransformOp::Rotate(number(*a, *b, k)),
        (TransformOp::Skew(ax, ay), TransformOp::Skew(bx, by)) => TransformOp::Skew(number(*ax, *bx, k), number(*ay, *by, k)),
        _ => return None,
    })
}

#[derive(Clone, Copy)]
struct Decomposed {
    rotation: f64,
    sx: f64,
    sy: f64,
    shear: f64,
    x: f64,
    y: f64,
}
fn matrix(ops: &[TransformOp]) -> Option<[f64; 6]> {
    // Different operations can mix the two reference axes. Without a box size, a
    // rotated percentage translation cannot be represented by a fixed Matrix.
    if ops.iter().any(|op| matches!(op,TransformOp::Translate(x,y) if x.percent!=0.0 || y.percent!=0.0)) {
        return None;
    }
    let mut style = Style::default();
    style.transforms = ops.to_vec();
    let values = style.transform_matrix(0.0, 0.0).as_coeffs();
    values.iter().all(|v| v.is_finite()).then_some(values)
}
fn decompose([a, b, c, d, x, y]: [f64; 6]) -> Option<Decomposed> {
    let sx = a.hypot(b);
    if sx < 1e-10 {
        return None;
    }
    let rotation = b.atan2(a);
    let sy = (a * d - b * c) / sx;
    let shear = (a * c + b * d) / sx;
    Some(Decomposed { rotation, sx, sy, shear, x, y })
}
fn matrix_mix(a: &[TransformOp], b: &[TransformOp], k: f64) -> Option<TransformOp> {
    let a = decompose(matrix(a)?)?;
    let mut b = decompose(matrix(b)?)?;
    while b.rotation - a.rotation > std::f64::consts::PI {
        b.rotation -= std::f64::consts::TAU;
    }
    while b.rotation - a.rotation < -std::f64::consts::PI {
        b.rotation += std::f64::consts::TAU;
    }
    let rotation = number(a.rotation, b.rotation, k);
    let sx = number(a.sx, b.sx, k);
    let sy = number(a.sy, b.sy, k);
    let shear = number(a.shear, b.shear, k);
    let (sin, cos) = rotation.sin_cos();
    Some(TransformOp::Matrix([cos * sx, sin * sx, cos * shear - sin * sy, sin * shear + cos * sy, number(a.x, b.x, k), number(a.y, b.y, k)]))
}
fn transforms(a: &[TransformOp], b: &[TransformOp], k: f64) -> Option<Vec<TransformOp>> {
    if k == 0.0 {
        return Some(a.to_vec());
    }
    if k == 1.0 {
        return Some(b.to_vec());
    }
    let mut out = Vec::new();
    for index in 0..a.len().max(b.len()) {
        let left = a.get(index).cloned().unwrap_or_else(|| identity(&b[index]));
        let right = b.get(index).cloned().unwrap_or_else(|| identity(&a[index]));
        if let Some(op) = same_operation(&left, &right, k) {
            out.push(op);
        } else {
            out.push(matrix_mix(&a[index.min(a.len())..], &b[index.min(b.len())..], k)?);
            break;
        }
    }
    Some(out)
}

/// Returns false when this property cannot be represented as a continuous Motif
/// style change. The destination stays untouched in that case.
pub(crate) fn property(property: &str, into: &mut Style, from: &Style, to: &Style, k: f64) -> bool {
    if !k.is_finite() {
        return false;
    }
    let k = k.clamp(0.0, 1.0);
    apply(property, into, from, to, k).is_some()
}
fn apply(property: &str, into: &mut Style, from: &Style, to: &Style, k: f64) -> Option<()> {
    macro_rules! numeric {
        ($field:ident) => {
            into.$field = number(from.$field, to.$field, k)
        };
    }
    match property {
        "color" => into.color = paint(&from.color, &to.color, k)?,
        "background-color" => into.background = paint(&from.background, &to.background, k)?,
        "border-color" => into.border_color = paint(&from.border_color, &to.border_color, k)?,
        "opacity" => numeric!(opacity),
        "width" => {
            let (px, percent) = size((from.width, from.width_percent), (to.width, to.width_percent), k)?;
            into.width = px;
            into.width_percent = percent;
        }
        "height" => {
            let (px, percent) = size((from.height, from.height_percent), (to.height, to.height_percent), k)?;
            into.height = px;
            into.height_percent = percent;
        }
        "top" | "right" | "bottom" | "left" => {
            let index = match property {
                "top" => 0,
                "right" => 1,
                "bottom" => 2,
                _ => 3,
            };
            into.offsets[index] = option_length(from.offsets[index], to.offsets[index], k)?;
        }
        "transform" => into.transforms = transforms(&from.transforms, &to.transforms, k)?,
        "transform-origin" => {
            into.transform_origin = [length(from.transform_origin[0], to.transform_origin[0], k), length(from.transform_origin[1], to.transform_origin[1], k)]
        }
        "border-width" => numeric!(border_width),
        "border-radius" => numeric!(radius),
        "font-size" => numeric!(font_size),
        "line-height" => numeric!(line_height),
        "letter-spacing" => numeric!(letter_spacing),
        "gap" => numeric!(gap),
        "padding" | "margin" => {
            let a = if property == "padding" { from.padding } else { from.margin };
            let b = if property == "padding" { to.padding } else { to.margin };
            let value = std::array::from_fn(|i| number(a[i], b[i], k));
            if property == "padding" {
                into.padding = value;
            } else {
                into.margin = value;
            }
        }
        "padding-top" | "padding-right" | "padding-bottom" | "padding-left" | "margin-top" | "margin-right" | "margin-bottom" | "margin-left" => {
            let index = match property.rsplit('-').next()? {
                "top" => 0,
                "right" => 1,
                "bottom" => 2,
                _ => 3,
            };
            if property.starts_with("padding") {
                into.padding[index] = number(from.padding[index], to.padding[index], k);
            } else {
                into.margin[index] = number(from.margin[index], to.margin[index], k);
            }
        }
        "box-shadow" => {
            let outer = shadow(&from.shadow, &to.shadow, k)?;
            let inner = shadow(&from.inset_shadow, &to.inset_shadow, k)?;
            into.shadow = outer;
            into.inset_shadow = inner;
        }
        _ => return None,
    }
    Some(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use motif_core::kurbo::Point;
    #[test]
    fn property_changes_only_its_own_fields_and_rejects_unsupported_units_atomically() {
        let mut a = Style::default();
        a.width = Some(100.0);
        a.opacity = 0.2;
        a.background = "#ff0000".into();
        a.offsets[3] = Some(Length { px: 10.0, percent: 0.0 });
        let mut b = a.clone();
        b.width = Some(200.0);
        b.opacity = 0.8;
        b.background = "#0000ff".into();
        b.offsets[3] = Some(Length { px: 0.0, percent: 0.5 });
        let mut out = a.clone();
        assert!(property("opacity", &mut out, &a, &b, 0.5));
        assert!((out.opacity - 0.5).abs() < 1e-10);
        assert_eq!(out.background, a.background);
        assert_eq!(out.width, a.width);
        assert!(property("left", &mut out, &a, &b, 0.5));
        assert_eq!(out.offsets[3], Some(Length { px: 5.0, percent: 0.25 }));
        assert!(property("width", &mut out, &a, &b, 0.5));
        assert_eq!(out.width, Some(150.0));
        b.width = None;
        b.width_percent = Some(0.5);
        let before = out.clone();
        assert!(!property("width", &mut out, &a, &b, 0.5));
        assert_eq!(out, before);
        assert!(!property("clip-path", &mut out, &a, &b, 0.5));
        assert_eq!(out, before);
    }
    #[test]
    fn none_paints_fade_with_the_other_rgb_and_preserve_the_exact_endpoints() {
        let a = Style::default();
        let mut b = a.clone();
        b.border_color = "#2761c3".into();
        let mut out = a.clone();
        assert!(property("border-color", &mut out, &a, &b, 0.5));
        let c = Color::from_hex(&out.border_color).unwrap();
        assert_eq!((c.r, c.g, c.b), (0x27, 0x61, 0xc3));
        assert!((c.a as i32 - 128).abs() <= 1);
        property("border-color", &mut out, &a, &b, 0.0);
        assert_eq!(out.border_color, "none");
        property("border-color", &mut out, &a, &b, 1.0);
        assert_eq!(out.border_color, "#2761c3");
    }
    #[test]
    fn matching_transforms_keep_full_rotations_and_missing_operations_get_css_identity() {
        let mut a = Style::default();
        a.transforms = vec![TransformOp::Rotate(350.0)];
        let mut b = a.clone();
        b.transforms = vec![TransformOp::Rotate(370.0), TransformOp::Scale(2.0, 3.0)];
        let mut out = a.clone();
        assert!(property("transform", &mut out, &a, &b, 0.5));
        assert_eq!(out.transforms, vec![TransformOp::Rotate(360.0), TransformOp::Scale(1.5, 2.0)]);
        let empty = Style::default();
        assert!(property("transform", &mut out, &empty, &b, 0.5));
        assert_eq!(out.transforms, vec![TransformOp::Rotate(185.0), TransformOp::Scale(1.5, 2.0)]);
    }
    #[test]
    fn different_transform_tail_uses_matrix_decomposition_and_rejects_unresolved_percent() {
        let mut a = Style::default();
        a.transforms = vec![TransformOp::Rotate(90.0)];
        let mut b = Style::default();
        b.transforms = vec![TransformOp::Translate(Length { px: 20.0, percent: 0.0 }, Length::default())];
        let mut out = a.clone();
        assert!(property("transform", &mut out, &a, &b, 0.5));
        let point = out.transform_matrix(20.0, 20.0) * Point::new(1.0, 0.0);
        let expected = std::f64::consts::FRAC_1_SQRT_2;
        assert!((point.x - 10.0 - expected).abs() < 1e-10 && (point.y - expected).abs() < 1e-10, "{point:?}");
        b.transforms = vec![TransformOp::Translate(Length { px: 0.0, percent: 0.5 }, Length::default())];
        let before = out.clone();
        assert!(!property("transform", &mut out, &a, &b, 0.5));
        assert_eq!(out, before);
    }
    #[test]
    fn shadows_interpolate_outer_and_inset_separately_from_transparent_zero() {
        let a = Style::default();
        let mut b = a.clone();
        b.inset_shadow = Some((10.0, 4.0, 20.0, "#27c39f".into()));
        b.shadow = Some((-2.0, 0.0, 8.0, "#2761c3".into()));
        let mut out = a.clone();
        assert!(property("box-shadow", &mut out, &a, &b, 0.5));
        let inner = out.inset_shadow.as_ref().unwrap();
        assert_eq!((inner.0, inner.1, inner.2), (5.0, 2.0, 10.0));
        let c = Color::from_hex(&inner.3).unwrap();
        assert_eq!((c.r, c.g, c.b), (0x27, 0xc3, 0x9f));
        assert!((c.a as i32 - 128).abs() <= 1);
        let outer = out.shadow.as_ref().unwrap();
        assert_eq!((outer.0, outer.1, outer.2), (-1.0, 0.0, 4.0));
        property("box-shadow", &mut out, &a, &b, 0.0);
        assert!(out.inset_shadow.is_none() && out.shadow.is_none());
        property("box-shadow", &mut out, &a, &b, 1.0);
        assert_eq!(out.inset_shadow, b.inset_shadow);
        assert_eq!(out.shadow, b.shadow);
    }
}
