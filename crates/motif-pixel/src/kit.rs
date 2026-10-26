//! UI kits: every interface piece of a scene (`ui`, `slice`) rendered on its own in each state,
//! and a Godot theme that uses the pictures as nine-slice styles.
//!
//! States are animations named after them: `Hover`, `Press`, `Disabled`, `Focus` (and `Normal`
//! or `Idle`, else the scene as written). A state is the pose at the end of its animation, so the
//! same animations also play the transitions in the editor. A piece that a state does not change
//! gets no picture for it.

use motif_core::kurbo::{Rect, Shape};
use motif_core::{Document, ShapeKind};
use motif_render::{CpuOptions, render_region};
use std::path::{Path, PathBuf};
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

/// One interface piece, rendered.
pub struct KitItem {
    pub id: String,
    /// Godot state name and its picture; `normal` first. All pictures have the same size.
    pub states: Vec<(&'static str, Pixmap)>,
    /// Nine-slice margins in pixels from the picture's edges: left, top, right, bottom.
    pub margins: Option<[u32; 4]>,
    /// How far the picture reaches past the piece's box (shadows), in pixels: the control's
    /// rect is the box, the rest draws outside it.
    pub expand: [u32; 4],
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
        .filter(|&k| doc.nodes[k].kind != ShapeKind::Bone && !masked(k))
        .filter_map(|k| doc.nodes[k].path().map(|p| doc.world_transform(k).transform_rect_bbox(p.bounding_box())))
        .reduce(|a, b| a.union(b))
}

/// The scene with only node `i` (and the groups around it) showing.
fn alone(doc: &Document, i: usize) -> Document {
    let mut d = doc.clone();
    for k in 0..d.nodes.len() {
        let keep = k == i || doc.is_inside(k, i) || doc.is_inside(i, k);
        if !keep {
            d.nodes[k].visible = false;
        }
    }
    d
}

/// Renders every interface piece of `doc` in every state.
pub fn render_kit(doc: &Document, opts: CpuOptions) -> Result<Vec<KitItem>, String> {
    let opts = CpuOptions { background: false, ..opts };
    let poses = state_poses(doc);
    let mut items = Vec::new();
    for i in 0..doc.nodes.len() {
        let n = &doc.nodes[i];
        if !(n.ui || n.slice.is_some()) || !doc.is_visible(i) {
            continue;
        }
        // The box is measured in the normal state: that is the size the game lays out.
        let Some(bx) = piece_box(&poses[0].1, i) else { continue };
        let region = bx.inflate(REACH, REACH);
        let mut pics: Vec<(&'static str, Pixmap)> = Vec::new();
        for (state, pose) in &poses {
            let px = render_region(&alone(pose, i), region, opts).ok_or("не удалось создать изображение")?;
            // A state that changes nothing on this piece needs no picture of its own.
            if pics.first().is_some_and(|(_, first)| first.data() == px.data()) {
                continue;
            }
            pics.push((state, px));
        }
        // One crop for all states, around everything any of them draws and the box itself.
        let (w, h) = (pics[0].1.width(), pics[0].1.height());
        let to_px = |v: f64| (v * opts.scale).round() as i64;
        let box_px = [to_px(bx.x0 - region.x0), to_px(bx.y0 - region.y0), to_px(bx.x1 - region.x0), to_px(bx.y1 - region.y0)];
        let mut crop = box_px;
        for (_, p) in &pics {
            if let Some([x0, y0, x1, y1]) = opaque_bounds(p) {
                crop = [crop[0].min(x0), crop[1].min(y0), crop[2].max(x1), crop[3].max(y1)];
            }
        }
        let crop = [crop[0].max(0), crop[1].max(0), crop[2].min(w as i64), crop[3].min(h as i64)];
        let states =
            pics.into_iter().map(|(s, p)| cut(&p, crop).map(|c| (s, c)).ok_or("пустой элемент интерфейса".to_string())).collect::<Result<Vec<_>, _>>()?;
        let expand = [box_px[0] - crop[0], box_px[1] - crop[1], crop[2] - box_px[2], crop[3] - box_px[3]].map(|v| v.max(0) as u32);
        let margins = n.slice.map(|s| [0, 1, 2, 3].map(|k| (s[k] * opts.scale).round().max(0.0) as u32 + expand[k]));
        items.push(KitItem { id: n.id.clone(), states, margins, expand });
    }
    if items.is_empty() {
        return Err("в сцене нет элементов интерфейса: отметьте их ui или задайте slice".into());
    }
    Ok(items)
}

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
const GODOT_CLASSES: [&str; 24] = [
    "Button",
    "CheckBox",
    "CheckButton",
    "ColorRect",
    "Container",
    "Control",
    "HBoxContainer",
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
    "ScrollContainer",
    "TextureButton",
    "TextureRect",
    "VBoxContainer",
    "VSlider",
];

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

/// Writes the pictures (`id_state.png`) into `dir` and a Godot 4 theme `<theme>.tres` next to
/// them: each piece is a type variation, of `Button` when it has states and of `PanelContainer`
/// otherwise, styled by its pictures as nine-slice `StyleBoxTexture`s. Returns the files written.
pub fn write_kit(dir: &Path, theme: &str, items: &[KitItem]) -> Result<Vec<PathBuf>, String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let mut files = Vec::new();
    let mut ext = String::new();
    let mut subs = String::new();
    let mut props = String::new();
    let mut n = 0;
    for item in items {
        let ty = type_name(&item.id);
        let button = item.states.len() > 1;
        props.push_str(&format!("{ty}/base_type = &\"{}\"\n", if button { "Button" } else { "PanelContainer" }));
        let mut style_of = Vec::new();
        for (state, px) in &item.states {
            let name = format!("{}_{state}.png", item.id);
            let path = dir.join(&name);
            px.save_png(&path).map_err(|e| format!("{}: {e}", path.display()))?;
            files.push(path);
            n += 1;
            ext.push_str(&format!("[ext_resource type=\"Texture2D\" path=\"{name}\" id=\"{n}_tex\"]\n"));
            subs.push_str(&format!("\n[sub_resource type=\"StyleBoxTexture\" id=\"{n}_style\"]\ntexture = ExtResource(\"{n}_tex\")\n"));
            if let Some(m) = item.margins {
                for (side, v) in ["left", "top", "right", "bottom"].iter().zip(m) {
                    subs.push_str(&format!("texture_margin_{side} = {v}.0\n"));
                }
            }
            for (side, v) in ["left", "top", "right", "bottom"].iter().zip(item.expand) {
                if v > 0 {
                    subs.push_str(&format!("expand_margin_{side} = {v}.0\n"));
                }
            }
            style_of.push((*state, n));
        }
        if button {
            // A state without its own picture looks normal.
            let normal = style_of[0].1;
            for state in ["normal", "hover", "pressed", "disabled", "focus"] {
                let id = style_of.iter().find(|s| s.0 == state).map_or(normal, |s| s.1);
                if state == "focus" && !style_of.iter().any(|s| s.0 == "focus") {
                    continue;
                }
                props.push_str(&format!("{ty}/styles/{state} = SubResource(\"{id}_style\")\n"));
            }
        } else {
            props.push_str(&format!("{ty}/styles/panel = SubResource(\"{}_style\")\n", style_of[0].1));
        }
    }
    let text = format!("[gd_resource type=\"Theme\" load_steps={} format=3]\n\n{ext}{subs}\n[resource]\n{props}", n * 2 + 1);
    let path = dir.join(format!("{theme}.tres"));
    std::fs::write(&path, text).map_err(|e| format!("{}: {e}", path.display()))?;
    files.push(path);
    Ok(files)
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
        assert_eq!(b.states.iter().map(|s| s.0).collect::<Vec<_>>(), ["normal", "hover"]);
        assert_eq!((b.states[0].1.width(), b.states[0].1.height()), (240, 80));
        assert_eq!(b.margins, Some([28; 4]));
        // The panel's hover would look the same: no picture. Its shadow reaches 12 px below.
        let p = &items[1];
        assert_eq!(p.states.len(), 1);
        assert_eq!(p.expand, [0, 0, 0, 12]);
        assert_eq!(p.margins, Some([20, 40, 20, 32]));
        assert_eq!(p.states[0].1.height(), 172);
    }

    #[test]
    fn the_theme_points_at_the_pictures() {
        let items = render_kit(&kit(), CpuOptions::default()).unwrap();
        let dir = std::env::temp_dir().join(format!("motif-kit-test-{}", std::process::id()));
        let files = write_kit(&dir, "kit", &items).unwrap();
        assert_eq!(files.len(), 4);
        let theme = std::fs::read_to_string(dir.join("kit.tres")).unwrap();
        assert!(theme.contains("PlayButton/base_type = &\"Button\""), "{theme}");
        assert!(theme.contains("PlayButton/styles/hover = SubResource(\"2_style\")"), "{theme}");
        assert!(theme.contains("PlayButton/styles/pressed = SubResource(\"1_style\")"), "{theme}");
        assert!(theme.contains("Card/styles/panel = SubResource(\"3_style\")"), "{theme}");
        assert!(theme.contains("path=\"card_normal.png\""), "{theme}");
        assert!(theme.contains("expand_margin_bottom = 6.0"), "{theme}");
        std::fs::remove_dir_all(dir).ok();
    }
}
