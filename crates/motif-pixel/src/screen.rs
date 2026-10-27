//! Screens for Godot: an artboard with a menu or a HUD as a scene of `Control` nodes. Interface
//! pieces become the controls their kit theme styles (`Button`, `ProgressBar`…), stacks become
//! `VBoxContainer`/`HBoxContainer` with their gap and padding, anchors become anchors, texts
//! become `Label`s, and the rest of the art becomes `TextureRect`s.

use crate::kit::{self, KitItem};
use motif_core::kurbo::Rect;
use motif_core::{Color, Document, ShapeKind, StackAlign, StackDir, UiPart};
use motif_render::CpuOptions;
use std::path::{Path, PathBuf};

/// Characters Godot does not allow in node names.
fn node_name(s: &str) -> String {
    let n: String = s.chars().filter(|c| !matches!(c, '.' | ':' | '@' | '/' | '"' | '%')).collect();
    if n.trim().is_empty() { "Screen".into() } else { n.trim().to_string() }
}

fn quote(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"").replace('\n', "\\n")
}

fn color(c: Color) -> String {
    let u = |v: u8| {
        let s = format!("{:.6}", v as f64 / 255.0);
        s.trim_end_matches('0').trim_end_matches('.').to_string()
    };
    format!("Color({}, {}, {}, {})", u(c.r), u(c.g), u(c.b), u(c.a))
}

fn num(v: f64) -> String {
    let r = (v * 1000.0).round() / 1000.0;
    if r.fract() == 0.0 { format!("{r:.1}") } else { format!("{r}") }
}

/// A text a `Label` draws as it is: with a gradient, a glow, an inner shadow or grain it stays a
/// picture.
fn plain_text(n: &motif_core::Node) -> bool {
    n.kind == ShapeKind::Text
        && n.part.is_none()
        && n.gradient.is_none()
        && n.stroke_gradient.is_none()
        && n.glow.is_none()
        && n.inset.is_none()
        && n.noise.is_none()
}

/// Where a control sits: on the screen (with the anchor of its object), in a group without a
/// stack (from the group's corner), or in a stack (the container places it).
#[derive(Clone, Copy)]
enum Place {
    Screen,
    Inside(Rect),
    Stacked(StackDir, StackAlign),
}

struct Writer<'a> {
    doc: &'a Document,
    /// The kit piece of each node, with its type variation name in the theme.
    pieces: Vec<Option<(&'a KitItem, String)>>,
    opts: CpuOptions,
    dir: &'a Path,
    file: String,
    ext: String,
    subs: String,
    nodes: String,
    n_ext: usize,
    n_sub: usize,
    fonts: Vec<String>,
    files: Vec<PathBuf>,
}

impl Writer<'_> {
    fn s(&self, v: f64) -> f64 {
        v * self.opts.scale
    }

    fn add_ext(&mut self, kind: &str, path: &str) -> String {
        self.n_ext += 1;
        let id = format!("{}_{}", self.n_ext, kind.to_lowercase());
        self.ext.push_str(&format!("[ext_resource type=\"{kind}\" path=\"{path}\" id=\"{id}\"]\n"));
        id
    }

    fn font(&mut self, f: &motif_core::FontFile) -> Result<String, String> {
        let path = self.dir.join(&f.name);
        if !self.fonts.contains(&f.name) {
            std::fs::write(&path, f.data.as_slice()).map_err(|e| format!("{}: {e}", path.display()))?;
            self.files.push(path);
            self.fonts.push(f.name.clone());
        }
        let name = f.name.clone();
        // One resource per font file.
        if let Some(id) = self
            .ext
            .lines()
            .find(|l| l.contains(&format!("path=\"{name}\"")))
            .and_then(|l| l.split("id=\"").nth(1))
            .map(|s| s.trim_end_matches("\"]").to_string())
        {
            return Ok(id);
        }
        Ok(self.add_ext("FontFile", &name))
    }

    /// The header of a node and where it sits.
    fn open(&mut self, name: &str, kind: &str, parent: &str, place: Place, b: Rect, anchor: Option<(f64, f64)>) {
        self.nodes.push_str(&format!("\n[node name=\"{}\" type=\"{kind}\" parent=\"{parent}\"]\n", quote(name)));
        let r = Rect::new(self.s(b.x0), self.s(b.y0), self.s(b.x1), self.s(b.y1));
        match place {
            Place::Stacked(dir, align) => {
                self.nodes.push_str("layout_mode = 2\n");
                self.nodes.push_str(&format!("custom_minimum_size = Vector2({}, {})\n", num(r.width().round()), num(r.height().round())));
                let flag = match align {
                    StackAlign::Start => 0,
                    StackAlign::Center => 4,
                    StackAlign::End => 8,
                };
                let across = if dir == StackDir::Column { "horizontal" } else { "vertical" };
                self.nodes.push_str(&format!("size_flags_{across} = {flag}\n"));
            }
            Place::Inside(o) => {
                let (x, y) = (self.s(o.x0), self.s(o.y0));
                self.nodes.push_str("layout_mode = 1\n");
                for (side, v) in [("left", r.x0 - x), ("top", r.y0 - y), ("right", r.x1 - x), ("bottom", r.y1 - y)] {
                    self.nodes.push_str(&format!("offset_{side} = {}\n", num(v.round())));
                }
            }
            Place::Screen => {
                let (h, v) = anchor.unwrap_or((0.0, 0.0));
                let (w, ht) = (self.s(self.doc.artboard.width), self.s(self.doc.artboard.height));
                self.nodes.push_str("layout_mode = 1\n");
                for (side, a) in [("left", h), ("top", v), ("right", h), ("bottom", v)] {
                    if a != 0.0 {
                        self.nodes.push_str(&format!("anchor_{side} = {}\n", num(a)));
                    }
                }
                for (side, val) in [("left", r.x0 - h * w), ("top", r.y0 - v * ht), ("right", r.x1 - h * w), ("bottom", r.y1 - v * ht)] {
                    self.nodes.push_str(&format!("offset_{side} = {}\n", num(val.round())));
                }
                // A centred control grows both ways, one at the right or bottom grows back.
                let grow = |a: f64| {
                    if a > 0.75 {
                        0
                    } else if a > 0.25 {
                        2
                    } else {
                        1
                    }
                };
                if grow(h) != 1 {
                    self.nodes.push_str(&format!("grow_horizontal = {}\n", grow(h)));
                }
                if grow(v) != 1 {
                    self.nodes.push_str(&format!("grow_vertical = {}\n", grow(v)));
                }
            }
        }
    }

    fn path(parent: &str, name: &str) -> String {
        if parent == "." { name.to_string() } else { format!("{parent}/{name}") }
    }

    /// Whether the node or anything inside it is interface: then it is kept as controls.
    fn live(&self, k: usize) -> bool {
        let d = self.doc;
        (0..d.nodes.len()).filter(|&j| j == k || d.is_inside(j, k)).any(|j| self.pieces[j].is_some() || d.nodes[j].stack.is_some() || plain_text(&d.nodes[j]))
    }

    fn children(&self, k: usize) -> Vec<usize> {
        (0..self.doc.nodes.len()).filter(|&j| self.doc.nodes[j].parent == Some(k) && self.doc.is_visible(j)).collect()
    }

    fn emit(&mut self, k: usize, parent: &str, place: Place) -> Result<(), String> {
        let d = self.doc;
        let n = &d.nodes[k];
        let name = node_name(&n.id);
        let anchor = n.anchor;
        if let Some(stack) = n.stack {
            return self.stack(k, stack, parent, place);
        }
        if let Some((item, ty)) = self.pieces[k].clone() {
            let b = kit::control_box(d, k).unwrap_or_default();
            self.open(&name, item.base, parent, place, b, anchor);
            self.nodes.push_str(&format!("theme_type_variation = &\"{ty}\"\n"));
            let text = (0..d.nodes.len())
                .find(|&j| (d.nodes[j].part == Some(UiPart::Text) && kit::owner(d, j) == Some(k)) || (j == k && n.kind == ShapeKind::Text))
                .and_then(|j| d.nodes[j].text.as_ref().map(|t| t.content.clone()));
            if let Some(t) = text.filter(|_| matches!(item.base, "Button" | "CheckBox" | "Label")) {
                self.nodes.push_str(&format!("text = \"{}\"\n", quote(&t)));
            }
            if item.base == "Label" {
                self.nodes.push_str("horizontal_alignment = 1\nvertical_alignment = 1\n");
            }
            let state = self.state(k, item, b);
            self.nodes.push_str(&state);
            return Ok(());
        }
        if plain_text(n) {
            return self.label(k, parent, place);
        }
        if matches!(n.kind, ShapeKind::Group | ShapeKind::Bone) && self.live(k) {
            // A plain group: its children keep their places inside it.
            let Some(b) = kit::piece_box(d, k) else { return Ok(()) };
            self.open(&name, "Control", parent, place, b, anchor);
            let me = Self::path(parent, &name);
            for c in self.children(k) {
                self.emit(c, &me, Place::Inside(b))?;
            }
            return Ok(());
        }
        // Art: a picture of the object as drawn.
        let Some((px, at)) = kit::picture(d, k, self.opts) else { return Ok(()) };
        let file = format!("{}_{}.png", self.file, n.id);
        let path = self.dir.join(&file);
        px.save_png(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        self.files.push(path);
        let tex = self.add_ext("Texture2D", &file);
        self.open(&name, "TextureRect", parent, place, at, anchor);
        self.nodes.push_str(&format!("mouse_filter = 2\ntexture = ExtResource(\"{tex}\")\nstretch_mode = 3\n"));
        Ok(())
    }

    /// What the control shows as drawn: how full a bar is, where a slider or a scroll bar
    /// stands, whether a box is ticked.
    fn state(&self, k: usize, item: &KitItem, frame: Rect) -> String {
        let d = self.doc;
        let part =
            |p: UiPart| (0..d.nodes.len()).find(|&j| d.nodes[j].part == Some(p) && kit::owner(d, j) == Some(k) && d.is_visible(j) && d.nodes[j].opacity > 0.0);
        let scale = self.opts.scale;
        // A picture's size and padding in artboard units, without what it draws outside.
        let picture = |slot: &str| {
            item.slots
                .iter()
                .find_map(|s| match s {
                    kit::Slot::Style { prop, picture: Some(p), content } if *prop == slot => Some((p, content)),
                    kit::Slot::Icon { prop, picture } if *prop == slot => Some((picture, &None)),
                    _ => None,
                })
                .map(|(&p, content)| {
                    let pic = &item.pictures[p];
                    let e = pic.expand.map(|v| v as f64);
                    let size = (pic.px.width() as f64 - e[0] - e[2], pic.px.height() as f64 - e[1] - e[3]);
                    let pad =
                        content.map(|c| c.map(|v| v as f64)).or(pic.margins.map(|m| [0, 1, 2, 3].map(|i| (m[i] as f64 - e[i]).max(0.0)))).unwrap_or([0.0; 4]);
                    ((size.0 / scale, size.1 / scale), pad.map(|v| v / scale))
                })
        };
        let pct = |v: f64| num((v * 100.0).clamp(0.0, 100.0).round());
        let (w, h) = (frame.width(), frame.height());
        match item.base {
            "ProgressBar" => {
                // Godot draws the fill picture from the left edge, as wide as the value asks
                // past its corners: the picture as cut is the fill as drawn.
                let value = match (part(UiPart::Fill), picture("fill")) {
                    (Some(_), Some(((fw, _), pad))) => {
                        let min = pad[0] + pad[2];
                        if w > min { (fw - min) / (w - min) } else { 1.0 }
                    }
                    _ => 0.0,
                };
                format!("value = {}\nshow_percentage = false\n", pct(value))
            }
            "HSlider" | "VSlider" => {
                let across = item.base == "HSlider";
                let value = match part(UiPart::Grabber).and_then(|g| kit::piece_box(d, g)) {
                    Some(g) => {
                        let size = picture("grabber").map_or(if across { g.width() } else { g.height() }, |((gw, gh), _)| if across { gw } else { gh });
                        if across {
                            (g.center().x - frame.x0 - size / 2.0) / (w - size).max(1.0)
                        } else {
                            1.0 - (g.center().y - frame.y0 - size / 2.0) / (h - size).max(1.0)
                        }
                    }
                    None => 0.5,
                };
                format!("value = {}\n", pct(value))
            }
            "HScrollBar" | "VScrollBar" => {
                let across = item.base == "HScrollBar";
                let Some(g) = part(UiPart::Grabber).and_then(|g| kit::piece_box(d, g)) else { return "page = 30.0\n".into() };
                // The grabber moves in the track inside its padding.
                let pad = picture("scroll").map_or([0.0; 4], |(_, p)| p);
                let (start, area, at, len) =
                    if across { (frame.x0 + pad[0], w - pad[0] - pad[2], g.x0, g.width()) } else { (frame.y0 + pad[1], h - pad[1] - pad[3], g.y0, g.height()) };
                let page = (len / area.max(1.0)).clamp(0.01, 1.0);
                let value = ((at - start) / area.max(1.0)).clamp(0.0, 1.0 - page);
                format!("page = {}\nvalue = {}\n", num((page * 100.0 * 10.0).round() / 10.0), num((value * 100.0 * 10.0).round() / 10.0))
            }
            "CheckBox" | "CheckButton" => {
                if part(UiPart::Checked).is_some() {
                    "button_pressed = true\n".into()
                } else {
                    String::new()
                }
            }
            _ => String::new(),
        }
    }

    fn label(&mut self, k: usize, parent: &str, place: Place) -> Result<(), String> {
        let d = self.doc;
        let n = &d.nodes[k];
        let Some(shape) = n.text.as_ref() else { return Ok(()) };
        let Some(b) = kit::text_box(d, k) else { return Ok(()) };
        self.open(&node_name(&n.id), "Label", parent, place, b, n.anchor);
        self.nodes.push_str(&format!("text = \"{}\"\n", quote(&shape.content)));
        let align = match n.text_style.align {
            motif_core::TextAlign::Left => 0,
            motif_core::TextAlign::Center => 1,
            motif_core::TextAlign::Right => 2,
        };
        self.nodes.push_str(&format!("horizontal_alignment = {align}\nvertical_alignment = 1\n"));
        let scale = d.world_transform(k).as_coeffs()[3].abs();
        if let Some(f) = &shape.font {
            let id = self.font(f)?;
            let spacing = self.s(n.text_style.spacing * scale).round();
            if spacing != 0.0 {
                // Letter spacing is a variation of the font in Godot.
                self.n_sub += 1;
                let sub = format!("{}_spacing", self.n_sub);
                self.subs
                    .push_str(&format!("\n[sub_resource type=\"FontVariation\" id=\"{sub}\"]\nbase_font = ExtResource(\"{id}\")\nspacing_glyph = {spacing}\n"));
                self.nodes.push_str(&format!("theme_override_fonts/font = SubResource(\"{sub}\")\n"));
            } else {
                self.nodes.push_str(&format!("theme_override_fonts/font = ExtResource(\"{id}\")\n"));
            }
        }
        self.nodes.push_str(&format!("theme_override_font_sizes/font_size = {}\n", self.s(n.text_style.size * scale).round().max(1.0)));
        if let Some(c) = n.fill {
            self.nodes
                .push_str(&format!("theme_override_colors/font_color = {}\n", color(Color { a: (c.a as f64 * n.opacity.clamp(0.0, 1.0)).round() as u8, ..c })));
        }
        if let Some((c, w)) = n.outlines.first().copied().or(n.stroke.filter(|_| n.stroke_width > 0.0).map(|c| (c, n.stroke_width))) {
            self.nodes.push_str(&format!(
                "theme_override_colors/font_outline_color = {}\ntheme_override_constants/outline_size = {}\n",
                color(c),
                self.s(w * scale).round().max(1.0)
            ));
        }
        if let Some(sh) = &n.shadow {
            self.nodes.push_str(&format!(
                "theme_override_colors/font_shadow_color = {}\ntheme_override_constants/shadow_offset_x = {}\ntheme_override_constants/shadow_offset_y = {}\n",
                color(sh.color),
                self.s(sh.dx * scale).round(),
                self.s(sh.dy * scale).round()
            ));
        }
        Ok(())
    }

    /// A stack: `PanelContainer` when it has a `fit` background, padding as a
    /// `MarginContainer`, the children in a `VBoxContainer` or `HBoxContainer`.
    fn stack(&mut self, k: usize, stack: motif_core::Stack, parent: &str, place: Place) -> Result<(), String> {
        let d = self.doc;
        let n = &d.nodes[k];
        let name = node_name(&n.id);
        let kids = self.children(k);
        let fit = kids.iter().copied().find(|&c| d.nodes[c].fit);
        let frame = n.frame.map(|f| d.world_transform(k).transform_rect_bbox(f)).or_else(|| kit::piece_box(d, k)).unwrap_or_default();
        let mut me;
        let pad = self.s(stack.pad).round();
        match fit.and_then(|f| kit::picture(d, f, self.opts).map(|p| (f, p))) {
            Some((f, (px, at))) => {
                self.open(&name, "PanelContainer", parent, place, frame, n.anchor);
                let file = format!("{}_{}.png", self.file, d.nodes[f].id);
                let path = self.dir.join(&file);
                px.save_png(&path).map_err(|e| format!("{}: {e}", path.display()))?;
                self.files.push(path);
                let tex = self.add_ext("Texture2D", &file);
                self.n_sub += 1;
                let sub = format!("{}_panel", self.n_sub);
                self.subs.push_str(&format!("\n[sub_resource type=\"StyleBoxTexture\" id=\"{sub}\"]\ntexture = ExtResource(\"{tex}\")\n"));
                // Corners from `slice`, else as round as the background is.
                let bg = &d.nodes[f];
                let corner = bg.slice.unwrap_or_else(|| {
                    let r = bg.radii.map_or(bg.radius, |r| r.iter().cloned().fold(0.0, f64::max)) + bg.stroke_width / 2.0;
                    [r.ceil(); 4]
                });
                let expand = [frame.x0 - at.x0, frame.y0 - at.y0, at.x1 - frame.x1, at.y1 - frame.y1].map(|v| self.s(v).round().max(0.0));
                for (i, side) in ["left", "top", "right", "bottom"].iter().enumerate() {
                    self.subs.push_str(&format!("texture_margin_{side} = {}\n", num(self.s(corner[i]).round() + expand[i])));
                }
                for (i, side) in ["left", "top", "right", "bottom"].iter().enumerate() {
                    if expand[i] > 0.0 {
                        self.subs.push_str(&format!("expand_margin_{side} = {}\n", num(expand[i])));
                    }
                }
                for side in ["left", "top", "right", "bottom"] {
                    self.subs.push_str(&format!("content_margin_{side} = {}\n", num(pad)));
                }
                self.nodes.push_str(&format!("theme_override_styles/panel = SubResource(\"{sub}\")\n"));
                me = Self::path(parent, &name);
                let kind = if stack.dir == StackDir::Column { "VBoxContainer" } else { "HBoxContainer" };
                self.nodes.push_str(&format!("\n[node name=\"Stack\" type=\"{kind}\" parent=\"{me}\"]\nlayout_mode = 2\n"));
                me = Self::path(&me, "Stack");
            }
            None => {
                let kind = if stack.dir == StackDir::Column { "VBoxContainer" } else { "HBoxContainer" };
                if pad > 0.0 {
                    self.open(&name, "MarginContainer", parent, place, frame, n.anchor);
                    for side in ["left", "top", "right", "bottom"] {
                        self.nodes.push_str(&format!("theme_override_constants/margin_{side} = {}\n", num(pad)));
                    }
                    me = Self::path(parent, &name);
                    self.nodes.push_str(&format!("\n[node name=\"Stack\" type=\"{kind}\" parent=\"{me}\"]\nlayout_mode = 2\n"));
                    me = Self::path(&me, "Stack");
                } else {
                    self.open(&name, kind, parent, place, frame, n.anchor);
                    me = Self::path(parent, &name);
                }
            }
        }
        self.nodes.push_str(&format!("theme_override_constants/separation = {}\n", self.s(stack.gap).round()));
        for c in kids.into_iter().filter(|c| Some(*c) != fit) {
            self.emit(c, &me, Place::Stacked(stack.dir, stack.align))?;
        }
        Ok(())
    }
}

/// Writes the screen `doc` as `<file>.tscn` in `dir`, with the pictures of its art next to it.
/// `pieces` are the kit items of this board with their type variation names in `theme` (the
/// kit's `.tres`, written separately). Returns the files written.
pub fn write_screen(
    dir: &Path,
    file: &str,
    title: &str,
    theme: Option<&str>,
    doc: &Document,
    pieces: &[(KitItem, String)],
    opts: CpuOptions,
) -> Result<Vec<PathBuf>, String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    // The screen as the game first shows it: laid out, in the normal state.
    let mut posed = doc.clone();
    posed.rig();
    let by_id = |id: &str| pieces.iter().find(|(it, _)| it.id == id).map(|(it, ty)| (it, ty.clone()));
    let mut w = Writer {
        doc: &posed,
        pieces: posed.nodes.iter().map(|n| by_id(&n.id)).collect(),
        opts,
        dir,
        file: file.to_string(),
        ext: String::new(),
        subs: String::new(),
        nodes: String::new(),
        n_ext: 0,
        n_sub: 0,
        fonts: Vec::new(),
        files: Vec::new(),
    };
    let theme_id = theme.map(|t| w.add_ext("Theme", t));
    let root = node_name(title);
    let mut head = format!(
        "[node name=\"{}\" type=\"Control\"]\nlayout_mode = 3\nanchors_preset = 15\nanchor_right = 1.0\nanchor_bottom = 1.0\ngrow_horizontal = 2\ngrow_vertical = 2\n",
        quote(&root)
    );
    if let Some(id) = &theme_id {
        head.push_str(&format!("theme = ExtResource(\"{id}\")\n"));
    }
    let bg = posed.artboard.background;
    head.push_str(&format!(
        "\n[node name=\"Background\" type=\"ColorRect\" parent=\".\"]\nlayout_mode = 1\nanchors_preset = 15\nanchor_right = 1.0\nanchor_bottom = 1.0\ngrow_horizontal = 2\ngrow_vertical = 2\nmouse_filter = 2\ncolor = {}\n",
        color(bg)
    ));
    for k in (0..posed.nodes.len()).filter(|&k| posed.nodes[k].parent.is_none() && posed.is_visible(k)) {
        w.emit(k, ".", Place::Screen)?;
    }
    let text = format!("[gd_scene load_steps={} format=3]\n\n{}{}\n{head}{}", w.n_ext + w.n_sub + 1, w.ext, w.subs, w.nodes);
    let path = dir.join(format!("{file}.tscn"));
    std::fs::write(&path, text).map_err(|e| format!("{}: {e}", path.display()))?;
    w.files.push(path);
    Ok(w.files)
}

/// Every board as a screen sharing one kit theme `<theme>.tres`: the kit and the scenes, all in
/// `dir`. `boards` are the boards' names and documents. Returns the files written.
pub fn write_screens(dir: &Path, theme: &str, boards: &[(String, Document)], opts: CpuOptions) -> Result<Vec<PathBuf>, String> {
    // A board without interface pieces still makes a screen, of pictures and labels.
    let kits: Vec<Vec<KitItem>> =
        boards.iter().map(|(_, d)| if kit::has_pieces(d) { kit::render_kit(d, opts) } else { Ok(Vec::new()) }).collect::<Result<_, _>>()?;
    let merged = kit::merge_boards(kits.clone());
    let mut files = Vec::new();
    let has_theme = !merged.is_empty();
    if has_theme {
        files.extend(kit::write_kit(dir, theme, &merged)?);
    }
    let mut at = 0;
    for ((name, doc), items) in boards.iter().zip(kits) {
        // Each piece by its own id, with the variation name the merged kit gave it.
        let named: Vec<(KitItem, String)> = items.into_iter().enumerate().map(|(j, it)| (it, kit::type_name(&merged[at + j].id))).collect();
        at += named.len();
        let file = if boards.len() > 1 { crate::screen::file_stem(name) } else { theme.to_string() + "_screen" };
        let theme_file = has_theme.then(|| format!("{theme}.tres"));
        files.extend(write_screen(dir, &file, name, theme_file.as_deref(), doc, &named, opts)?);
    }
    Ok(files)
}

/// A file name for a board's screen: its name with the characters files cannot have taken out.
pub fn file_stem(name: &str) -> String {
    let s: String = name.chars().map(|c| if matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') { '_' } else { c }).collect();
    if s.trim().is_empty() { "screen".into() } else { s.trim().replace(' ', "_") }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn boards(src: &str) -> Vec<(String, Document)> {
        motif_script::parse_boards(src, None).unwrap_or_else(|e| panic!("{e:?}")).into_iter().map(|s| (s.doc.artboard.name.clone(), s.doc)).collect()
    }

    fn scene(dir: &Path, name: &str) -> String {
        std::fs::read_to_string(dir.join(name)).unwrap_or_else(|e| panic!("{name}: {e}"))
    }

    /// The node `name` of a scene: its header and properties.
    fn node<'s>(tscn: &'s str, name: &str) -> &'s str {
        let at = tscn.find(&format!("[node name=\"{name}\"")).unwrap_or_else(|| panic!("нет узла {name}:\n{tscn}"));
        let rest = &tscn[at..];
        &rest[..rest[1..].find("\n[").map_or(rest.len(), |e| e + 1)]
    }

    #[test]
    fn a_menu_becomes_controls_in_a_stack() {
        let dir = std::env::temp_dir().join(format!("motif-screen-menu-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let files = write_screens(&dir, "menu", &boards(include_str!("../../../examples/menu.msc")), CpuOptions::default()).unwrap();
        assert!(files.iter().any(|f| f.ends_with("menu.tres")), "{files:?}");
        let tscn = scene(&dir, "menu_screen.tscn");
        assert!(tscn.contains("[ext_resource type=\"Theme\" path=\"menu.tres\""), "{tscn}");
        // The centred stack: a panel around a VBox with the gap of the stack.
        let menu = node(&tscn, "menu");
        assert!(menu.contains("type=\"PanelContainer\"") && menu.contains("anchor_left = 0.5") && menu.contains("grow_horizontal = 2"), "{menu}");
        let stack = node(&tscn, "Stack");
        assert!(stack.contains("type=\"VBoxContainer\"") && stack.contains("separation = 10"), "{stack}");
        // Buttons of the kit, with their captions as text.
        let play = node(&tscn, "play");
        assert!(
            play.contains("type=\"Button\" parent=\"menu/Stack\"") && play.contains("text = \"Играть\"") && play.contains("theme_type_variation"),
            "{play}"
        );
        assert!(node(&tscn, "title").contains("type=\"Label\""), "{tscn}");
        // The close button keeps to the top right corner.
        let close = node(&tscn, "close");
        assert!(close.contains("anchor_left = 1.0") && close.contains("anchor_right = 1.0") && close.contains("grow_horizontal = 0"), "{close}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn controls_show_what_is_drawn() {
        let dir = std::env::temp_dir().join(format!("motif-screen-kit-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        write_screens(&dir, "ui-kit", &boards(include_str!("../../../examples/ui-kit.msc")), CpuOptions::default()).unwrap();
        // The fill picture is 140 of 180 px wide and its corners take 36: (140 − 36) / (180 − 36).
        let bar = scene(&dir, "Полоса.tscn");
        assert!(node(&bar, "health").contains("value = 72.0"), "{bar}");
        assert!(node(&scene(&dir, "Флажок.tscn"), "sound").contains("button_pressed = true"));
        // The thumb is 50 of the 160 px track, 25 px down it.
        let scroll = scene(&dir, "Прокрутка.tscn");
        let scroller = node(&scroll, "scroller");
        assert!(scroller.contains("page = 31.3") && scroller.contains("value = 15.6"), "{scroller}");
        assert!(node(&scene(&dir, "Слайдер.tscn"), "volume").contains("value = 50.0"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn effects_are_baked() {
        let dir = std::env::temp_dir().join(format!("motif-screen-fx-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        // A board without pieces (the sign) still makes a screen next to the kit.
        let files = write_screens(&dir, "ui-effects", &boards(include_str!("../../../examples/ui-effects.msc")), CpuOptions::default()).unwrap();
        assert!(files.iter().any(|f| f.ends_with("ui-effects.tres")), "{files:?}");
        // A glowing text is a picture: a Label could not draw the glow.
        let sign = scene(&dir, "Вывеска.tscn");
        assert!(node(&sign, "sign").contains("type=\"TextureRect\""), "{sign}");
        // The glow of the button reaches past it: drawn outside the control, not in its padding.
        let theme = scene(&dir, "ui-effects.tres");
        assert!(theme.contains("expand_margin_left"), "{theme}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
