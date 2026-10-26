//! Godot export: every animation of a scene baked into one sprite atlas, a `SpriteFrames`
//! resource over it, and a ready scene with an `AnimatedSprite2D`. When the scene has a state
//! machine, the scene also gets an `AnimationPlayer`, an `AnimationTree` with the same states and
//! transitions, and a script whose variables are the machine's inputs, so the game drives the
//! character exactly as the machine panel does.
//!
//! All frames share one size: the box that holds every pose of every animation, so the sprite
//! does not jump when an animation switches. Identical frames are stored in the atlas once.

use motif_core::kurbo::Point;
use motif_core::{CondOp, Document, InputKind, Machine, SimCache};
use motif_render::{CpuOptions, render_cpu};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tiny_skia::{IntSize, Pixmap, PixmapPaint, Transform};

/// The widest atlas Godot is comfortable with on every renderer.
const MAX_ATLAS: u32 = 8192;
/// Transparent pixels between frames in the atlas, so filtering never bleeds a neighbour in.
const GAP: u32 = 2;

/// What to bake.
#[derive(Clone, Debug)]
pub struct GodotExport {
    pub fps: f64,
    pub render: CpuOptions,
    /// Bake only this object (and what is inside it), not the whole artboard.
    pub only: Option<String>,
    /// The sprite's origin in artboard units: where its node sits in the game. By default the
    /// object's own origin, or the middle of the artboard.
    pub pivot: Option<Point>,
}

impl Default for GodotExport {
    fn default() -> Self {
        GodotExport { fps: 12.0, render: CpuOptions { scale: 1.0, antialias: true, background: false }, only: None, pivot: None }
    }
}

/// One animation in the atlas.
#[derive(Clone, Debug, PartialEq)]
pub struct SheetAnim {
    pub name: String,
    pub looping: bool,
    /// Index into [`Sheet::cells`] for each frame.
    pub frames: Vec<usize>,
    /// The animation's events by the frame they fall on.
    pub events: Vec<(usize, String)>,
}

/// Every animation baked into one picture.
pub struct Sheet {
    pub atlas: Pixmap,
    /// Where each distinct frame is in the atlas: x, y (width and height are `size`).
    pub cells: Vec<(u32, u32)>,
    /// The size of every frame, in pixels.
    pub size: (u32, u32),
    /// Where the top-left corner of a frame is relative to the sprite's origin, in pixels.
    pub offset: (f64, f64),
    pub fps: f64,
    pub anims: Vec<SheetAnim>,
}

/// One animation rendered at full artboard size, before trimming and packing.
struct RawAnim {
    name: String,
    looping: bool,
    frames: Vec<Pixmap>,
    events: Vec<(usize, String)>,
}

/// The scene ready to bake: only the chosen object shown, the background off.
fn prepared(doc: &Document, only: Option<&str>) -> Result<Document, String> {
    let mut doc = doc.clone();
    if let Some(id) = only {
        let i = doc.index_of(id).ok_or_else(|| format!("нет объекта «{id}»"))?;
        for k in 0..doc.nodes.len() {
            let keep = k == i || doc.is_inside(k, i) || doc.is_inside(i, k);
            if !keep {
                doc.nodes[k].visible = false;
            }
        }
    }
    Ok(doc)
}

/// Renders every frame of every animation (the scene as written when it has none), trims them
/// to one shared box and packs them into an atlas.
pub fn bake_sheet(doc: &Document, opts: &GodotExport) -> Result<Sheet, String> {
    bake_sheet_with(doc, opts, Ok)
}

/// [`bake_sheet`] with a pass over the distinct frames before they are packed, such as
/// [`crate::pixatool::pixelize_frames`]. The pass gets every frame at once and must return as
/// many, all of one size; a smaller size (a pixelized frame) scales the sprite's offset with it.
pub fn bake_sheet_with(doc: &Document, opts: &GodotExport, process: impl FnOnce(Vec<Pixmap>) -> Result<Vec<Pixmap>, String>) -> Result<Sheet, String> {
    if opts.fps <= 0.0 {
        return Err("fps должен быть больше нуля".into());
    }
    let base = prepared(doc, opts.only.as_deref())?;
    let render = CpuOptions { background: false, ..opts.render };
    // Every animation's frames, full artboard size.
    let mut raw: Vec<RawAnim> = Vec::new();
    for anim in &base.animations {
        let mut count = (anim.duration * opts.fps).round() as usize;
        if !anim.looping {
            count += 1;
        }
        let mut sim = SimCache::default();
        let frames = (0..count.max(1))
            .map(|i| {
                let t = (i as f64 / opts.fps).min(anim.duration);
                render_cpu(&anim.pose_with(&base, t, &mut sim), render).ok_or_else(|| "не удалось создать изображение".to_string())
            })
            .collect::<Result<Vec<_>, _>>()?;
        // An event belongs to the frame nearest its time; on a loop the end is the next start.
        let events = anim
            .events
            .iter()
            .map(|e| {
                let i = (e.time * opts.fps).round() as usize;
                (if anim.looping { i % frames.len() } else { i.min(frames.len() - 1) }, e.name.clone())
            })
            .collect();
        raw.push(RawAnim { name: anim.name.clone(), looping: anim.looping, frames, events });
    }
    if raw.is_empty() {
        let mut posed = base.clone();
        posed.rig();
        let px = render_cpu(&posed, render).ok_or("не удалось создать изображение")?;
        raw.push(RawAnim { name: "default".into(), looping: false, frames: vec![px], events: Vec::new() });
    }

    // The box that holds every pose.
    let mut bounds: Option<(u32, u32, u32, u32)> = None;
    for px in raw.iter().flat_map(|r| &r.frames) {
        if let Some(b) = opaque_box(px) {
            bounds = Some(match bounds {
                None => b,
                Some(a) => (a.0.min(b.0), a.1.min(b.1), a.2.max(b.2), a.3.max(b.3)),
            });
        }
    }
    let (x0, y0, x1, y1) = bounds.ok_or("на кадрах ничего не видно: нечего выгружать")?;
    let size = (x1 - x0, y1 - y0);

    // Distinct frames, cropped.
    let mut cells: Vec<Pixmap> = Vec::new();
    let mut seen: HashMap<Vec<u8>, usize> = HashMap::new();
    let mut anims = Vec::new();
    for RawAnim { name, looping, frames, events } in raw {
        let mut ids = Vec::new();
        for px in frames {
            let crop = px.clone_rect(tiny_skia::IntRect::from_xywh(x0 as i32, y0 as i32, size.0, size.1).unwrap()).ok_or("пустой кадр")?;
            let id = *seen.entry(crop.data().to_vec()).or_insert_with(|| {
                cells.push(crop);
                cells.len() - 1
            });
            ids.push(id);
        }
        anims.push(SheetAnim { name, looping, frames: ids, events });
    }
    let count = cells.len();
    let cells = process(cells)?;
    if cells.len() != count {
        return Err(format!("после обработки {} кадров вместо {count}", cells.len()));
    }
    let done = (cells[0].width(), cells[0].height());
    if cells.iter().any(|c| (c.width(), c.height()) != done) {
        return Err("после обработки кадры разного размера".into());
    }
    let ratio = (done.0 as f64 / size.0 as f64, done.1 as f64 / size.1 as f64);
    let size = done;

    // A grid as close to square as fits.
    let (cw, ch) = (size.0 + GAP, size.1 + GAP);
    let n = cells.len() as u32;
    let mut cols = ((n as f64 * ch as f64 / cw as f64).sqrt().ceil() as u32).clamp(1, n);
    cols = cols.min((MAX_ATLAS / cw).max(1));
    let rows = n.div_ceil(cols);
    if rows * ch > MAX_ATLAS {
        return Err(format!("кадров слишком много для одного атласа {MAX_ATLAS}×{MAX_ATLAS}: уменьшите масштаб или кадры в секунду"));
    }
    let mut atlas = Pixmap::from_vec(vec![0; (cols * cw * rows * ch * 4) as usize], IntSize::from_wh(cols * cw, rows * ch).ok_or("пустой атлас")?)
        .ok_or("не удалось создать атлас")?;
    let mut at = Vec::new();
    for (k, cell) in cells.iter().enumerate() {
        let (x, y) = ((k as u32 % cols) * cw, (k as u32 / cols) * ch);
        atlas.draw_pixmap(x as i32, y as i32, cell.as_ref(), &PixmapPaint::default(), Transform::identity(), None);
        at.push((x, y));
    }

    let s = render.scale;
    let pivot = opts.pivot.unwrap_or_else(|| {
        let own = opts.only.as_deref().and_then(|id| base.index_of(id)).map(|i| base.world_transform(i) * Point::ZERO);
        own.unwrap_or(Point::new(base.artboard.width / 2.0, base.artboard.height / 2.0))
    });
    let mut offset = ((x0 as f64 - pivot.x * s) * ratio.0, (y0 as f64 - pivot.y * s) * ratio.1);
    if ratio != (1.0, 1.0) {
        // Resized frames are pixel art: on whole pixels, so none comes out wider than the rest.
        offset = (offset.0.round(), offset.1.round());
    }
    Ok(Sheet { atlas, cells: at, size, offset, fps: opts.fps, anims })
}

/// The smallest box around pixels that are not fully transparent: x0, y0, x1, y1.
fn opaque_box(px: &Pixmap) -> Option<(u32, u32, u32, u32)> {
    let (w, h) = (px.width(), px.height());
    let mut b: Option<(u32, u32, u32, u32)> = None;
    for (k, p) in px.pixels().iter().enumerate() {
        if p.alpha() == 0 {
            continue;
        }
        let (x, y) = (k as u32 % w, k as u32 / w);
        b = Some(match b {
            None => (x, y, x + 1, y + 1),
            Some(a) => (a.0.min(x), a.1.min(y), a.2.max(x + 1), a.3.max(y + 1)),
        });
    }
    b.filter(|_| w > 0 && h > 0)
}

/// A name Godot takes as a node name or a script identifier.
fn ident(name: &str) -> String {
    let mut s: String = name.chars().map(|c| if c.is_alphanumeric() || c == '_' { c } else { '_' }).collect();
    if s.starts_with(|c: char| c.is_ascii_digit()) || s.is_empty() {
        s.insert(0, '_');
    }
    s
}

fn quote(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

fn num(v: f64) -> String {
    let s = format!("{:.4}", v);
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s.contains('.') { s.to_string() } else { format!("{s}.0") }
}

/// The `advance_expression` of a transition: all its conditions together.
fn expression(m: &Machine, conds: &[motif_core::Cond]) -> String {
    let parts: Vec<String> = conds
        .iter()
        .map(|c| {
            let v = ident(&c.input);
            let number = m.input(&c.input).is_some_and(|i| m.inputs[i].kind == InputKind::Number);
            match c.op {
                CondOp::Is if number => format!("{v} != 0.0"),
                CondOp::Is => v,
                CondOp::Not if number => format!("{v} == 0.0"),
                CondOp::Not => format!("not {v}"),
                op => format!("{v} {} {}", op.symbol(), num(c.value)),
            }
        })
        .collect();
    parts.join(" and ")
}

/// The part of the scene script that turns animation events into the `event` signal: a table of
/// events by animation and frame, read whenever the sprite shows another frame. `None` when the
/// animations have no events.
fn events_gd(sheet: &Sheet) -> Option<String> {
    if sheet.anims.iter().all(|a| a.events.is_empty()) {
        return None;
    }
    let mut table = Vec::new();
    for a in sheet.anims.iter().filter(|a| !a.events.is_empty()) {
        let mut frames: Vec<(usize, Vec<&str>)> = Vec::new();
        for (f, name) in &a.events {
            match frames.iter_mut().find(|(k, _)| k == f) {
                Some((_, names)) => names.push(name),
                None => frames.push((*f, vec![name])),
            }
        }
        frames.sort_by_key(|(f, _)| *f);
        let entries: Vec<String> =
            frames.iter().map(|(f, names)| format!("{f}: [{}]", names.iter().map(|n| format!("\"{}\"", quote(n))).collect::<Vec<_>>().join(", "))).collect();
        table.push(format!("\t\"{}\": {{{}}}", quote(&a.name), entries.join(", ")));
    }
    Some(format!(
        "\n## Animation events from Motif: connect to it, e.g. event.connect(func(name): if name == \"step\": $Step.play()).\nsignal event(name: String)\n\n\
## Events by animation and frame.\nconst EVENTS := {{\n{}\n}}\n\nvar _shown := []\n\n\
func _ready() -> void:\n\t$Sprite.frame_changed.connect(_on_frame)\n\t# Right after a switch the sprite still holds the old frame number: read once it is set.\n\t$Sprite.animation_changed.connect(_on_frame.call_deferred)\n\t_on_frame()\n\n\
func _on_frame() -> void:\n\tvar anim := String($Sprite.animation)\n\tvar shown := [anim, $Sprite.frame]\n\tif shown == _shown:\n\t\treturn\n\t_shown = shown\n\
\tfor name in EVENTS.get(anim, {{}}).get($Sprite.frame, []):\n\t\tevent.emit(name)\n",
        table.join(",\n")
    ))
}

/// Writes `<name>.png` (the atlas), `<name>_frames.tres` (SpriteFrames) and `<name>.tscn` (a
/// scene with the sprite) into `dir`; with a state machine or animation events also `<name>.gd`,
/// the script of its inputs and its `event` signal. `pixel` turns texture filtering off. Returns
/// the files written.
pub fn write_godot(dir: &Path, name: &str, sheet: &Sheet, machine: Option<&Machine>, pixel: bool) -> Result<Vec<PathBuf>, String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let png = format!("{name}.png");
    let path = dir.join(&png);
    sheet.atlas.save_png(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut files = vec![path];
    let mut save = |file: String, text: String| -> Result<(), String> {
        let path = dir.join(file);
        std::fs::write(&path, text).map_err(|e| format!("{}: {e}", path.display()))?;
        files.push(path);
        Ok(())
    };

    // SpriteFrames: one AtlasTexture per distinct frame.
    let (w, h) = sheet.size;
    let mut tres = format!("[gd_resource type=\"SpriteFrames\" load_steps={} format=3]\n\n", sheet.cells.len() + 2);
    tres.push_str(&format!("[ext_resource type=\"Texture2D\" path=\"{png}\" id=\"1_atlas\"]\n"));
    for (k, (x, y)) in sheet.cells.iter().enumerate() {
        tres.push_str(&format!("\n[sub_resource type=\"AtlasTexture\" id=\"f{k}\"]\natlas = ExtResource(\"1_atlas\")\nregion = Rect2({x}, {y}, {w}, {h})\n"));
    }
    tres.push_str("\n[resource]\nanimations = [");
    for (i, a) in sheet.anims.iter().enumerate() {
        let frames: Vec<String> = a.frames.iter().map(|f| format!("{{\n\"duration\": 1.0,\n\"texture\": SubResource(\"f{f}\")\n}}")).collect();
        tres.push_str(&format!(
            "{}{{\n\"frames\": [{}],\n\"loop\": {},\n\"name\": &\"{}\",\n\"speed\": {}\n}}",
            if i > 0 { ", " } else { "" },
            frames.join(", "),
            a.looping,
            quote(&a.name),
            num(sheet.fps)
        ));
    }
    tres.push_str("]\n");
    let frames_file = format!("{name}_frames.tres");
    save(frames_file.clone(), tres)?;

    let root = ident(name);
    let first = sheet.anims.first().map_or("default".to_string(), |a| a.name.clone());
    let sprite_props = |autoplay: bool| {
        let mut s = format!(
            "sprite_frames = ExtResource(\"1_frames\")\nanimation = &\"{}\"\ncentered = false\noffset = Vector2({}, {})\n",
            quote(&first),
            num(sheet.offset.0),
            num(sheet.offset.1)
        );
        if autoplay {
            s.push_str(&format!("autoplay = \"{}\"\n", quote(&first)));
        }
        if pixel {
            s.push_str("texture_filter = 1\n");
        }
        s
    };

    let events = events_gd(sheet);
    let Some(m) = machine.filter(|m| !m.states.is_empty()) else {
        // Without a machine a script is needed only to hand out the events.
        let script = match &events {
            Some(ev) => {
                save(format!("{name}.gd"), format!("extends Node2D\n## Exported from Motif.\n{ev}"))?;
                (format!("[ext_resource type=\"Script\" path=\"{name}.gd\" id=\"1_script\"]\n"), "script = ExtResource(\"1_script\")\n")
            }
            None => (String::new(), ""),
        };
        let tscn = format!(
            "[gd_scene load_steps={} format=3]\n\n{}[ext_resource type=\"SpriteFrames\" path=\"{frames_file}\" id=\"1_frames\"]\n\n[node name=\"{root}\" type=\"Node2D\"]\n{}\n[node name=\"Sprite\" type=\"AnimatedSprite2D\" parent=\".\"]\n{}",
            if events.is_some() { 3 } else { 2 },
            script.0,
            script.1,
            sprite_props(true)
        );
        save(format!("{name}.tscn"), tscn)?;
        return Ok(files);
    };

    // The inputs as script variables: the AnimationTree reads them in its expressions.
    let mut gd = format!(
        "extends Node2D\n## Inputs of the state machine «{}», exported from Motif.\n## Set them from the game; call fire_…() for a one-shot trigger.\n\n",
        m.name
    );
    for input in &m.inputs {
        let v = ident(&input.name);
        match input.kind {
            InputKind::Number => gd.push_str(&format!("@export var {v} := {}\n", num(input.default))),
            InputKind::Bool => gd.push_str(&format!("@export var {v} := {}\n", input.default != 0.0)),
            InputKind::Trigger => gd.push_str(&format!("var {v} := false\n")),
        }
    }
    for input in m.inputs.iter().filter(|i| i.kind == InputKind::Trigger) {
        let v = ident(&input.name);
        gd.push_str(&format!(
            "\n## Fires «{}» once: the transitions that wait for it get to see it, then it is off again.\nfunc fire_{v}() -> void:\n\t{v} = true\n\tawait get_tree().process_frame\n\tawait get_tree().process_frame\n\t{v} = false\n",
            input.name
        ));
    }
    if let Some(ev) = &events {
        gd.push_str(ev);
    }
    save(format!("{name}.gd"), gd)?;

    // One Animation per baked animation: it sets the sprite's animation and steps its frames.
    let mut subs = String::new();
    let mut lib = Vec::new();
    for (k, a) in sheet.anims.iter().enumerate() {
        let n = a.frames.len();
        let times: Vec<String> = (0..n).map(|i| num(i as f64 / sheet.fps)).collect();
        let ones: Vec<&str> = (0..n).map(|_| "1").collect();
        let values: Vec<String> = (0..n).map(|i| i.to_string()).collect();
        let length = if a.looping || n == 1 { n as f64 / sheet.fps } else { (n - 1) as f64 / sheet.fps };
        subs.push_str(&format!(
            "\n[sub_resource type=\"Animation\" id=\"a{k}\"]\nresource_name = \"{0}\"\nlength = {1}\nloop_mode = {2}\nstep = {3}\n\
tracks/0/type = \"value\"\ntracks/0/imported = false\ntracks/0/enabled = true\ntracks/0/path = NodePath(\"Sprite:animation\")\ntracks/0/interp = 0\ntracks/0/loop_wrap = true\ntracks/0/keys = {{\n\"times\": PackedFloat32Array(0),\n\"transitions\": PackedFloat32Array(1),\n\"update\": 1,\n\"values\": [&\"{0}\"]\n}}\n\
tracks/1/type = \"value\"\ntracks/1/imported = false\ntracks/1/enabled = true\ntracks/1/path = NodePath(\"Sprite:frame\")\ntracks/1/interp = 0\ntracks/1/loop_wrap = true\ntracks/1/keys = {{\n\"times\": PackedFloat32Array({4}),\n\"transitions\": PackedFloat32Array({5}),\n\"update\": 1,\n\"values\": [{6}]\n}}\n",
            quote(&a.name),
            num(length.max(1e-3)),
            u8::from(a.looping),
            num(1.0 / sheet.fps),
            times.join(", "),
            ones.join(", "),
            values.join(", ")
        ));
        lib.push(format!("&\"{}\": SubResource(\"a{k}\")", quote(&a.name)));
    }
    subs.push_str(&format!("\n[sub_resource type=\"AnimationLibrary\" id=\"lib\"]\n_data = {{\n{}\n}}\n", lib.join(",\n")));

    // The state machine: states play the animations, `any` becomes a transition from every state.
    let state_name = |i: usize| quote(&m.states[i].name);
    for (k, s) in m.states.iter().enumerate() {
        subs.push_str(&format!("\n[sub_resource type=\"AnimationNodeAnimation\" id=\"s{k}\"]\nanimation = &\"{}\"\n", quote(&s.anim)));
    }
    // Godot keeps one transition per pair of states: conditions of a second one are joined with «or».
    let mut pairs: Vec<((usize, usize), Vec<String>, bool)> = Vec::new();
    for t in &m.transitions {
        let froms: Vec<usize> = match t.from {
            Some(f) => vec![f],
            None => (0..m.states.len()).filter(|&f| f != t.to).collect(),
        };
        let expr = expression(m, &t.conds);
        for f in froms {
            match pairs.iter_mut().find(|p| p.0 == (f, t.to)) {
                Some(p) => p.1.push(expr.clone()),
                None => pairs.push(((f, t.to), vec![expr.clone()], t.at_end)),
            }
        }
    }
    subs.push_str("\n[sub_resource type=\"AnimationNodeStateMachineTransition\" id=\"t_start\"]\nadvance_mode = 2\n");
    let mut list = vec![format!("\"Start\", \"{}\", SubResource(\"t_start\")", state_name(0))];
    for (k, ((f, to), exprs, at_end)) in pairs.iter().enumerate() {
        let expr = if exprs.iter().any(String::is_empty) {
            String::new()
        } else if exprs.len() == 1 {
            exprs[0].clone()
        } else {
            exprs.iter().map(|e| format!("({e})")).collect::<Vec<_>>().join(" or ")
        };
        // Sprite frames cannot be cross-faded (Godot would blend the animation names letter by
        // letter), so the switch is instant; `blend` only lives on in the editor preview.
        subs.push_str(&format!("\n[sub_resource type=\"AnimationNodeStateMachineTransition\" id=\"t{k}\"]\n"));
        if *at_end {
            subs.push_str("switch_mode = 2\n");
        }
        subs.push_str("advance_mode = 2\n");
        if !expr.is_empty() {
            subs.push_str(&format!("advance_expression = \"{}\"\n", quote(&expr)));
        }
        list.push(format!("\"{}\", \"{}\", SubResource(\"t{k}\")", state_name(*f), state_name(*to)));
    }
    subs.push_str("\n[sub_resource type=\"AnimationNodeStateMachine\" id=\"machine\"]\n");
    for (k, s) in m.states.iter().enumerate() {
        let (x, y) = s.pos.unwrap_or((200.0 * k as f64, 100.0));
        subs.push_str(&format!(
            "states/{0}/node = SubResource(\"s{k}\")\nstates/{0}/position = Vector2({1}, {2})\n",
            quote(&s.name),
            num(x + 200.0),
            num(y + 100.0)
        ));
    }
    subs.push_str(&format!("transitions = [{}]\n", list.join(", ")));

    let tscn = format!(
        "[gd_scene load_steps={} format=3]\n\n[ext_resource type=\"Script\" path=\"{name}.gd\" id=\"1_script\"]\n[ext_resource type=\"SpriteFrames\" path=\"{frames_file}\" id=\"1_frames\"]\n{subs}\n\
[node name=\"{root}\" type=\"Node2D\"]\nscript = ExtResource(\"1_script\")\n\n\
[node name=\"Sprite\" type=\"AnimatedSprite2D\" parent=\".\"]\n{}\n\
[node name=\"AnimationPlayer\" type=\"AnimationPlayer\" parent=\".\"]\nlibraries = {{\n\"\": SubResource(\"lib\")\n}}\n\n\
[node name=\"AnimationTree\" type=\"AnimationTree\" parent=\".\"]\ntree_root = SubResource(\"machine\")\nadvance_expression_base_node = NodePath(\"..\")\nanim_player = NodePath(\"../AnimationPlayer\")\n",
        3 + sheet.anims.len() + 1 + m.states.len() + pairs.len() + 2,
        sprite_props(false)
    );
    save(format!("{name}.tscn"), tscn)?;
    Ok(files)
}

#[cfg(test)]
mod tests {
    use super::*;
    use motif_core::{Animation, Artboard, Ease, Keyframe, Node, Prop, ShapeKind, Track, Value};

    fn hero() -> Document {
        let mut d = Document::new(Artboard { width: 200.0, height: 100.0, ..Artboard::default() });
        let mut ball = Node::new("ball", ShapeKind::Ellipse);
        (ball.x, ball.y, ball.width, ball.height) = (100.0, 50.0, 20.0, 20.0);
        let mut floor = Node::new("floor", ShapeKind::Rect);
        (floor.x, floor.y, floor.width, floor.height) = (100.0, 95.0, 200.0, 10.0);
        d.nodes = vec![ball, floor];
        let key = |t, v| Keyframe { time: t, value: Value::Num(v), ease: Ease::Linear };
        d.animations.push(Animation { name: "Idle".into(), duration: 1.0, looping: true, tracks: Vec::new(), events: Vec::new() });
        d.animations.push(Animation {
            name: "Jump".into(),
            duration: 0.5,
            looping: false,
            tracks: vec![Track { node: "ball".into(), prop: Prop::Y, keys: vec![key(0.0, 50.0), key(0.5, 20.0)] }],
            events: Vec::new(),
        });
        d
    }

    #[test]
    fn one_box_for_every_pose_and_still_frames_stored_once() {
        let opts = GodotExport { fps: 4.0, only: Some("ball".into()), ..GodotExport::default() };
        let sheet = bake_sheet(&hero(), &opts).unwrap();
        assert_eq!(sheet.anims.len(), 2);
        assert_eq!(sheet.anims[0].frames, vec![0, 0, 0, 0], "Idle never moves: one picture");
        assert_eq!(sheet.anims[1].frames.len(), 3, "0, 0.25, 0.5 s");
        assert_eq!(sheet.anims[1].frames[0], 0, "Jump starts in the Idle pose");
        // The box spans the ball from its lowest to its highest pose, the floor is left out.
        assert_eq!(sheet.size, (20, 50));
        // The origin is the ball's own: its first pose is centred on it.
        assert_eq!(sheet.offset, (-10.0, -40.0));
    }

    #[test]
    fn events_become_a_signal_by_frame() {
        let mut doc = hero();
        let ev = |name: &str, time| motif_core::AnimEvent { name: name.into(), time };
        doc.animations[0].events = vec![ev("blink", 1.0)];
        doc.animations[1].events = vec![ev("push", 0.25), ev("land", 0.5), ev("dust", 0.5)];
        let opts = GodotExport { fps: 4.0, only: Some("ball".into()), ..GodotExport::default() };
        let sheet = bake_sheet(&doc, &opts).unwrap();
        assert_eq!(sheet.anims[0].events, [(0, "blink".to_string())], "the end of a loop is its start");
        assert_eq!(sheet.anims[1].events, [(1, "push".to_string()), (2, "land".to_string()), (2, "dust".to_string())]);
        // Without a machine the scene gets a script only for the signal.
        let dir = std::env::temp_dir().join(format!("motif-godot-events-{}", std::process::id()));
        write_godot(&dir, "ball", &sheet, None, false).unwrap();
        let gd = std::fs::read_to_string(dir.join("ball.gd")).unwrap();
        assert!(gd.contains("signal event(name: String)"), "{gd}");
        assert!(gd.contains("\"Jump\": {1: [\"push\"], 2: [\"land\", \"dust\"]}"), "{gd}");
        let tscn = std::fs::read_to_string(dir.join("ball.tscn")).unwrap();
        assert!(tscn.starts_with("[gd_scene load_steps=3") && tscn.contains("script = ExtResource(\"1_script\")"), "{tscn}");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_pass_over_the_frames_runs_once_and_scales_the_offset() {
        let opts = GodotExport { fps: 4.0, only: Some("ball".into()), ..GodotExport::default() };
        let mut seen = 0;
        let half = |frames: Vec<Pixmap>| {
            seen = frames.len();
            Ok(frames.iter().map(|f| Pixmap::new(f.width() / 2, f.height() / 2).unwrap()).collect())
        };
        let sheet = bake_sheet_with(&hero(), &opts, half).unwrap();
        assert_eq!(seen, sheet.cells.len(), "only distinct frames are processed");
        assert_eq!(sheet.size, (10, 25));
        assert_eq!(sheet.offset, (-5.0, -20.0));
        let odd = bake_sheet_with(&hero(), &opts, |mut f: Vec<Pixmap>| {
            f[0] = Pixmap::new(3, 3).unwrap();
            Ok(f)
        });
        assert!(odd.is_err(), "frames of different sizes are refused");
    }

    /// Runs only where a PixaTool copy is given: `MOTIF_TEST_PIXATOOL=…/Pixatool cargo test`.
    #[test]
    fn pixelizes_through_a_real_pixatool() {
        let Some(exe) = std::env::var_os("MOTIF_TEST_PIXATOOL").map(PathBuf::from) else { return };
        let opts = GodotExport { fps: 4.0, only: Some("ball".into()), ..GodotExport::default() };
        let preset = Path::new(env!("CARGO_MANIFEST_DIR")).join("testdata/px4.preset");
        let sheet = bake_sheet_with(&hero(), &opts, |f| crate::pixatool::pixelize_frames(&exe, Some(&preset), f, |_| {})).unwrap();
        // Pixel size 4: the 20×50 box becomes 5×12 or 5×13 pixels.
        assert_eq!(sheet.size.0, 5);
        assert!((12..=13).contains(&sheet.size.1), "{:?}", sheet.size);
    }

    #[test]
    fn the_scene_holds_the_machine() {
        let mut doc = hero();
        let src_machine = Machine {
            name: "Hero".into(),
            inputs: vec![
                motif_core::Input { name: "speed".into(), kind: InputKind::Number, default: 0.0 },
                motif_core::Input { name: "jump".into(), kind: InputKind::Trigger, default: 0.0 },
            ],
            listeners: Vec::new(),
            states: vec![
                motif_core::State { name: "Idle".into(), anim: "Idle".into(), pos: None },
                motif_core::State { name: "Jump".into(), anim: "Jump".into(), pos: None },
            ],
            transitions: vec![
                motif_core::Transition {
                    from: None,
                    to: 1,
                    conds: vec![motif_core::Cond { input: "jump".into(), op: CondOp::Is, value: 0.0 }],
                    at_end: false,
                    blend: 0.1,
                },
                motif_core::Transition {
                    from: Some(0),
                    to: 1,
                    conds: vec![motif_core::Cond { input: "speed".into(), op: CondOp::Gt, value: 2.0 }],
                    at_end: false,
                    blend: 0.0,
                },
                motif_core::Transition { from: Some(1), to: 0, conds: Vec::new(), at_end: true, blend: 0.2 },
            ],
            any_pos: None,
        };
        doc.machines.push(src_machine.clone());
        let sheet = bake_sheet(&doc, &GodotExport { fps: 4.0, ..GodotExport::default() }).unwrap();
        let dir = std::env::temp_dir().join(format!("motif-godot-test-{}", std::process::id()));
        let files = write_godot(&dir, "hero", &sheet, Some(&src_machine), true).unwrap();
        assert_eq!(files.len(), 4);
        let tscn = std::fs::read_to_string(dir.join("hero.tscn")).unwrap();
        assert!(tscn.contains("advance_expression = \"(jump) or (speed > 2.0)\""), "{tscn}");
        assert!(tscn.contains("transitions = [\"Start\", \"Idle\", SubResource(\"t_start\"), \"Idle\", \"Jump\", SubResource(\"t0\"), \"Jump\", \"Idle\", SubResource(\"t1\")]"), "{tscn}");
        assert!(tscn.contains("switch_mode = 2"), "{tscn}");
        assert!(tscn.contains("texture_filter = 1"), "{tscn}");
        let gd = std::fs::read_to_string(dir.join("hero.gd")).unwrap();
        assert!(gd.contains("@export var speed := 0.0") && gd.contains("func fire_jump() -> void:"), "{gd}");
        let frames = std::fs::read_to_string(dir.join("hero_frames.tres")).unwrap();
        assert!(frames.contains("\"name\": &\"Jump\"") && frames.contains("\"loop\": false"), "{frames}");
        std::fs::remove_dir_all(dir).ok();
    }
}
