//! Parsing and source edits for pose parameters and their authored response curves.
use crate::parser::{Stmt, ValueKind};
use crate::{KeySite, Scene, ScriptError, Span, TrackSite};
use motif_core::{Ease, Keyframe, PoseDriver, Prop, Track, Value};
use std::collections::HashSet;

pub(crate) fn evaluate(src: &str, scene: &mut Scene, stmts: &[Stmt]) -> Vec<ScriptError> {
    let mut errors = Vec::new();
    let mut error = |span: Span, text: String| errors.push(ScriptError::at(src, span.start, text));
    for stmt in stmts {
        let Stmt::Parameter { parameter: p, span, value_span, uid } = stmt else { continue };
        if span.start < crate::head_end(src) {
            error(*span, "parameter пишется после artboard: параметры движения принадлежат одному холсту".into());
            continue;
        }
        if p.id == "world" || scene.doc.node(&p.id).is_some() || scene.doc.parameter(&p.id).is_some() {
            error(*span, format!("имя параметра «{}» занято", p.id));
            continue;
        }
        if ![p.min, p.max, p.default, p.value].into_iter().all(f64::is_finite)
            || p.min >= p.max
            || !(p.min..=p.max).contains(&p.default)
            || !(p.min..=p.max).contains(&p.value)
        {
            error(*span, "нужен конечный диапазон min < max, default и value внутри него".into());
            continue;
        }
        if let Err((span, msg)) = crate::eval::give_id(&mut scene.doc.ids, motif_core::IdKind::Parameter, "", &p.id, uid) {
            error(span, msg);
        }
        scene.doc.parameters.push(p.clone());
        scene.parameter_values.insert(p.id.clone(), *value_span);
    }
    let mut targets = HashSet::new();
    for stmt in stmts {
        let Stmt::Driver { parameter, tracks, span } = stmt else { continue };
        if span.start < crate::head_end(src) {
            error(*span, "driver пишется после artboard: связи принадлежат одному холсту".into());
            continue;
        }
        scene.blocks_start = Some(scene.blocks_start.map_or(span.start, |s| s.min(span.start)));
        let Some(p) = scene.doc.parameter(parameter).cloned() else {
            error(*span, format!("нет параметра движения «{parameter}»"));
            continue;
        };
        if scene.driver_sites.contains_key(parameter) {
            error(*span, format!("повторяется driver {parameter}: добавьте связи в существующий блок"));
            continue;
        }
        let mut driver = PoseDriver { parameter: parameter.clone(), tracks: Vec::new() };
        let mut sites = Vec::new();
        for t in tracks {
            let Some(prop) = Prop::from_path_name(&t.prop) else {
                error(t.span, format!("неизвестное свойство «{}»", t.prop));
                continue;
            };
            let Some(node) = scene.doc.node(&t.node) else {
                error(t.span, format!("нет фигуры «{}» для driver", t.node));
                continue;
            };
            if prop.is_world() || prop.is_color() || prop == Prop::Parameter {
                error(t.span, "driver управляет числовыми свойствами фигур, не цветом, миром или другим параметром".into());
                continue;
            }
            if prop == Prop::Morph && node.morph.is_none() {
                error(t.span, "для связи morph у фигуры нужны цели морфинга".into());
                continue;
            }
            if matches!(prop, Prop::WarpX(_) | Prop::WarpY(_)) && node.lattice.is_none() {
                error(t.span, "для связи warp у группы нужна сетка deform".into());
                continue;
            }
            if prop == Prop::Contour && node.forms.is_none() {
                error(t.span, "для связи contour нужны forms".into());
                continue;
            }
            if !targets.insert((t.node.clone(), prop)) {
                error(t.span, format!("{}.{} уже управляется связью: используйте одну связь на свойство", t.node, t.prop));
                continue;
            }
            let mut keys = Vec::new();
            for k in &t.keys {
                let ValueKind::Num(value) = k.value.kind else {
                    error(k.value.span, "опорное значение driver должно быть обычным числом".into());
                    continue;
                };
                if !k.time.is_finite() || k.time < p.min || k.time > p.max || !value.is_finite() {
                    error(k.span, "позиция опоры должна быть в диапазоне параметра, значение — конечным числом".into());
                    continue;
                }
                if prop == Prop::Contour && (value < 0.0 || value.fract() != 0.0 || value as usize > node.forms.as_ref().map_or(0, |f| f.shapes.len())) {
                    error(k.span, "опора contour должна ссылаться на существующую целую форму: 0 — исходный контур".into());
                    continue;
                }
                keys.push(Keyframe { time: k.time, value: Value::Num(value), ease: k.ease.unwrap_or(Ease::Linear) });
            }
            keys.sort_by(|a, b| a.time.total_cmp(&b.time));
            if keys.len() < 2
                || keys.first().is_none_or(|k| k.time != p.min)
                || keys.last().is_none_or(|k| k.time != p.max)
                || keys.windows(2).any(|w| w[0].time == w[1].time)
            {
                error(t.span, "связь должна иметь разные опорные позиции, включая минимум и максимум параметра".into());
            }
            driver.tracks.push(Track { node: t.node.clone(), prop, keys });
            sites.push(TrackSite {
                node: t.node.clone(),
                prop,
                span: Span::new(t.span.start, t.keys.last().map_or(t.span.end, |k| k.span.end)),
                keys: t.keys.iter().map(|k| KeySite { time: k.time, span: k.span, value: k.value.span, time_span: k.time_span, ease: k.ease_span }).collect(),
            });
        }
        scene.doc.drivers.push(driver);
        scene.driver_sites.insert(parameter.clone(), sites);
    }
    errors
}

pub fn set_value(src: &str, scene: &Scene, id: &str, value: f64) -> Option<String> {
    let p = scene.doc.parameter(id)?;
    if !value.is_finite() {
        return None;
    }
    Some(crate::edit::splice(src, *scene.parameter_values.get(id)?, &crate::fmt_num(value.clamp(p.min, p.max))))
}

/// Edit a reusable response, not a clip. Every animation using it sees the correction.
pub fn set_form_value(src: &str, scene: &Scene, parameter: &str, node: &str, prop: Prop, position: f64, value: f64) -> Option<String> {
    if !value.is_finite() {
        return None;
    }
    let track = scene.driver_sites.get(parameter)?.iter().find(|t| t.node == node && t.prop == prop)?;
    let key = track.keys.iter().find(|k| (k.time - position).abs() < 1e-9)?;
    Some(crate::edit::splice(src, key.value, &crate::fmt_num(value)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use motif_core::kurbo::Shape;

    const SIMPLE: &str = "artboard 200x200\nparameter turn \"Поворот\" range -1 1 default 0 value 0\nellipse head at 100 100 size 100\nellipse eye on head lat 10 lon 20 wrap size 15\ndriver turn {\n head.yaw -1: -30 | 0: 0 | 1: 30\n eye.sx -1: 0.8 | 0: 1 | 1: 0.8\n}\nanim Turn 2s {\n turn.value 0: -1 linear | 2: 1\n}\n";
    const PORTRAIT: &str = include_str!("../../../examples/character-portrait.msc");
    fn parse(src: &str) -> Scene {
        crate::parse(src).unwrap_or_else(|e| panic!("{e:?}"))
    }

    #[test]
    fn parameter_tracks_pose_multiple_parts_without_mutation_or_accumulation() {
        let s = parse(SIMPLE);
        let before = s.clone();
        for (time, yaw, sx) in [(0.0, -30.0, 0.8), (0.5, -15.0, 0.9), (1.0, 0.0, 1.0), (2.0, 30.0, 0.8)] {
            let mut pose = s.doc.animations[0].pose(&s.doc, time);
            assert!((pose.node("head").unwrap().yaw - yaw).abs() < 1e-9);
            assert!((pose.node("eye").unwrap().scale_x - sx).abs() < 1e-9);
            let outline = pose.node("eye").unwrap().path().unwrap();
            for _ in 0..3 {
                pose.apply_parameters();
                pose.rig();
            }
            assert_eq!(pose.node("eye").unwrap().path().unwrap(), outline);
        }
        assert_eq!(s, before);
        let mut d = s.doc.clone();
        assert!(d.set_parameter("turn", 20.0));
        assert_eq!(d.parameter("turn").unwrap().value, 1.0);
        assert!(!d.set_parameter("turn", f64::NAN));
        assert!(!d.set_parameter("missing", 1.0));
    }

    #[test]
    fn source_edits_preserve_clips_and_driver_changes_affect_every_clip() {
        let s = parse(SIMPLE);
        let edited = set_value(SIMPLE, &s, "turn", 0.25).unwrap();
        let e = parse(&edited);
        assert_eq!(e.doc.animations, s.doc.animations);
        assert_eq!(e.doc.drivers, s.doc.drivers);
        assert_eq!(e.doc.node("head").unwrap().yaw, 7.5);
        let keyed = crate::set_key(&edited, &e, "Turn", "turn", Prop::Parameter, 0.7, Value::Num(0.4)).unwrap();
        let k = parse(&keyed);
        assert_eq!(k.doc.drivers, s.doc.drivers);
        assert_eq!(k.doc.animations[0].tracks[0].keys.len(), 3);
        let revised = set_form_value(&keyed, &k, "turn", "head", Prop::Yaw, 1.0, 45.0).unwrap();
        let r = parse(&revised);
        assert_eq!(r.doc.animations, k.doc.animations);
        assert_eq!(r.doc.animations[0].pose(&r.doc, 2.0).node("head").unwrap().yaw, 45.0);
        assert_eq!(parse(&revised), r, "saved text reopens identically");
    }

    #[test]
    fn invalid_or_ambiguous_rigs_are_rejected() {
        parse(SIMPLE);
        for bad in [
            SIMPLE.replace("range -1 1", "range 1 -1"),
            SIMPLE.replace("default 0", "default 5"),
            SIMPLE.replace("value 0\n", "value 5\n"),
            SIMPLE.replace("parameter turn", "parameter head"),
            SIMPLE.replace("eye.sx -1: 0.8", "head.yaw -1: 0.8"),
            SIMPLE.replace("head.yaw -1", "missing.yaw -1"),
            SIMPLE.replace("head.yaw -1", "turn.value -1"),
            SIMPLE.replace("head.yaw -1", "head.fill -1"),
            SIMPLE.replace("eye.sx -1", "eye.morph -1"),
            SIMPLE.replace("-1: -30 | 0: 0", "-1: -30 | -1: 0"),
            SIMPLE.replace("-1: -30", "-0.5: -30"),
            SIMPLE.replace("turn.value 0: -1", "turn.yaw 0: -1"),
            SIMPLE.replace("turn.value 0: -1", "turn.value 0: -2"),
            SIMPLE.replace("turn.value 0: -1", "turn.value 0: #ffffff"),
            format!("parameter shared range 0 1 default 0 value 0\n{SIMPLE}"),
            format!("{SIMPLE}\ngroup bad {{\nparameter nested range 0 1 default 0 value 0\n}}\n"),
        ] {
            assert!(crate::parse(&bad).is_err(), "accepted invalid rig: {bad}");
        }
    }

    #[test]
    fn deleting_a_driven_part_removes_its_bindings_and_boards_stay_independent() {
        let s = parse(SIMPLE);
        let out = crate::delete_node(SIMPLE, &s, "eye").unwrap();
        let reduced = parse(&out);
        assert_eq!(reduced.doc.drivers[0].tracks.len(), 1);
        let out = crate::delete_node(&out, &reduced, "head").unwrap();
        assert!(parse(&out).doc.drivers[0].tracks.is_empty());
        let file = format!("{SIMPLE}\nartboard 20x20\nrect keep size 5\n");
        let boards = crate::parse_boards(&file, None).unwrap();
        assert_eq!(boards.len(), 2);
        assert!(boards[1].doc.parameters.is_empty());
    }

    #[test]
    fn machine_blends_controls_before_evaluating_their_non_linear_responses() {
        let src = SIMPLE.replace("head.yaw -1: -30 | 0: 0 | 1: 30", "head.yaw -1: -20 | 0: 0 | 1: 60")
            + "\nanim Left 1s loop {\n turn.value 0: -1\n}\nanim Right 1s loop {\n turn.value 0: 1\n}\nmachine Face {\n input go false\n state Left anim Left\n state Right anim Right\n Left -> Right when go blend 1s\n}\n";
        let s = parse(&src);
        let m = &s.doc.machines[0];
        let mut player = motif_core::MachinePlayer::new(m);
        player.set(m, "go", 1.0);
        player.advance(m, &s.doc, 0.01);
        player.advance(m, &s.doc, 0.5);
        let pose = player.pose(m, &s.doc);
        assert!((pose.parameter("turn").unwrap().value).abs() < 1e-9);
        assert!((pose.node("head").unwrap().yaw).abs() < 1e-9, "must sample the middle form, not average endpoint yaws to 20");
    }

    #[test]
    fn portrait_combinations_stay_finite_and_hat_copies_inherit_the_common_mount() {
        let s = parse(PORTRAIT);
        let catalog = crate::character::read(PORTRAIT).unwrap().unwrap();
        catalog.validate(&s).unwrap();
        assert_eq!(s.doc.parameters.len(), 3);
        for turn in [-1.0, -0.5, 0.0, 0.5, 1.0] {
            for blink in [0.0, 0.65, 1.0] {
                for mouth in [0.0, 0.5, 1.0] {
                    let mut pose = s.doc.clone();
                    for (id, value) in [("turn", turn), ("blink", blink), ("mouth", mouth)] {
                        pose.set_parameter(id, value);
                    }
                    pose.apply_parameters();
                    pose.rig();
                    for (i, n) in pose.nodes.iter().enumerate() {
                        assert!(pose.world_transform(i).as_coeffs().iter().all(|x| x.is_finite()), "{}", n.id);
                        if let Some(path) = n.path() {
                            assert!(path.elements().iter().all(|e| e.is_finite()), "{}", n.id);
                        }
                    }
                    assert_eq!(pose.node("head").unwrap().yaw, 34.0 * turn);
                    assert!(
                        (pose.node("eye_l").unwrap().scale_y
                            - if blink == 0.0 {
                                1.0
                            } else if blink == 1.0 {
                                0.04
                            } else {
                                0.4
                            })
                        .abs()
                            < 1e-9
                    );
                    let root = pose.index_of("hero").unwrap();
                    let bounds = pose.world_bounds(root).unwrap();
                    assert!(bounds.x0 >= 0.0 && bounds.y0 >= 0.0 && bounds.x1 <= 400.0 && bounds.y1 <= 420.0, "{bounds:?}");
                }
            }
        }
        let (copy, ids) = crate::character::copy_variant(PORTRAIT, &s, "hero", "hat", None).unwrap();
        let copied = parse(&copy);
        assert_eq!(copied.doc.animations, s.doc.animations);
        assert_eq!(copied.doc.drivers, s.doc.drivers);
        assert_eq!(copied.doc.node(&ids[0]).unwrap().parent, s.doc.node("hat_red").unwrap().parent);
    }

    #[test]
    fn driven_morphs_update_when_other_parts_have_physics() {
        let src = PORTRAIT.replace("ellipse shadow at", "ellipse shadow spring 3 0.5 at");
        let s = parse(&src);
        let a = s.doc.animation("Look").unwrap();
        for time in [0.0, 1.0, 2.0, 3.0] {
            let pose = a.pose(&s.doc, time);
            let mut expected = a.keys_at(&s.doc, time);
            expected.rig();
            assert_eq!(pose.node("head").unwrap().base_path(), expected.node("head").unwrap().base_path());
            assert!(pose.node("head").unwrap().path().unwrap().bounding_box().width() > 100.0);
        }
    }
}
