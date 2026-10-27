//! UI kits: every interface piece of a scene (`ui`, `slice`) rendered on its own in each state,
//! and a Godot theme that uses the pictures as nine-slice styles.
//!
//! States are animations named after them: `Hover`, `Press`, `Disabled`, `Focus` (and `Normal`
//! or `Idle`, else the scene as written). A state is the pose at the end of its animation, so the
//! same animations also play the transitions in the editor. A piece that a state does not change
//! gets no picture for it.
//!
//! `ui progress`, `ui check`, `ui slider`, `ui scroll` and `ui label` style the Godot control of
//! that kind from the parts inside the piece (`part fill`, `part grabber`…); a `part text` is
//! not baked: Godot draws it with the font, size and per-state colors the theme gets.

use motif_core::kurbo::{Rect, Shape};
use motif_core::{Color, Document, ShapeKind, UiPart, UiRole};
use motif_render::{CpuOptions, render_region};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tiny_skia::Pixmap;

/// The states Godot's controls know, with the animation names that set them.
pub const STATES: [(&str, &[&str]); 5] = [
    ("normal", &["normal", "idle"]),
    ("hover", &["hover"]),
    ("pressed", &["press", "pressed", "down"]),
    ("disabled", &["disabled"]),
    ("focus", &["focus", "focused"]),
];

/// How far past its box a piece is looked at for shadows, glows and strokes, in artboard units.
const REACH: f64 = 200.0;

/// One picture of a piece: a state of it, or of one of its parts.
pub struct KitPicture {
    /// What the file is called after the piece's id: `normal`, `fill`, `grabber_hover`.
    pub name: String,
    pub px: Pixmap,
    /// Nine-slice margins in pixels from the picture's edges: left, top, right, bottom.
    pub margins: Option<[u32; 4]>,
    /// How far the picture reaches past the rect Godot gives it (shadows), in pixels.
    pub expand: [u32; 4],
}

/// One theme property of a piece.
pub enum Slot {
    /// `styles/<prop>`: a picture as a nine-slice `StyleBoxTexture`, or nothing (`StyleBoxEmpty`).
    /// `content` keeps the caption where it was drawn: left, top, right, bottom in pixels.
    Style { prop: &'static str, picture: Option<usize>, content: Option<[u32; 4]> },
    /// `icons/<prop>`: a picture drawn at its own size (a check mark, a slider's grabber).
    Icon { prop: &'static str, picture: usize },
    /// `constants/<prop>`.
    Constant { prop: &'static str, value: i64 },
}

/// The caption of a piece, drawn by Godot: its font, size and colors.
pub struct KitFont {
    /// The font file, saved next to the theme.
    pub file: String,
    pub data: Arc<Vec<u8>>,
    /// Pixels, at the export scale.
    pub size: u32,
    /// `colors/<name>`: `font_color`, `font_hover_color`…
    pub colors: Vec<(&'static str, Color)>,
    /// Outline color and width in pixels.
    pub outline: Option<(Color, u32)>,
    /// Shadow color and offset in pixels (Label only).
    pub shadow: Option<(Color, [i64; 2])>,
}

/// One interface piece, rendered, with the Godot control it styles.
pub struct KitItem {
    pub id: String,
    /// The Godot class the type variation is based on: `Button`, `ProgressBar`, `HSlider`…
    pub base: &'static str,
    pub pictures: Vec<KitPicture>,
    pub slots: Vec<Slot>,
    pub font: Option<KitFont>,
}

/// The poses of the scene for each state that has an animation, `normal` first.
pub fn state_poses(doc: &Document) -> Vec<(&'static str, Document)> {
    let mut out = Vec::new();
    for (state, names) in STATES {
        let anim = doc.animations.iter().find(|a| names.contains(&a.name.to_lowercase().as_str()));
        let pose = match anim {
            Some(a) => a.pose(doc, if a.looping { 0.0 } else { a.duration }),
            None if state == "normal" => {
                let mut d = doc.clone();
                d.rig();
                d
            }
            None => continue,
        };
        out.push((state, pose));
    }
    out
}

/// The box of node `i` and everything inside it, on the artboard: what the game lays out.
pub fn piece_box(doc: &Document, i: usize) -> Option<Rect> {
    piece_box_except(doc, i, &[])
}

/// [`piece_box`] leaving out the nodes in `skip` and what is inside them.
fn piece_box_except(doc: &Document, i: usize, skip: &[usize]) -> Option<Rect> {
    // Inside a masking group only the mask counts: the rest is cut to it.
    let masked = |k: usize| {
        let mut cur = doc.nodes[k].parent;
        let mut child = k;
        while let Some(g) = cur {
            if doc.nodes[g].clip && doc.nodes.iter().position(|n| n.parent == Some(g)) != Some(child) {
                return true;
            }
            if g == i {
                break;
            }
            child = g;
            cur = doc.nodes[g].parent;
        }
        false
    };
    (0..doc.nodes.len())
        .filter(|&k| k == i || doc.is_inside(k, i))
        .filter(|&k| !skip.iter().any(|&s| k == s || doc.is_inside(k, s)))
        .filter(|&k| doc.nodes[k].kind != ShapeKind::Bone && !masked(k))
        .filter_map(|k| doc.nodes[k].path().map(|p| doc.world_transform(k).transform_rect_bbox(p.bounding_box())))
        .reduce(|a, b| a.union(b))
}

/// The scene with only node `i` (and the groups around it) showing, without the nodes in `hide`.
fn alone(doc: &Document, i: usize, hide: &[usize]) -> Document {
    let mut d = doc.clone();
    for k in 0..d.nodes.len() {
        let keep = k == i || doc.is_inside(k, i) || doc.is_inside(i, k);
        if !keep || hide.contains(&k) {
            d.nodes[k].visible = false;
        }
    }
    d
}

/// The piece (`ui`) node `k` is a part of: its nearest ancestor with `ui`.
fn owner(doc: &Document, k: usize) -> Option<usize> {
    let mut up = doc.nodes[k].parent;
    while let Some(p) = up {
        if doc.nodes[p].ui {
            return Some(p);
        }
        up = doc.nodes[p].parent;
    }
    None
}

/// A piece drawn in every state: one crop for all of them.
struct Shot {
    states: Vec<(&'static str, Pixmap)>,
    margins: Option<[u32; 4]>,
    expand: [u32; 4],
}

/// Renders node `show` (without `hide`) in every pose. `frame` is the rect the control gets in
/// the game; the nine-slice margins are `slice` measured from `inner` (the drawn box, which may
/// sit inside the frame, like the fill of a progress bar inside its background).
fn shoot(
    poses: &[(&'static str, Document)],
    show: usize,
    hide: &[usize],
    frame: Rect,
    inner: Rect,
    slice: Option<[f64; 4]>,
    opts: CpuOptions,
) -> Result<Shot, String> {
    // On the artboard's pixel grid, so edges that are crisp on the artboard stay crisp here.
    let r = frame.union(inner).inflate(REACH, REACH);
    let k = opts.scale;
    let region = Rect::new((r.x0 * k).floor() / k, (r.y0 * k).floor() / k, (r.x1 * k).ceil() / k, (r.y1 * k).ceil() / k);
    let mut pics: Vec<(&'static str, Pixmap)> = Vec::new();
    for (state, pose) in poses {
        let px = render_region(&alone(pose, show, hide), region, opts).ok_or("не удалось создать изображение")?;
        // A state that changes nothing here needs no picture of its own.
        if pics.first().is_some_and(|(_, first)| first.data() == px.data()) {
            continue;
        }
        pics.push((state, px));
    }
    let (w, h) = (pics[0].1.width(), pics[0].1.height());
    let to_px = |v: f64| (v * opts.scale).round() as i64;
    let box_px = [to_px(frame.x0 - region.x0), to_px(frame.y0 - region.y0), to_px(frame.x1 - region.x0), to_px(frame.y1 - region.y0)];
    let mut crop = box_px;
    for (_, p) in &pics {
        if let Some([x0, y0, x1, y1]) = opaque_bounds(p) {
            crop = [crop[0].min(x0), crop[1].min(y0), crop[2].max(x1), crop[3].max(y1)];
        }
    }
    let crop = [crop[0].max(0), crop[1].max(0), crop[2].min(w as i64), crop[3].min(h as i64)];
    let states = pics.into_iter().map(|(s, p)| cut(&p, crop).map(|c| (s, c)).ok_or("пустой элемент интерфейса".to_string())).collect::<Result<Vec<_>, _>>()?;
    let expand = [box_px[0] - crop[0], box_px[1] - crop[1], crop[2] - box_px[2], crop[3] - box_px[3]].map(|v| v.max(0) as u32);
    let inset = [inner.x0 - frame.x0, inner.y0 - frame.y0, frame.x1 - inner.x1, frame.y1 - inner.y1].map(|v| to_px(v).max(0) as u32);
    let margins = (slice.is_some() || inset.iter().any(|&v| v > 0))
        .then(|| [0, 1, 2, 3].map(|k| (slice.map_or(0.0, |s| s[k]) * opts.scale).round().max(0.0) as u32 + inset[k] + expand[k]));
    Ok(Shot { states, margins, expand })
}

/// Where a text is laid out (its line box, not the ink of its letters), on the artboard.
fn text_box(doc: &Document, t: usize) -> Option<Rect> {
    let n = &doc.nodes[t];
    Some(doc.world_transform(t).transform_rect_bbox(n.text.as_ref()?.bounds(&n.text_style)))
}

/// Content margins `[l, t, r, b]` in one state.
type StateMargins = (&'static str, [u32; 4]);

/// Godot draws the caption: what it needs from the text node `t` in every pose.
fn caption(
    poses: &[(&'static str, Document)],
    t: usize,
    frame: Rect,
    colors: &[(&str, &'static str)],
    opts: CpuOptions,
) -> Option<(KitFont, Vec<StateMargins>)> {
    let doc = &poses[0].1;
    let node = &doc.nodes[t];
    let shape = node.text.as_ref()?;
    let font = shape.font.clone()?;
    let to_px = |v: f64| (v * opts.scale).round() as i64;
    let mut out_colors = Vec::new();
    let mut content = Vec::new();
    for (state, pose) in poses {
        let n = &pose.nodes[t];
        if let Some(c) = n.fill
            && let Some((_, prop)) = colors.iter().find(|(s, _)| s == state)
        {
            out_colors.push((*prop, Color { a: (c.a as f64 * n.opacity.clamp(0.0, 1.0)).round() as u8, ..c }));
        }
        // Godot centres one line of letters (ascender to descender) in the content rect: margins
        // that put its centre where the text is drawn in this state.
        let tr = pose.world_transform(t);
        let b = tr.transform_rect_bbox(shape.bounds(&n.text_style));
        let k = b.height() / shape.bounds(&n.text_style).height().max(1e-9);
        let (half_w, half_h) = (b.width() / 2.0, (shape.ascender + shape.descender) * n.text_style.size * k / 2.0);
        let c = b.center();
        let m = [c.x - half_w - frame.x0, c.y - half_h - frame.y0, frame.x1 - c.x - half_w, frame.y1 - c.y - half_h].map(|v| to_px(v).max(0) as u32);
        content.push((*state, m));
    }
    let scale = doc.world_transform(t).as_coeffs()[3].abs().max(1e-9);
    let outline = node
        .outlines
        .first()
        .map(|(c, w)| (*c, to_px(w * scale).max(1) as u32))
        .or_else(|| node.stroke.filter(|_| node.stroke_width > 0.0).map(|c| (c, to_px(node.stroke_width * scale).max(1) as u32)));
    let shadow = node.shadow.as_ref().map(|s| (s.color, [to_px(s.dx * scale), to_px(s.dy * scale)]));
    let size = to_px(node.text_style.size * scale).max(1) as u32;
    Some((KitFont { file: font.name, data: font.data, size, colors: out_colors, outline, shadow }, content))
}

/// Content margins `[l, t, r, b]` with the shared part of each pair cut to at most `cap`.
fn snug(m: [u32; 4], cap: Option<[u32; 4]>) -> [u32; 4] {
    let Some(cap) = cap else { return m };
    let (h, v) = (m[0].min(m[2]), m[1].min(m[3]));
    let (ph, pv) = (h.min(cap[0].min(cap[2])), v.min(cap[1].min(cap[3])));
    [m[0] - h + ph, m[1] - v + pv, m[2] - h + ph, m[3] - v + pv]
}

/// Font colours of a button-like control, by state.
const BUTTON_COLORS: [(&str, &str); 5] = [
    ("normal", "font_color"),
    ("hover", "font_hover_color"),
    ("pressed", "font_pressed_color"),
    ("disabled", "font_disabled_color"),
    ("focus", "font_focus_color"),
];

/// Renders every interface piece of `doc` in every state.
pub fn render_kit(doc: &Document, opts: CpuOptions) -> Result<Vec<KitItem>, String> {
    render_pieces(doc, opts, None)
}

/// Renders the piece `id` alone, as [`render_kit`] would.
pub fn render_piece(doc: &Document, id: &str, opts: CpuOptions) -> Result<KitItem, String> {
    render_pieces(doc, opts, Some(id))?.pop().ok_or_else(|| format!("«{id}» — не деталь интерфейса"))
}

fn render_pieces(doc: &Document, opts: CpuOptions, only: Option<&str>) -> Result<Vec<KitItem>, String> {
    let opts = CpuOptions { background: false, ..opts };
    let poses = state_poses(doc);
    let normal = &poses[0].1;
    let mut items = Vec::new();
    for i in 0..doc.nodes.len() {
        let n = &doc.nodes[i];
        if !(n.ui || n.slice.is_some()) || n.part.is_some() || !doc.is_visible(i) || only.is_some_and(|o| o != n.id) {
            continue;
        }
        let parts: Vec<(UiPart, usize)> = (0..doc.nodes.len()).filter_map(|k| doc.nodes[k].part.filter(|_| owner(doc, k) == Some(i)).map(|p| (p, k))).collect();
        let part = |p: UiPart| parts.iter().find(|(q, _)| *q == p).map(|(_, k)| *k);
        let all: Vec<usize> = parts.iter().map(|(_, k)| *k).collect();
        let text = part(UiPart::Text).or((n.ui_role == UiRole::Label && n.kind == ShapeKind::Text).then_some(i));
        let item = |base: &'static str| KitItem { id: n.id.clone(), base, pictures: Vec::new(), slots: Vec::new(), font: None };
        // A part on its own: the picture of each state, framed by `frame`.
        let part_shot = |k: usize, frame: Option<Rect>| -> Result<Option<Shot>, String> {
            let Some(own) = piece_box(normal, k) else { return Ok(None) };
            shoot(&poses, k, &[], frame.unwrap_or(own), own, doc.nodes[k].slice, opts).map(Some)
        };
        let add = |item: &mut KitItem, shot: Shot, name: &str| -> Vec<(&'static str, usize)> {
            let mut out = Vec::new();
            for (state, px) in shot.states {
                let file = if state == "normal" {
                    name.to_string()
                } else if name.is_empty() {
                    state.to_string()
                } else {
                    format!("{name}_{state}")
                };
                item.pictures.push(KitPicture { name: if file.is_empty() { "normal".into() } else { file }, px, margins: shot.margins, expand: shot.expand });
                out.push((state, item.pictures.len() - 1));
            }
            out
        };
        let pick = |shots: &[(&'static str, usize)], state: &str| shots.iter().find(|s| s.0 == state).or(shots.first()).map(|s| s.1);
        match n.ui_role {
            UiRole::Progress | UiRole::Slider | UiRole::Scroll => {
                // The background (track) carries everything that is not another part.
                let (bg_part, fill_part) = match n.ui_role {
                    UiRole::Progress => (UiPart::Background, UiPart::Fill),
                    _ => (UiPart::Track, UiPart::Fill),
                };
                let Some(bg) = part(bg_part) else { return Err(format!("«{}»: нет part {}", n.id, bg_part.keyword())) };
                let Some(frame) = piece_box(normal, bg) else { continue };
                let hide: Vec<usize> = all.iter().copied().filter(|&k| k != bg).collect();
                let base_shot = shoot(&poses, i, &hide, frame, frame, doc.nodes[bg].slice, opts)?;
                let horizontal = frame.width() >= frame.height();
                let base = match (n.ui_role, horizontal) {
                    (UiRole::Progress, _) => "ProgressBar",
                    (UiRole::Slider, true) => "HSlider",
                    (UiRole::Slider, false) => "VSlider",
                    (_, true) => "HScrollBar",
                    (_, false) => "VScrollBar",
                };
                let mut it = item(base);
                let bgs = add(&mut it, base_shot, bg_part.keyword());
                // A slider's track and a scroll bar are as thick as their style's minimum size.
                let thick = |b: Rect| {
                    let (w, h) = ((b.width() * opts.scale).round() as u32, (b.height() * opts.scale).round() as u32);
                    if horizontal { [0, h / 2, 0, h - h / 2] } else { [w / 2, 0, w - w / 2, 0] }
                };
                let track_content = (n.ui_role != UiRole::Progress).then(|| thick(frame));
                let style = |prop, picture, content| Slot::Style { prop, picture, content };
                match n.ui_role {
                    UiRole::Progress => {
                        it.slots.push(style("background", pick(&bgs, "normal"), None));
                        let fill = part(fill_part).ok_or(format!("«{}»: нет part fill", n.id))?;
                        // The fill grows from the start; as wide as the frame when full, keeping its inset at the end too.
                        if let Some(own) = piece_box(normal, fill) {
                            let full = if horizontal {
                                Rect::new(frame.x0, frame.y0, own.x1 + (own.x0 - frame.x0), frame.y1)
                            } else {
                                Rect::new(frame.x0, own.y0 - (frame.y1 - own.y1), frame.x1, frame.y1)
                            };
                            let shot = shoot(&poses, fill, &[], full, own, doc.nodes[fill].slice, opts)?;
                            let fills = add(&mut it, shot, "fill");
                            it.slots.push(style("fill", pick(&fills, "normal"), None));
                        }
                    }
                    UiRole::Slider => {
                        it.slots.push(style("slider", pick(&bgs, "normal"), track_content));
                        if let Some(fill) = part(UiPart::Fill)
                            && let Some(own) = piece_box(normal, fill)
                        {
                            let full = if horizontal {
                                Rect::new(frame.x0, frame.y0, own.x1 + (own.x0 - frame.x0), frame.y1)
                            } else {
                                Rect::new(frame.x0, own.y0 - (frame.y1 - own.y1), frame.x1, frame.y1)
                            };
                            let shot = shoot(&poses, fill, &[], full, own, doc.nodes[fill].slice, opts)?;
                            let fills = add(&mut it, shot, "fill");
                            it.slots.push(style("grabber_area", pick(&fills, "normal"), track_content));
                            it.slots.push(style("grabber_area_highlight", pick(&fills, "hover"), track_content));
                        }
                        let grabber = part(UiPart::Grabber).ok_or(format!("«{}»: нет part grabber", n.id))?;
                        if let Some(shot) = part_shot(grabber, None)? {
                            let gs = add(&mut it, shot, "grabber");
                            for (prop, state) in [("grabber", "normal"), ("grabber_highlight", "hover"), ("grabber_disabled", "disabled")] {
                                if let Some(p) = pick(&gs, state) {
                                    it.slots.push(Slot::Icon { prop, picture: p });
                                }
                            }
                        }
                    }
                    _ => {
                        it.slots.push(style("scroll", pick(&bgs, "normal"), track_content));
                        it.slots.push(style("scroll_focus", pick(&bgs, "focus"), track_content));
                        let grabber = part(UiPart::Grabber).ok_or(format!("«{}»: нет part grabber", n.id))?;
                        // The grabber spans the bar across: its inset from the track is in its picture.
                        if let Some(own) = piece_box(normal, grabber) {
                            let full = if horizontal { Rect::new(own.x0, frame.y0, own.x1, frame.y1) } else { Rect::new(frame.x0, own.y0, frame.x1, own.y1) };
                            let shot = shoot(&poses, grabber, &[], full, own, doc.nodes[grabber].slice, opts)?;
                            let gs = add(&mut it, shot, "grabber");
                            for (prop, state) in [("grabber", "normal"), ("grabber_highlight", "hover"), ("grabber_pressed", "pressed")] {
                                it.slots.push(style(prop, pick(&gs, state), None));
                            }
                        }
                    }
                }
                items.push(it);
            }
            UiRole::Check => {
                let mut it = item("CheckBox");
                for (p, names) in [(UiPart::Unchecked, ["unchecked", "unchecked_disabled"]), (UiPart::Checked, ["checked", "checked_disabled"])] {
                    let k = part(p).ok_or(format!("«{}»: нет part {}", n.id, p.keyword()))?;
                    if let Some(shot) = part_shot(k, None)? {
                        let shots = add(&mut it, shot, p.keyword());
                        it.slots.push(Slot::Icon { prop: names[0], picture: pick(&shots, "normal").unwrap_or(0) });
                        it.slots.push(Slot::Icon { prop: names[1], picture: pick(&shots, "disabled").unwrap_or(0) });
                    }
                }
                let frame = piece_box_except(normal, i, &text.into_iter().collect::<Vec<_>>()).unwrap_or_default();
                let full = text.and_then(|t| text_box(normal, t)).map_or(frame, |b| frame.union(b));
                // Whatever else the piece draws is its background in every state.
                let rest = piece_box_except(normal, i, &all);
                let bgs = match rest {
                    Some(_) => add(&mut it, shoot(&poses, i, &all, full, full, n.slice, opts)?, ""),
                    None => Vec::new(),
                };
                if let Some(t) = text
                    && let Some((font, content)) = caption(&poses, t, full, &BUTTON_COLORS, opts)
                {
                    // The gap between the box and the caption.
                    let icon = part(UiPart::Unchecked).and_then(|k| piece_box(normal, k));
                    if let (Some(icon), Some(tb)) = (icon, text_box(normal, t)) {
                        it.slots.push(Slot::Constant { prop: "h_separation", value: ((tb.x0 - icon.x1) * opts.scale).round().max(0.0) as i64 });
                    }
                    let left = icon.map_or(0, |b| ((b.x0 - full.x0) * opts.scale).round().max(0.0) as u32);
                    // Top and bottom as drawn; Godot centres the box and the caption in what is left.
                    let [_, top, _, bottom] = content.first().map_or([0; 4], |c| c.1);
                    let pad = top.min(bottom);
                    state_styles(&mut it, &bgs, Some([left, pad, 0, pad]));
                    it.font = Some(font);
                } else {
                    state_styles(&mut it, &bgs, None);
                }
                items.push(it);
            }
            UiRole::Label => {
                let t = text.ok_or(format!("«{}»: ui label — это text или группа с part text", n.id))?;
                let mut it = item("Label");
                let line = text_box(normal, t).unwrap_or_default();
                let frame = piece_box_except(normal, i, &[t]).map_or(line, |b| b.union(line));
                if let Some((font, content)) = caption(&poses, t, frame, &[("normal", "font_color")], opts) {
                    let content = content.first().map(|c| c.1);
                    if t != i && piece_box_except(normal, i, &all).is_some() {
                        let shot = shoot(&poses, i, &all, frame, frame, n.slice, opts)?;
                        let bgs = add(&mut it, shot, "");
                        it.slots.push(Slot::Style { prop: "normal", picture: pick(&bgs, "normal"), content });
                    }
                    it.font = Some(font);
                }
                items.push(it);
            }
            role => {
                // Buttons and panels: one picture per state, the caption drawn by Godot.
                let hide: Vec<usize> = all.clone();
                let Some(frame) = piece_box_except(normal, i, &hide) else { continue };
                let shot = shoot(&poses, i, &hide, frame, frame, n.slice, opts)?;
                let button = role == UiRole::Button || (role == UiRole::Auto && (shot.states.len() > 1 || text.is_some()));
                let mut it = item(if button { "Button" } else { "PanelContainer" });
                let pics = add(&mut it, shot, "");
                if !button {
                    it.slots.push(Slot::Style { prop: "panel", picture: pick(&pics, "normal"), content: None });
                    items.push(it);
                    continue;
                }
                let cap = text.and_then(|t| caption(&poses, t, frame, &BUTTON_COLORS, opts));
                // The padding around the caption goes no further than the nine-slice corners, so the
                // button can be narrower than drawn; what differs between sides (and states) stays.
                let corners = it.pictures[0].margins.map(|m| [0, 1, 2, 3].map(|k| m[k].saturating_sub(it.pictures[0].expand[k])));
                let content_of = |state: &str| cap.as_ref().and_then(|(_, c)| c.iter().find(|s| s.0 == state).or(c.first())).map(|s| snug(s.1, corners));
                for (state, prop) in STYLE_STATES {
                    // Focus is drawn over the button: without a picture of its own Godot's outline stays.
                    if state == "focus" && !pics.iter().any(|s| s.0 == "focus") {
                        continue;
                    }
                    it.slots.push(Slot::Style { prop, picture: pick(&pics, state), content: content_of(state) });
                }
                if let Some((mut font, _)) = cap {
                    if let Some(c) = font.colors.iter().find(|c| c.0 == "font_pressed_color").map(|c| c.1) {
                        font.colors.push(("font_hover_pressed_color", c));
                    }
                    it.font = Some(font);
                }
                items.push(it);
            }
        }
    }
    if items.is_empty() && only.is_none() {
        return Err("в сцене нет элементов интерфейса: отметьте их ui или задайте slice".into());
    }
    Ok(items)
}

/// The styles of a button-like control from its pictures by state (`StyleBoxEmpty` with none).
fn state_styles(it: &mut KitItem, pics: &[(&'static str, usize)], content: Option<[u32; 4]>) {
    for (state, prop) in STYLE_STATES {
        // Focus is drawn over the control: without a picture of its own Godot's outline stays.
        if state == "focus" && !pics.iter().any(|s| s.0 == "focus") {
            continue;
        }
        let picture = pics.iter().find(|s| s.0 == state).or(pics.first()).map(|s| s.1);
        it.slots.push(Slot::Style { prop, picture, content });
    }
}

/// The styles of a button-like control, with the state whose picture each one shows.
const STYLE_STATES: [(&str, &str); 6] =
    [("normal", "normal"), ("hover", "hover"), ("pressed", "pressed"), ("pressed", "hover_pressed"), ("disabled", "disabled"), ("focus", "focus")];

/// Pixels with any opacity: `[x0, y0, x1, y1)`.
fn opaque_bounds(p: &Pixmap) -> Option<[i64; 4]> {
    let (w, h) = (p.width() as usize, p.height() as usize);
    let px = p.pixels();
    let (mut x0, mut y0, mut x1, mut y1) = (w, h, 0, 0);
    for y in 0..h {
        for x in 0..w {
            if px[y * w + x].alpha() > 0 {
                (x0, y0, x1, y1) = (x0.min(x), y0.min(y), x1.max(x + 1), y1.max(y + 1));
            }
        }
    }
    (x1 > x0).then_some([x0 as i64, y0 as i64, x1 as i64, y1 as i64])
}

fn cut(p: &Pixmap, [x0, y0, x1, y1]: [i64; 4]) -> Option<Pixmap> {
    p.clone_rect(tiny_skia::IntRect::from_ltrb(x0 as i32, y0 as i32, x1 as i32, y1 as i32)?)
}

/// Godot classes a type variation must not be named after: the theme would restyle every one.
const GODOT_CLASSES: [&str; 28] = [
    "Button",
    "CheckBox",
    "CheckButton",
    "ColorRect",
    "Container",
    "Control",
    "HBoxContainer",
    "HScrollBar",
    "HSlider",
    "ItemList",
    "Label",
    "LineEdit",
    "MarginContainer",
    "MenuButton",
    "NinePatchRect",
    "OptionButton",
    "Panel",
    "PanelContainer",
    "ProgressBar",
    "RichTextLabel",
    "ScrollBar",
    "ScrollContainer",
    "Slider",
    "TextureButton",
    "TextureRect",
    "VBoxContainer",
    "VScrollBar",
    "VSlider",
];

/// The pieces of several boards as one kit, in board order. A piece whose id (or Godot type)
/// an earlier board already has gets a number: the second `knob` becomes `knob_2`.
pub fn merge_boards(boards: Vec<Vec<KitItem>>) -> Vec<KitItem> {
    let mut out: Vec<KitItem> = Vec::new();
    for mut item in boards.into_iter().flatten() {
        let taken = |id: &str, out: &[KitItem]| out.iter().any(|o| o.id == id || type_name(&o.id) == type_name(id));
        if taken(&item.id, &out) {
            let base = item.id.clone();
            item.id = (2..).map(|k| format!("{base}_{k}")).find(|id| !taken(id, &out)).expect("some number is free");
        }
        out.push(item);
    }
    out
}

/// `play_button` → `PlayButton`: the name of the piece's type variation in the theme. A name
/// Godot already uses for a control gets `Kit` after it (`button` → `ButtonKit`).
pub fn type_name(id: &str) -> String {
    let name: String = id
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(|w| {
            let mut c = w.chars();
            c.next().map_or(String::new(), |f| f.to_ascii_uppercase().to_string() + c.as_str())
        })
        .collect();
    if GODOT_CLASSES.contains(&name.as_str()) || name.starts_with(|c: char| c.is_ascii_digit()) { format!("{name}Kit") } else { name }
}

/// Writes the pictures (`id_name.png`), the fonts of the captions, and a Godot 4 theme
/// `<theme>.tres` into `dir`: each piece is a type variation of its Godot control, styled by
/// its pictures as nine-slice `StyleBoxTexture`s. Returns the files written.
pub fn write_kit(dir: &Path, theme: &str, items: &[KitItem]) -> Result<Vec<PathBuf>, String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let mut files = Vec::new();
    let mut ext = String::new();
    let mut subs = String::new();
    let mut props = String::new();
    let (mut n_ext, mut n_sub) = (0, 0);
    let mut fonts: Vec<String> = Vec::new();
    let color = |c: Color| format!("Color({}, {}, {}, {})", unit(c.r), unit(c.g), unit(c.b), unit(c.a));
    for item in items {
        let ty = type_name(&item.id);
        props.push_str(&format!("{ty}/base_type = &\"{}\"\n", item.base));
        let mut tex = Vec::new();
        for pic in &item.pictures {
            let name = format!("{}_{}.png", item.id, pic.name);
            let path = dir.join(&name);
            pic.px.save_png(&path).map_err(|e| format!("{}: {e}", path.display()))?;
            files.push(path);
            n_ext += 1;
            ext.push_str(&format!("[ext_resource type=\"Texture2D\" path=\"{name}\" id=\"{n_ext}_tex\"]\n"));
            tex.push(n_ext);
        }
        for slot in &item.slots {
            match slot {
                Slot::Style { prop, picture, content } => {
                    n_sub += 1;
                    match picture {
                        Some(p) => {
                            let pic = &item.pictures[*p];
                            subs.push_str(&format!(
                                "\n[sub_resource type=\"StyleBoxTexture\" id=\"{n_sub}_style\"]\ntexture = ExtResource(\"{}_tex\")\n",
                                tex[*p]
                            ));
                            if let Some(m) = pic.margins {
                                for (side, v) in SIDES.iter().zip(m) {
                                    subs.push_str(&format!("texture_margin_{side} = {v}.0\n"));
                                }
                            }
                            for (side, v) in SIDES.iter().zip(pic.expand) {
                                if v > 0 {
                                    subs.push_str(&format!("expand_margin_{side} = {v}.0\n"));
                                }
                            }
                        }
                        None => subs.push_str(&format!("\n[sub_resource type=\"StyleBoxEmpty\" id=\"{n_sub}_style\"]\n")),
                    }
                    if let Some(c) = content {
                        for (side, v) in SIDES.iter().zip(c) {
                            subs.push_str(&format!("content_margin_{side} = {v}.0\n"));
                        }
                    }
                    props.push_str(&format!("{ty}/styles/{prop} = SubResource(\"{n_sub}_style\")\n"));
                }
                Slot::Icon { prop, picture } => props.push_str(&format!("{ty}/icons/{prop} = ExtResource(\"{}_tex\")\n", tex[*picture])),
                Slot::Constant { prop, value } => props.push_str(&format!("{ty}/constants/{prop} = {value}\n")),
            }
        }
        if let Some(f) = &item.font {
            // One copy of each font, whichever pieces use it.
            let k = match fonts.iter().position(|name| *name == f.file) {
                Some(k) => k,
                None => {
                    let path = dir.join(&f.file);
                    std::fs::write(&path, f.data.as_slice()).map_err(|e| format!("{}: {e}", path.display()))?;
                    files.push(path);
                    fonts.push(f.file.clone());
                    n_ext += 1;
                    ext.push_str(&format!("[ext_resource type=\"FontFile\" path=\"{}\" id=\"font_{}\"]\n", f.file, fonts.len()));
                    fonts.len() - 1
                }
            };
            props.push_str(&format!("{ty}/fonts/font = ExtResource(\"font_{}\")\n{ty}/font_sizes/font_size = {}\n", k + 1, f.size));
            for (name, c) in &f.colors {
                props.push_str(&format!("{ty}/colors/{name} = {}\n", color(*c)));
            }
            if let Some((c, w)) = f.outline {
                props.push_str(&format!("{ty}/colors/font_outline_color = {}\n{ty}/constants/outline_size = {w}\n", color(c)));
            }
            if let Some((c, [x, y])) = f.shadow {
                props.push_str(&format!(
                    "{ty}/colors/font_shadow_color = {}\n{ty}/constants/shadow_offset_x = {x}\n{ty}/constants/shadow_offset_y = {y}\n",
                    color(c)
                ));
            }
        }
    }
    let text = format!("[gd_resource type=\"Theme\" load_steps={} format=3]\n\n{ext}{subs}\n[resource]\n{props}", n_ext + n_sub + 1);
    let path = dir.join(format!("{theme}.tres"));
    std::fs::write(&path, text).map_err(|e| format!("{}: {e}", path.display()))?;
    files.push(path);
    Ok(files)
}

const SIDES: [&str; 4] = ["left", "top", "right", "bottom"];

/// A colour channel as Godot writes it: `0.231373`.
fn unit(v: u8) -> String {
    let s = format!("{:.6}", v as f64 / 255.0);
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use motif_core::{Animation, Artboard, Color, Keyframe, Node, Prop, Track, Value};

    fn kit() -> Document {
        let mut d = Document::new(Artboard::default());
        let mut button = Node::new("play_button", ShapeKind::Rect);
        (button.x, button.y, button.width, button.height, button.radius) = (100.0, 100.0, 120.0, 40.0, 12.0);
        button.fill = Some(Color::rgb(60, 110, 255));
        button.slice = Some([14.0; 4]);
        let mut panel = Node::new("card", ShapeKind::Rect);
        (panel.x, panel.y, panel.width, panel.height) = (300.0, 200.0, 100.0, 80.0);
        panel.fill = Some(Color::rgb(40, 48, 73));
        panel.shadow = Some(motif_core::Shadow { dx: 0.0, dy: 6.0, blur: 0.0, color: Color::BLACK });
        panel.slice = Some([10.0, 20.0, 10.0, 10.0]);
        let deco = Node::new("deco", ShapeKind::Ellipse);
        d.nodes = vec![button, panel, deco];
        let key = |v| Keyframe { time: 0.0, value: Value::Color(v), ease: motif_core::Ease::Linear };
        let mut hover = Animation { name: "Hover".into(), duration: 0.2, looping: false, tracks: Vec::new(), events: Vec::new() };
        hover.tracks.push(Track { node: "play_button".into(), prop: Prop::Fill, keys: vec![key(Color::rgb(100, 150, 255))] });
        d.animations.push(hover);
        d
    }

    #[test]
    fn pieces_get_a_picture_per_state_and_nine_slice_margins() {
        let items = render_kit(&kit(), CpuOptions { scale: 2.0, antialias: false, background: true }).unwrap();
        assert_eq!(items.len(), 2, "the ellipse is not interface");
        let b = &items[0];
        assert_eq!(b.base, "Button");
        assert_eq!(b.pictures.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(), ["normal", "hover"]);
        assert_eq!((b.pictures[0].px.width(), b.pictures[0].px.height()), (240, 80));
        assert_eq!(b.pictures[0].margins, Some([28; 4]));
        // The panel's hover would look the same: no picture. Its shadow reaches 12 px below.
        let p = &items[1];
        assert_eq!((p.base, p.pictures.len()), ("PanelContainer", 1));
        assert_eq!(p.pictures[0].expand, [0, 0, 0, 12]);
        assert_eq!(p.pictures[0].margins, Some([20, 40, 20, 32]));
        assert_eq!(p.pictures[0].px.height(), 172);
    }

    #[test]
    fn the_theme_points_at_the_pictures() {
        let items = render_kit(&kit(), CpuOptions::default()).unwrap();
        let dir = std::env::temp_dir().join(format!("motif-kit-test-{}", std::process::id()));
        let files = write_kit(&dir, "kit", &items).unwrap();
        assert_eq!(files.len(), 4);
        let theme = std::fs::read_to_string(dir.join("kit.tres")).unwrap();
        assert!(theme.contains("PlayButton/base_type = &\"Button\""), "{theme}");
        assert!(theme.contains("PlayButton/styles/hover = SubResource(\"2_style\")\n"), "{theme}");
        assert!(theme.contains("[sub_resource type=\"StyleBoxTexture\" id=\"3_style\"]\ntexture = ExtResource(\"1_tex\")"), "pressed looks normal: {theme}");
        assert!(!theme.contains("PlayButton/styles/focus"), "Godot's focus outline stays: {theme}");
        assert!(theme.contains("Card/styles/panel = SubResource(\"6_style\")"), "{theme}");
        assert!(theme.contains("path=\"card_normal.png\""), "{theme}");
        assert!(theme.contains("expand_margin_bottom = 6.0"), "{theme}");
        std::fs::remove_dir_all(dir).ok();
    }

    fn doc(src: &str) -> Document {
        motif_script::parse(src).unwrap_or_else(|e| panic!("{e:?}")).doc
    }

    #[test]
    fn a_button_caption_is_drawn_by_godot_in_each_state() {
        let d = doc(r#"artboard 300x200
group play at 150 100 ui button slice 12 {
  rect body size 160 48 r 12 fill #3b6cff
  text label content "Играть" font bold font-size 20 fill #ffffff part text
}
anim Hover 0.1s {
  label.fill 0: #ffffff | 0.1: #ffe08a
}
anim Press 0.1s {
  body.y 0: 0 | 0.1: 4
  label.y 0: 0 | 0.1: 4
}
"#);
        let items = render_kit(&d, CpuOptions { scale: 2.0, antialias: true, background: false }).unwrap();
        assert_eq!(items.len(), 1, "the caption is a part, not a piece");
        let b = &items[0];
        assert_eq!(b.base, "Button");
        // The pictures have no letters: hover changes only the caption, so it has no picture.
        assert_eq!(b.pictures.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(), ["normal", "pressed"]);
        let font = b.font.as_ref().unwrap();
        assert_eq!((font.file.as_str(), font.size), ("IBMPlexSans-SemiBold.ttf", 40));
        let color = |name| font.colors.iter().find(|c| c.0 == name).map(|c| c.1);
        assert_eq!(color("font_color"), Some(Color::rgb(255, 255, 255)));
        assert_eq!(color("font_hover_color"), Some(Color::rgb(255, 224, 138)));
        // Pressed moves the caption down 8 px with the body.
        let content = |prop| {
            b.slots.iter().find_map(|s| match s {
                Slot::Style { prop: p, content, .. } if *p == prop => *content,
                _ => None,
            })
        };
        let [l, t, r, bt] = content("normal").unwrap();
        let [pl, pt, pr, pb] = content("pressed").unwrap();
        assert_eq!((l, r), (pl, pr));
        assert!(l.abs_diff(r) <= 1, "centred: {l} {r}");
        assert_eq!((pt, pb), (t + 8, bt - 8));
        let dir = std::env::temp_dir().join(format!("motif-kit-font-{}", std::process::id()));
        write_kit(&dir, "kit", &items).unwrap();
        let theme = std::fs::read_to_string(dir.join("kit.tres")).unwrap();
        assert!(theme.contains("[ext_resource type=\"FontFile\" path=\"IBMPlexSans-SemiBold.ttf\" id=\"font_1\"]"), "{theme}");
        assert!(theme.contains("Play/font_sizes/font_size = 40"), "{theme}");
        assert!(theme.contains("Play/colors/font_hover_color = Color(1, 0.878431, 0.541176, 1)"), "{theme}");
        assert!(dir.join("IBMPlexSans-SemiBold.ttf").exists());
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn pictures_keep_the_artboard_pixel_grid() {
        // A 1 px stroke around edges on half pixels covers whole pixels on the artboard: the
        // picture must not shift it by half a pixel and blur it.
        let d = doc("artboard 100x100\nrect tag at 50.5 50.5 size 20 20 fill #ffffff stroke #000000 1 ui\n");
        let item = render_piece(&d, "tag", CpuOptions { scale: 1.0, antialias: true, background: false }).unwrap();
        let px = &item.pictures[0].px;
        assert_eq!((px.width(), px.height()), (21, 21));
        // No half-covered pixels along the edges (the rasterizer leaves a few nearly full ones).
        let soft: Vec<u8> = px.pixels().iter().map(|p| p.alpha()).filter(|a| *a != 0 && *a < 200).collect();
        assert!(soft.is_empty(), "soft edge pixels: {soft:?}");
    }

    #[test]
    fn composite_pieces_become_their_godot_controls() {
        let d = doc(r#"artboard 400x400
group hp at 200 40 ui progress {
  rect frame size 180 28 r 14 fill #1b2132 slice 14 part background
  rect juice at -20 0 size 120 16 r 8 fill #5ee0a0 slice 8 part fill
}
group sound at 200 120 ui check {
  rect off at -40 0 size 32 32 r 8 fill #1b2132 part unchecked
  rect on at -40 0 size 32 32 r 8 fill #3b6cff part checked
  text caption at 10 0 content "Звук" font-size 18 fill #f4f1ea align left part text
}
group volume at 200 200 ui slider {
  rect rail size 200 10 r 5 fill #1b2132 slice 5 part track
  rect level at -50 0 size 100 10 r 5 fill #3b6cff slice 5 part fill
  ellipse knob size 24 24 fill #f4f1ea part grabber
}
group list at 200 280 ui scroll {
  rect gutter size 12 120 r 6 fill #1b2132 slice 6 part track
  rect thumb at 0 -30 size 8 40 r 4 fill #6f93ff slice 4 part grabber
}
text title at 200 360 content "Меню" font bold font-size 30 fill #ffffff outline #10131d 4 shadow 0 3 0 #000000 ui label
"#);
        let items = render_kit(&d, CpuOptions { scale: 1.0, antialias: true, background: false }).unwrap();
        let bases: Vec<_> = items.iter().map(|i| (i.id.as_str(), i.base)).collect();
        assert_eq!(bases, [("hp", "ProgressBar"), ("sound", "CheckBox"), ("volume", "HSlider"), ("list", "VScrollBar"), ("title", "Label")]);
        let names = |i: &KitItem| i.pictures.iter().map(|p| p.name.clone()).collect::<Vec<_>>();
        let props = |i: &KitItem| {
            i.slots
                .iter()
                .map(|s| match s {
                    Slot::Style { prop, .. } => format!("styles/{prop}"),
                    Slot::Icon { prop, .. } => format!("icons/{prop}"),
                    Slot::Constant { prop, value } => format!("constants/{prop}={value}"),
                })
                .collect::<Vec<_>>()
        };
        let hp = &items[0];
        assert_eq!(names(hp), ["background", "fill"]);
        assert_eq!(props(hp), ["styles/background", "styles/fill"]);
        // The fill sits 10 px inside the frame at the ends and 6 px at the edges: a transparent
        // border of its picture, inside the nine-slice corners.
        let fill = &hp.pictures[1];
        assert_eq!((fill.px.width(), fill.px.height()), (140, 28));
        assert_eq!(fill.margins, Some([18, 14, 18, 14]));
        let sound = &items[1];
        assert_eq!(names(sound), ["unchecked", "checked"]);
        assert!(props(sound).contains(&"icons/checked".to_string()));
        assert!(props(sound).contains(&"constants/h_separation=34".to_string()), "{:?}", props(sound));
        assert!(sound.slots.iter().any(|s| matches!(s, Slot::Style { prop: "normal", picture: None, .. })), "nothing behind the box");
        assert_eq!(sound.font.as_ref().unwrap().size, 18);
        let volume = &items[2];
        assert_eq!(names(volume), ["track", "fill", "grabber"]);
        assert_eq!(
            props(volume),
            ["styles/slider", "styles/grabber_area", "styles/grabber_area_highlight", "icons/grabber", "icons/grabber_highlight", "icons/grabber_disabled"]
        );
        // A track is as thick as its style's minimum size.
        assert!(volume.slots.iter().any(|s| matches!(s, Slot::Style { prop: "slider", content: Some([0, 5, 0, 5]), .. })));
        let list = &items[3];
        assert_eq!(props(list), ["styles/scroll", "styles/scroll_focus", "styles/grabber", "styles/grabber_highlight", "styles/grabber_pressed"]);
        assert_eq!(list.pictures[1].px.width(), 12, "the grabber spans the bar");
        let title = &items[4];
        assert!(title.pictures.is_empty());
        let font = title.font.as_ref().unwrap();
        assert_eq!(font.outline, Some((Color::rgb(16, 19, 29), 4)));
        assert_eq!(font.shadow, Some((Color::BLACK, [0, 3])));
    }
    #[test]
    fn boards_share_one_kit_without_clashing_names() {
        let a = render_kit(&kit(), CpuOptions::default()).unwrap();
        let b = render_kit(&kit(), CpuOptions::default()).unwrap();
        let ids: Vec<String> = merge_boards(vec![a, b]).into_iter().map(|i| i.id).collect();
        assert_eq!(ids, ["play_button", "card", "play_button_2", "card_2"]);
        // Ids that differ only in punctuation are one Godot type: numbered too.
        let mut c = render_kit(&kit(), CpuOptions::default()).unwrap();
        c.truncate(1);
        let mut d = render_kit(&kit(), CpuOptions::default()).unwrap();
        d.truncate(1);
        d[0].id = "play-button".into();
        let ids: Vec<String> = merge_boards(vec![c, d]).into_iter().map(|i| i.id).collect();
        assert_eq!(ids, ["play_button", "play-button_2"]);
    }
}
