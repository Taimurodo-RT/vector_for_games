//! Template controls rewrite declared board-local literals, keeping all expressions linked.
use crate::character::{Character, read};
use crate::lexer::{Tok, lex};
use crate::{Scene, Span, fmt_num, head_end, parse_in};
use std::collections::BTreeMap;
use std::path::Path;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Settings {
    pub parameters: BTreeMap<String, f64>,
    pub palette: BTreeMap<String, String>,
}

/// Only one direct literal declaration is allowed. Neither a shared document header,
/// an expression nor an inner binding is silently replaced by a character slider.
fn literals(src: &str, character: &Character) -> Result<BTreeMap<String, (Span, Tok)>, String> {
    let names: Vec<_> = character.parameters.iter().map(|p| &p.name).chain(character.palette.iter().map(|p| &p.name)).collect();
    if names.is_empty() {
        return Ok(BTreeMap::new());
    }
    let tokens = lex(src).map_err(|e| e.message)?;
    let mut found = BTreeMap::new();
    let mut depth = 0i32;
    for (i, token) in tokens.iter().enumerate() {
        match token.tok {
            Tok::BlockOpen => depth += 1,
            Tok::BlockClose => depth -= 1,
            _ => {}
        }
        if depth != 0 || !matches!(&token.tok, Tok::Ident(w) if w == "let") {
            continue;
        }
        let Some(next) = tokens.get(i + 1) else { continue };
        let Tok::Ident(name) = &next.tok else { continue };
        if !names.contains(&name) {
            continue;
        }
        let value = tokens.get(i + 3).ok_or("Не найдено значение настройки")?;
        if token.span.start < head_end(src)
            || !matches!(tokens.get(i + 2).map(|t| &t.tok), Some(Tok::Eq))
            || !matches!(value.tok, Tok::Num(_) | Tok::Color(_))
            || found.insert(name.clone(), (value.span, value.tok.clone())).is_some()
        {
            return Err(format!("Настройка «{name}» должна быть единственным числом или цветом let после artboard"));
        }
    }
    if names.iter().any(|name| !found.contains_key(*name)) {
        return Err("Не найдена строка let одной из настроек персонажа".into());
    }
    Ok(found)
}

pub fn settings(src: &str, character: &Character) -> Result<Settings, String> {
    let found = literals(src, character)?;
    let mut settings = Settings::default();
    for p in &character.parameters {
        match found[&p.name].1 {
            Tok::Num(v) if v.is_finite() => {
                settings.parameters.insert(p.name.clone(), v);
            }
            _ => return Err(format!("«{}» должна быть числом", p.name)),
        }
    }
    for p in &character.palette {
        match found[&p.name].1 {
            Tok::Color(v) => {
                settings.palette.insert(p.name.clone(), v.to_hex());
            }
            _ => return Err(format!("«{}» должна быть цветом", p.name)),
        }
    }
    Ok(settings)
}

pub fn configure(src: &str, scene: &Scene, root: &str, values: &Settings, base: Option<&Path>) -> Result<String, String> {
    let catalog = read(src)?.ok_or("В сцене нет описания персонажа")?;
    catalog.validate(scene)?;
    let character = catalog.characters.iter().find(|c| c.root == root).ok_or("Персонаж не найден")?;
    let found = literals(src, character)?;
    let mut edits = Vec::new();
    for (name, value) in &values.parameters {
        let control = character.parameters.iter().find(|p| &p.name == name).ok_or("Неизвестная настройка")?;
        if !value.is_finite() || *value < control.min || *value > control.max || !matches!(found[name].1, Tok::Num(_)) {
            return Err(format!("«{}»: допустимо от {} до {}", control.label, control.min, control.max));
        }
        edits.push((found[name].0, fmt_num(*value)));
    }
    for (name, value) in &values.palette {
        if !character.palette.iter().any(|p| &p.name == name) || !matches!(found.get(name).map(|v| &v.1), Some(Tok::Color(_))) {
            return Err("Неизвестный цвет персонажа".into());
        }
        let color = motif_core::Color::from_hex(value).ok_or("Некорректный цвет")?;
        edits.push((found[name].0, color.to_hex()));
    }
    edits.sort_by_key(|(span, _)| std::cmp::Reverse(span.start));
    let mut out = src.to_string();
    for (span, value) in edits {
        out.replace_range(span.start..span.end, &value);
    }
    let current = parse_in(&out, base).map_err(|e| e[0].message.clone())?;
    catalog.validate(&current)?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::character::{apply_look, copy_look, save_look};
    use motif_core::ShapeKind;
    use motif_core::kurbo::{Point, Shape};

    const BASE: &str = include_str!("../../../examples/character-platformer.msc");
    fn scene(src: &str) -> Scene {
        parse_in(src, None).unwrap()
    }

    #[test]
    fn presets_and_saved_looks_restore_linked_proportions_and_colors() {
        let before = scene(BASE);
        let catalog = read(BASE).unwrap().unwrap();
        catalog.validate(&before).unwrap();
        for look in &catalog.characters[0].looks {
            let out = apply_look(BASE, &before, "hero", &look.name, None).unwrap();
            assert!(look.matches(&out, &scene(&out), &catalog.characters[0]));
        }
        let values = Settings { parameters: BTreeMap::from([("leg_length".into(), 0.85)]), palette: BTreeMap::from([("red".into(), "#8448c9".into())]) };
        let custom = configure(BASE, &before, "hero", &values, None).unwrap();
        let edited = scene(&custom);
        assert_eq!(edited.doc.node("shin_front_bone").unwrap().width, 34.0 * 0.85);
        assert_eq!(edited.doc.node("cap_crown").unwrap().fill, motif_core::Color::from_hex("#8448c9"));
        let (saved, name) = save_look(&custom, &edited, "hero").unwrap();
        let other = apply_look(&saved, &scene(&saved), "hero", "Зелёный герой", None).unwrap();
        let restored = apply_look(&other, &scene(&other), "hero", &name, None).unwrap();
        assert_eq!(scene(&restored).doc, edited.doc);
        assert_eq!(settings(&restored, &catalog.characters[0]).unwrap(), settings(&custom, &catalog.characters[0]).unwrap());
    }

    #[test]
    fn parameter_extremes_keep_feet_attached_and_visible_art_inside_the_frame() {
        let before = scene(BASE);
        let catalog = read(BASE).unwrap().unwrap();
        let c = &catalog.characters[0];
        // Every corner of the supported parameter box, across all four motions.
        for mask in 0..(1 << c.parameters.len()) {
            let parameters = c.parameters.iter().enumerate().map(|(i, p)| (p.name.clone(), if mask & (1 << i) == 0 { p.min } else { p.max })).collect();
            let out = configure(BASE, &before, "hero", &Settings { parameters, ..Settings::default() }, None).unwrap();
            let doc = scene(&out).doc;
            for anim in &doc.animations {
                let mut endpoints = anim.clone();
                endpoints.looping = false;
                let start = endpoints.pose(&doc, 0.0);
                let end = endpoints.pose(&doc, anim.duration);
                for bone in &c.bones {
                    let i = doc.index_of(bone).unwrap();
                    assert_eq!(start.world_transform(i), end.world_transform(i), "{} seam {bone}", anim.name);
                }
                for frame in 0..=8 {
                    let pose = anim.pose(&doc, anim.duration * frame as f64 / 8.0);
                    for side in ["front", "back"] {
                        let shin = pose.index_of(&format!("shin_{side}_bone")).unwrap();
                        let foot = pose.index_of(&format!("foot_{side}")).unwrap();
                        let tip = pose.world_transform(shin) * pose.nodes[shin].bone_tip();
                        assert!(tip.distance(pose.world_transform(foot) * Point::ZERO) < 0.1, "mask {mask} {} frame {frame} {side}", anim.name);
                    }
                    for (i, node) in pose.nodes.iter().enumerate() {
                        if !pose.is_visible(i) || pose.world_opacity(i) == 0.0 || matches!(node.kind, ShapeKind::Group | ShapeKind::Bone) {
                            continue;
                        }
                        let bounds = pose.world_transform(i).transform_rect_bbox(node.path().unwrap().bounding_box().inflate(1.5, 1.5));
                        assert!(
                            bounds.x0 >= 0.0 && bounds.y0 >= 0.0 && bounds.x1 <= doc.artboard.width && bounds.y1 <= doc.artboard.height,
                            "mask {mask} {} frame {frame} {}: {bounds:?}",
                            anim.name,
                            node.id
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn shared_headers_duplicate_bindings_expressions_and_bad_values_are_not_overwritten() {
        let before = scene(BASE);
        for value in [f64::NAN, f64::INFINITY, -1.0, 20.0] {
            let values = Settings { parameters: BTreeMap::from([("leg_length".into(), value)]), ..Settings::default() };
            assert!(configure(BASE, &before, "hero", &values, None).is_err());
        }
        let values = Settings { parameters: BTreeMap::from([("leg_length".into(), 1.1)]), ..Settings::default() };
        for src in [
            BASE.replace("let leg_length = 1.0", "let leg_length = {0.5 + 0.5}"),
            BASE.replace("let leg_length = 1.0", "let leg_length = 1.0\nlet leg_length = 1.0"),
            format!("let leg_length = 1.0\n{}", BASE.replace("let leg_length = 1.0", "")),
            BASE.replace("let leg_length = 1.0", "let leg_length = #fff"),
        ] {
            assert!(configure(&src, &before, "hero", &values, None).is_err());
        }
        let unknown = Settings { palette: BTreeMap::from([("unregistered".into(), "#fff".into())]), ..Settings::default() };
        assert!(configure(BASE, &before, "hero", &unknown, None).is_err());
        let out_of_range = BASE.replace("let head_size = 1.0", "let head_size = 9.0");
        assert!(save_look(&out_of_range, &scene(&out_of_range), "hero").is_err());
    }

    #[test]
    fn a_whole_look_forks_equipped_parts_once_and_keeps_the_rig_and_motion() {
        let before = scene(BASE);
        let original = read(BASE).unwrap().unwrap();
        let (out, name) = copy_look(BASE, &before, "hero", None).unwrap();
        let after = scene(&out);
        let catalog = read(&out).unwrap().unwrap();
        let c = &catalog.characters[0];
        catalog.validate(&after).unwrap();
        assert_eq!(after.doc.animations, before.doc.animations);
        assert_eq!(after.doc.machines, before.doc.machines);
        assert!(c.looks.iter().find(|l| l.name == name).unwrap().matches(&out, &after, c));
        for bone in &c.bones {
            let mut a = before.doc.node(bone).unwrap().clone();
            let mut b = after.doc.node(bone).unwrap().clone();
            assert_eq!(a.parent.map(|i| &before.doc.nodes[i].id), b.parent.map(|i| &after.doc.nodes[i].id));
            a.parent = None;
            b.parent = None;
            assert_eq!(a, b);
        }
        for (old, slot) in original.characters[0].slots.iter().zip(&c.slots) {
            let old_variant = old.selected(&before).unwrap();
            let new_variant = slot.selected(&after).unwrap();
            if old_variant.groups.is_empty() {
                assert!(new_variant.groups.is_empty());
                continue;
            }
            assert_ne!(old_variant.id, new_variant.id);
            for (a, b) in old_variant.groups.iter().zip(&new_variant.groups) {
                assert!(!after.doc.node(a).unwrap().visible);
                let a = before.doc.index_of(a).unwrap();
                let b = after.doc.index_of(b).unwrap();
                let parent_id = |s: &Scene, i: usize| s.doc.nodes[i].parent.map(|p| s.doc.nodes[p].id.clone());
                assert_eq!(parent_id(&before, a), parent_id(&after, b));
            }
        }
    }
}
