use crate::{Asset, Config, Profile, config, num, xml};
use motif_core::kurbo::{Affine, BezPath, PathEl, Shape};
use motif_core::{Color, Document, Gradient, Node, Prop, ShapeKind};
use motif_script::Scene;
use std::collections::{BTreeMap, HashSet};

pub struct Svg {
    pub text: String,
    pub id: String,
    pub file: String,
    pub node_ids: BTreeMap<String, String>,
    pub warnings: Vec<String>,
}

pub(crate) fn matrix(a: Affine) -> String {
    a.as_coeffs().iter().map(|x| motif_script::fmt_num_with(*x, 6)).collect::<Vec<_>>().join(" ")
}

pub(crate) fn transform(a: Affine) -> String {
    if a == Affine::IDENTITY {
        return String::new();
    }
    let [a0, a1, a2, a3, x, y] = a.as_coeffs();
    if a0 == 1.0 && a1 == 0.0 && a2 == 0.0 && a3 == 1.0 {
        format!(" transform=\"translate({} {})\"", num(x), num(y))
    } else {
        format!(" transform=\"matrix({})\"", matrix(a))
    }
}

pub(crate) fn path(p: &BezPath) -> String {
    let xy = |p: motif_core::kurbo::Point| format!("{} {}", num(p.x), num(p.y));
    p.elements()
        .iter()
        .map(|e| match e {
            PathEl::MoveTo(p) => format!("M{}", xy(*p)),
            PathEl::LineTo(p) => format!("L{}", xy(*p)),
            PathEl::QuadTo(a, p) => format!("Q{} {}", xy(*a), xy(*p)),
            PathEl::CurveTo(a, b, p) => format!("C{} {} {}", xy(*a), xy(*b), xy(*p)),
            PathEl::ClosePath => "Z".into(),
        })
        .collect::<String>()
}

fn geometry(n: &Node) -> (String, String) {
    if n.deformed.is_none() && n.morph.is_none() {
        match n.kind {
            ShapeKind::Rect if n.radii.is_none() => {
                return (
                    "rect".into(),
                    format!(
                        " x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" rx=\"{}\"",
                        num(-n.width / 2.0),
                        num(-n.height / 2.0),
                        num(n.width),
                        num(n.height),
                        num(n.radius.max(0.0).min(n.width.min(n.height) / 2.0))
                    ),
                );
            }
            ShapeKind::Ellipse => return ("ellipse".into(), format!(" rx=\"{}\" ry=\"{}\"", num(n.width / 2.0), num(n.height / 2.0))),
            _ => (),
        }
    }
    ("path".into(), format!(" d=\"{}\"", path(&n.path().unwrap_or_default())))
}

/// Uses provenance, never substitutes an unrelated literal just because its HEX matches.
pub(crate) fn global_reference(scene: &Scene, node: &str, name: &str) -> bool {
    !scene.instances.iter().any(|(id, instance)| node.starts_with(&format!("{id}-")) && instance.params.iter().any(|p| p.name == name))
}

pub(crate) fn paint(c: Color, reference: Option<&str>, scene: &Scene, config: &Config, node: &str) -> String {
    let Some(name) = reference else { return c.to_hex() };
    if !global_reference(scene, node, name) || !scene.palette.iter().any(|s| s.name == name && s.color == c) {
        return c.to_hex();
    }
    match config.tokens.get(name).map(String::as_str) {
        Some("currentColor") => "currentColor".into(),
        Some(var) => format!("var({var},{})", c.to_hex()),
        None => c.to_hex(),
    }
}

fn gradient(g: &Gradient, n: &Node, stroke: bool, id: &str, scene: &Scene, config: &Config) -> String {
    let (a, b) = g.line(n.path().unwrap_or_default().bounding_box());
    let (tag, coords) = if g.radial {
        ("radialGradient", format!("cx=\"{}\" cy=\"{}\" r=\"{}\"", num(a.x), num(a.y), num(a.distance(b))))
    } else {
        ("linearGradient", format!("x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\"", num(a.x), num(a.y), num(b.x), num(b.y)))
    };
    let refs = scene.gradient_refs.get(&(n.id.clone(), stroke));
    let stops = g
        .colors
        .iter()
        .enumerate()
        .map(|(i, c)| {
            format!(
                "<stop offset=\"{}\" stop-color=\"{}\"/>",
                num(i as f64 / (g.colors.len() - 1) as f64),
                paint(*c, refs.and_then(|r| r.get(i)).and_then(|r| r.as_deref()), scene, config, &n.id)
            )
        })
        .collect::<String>();
    format!("<{tag} id=\"{id}\" gradientUnits=\"userSpaceOnUse\" {coords}>{stops}</{tag}>")
}

struct Writer<'a> {
    scene: &'a Scene,
    doc: &'a Document,
    config: &'a Config,
    paint_doc: Document,
    asset: &'a Asset,
    ids: BTreeMap<String, String>,
    defs: String,
    prefix: String,
}

impl Writer<'_> {
    // World-space bounds deliberately overestimate clipping, never cut off a soft edge.
    fn bounds(&self, index: usize) -> Option<motif_core::kurbo::Rect> {
        let n = &self.doc.nodes[index];
        if !self.doc.is_drawn(index) {
            return None;
        }
        if !n.kind.is_container() {
            return motif_render::ops_bounds(&motif_render::node_ops(&self.paint_doc, index));
        }
        self.doc
            .nodes
            .iter()
            .enumerate()
            .filter(|(_, n)| n.parent == Some(index))
            .filter_map(|(i, _)| self.bounds(i))
            .reduce(|a, b| a.union(b))
            .map(|b| b.inflate(n.blur * 2.0, n.blur * 2.0))
    }
    fn node(&mut self, index: usize) -> Result<String, String> {
        let n = &self.doc.nodes[index];
        if !n.visible {
            return Ok(String::new());
        }
        let id = &self.ids[&n.id];
        let layer = self.asset.layers.get(&n.id);
        let class = layer.map(|l| l.class.as_str()).filter(|s| !s.is_empty()).or_else(|| n.name.as_deref().filter(|s| s.split_whitespace().all(config::ident)));
        let mut attrs = format!(" id=\"{id}\"{}", transform(n.local_transform()));
        // The layer's own name, in Russian as written: ids and classes are Latin, the name stays.
        if let Some(name) = n.name.as_deref().filter(|s| !s.trim().is_empty()).or_else(|| (!n.id.is_ascii()).then_some(n.id.as_str())) {
            attrs.push_str(&format!(" data-name=\"{}\"", xml(name)));
        }
        if let Some(c) = class {
            attrs.push_str(&format!(" class=\"{}\"", xml(c)));
        }
        // Motif multiplies ancestor opacity into each paint, without isolating a group.
        // SVG group opacity composites the group first and would change overlapping shapes.
        let opacity = self.doc.world_opacity(index).clamp(0.0, 1.0);
        if !n.kind.is_container() && opacity != 1.0 {
            attrs.push_str(&format!(" fill-opacity=\"{}\" stroke-opacity=\"{}\"", num(opacity), num(opacity)));
        }
        if n.even_odd {
            attrs.push_str(" fill-rule=\"evenodd\"");
        }
        let children: Vec<usize> = self.doc.nodes.iter().enumerate().filter(|(_, c)| c.parent == Some(index)).map(|(i, _)| i).collect();
        let mut skip = None;
        let mut clip = String::new();
        if n.clip {
            let &first = children.first().ok_or_else(|| format!("{}: clip без маски", n.id))?;
            let mask = &self.doc.nodes[first];
            if mask.kind.is_container() {
                return Err(format!("{}: маска-группа пока не поддерживается SVG-экспортом", n.id));
            }
            let cid = format!("{}-clip-{index}", self.prefix);
            self.defs.push_str(&format!(
                "<clipPath id=\"{cid}\" clipPathUnits=\"userSpaceOnUse\"><path id=\"{}\" d=\"{}\"{} clip-rule=\"nonzero\"/></clipPath>",
                self.ids[&mask.id],
                path(&mask.path().unwrap_or_default()),
                transform(mask.local_transform())
            ));
            clip = format!(" clip-path=\"url(#{cid})\"");
            skip = Some(first);
        }
        let mut content = String::new();
        if !n.kind.is_container() && crate::effects::expanded(n) {
            let prefix = format!("{}-nodefx-{index}", self.prefix);
            let mut effects = crate::effects::Effects::new(self.scene, self.config, &prefix);
            let body = effects.ops(&motif_render::node_ops(&self.paint_doc, index))?;
            self.defs.push_str(&effects.defs);
            content = format!("<g{attrs}{clip}><g{}>{body}</g></g>", transform(self.doc.world_transform(index).inverse()));
        } else if !n.kind.is_container() {
            attrs.push_str(&clip);
            let (tag, geom) = geometry(n);
            let mut style = String::new();
            for (stroke, g, color, prop, label) in
                [(false, n.gradient.as_ref(), n.fill, Prop::Fill, "fill"), (true, n.stroke_gradient.as_ref(), n.stroke, Prop::StrokeColor, "stroke")]
            {
                let color = if let Some(g) = g {
                    let gid = format!("{}-{}-{index}", self.prefix, label);
                    self.defs.push_str(&gradient(g, n, stroke, &gid, self.scene, self.config));
                    format!("url(#{gid})")
                } else {
                    color
                        .map(|c| paint(c, self.scene.refs.get(&(n.id.clone(), prop)).map(String::as_str), self.scene, self.config, &n.id))
                        .unwrap_or_else(|| "none".into())
                };
                if label == "fill" || color != "none" {
                    style.push_str(&format!(" {label}=\"{color}\""));
                }
            }
            if n.stroke.is_some() || n.stroke_gradient.is_some() {
                style.push_str(&format!(" stroke-width=\"{}\"", num(n.stroke_width.max(0.0))));
                if n.cap != motif_core::Cap::Round {
                    style.push_str(&format!(" stroke-linecap=\"{}\"", n.cap.keyword()));
                }
                if n.join != motif_core::Join::Round {
                    style.push_str(&format!(" stroke-linejoin=\"{}\"", n.join.keyword()));
                }
                if !n.dash.is_empty() {
                    style.push_str(&format!(" stroke-dasharray=\"{}\"", n.dash.iter().map(|x| num(*x)).collect::<Vec<_>>().join(" ")));
                }
            }
            content = format!("<{tag}{attrs}{geom}{style}/>");
        }
        let mut body = String::new();
        for child in children {
            if Some(child) != skip {
                body.push_str(&self.node(child)?);
            }
        }
        if n.kind.is_container() {
            if n.blur > 0.0 {
                let world = self.doc.world_transform(index);
                let prefix = format!("{}-groupfx-{index}", self.prefix);
                let mut effects = crate::effects::Effects::new(self.scene, self.config, &prefix);
                let inner = format!("<g{}{clip}>{body}</g>", transform(world));
                let body = match self.bounds(index) {
                    Some(bounds) => effects.blur(&inner, n.blur, bounds),
                    None => inner,
                };
                self.defs.push_str(&effects.defs);
                Ok(format!("<g{attrs}><g{}>{body}</g></g>", transform(world.inverse())))
            } else {
                Ok(format!("<g{attrs}{clip}>{body}</g>"))
            }
        } else if body.is_empty() {
            Ok(content)
        } else {
            Err(format!("{}: дочерние объекты у фигуры не поддерживаются", n.id))
        }
    }
}

pub fn render(scene: &Scene, config: &Config, override_profile: Option<Profile>) -> Result<Svg, String> {
    config::validate_metadata(scene, config)?;
    let doc = &scene.doc;
    if !doc.artboard.width.is_finite() || !doc.artboard.height.is_finite() || doc.artboard.width <= 0.0 || doc.artboard.height <= 0.0 {
        return Err("SVG: некорректный размер холста".into());
    }
    let mut asset = config.assets.get(&doc.artboard.name).cloned().unwrap_or_default();
    if let Some(p) = override_profile {
        asset.profile = p;
    }
    let file = if asset.file.is_empty() { format!("{}.svg", config::slug(&doc.artboard.name)) } else { asset.file.clone() };
    let prefix = config::slug(&file.trim_end_matches(".svg").replace('/', "-"));
    let id = if asset.id.is_empty() { config::slug(&doc.artboard.name) } else { asset.id.clone() };
    let mut ids = BTreeMap::new();
    let mut used = HashSet::from([format!("{prefix}-title")]);
    for n in &doc.nodes {
        let custom = asset.layers.get(&n.id).map(|l| l.id.as_str()).filter(|s| !s.is_empty());
        let nid = format!("{prefix}-node-{}", custom.map(String::from).unwrap_or_else(|| config::slug(&n.id)));
        if !used.insert(nid.clone()) {
            return Err(format!("Повторяющийся SVG id: {nid}"));
        }
        ids.insert(n.id.clone(), nid);
        if !doc.is_drawn(doc.index_of(&n.id).unwrap()) {
            continue;
        }
        let mut unsupported = Vec::new();
        if crate::effects::expanded(n) && doc.world_transform(doc.index_of(&n.id).unwrap()).determinant().abs() < 1e-12 {
            unsupported.push("effect with degenerate transform");
        }
        if n.blend != motif_core::Blend::Normal {
            unsupported.push("blend");
        }
        if n.body.is_some() || n.on.is_some() || n.wrap {
            unsupported.push("3D");
        }
        if n.emit.is_some() || n.particles.is_some() || n.soft.is_some() || n.fluid.is_some() || n.rope.is_some() {
            unsupported.push("simulation");
        }
        if !unsupported.is_empty() {
            return Err(format!(
                "SVG: слой {} использует {}. Экспорт остановлен без потери эффекта; используйте PNG или Motif runtime.",
                n.id,
                unsupported.join(", ")
            ));
        }
        if n.local_transform().as_coeffs().iter().any(|n| !n.is_finite()) {
            return Err(format!("{}: бесконечная трансформация", n.id));
        }
    }
    for layer in asset.layers.keys() {
        if !ids.contains_key(layer) {
            return Err(format!("В метаданных указан отсутствующий слой {layer}"));
        }
    }
    // Effect passes inherit each node's combined paint opacity from its semantic wrapper.
    // Keeping geometry at zero alpha also lets CSS fade an initially invisible object in.
    let mut paint_doc = doc.clone();
    for n in &mut paint_doc.nodes {
        n.opacity = 1.0;
    }
    let mut w = Writer { scene, doc, config, paint_doc, asset: &asset, ids, defs: String::new(), prefix: prefix.clone() };
    let mut body = String::new();
    for (i, n) in doc.nodes.iter().enumerate() {
        if n.parent.is_none() {
            body.push_str(&w.node(i)?);
        }
    }
    let defs = if w.defs.is_empty() { String::new() } else { format!("<defs>{}</defs>", w.defs) };
    let text = format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 {} {}\" stroke-linecap=\"round\" stroke-linejoin=\"round\" role=\"img\" aria-labelledby=\"{prefix}-title\" data-asset=\"{id}\"><title id=\"{prefix}-title\">{}</title>{defs}{body}</svg>\n",
        num(doc.artboard.width),
        num(doc.artboard.height),
        xml(&doc.artboard.name)
    );
    let mut warnings = Vec::new();
    let p = asset.profile;
    if let Some(size) = p.icon_size() {
        if doc.artboard.width != size || doc.artboard.height != size {
            return Err(format!("{} требует холст {size}×{size}", p.name()));
        }
        let expected = if size == 16.0 {
            1.5
        } else if size == 24.0 {
            1.75
        } else {
            2.0
        };
        if doc.nodes.iter().any(|n| n.stroke.is_some() && (n.stroke_width - expected).abs() > 0.001) {
            warnings.push(format!("{}: есть обводки не {expected} px", p.name()));
        }
        if doc.nodes.iter().any(|n| n.gradient.is_some() || n.stroke_gradient.is_some() || crate::effects::expanded(n)) {
            warnings.push("Иконка использует градиент или дополнительные эффекты".into());
        }
        if size == 16.0 {
            let colors: HashSet<_> = doc.nodes.iter().flat_map(|n| [n.fill, n.stroke]).flatten().filter(|c| c.a > 0).map(|c| (c.r, c.g, c.b)).collect();
            if colors.len() > 1 {
                warnings.push("Icon16 должен быть монохромным".into());
            }
            let off_grid = doc.nodes.iter().enumerate().any(|(i, n)| {
                n.path().is_some_and(|p| {
                    let p = doc.world_transform(i) * p;
                    p.elements().iter().any(|e| {
                        let points: Vec<_> = match e {
                            PathEl::MoveTo(p) | PathEl::LineTo(p) => vec![p],
                            PathEl::QuadTo(a, p) => vec![a, p],
                            PathEl::CurveTo(a, b, p) => vec![a, b, p],
                            PathEl::ClosePath => vec![],
                        };
                        points.iter().any(|p| (p.x * 2.0 - (p.x * 2.0).round()).abs() > 0.001 || (p.y * 2.0 - (p.y * 2.0).round()).abs() > 0.001)
                    })
                })
            });
            if off_grid {
                warnings.push("Icon16: некоторые координаты, включая контрольные точки кривых, вне сетки 0.5 px".into());
            }
        }
    }
    let max = match p {
        Profile::Icon16 | Profile::Icon24 | Profile::Icon48 => 2048,
        Profile::Tile => 8192,
        Profile::Illustration => 61440,
        Profile::General => usize::MAX,
    };
    if text.len() > max {
        warnings.push(format!("Размер {} байт превышает бюджет {max}", text.len()));
    }
    if !doc.machines.is_empty() {
        warnings.push("Машины состояний не исполняются SVG; для них нужен Motif runtime".into());
    }
    if scene.backdrop.is_some() {
        warnings.push("Подложка редактора не входит в SVG".into());
    }
    if let Some(s) = asset.slices
        && (s[1] + s[3] > doc.artboard.width || s[0] + s[2] > doc.artboard.height)
    {
        return Err("Slice выходит за размер холста".into());
    }
    Ok(Svg { text, id, file, node_ids: w.ids, warnings })
}
