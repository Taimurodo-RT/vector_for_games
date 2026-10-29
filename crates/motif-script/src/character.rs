//! Character wardrobes are optional board-local metadata. Drawings, bones and animation
//! stay ordinary MSC nodes; choosing an outfit only changes visibility of owned groups.
use crate::{Scene, Span, copy_nodes, parse_in, set_style};
use motif_core::ShapeKind;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};
use std::path::Path;

const MARKER: &str = "// @motif-characters ";
pub use crate::character_controls::{Settings, configure, settings};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Catalog {
    pub schema: u32,
    pub characters: Vec<Character>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Character {
    pub root: String,
    pub name: String,
    pub rig: String,
    pub bones: Vec<String>,
    pub slots: Vec<Slot>,
    pub looks: Vec<Look>,
    pub motions: Vec<Motion>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub parameters: Vec<Parameter>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub palette: Vec<PaletteRole>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Parameter {
    pub name: String,
    pub label: String,
    pub min: f64,
    pub max: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PaletteRole {
    pub name: String,
    pub label: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Slot {
    pub id: String,
    pub name: String,
    pub variants: Vec<Variant>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Variant {
    pub id: String,
    pub name: String,
    pub groups: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Look {
    pub name: String,
    pub slots: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub parameters: BTreeMap<String, f64>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub palette: BTreeMap<String, String>,
}

impl Look {
    pub fn matches(&self, src: &str, scene: &Scene, character: &Character) -> bool {
        character.slots.iter().all(|s| s.selected(scene).is_some_and(|v| self.slots.get(&s.id) == Some(&v.id)))
            && settings(src, character).is_ok_and(|current| {
                self.parameters.iter().all(|(n, v)| current.parameters.get(n).is_some_and(|c| (c - v).abs() < 1e-6))
                    && self.palette.iter().all(|(n, v)| current.palette.get(n).and_then(|c| motif_core::Color::from_hex(c)) == motif_core::Color::from_hex(v))
            })
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Motion {
    pub name: String,
    pub clip: String,
}

fn metadata(src: &str) -> Result<Option<(Span, &str)>, String> {
    let mut at = 0;
    let mut found = None;
    for line in src.split_inclusive('\n') {
        if let Some(json) = line.trim_start().strip_prefix(MARKER) {
            if found.is_some() {
                return Err("На одном холсте допускается одна запись @motif-characters".into());
            }
            found = Some((Span::new(at, at + line.trim_end_matches(['\r', '\n']).len()), json.trim()));
        }
        at += line.len();
    }
    Ok(found)
}

pub fn read(src: &str) -> Result<Option<Catalog>, String> {
    let Some((_, json)) = metadata(src)? else { return Ok(None) };
    let catalog: Catalog = serde_json::from_str(json).map_err(|e| format!("Описание персонажа: {e}"))?;
    if catalog.schema != 1 {
        return Err(format!("Версия описания персонажа {} не поддерживается", catalog.schema));
    }
    Ok(Some(catalog))
}

fn write(src: &str, catalog: &Catalog) -> Result<String, String> {
    let (span, _) = metadata(src)?.ok_or("Описание персонажа не найдено")?;
    let json = serde_json::to_string(catalog).map_err(|e| e.to_string())?;
    let mut out = src.to_string();
    out.replace_range(span.start..span.end, &format!("{MARKER}{json}"));
    Ok(out)
}

impl Catalog {
    /// Invalid or overlapping ownership cannot hide a skeleton or another character's parts.
    pub fn validate(&self, scene: &Scene) -> Result<(), String> {
        let doc = &scene.doc;
        let mut roots = HashSet::new();
        let mut owned = Vec::new();
        let mut controls = HashSet::new();
        for character in &self.characters {
            for p in &character.parameters {
                if p.name.is_empty() || !controls.insert(&p.name) || !p.min.is_finite() || !p.max.is_finite() || p.min >= p.max {
                    return Err("Некорректные параметры персонажа".into());
                }
            }
            for p in &character.palette {
                if p.name.is_empty() || !controls.insert(&p.name) {
                    return Err("Повторяются имена настроек персонажа".into());
                }
            }
            if !roots.insert(&character.root) {
                return Err("Повторяется корень персонажа".into());
            }
            let root = doc.index_of(&character.root).ok_or_else(|| format!("Не найден персонаж «{}»", character.root))?;
            if doc.nodes[root].kind != ShapeKind::Group || scene.sites.get(&character.root).is_none_or(|s| s.generated) {
                return Err("Корень персонажа должен быть обычной группой".into());
            }
            for bone in &character.bones {
                let b = doc.index_of(bone).ok_or_else(|| format!("Не найдена кость «{bone}»"))?;
                if doc.nodes[b].kind != ShapeKind::Bone || !doc.is_inside(b, root) {
                    return Err(format!("«{bone}» не является костью этого персонажа"));
                }
            }
            let mut slots = HashSet::new();
            for slot in &character.slots {
                if slot.id.is_empty() || !slots.insert(&slot.id) || slot.variants.is_empty() {
                    return Err("Слоты должны иметь уникальные имена и хотя бы один вариант".into());
                }
                let mut variants = HashSet::new();
                let mut empty = false;
                for variant in &slot.variants {
                    if variant.id.is_empty() || !variants.insert(&variant.id) || (variant.groups.is_empty() && empty) {
                        return Err(format!("Повторяется вариант слота «{}»", slot.name));
                    }
                    empty |= variant.groups.is_empty();
                    for group in &variant.groups {
                        let i = doc.index_of(group).ok_or_else(|| format!("Не найдена деталь «{group}»"))?;
                        if doc.nodes[i].kind != ShapeKind::Group || !doc.is_inside(i, root) || scene.sites.get(group).is_none_or(|s| s.generated) {
                            return Err(format!("Деталь «{group}» должна быть обычной группой внутри персонажа"));
                        }
                        if owned.iter().any(|&p| p == i || doc.is_inside(i, p) || doc.is_inside(p, i))
                            || doc.nodes.iter().enumerate().any(|(b, n)| n.kind == ShapeKind::Bone && doc.is_inside(b, i))
                        {
                            return Err(format!("Деталь «{group}» пересекается с другой деталью или содержит кости"));
                        }
                        owned.push(i);
                    }
                }
            }
            let mut look_names = HashSet::new();
            for look in &character.looks {
                if look.parameters.iter().any(|(n, v)| !v.is_finite() || !character.parameters.iter().any(|p| &p.name == n && *v >= p.min && *v <= p.max))
                    || look.palette.iter().any(|(n, v)| !character.palette.iter().any(|p| &p.name == n) || motif_core::Color::from_hex(v).is_none())
                {
                    return Err(format!("Комплект «{}»: некорректные пропорции или цвета", look.name));
                }
                if look.name.trim().is_empty() || !look_names.insert(&look.name) {
                    return Err("Названия комплектов должны быть непустыми и уникальными".into());
                }
                if look.slots.len() != character.slots.len() {
                    return Err(format!("Комплект «{}» должен задавать каждый слот", look.name));
                }
                for slot in &character.slots {
                    if !look.slots.get(&slot.id).is_some_and(|v| slot.variants.iter().any(|part| &part.id == v)) {
                        return Err(format!("Комплект «{}»: неизвестный вариант слота «{}»", look.name, slot.name));
                    }
                }
            }
        }
        Ok(())
    }
}

impl Slot {
    pub fn selected<'a>(&'a self, scene: &Scene) -> Option<&'a Variant> {
        self.variants
            .iter()
            .find(|candidate| self.variants.iter().all(|v| v.groups.iter().all(|id| scene.doc.node(id).is_some_and(|n| n.visible == (v.id == candidate.id)))))
    }
}

/// Change one or all slots atomically; comments, poses, tracks and edited drawings survive.
pub fn apply(src: &str, scene: &Scene, root: &str, choices: &BTreeMap<String, String>, base: Option<&Path>) -> Result<String, String> {
    let catalog = read(src)?.ok_or("В сцене нет описания персонажа")?;
    catalog.validate(scene)?;
    let character = catalog.characters.iter().find(|c| c.root == root).ok_or("Персонаж не найден")?;
    let mut changes = Vec::new();
    for (slot_id, variant_id) in choices {
        let slot = character.slots.iter().find(|s| &s.id == slot_id).ok_or("Неизвестный слот")?;
        if !slot.variants.iter().any(|v| &v.id == variant_id) {
            return Err(format!("Неизвестный вариант слота «{}»", slot.name));
        }
        for variant in &slot.variants {
            for group in &variant.groups {
                changes.push((group, &variant.id != variant_id));
            }
        }
    }
    let mut out = src.to_string();
    for (id, hidden) in changes {
        let current = parse_in(&out, base).map_err(|e| e[0].message.clone())?;
        out = set_style(&out, &current, id, "hidden", hidden.then_some("hidden")).ok_or_else(|| format!("Деталь «{id}» не изменяется"))?;
    }
    let final_scene = parse_in(&out, base).map_err(|e| e[0].message.clone())?;
    catalog.validate(&final_scene)?;
    Ok(out)
}

/// Duplicate a static appearance in place, preserving each part's parent and local space.
/// Fresh ids isolate edits; the new variant is selected without replacing the original.
pub fn copy_variant(src: &str, scene: &Scene, root: &str, slot_id: &str, base: Option<&Path>) -> Result<(String, Vec<String>), String> {
    let mut catalog = read(src)?.ok_or("В сцене нет описания персонажа")?;
    catalog.validate(scene)?;
    let character = catalog.characters.iter_mut().find(|c| c.root == root).ok_or("Персонаж не найден")?;
    let slot = character.slots.iter_mut().find(|s| s.id == slot_id).ok_or("Неизвестный слот")?;
    let active = slot.selected(scene).ok_or("Сначала выберите один вариант слота")?;
    if active.groups.is_empty() {
        return Err("Выберите непустую деталь, чтобы создать свою копию".into());
    }
    let mut taken: HashSet<_> = scene.doc.nodes.iter().map(|n| n.id.clone()).collect();
    let mut edits = Vec::new();
    let mut new_groups = Vec::new();
    let mut override_copies = Vec::new();
    for group in &active.groups {
        let i = scene.doc.index_of(group).ok_or("Деталь не найдена")?;
        let members: Vec<_> = scene.doc.nodes.iter().enumerate().filter(|(n, _)| *n == i || scene.doc.is_inside(*n, i)).map(|(_, n)| &n.id).collect();
        if members.iter().any(|id| {
            scene.sites.get(*id).is_none_or(|s| s.generated)
                || scene.doc.node(id).is_some_and(|n| n.skin.is_some() || n.morph.is_some())
                || scene.anims.values().any(|a| a.tracks.iter().any(|t| &t.node == *id))
                || scene.driver_sites.values().any(|tracks| tracks.iter().any(|t| &t.node == *id))
                || scene.corrective_sites.values().any(|site| site.tracks.iter().any(|t| &t.node == *id))
        }) {
            return Err("Копия варианта пока доступна для обычных деталей без своих дорожек, связей параметров, циклов, весов и морфинга; используйте общую опору вне детали".into());
        }
        let snippet = copy_nodes(scene, src, std::slice::from_ref(group)).into_iter().next().ok_or("Деталь не копируется")?;
        let mut text = snippet.text;
        let mut renames = Vec::new();
        for (k, (span, id, _)) in snippet.ids.into_iter().enumerate() {
            let new = (1..).map(|n| format!("{id}_look{n}")).find(|id| taken.insert(id.clone())).expect("unbounded ids");
            if k == 0 {
                new_groups.push(new.clone());
            }
            for (_, entry) in scene.override_entries.iter().filter(|(node, _)| node == &id) {
                let text = &src[entry.start..entry.end];
                let tail = text.strip_prefix(&id).ok_or("Не удалось скопировать переопределение детали")?;
                override_copies.push(format!("  {new}{tail}\n"));
            }
            renames.push((span, new));
        }
        for (span, new) in renames.into_iter().rev() {
            text.replace_range(span.start..span.end, &new);
        }
        let site = &scene.sites[group];
        let start = src[..site.span.start].rfind('\n').map_or(0, |n| n + 1);
        let indent = &src[start..site.span.start];
        let end = src[site.span.end..].find('\n').map_or(src.len(), |n| site.span.end + n + 1);
        edits.push((end, format!("{}{}{}\n", if end == src.len() && !src.ends_with('\n') { "\n" } else { "" }, indent, text)));
    }
    let variant_id = (1..).map(|n| format!("custom{n}")).find(|id| !slot.variants.iter().any(|v| &v.id == id)).expect("unbounded ids");
    let name = format!("Мой вариант {}", variant_id.trim_start_matches("custom"));
    slot.variants.push(Variant { id: variant_id.clone(), name, groups: new_groups.clone() });
    edits.sort_by_key(|(at, _)| std::cmp::Reverse(*at));
    let mut out = src.to_string();
    for (at, text) in edits {
        out.insert_str(at, &text);
    }
    if !override_copies.is_empty() {
        out.push_str("\noverride {\n");
        for line in override_copies {
            out.push_str(&line);
        }
        out.push_str("}\n");
    }
    out = write(&out, &catalog)?;
    let current = parse_in(&out, base).map_err(|e| e[0].message.clone())?;
    out = apply(&out, &current, root, &BTreeMap::from([(slot_id.to_string(), variant_id)]), base)?;
    Ok((out, new_groups))
}

/// Remember a mix of existing variants without duplicating either artwork or animation.
pub fn save_look(src: &str, scene: &Scene, root: &str) -> Result<(String, String), String> {
    let mut catalog = read(src)?.ok_or("В сцене нет описания персонажа")?;
    catalog.validate(scene)?;
    let character = catalog.characters.iter_mut().find(|c| c.root == root).ok_or("Персонаж не найден")?;
    let slots: BTreeMap<_, _> = character
        .slots
        .iter()
        .map(|s| s.selected(scene).map(|v| (s.id.clone(), v.id.clone())).ok_or_else(|| format!("Выберите один вариант в слоте «{}»", s.name)))
        .collect::<Result<_, _>>()?;
    let name = (1..).map(|n| format!("Мой комплект {n}")).find(|name| !character.looks.iter().any(|l| &l.name == name)).expect("unbounded names");
    let current = settings(src, character)?;
    character.looks.push(Look { name: name.clone(), slots, parameters: current.parameters, palette: current.palette });
    catalog.validate(scene)?;
    Ok((write(src, &catalog)?, name))
}

/// Restore the wardrobe plus any captured proportions and semantic colors in one edit.
pub fn apply_look(src: &str, scene: &Scene, root: &str, name: &str, base: Option<&Path>) -> Result<String, String> {
    let catalog = read(src)?.ok_or("В сцене нет описания персонажа")?;
    catalog.validate(scene)?;
    let character = catalog.characters.iter().find(|c| c.root == root).ok_or("Персонаж не найден")?;
    let look = character.looks.iter().find(|l| l.name == name).ok_or("Комплект не найден")?;
    let out = apply(src, scene, root, &look.slots, base)?;
    if look.parameters.is_empty() && look.palette.is_empty() {
        return Ok(out);
    }
    let scene = parse_in(&out, base).map_err(|e| e[0].message.clone())?;
    configure(&out, &scene, root, &Settings { parameters: look.parameters.clone(), palette: look.palette.clone() }, base)
}

/// Fork every equipped part, then remember it as one look. The caller commits once,
/// so failure on any later slot never leaves a half-copied costume in the document.
pub fn copy_look(src: &str, scene: &Scene, root: &str, base: Option<&Path>) -> Result<(String, String), String> {
    let catalog = read(src)?.ok_or("В сцене нет описания персонажа")?;
    catalog.validate(scene)?;
    let character = catalog.characters.iter().find(|c| c.root == root).ok_or("Персонаж не найден")?;
    let mut out = src.to_string();
    for slot in &character.slots {
        let active = slot.selected(scene).ok_or("Сначала выберите один вариант каждого слота")?;
        if !active.groups.is_empty() {
            let current = parse_in(&out, base).map_err(|e| e[0].message.clone())?;
            out = copy_variant(&out, &current, root, &slot.id, base)?.0;
        }
    }
    let current = parse_in(&out, base).map_err(|e| e[0].message.clone())?;
    save_look(&out, &current, root)
}

#[cfg(test)]
mod tests {
    use super::*;
    use motif_core::{Color, Prop, Value};

    const BASE: &str = include_str!("../../../examples/character-side.msc");

    fn scene(src: &str) -> Scene {
        parse_in(src, None).unwrap()
    }

    #[test]
    fn all_looks_share_geometry_rig_and_animation_at_every_sampled_pose() {
        let before = scene(BASE);
        let catalog = read(BASE).unwrap().unwrap();
        catalog.validate(&before).unwrap();
        let character = &catalog.characters[0];
        assert_eq!(character.slots.len(), 8);
        assert_eq!(character.motions.len(), 4);
        assert!(character.motions.iter().all(|m| before.doc.animation(&m.clip).is_some()));
        for look in &character.looks {
            let text = apply(BASE, &before, &character.root, &look.slots, None).unwrap();
            let after = scene(&text);
            assert_eq!(after.doc.animations, before.doc.animations);
            assert_eq!(after.doc.machines, before.doc.machines);
            assert_eq!(after.doc.nodes.len(), before.doc.nodes.len());
            for (a, b) in before.doc.nodes.iter().zip(&after.doc.nodes) {
                assert_eq!(a.geometry, b.geometry);
                assert_eq!(a.local_transform(), b.local_transform());
                assert_eq!(a.parent, b.parent);
            }
            for slot in &character.slots {
                assert_eq!(slot.selected(&after).unwrap().id, look.slots[&slot.id]);
            }
            for (a, b) in before.doc.animations.iter().zip(&after.doc.animations) {
                for frame in 0..=16 {
                    let t = a.duration * frame as f64 / 16.0;
                    let pa = a.pose(&before.doc, t);
                    let pb = b.pose(&after.doc, t);
                    for bone in &character.bones {
                        assert_eq!(pa.world_transform(pa.index_of(bone).unwrap()), pb.world_transform(pb.index_of(bone).unwrap()));
                    }
                }
            }
        }
    }

    #[test]
    fn equipment_changes_only_its_slot_and_survives_reopening() {
        let before = scene(BASE);
        let text = apply(BASE, &before, "hero", &BTreeMap::from([("hand_item".into(), "sword".into())]), None).unwrap();
        let after = scene(&text);
        assert!(after.doc.node("sword").unwrap().visible);
        for (a, b) in before.doc.nodes.iter().zip(&after.doc.nodes) {
            if a.id != "sword" {
                assert_eq!(a.visible, b.visible);
            }
        }
        assert!(text.contains("// Покой и циклы движений."));
        assert_eq!(read(&text).unwrap().unwrap().characters[0].slots[7].selected(&after).unwrap().id, "sword");
        let off = apply(&text, &after, "hero", &BTreeMap::from([("hand_item".into(), "none".into())]), None).unwrap();
        assert!(!scene(&off).doc.node("sword").unwrap().visible);
    }

    #[test]
    fn copying_a_multi_part_variant_preserves_parents_and_creates_independent_editable_ids() {
        let before = scene(BASE);
        let (text, groups) = copy_variant(BASE, &before, "hero", "arms", None).unwrap();
        assert_eq!(groups.len(), 4);
        let after = scene(&text);
        let catalog = read(&text).unwrap().unwrap();
        let slot = catalog.characters[0].slots.iter().find(|s| s.id == "arms").unwrap();
        assert_eq!(slot.selected(&after).unwrap().id, "custom1");
        for (original, copied) in slot.variants[0].groups.iter().zip(&groups) {
            let a = before.doc.node(original).unwrap();
            let b = after.doc.node(copied).unwrap();
            assert_eq!(before.doc.nodes[a.parent.unwrap()].id, after.doc.nodes[b.parent.unwrap()].id);
            assert_eq!(a.local_transform(), b.local_transform());
            assert!(!after.doc.node(original).unwrap().visible);
        }
        assert_eq!(after.doc.animations, before.doc.animations);
        let copied_shape = "upper_front_base_shape_look1";
        let custom = crate::set_prop(&text, &after, copied_shape, Prop::Fill, Value::Color(Color::rgb(255, 0, 0))).unwrap();
        let c = scene(&custom);
        assert_ne!(c.doc.node(copied_shape).unwrap().fill, c.doc.node("upper_front_base_shape").unwrap().fill);
        let other = apply(&custom, &c, "hero", &catalog.characters[0].looks[1].slots, None).unwrap();
        let again = apply(&other, &scene(&other), "hero", &BTreeMap::from([("arms".into(), "custom1".into())]), None).unwrap();
        assert_eq!(scene(&again).doc.node(copied_shape).unwrap().fill, Some(Color::rgb(255, 0, 0)));
        let (second_text, second) = copy_variant(&again, &scene(&again), "hero", "arms", None).unwrap();
        assert!(second.iter().all(|id| !groups.contains(id)));
        assert_eq!(scene(&second_text).doc.node("upper_front_base_shape_look1_look1").unwrap().fill, Some(Color::rgb(255, 0, 0)));
    }

    #[test]
    fn bad_metadata_and_invalid_ownership_are_rejected_before_an_edit() {
        assert!(read("rect a\n").unwrap().is_none());
        assert!(read("// @motif-characters broken\n").is_err());
        assert!(read(&BASE.replace("\"schema\":1", "\"schema\":2")).is_err());
        assert!(read(&format!("{BASE}// @motif-characters {{}}\n")).is_err());
        let before = scene(BASE);
        let choices = BTreeMap::from([("head".into(), "traveler".into())]);
        for bad in [
            BASE.replace("\"groups\":[\"head_base\"]", "\"groups\":[\"missing\"]"),
            BASE.replace("\"groups\":[\"head_base\"]", "\"groups\":[\"hero\"]"),
            BASE.replace("\"groups\":[\"head_base\"]", "\"groups\":[\"head_traveler\"]"),
        ] {
            assert!(apply(&bad, &before, "hero", &choices, None).is_err());
        }
        let mixed = BTreeMap::from([("head".into(), "traveler".into()), ("feet".into(), "missing".into())]);
        assert!(apply(BASE, &before, "hero", &mixed, None).is_err());
    }

    #[test]
    fn copying_empty_or_independently_animated_parts_explains_the_limit() {
        assert!(copy_variant(BASE, &scene(BASE), "hero", "back", None).is_err());
        let animated = format!("{BASE}\nanim Blink 1s {{\n head_base.opacity 0: 1 | 1: 0\n}}\n");
        assert!(copy_variant(&animated, &scene(&animated), "hero", "head", None).is_err());
    }

    #[test]
    fn renaming_a_clip_does_not_disable_the_wardrobe() {
        let src = crate::rename_anim(BASE, &scene(BASE), "Walk", "MyWalk", None).unwrap();
        let catalog = read(&src).unwrap().unwrap();
        let out = apply(&src, &scene(&src), "hero", &catalog.characters[0].looks[1].slots, None).unwrap();
        assert!(scene(&out).doc.node("head_traveler").unwrap().visible);
        assert!(scene(&out).doc.animation("MyWalk").is_some());
    }

    #[test]
    fn a_saved_custom_look_restores_the_mix_after_reopening() {
        let before = scene(BASE);
        let choices = BTreeMap::from([("head".into(), "traveler".into()), ("hand_item".into(), "sword".into())]);
        let mixed = apply(BASE, &before, "hero", &choices, None).unwrap();
        let (saved, name) = save_look(&mixed, &scene(&mixed), "hero").unwrap();
        assert_eq!(name, "Мой комплект 1");
        assert_eq!(scene(&saved).doc, scene(&mixed).doc);
        let catalog = read(&saved).unwrap().unwrap();
        let other = apply(&saved, &scene(&saved), "hero", &catalog.characters[0].looks[0].slots, None).unwrap();
        let own = catalog.characters[0].looks.last().unwrap();
        let back = apply(&other, &scene(&other), "hero", &own.slots, None).unwrap();
        assert_eq!(scene(&back).doc, scene(&mixed).doc);
    }

    #[test]
    fn feet_reach_ik_targets_and_loops_match_at_the_seam() {
        use motif_core::kurbo::Point;
        let scene = scene(BASE);
        for anim in &scene.doc.animations {
            for frame in 0..=32 {
                let pose = anim.pose(&scene.doc, anim.duration * frame as f64 / 32.0);
                for side in ["front", "back"] {
                    let shin = pose.index_of(&format!("shin_{side}_bone")).unwrap();
                    let foot = pose.index_of(&format!("foot_{side}")).unwrap();
                    let tip = pose.world_transform(shin) * pose.nodes[shin].bone_tip();
                    let target = pose.world_transform(foot) * Point::ZERO;
                    // The IK solver stores angles rounded to hundredths of a degree.
                    assert!(tip.distance(target) < 0.05, "{} frame {frame} {side}: {:?} != {:?}", anim.name, tip, target);
                }
            }
            if anim.looping {
                let mut nonloop = anim.clone();
                nonloop.looping = false;
                let a = nonloop.pose(&scene.doc, 0.0);
                let b = nonloop.pose(&scene.doc, anim.duration);
                for i in 0..a.nodes.len() {
                    assert_eq!(a.world_transform(i), b.world_transform(i), "{} / {}", anim.name, a.nodes[i].id);
                }
            }
        }
    }

    #[test]
    fn the_side_character_can_face_left_and_scale_without_losing_foot_contacts() {
        use motif_core::kurbo::Point;
        let scene = scene(BASE);
        for scale in [-1.0, -1.3, 1.3] {
            let mut doc = scene.doc.clone();
            let hero = doc.index_of("hero").unwrap();
            doc.nodes[hero].scale_x = scale;
            doc.nodes[hero].scale_y = 0.9;
            doc.nodes[hero].rotation = 18.0;
            doc.nodes[hero].skew_x = 7.0;
            for anim in &doc.animations {
                for frame in 0..=8 {
                    let pose = anim.pose(&doc, anim.duration * frame as f64 / 8.0);
                    for side in ["front", "back"] {
                        let shin = pose.index_of(&format!("shin_{side}_bone")).unwrap();
                        let foot = pose.index_of(&format!("foot_{side}")).unwrap();
                        let tip = pose.world_transform(shin) * pose.nodes[shin].bone_tip();
                        let target = pose.world_transform(foot) * Point::ZERO;
                        assert!(tip.distance(target) < 0.1, "scale {scale}, {} frame {frame}: distance {}", anim.name, tip.distance(target));
                    }
                }
            }
        }
    }
}
