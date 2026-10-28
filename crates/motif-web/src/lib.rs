//! Deterministic SVG from Motif's evaluated model; shared by GUI and CLI.
mod config;
mod package;
mod svg;
pub use config::{Asset, Config, Layer, Profile, read, slug, write};
pub use package::{Package, build, export, gallery};
pub use svg::{Svg, render};

/// Preview only: never changes the document's authored colors or source text.
pub fn preview_theme(scene: &motif_script::Scene, doc: &mut motif_core::Document, config: &Config, theme: &str) {
    let Some(colors) = config.themes.get(theme) else { return };
    for n in &mut doc.nodes {
        for prop in [motif_core::Prop::Fill, motif_core::Prop::StrokeColor] {
            let Some(name) = scene.refs.get(&(n.id.clone(), prop)) else { continue };
            if !svg::global_reference(scene, &n.id, name) {
                continue;
            }
            let Some(c) = colors.get(name).and_then(|s| motif_core::Color::from_hex(s)) else { continue };
            // Animated or locally edited colors take precedence over a preview token.
            if scene.palette.iter().any(|s| s.name == *name && n.get(prop) == motif_core::Value::Color(s.color)) {
                n.set(prop, motif_core::Value::Color(c));
            }
        }
        for (stroke, gradient) in [(false, &mut n.gradient), (true, &mut n.stroke_gradient)] {
            if let Some(g) = gradient
                && let Some(refs) = scene.gradient_refs.get(&(n.id.clone(), stroke))
            {
                for (c, name) in g.colors.iter_mut().zip(refs) {
                    if let Some(name) = name
                        && svg::global_reference(scene, &n.id, name)
                        && scene.palette.iter().any(|s| s.name == *name && s.color == *c)
                        && let Some(replacement) = colors.get(name).and_then(|s| motif_core::Color::from_hex(s))
                    {
                        *c = replacement;
                    }
                }
            }
        }
    }
}

pub(crate) fn xml(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;").replace('\'', "&apos;")
}
pub(crate) fn num(v: f64) -> String {
    motif_script::fmt_num_with(if v.abs() < 0.005 { 0.0 } else { v }, 2)
}
