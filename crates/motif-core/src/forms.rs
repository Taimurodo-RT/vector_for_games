//! Authored vector forms, addressed by contour keys. Form zero is the drawn source.
use crate::Document;
use kurbo::{BezPath, PathEl};

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Forms {
    pub shapes: Vec<BezPath>,
    pub amount: f64,
    pub(crate) mix: Option<(usize, usize, f64)>,
    pub(crate) path: Option<BezPath>,
}

impl Document {
    pub fn forms_all(&mut self) {
        for node in &mut self.nodes {
            if node.forms.is_none() {
                continue;
            }
            let base = node.morph.as_ref().and_then(|m| m.path.clone()).or_else(|| node.shape_path());
            let (Some(forms), Some(base)) = (&mut node.forms, base) else { continue };
            let amount = forms.amount.clamp(0.0, forms.shapes.len() as f64);
            let (a, b, t) = forms.mix.unwrap_or((amount.floor() as usize, amount.ceil() as usize, amount.fract()));
            let get = |i: usize| if i == 0 { &base } else { forms.shapes.get(i - 1).unwrap_or(&base) };
            // The artist's marks hold between neighbours, whichever way the step goes.
            let links = |k: usize| node.morph_links.iter().filter(|l| l.key == k).copied().collect::<Vec<_>>();
            let (forward, back) = (if b == a + 1 { links(a) } else { Vec::new() }, if a == b + 1 { links(b) } else { Vec::new() });
            forms.path = Some(if !forward.is_empty() && t > 0.0 && t < 1.0 {
                crate::morph::blend_linked(get(a), get(b), t, &forward)
            } else if !back.is_empty() && t > 0.0 && t < 1.0 {
                crate::morph::blend_linked(get(b), get(a), 1.0 - t, &back)
            } else {
                blend_forms(get(a), get(b), t)
            });
        }
    }
}

pub(crate) fn blend_forms(a: &BezPath, b: &BezPath, t: f64) -> BezPath {
    if t == 0.0 || a == b {
        return a.clone();
    }
    if t == 1.0 {
        return b.clone();
    }
    if a.elements().len() == b.elements().len() {
        let mixed: Option<Vec<_>> = a
            .elements()
            .iter()
            .zip(b.elements())
            .map(|(a, b)| {
                Some(match (*a, *b) {
                    (PathEl::MoveTo(a), PathEl::MoveTo(b)) => PathEl::MoveTo(a.lerp(b, t)),
                    (PathEl::LineTo(a), PathEl::LineTo(b)) => PathEl::LineTo(a.lerp(b, t)),
                    (PathEl::QuadTo(a, p), PathEl::QuadTo(b, q)) => PathEl::QuadTo(a.lerp(b, t), p.lerp(q, t)),
                    (PathEl::CurveTo(a, b, p), PathEl::CurveTo(c, d, q)) => PathEl::CurveTo(a.lerp(c, t), b.lerp(d, t), p.lerp(q, t)),
                    (PathEl::ClosePath, PathEl::ClosePath) => PathEl::ClosePath,
                    _ => return None,
                })
            })
            .collect();
        if let Some(els) = mixed {
            return BezPath::from_vec(els);
        }
    }
    crate::morph::blend(a, b, t.clamp(0.0, 1.0))
}
