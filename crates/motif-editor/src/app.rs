use crate::canvas::{Canvas, View, artboard_rect};
use crate::pathedit::{PenPoint, PointEdit};
use crate::theme::{self, ACCENT, ACCENT_INK, BG, ButtonKind, DANGER, FAINT, FG, GOOD, Icon, KeyState, LINE, MUTED, PANEL, SEL};
use egui::{Align2, Color32, Key, Modifiers, Pos2, Rect, RichText, Sense, Stroke, StrokeKind, TextureHandle, TextureOptions, pos2, vec2};
use motif_core::kurbo::{Affine, Point, Shape};
use motif_core::{Color, Document, Prop, Purpose, ShapeKind, Value};
use motif_pixel::{FrameExport, PIXATOOL_MAX_COLORS, parse_jasc, quantize, render_frames, scene_palette, to_jasc};
use motif_render::{CpuOptions, render_cpu};
use motif_script::{Scene, ScriptError, add_node, delete_key, delete_node, fmt_num, parse, set_key, set_prop};
use std::path::PathBuf;
use std::time::{Instant, SystemTime};

const NEW_UI_KIT: &str = include_str!("../../../examples/ui-kit.msc");
const NEW_SCENE: &str = "artboard \"Сцена\" 320x240 bg #f4f1ff game\n\nrect bg at 160 200 size 280 16 r 8 fill #2a2e39\nellipse hero at 160 120 size 64 fill #ff5a36 stroke #1b1d24 3\n\nanim Idle 1s loop {\n  hero.y 0: 120 ease-in-out | 0.5: 100 ease-in-out | 1: 120\n}\n";
const EXAMPLE: &str = include_str!("../../../examples/bounce.msc");
/// Frame length used by `,` and `.` and shown in the timecode.
pub const FPS: f64 = 60.0;
/// Frames between onion-skin ghosts.
const ONION_STEP: f64 = 6.0;
/// Version shown in the help window; CI sets `MOTIF_BUILD` to the date and commit.
pub const VERSION: &str = match option_env!("MOTIF_BUILD") {
    Some(b) => b,
    None => concat!(env!("CARGO_PKG_VERSION"), " (локальная сборка)"),
};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tool {
    Select,
    Hand,
    Create(ShapeKind),
    Pen,
}

pub enum DragOp {
    Pan,
    Move {
        /// Pointer position (artboard space) when the drag started.
        from: Point,
        items: Vec<MoveItem>,
        /// Bounds of everything moving at the start, and what it can line up with.
        start_box: Option<motif_core::kurbo::Rect>,
        targets: crate::guides::Targets,
    },
    /// Rubber-band selection; with Shift it adds to the selection.
    Marquee {
        from: Point,
        to: Point,
        add: bool,
    },
    Resize {
        node: String,
        /// Artboard space to the node's local space, fixed at drag start.
        to_local: Affine,
        start: (f64, f64),
    },
    Rotate {
        node: String,
        center: Point,
        start_angle: f64,
        start_rot: f64,
    },
    Create {
        kind: ShapeKind,
        from: Point,
        to: Point,
    },
    /// The tip of a bone: turns it, and in Design also sets its length.
    BoneTip {
        node: String,
        /// The bone's joint on the artboard.
        joint: Point,
        /// Direction of the parent's x axis, radians.
        parent_angle: f64,
    },
}

/// One object moved by a drag.
pub struct MoveItem {
    pub(crate) node: String,
    /// Node origin in artboard space when the drag started.
    pub(crate) origin: Point,
    /// Artboard space to the node's parent space.
    pub(crate) to_parent: Affine,
}

/// A key picked in the timeline: node, property and time.
pub type KeyRef = motif_script::KeyAt;

/// Keys being dragged in the timeline: the grabbed key and the time it is pulled to. The other
/// selected keys follow; `stretch` (Alt) scales their times instead of shifting them.
#[derive(Clone, Debug)]
pub struct KeyDrag {
    pub grabbed: KeyRef,
    pub to: f64,
    pub stretch: bool,
    /// Keys moved with it: the selection, or the keys of an object at one time.
    pub keys: Vec<KeyRef>,
}

impl KeyDrag {
    /// Where a key at `t` goes while dragging.
    pub fn map(&self) -> impl Fn(f64) -> f64 + use<> {
        let from = self.grabbed.time;
        let to = self.to;
        let (lo, hi) = self.keys.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), k| (lo.min(k.time), hi.max(k.time)));
        // Stretching pulls the grabbed end and pins the opposite end of the selection.
        let pivot = if (from - lo).abs() < 0.005 { hi } else { lo };
        let stretch = self.stretch && (from - pivot).abs() > 0.005;
        move |t| if stretch { (pivot + (t - pivot) * (to - pivot) / (from - pivot)).max(0.0) } else { (t + to - from).max(0.0) }
    }

    pub fn moves(&self, k: &KeyRef) -> bool {
        self.keys.iter().any(|x| x.node == k.node && x.prop == k.prop && (x.time - k.time).abs() < 0.005)
    }
}

pub struct PixelPreview {
    pub on: bool,
    pub width: u32,
    pub palette: Option<(String, Vec<Color>)>,
    texture: Option<TextureHandle>,
}

pub struct ExportSettings {
    fps: f64,
    scale: f64,
    antialias: bool,
    transparent: bool,
}

pub struct App {
    pub(crate) path: Option<PathBuf>,
    pub(crate) src: String,
    saved_src: String,
    disk_mtime: Option<SystemTime>,
    pub(crate) scene: Scene,
    pub(crate) errors: Vec<ScriptError>,
    pub(crate) selected: Option<String>,
    /// Further selected objects besides `selected` (Shift-click, marquee).
    pub(crate) also: Vec<String>,
    /// Last copy: its text on the system clipboard and the declarations to paste.
    pub(crate) clipboard: (String, Vec<motif_script::Snippet>),
    /// Undo-step name shared by a batch of edits (moving or aligning several objects).
    pub(crate) edit_group: Option<String>,
    pub(crate) hovered: Option<String>,
    view: Option<View>,
    pub(crate) anim: Option<String>,
    /// Animation to return to when switching back to Animate.
    pub(crate) last_anim: Option<String>,
    pub(crate) time: f64,
    pub(crate) playing: bool,
    /// The bottom panel shows the state machine instead of the timeline.
    pub(crate) bottom_machine: bool,
    /// The machine picked in the bottom panel.
    pub(crate) machine: Option<String>,
    /// The state, transition or input picked in the machine graph.
    pub(crate) machine_pick: Option<motif_script::MachineItem>,
    /// The machine playing on the canvas.
    pub(crate) run: Option<crate::machine_ui::Run>,
    /// The pointer listener being put together in the machine panel: object, event, input.
    pub(crate) listen_draft: (String, motif_core::PointerEvent, String),
    /// The weight brush of skinned shapes.
    pub(crate) brush_w: crate::weights_ui::WeightBrush,
    pub(crate) graph: crate::machine_ui::Graph,
    pub(crate) undo: Vec<String>,
    redo: Vec<String>,
    last_checkpoint: (String, Instant),
    pub(crate) tool: Tool,
    pub(crate) drag: Option<DragOp>,
    /// Values changed by the drag in progress: shown on the canvas, written to the script on release.
    live: Vec<(String, Prop, Value)>,
    pub(crate) snap: crate::snap::Snap,
    /// Smart guides shown while moving.
    guides: Vec<crate::guides::Guide>,
    /// Points placed by the pen so far.
    pub(crate) pen: Vec<PenPoint>,
    pub(crate) pen_dragging: bool,
    pub(crate) point_edit: Option<PointEdit>,
    /// Onion skin: faint poses before (red) and after (green) the current time while paused.
    pub(crate) onion: bool,
    /// Keys picked in the timeline; the last one is shown in the curve picker.
    pub(crate) sel_keys: Vec<KeyRef>,
    pub(crate) key_drag: Option<KeyDrag>,
    /// Copied keys: their text on the system clipboard and the keys.
    pub(crate) key_clip: (String, Vec<motif_script::KeyCopy>),
    /// Where a frame-select drag in the timeline started.
    pub(crate) key_marquee: Option<egui::Pos2>,
    /// An event flag being dragged on the timeline: its name, its time and where it is now.
    pub(crate) event_drag: Option<(String, f64, f64)>,
    /// The name typed in an event flag's menu.
    pub(crate) event_name: String,
    /// The timeline shows value curves instead of key lanes.
    pub(crate) curves: bool,
    /// The property whose curve is edited in the graph.
    pub(crate) curve_focus: Option<(String, Prop)>,
    pub(crate) curve_drag: Option<crate::timeline::CurveDrag>,
    pub(crate) show_script: bool,
    show_export: bool,
    /// A file action waiting for an answer about unsaved changes.
    pending_file: Option<FileAction>,
    show_help: bool,
    pub(crate) show_syntax: bool,
    space_used_for_pan: bool,
    hex_edit: Option<(Prop, String)>,
    pixel: PixelPreview,
    export: ExportSettings,
    pixa: crate::pixabridge::Bridge,
    /// For work that finishes on another thread and must wake the window (re-export on save).
    ctx: egui::Context,
    canvas: Canvas,
    pub(crate) backdrop_img: crate::importer::BackdropCache,
    pub(crate) status: String,
    last_poll: Instant,
    pointer_doc: Option<Point>,
    frame_ms: f32,
    pub(crate) layers: crate::layers::Layers,
    /// Soft-body frames already simulated, for the script and animation they were simulated for.
    sim: std::cell::RefCell<(u64, motif_core::SimCache)>,
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>, path: Option<PathBuf>) -> Self {
        theme::install(&cc.egui_ctx);
        let (src, status) = match &path {
            Some(p) => match std::fs::read_to_string(p) {
                Ok(s) => (s, format!("Открыт {}", p.display())),
                Err(e) => (NEW_SCENE.to_string(), format!("Не удалось открыть {}: {e}", p.display())),
            },
            None => (EXAMPLE.to_string(), "Открыт пример. Сохраните его, чтобы работать с файлом.".into()),
        };
        let scene = parse(&src).or_else(|_| parse(NEW_SCENE)).expect("built-in scene parses");
        let mut app = App {
            disk_mtime: path.as_ref().and_then(|p| mtime(p)),
            pending_file: None,
            path,
            saved_src: src.clone(),
            src,
            scene,
            errors: Vec::new(),
            selected: None,
            also: Vec::new(),
            clipboard: (String::new(), Vec::new()),
            edit_group: None,
            hovered: None,
            view: None,
            anim: None,
            last_anim: None,
            time: 0.0,
            playing: false,
            bottom_machine: false,
            machine: None,
            machine_pick: None,
            run: None,
            listen_draft: (String::new(), motif_core::PointerEvent::Hover, String::new()),
            graph: Default::default(),
            undo: Vec::new(),
            redo: Vec::new(),
            last_checkpoint: (String::new(), Instant::now()),
            tool: Tool::Select,
            drag: None,
            live: Vec::new(),
            snap: Default::default(),
            pen: Vec::new(),
            pen_dragging: false,
            point_edit: None,
            brush_w: Default::default(),
            onion: false,
            sel_keys: Vec::new(),
            key_drag: None,
            key_clip: (String::new(), Vec::new()),
            key_marquee: None,
            event_drag: None,
            event_name: String::new(),
            curves: false,
            curve_focus: None,
            curve_drag: None,
            show_script: true,
            show_export: false,
            show_help: false,
            show_syntax: false,
            space_used_for_pan: false,
            hex_edit: None,
            pixel: PixelPreview { on: false, width: 96, palette: None, texture: None },
            export: ExportSettings { fps: 12.0, scale: 1.0, antialias: true, transparent: true },
            pixa: crate::pixabridge::Bridge::load(),
            ctx: cc.egui_ctx.clone(),
            canvas: Canvas::new(),
            guides: Vec::new(),
            backdrop_img: Default::default(),
            status,
            last_poll: Instant::now(),
            pointer_doc: None,
            frame_ms: 16.0,
            sim: Default::default(),
            layers: Default::default(),
        };
        app.reparse();
        app
    }

    // ---------- script state ----------

    pub(crate) fn reparse(&mut self) {
        match motif_script::parse_in(&self.src, self.path.as_ref().and_then(|p| p.parent())) {
            Ok(scene) => {
                self.scene = scene;
                self.errors.clear();
                if let Some(id) = &self.selected
                    && self.scene.doc.node(id).is_none()
                {
                    self.selected = None;
                }
                if let Some(a) = &self.anim
                    && self.scene.doc.animation(a).is_none()
                {
                    self.anim = None;
                }
                let keys = std::mem::take(&mut self.sel_keys);
                self.sel_keys = keys.into_iter().filter(|k| self.key_exists(k)).collect();
                self.prune_selection();
                self.prune_machine();
            }
            Err(errors) => self.errors = errors,
        }
    }

    fn key_exists(&self, k: &KeyRef) -> bool {
        self.current_anim()
            .and_then(|a| a.tracks.iter().find(|t| t.node == k.node && t.prop == k.prop))
            .is_some_and(|t| t.keys.iter().any(|key| (key.time - k.time).abs() < 0.005))
    }

    /// Saves an undo step, merging rapid changes of the same kind (typing, dragging) into one.
    pub(crate) fn checkpoint(&mut self, kind: &str) {
        if self.last_checkpoint.0 != kind || self.last_checkpoint.1.elapsed().as_secs_f32() > 0.8 {
            self.undo.push(self.src.clone());
            if self.undo.len() > 300 {
                self.undo.remove(0);
            }
            self.redo.clear();
        }
        self.last_checkpoint = (kind.to_string(), Instant::now());
    }

    /// Replaces the script with the result of an edit, as one undo step.
    pub(crate) fn apply(&mut self, kind: &str, new_src: Option<String>) -> bool {
        match new_src {
            Some(s) if s != self.src => {
                self.checkpoint(kind);
                self.src = s;
                self.reparse();
                true
            }
            _ => false,
        }
    }

    pub(crate) fn can_edit(&mut self) -> bool {
        if self.errors.is_empty() {
            return true;
        }
        self.status = "Сначала исправьте ошибки в скрипте: правки мышью пишутся в рабочую версию".into();
        false
    }

    /// Applies a visual edit: in Design it changes the value in the script, in Animate it records
    /// a key at the current time instead.
    pub(crate) fn edit(&mut self, node: &str, prop: Prop, value: Value) {
        if !self.can_edit() {
            return;
        }
        let new_src = match self.key_time() {
            Some((anim, t)) => {
                self.playing = false;
                self.status = format!("Ключ {node}.{} на {} с в «{anim}»", prop.path_name(), fmt_num(t));
                set_key(&self.src, &self.scene, &anim, node, prop, t, value)
            }
            None if prop.is_world() => {
                let mut w = self.scene.doc.world;
                w.set(prop, value);
                Some(motif_script::set_world(&self.src, &self.scene, &w))
            }
            None => set_prop(&self.src, &self.scene, node, prop, value),
        };
        let kind = self.edit_group.clone().unwrap_or_else(|| format!("{node}.{}", prop.path_name()));
        self.apply(&kind, new_src);
    }

    /// The diamond next to a property: removes the key at the current time, or keys the current value.
    pub(crate) fn toggle_key(&mut self, node: &str, prop: Prop) {
        let Some((anim, t)) = self.key_time() else { return };
        if self.key_state(node, prop) == KeyState::Key {
            let out = delete_key(&self.src, &self.scene, &anim, node, prop, t);
            self.apply("key", out);
            self.status = format!("Ключ {node}.{} на {} с удалён", prop.path_name(), fmt_num(t));
        } else if prop.is_world() {
            let value = self.display_doc().world.get(prop);
            self.edit(node, prop, value);
        } else if let Some(n) = self.display_doc().node(node) {
            let value = n.get(prop);
            self.edit(node, prop, value);
        }
    }

    pub(crate) fn current_anim(&self) -> Option<&motif_core::Animation> {
        self.scene.doc.animation(self.anim.as_deref()?)
    }

    /// The selected animation and the time within it, rounded the way keys are written.
    pub(crate) fn key_time(&self) -> Option<(String, f64)> {
        let anim = self.current_anim()?;
        Some((anim.name.clone(), (anim.local_time(self.time) * 100.0).round() / 100.0))
    }

    pub(crate) fn key_state(&self, node: &str, prop: Prop) -> KeyState {
        let Some((_, t)) = self.key_time() else { return KeyState::Hidden };
        let Some(track) = self.current_anim().and_then(|a| a.tracks.iter().find(|tr| tr.node == node && tr.prop == prop)) else {
            return KeyState::None;
        };
        if track.keys.iter().any(|k| (k.time - t).abs() < 0.005) { KeyState::Key } else { KeyState::Track }
    }

    pub(crate) fn set_mode_animate(&mut self, on: bool) {
        if on {
            let name =
                self.last_anim.clone().filter(|a| self.scene.doc.animation(a).is_some()).or_else(|| self.scene.doc.animations.first().map(|a| a.name.clone()));
            match name {
                Some(n) => self.select_anim(n),
                None => {
                    let (out, name) = motif_script::add_anim(&self.src, &self.scene);
                    if self.apply("anim", Some(out)) {
                        self.select_anim(name);
                    }
                }
            }
        } else {
            if self.anim.is_some() {
                self.last_anim = self.anim.clone();
            }
            self.anim = None;
            self.playing = false;
            self.sel_keys.clear();
        }
    }

    /// Marks the scene for interface or game work; the panels follow.
    fn set_purpose(&mut self, purpose: Purpose) {
        if purpose == self.scene.doc.purpose || !self.can_edit() {
            return;
        }
        if self.apply("purpose", motif_script::set_purpose(&self.src, purpose)) {
            self.status = match purpose {
                Purpose::Ui => "Режим UI: экспорт UI-набора и состояния кнопок на виду, кости, физика и 3D спрятаны".into(),
                Purpose::Game => "Режим «Игра»: кости, физика, 3D и кисти; раздел «Интерфейс» спрятан".into(),
                Purpose::Any => "Все инструменты на виду".into(),
            };
        }
    }

    /// Adds the animations a kit takes its states from, and opens the first in Animate.
    fn add_ui_states(&mut self) {
        if !self.can_edit() {
            return;
        }
        let (out, added) = motif_script::add_states(&self.src, &self.scene, &["Hover", "Press"], 0.25);
        self.apply("states", Some(out));
        self.set_mode_animate(true);
        self.select_anim("Hover".into());
        self.status = if added.is_empty() {
            "Анимации Hover и Press уже есть: правки в Animate ставят в них ключи".into()
        } else {
            format!("Добавлены анимации {}: меняйте свойства в Animate, последний кадр станет состоянием", added.join(", "))
        };
    }

    pub(crate) fn select_anim(&mut self, name: String) {
        self.run = None;
        if self.anim.as_deref() != Some(name.as_str()) {
            self.time = 0.0;
            self.sel_keys.clear();
        }
        self.last_anim = Some(name.clone());
        self.anim = Some(name);
    }

    fn undo(&mut self) {
        if let Some(prev) = self.undo.pop() {
            self.redo.push(std::mem::replace(&mut self.src, prev));
            self.last_checkpoint.0.clear();
            self.reparse();
        }
    }

    fn redo(&mut self) {
        if let Some(next) = self.redo.pop() {
            self.undo.push(std::mem::replace(&mut self.src, next));
            self.last_checkpoint.0.clear();
            self.reparse();
        }
    }

    fn dirty(&self) -> bool {
        self.src != self.saved_src
    }

    pub(crate) fn display_doc(&self) -> Document {
        let mut doc = match (self.running(), self.current_anim()) {
            (Some((m, player)), _) => player.pose(m, &self.scene.doc),
            (None, Some(a)) => self.pose_anim(a, self.time),
            (None, None) => self.scene.doc.clone(),
        };
        if let Some(e) = &self.point_edit
            && let Some(o) = &e.live
            && let Some(i) = doc.index_of(&e.node)
        {
            doc.nodes[i].geometry = o.to_bez();
        }
        for (node, prop, value) in &self.live {
            if let Some(i) = doc.index_of(node) {
                doc.nodes[i].set(*prop, *value);
            }
        }
        // The weight stroke being drawn bends the shape at once.
        if let Some((node, stroke)) = self.brush_w.live()
            && let Some(skin) = doc.index_of(node).and_then(|i| doc.nodes[i].skin.as_mut())
        {
            skin.paint.push(stroke);
        }
        doc.rig();
        doc
    }

    /// `anim` posed at `t`, reusing the soft-body frames simulated for the same script.
    pub(crate) fn pose_anim(&self, anim: &motif_core::Animation, t: f64) -> Document {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        (&self.src, &anim.name).hash(&mut h);
        let key = h.finish();
        let mut sim = self.sim.borrow_mut();
        if sim.0 != key {
            *sim = (key, motif_core::SimCache::default());
        }
        anim.pose_with(&self.scene.doc, t, &mut sim.1)
    }

    /// A drag step: shown at once, written to the script when the mouse is released.
    pub(crate) fn preview(&mut self, node: &str, prop: Prop, value: Value) {
        match self.live.iter_mut().find(|(n, p, _)| n == node && *p == prop) {
            Some(slot) => slot.2 = value,
            None => self.live.push((node.to_string(), prop, value)),
        }
    }

    /// Writes the values of a finished drag to the script as one undo step.
    fn commit_live(&mut self) {
        let live = std::mem::take(&mut self.live);
        if live.is_empty() {
            return;
        }
        self.edit_group = Some("drag".into());
        for (node, prop, value) in live {
            self.edit(&node, prop, value);
        }
        self.edit_group = None;
    }

    /// The current pose with onion-skin ghosts behind it, when onion skin is on and the
    /// animation is paused.
    fn onion_doc(&self) -> Option<Document> {
        let anim = self.current_anim().filter(|_| self.onion && !self.playing)?;
        let now = anim.local_time(self.time);
        let mut out = self.display_doc();
        let current = std::mem::take(&mut out.nodes);
        // Farthest first, so nearer ghosts sit on top.
        for (k, frames) in [-2.0, 2.0, -1.0, 1.0].into_iter().enumerate() {
            let t = now + frames * ONION_STEP / FPS;
            if !anim.looping && !(0.0..=anim.duration).contains(&t) {
                continue;
            }
            let tint = if frames < 0.0 { Color::rgb(0xff, 0x5a, 0x5a) } else { Color::rgb(0x4f, 0xd3, 0x8c) };
            let alpha = if frames.abs() > 1.5 { 0.12 } else { 0.24 };
            let base = out.nodes.len();
            for mut n in self.pose_anim(anim, t).nodes {
                n.id = format!("~onion{k}~{}", n.id);
                n.parent = n.parent.map(|p| p + base);
                n.opacity *= alpha;
                let mix = |c: Color| Color { r: mix_u8(c.r, tint.r), g: mix_u8(c.g, tint.g), b: mix_u8(c.b, tint.b), a: c.a };
                if let Some(g) = n.gradient.take() {
                    n.fill = g.colors.first().copied();
                }
                n.fill = n.fill.map(mix);
                n.stroke = n.stroke.map(mix);
                out.nodes.push(n);
            }
        }
        let base = out.nodes.len();
        out.nodes.extend(current.into_iter().map(|mut n| {
            n.parent = n.parent.map(|p| p + base);
            n
        }));
        Some(out)
    }

    pub(crate) fn delete_selection(&mut self) {
        if !self.sel_keys.is_empty()
            && let Some(anim) = self.anim.clone()
        {
            let keys = std::mem::take(&mut self.sel_keys);
            let out = motif_script::delete_keys(&self.src, &self.scene, &anim, &keys);
            if self.apply("key", out) {
                self.status = match keys.as_slice() {
                    [k] => format!("Ключ {}.{} на {} с удалён", k.node, k.prop.path_name(), fmt_num(k.time)),
                    _ => format!("Удалено ключей: {}", keys.len()),
                };
            }
            return;
        }
        let Some(id) = self.selected.clone() else { return };
        if !self.can_edit() {
            return;
        }
        if !self.also.is_empty() {
            self.delete_selected_nodes();
            return;
        }
        match delete_node(&self.src, &self.scene, &id) {
            Some(out) => {
                self.apply("delete", Some(out));
                self.status = format!("Удалён «{id}» вместе с его ключами");
            }
            None => self.status = format!("«{id}» создан циклом: удалите или измените цикл в скрипте"),
        }
    }

    // ---------- files ----------

    /// Opens `update.bat` in its own console window; it asks to close the editor before replacing it.
    fn run_updater(&mut self, updater: &std::path::Path) {
        let result = std::process::Command::new("cmd").arg("/C").arg("start").arg("Motif update").arg(updater).spawn();
        self.status = match result {
            Ok(_) => "Открыто окно обновления. Сохраните сцену и закройте редактор, когда оно попросит".into(),
            Err(e) => format!("Не удалось запустить {}: {e}", updater.display()),
        };
    }

    /// Runs a file action, first asking what to do with unsaved changes.
    fn file_action(&mut self, action: FileAction) {
        if self.dirty() {
            self.pending_file = Some(action);
        } else {
            self.run_file_action(action);
        }
    }

    fn run_file_action(&mut self, action: FileAction) {
        match action {
            FileAction::New(template) => self.new_scene(template),
            FileAction::Open => self.open(),
        }
    }

    /// «Сохранить», «Не сохранять» or «Отмена» for the waiting file action.
    fn unsaved_window(&mut self, ctx: &egui::Context) {
        let Some(action) = self.pending_file else { return };
        let mut answer = None;
        egui::Window::new("Несохранённые правки").collapsible(false).resizable(false).anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0)).show(ctx, |ui| {
            let name = self.path.as_ref().and_then(|p| p.file_name()).map_or("без имени.msc".into(), |n| n.to_string_lossy().into_owned());
            ui.label(format!("В {name} есть несохранённые правки. Сохранить их?"));
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if theme::button(ui, "Сохранить", ButtonKind::Primary).clicked() {
                    answer = Some(true);
                }
                if theme::button(ui, "Не сохранять", ButtonKind::Normal).clicked() {
                    answer = Some(false);
                }
                if theme::button(ui, "Отмена", ButtonKind::Normal).clicked() {
                    self.pending_file = None;
                }
            });
        });
        let Some(save) = answer else { return };
        self.pending_file = None;
        if save {
            self.save(false);
            if self.dirty() {
                // The save dialog was cancelled or failed: keep the work.
                return;
            }
        }
        // Without saving, the edits stay marked as unsaved if the open dialog is cancelled.
        self.run_file_action(action);
    }

    fn new_scene(&mut self, template: &str) {
        self.checkpoint("file");
        self.src = template.into();
        // An untouched template is not unsaved work: the next «Новая сцена» or «Открыть» goes
        // straight through.
        self.saved_src = template.into();
        self.status = "Новая сцена. Сохраните её, чтобы работать с файлом.".into();
        self.selected = None;
        self.also.clear();
        self.anim = None;
        self.path = None;
        self.view = None;
        self.reparse();
    }

    fn open(&mut self) {
        let Some(p) = rfd::FileDialog::new().add_filter("Сцена Motif", &["msc"]).pick_file() else { return };
        match std::fs::read_to_string(&p) {
            Ok(s) => {
                self.undo.push(std::mem::replace(&mut self.src, s.clone()));
                self.saved_src = s;
                self.disk_mtime = mtime(&p);
                self.status = format!("Открыт {}", p.display());
                self.path = Some(p);
                self.selected = None;
                self.view = None;
                self.anim = None;
                self.reparse();
            }
            Err(e) => self.status = format!("Не удалось открыть {}: {e}", p.display()),
        }
    }

    fn save(&mut self, save_as: bool) {
        let path = match (&self.path, save_as) {
            (Some(p), false) => p.clone(),
            _ => match rfd::FileDialog::new().add_filter("Сцена Motif", &["msc"]).set_file_name("scene.msc").save_file() {
                Some(p) => p,
                None => return,
            },
        };
        match std::fs::write(&path, &self.src) {
            Ok(()) => {
                self.saved_src = self.src.clone();
                self.disk_mtime = mtime(&path);
                self.status = format!("Сохранено: {}", path.display());
                self.path = Some(path.clone());
                if let Some(link) = self.pixa.link(&path).cloned() {
                    let ctx = self.ctx.clone();
                    self.status = format!("Сохранено. {}", self.godot_to(&link.dir, link.node, &ctx));
                }
            }
            Err(e) => self.status = format!("Не удалось сохранить {}: {e}", path.display()),
        }
    }

    /// Picks up changes made to the file in another editor (VS Code, Notepad++…).
    fn poll_disk(&mut self) {
        if self.last_poll.elapsed().as_millis() < 500 {
            return;
        }
        self.last_poll = Instant::now();
        let Some(path) = self.path.clone() else { return };
        let now = mtime(&path);
        if now.is_none() || now == self.disk_mtime {
            return;
        }
        self.disk_mtime = now;
        if self.dirty() {
            self.status = "Файл изменён другой программой, но здесь есть несохранённые правки — оставляю версию редактора".into();
            return;
        }
        if let Ok(s) = std::fs::read_to_string(&path)
            && s != self.src
        {
            self.undo.push(std::mem::replace(&mut self.src, s.clone()));
            self.saved_src = s;
            self.reparse();
            self.status = "Файл обновлён с диска".into();
        }
    }

    /// The frames the export window would save: the current animation, or the scene as one frame.
    fn export_render(&self) -> (String, Result<Vec<tiny_skia::Pixmap>, String>) {
        let render = CpuOptions { scale: self.export.scale, antialias: self.export.antialias, background: !self.export.transparent };
        let doc = &self.scene.doc;
        match &self.anim {
            Some(a) => (a.clone(), render_frames(doc, a, &FrameExport { fps: self.export.fps, render })),
            None => ("scene".to_string(), render_cpu(doc, render).map(|p| vec![p]).ok_or_else(|| "не удалось создать изображение".to_string())),
        }
    }

    fn export_frames(&mut self) {
        let Some(dir) = rfd::FileDialog::new().set_title("Папка для PNG-кадров").pick_folder() else { return };
        let (name, frames) = self.export_render();
        let result = frames.and_then(|frames| {
            for (i, px) in frames.iter().enumerate() {
                let path = dir.join(format!("{name}_{i:04}.png"));
                px.save_png(&path).map_err(|e| format!("{}: {e}", path.display()))?;
            }
            Ok(frames.len())
        });
        self.status = match result {
            Ok(n) => format!("Сохранено кадров: {n} в {}", dir.display()),
            Err(e) => format!("Экспорт не удался: {e}"),
        };
    }

    /// Every UI piece of the scene in each of its states, plus a Godot theme over the pictures.
    fn export_kit(&mut self) {
        let Some(dir) = rfd::FileDialog::new().set_title("Папка для UI-набора").pick_folder() else { return };
        let render = CpuOptions { scale: self.export.scale, antialias: self.export.antialias, background: false };
        let theme = self.path.as_ref().and_then(|p| p.file_stem()).map_or("ui".to_string(), |s| s.to_string_lossy().into_owned());
        let result = motif_pixel::kit::render_kit(&self.scene.doc, render)
            .and_then(|items| motif_pixel::kit::write_kit(&dir, &theme, &items).map(|files| (items.len(), files.len())));
        self.status = match result {
            Ok((pieces, files)) => format!("UI-набор: деталей {pieces}, файлов {files} в {} (тема {theme}.tres)", dir.display()),
            Err(e) => format!("UI-набор не сохранён: {e}"),
        };
    }

    /// Every animation of the scene (or of the selected object) for Godot: atlas, SpriteFrames,
    /// a scene with the sprite and, with a state machine, its AnimationTree. A saved scene stays
    /// linked to the folder and is exported there again on every save.
    fn export_godot(&mut self, ctx: &egui::Context) {
        let Some(dir) = rfd::FileDialog::new().set_title("Папка в проекте Godot").pick_folder() else { return };
        let only = self.selected.clone().filter(|id| self.scene.doc.node(id).is_some());
        if let Some(path) = self.path.clone() {
            self.pixa.set_link(&path, Some((dir.clone(), only.clone())));
        }
        self.status = self.godot_to(&dir, only, ctx);
    }

    /// Exports into `dir` and returns the status line; with PixaTool it only starts the job.
    fn godot_to(&mut self, dir: &std::path::Path, only: Option<String>, ctx: &egui::Context) -> String {
        let render = CpuOptions { scale: self.export.scale, antialias: self.export.antialias, background: false };
        let only = only.filter(|id| self.scene.doc.node(id).is_some());
        let stem = self.path.as_ref().and_then(|p| p.file_stem()).map_or("scene".to_string(), |s| s.to_string_lossy().into_owned());
        let name = only.clone().unwrap_or(stem);
        let opts = motif_pixel::godot::GodotExport { fps: self.export.fps, render, only, pivot: None };
        if self.pixa.godot && self.pixa.exe.is_some() {
            if self.pixa.job.is_some() {
                return "Godot: PixaTool ещё занят прошлой выгрузкой".into();
            }
            let (doc, machine) = (self.scene.doc.clone(), self.scene.doc.machines.first().cloned());
            return match self.pixa.start_godot(doc, opts, dir.to_path_buf(), name, machine, ctx) {
                Ok(()) => "Godot: снимаю кадры для PixaTool…".into(),
                Err(e) => format!("Экспорт в Godot не удался: {e}"),
            };
        }
        let doc = &self.scene.doc;
        let result = motif_pixel::godot::bake_sheet(doc, &opts)
            .and_then(|sheet| motif_pixel::godot::write_godot(dir, &name, &sheet, doc.machines.first(), !self.export.antialias).map(|f| (sheet, f.len())));
        match result {
            Ok((sheet, files)) => {
                let tree = doc.machines.first().map_or(String::new(), |m| format!(", машина «{}» в AnimationTree", m.name));
                format!("Godot: {name}.tscn, анимаций {}, кадров {} в атласе{tree} — {files} файлов в {}", sheet.anims.len(), sheet.cells.len(), dir.display())
            }
            Err(e) => format!("Экспорт в Godot не удался: {e}"),
        }
    }

    fn pixelize_frames(&mut self, ctx: &egui::Context) {
        let Some(dir) = rfd::FileDialog::new().set_title("Папка для кадров после PixaTool").pick_folder() else { return };
        let (name, frames) = self.export_render();
        self.status = match frames.and_then(|f| self.pixa.start(&f, &name, &dir, ctx).map(|()| f.len())) {
            Ok(n) => format!("PixaTool: обрабатываю {n} кадров…"),
            Err(e) => format!("Пикселизация не удалась: {e}"),
        };
    }

    fn export_palette(&mut self) {
        let colors = scene_palette(&self.scene.doc);
        let Some(path) = rfd::FileDialog::new().add_filter("Палитра JASC", &["pal"]).set_file_name("palette.pal").save_file() else { return };
        self.status = match std::fs::write(&path, to_jasc(&colors)) {
            Ok(()) => format!("Палитра на {} цветов сохранена: {}", colors.len(), path.display()),
            Err(e) => format!("Не удалось сохранить палитру: {e}"),
        };
    }

    fn load_palette(&mut self) {
        let Some(path) = rfd::FileDialog::new().add_filter("Палитра JASC", &["pal"]).pick_file() else { return };
        match std::fs::read_to_string(&path).map_err(|e| e.to_string()).and_then(|t| parse_jasc(&t)) {
            Ok(colors) => {
                let name = path.file_name().map_or("палитра".into(), |n| n.to_string_lossy().into_owned());
                self.status = format!("Палитра {name}: {} цветов", colors.len());
                self.pixel.palette = Some((name, colors));
                self.pixel.on = true;
            }
            Err(e) => self.status = format!("Не удалось прочитать палитру: {e}"),
        }
    }

    // ---------- top bar ----------

    fn top_bar(&mut self, ui: &mut egui::Ui) {
        let rect = ui.max_rect();
        ui.horizontal_centered(|ui| {
            ui.add_space(4.0);
            // Brand mark.
            let (m, _) = ui.allocate_exact_size(vec2(22.0, 22.0), Sense::hover());
            ui.painter().rect_filled(m, 5.0, ACCENT);
            ui.painter().text(m.center() + vec2(0.0, 0.5), Align2::CENTER_CENTER, "M", theme::semibold(13.0), ACCENT_INK);
            ui.label(RichText::new("Motif").font(theme::semibold(13.5)).color(FG));
            ui.add_space(4.0);
            let file = ui.add(egui::Button::new(RichText::new("Файл").color(MUTED)).frame(false));
            egui::Popup::menu(&file).show(|ui| {
                ui.set_min_width(220.0);
                if ui.add(egui::Button::new("Новая сцена")).clicked() {
                    self.file_action(FileAction::New(NEW_SCENE));
                }
                if ui
                    .add(egui::Button::new("Новый UI-набор"))
                    .on_hover_text("Заготовка: кнопка с состояниями, панель, полоса, флажок, подсказка — для экспорта в Godot")
                    .clicked()
                {
                    self.file_action(FileAction::New(NEW_UI_KIT));
                }
                if ui.add(egui::Button::new("Открыть…").shortcut_text("Ctrl+O")).clicked() {
                    self.file_action(FileAction::Open);
                }
                if ui.add(egui::Button::new("Сохранить").shortcut_text("Ctrl+S")).clicked() {
                    self.save(false);
                }
                if ui.add(egui::Button::new("Сохранить как…").shortcut_text("Ctrl+Shift+S")).clicked() {
                    self.save(true);
                }
                ui.separator();
                if ui.add(egui::Button::new("Импорт SVG…")).on_hover_text("Фигуры из SVG станут контурами в новой группе").clicked()
                {
                    self.pick_svg();
                }
                if ui.add(egui::Button::new("Подложка PNG…")).on_hover_text("Картинка-референс под фигурами, только в редакторе").clicked()
                {
                    self.pick_backdrop();
                }
                if let Some(updater) = updater_path() {
                    ui.separator();
                    if ui.add(egui::Button::new("Проверить обновления…")).on_hover_text(format!("Версия {VERSION}")).clicked() {
                        self.run_updater(&updater);
                    }
                }
            });
            let name = self.path.as_ref().and_then(|p| p.file_name()).map_or("без имени.msc".into(), |n| n.to_string_lossy().into_owned());
            let folder = self.path.as_ref().and_then(|p| p.parent()).and_then(|p| p.file_name()).map(|n| n.to_string_lossy().into_owned());
            let mut job = egui::text::LayoutJob::default();
            let fmt = |c: Color32, f: egui::FontId| egui::TextFormat { color: c, font_id: f, ..Default::default() };
            if let Some(f) = folder {
                job.append(&format!("{f} / "), 0.0, fmt(MUTED, theme::ui_font(13.0)));
            }
            job.append(&name, 0.0, fmt(FG, theme::ui_font(13.0)));
            if self.dirty() {
                job.append(" •", 0.0, fmt(ACCENT, theme::ui_font(15.0)));
            }
            job.append(&format!(" · артборд «{}»", self.scene.doc.artboard.name), 0.0, fmt(MUTED, theme::ui_font(13.0)));
            ui.label(job);
        });

        // Mode switch in the middle of the bar.
        let seg_w = 176.0;
        let seg_rect = Rect::from_center_size(rect.center(), vec2(seg_w, 30.0));
        let mut seg_ui = ui.new_child(egui::UiBuilder::new().max_rect(seg_rect).layout(egui::Layout::left_to_right(egui::Align::Center)));
        let mode = usize::from(self.anim.is_some());
        if let Some(i) = theme::segmented(&mut seg_ui, &["Design", "Animate"], mode, true) {
            self.set_mode_animate(i == 1);
        }

        // Right side.
        let right = Rect::from_min_max(pos2(rect.center().x + seg_w / 2.0 + 8.0, rect.top()), rect.max);
        let mut rui = ui.new_child(egui::UiBuilder::new().max_rect(right).layout(egui::Layout::right_to_left(egui::Align::Center)));
        rui.add_space(4.0);
        if theme::button(&mut rui, "?", ButtonKind::Normal).on_hover_text("Горячие клавиши и версия").clicked() {
            self.show_help = !self.show_help;
        }
        if theme::button(&mut rui, "Экспорт", ButtonKind::Primary).clicked() {
            self.show_export = !self.show_export;
        }
        let px_kind = if self.pixel.on { ButtonKind::On } else { ButtonKind::Normal };
        if theme::button(&mut rui, "Пиксели", px_kind)
            .on_hover_text("Пиксельное превью: рендер без сглаживания в низком разрешении с палитрой — как будет после PixaTool")
            .clicked()
        {
            self.pixel.on = !self.pixel.on;
        }
        if self.pixel.on {
            rui.add(egui::DragValue::new(&mut self.pixel.width).range(8..=1024).suffix(" px"));
        }
        if self.shown_machine().is_some_and(|m| !m.listeners.is_empty()) {
            let kind = if self.run.is_some() { ButtonKind::On } else { ButtonKind::Normal };
            if theme::button(&mut rui, if self.run.is_some() { "■ Проверка" } else { "▶ Проверить" }, kind)
                .on_hover_text("Живой предпросмотр: машина состояний играет, а мышь на холсте наводит, нажимает и кликает по объектам")
                .clicked()
            {
                self.toggle_run();
            }
        }
        let zoom = self.view.map_or(100.0, |v| v.zoom * 100.0);
        if theme::button(&mut rui, &format!("{zoom:.0}%"), ButtonKind::Normal).on_hover_text("Вписать артборд (F)").clicked() {
            self.view = None;
        }
        let script_kind = if self.show_script { ButtonKind::On } else { ButtonKind::Normal };
        if theme::button(&mut rui, "Скрипт", script_kind).on_hover_text("Показать или скрыть скрипт сцены (Ctrl+E)").clicked()
        {
            self.show_script = !self.show_script;
        }
        rui.add_space(6.0);
        let purposes = [Purpose::Any, Purpose::Ui, Purpose::Game];
        let at = purposes.iter().position(|p| *p == self.scene.doc.purpose).unwrap_or(0);
        if let Some(i) = theme::segmented(&mut rui, &["Всё", "UI", "Игра"], at, false) {
            self.set_purpose(purposes[i]);
        }
    }

    // ---------- tool rail ----------

    fn tool_rail(&mut self, ui: &mut egui::Ui) {
        ui.vertical_centered(|ui| {
            ui.add_space(8.0);
            ui.spacing_mut().item_spacing.y = 4.0;
            let tools = [(Tool::Select, Icon::Select, "V", "Выделение и перемещение"), (Tool::Hand, Icon::Hand, "H", "Рука: двигать холст")];
            for (tool, icon, key, tip) in tools {
                if theme::tool_button(ui, icon, self.tool == tool, key, tip).clicked() {
                    self.tool = tool;
                }
            }
            let (r, _) = ui.allocate_exact_size(vec2(24.0, 9.0), Sense::hover());
            ui.painter().line_segment([r.left_center(), r.right_center()], Stroke::new(1.0, LINE));
            let shapes = [
                (ShapeKind::Rect, "R", "Прямоугольник: тяните по холсту"),
                (ShapeKind::Ellipse, "O", "Эллипс: тяните по холсту"),
                (ShapeKind::Star, "S", "Звезда: тяните по холсту"),
                (ShapeKind::Polygon, "N", "Многоугольник: тяните по холсту"),
                (ShapeKind::Text, "T", "Текст: клик по холсту (протяжка задаёт высоту букв)"),
            ];
            for (kind, key, tip) in shapes {
                let tool = Tool::Create(kind);
                if theme::tool_button(ui, theme::shape_icon(kind), self.tool == tool, key, tip).clicked() {
                    self.tool = tool;
                }
            }
            let tip = "Кость: тяните от сустава к кончику. С выбранной костью новая становится её продолжением";
            if self.scene.doc.purpose != Purpose::Ui && theme::tool_button(ui, Icon::Bone, self.bone_tool(), "B", tip).clicked() {
                self.tool = Tool::Create(ShapeKind::Bone);
            }
            let tip = "Перо: клик — угол, протяжка — плавная точка, клик по первой точке замыкает";
            if theme::tool_button(ui, Icon::Pen, self.tool == Tool::Pen, "P", tip).clicked() {
                self.tool = Tool::Pen;
            }
            let (r, _) = ui.allocate_exact_size(vec2(24.0, 9.0), Sense::hover());
            ui.painter().line_segment([r.left_center(), r.right_center()], Stroke::new(1.0, LINE));
            self.snap_button(ui);
        });
    }

    // ---------- script ----------

    fn script_panel(&mut self, ui: &mut egui::Ui) {
        let status = if self.errors.is_empty() { ("без ошибок".to_string(), GOOD) } else { (format!("ошибок: {}", self.errors.len()), DANGER) };
        // Header: title, status, and buttons for the syntax help and collapsing the panel.
        let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 32.0), Sense::hover());
        {
            let p = ui.painter();
            p.text(pos2(r.left() + 12.0, r.center().y), Align2::LEFT_CENTER, "СКРИПТ СЦЕНЫ", theme::semibold(11.0), MUTED);
            p.text(pos2(r.right() - 66.0, r.center().y), Align2::RIGHT_CENTER, &status.0, theme::mono(11.0), status.1);
            p.line_segment([r.left_bottom(), r.right_bottom()], Stroke::new(1.0, LINE));
        }
        let small = |ui: &mut egui::Ui, at: f32, icon: Icon, on: bool, tip: &str, salt: &str| {
            let b = Rect::from_center_size(pos2(r.right() - at, r.center().y), vec2(24.0, 24.0));
            let resp = ui.interact(b, ui.scope_id().with(salt), Sense::click()).on_hover_text(tip);
            if resp.hovered() || on {
                ui.painter().rect_filled(b, 5.0, theme::RAISE);
            }
            theme::paint_icon(
                ui.painter(),
                b.shrink(4.0),
                icon,
                if on {
                    ACCENT
                } else if resp.hovered() {
                    FG
                } else {
                    MUTED
                },
            );
            resp.clicked()
        };
        if small(ui, 46.0, Icon::Help, self.show_syntax, "Справка по синтаксису (F1)", "syntax") {
            self.show_syntax = !self.show_syntax;
        }
        if small(ui, 18.0, Icon::CollapseLeft, false, "Свернуть скрипт (Ctrl+E)", "collapse") {
            self.show_script = false;
        }
        let errors_h = if self.errors.is_empty() { 0.0 } else { 10.0 + 18.0 * self.errors.len().min(4) as f32 };
        let error_lines: Vec<usize> = self.errors.iter().map(|e| e.line).collect();
        let selected_span = self.selected.as_ref().and_then(|id| self.scene.sites.get(id)).filter(|s| !s.generated).map(|s| (s.span.start, s.span.end));
        let mut cursor_at = None;
        egui::Frame::new().fill(BG).show(ui, |ui| {
            egui::ScrollArea::both().id_salt("script").max_height(ui.available_height() - errors_h).auto_shrink([false, false]).show(ui, |ui| {
                let before = self.src.clone();
                let mut layouter = |ui: &egui::Ui, text: &dyn egui::TextBuffer, _wrap: f32| {
                    ui.painter().layout_job(crate::highlight::highlight(text.as_str(), &error_lines, selected_span))
                };
                let out = egui::TextEdit::multiline(&mut self.src)
                    .code_editor()
                    .frame(egui::Frame::NONE)
                    .margin(egui::Margin::symmetric(12, 10))
                    .desired_width(f32::INFINITY)
                    .desired_rows(30)
                    .lock_focus(true)
                    .layouter(&mut layouter)
                    .show(ui);
                if out.response.changed() {
                    let after = std::mem::replace(&mut self.src, before);
                    self.checkpoint("text");
                    self.src = after;
                    self.reparse();
                } else if out.response.clicked()
                    && let Some(c) = out.cursor_range
                {
                    cursor_at = Some(c.primary.index.0);
                }
            });
        });
        if let Some(char_index) = cursor_at {
            let byte = self.src.char_indices().nth(char_index).map_or(self.src.len(), |(b, _)| b);
            // The innermost declaration around the cursor.
            let hit = self
                .scene
                .sites
                .iter()
                .filter(|(_, s)| !s.generated && s.span.start <= byte && byte <= s.span.end)
                .min_by_key(|(_, s)| s.span.end - s.span.start);
            if let Some((id, _)) = hit {
                self.selected = Some(id.clone());
            }
        }
        if !self.errors.is_empty() {
            ui.add_space(4.0);
            for e in self.errors.iter().take(4) {
                ui.horizontal(|ui| {
                    ui.add_space(10.0);
                    ui.label(RichText::new(e.to_string()).font(theme::mono(11.5)).color(DANGER));
                });
            }
        }
    }

    // ---------- properties ----------

    fn props_panel(&mut self, ui: &mut egui::Ui) {
        egui::ScrollArea::vertical().id_salt("props").auto_shrink([false, false]).show(ui, |ui| {
            let Some(id) = self.selected.clone() else {
                self.artboard_info(ui);
                return;
            };
            if !self.also.is_empty() {
                self.multi_panel(ui);
                return;
            }
            let Some(node) = self.display_doc().node(&id).cloned() else { return };
            let generated = self.scene.sites.get(&id).is_some_and(|s| s.generated);
            section(ui, |ui| {
                ui.horizontal(|ui| {
                    let (r, _) = ui.allocate_exact_size(vec2(16.0, 18.0), Sense::hover());
                    theme::paint_icon(ui.painter(), r, theme::shape_icon(node.kind), MUTED);
                    ui.label(RichText::new(node.display_name()).font(theme::semibold(14.0)).color(FG));
                });
                ui.label(RichText::new(format!("{} · id {}", kind_name(node.kind), node.id)).size(11.5).color(FAINT));
            });

            let mut changes: Vec<(Prop, Value)> = Vec::new();
            let mut diamonds: Vec<Prop> = Vec::new();
            let mut styles: Vec<crate::style_ui::StyleEdit> = Vec::new();
            let half = (ui.available_width() - 24.0 - 6.0) / 2.0;
            let mut field =
                |app: &App, ui: &mut egui::Ui, label: &str, prop: Prop, speed: f64, range: std::ops::RangeInclusive<f64>, scale: f64, decimals: usize| {
                    let v = node.get(prop).as_num().unwrap_or(0.0) * scale;
                    let r = (*range.start() * scale)..=(*range.end() * scale);
                    let out = theme::num_field(ui, half, label, v, speed * scale, r, decimals, app.key_state(&id, prop));
                    if let Some(nv) = out.changed {
                        changes.push((prop, Value::Num(nv / scale)));
                    }
                    if out.diamond {
                        diamonds.push(prop);
                    }
                };
            fold(ui, "Трансформация", |ui| {
                ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
                ui.horizontal(|ui| {
                    field(self, ui, "X", Prop::X, 1.0, f64::MIN..=f64::MAX, 1.0, 1);
                    field(self, ui, "Y", Prop::Y, 1.0, f64::MIN..=f64::MAX, 1.0, 1);
                });
                if node.kind == ShapeKind::Bone {
                    ui.horizontal(|ui| field(self, ui, "Длина", Prop::Width, 1.0, 0.0..=100_000.0, 1.0, 1));
                } else if !matches!(node.kind, ShapeKind::Group | ShapeKind::Path | ShapeKind::Text) {
                    ui.horizontal(|ui| {
                        field(self, ui, "W", Prop::Width, 1.0, 0.0..=100_000.0, 1.0, 1);
                        field(self, ui, "H", Prop::Height, 1.0, 0.0..=100_000.0, 1.0, 1);
                    });
                }
                ui.horizontal(|ui| {
                    field(self, ui, "Угол °", Prop::Rotation, 1.0, -3600.0..=3600.0, 1.0, 1);
                    field(self, ui, "Альфа %", Prop::Opacity, 0.01, 0.0..=1.0, 100.0, 0);
                });
                ui.horizontal(|ui| {
                    field(self, ui, "Масшт. X", Prop::ScaleX, 0.01, -100.0..=100.0, 1.0, 2);
                    field(self, ui, "Масшт. Y", Prop::ScaleY, 0.01, -100.0..=100.0, 1.0, 2);
                });
                ui.horizontal(|ui| {
                    field(self, ui, "Накл. X °", Prop::SkewX, 0.5, -89.0..=89.0, 1.0, 1);
                    field(self, ui, "Накл. Y °", Prop::SkewY, 0.5, -89.0..=89.0, 1.0, 1);
                });
                crate::style_ui::pivot_row(ui, &node, &mut styles);
            });
            let mut pick_font = false;
            let mut outline_text = false;
            if node.kind == ShapeKind::Text {
                let font = self.scene.prop_spans.get(&(id.clone(), "font".to_string())).map(|s| {
                    let t = self.src[s.start..s.end].trim_start_matches("font").trim();
                    t.trim_matches('"').to_string()
                });
                fold(ui, "Текст", |ui| {
                    ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
                    crate::text_ui::content_row(ui, &node, &mut styles);
                    ui.horizontal(|ui| {
                        field(self, ui, "Размер", Prop::FontSize, 0.5, 1.0..=2000.0, 1.0, 1);
                        field(self, ui, "Интервал", Prop::LetterSpacing, 0.1, -500.0..=500.0, 1.0, 1);
                    });
                    pick_font = crate::text_ui::layout_rows(ui, &node, font.as_deref(), &mut styles);
                    if theme::button(ui, "В кривые", ButtonKind::Normal)
                        .on_hover_text("Заменить текст контуром: буквы можно править по точкам, шрифт больше не нужен")
                        .clicked()
                    {
                        outline_text = true;
                    }
                });
                fold(ui, "Анимация букв", |ui| {
                    ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
                    ui.horizontal(|ui| {
                        field(self, ui, "Появл. %", Prop::Reveal, 0.01, 0.0..=1.0, 100.0, 0);
                        field(self, ui, "Волна", Prop::Wave, 0.2, -500.0..=500.0, 1.0, 1);
                    });
                    ui.horizontal(|ui| {
                        field(self, ui, "Фаза", Prop::Phase, 0.01, -1000.0..=1000.0, 1.0, 2);
                    });
                    ui.label(
                        RichText::new("Ставьте ключи в Animate: «Появление» 0 → 100% — буквы выпрыгивают по одной; «Фаза» 0 → 1 — волна пробегает по тексту.")
                            .size(11.5)
                            .color(FAINT),
                    );
                });
            }
            if matches!(node.kind, ShapeKind::Rect | ShapeKind::Star | ShapeKind::Polygon) {
                fold(ui, "Форма", |ui| {
                    let corners = node.kind == ShapeKind::Rect && node.radii.is_some();
                    if !corners {
                        ui.horizontal(|ui| match node.kind {
                            ShapeKind::Rect => {
                                if !corners {
                                    field(self, ui, "Скругл.", Prop::Radius, 0.5, 0.0..=10_000.0, 1.0, 1);
                                }
                            }
                            ShapeKind::Star => {
                                field(self, ui, "Лучи", Prop::Points, 0.1, 3.0..=64.0, 1.0, 0);
                                field(self, ui, "Внутр.", Prop::Inner, 0.01, 0.05..=1.0, 1.0, 2);
                            }
                            _ => field(self, ui, "Стороны", Prop::Points, 0.1, 3.0..=64.0, 1.0, 0),
                        });
                    }
                    if node.kind == ShapeKind::Rect {
                        crate::style_ui::corners_row(ui, &node, &mut styles);
                    }
                });
            }
            if node.kind == ShapeKind::Path {
                let info = crate::pathedit::Outline::from_bez(&node.geometry)
                    .map(|o| format!("{} точ. · {}", o.verts.len(), if o.closed { "замкнут" } else { "открыт" }));
                fold(ui, "Контур", |ui| {
                    ui.label(RichText::new(info.clone().unwrap_or_else(|| "несколько частей: правка в скрипте".into())).size(12.0).color(MUTED));
                    let editing = self.point_edit.as_ref().is_some_and(|e| e.node == id);
                    let kind = if editing { ButtonKind::On } else { ButtonKind::Normal };
                    if info.is_some() && theme::button(ui, if editing { "Готово (Enter)" } else { "Редактировать точки (Enter)" }, kind).clicked()
                    {
                        if editing {
                            self.point_edit = None;
                        } else {
                            self.begin_point_edit();
                        }
                    }
                });
            }
            self.mask_ui(ui, &node);
            self.symbol_section(ui, &node);
            // Interface work needs no skeletons or physics; game art needs no kit.
            let game = self.scene.doc.purpose != Purpose::Ui;
            let interface = self.scene.doc.purpose != Purpose::Game;
            if game {
                self.bone_section(ui, &node);
                self.weights_section(ui, &node);
                self.physics_section(ui, &node);
            }
            if !node.kind.is_container() {
                fold(ui, "Заливка", |ui| {
                    if self.fill_kind_ui(ui, &node) {
                        ui.add_space(6.0);
                        self.paint_row(ui, &id, Prop::Fill, node.fill, &mut changes, &mut diamonds);
                    }
                });
                fold(ui, "Обводка", |ui| {
                    self.paint_row(ui, &id, Prop::StrokeColor, node.stroke, &mut changes, &mut diamonds);
                    if node.stroke.is_some() {
                        let w = ui.available_width();
                        let out = theme::num_field(ui, w, "Толщина", node.stroke_width, 0.1, 0.0..=1000.0, 1, self.key_state(&id, Prop::StrokeWidth));
                        if let Some(v) = out.changed {
                            changes.push((Prop::StrokeWidth, Value::Num(v)));
                        }
                        if out.diamond {
                            diamonds.push(Prop::StrokeWidth);
                        }
                        crate::style_ui::line_rows(ui, &node, &mut styles);
                    }
                    ui.add_space(4.0);
                    ui.label(RichText::new("Слои обводки под фигурой").size(12.0).color(MUTED));
                    crate::text_ui::outline_rows(ui, &node, &mut styles);
                });
            }
            // Blur, height and shadow: keyable, so they animate like position or colour.
            let mut fx_changes = Vec::new();
            let mut fx_diamonds = Vec::new();
            if node.kind != ShapeKind::Bone {
                fold(ui, "Эффекты", |ui| {
                    ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
                    let mut fx = |ui: &mut egui::Ui, label: &str, prop: Prop| {
                        let v = node.get(prop).as_num().unwrap_or(0.0);
                        let range = if matches!(prop, Prop::ShadowX | Prop::ShadowY) { -1000.0..=1000.0 } else { 0.0..=500.0 };
                        let out = theme::num_field(ui, half, label, v, 0.2, range, 1, self.key_state(&id, prop));
                        if let Some(nv) = out.changed {
                            fx_changes.push((prop, Value::Num((nv * 10.0).round() / 10.0)));
                        }
                        if out.diamond {
                            fx_diamonds.push(prop);
                        }
                    };
                    ui.horizontal(|ui| {
                        fx(ui, "Размытие", Prop::Blur);
                        if node.kind != ShapeKind::Group {
                            fx(ui, "Высота", Prop::Elevation);
                        }
                    });
                    if node.kind == ShapeKind::Group {
                        ui.label(RichText::new("Группа размывается целиком, вместе со всеми детьми.").size(11.5).color(FAINT));
                        return;
                    }
                    if crate::style_ui::shadow_rows(ui, &node, &mut styles) {
                        ui.horizontal(|ui| {
                            fx(ui, "Тень X", Prop::ShadowX);
                            fx(ui, "Тень Y", Prop::ShadowY);
                        });
                        ui.horizontal(|ui| fx(ui, "Мягкость", Prop::ShadowBlur));
                    }
                    ui.label(
                        RichText::new(
                            "«Высота» поднимает объект над артбордом: тень уходит от света сцены, длиннее и мягче. Размытие 0 — жёсткая тень для пиксель-арта.",
                        )
                        .size(11.5)
                        .color(FAINT),
                    );
                    crate::style_ui::blend_row(ui, &node, &mut styles);
                });
            }
            changes.extend(fx_changes);
            diamonds.extend(fx_diamonds);
            if game {
                self.orbit_section(ui, &node, half, &mut changes, &mut diamonds, &mut styles);
                self.body_section(ui, &node, half, &mut changes, &mut diamonds, &mut styles);
            }
            self.morph_section(ui, &node, &mut changes, &mut diamonds, &mut styles);
            if game {
                self.brush_section(ui, &node, half, &mut changes, &mut diamonds, &mut styles);
            }
            let mut add_states = false;
            if interface {
                self.layout_section(ui, &node, half, &mut styles);
                add_states = self.kit_section(ui, &node, half, &mut styles);
            }
            section(ui, |ui| {
                let mut job = egui::text::LayoutJob::default();
                job.wrap.max_width = ui.available_width();
                let fmt = |c: Color32| egui::TextFormat { color: c, font_id: theme::ui_font(12.0), line_height: Some(18.0), ..Default::default() };
                match self.key_time() {
                    Some((anim, t)) => {
                        job.append(&format!("Animate · {anim}"), 0.0, fmt(ACCENT));
                        job.append(": изменения ставят ключи в ", 0.0, fmt(MUTED));
                        job.append(&format!("{} с", fmt_num(t)), 0.0, fmt(ACCENT));
                        job.append(". Ромб рядом со свойством ставит или снимает ключ.", 0.0, fmt(MUTED));
                    }
                    None if generated && !self.scene.instances.contains_key(&id) && self.symbol_of(&id).is_some() => {
                        job.append("Часть символа", 0.0, fmt(FG));
                        job.append(": правки попадут в блок override { } и изменят только эту копию. Все копии меняются в блоке symbol.", 0.0, fmt(MUTED));
                    }
                    None if generated => {
                        job.append("Создан циклом", 0.0, fmt(FG));
                        job.append(": правки попадут в блок override { } в конце скрипта, сам цикл не меняется.", 0.0, fmt(MUTED));
                    }
                    None => {
                        job.append("Design", 0.0, fmt(FG));
                        job.append(": правки меняют числа прямо в скрипте. Для ключей включите Animate.", 0.0, fmt(MUTED));
                    }
                }
                ui.label(job);
            });
            for prop in diamonds {
                self.toggle_key(&id, prop);
            }
            for (prop, value) in changes {
                self.edit(&id, prop, value);
            }
            self.apply_styles(&id, styles);
            if pick_font {
                self.pick_font(&id);
            }
            if outline_text {
                self.select_only(Some(id.clone()));
                self.boolean(motif_script::BoolOp::Union);
            }
            if add_states {
                self.add_ui_states();
            }
        });
    }

    fn artboard_info(&mut self, ui: &mut egui::Ui) {
        let ab = &self.scene.doc.artboard;
        section(ui, |ui| {
            ui.horizontal(|ui| {
                let (r, _) = ui.allocate_exact_size(vec2(16.0, 18.0), Sense::hover());
                theme::paint_icon(ui.painter(), r, Icon::Artboard, MUTED);
                ui.label(RichText::new(&ab.name).font(theme::semibold(14.0)).color(FG));
            });
            ui.label(RichText::new(format!("Артборд · {}×{} · фон {}", fmt_num(ab.width), fmt_num(ab.height), ab.background.to_hex())).size(11.5).color(FAINT));
        });
        section(ui, |ui| {
            ui.label(RichText::new("Выберите объект на холсте, в слоях или кликом по его строке в скрипте.").size(12.0).color(FAINT));
            ui.label(RichText::new("Новые фигуры: R, O, S, N — тяните по холсту; P — перо.").size(12.0).color(FAINT));
            ui.label(RichText::new("Перетащите на окно SVG — он станет контурами, PNG — подложкой.").size(12.0).color(FAINT));
        });
        if self.scene.doc.purpose == Purpose::Ui {
            self.symbols_library(ui);
            self.backdrop_section(ui);
            return;
        }
        let mut light = None;
        fold(ui, "Свет сцены", |ui| {
            let w = ui.available_width();
            if let Some(v) = theme::num_field(ui, w, "Тени падают в сторону °", self.scene.doc.light, 1.0, -360.0..=360.0, 0, KeyState::Hidden).changed
            {
                light = Some(v.round());
            }
            ui.label(RichText::new("90 — вниз, 0 — вправо. Действует на объекты с «Высотой».").size(11.5).color(FAINT));
        });
        if let Some(a) = light
            && self.can_edit()
        {
            let out = motif_script::set_light(&self.src, a);
            self.apply("light", out);
        }
        self.world_section(ui);
        self.symbols_library(ui);
        self.backdrop_section(ui);
    }

    fn paint_row(&mut self, ui: &mut egui::Ui, id: &str, prop: Prop, paint: Option<Color>, changes: &mut Vec<(Prop, Value)>, diamonds: &mut Vec<Prop>) {
        ui.horizontal(|ui| {
            // Swatch with a colour picker.
            let (sw, resp) = ui.allocate_exact_size(vec2(28.0, 28.0), Sense::click());
            match paint {
                Some(c) => {
                    ui.painter().rect_filled(sw, 6.0, Color32::from_rgb(c.r, c.g, c.b));
                }
                None => {
                    ui.painter().rect_filled(sw, 6.0, BG);
                    ui.painter().line_segment([sw.left_bottom() + vec2(5.0, -5.0), sw.right_top() + vec2(-5.0, 5.0)], Stroke::new(1.5, DANGER));
                }
            }
            ui.painter().rect_stroke(sw, 6.0, Stroke::new(1.0, LINE), StrokeKind::Inside);
            let mut c32 = paint.map_or(Color32::BLACK, |c| Color32::from_rgb(c.r, c.g, c.b));
            theme::color_popup(&resp).show(|ui| {
                if egui::color_picker::color_picker_color32(ui, &mut c32, egui::color_picker::Alpha::Opaque) {
                    changes.push((prop, Value::Color(Color { r: c32.r(), g: c32.g(), b: c32.b(), a: paint.map_or(255, |p| p.a) })));
                }
                if paint.is_some() && ui.button("Убрать (none)").clicked() {
                    changes.push((prop, Value::None));
                }
            });
            // Hex field: edit and press Enter.
            let key = self.key_state(id, prop);
            let w = ui.available_width();
            let (rect, _) = ui.allocate_exact_size(vec2(w, 28.0), Sense::hover());
            let animated = matches!(key, KeyState::Track | KeyState::Key);
            ui.painter().rect(rect, 6.0, BG, Stroke::new(1.0, if animated { theme::mix(ACCENT, LINE, 0.45) } else { LINE }), StrokeKind::Inside);
            let diamond_w = if key == KeyState::Hidden { 0.0 } else { 22.0 };
            let text_rect = Rect::from_min_max(rect.min + vec2(8.0, 4.0), rect.max - vec2(diamond_w + 4.0, 4.0));
            let shown = paint.map_or("none".to_string(), |c| c.to_hex());
            let editing = self.hex_edit.as_ref().is_some_and(|(p, _)| *p == prop);
            let mut text = if editing { self.hex_edit.as_ref().unwrap().1.clone() } else { shown.clone() };
            let resp =
                ui.place(text_rect, egui::TextEdit::singleline(&mut text).frame(egui::Frame::NONE).font(theme::mono(12.0)).desired_width(text_rect.width()));
            if resp.has_focus() {
                self.hex_edit = Some((prop, text.clone()));
            }
            if resp.lost_focus() {
                if let Some(v) = parse_paint(&text)
                    && text.trim() != shown
                {
                    changes.push((prop, v));
                }
                self.hex_edit = None;
            }
            if key != KeyState::Hidden {
                let d = Rect::from_min_max(pos2(rect.right() - 22.0, rect.top()), rect.max);
                let dresp = ui.interact(d, ui.scope_id().with(("kf-paint", prop.path_name())), Sense::click());
                let (fill, stroke) = match key {
                    KeyState::Key => (ACCENT, ACCENT),
                    KeyState::Track => (Color32::TRANSPARENT, ACCENT),
                    _ => (Color32::TRANSPARENT, if dresp.hovered() { MUTED } else { FAINT }),
                };
                theme::diamond(ui.painter(), d.center(), 4.5, fill, Stroke::new(1.5, stroke));
                if dresp.clicked() {
                    diamonds.push(prop);
                }
            }
        });
    }

    // ---------- status ----------

    fn status_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_centered(|ui| {
            ui.spacing_mut().item_spacing.x = 16.0;
            let small = |t: String, c: Color32| RichText::new(t).font(theme::mono(11.0)).color(c);
            ui.label(small(format!("● {}", self.canvas.backend()), GOOD));
            ui.label(small(format!("{:.0} fps", 1000.0 / self.frame_ms.max(1.0)), FAINT));
            let (x, y) = self.pointer_doc.map_or(("—".into(), "—".into()), |p| (format!("{:.0}", p.x), format!("{:.0}", p.y)));
            ui.label(small(format!("x {x}  y {y}"), FAINT));
            let mode = match &self.anim {
                Some(a) => format!("Animate · {a}"),
                None => "Design".into(),
            };
            ui.label(small(mode, FAINT));
            ui.label(small(format!("история: {}", self.undo.len()), FAINT));
            ui.label(small(self.status.clone(), MUTED));
        });
    }

    // ---------- canvas ----------

    fn canvas_ui(&mut self, ui: &mut egui::Ui, frame: &eframe::Frame) {
        if self.tool != Tool::Pen && !self.pen.is_empty() {
            self.finish_pen(false);
        }
        if let Some(e) = &self.point_edit
            && (self.selected.as_deref() != Some(e.node.as_str()) || self.tool != Tool::Select)
        {
            self.point_edit = None;
        }
        let rect = ui.available_rect_before_wrap();
        let resp = ui.allocate_rect(rect, Sense::click_and_drag());
        let view = *self.view.get_or_insert_with(|| View::fit(rect, &self.scene.doc));
        let doc = self.display_doc();
        let ghosts = self.onion_doc();
        let backdrop = crate::importer::backdrop_view(&self.scene, &self.backdrop_img);
        self.canvas.paint(ui, frame, rect, view, ghosts.as_ref().unwrap_or(&doc), backdrop);
        if self.pixel.on {
            self.paint_pixel_preview(ui, rect, view, &doc);
        }
        let board = artboard_rect(rect, view, &doc);
        let painter = ui.painter_at(rect);
        paint_dots(&painter, rect, board, view);
        if self.snap.grid {
            crate::guides::paint_grid(&painter, rect, view, &doc, self.snap.grid_size);
        }

        // Zoom around the pointer.
        if resp.hovered() {
            let (scroll, pointer) = ui.input(|i| (i.smooth_scroll_delta.y + (i.zoom_delta() - 1.0) * 200.0, i.pointer.hover_pos()));
            if scroll != 0.0
                && let (Some(p), Some(v)) = (pointer, self.view.as_mut())
            {
                let nz = (v.zoom * (scroll * 0.0025).exp()).clamp(0.05, 32.0);
                let anchor = p - rect.min;
                v.pan = anchor - (anchor - v.pan) * (nz / v.zoom);
                v.zoom = nz;
            }
        }

        let pointer = resp.interact_pointer_pos().or(resp.hover_pos());
        let doc_pt = pointer.map(|p| view.to_doc(rect, p));
        self.pointer_doc = doc_pt.filter(|_| resp.hovered() || resp.dragged());
        self.hovered = if self.drag.is_none() {
            pointer
                .and_then(|p| crate::bone_ui::pick_bone(&doc, rect, view, p))
                .or_else(|| doc_pt.and_then(|p| doc.hit_test(p)))
                .map(|i| doc.nodes[i].id.clone())
        } else {
            None
        };
        let handles = self.selected.as_ref().filter(|_| self.also.is_empty()).and_then(|s| doc.index_of(s)).and_then(|i| handle_points(rect, view, &doc, i));
        let hit_handle = |p: Pos2| -> Option<Handle> {
            let h = handles.as_ref()?;
            if h.rotate.distance(p) < 8.0 {
                return Some(Handle::Rotate);
            }
            if h.resizable {
                for c in h.corners {
                    if c.distance(p) < 7.0 {
                        return Some(Handle::Corner);
                    }
                }
            }
            None
        };

        let (space, middle, alt, shift) = ui.input(|i| (i.key_down(Key::Space), i.pointer.middle_down(), i.modifiers.alt, i.modifiers.shift));
        let pan_gesture = space || middle || (alt && self.point_edit.is_none()) || self.tool == Tool::Hand;
        // A running machine that listens to the pointer takes it: the canvas is a live preview.
        let live = !pan_gesture && self.drag.is_none() && self.running().is_some_and(|(m, _)| !m.listeners.is_empty());
        if live {
            self.hovered = None;
            self.live_pointer(ui, &resp, &doc, doc_pt);
        }
        // The pen and point editing take the pointer themselves.
        let special = live
            || (!pan_gesture
                && self.drag.is_none()
                && if self.tool == Tool::Pen {
                    self.pen_input(ui, &resp, rect, view);
                    true
                } else if self.brush_w.on.is_some() && self.weight_paint_input(ui, &resp, rect, view, &doc) {
                    true
                } else if self.point_edit.is_some() {
                    self.point_edit_input(ui, &resp, rect, view, &doc)
                } else {
                    false
                });
        if !special
            && let Some(p) = pointer
            && resp.hovered()
            && self.drag.is_none()
        {
            let cursor = if pan_gesture {
                egui::CursorIcon::Grab
            } else if matches!(self.tool, Tool::Create(_)) {
                egui::CursorIcon::Crosshair
            } else {
                match hit_handle(p) {
                    Some(Handle::Rotate) => egui::CursorIcon::None,
                    Some(Handle::Corner) => egui::CursorIcon::ResizeNwSe,
                    None if self.hovered.is_some() => egui::CursorIcon::Move,
                    None => egui::CursorIcon::Default,
                }
            };
            ui.ctx().set_cursor_icon(cursor);
        }

        // Drags start where the button went down, not where the movement threshold was passed.
        let press = ui.input(|i| i.pointer.press_origin()).or(pointer);
        if !special
            && resp.drag_started()
            && let Some(p) = press
            && let dp = view.to_doc(rect, p)
        {
            self.drag = if pan_gesture {
                if space {
                    self.space_used_for_pan = true;
                }
                Some(DragOp::Pan)
            } else if let Tool::Create(kind) = self.tool {
                Some(DragOp::Create { kind, from: dp, to: dp })
            } else if let Some(h) = hit_handle(p)
                && let Some(id) = self.selected.clone()
                && let Some(i) = doc.index_of(&id)
            {
                let n = &doc.nodes[i];
                match h {
                    Handle::Corner => Some(DragOp::Resize { node: id, to_local: doc.world_transform(i).inverse(), start: (n.width, n.height) }),
                    Handle::Rotate => {
                        let center = doc.world_transform(i) * (Point::ZERO + n.pivot);
                        let a = (dp - center).atan2().to_degrees();
                        Some(DragOp::Rotate { node: id, center, start_angle: a, start_rot: n.rotation })
                    }
                }
            } else if self.tool == Tool::Select
                && !shift
                && let Some(op) = self.bone_drag(&doc, rect, view, p, dp)
            {
                Some(op)
            } else {
                match doc.hit_test(dp) {
                    Some(i) => {
                        let id = doc.nodes[i].id.clone();
                        if shift && !self.is_selected(&id) {
                            self.toggle_selected(id.clone());
                        } else if !self.is_selected(&id) {
                            self.select_only(Some(id.clone()));
                        }
                        self.sel_keys.clear();
                        let items = self
                            .selection()
                            .into_iter()
                            .filter_map(|id| doc.index_of(&id).map(|k| (id, k)))
                            // A stack places its children itself.
                            .filter(|(_, k)| doc.nodes[*k].parent.is_none_or(|p| doc.nodes[p].stack.is_none()))
                            .map(|(node, k)| MoveItem {
                                node,
                                origin: doc.world_transform(k) * Point::ZERO,
                                to_parent: doc.nodes[k].parent.map_or(Affine::IDENTITY, |pi| doc.world_transform(pi).inverse()),
                            })
                            .collect::<Vec<_>>();
                        if items.is_empty() {
                            self.status = "Объект в стеке: место задаёт стек группы. Порядок меняется перетаскиванием в слоях".into();
                        }
                        let moving: Vec<usize> = items.iter().filter_map(|it| doc.index_of(&it.node)).collect();
                        let start_box = moving.iter().filter_map(|&k| crate::selection::world_bounds(&doc, k)).reduce(|a, b| a.union(b));
                        let targets = crate::guides::Targets::collect(&doc, &moving);
                        Some(DragOp::Move { from: dp, items, start_box, targets })
                    }
                    None => Some(DragOp::Marquee { from: dp, to: dp, add: shift }),
                }
            };
        }
        // Rotation has no system cursor: draw a circular arrow at the pointer instead.
        let rotating = matches!(self.drag, Some(DragOp::Rotate { .. }));
        if let Some(p) = pointer
            && (rotating || (self.drag.is_none() && !pan_gesture && self.tool == Tool::Select && hit_handle(p) == Some(Handle::Rotate)))
        {
            ui.ctx().set_cursor_icon(egui::CursorIcon::None);
            let fg = ui.ctx().layer_painter(egui::LayerId::new(egui::Order::Tooltip, egui::Id::unique("rotate-cursor")));
            let r = Rect::from_center_size(p, vec2(22.0, 22.0));
            fg.circle_filled(p, 11.0, Color32::from_black_alpha(140));
            theme::paint_icon(&fg, r, Icon::Rotate, Color32::WHITE);
        }
        let snap = self.snap;
        if resp.dragged()
            && let Some(dp) = doc_pt
            && let Some(DragOp::BoneTip { node, joint, parent_angle }) = &self.drag
        {
            let (node, joint, parent_angle) = (node.clone(), *joint, *parent_angle);
            self.drag_bone_tip(&doc, &node, joint, parent_angle, dp, shift);
        }
        if resp.dragged()
            && let Some(dp) = doc_pt
        {
            match &mut self.drag {
                Some(DragOp::Pan) => {
                    if let Some(v) = self.view.as_mut() {
                        v.pan += resp.drag_delta();
                    }
                }
                Some(DragOp::Create { to, .. } | DragOp::Marquee { to, .. }) => *to = dp,
                Some(DragOp::Move { from, items, start_box, targets }) => {
                    let mut delta = dp - *from;
                    let mut stuck = [false; 2];
                    self.guides.clear();
                    // Alt moves freely past the guides.
                    if snap.guides
                        && !ui.input(|i| i.modifiers.alt)
                        && let Some(b) = start_box
                    {
                        let (d, s, g) = targets.snap(*b, delta, view.zoom);
                        (delta, stuck, self.guides) = (d, s, g);
                    }
                    let fine = |v: f64| (v * 10.0).round() / 10.0;
                    let moves: Vec<(String, f64, f64)> = items
                        .iter()
                        .map(|it| {
                            let target = it.to_parent * (it.origin + delta);
                            // Stacks and anchors move objects by an offset the script does not hold.
                            let off = doc.node(&it.node).map_or(motif_core::kurbo::Vec2::ZERO, |n| n.offset);
                            let x = if stuck[0] { fine(target.x) } else { snap.pos(target.x) } - off.x;
                            let y = if stuck[1] { fine(target.y) } else { snap.pos(target.y) } - off.y;
                            (it.node.clone(), fine(x), fine(y))
                        })
                        .collect();
                    for (node, x, y) in moves {
                        let cur = doc.node(&node).map(|n| (n.x, n.y));
                        if cur.is_some_and(|c| c.0 != x) {
                            self.preview(&node, Prop::X, Value::Num(x));
                        }
                        if cur.is_some_and(|c| c.1 != y) {
                            self.preview(&node, Prop::Y, Value::Num(y));
                        }
                    }
                }
                Some(DragOp::Resize { node, to_local, start }) => {
                    let local = *to_local * dp;
                    let (mut w, mut h) = (snap.size(local.x.abs() * 2.0), snap.size(local.y.abs() * 2.0));
                    if shift && start.0 > 0.0 && start.1 > 0.0 {
                        let k = (w / start.0).max(h / start.1);
                        (w, h) = (snap.size(start.0 * k), snap.size(start.1 * k));
                    }
                    let node = node.clone();
                    let cur = doc.node(&node).map(|n| (n.width, n.height));
                    if cur.is_some_and(|c| c.0 != w) {
                        self.preview(&node, Prop::Width, Value::Num(w.max(1.0)));
                    }
                    if cur.is_some_and(|c| c.1 != h) {
                        self.preview(&node, Prop::Height, Value::Num(h.max(1.0)));
                    }
                }
                Some(DragOp::Rotate { node, center, start_angle, start_rot }) => {
                    let a = (dp - *center).atan2().to_degrees();
                    let mut rot = *start_rot + (a - *start_angle);
                    rot = if shift { (rot / 15.0).round() * 15.0 } else { snap.rot(rot) };
                    let node = node.clone();
                    if doc.node(&node).is_some_and(|n| n.rotation != rot) {
                        self.preview(&node, Prop::Rotation, Value::Num(rot));
                    }
                }
                Some(DragOp::BoneTip { .. }) | None => {}
            }
        }
        if resp.drag_stopped() {
            self.guides.clear();
            self.commit_live();
            match self.drag.take() {
                Some(DragOp::Create { kind: ShapeKind::Bone, from, to }) => self.create_bone(from, to, 10.0 / view.zoom as f64),
                Some(DragOp::Create { kind, from, to }) => self.create_shape(kind, from, to, shift),
                Some(DragOp::Marquee { from, to, add }) => {
                    let area = motif_core::kurbo::Rect::from_points(from, to);
                    let hits: Vec<String> = (0..doc.nodes.len())
                        .filter(|&i| doc.nodes[i].parent.is_none() && doc.is_visible(i) && !doc.is_locked(i))
                        .filter(|&i| crate::selection::world_bounds(&doc, i).is_some_and(|b| b.intersect(area).area() > 0.0 || area.contains(b.center())))
                        .map(|i| doc.nodes[i].id.clone())
                        .collect();
                    if !add {
                        self.select_only(None);
                    }
                    for id in hits {
                        if !self.is_selected(&id) {
                            self.toggle_selected(id);
                        }
                    }
                }
                _ => {}
            }
            self.drag = None;
        }
        if !special && resp.double_clicked() && self.selected.as_ref().and_then(|s| doc.node(s)).is_some_and(|n| n.kind == ShapeKind::Path) {
            self.begin_point_edit();
        } else if !special && resp.clicked() && !pan_gesture {
            if self.bone_tool()
                && let Some(p) = doc_pt
            {
                self.create_bone(p, p, 10.0 / view.zoom as f64);
            } else if let Tool::Create(kind) = self.tool
                && let Some(p) = doc_pt
            {
                self.create_shape(kind, p, p, false);
            } else if !doc_pt.and_then(|p| handles.as_ref().and_then(|_| hit_handle(view.to_screen(rect, p)))).is_some() {
                let hit = pointer
                    .and_then(|p| crate::bone_ui::pick_bone(&doc, rect, view, p))
                    .or_else(|| doc_pt.and_then(|p| doc.hit_test(p)))
                    .map(|i| doc.nodes[i].id.clone());
                match hit {
                    Some(id) if shift => self.toggle_selected(id),
                    hit => self.select_only(hit),
                }
            }
        }

        // Overlays.
        painter.text(
            board.left_top() - vec2(0.0, 6.0),
            Align2::LEFT_BOTTOM,
            format!("{}  {}×{}", doc.artboard.name, fmt_num(doc.artboard.width), fmt_num(doc.artboard.height)),
            theme::ui_font(12.0),
            MUTED,
        );
        self.paint_bones(&painter, rect, view, &doc);
        self.paint_fields(&painter, rect, view, &doc);
        if let Some(h) = &self.hovered
            && self.selected.as_ref() != Some(h)
            && let Some(i) = doc.index_of(h)
        {
            outline(&painter, rect, view, &doc, i, Stroke::new(1.0, SEL.gamma_multiply(0.7)));
        }
        if let Some(s) = &self.selected
            && let Some(i) = doc.index_of(s)
        {
            if self.point_edit.is_none() {
                outline(&painter, rect, view, &doc, i, Stroke::new(1.25, SEL));
                slice_guides(&painter, rect, view, &doc, i);
            }
            if let Some(h) = &handles
                && self.point_edit.is_none()
                && self.also.is_empty()
            {
                let top_mid = h.corners[0].lerp(h.corners[1], 0.5);
                painter.line_segment([top_mid, h.rotate], Stroke::new(1.0, SEL));
                painter.circle(h.rotate, 4.0, Color32::WHITE, Stroke::new(1.0, SEL));
                if h.resizable {
                    for c in h.corners {
                        painter.rect(Rect::from_center_size(c, vec2(7.0, 7.0)), 1.0, Color32::WHITE, Stroke::new(1.0, SEL), StrokeKind::Middle);
                    }
                }
            }
        }
        for a in &self.also {
            if let Some(i) = doc.index_of(a) {
                outline(&painter, rect, view, &doc, i, Stroke::new(1.25, SEL));
            }
        }
        if let Some(DragOp::Marquee { from, to, .. }) = &self.drag {
            let r = Rect::from_two_pos(view.to_screen(rect, *from), view.to_screen(rect, *to));
            painter.rect(r, 0.0, SEL.gamma_multiply(0.08), Stroke::new(1.0, SEL), StrokeKind::Middle);
        }
        if let Some(DragOp::Create { from, to, kind }) = &self.drag
            && *kind != ShapeKind::Bone
        {
            let r = Rect::from_two_pos(view.to_screen(rect, *from), view.to_screen(rect, *to));
            painter.rect_stroke(r, 0.0, Stroke::new(1.0, SEL), StrokeKind::Middle);
        }
        crate::guides::paint_guides(&painter, rect, view, &self.guides);
        let ruler = if self.snap.rulers {
            let span =
                self.selection().iter().filter_map(|id| doc.index_of(id)).filter_map(|k| crate::selection::world_bounds(&doc, k)).reduce(|a, b| a.union(b));
            crate::guides::paint_rulers(&painter, rect, view, self.pointer_doc, span);
            crate::guides::RULER
        } else {
            0.0
        };
        if self.anim.is_some() {
            painter.rect_stroke(rect, 0.0, Stroke::new(2.0, ACCENT), StrokeKind::Inside);
            let badge = painter.layout_no_wrap("ANIMATE · АВТОКЛЮЧИ".into(), theme::semibold(11.0), ACCENT_INK);
            let b = Rect::from_min_size(rect.min + vec2(12.0 + ruler, 10.0 + ruler), galley_size(&badge) + vec2(16.0, 6.0));
            painter.rect_filled(b, 5.0, ACCENT);
            painter.galley(b.min + vec2(8.0, 3.0), badge, ACCENT_INK);
        }
        painter.text(
            rect.right_bottom() - vec2(12.0, 10.0),
            Align2::RIGHT_BOTTOM,
            "Колесо — масштаб · пробел + мышь — панорама · F — вписать",
            theme::ui_font(11.5),
            FAINT,
        );
    }

    fn create_shape(&mut self, kind: ShapeKind, from: Point, to: Point, square: bool) {
        if !self.can_edit() {
            return;
        }
        let (mut w, mut h) = ((to.x - from.x).abs().round(), (to.y - from.y).abs().round());
        if w < 4.0 && h < 4.0 {
            (w, h) = (80.0, 80.0);
        }
        if square || matches!(kind, ShapeKind::Star | ShapeKind::Polygon) && (w - h).abs() < 6.0 {
            let m = w.max(h);
            (w, h) = (m, m);
        }
        let (cx, cy) = if from == to { (from.x.round(), from.y.round()) } else { (((from.x + to.x) / 2.0).round(), ((from.y + to.y) / 2.0).round()) };
        let (out, id) = add_node(&self.src, &self.scene, kind, (cx, cy), (w.max(1.0), h.max(1.0)));
        if self.apply("create", Some(out)) {
            self.selected = Some(id.clone());
            self.status = format!("Добавлен {} «{id}» — строка в скрипте перед блоками anim", kind.keyword());
        }
        self.tool = Tool::Select;
    }

    fn paint_pixel_preview(&mut self, ui: &mut egui::Ui, rect: Rect, view: View, doc: &Document) {
        let scale = self.pixel.width as f64 / doc.artboard.width;
        let Some(mut px) = render_cpu(doc, CpuOptions { scale, antialias: false, background: true }) else { return };
        let scene_colors;
        let palette: &[Color] = match &self.pixel.palette {
            Some((_, p)) => p,
            None => {
                scene_colors = scene_palette(&self.scene.doc);
                &scene_colors
            }
        };
        quantize(&mut px, palette);
        let image = egui::ColorImage::from_rgba_premultiplied([px.width() as usize, px.height() as usize], px.data());
        match &mut self.pixel.texture {
            Some(t) => t.set(image, TextureOptions::NEAREST),
            None => self.pixel.texture = Some(ui.ctx().load_texture("pixel-preview", image, TextureOptions::NEAREST)),
        }
        let tex = self.pixel.texture.as_ref().unwrap();
        let board = artboard_rect(rect, view, doc);
        let painter = ui.painter_at(rect);
        painter.image(tex.id(), board, Rect::from_min_max(Pos2::ZERO, pos2(1.0, 1.0)), Color32::WHITE);
        let name = self.pixel.palette.as_ref().map_or_else(|| format!("палитра сцены, {} цв.", palette.len()), |(n, p)| format!("{n}, {} цв.", p.len()));
        painter.text(
            board.right_top() - vec2(0.0, 6.0),
            Align2::RIGHT_BOTTOM,
            format!("пиксели {}×{} · {name}", px.width(), px.height()),
            theme::ui_font(12.0),
            ACCENT,
        );
    }

    // ---------- windows ----------

    fn export_window(&mut self, ctx: &egui::Context) {
        let mut open = self.show_export;
        egui::Window::new("Экспорт")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_width(380.0)
            .anchor(Align2::RIGHT_TOP, vec2(-280.0, 56.0))
            .show(ctx, |ui| {
                // An interface scene is exported as a kit first of all; game art has no kit.
                let purpose = self.scene.doc.purpose;
                if purpose == Purpose::Ui {
                    self.kit_export(ui);
                } else {
                    self.godot_export(ui);
                }
                theme::caption(ui, "Кадры PNG для PixaTool");
                egui::Grid::new("export").num_columns(2).spacing([12.0, 8.0]).show(ui, |ui| {
                    ui.label("Кадров в секунду");
                    ui.add(egui::DragValue::new(&mut self.export.fps).range(1.0..=60.0));
                    ui.end_row();
                    ui.label("Масштаб");
                    ui.add(egui::DragValue::new(&mut self.export.scale).range(0.1..=8.0).speed(0.05));
                    ui.end_row();
                });
                ui.checkbox(&mut self.export.antialias, "Сглаживание краёв");
                ui.checkbox(&mut self.export.transparent, "Прозрачный фон");
                let what = self.anim.as_deref().map_or("кадр сцены".to_string(), |a| format!("анимация «{a}»"));
                if theme::button(ui, &format!("Сохранить кадры… ({what})"), ButtonKind::Primary).clicked() {
                    self.export_frames();
                }
                ui.add_space(6.0);
                theme::rule(ui);
                ui.add_space(4.0);
                theme::caption(ui, "Палитра");
                if theme::button(ui, &format!("Палитра сцены в .pal… (до {PIXATOOL_MAX_COLORS} цветов)"), ButtonKind::Normal).clicked() {
                    self.export_palette();
                }
                ui.horizontal(|ui| {
                    if theme::button(ui, "Загрузить .pal для превью…", ButtonKind::Normal).clicked() {
                        self.load_palette();
                    }
                    if self.pixel.palette.is_some() && theme::button(ui, "Палитра сцены", ButtonKind::Normal).clicked() {
                        self.pixel.palette = None;
                    }
                });
                ui.label(RichText::new("Кадры называются ИМЯ_0000.png: PixaTool берёт их пакетом.").size(12.0).color(FAINT));
                ui.add_space(6.0);
                theme::rule(ui);
                ui.add_space(4.0);
                if purpose == Purpose::Any {
                    self.kit_export(ui);
                }
                self.pixatool_section(ui);
            });
        self.show_export = open;
    }

    fn godot_export(&mut self, ui: &mut egui::Ui) {
        theme::caption(ui, "Анимации для Godot");
        let what =
            self.selected.as_deref().and_then(|id| self.scene.doc.node(id)).map_or("вся сцена".to_string(), |n| format!("«{}»", n.display_name()));
        let busy = self.pixa.job.is_some();
        let r = ui.add_enabled_ui(!busy, |ui| theme::button(ui, &format!("Выгрузить в Godot… ({what})"), ButtonKind::Primary)).inner;
        if r.on_hover_text("Все анимации в одном атласе, SpriteFrames и готовая сцена .tscn; машина состояний — AnimationTree").clicked()
        {
            let ctx = ui.ctx().clone();
            self.export_godot(&ctx);
        }
        let has_exe = self.pixa.exe.is_some();
        let mut through = self.pixa.godot && has_exe;
        let r = ui.add_enabled(has_exe && !busy, egui::Checkbox::new(&mut through, "Через PixaTool"));
        let r = r.on_hover_text("Каждый кадр перед упаковкой в атлас проходит через PixaTool с пресетом из раздела PixaTool ниже");
        let r = if has_exe { r } else { r.on_disabled_hover_text("Укажите Pixatool.exe в разделе PixaTool ниже") };
        if r.changed() {
            self.pixa.godot = through;
            self.pixa.save();
        }
        if let Some(job) = &self.pixa.job {
            job_progress(ui, job);
        }
        if let Some(link) = self.path.as_deref().and_then(|p| self.pixa.link(p)).cloned() {
            let what =
                link.node.as_deref().and_then(|id| self.scene.doc.node(id)).map_or("вся сцена".to_string(), |n| format!("«{}»", n.display_name()));
            let mut unlink = false;
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new(format!("Обновляется при каждом сохранении: {what} → {}", crate::pixabridge::short(&link.dir))).size(12.0))
                    .on_hover_text(link.dir.display().to_string());
                unlink = theme::button(ui, "Отвязать", ButtonKind::Normal).on_hover_text("Больше не выгружать при сохранении").clicked();
            });
            if unlink && let Some(path) = self.path.clone() {
                self.pixa.set_link(&path, None);
            }
        }
        let machine = self
            .scene
            .doc
            .machines
            .first()
            .map_or(String::new(), |m| format!(" Машина «{}» станет AnimationTree, её входы — переменными скрипта сцены (speed, fire_jump()…).", m.name));
        ui.label(
            RichText::new(format!(
                "Выделите персонажа, чтобы выгрузить его без фона: точкой привязки станет его центр. Кадры в секунду, масштаб и сглаживание — из настроек ниже. Через PixaTool спрайт выйдет в размере пикселей пресета, с фильтром Nearest. После выгрузки сохранённая сцена обновляется в Godot при каждом Ctrl+S.{machine}"
            ))
            .size(12.0)
            .color(FAINT),
        );
        ui.add_space(6.0);
        theme::rule(ui);
        ui.add_space(4.0);
    }

    fn kit_export(&mut self, ui: &mut egui::Ui) {
        theme::caption(ui, "UI-набор для Godot");
        if theme::button(ui, "Сохранить UI-набор…", if self.scene.doc.purpose == Purpose::Ui { ButtonKind::Primary } else { ButtonKind::Normal })
            .on_hover_text("Каждая деталь с ui или slice — отдельные PNG по состояниям и тема .tres со StyleBoxTexture")
            .clicked()
        {
            self.export_kit();
        }
        ui.label(
            RichText::new("Масштаб и сглаживание — те же, что для кадров. Тему подключают в Godot как theme, детали — через Theme Type Variation.")
                .size(12.0)
                .color(FAINT),
        );
        ui.add_space(6.0);
        theme::rule(ui);
        ui.add_space(4.0);
    }

    fn pixatool_section(&mut self, ui: &mut egui::Ui) {
        use crate::pixabridge::short;
        theme::caption(ui, "PixaTool");
        let mut changed = false;
        egui::Grid::new("pixatool").num_columns(2).spacing([12.0, 8.0]).show(ui, |ui| {
            ui.label("Программа");
            let exe = self.pixa.exe.as_deref().map_or("не найдена".to_string(), short);
            let r = theme::button(ui, &exe, ButtonKind::Normal)
                .on_hover_text(self.pixa.exe.as_ref().map_or("Укажите Pixatool.exe".into(), |p| p.display().to_string()));
            if r.clicked()
                && let Some(p) = rfd::FileDialog::new().set_title("Pixatool.exe").pick_file()
            {
                self.pixa.exe = Some(p);
                changed = true;
            }
            ui.end_row();
            ui.label("Пресет");
            ui.horizontal(|ui| {
                let preset = self.pixa.preset.as_deref().map_or("настройки PixaTool".to_string(), short);
                let r = theme::button(ui, &preset, ButtonKind::Normal)
                    .on_hover_text(self.pixa.preset.as_ref().map_or("Файл из «Save Preset» в PixaTool".into(), |p| p.display().to_string()));
                if r.clicked()
                    && let Some(p) = rfd::FileDialog::new().set_title("Пресет PixaTool").add_filter("Пресет PixaTool", &["preset"]).pick_file()
                {
                    self.pixa.preset = Some(p);
                    changed = true;
                }
                if self.pixa.preset.is_some() && theme::button(ui, "×", ButtonKind::Normal).on_hover_text("Без пресета").clicked() {
                    self.pixa.preset = None;
                    changed = true;
                }
            });
            ui.end_row();
        });
        if changed {
            self.pixa.save();
        }
        if let Some(job) = &self.pixa.job {
            job_progress(ui, job);
        } else if ui
            .add_enabled_ui(self.pixa.exe.is_some(), |ui| theme::button(ui, "Пикселизировать кадры…", ButtonKind::Primary))
            .inner
            .on_hover_text("Снять кадры и прогнать их через PixaTool с пресетом. PixaTool откроется сам, вне экрана.")
            .clicked()
        {
            let ctx = ui.ctx().clone();
            self.pixelize_frames(&ctx);
        }
        ui.label(RichText::new("Пресет сохраняется в PixaTool кнопкой «Save Preset».").size(12.0).color(FAINT));
    }

    fn help_window(&mut self, ctx: &egui::Context) {
        let mut open = self.show_help;
        egui::Window::new("Горячие клавиши").open(&mut open).collapsible(false).resizable(false).anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0)).show(
            ctx,
            |ui| {
                let rows = [
                    ("V H", "Выделение, рука"),
                    ("R O S N", "Прямоугольник, эллипс, звезда, многоугольник"),
                    ("T", "Текст: клик по холсту"),
                    ("P", "Перо; Enter — закончить, клик по первой точке — замкнуть"),
                    ("B", "Кость: протяжка от сустава к кончику, следующая продолжает цепочку"),
                    ("Enter", "Редактировать точки контура (или двойной клик)"),
                    ("Tab", "Переключить Design / Animate"),
                    ("Пробел", "Play / Pause (в машине состояний — запуск машины); удерживать + тянуть — панорама"),
                    ("K", "Ключ на позицию, поворот и масштаб выделенного"),
                    (", .", "Кадр назад / вперёд"),
                    ("Shift", "Пропорции при масштабе, шаг 15° при повороте"),
                    ("Стрелки", "Сдвиг на 1, с Shift — на 10"),
                    ("Ctrl+Z", "Отменить; Ctrl+Shift+Z или Ctrl+Y — повторить"),
                    ("Delete", "Удалить объект, выбранный ключ или (курсор над графом машины) состояние и переход"),
                    ("Shift+клик", "Добавить к выделению; рамка по пустому месту — выделить"),
                    ("Ctrl+C, Ctrl+V", "Копировать и вставить; Ctrl+D — дублировать, Ctrl+A — всё"),
                    ("Ctrl+G", "Сгруппировать; Ctrl+Shift+G — разгруппировать"),
                    ("Ctrl+] Ctrl+[", "На шаг выше или ниже соседей"),
                    ("F", "Вписать артборд"),
                    ("G", "Сетка"),
                    ("Alt при сдвиге", "Без умных направляющих"),
                    ("Ctrl+S, Ctrl+O", "Сохранить, открыть"),
                    ("Ctrl+E", "Свернуть или развернуть скрипт"),
                    ("F1", "Шпаргалка по синтаксису скрипта"),
                ];
                egui::Grid::new("keys").num_columns(2).spacing([16.0, 6.0]).show(ui, |ui| {
                    for (k, what) in rows {
                        let g = ui.painter().layout_no_wrap(k.to_string(), theme::mono(11.5), FG);
                        let (r, _) = ui.allocate_exact_size(galley_size(&g) + vec2(12.0, 6.0), Sense::hover());
                        ui.painter().rect(r, 4.0, BG, Stroke::new(1.0, LINE), StrokeKind::Inside);
                        ui.painter().galley(r.min + vec2(6.0, 3.0), g, FG);
                        ui.label(what);
                        ui.end_row();
                    }
                });
                ui.add_space(8.0);
                if theme::button(ui, "Открыть документацию", ButtonKind::Primary)
                    .on_hover_text("Страницы с картинками о всех инструментах, синтаксисе и экспорте")
                    .clicked()
                {
                    match docs_index() {
                        Some(path) => ctx.open_url(egui::OpenUrl::new_tab(file_url(&path))),
                        None => self.status = "Документация не найдена: папка docs должна лежать рядом с программой".into(),
                    }
                }
                ui.add_space(6.0);
                ui.label(RichText::new(format!("Motif {VERSION}")).font(theme::mono(11.0)).color(FAINT));
            },
        );
        self.show_help = open;
    }

    // ---------- keyboard ----------

    fn shortcuts(&mut self, ctx: &egui::Context) {
        let typing = ctx.text_edit_focused();
        let i = ctx.input(|i| i.clone());
        let cmd = |k: Key| i.modifiers.command && !i.modifiers.shift && i.key_pressed(k);
        if cmd(Key::S) {
            self.save(false);
        }
        if i.modifiers.command && i.modifiers.shift && i.key_pressed(Key::S) {
            self.save(true);
        }
        if cmd(Key::O) {
            self.file_action(FileAction::Open);
        }
        if cmd(Key::E) {
            self.show_script = !self.show_script;
        }
        if i.key_pressed(Key::F1) {
            self.show_syntax = !self.show_syntax;
        }
        if typing {
            return;
        }
        for e in &i.events {
            match e {
                egui::Event::Copy if self.keys_picked() => self.copy_keys(ctx),
                egui::Event::Cut if self.keys_picked() => {
                    self.copy_keys(ctx);
                    self.delete_selection();
                }
                egui::Event::Paste(text) if self.anim.is_some() && !self.key_clip.1.is_empty() && text.trim() == self.key_clip.0.trim() => self.paste_keys(),
                egui::Event::Copy => {
                    self.copy_selection(ctx);
                }
                egui::Event::Cut => {
                    if self.copy_selection(ctx) {
                        self.delete_selected_nodes();
                    }
                }
                egui::Event::Paste(text) => self.paste(Some(text)),
                _ => {}
            }
        }
        if cmd(Key::D) {
            self.duplicate();
        }
        if cmd(Key::G) {
            self.group_selection();
        }
        if i.modifiers.command && i.modifiers.shift && i.key_pressed(Key::G) {
            self.ungroup_selection();
        }
        if cmd(Key::CloseBracket) {
            self.restack_selection(true);
        }
        if cmd(Key::OpenBracket) {
            self.restack_selection(false);
        }
        if cmd(Key::A) {
            self.select_all();
        }
        if cmd(Key::Z) {
            self.undo();
        }
        if (i.modifiers.command && i.modifiers.shift && i.key_pressed(Key::Z)) || cmd(Key::Y) {
            self.redo();
        }
        let plain = |k: Key| i.modifiers == Modifiers::NONE && i.key_pressed(k);
        // Space: a tap toggles playback, holding it while dragging pans.
        if i.key_released(Key::Space) {
            if !self.space_used_for_pan && self.bottom_machine {
                self.toggle_run();
            } else if !self.space_used_for_pan && self.anim.is_some() {
                self.playing = !self.playing;
            }
            self.space_used_for_pan = false;
        }
        for (k, tool) in [
            (Key::V, Tool::Select),
            (Key::H, Tool::Hand),
            (Key::R, Tool::Create(ShapeKind::Rect)),
            (Key::O, Tool::Create(ShapeKind::Ellipse)),
            (Key::S, Tool::Create(ShapeKind::Star)),
            (Key::N, Tool::Create(ShapeKind::Polygon)),
            (Key::T, Tool::Create(ShapeKind::Text)),
            (Key::P, Tool::Pen),
            (Key::B, Tool::Create(ShapeKind::Bone)),
        ] {
            if plain(k) {
                self.tool = tool;
            }
        }
        if plain(Key::Tab) {
            let on = self.anim.is_none();
            self.set_mode_animate(on);
        }
        if plain(Key::G) {
            self.snap.grid = !self.snap.grid;
        }
        if plain(Key::F) || (i.modifiers.shift && i.key_pressed(Key::Num1)) {
            self.view = None;
        }
        if plain(Key::Enter) {
            if !self.pen.is_empty() {
                self.finish_pen(false);
            } else if self.point_edit.is_some() {
                self.point_edit = None;
            } else {
                self.begin_point_edit();
            }
        }
        if plain(Key::Escape) {
            if !self.pen.is_empty() {
                self.pen.clear();
            } else if self.brush_w.on.is_some() {
                self.brush_w.on = None;
            } else if self.point_edit.is_some() {
                self.point_edit = None;
            } else {
                self.select_only(None);
                self.tool = Tool::Select;
            }
        }
        if plain(Key::Delete) || plain(Key::Backspace) {
            if self.bottom_machine && self.graph.hovered && self.machine_pick.is_some() {
                self.delete_machine_pick();
            } else if !self.delete_point() {
                self.delete_selection();
            }
        }
        if let Some(anim) = self.current_anim().cloned() {
            let step = 1.0 / FPS;
            if plain(Key::Comma) {
                self.time = (anim.local_time(self.time) - step).max(0.0);
                self.playing = false;
            }
            if plain(Key::Period) {
                self.time = (anim.local_time(self.time) + step).min(anim.duration);
                self.playing = false;
            }
            if plain(Key::K)
                && let Some(id) = self.selected.clone()
                && let Some(n) = self.display_doc().node(&id).cloned()
            {
                for prop in [Prop::X, Prop::Y, Prop::Rotation, Prop::ScaleX, Prop::ScaleY] {
                    self.edit(&id, prop, n.get(prop));
                }
            }
        }
        if let Some(id) = self.selected.clone() {
            let base = if self.snap.free { 1.0 } else { self.snap.moving };
            let step = if i.modifiers.shift { base * 10.0 } else { base };
            let node = self.display_doc().node(&id).cloned();
            let nudge = [(Key::ArrowLeft, Prop::X, -step), (Key::ArrowRight, Prop::X, step), (Key::ArrowUp, Prop::Y, -step), (Key::ArrowDown, Prop::Y, step)];
            for (key, prop, delta) in nudge {
                if i.key_pressed(key)
                    && let Some(n) = &node
                    && let Some(v) = n.get(prop).as_num()
                {
                    self.edit(&id, prop, Value::Num(v + delta));
                }
            }
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Handle {
    Corner,
    Rotate,
}

struct Handles {
    corners: [Pos2; 4],
    rotate: Pos2,
    resizable: bool,
}

fn handle_points(rect: Rect, view: View, doc: &Document, i: usize) -> Option<Handles> {
    let node = &doc.nodes[i];
    // Bones have their own handles: the joint and the tip.
    if node.kind == ShapeKind::Bone {
        return None;
    }
    let bounds = node_bounds(doc, i)?;
    let t = doc.world_transform(i);
    let corners = [(bounds.x0, bounds.y0), (bounds.x1, bounds.y0), (bounds.x1, bounds.y1), (bounds.x0, bounds.y1)]
        .map(|(x, y)| view.to_screen(rect, t * Point::new(x, y)));
    let top_mid = corners[0].lerp(corners[1], 0.5);
    let center = corners[0].lerp(corners[2], 0.5);
    let up = (top_mid - center).normalized();
    let up = if up.is_finite() { up } else { vec2(0.0, -1.0) };
    Some(Handles { corners, rotate: top_mid + up * 22.0, resizable: !matches!(node.kind, ShapeKind::Group | ShapeKind::Path | ShapeKind::Text) })
}

pub(crate) fn node_bounds(doc: &Document, i: usize) -> Option<motif_core::kurbo::Rect> {
    match doc.nodes[i].path() {
        Some(p) => Some(p.bounding_box()),
        // Groups: the union of their direct children.
        None => {
            let inv = doc.world_transform(i).inverse();
            (0..doc.nodes.len())
                .filter(|&k| doc.nodes[k].parent == Some(i))
                .filter_map(|k| doc.nodes[k].path().map(|p| (inv * doc.world_transform(k)).transform_rect_bbox(p.bounding_box())))
                .reduce(|a, b| a.union(b))
        }
    }
}

fn outline(painter: &egui::Painter, rect: Rect, view: View, doc: &Document, i: usize, stroke: Stroke) {
    if doc.nodes[i].kind == ShapeKind::Bone {
        return;
    }
    let Some(bounds) = node_bounds(doc, i) else { return };
    let t = doc.world_transform(i);
    let corners = [(bounds.x0, bounds.y0), (bounds.x1, bounds.y0), (bounds.x1, bounds.y1), (bounds.x0, bounds.y1)]
        .map(|(x, y)| view.to_screen(rect, t * Point::new(x, y)));
    painter.add(egui::Shape::closed_line(corners.to_vec(), stroke));
}

/// The stage's dot grid, around the artboard only.
/// The nine-slice lines of a UI piece: what lies outside them keeps its size when the piece stretches.
fn slice_guides(painter: &egui::Painter, rect: Rect, view: View, doc: &Document, i: usize) {
    let (Some([l, t, r, b]), Some(bx)) = (doc.nodes[i].slice, motif_pixel::kit::piece_box(doc, i)) else { return };
    let stroke = Stroke::new(1.0, Color32::from_rgb(255, 92, 170));
    let p = |x: f64, y: f64| view.to_screen(rect, Point::new(x, y));
    let lines = [
        [p(bx.x0 + l, bx.y0), p(bx.x0 + l, bx.y1)],
        [p(bx.x1 - r, bx.y0), p(bx.x1 - r, bx.y1)],
        [p(bx.x0, bx.y0 + t), p(bx.x1, bx.y0 + t)],
        [p(bx.x0, bx.y1 - b), p(bx.x1, bx.y1 - b)],
    ];
    for line in lines {
        painter.extend(egui::Shape::dashed_line(&line, stroke, 4.0, 3.0));
    }
}

fn paint_dots(painter: &egui::Painter, rect: Rect, board: Rect, view: View) {
    let step = 16.0;
    let origin = rect.min + view.pan;
    let start_x = origin.x - ((origin.x - rect.left()) / step).floor() * step;
    let start_y = origin.y - ((origin.y - rect.top()) / step).floor() * step;
    let mut y = start_y;
    while y < rect.bottom() {
        let mut x = start_x;
        while x < rect.right() {
            let p = pos2(x, y);
            if !board.expand(2.0).contains(p) {
                painter.circle_filled(p, 1.0, theme::DOT);
            }
            x += step;
        }
        y += step;
    }
}

fn galley_size(g: &std::sync::Arc<egui::Galley>) -> egui::Vec2 {
    g.size()
}

/// Caps title with a counter on the right, under a bottom line (`СЛОИ 5`).
pub(crate) fn panel_header(ui: &mut egui::Ui, title: &str, meta: &str) {
    panel_header_colored(ui, title, meta, MUTED);
}

fn panel_header_colored(ui: &mut egui::Ui, title: &str, meta: &str, meta_color: Color32) {
    let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 32.0), Sense::hover());
    let p = ui.painter();
    let g = p.layout_no_wrap(title.to_uppercase(), theme::semibold(11.0), MUTED);
    p.galley(pos2(r.left() + 12.0, r.center().y - g.size().y / 2.0), g, MUTED);
    p.text(pos2(r.right() - 12.0, r.center().y), Align2::RIGHT_CENTER, meta, theme::mono(11.0), meta_color);
    p.line_segment([r.left_bottom(), r.right_bottom()], Stroke::new(1.0, LINE));
}

/// A properties section: padded, with a line under it.
pub(crate) fn section(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui)) {
    egui::Frame::new().inner_margin(egui::Margin::same(12)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.spacing_mut().item_spacing.y = 8.0;
        add(ui);
    });
    let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 1.0), Sense::hover());
    ui.painter().rect_filled(r, 0.0, LINE);
}

/// A properties section under a caption that folds it away. Whether a caption is folded is kept
/// per caption, so «Эффекты» folded once stays folded for every object.
pub(crate) fn fold(ui: &mut egui::Ui, title: &str, add: impl FnOnce(&mut egui::Ui)) {
    let key = egui::Id::unique(("fold", title));
    let mut open = ui.data_mut(|d| d.get_persisted::<bool>(key)).unwrap_or(true);
    egui::Frame::new().inner_margin(egui::Margin::same(12)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.spacing_mut().item_spacing.y = 8.0;
        let (r, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 16.0), Sense::click());
        let color = if resp.hovered() { FG } else { MUTED };
        fold_arrow(ui.painter(), pos2(r.left() + 5.0, r.center().y), open, color);
        let g = ui.painter().layout_no_wrap(title.to_uppercase(), theme::semibold(11.0), color);
        ui.painter().galley(pos2(r.left() + 16.0, r.center().y - g.size().y / 2.0), g, color);
        if resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
            open = !open;
            ui.data_mut(|d| d.insert_persisted(key, open));
        }
        if open {
            add(ui);
        }
    });
    let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 1.0), Sense::hover());
    ui.painter().rect_filled(r, 0.0, LINE);
}

/// The small triangle of a folding row: pointing down when open, right when folded.
pub(crate) fn fold_arrow(painter: &egui::Painter, c: Pos2, open: bool, color: Color32) {
    let pts = if open {
        vec![c + vec2(-4.0, -2.0), c + vec2(4.0, -2.0), c + vec2(0.0, 3.0)]
    } else {
        vec![c + vec2(-2.0, -4.0), c + vec2(3.0, 0.0), c + vec2(-2.0, 4.0)]
    };
    painter.add(egui::Shape::convex_polygon(pts, color, Stroke::NONE));
}

fn kind_name(kind: ShapeKind) -> &'static str {
    match kind {
        ShapeKind::Group => "Группа",
        ShapeKind::Rect => "Прямоугольник",
        ShapeKind::Ellipse => "Эллипс",
        ShapeKind::Star => "Звезда",
        ShapeKind::Polygon => "Многоугольник",
        ShapeKind::Path => "Контур",
        ShapeKind::Text => "Текст",
        ShapeKind::Bone => "Кость",
    }
}

fn parse_paint(text: &str) -> Option<Value> {
    let t = text.trim();
    if t.eq_ignore_ascii_case("none") {
        return Some(Value::None);
    }
    let t = if t.starts_with('#') { t.to_string() } else { format!("#{t}") };
    Color::from_hex(&t).map(Value::Color)
}

/// `update.bat` next to the editor, as in the Windows build folder.
/// What «Файл» was asked to do when the scene had unsaved changes.
#[derive(Clone, Copy)]
enum FileAction {
    New(&'static str),
    Open,
}

fn updater_path() -> Option<PathBuf> {
    let p = std::env::current_exe().ok()?.parent()?.join("update.bat");
    p.exists().then_some(p)
}

pub(crate) fn mtime(p: &std::path::Path) -> Option<SystemTime> {
    std::fs::metadata(p).and_then(|m| m.modified()).ok()
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let dt = ctx.input(|i| i.stable_dt);
        self.frame_ms = self.frame_ms * 0.9 + dt * 1000.0 * 0.1;
        self.poll_disk();
        self.dropped_files(&ctx);
        self.refresh_backdrop();
        if let Some(status) = self.pixa.poll() {
            self.status = status;
        }
        self.shortcuts(&ctx);
        if self.run.is_some() {
            self.advance_run(dt.min(0.1));
            ctx.request_repaint();
        } else if self.playing {
            self.time += dt.min(0.1) as f64;
            ctx.request_repaint();
        } else if self.path.is_some() {
            // Keep polling the file on disk for external edits.
            ctx.request_repaint_after(std::time::Duration::from_millis(500));
        }
        let title = format!("Motif — {}{}", self.path.as_ref().map_or("без имени".into(), |p| p.display().to_string()), if self.dirty() { " *" } else { "" });
        ctx.send_viewport_cmd(egui::ViewportCommand::Title(title));

        let panel = egui::Frame::new().fill(PANEL);
        egui::Panel::top("top").exact_size(44.0).frame(panel.inner_margin(egui::Margin::symmetric(8, 0))).show(ui, |ui| self.top_bar(ui));
        egui::Panel::bottom("status").exact_size(26.0).frame(panel.inner_margin(egui::Margin::symmetric(12, 0))).show(ui, |ui| self.status_bar(ui));
        egui::Panel::right("props").resizable(true).default_size(264.0).size_range(240.0..=560.0).frame(panel).show(ui, |ui| self.props_panel(ui));
        egui::Panel::bottom("timeline").resizable(true).default_size(236.0).size_range(120.0..=520.0).frame(panel).show(ui, |ui| self.timeline(ui));
        egui::Panel::left("tools").exact_size(48.0).frame(panel).show(ui, |ui| self.tool_rail(ui));
        egui::Panel::left("layers").resizable(true).default_size(210.0).size_range(160.0..=360.0).frame(panel).show(ui, |ui| self.layers_panel(ui));
        if self.show_script {
            egui::Panel::left("script").resizable(true).default_size(440.0).size_range(240.0..=900.0).frame(panel).show(ui, |ui| self.script_panel(ui));
        } else {
            egui::Panel::left("script-strip").exact_size(30.0).frame(panel).show(ui, |ui| self.script_strip(ui));
        }
        egui::CentralPanel::no_frame().show(ui, |ui| self.canvas_ui(ui, frame));
        self.export_window(&ctx);
        self.help_window(&ctx);
        self.unsaved_window(&ctx);
        self.syntax_window(&ctx);
    }
}

fn mix_u8(a: u8, b: u8) -> u8 {
    ((a as u16 + b as u16) / 2) as u8
}

/// The documentation start page: `docs` next to the program, or in the source tree when run with cargo.
fn docs_index() -> Option<std::path::PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let near = exe.parent()?.join("docs").join("index.html");
    let source = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/index.html");
    [near, source].into_iter().find(|p| p.is_file())
}

/// `file:///C:/…` with non-ASCII letters and spaces percent-encoded, so any browser opens it.
fn file_url(path: &std::path::Path) -> String {
    let text = path.to_string_lossy().replace('\\', "/");
    let mut out = String::from("file://");
    if !text.starts_with('/') {
        out.push('/');
    }
    for b in text.bytes() {
        if b.is_ascii_alphanumeric() || b"/:._-~".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// A running PixaTool job: frames done of how many, or that they are still being baked.
fn job_progress(ui: &mut egui::Ui, job: &crate::pixabridge::Job) {
    let text = if job.total == 0 { "снимаю кадры…".to_string() } else { format!("PixaTool: кадр {} из {}", job.done, job.total) };
    ui.add(egui::ProgressBar::new(job.done as f32 / job.total.max(1) as f32).text(text));
}
