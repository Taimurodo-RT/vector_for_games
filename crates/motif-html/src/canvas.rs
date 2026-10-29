//! Fit the imported component and its animation envelope without editing object keys.
use motif_core::Animation;
use motif_core::kurbo::{Rect, Shape};
use motif_render::{Op, display_ops, ops_bounds};

const MARGIN: f64 = 24.0;
const MAX_POSES: usize = 720;
const MAX_SIDE: f64 = 8192.0;

fn nonempty(rect: Rect) -> Option<Rect> {
    (rect.x0.is_finite() && rect.y0.is_finite() && rect.x1.is_finite() && rect.y1.is_finite() && rect.width() > 0.0 && rect.height() > 0.0).then_some(rect)
}
fn clipped(rect: Rect, clip: Option<Rect>) -> Option<Rect> {
    nonempty(clip.map_or(rect, |clip| rect.intersect(clip)))
}
fn add(bounds: &mut Option<Rect>, rect: Option<Rect>) {
    if let Some(rect) = rect {
        *bounds = Some(bounds.map_or(rect, |old| old.union(rect)));
    }
}

/// Respect the renderer's nested clips before extending blur layers. In particular,
/// a native inset shadow has its own PushClip and never grows the canvas.
fn paint_bounds(ops: &[Op], inherited: Option<Rect>) -> Option<Rect> {
    let mut bounds = None;
    let mut clip = inherited;
    let mut stack = Vec::new();
    for op in ops {
        match op {
            Op::Draw(_) => add(&mut bounds, ops_bounds(std::slice::from_ref(op)).and_then(|r| clipped(r, clip))),
            Op::PushClip { path, transform, .. } => {
                stack.push(clip);
                let rect = transform.transform_rect_bbox(path.bounding_box());
                // An empty mask stays an empty clip rather than meaning "unclipped".
                clip = Some(clip.map_or(rect, |old| old.intersect(rect)));
            }
            Op::Pop => clip = stack.pop().unwrap_or(inherited),
            Op::Blur { ops, radius } => {
                let rect = paint_bounds(ops, None).and_then(|r| clipped(r.inflate(radius * 1.5, radius * 1.5), clip));
                add(&mut bounds, rect);
            }
        }
    }
    bounds
}

fn times(animation: &Animation, warnings: &mut Vec<String>) -> Vec<f64> {
    let duration = animation.duration.max(0.0);
    let mut keys: Vec<_> =
        animation.tracks.iter().flat_map(|t| &t.keys).map(|k| k.time).filter(|t| t.is_finite() && *t >= 0.0 && *t <= duration).chain([0.0, duration]).collect();
    keys.sort_by(f64::total_cmp);
    keys.dedup_by(|a, b| (*a - *b).abs() < 1e-9);
    if keys.len() > MAX_POSES {
        warnings.push(format!(
            "CSS {}: автоматический размер холста проверен по {MAX_POSES} опорным позам; для более плотного клипа проверьте края вручную",
            animation.name
        ));
        return (0..MAX_POSES).map(|i| keys[i * (keys.len() - 1) / (MAX_POSES - 1)]).collect();
    }
    // Key times preserve brief visibility/hold changes; uniform additional samples
    // also catch a rotated shape's extrema between distant authored keys.
    let remaining = MAX_POSES - keys.len();
    let count = ((duration * 60.0).ceil() as usize).max(2).min(remaining);
    if count >= 2 {
        keys.extend((0..count).map(|i| duration * i as f64 / (count - 1) as f64));
    }
    keys.sort_by(f64::total_cmp);
    keys.dedup_by(|a, b| (*a - *b).abs() < 1e-9);
    keys
}

/// Preserve the original layout rectangle as well as all visible vector/effect
/// bounds. A neutral wrapper changes only the canvas offset, leaving IDs, local
/// coordinates, animation keys and UI roles intact.
pub(crate) fn fit(source: &mut String, warnings: &mut Vec<String>) -> Result<(), String> {
    let scene = motif_script::parse(source).map_err(|errors| format!("CSS размер холста: {errors:?}"))?;
    let mut bounds = paint_bounds(&display_ops(&scene.doc), None);
    for animation in &scene.doc.animations {
        let sample_times = times(animation, warnings);
        let mut sampled = animation.clone();
        // Include the last raw key of a loop; pose(duration) would wrap to its start.
        sampled.looping = false;
        for time in sample_times {
            let pose = sampled.pose(&scene.doc, time);
            add(&mut bounds, paint_bounds(&display_ops(&pose), None));
        }
    }
    let Some(mut bounds) = bounds else {
        return Ok(());
    };
    let old = &scene.doc.artboard;
    bounds = bounds.union(Rect::new(MARGIN, MARGIN, (old.width - MARGIN).max(MARGIN), (old.height - MARGIN).max(MARGIN)));
    let dx = (MARGIN - bounds.x0).max(0.0);
    let dy = (MARGIN - bounds.y0).max(0.0);
    let requested_width = (bounds.x1 + dx + MARGIN).ceil().max(old.width);
    let requested_height = (bounds.y1 + dy + MARGIN).ceil().max(old.height);
    if requested_width > MAX_SIDE || requested_height > MAX_SIDE {
        warnings.push(format!("CSS: границы графики и анимации требуют холст больше {MAX_SIDE}; размер ограничен, проверьте выходящие за него части"));
    }
    let width = requested_width.clamp(64.0, MAX_SIDE);
    let height = requested_height.clamp(64.0, MAX_SIDE);
    if dx > 1e-8 || dy > 1e-8 {
        let start = scene.doc.nodes.iter().filter(|n| n.parent.is_none()).filter_map(|n| scene.sites.get(&n.id).map(|s| s.span.start)).min();
        if let Some(start) = start {
            let end = scene.blocks_start.unwrap_or(source.len());
            if start > end {
                return Err("CSS: не удалось выделить объекты для рамки холста".into());
            }
            let mut id = "html_import_canvas".to_owned();
            let mut suffix = 2;
            while scene.doc.node(&id).is_some() || scene.doc.parameters.iter().any(|p| p.id == id) {
                id = format!("html_import_canvas_{suffix}");
                suffix += 1;
            }
            let mut wrapped = source[..start].to_owned();
            wrapped.push_str(&format!("group {id} at {} {} {{\n", crate::num(dx), crate::num(dy)));
            for line in source[start..end].lines() {
                wrapped.push_str("  ");
                wrapped.push_str(line);
                wrapped.push('\n');
            }
            wrapped.push_str("}\n");
            wrapped.push_str(&source[end..]);
            *source = wrapped;
        }
    }
    *source = motif_script::set_artboard_size(source, width, height).ok_or("CSS: не удалось обновить размер холста")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use motif_core::Document;
    fn parse(source: &str) -> Document {
        motif_script::parse(source).unwrap().doc
    }
    #[test]
    fn negative_geometry_and_animation_fit_without_changing_local_properties_or_keys() {
        let mut source="artboard \"Keep\" 100x100 bg #123456 ui\ngroup html_import_canvas {\n rect object at -10 50 size 20 fill #f00\n}\nanim Move 1s {\n object.x 0: -10 | 1: 130\n}\n".to_owned();
        let before = parse(&source);
        let mut warnings = Vec::new();
        fit(&mut source, &mut warnings).unwrap();
        let after = parse(&source);
        assert!(warnings.is_empty(), "{warnings:?}");
        assert!(after.node("html_import_canvas_2").is_some());
        assert_eq!(before.animations, after.animations);
        let mut original = before.node("object").unwrap().clone();
        original.parent = after.node("object").unwrap().parent;
        assert_eq!(original, *after.node("object").unwrap());
        assert_eq!(before.artboard.background, after.artboard.background);
        let mut shift = None;
        for time in [0.0, 0.25, 0.5, 0.75, 1.0] {
            let old = before.animations[0].pose(&before, time);
            let new = after.animations[0].pose(&after, time);
            let a = old.world_bounds(old.index_of("object").unwrap()).unwrap();
            let b = new.world_bounds(new.index_of("object").unwrap()).unwrap();
            let delta = (b.x0 - a.x0, b.y0 - a.y0);
            if let Some(expected) = shift {
                assert_eq!(delta, expected);
            } else {
                shift = Some(delta);
            }
            assert!(b.x0 >= MARGIN && b.y0 >= MARGIN && b.x1 <= after.artboard.width - MARGIN && b.y1 <= after.artboard.height - MARGIN, "{b:?}\n{source}");
        }
    }
    #[test]
    fn ancestor_masks_and_inset_effects_do_not_expand_the_canvas() {
        let mut source =
            "artboard 120x120 ui\ngroup clipped at 60 60 clip {\n rect mask size 40 fill #fff\n rect content size 1000 fill #f00 inset 2 2 100 #000\n}\n"
                .to_owned();
        let mut warnings = Vec::new();
        fit(&mut source, &mut warnings).unwrap();
        let doc = parse(&source);
        assert_eq!((doc.artboard.width, doc.artboard.height), (120.0, 120.0));
        assert!(doc.node("html_import_canvas").is_none());
        let mut inset = "artboard 100x100 ui\nrect box at 50 50 size 30 fill #fff inset 0 0 100 #000\n".to_owned();
        fit(&mut inset, &mut warnings).unwrap();
        let doc = parse(&inset);
        assert_eq!((doc.artboard.width, doc.artboard.height), (100.0, 100.0));
    }
    #[test]
    fn outer_shadow_expands_bounds_and_the_raw_last_loop_key_is_included() {
        let mut source =
            "artboard 100x100 ui\nrect object at 50 50 size 20 fill #fff shadow 20 0 10 #000\nanim Loop 1s loop {\n object.x 0: 50 | 1: -50 hold\n}\n"
                .to_owned();
        let mut warnings = Vec::new();
        fit(&mut source, &mut warnings).unwrap();
        let doc = parse(&source);
        assert!(doc.artboard.width > 170.0, "{source}");
        let mut last = doc.animations[0].clone();
        last.looping = false;
        let pose = last.pose(&doc, 1.0);
        let bounds = paint_bounds(&display_ops(&pose), None).unwrap();
        assert!(bounds.x0 >= MARGIN && bounds.x1 <= doc.artboard.width - MARGIN, "{bounds:?}\n{source}");
    }
    #[test]
    fn excessive_extent_warns_and_empty_scene_keeps_its_header() {
        let mut empty = "artboard \"Empty\" 64x64 bg #123456 ui\n".to_owned();
        let before = empty.clone();
        let mut warnings = Vec::new();
        fit(&mut empty, &mut warnings).unwrap();
        assert_eq!(empty, before);
        let mut huge = "artboard 64x64 ui\nrect big at 9000 50 size 100 fill #fff\n".to_owned();
        fit(&mut huge, &mut warnings).unwrap();
        assert_eq!(parse(&huge).artboard.width, MAX_SIDE);
        assert!(warnings.iter().any(|w| w.contains("8192")));
    }
}
