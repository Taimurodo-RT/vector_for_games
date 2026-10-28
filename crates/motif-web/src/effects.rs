//! SVG effects consume the same vector operations as both native renderers.
//! All filters live in artboard coordinates, so skew/scale never distorts blur radii.
use crate::{Config, num, svg};
use motif_core::{Node, Prop};
use motif_render::{Op, Paint};
use motif_script::Scene;

pub(crate) fn expanded(n: &Node) -> bool {
    n.blur > 0.0
        || n.shadow.is_some()
        || n.elevation != 0.0
        || n.glow.is_some()
        || n.inset.is_some()
        || n.noise.is_some()
        || !n.outlines.is_empty()
        || !n.brush.is_plain()
}

pub(crate) struct Effects<'a> {
    pub scene: &'a Scene,
    pub config: &'a Config,
    pub prefix: &'a str,
    pub defs: String,
    sequence: usize,
}

impl<'a> Effects<'a> {
    pub fn new(scene: &'a Scene, config: &'a Config, prefix: &'a str) -> Self {
        Self { scene, config, prefix, defs: String::new(), sequence: 0 }
    }
    fn id(&mut self) -> String {
        self.sequence += 1;
        format!("{}-fx-{}", self.prefix, self.sequence)
    }
    pub fn blur(&mut self, body: &str, radius: f64, bounds: motif_core::kurbo::Rect) -> String {
        let id = self.id();
        let b = bounds.inflate(radius * 2.0 + 1.0, radius * 2.0 + 1.0);
        self.defs.push_str(&format!("<filter id=\"{id}\" filterUnits=\"userSpaceOnUse\" color-interpolation-filters=\"sRGB\" x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\"><feGaussianBlur stdDeviation=\"{}\"/></filter>",num(b.x0),num(b.y0),num(b.width().max(0.01)),num(b.height().max(0.01)),num(radius/2.0)));
        format!("<g filter=\"url(#{id})\">{body}</g>")
    }
    pub fn ops(&mut self, ops: &[Op]) -> Result<String, String> {
        let mut out = String::new();
        let mut depth = 0;
        for op in ops {
            match op {
                Op::PushClip { path, transform, even_odd } => {
                    let id = self.id();
                    self.defs.push_str(&format!(
                        "<clipPath id=\"{id}\" clipPathUnits=\"userSpaceOnUse\"><path d=\"{}\"{} clip-rule=\"{}\"/></clipPath>",
                        svg::path(path),
                        svg::transform(*transform),
                        if *even_odd { "evenodd" } else { "nonzero" }
                    ));
                    out.push_str(&format!("<g clip-path=\"url(#{id})\">"));
                    depth += 1;
                }
                Op::Pop => {
                    if depth == 0 {
                        return Err("Несбалансированная clip-маска".into());
                    }
                    out.push_str("</g>");
                    depth -= 1;
                }
                Op::Blur { ops, radius } => {
                    let inner = self.ops(ops)?;
                    if let Some(bounds) = motif_render::ops_bounds(ops) {
                        out.push_str(&self.blur(&inner, *radius, bounds));
                    }
                }
                Op::Draw(item) => {
                    let n = &self.scene.doc.nodes[item.node];
                    let mut fill = "none".to_string();
                    if let Some(paint) = &item.fill {
                        fill = match paint {
                            Paint::Solid(c) => svg::paint(
                                *c,
                                item.fill_source.and_then(|p| self.scene.refs.get(&(n.id.clone(), p))).map(String::as_str),
                                self.scene,
                                self.config,
                                &n.id,
                            ),
                            Paint::Gradient { gradient, from, to } => {
                                let id = self.id();
                                let source = item.fill_source;
                                let refs = source.and_then(|p| self.scene.gradient_refs.get(&(n.id.clone(), p == Prop::StrokeColor)));
                                // Banded gradients have derived stops, not original palette references.
                                let original = source.and_then(|p| if p == Prop::StrokeColor { n.stroke_gradient.as_ref() } else { n.gradient.as_ref() });
                                let refs = refs.filter(|_| original.is_some_and(|g| g.colors == gradient.colors));
                                let (tag, coords) = if gradient.radial {
                                    ("radialGradient", format!("cx=\"{}\" cy=\"{}\" r=\"{}\"", num(from.x), num(from.y), num(from.distance(*to))))
                                } else {
                                    ("linearGradient", format!("x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\"", num(from.x), num(from.y), num(to.x), num(to.y)))
                                };
                                let stops = gradient
                                    .colors
                                    .iter()
                                    .enumerate()
                                    .map(|(i, c)| {
                                        format!(
                                            "<stop offset=\"{}\" stop-color=\"{}\"/>",
                                            motif_script::fmt_num_with(i as f64 / (gradient.colors.len() - 1).max(1) as f64, 6),
                                            svg::paint(*c, refs.and_then(|r| r.get(i)).and_then(|s| s.as_deref()), self.scene, self.config, &n.id)
                                        )
                                    })
                                    .collect::<String>();
                                self.defs.push_str(&format!("<{tag} id=\"{id}\" gradientUnits=\"userSpaceOnUse\" {coords}>{stops}</{tag}>"));
                                format!("url(#{id})")
                            }
                        };
                    }
                    let mut attrs = format!(" fill=\"{fill}\"");
                    if item.fill_source == Some(Prop::Fill) {
                        attrs.push_str(" data-motif-paint=\"fill\"");
                    }
                    if item.even_odd {
                        attrs.push_str(" fill-rule=\"evenodd\"");
                    }
                    if let Some((c, w)) = item.stroke {
                        let color = svg::paint(
                            c,
                            item.stroke_source.and_then(|p| self.scene.refs.get(&(n.id.clone(), p))).map(String::as_str),
                            self.scene,
                            self.config,
                            &n.id,
                        );
                        attrs.push_str(&format!(
                            " stroke=\"{color}\" stroke-width=\"{}\" stroke-linecap=\"{}\" stroke-linejoin=\"{}\"",
                            num(w),
                            item.line.cap.keyword(),
                            item.line.join.keyword()
                        ));
                        if !item.line.dash.is_empty() {
                            attrs.push_str(&format!(" stroke-dasharray=\"{}\"", item.line.dash.iter().map(|v| num(*v)).collect::<Vec<_>>().join(" ")));
                        }
                    }
                    out.push_str(&format!("<path d=\"{}\"{}{attrs}/>", svg::path(&item.path), svg::transform(item.transform)));
                }
            }
        }
        if depth != 0 {
            return Err("Незакрытая clip-маска".into());
        }
        Ok(out)
    }
}
