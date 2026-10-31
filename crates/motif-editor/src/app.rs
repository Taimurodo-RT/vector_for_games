use crate::canvas::{Canvas, View, artboard_rect};
use crate::pathedit::{PenPoint, PointEdit};
use crate::theme::{self, ACCENT, ACCENT_INK, BG, ButtonKind, DANGER, FAINT, FG, GOOD, Icon, KeyState, LINE, MUTED, PANEL, SEL};
use egui::{Align2, Color32, Key, Modifiers, Pos2, Rect, RichText, Sense, Stroke, StrokeKind, TextureHandle, TextureOptions, pos2, vec2};
use motif_core::kurbo::{Affine, Point, Shape};
use motif_core::{Color, Document, Prop, Purpose, ShapeKind, Value};
use motif_pixel::{FrameExport, PIXATOOL_MAX_COLORS, parse_jasc, quantize, render_frames_facing, scene_palette, to_jasc};
use motif_render::{CpuOptions, render_cpu};
use motif_script::{Scene, ScriptError, add_node, delete_key, delete_node, fmt_num, parse, set_key, set_prop};
use std::path::PathBuf;
use std::time::{Instant, SystemTime};

pub(crate) const NEW_UI_KIT: &str = include_str!("../../../examples/ui-kit.msc");
pub(crate) const NEW_SCENE: &str = "artboard \"Сцена\" 320x240 bg #f4f1ff game\n\nrect bg at 160 200 size 280 16 r 8 fill #2a2e39\nellipse hero at 160 120 size 64 fill #ff5a36 stroke #1b1d24 3\n\nanim Idle 1s loop {\n  hero.y 0: 120 ease-in-out | 0.5: 100 ease-in-out | 1: 120\n}\n";
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
    pub(crate) scale: f64,
    pub(crate) antialias: bool,
    transparent: bool,
    /// Export every board of the file, not only the open one.
    pub(crate) all_boards: bool,
}

pub struct App {
    pub(crate) web: crate::web_ui::WebState,
    pub(crate) path: Option<PathBuf>,
    pub(crate) src: String,
    pub(crate) saved_src: String,
    recovery: crate::storage::Recovery,
    recovery_records: Vec<crate::storage::Record>,
    show_recovery: bool,
    /// The copy waiting for «Точно удалить»: one of them, or all (`Some(None)`).
    recovery_drop: Option<Option<usize>>,
    /// The workspace a UI check started from, to return to when it stops.
    pub(crate) check_from: Option<crate::workspace::Workspace>,
    recovery_title: String,
    last_recovery: Instant,
    close_allowed: bool,
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
    /// Where the next click on the canvas puts the color under it.
    pub(crate) eyedrop: Option<crate::palette_ui::Eyedrop>,
    /// The «Растяжение 9-slice» window, when open.
    pub(crate) stretch: Option<crate::stretch_ui::Stretch>,
    /// The «Новое…» library of templates, when open.
    pub(crate) library: Option<crate::library_ui::Library>,
    pub(crate) hovered: Option<String>,
    view: Option<View>,
    pub(crate) anim: Option<String>,
    /// Animation to return to when switching back to Animate.
    pub(crate) last_anim: Option<String>,
    pub(crate) panels: crate::workspace::Panels,
    workspace_store: Option<crate::workspace::Store>,
    profile: Option<Purpose>,
    clip_times: std::collections::BTreeMap<(usize, String), f64>,
    pub(crate) auto_key: bool,
    draft_pose: crate::workspace::DraftPose,
    play_needs_record: bool,
    floating: Option<canvas_menu::FloatingInspector>,
    canvas_bounds: Rect,
    preview: crate::preview::Preview,
    preview_canvas: preview_ui::PreviewCanvas,
    preview_tab: bool,
    deformer: deformer_ui::DeformerEdit,
    html_import: html_ui::HtmlImport,
    pub(crate) pose_preview: std::collections::BTreeMap<String, f64>,
    /// The side of `directions` the canvas shows (the compass in «Стороны»), or the scene as written.
    pub(crate) side: Option<usize>,
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
    /// The machine run being recorded («Запись»).
    pub(crate) take: Option<motif_core::Take>,
    /// The interface check: the `ui` pieces working under the mouse.
    pub(crate) check: Option<motif_core::UiCheck>,
    /// The pointer listener being put together in the machine panel: object, event, input.
    pub(crate) listen_draft: (String, motif_core::PointerEvent, String),
    /// The weight brush of skinned shapes.
    pub(crate) brush_w: crate::weights_ui::WeightBrush,
    pub(crate) graph: crate::machine_ui::Graph,
    /// Undo steps: the edited board and the whole file before the change.
    pub(crate) undo: Vec<(usize, String)>,
    redo: Vec<(usize, String)>,
    pub(crate) boards: crate::boards_ui::Boards,
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
    /// Motion paths of the selected objects in Animate.
    pub(crate) motion_path: bool,
    pub(crate) motion: std::cell::RefCell<(u64, Vec<crate::motion_ui::MotionPath>)>,
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
    /// The data of the event whose menu is open, as edited: key, value as text, value as read.
    pub(crate) event_data: Vec<(String, String, Option<motif_core::EventValue>)>,
    /// The name typed in the animation menu.
    pub(crate) anim_name: String,
    /// The timeline shows value curves instead of key lanes.
    pub(crate) curves: bool,
    /// The property whose curve is edited in the graph.
    pub(crate) curve_focus: Option<(String, Prop)>,
    pub(crate) curve_drag: Option<crate::timeline::CurveDrag>,
    show_export: bool,
    /// A file action waiting for an answer about unsaved changes.
    pending_file: Option<FileAction>,
    show_help: bool,
    pub(crate) show_syntax: bool,
    space_used_for_pan: bool,
    hex_edit: Option<(Prop, String)>,
    pixel: PixelPreview,
    pub(crate) export: ExportSettings,
    pub(crate) pixa: crate::pixabridge::Bridge,
    pub(crate) video: crate::video_ui::VideoExport,
    /// For work that finishes on another thread and must wake the window (re-export on save).
    pub(crate) ctx: egui::Context,
    canvas: Canvas,
    pub(crate) backdrop_img: crate::importer::BackdropCache,
    pub(crate) status: String,
    last_poll: Instant,
    pointer_doc: Option<Point>,
    frame_ms: f32,
    pub(crate) layers: crate::layers::Layers,
    /// Soft-body frames already simulated, for the script and animation they were simulated for.
    sim: std::cell::RefCell<(u64, motif_core::SimCache)>,
    /// The live link with a game (Forge), when it is on.
    pub(crate) link: crate::link_ui::Link,
    /// Scenes hung on attachment points while editing; never saved.
    pub(crate) tryon: Vec<crate::game_ui::TryOn>,
}

impl App {
    /// `link`: the port a game asked Motif to listen on when it started it.
    pub fn new(cc: &eframe::CreationContext<'_>, path: Option<PathBuf>, link: Option<u16>) -> Self {
        let mut app =
            Self::initialize(cc.egui_ctx.clone(), path, crate::pixabridge::Bridge::load(), crate::storage::Recovery::new(crate::storage::recovery_dir()));
        let path = crate::storage::recovery_dir().with_file_name("workspaces.json");
        let (store, settings, warning) = crate::workspace::Store::load(path);
        let space = settings.workspace;
        app.panels.settings = settings;
        app.workspace_store = Some(store);
        app.set_workspace(space);
        if let Some(port) = link.or(app.panels.settings.link.then_some(app.panels.settings.link_port)) {
            app.start_link(port);
        }
        if let Some(warning) = warning {
            app.status = warning;
        }
        app
    }

    fn initialize(ctx: egui::Context, path: Option<PathBuf>, pixa: crate::pixabridge::Bridge, recovery: crate::storage::Recovery) -> Self {
        theme::install(&ctx);
        let path = path.map(|p| std::path::absolute(&p).unwrap_or(p));
        let (src, status) = match &path {
            Some(p) => match std::fs::read_to_string(p) {
                Ok(s) => (s, format!("Открыт {}", p.display())),
                Err(e) => (NEW_SCENE.to_string(), format!("Не удалось открыть {}: {e}", p.display())),
            },
            None => (EXAMPLE.to_string(), "Открыт пример. Сохраните его, чтобы работать с файлом.".into()),
        };
        let src_file = src.clone();
        let scene = parse(&src).or_else(|_| parse(NEW_SCENE)).expect("built-in scene parses");
        let recovery_records = recovery.entries();
        let mut app = App {
            show_recovery: !recovery_records.is_empty(),
            recovery_drop: None,
            check_from: None,
            recovery,
            recovery_records,
            recovery_title: "Восстановление документов".into(),
            last_recovery: Instant::now(),
            close_allowed: false,
            web: crate::web_ui::WebState::default(),
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
            stretch: None,
            library: None,
            eyedrop: None,
            hovered: None,
            view: None,
            anim: None,
            last_anim: None,
            panels: Default::default(),
            workspace_store: None,
            profile: None,
            clip_times: Default::default(),
            auto_key: true,
            draft_pose: Default::default(),
            play_needs_record: false,
            floating: None,
            canvas_bounds: Rect::NOTHING,
            preview: Default::default(),
            preview_canvas: Default::default(),
            preview_tab: true,
            deformer: Default::default(),
            html_import: Default::default(),
            pose_preview: Default::default(),
            side: None,
            time: 0.0,
            playing: false,
            bottom_machine: false,
            machine: None,
            machine_pick: None,
            run: None,
            take: None,
            check: None,
            listen_draft: (String::new(), motif_core::PointerEvent::Hover, String::new()),
            graph: Default::default(),
            undo: Vec::new(),
            redo: Vec::new(),
            boards: Default::default(),
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
            motion_path: true,
            motion: Default::default(),
            sel_keys: Vec::new(),
            key_drag: None,
            key_clip: (String::new(), Vec::new()),
            key_marquee: None,
            event_drag: None,
            event_name: String::new(),
            event_data: Vec::new(),
            anim_name: String::new(),
            curves: false,
            curve_focus: None,
            curve_drag: None,
            show_export: false,
            show_help: false,
            show_syntax: false,
            space_used_for_pan: false,
            hex_edit: None,
            pixel: PixelPreview { on: false, width: 96, palette: None, texture: None },
            export: ExportSettings { fps: 12.0, scale: 1.0, antialias: true, transparent: true, all_boards: false },
            video: crate::video_ui::VideoExport::new(pixa.ffmpeg.clone()),
            pixa,
            ctx,
            canvas: Canvas::new(),
            guides: Vec::new(),
            backdrop_img: Default::default(),
            status,
            last_poll: Instant::now(),
            pointer_doc: None,
            frame_ms: 16.0,
            sim: Default::default(),
            link: Default::default(),
            tryon: Vec::new(),
            layers: Default::default(),
        };
        app.set_file(&src_file, 0);
        app.reparse();
        app
    }

    // ---------- script state ----------

    pub(crate) fn reparse(&mut self) {
        self.draft_pose.clear();
        self.split_active();
        match motif_script::parse_in(&self.src, self.path.as_ref().and_then(|p| p.parent())) {
            Ok(scene) => self.accept_scene(scene),
            Err(errors) => self.errors = errors,
        }
    }

    fn accept_scene(&mut self, scene: Scene) {
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
        if self.anim.is_none() {
            self.anim = self
                .last_anim
                .clone()
                .filter(|name| self.scene.doc.animation(name).is_some())
                .or_else(|| self.scene.doc.animations.first().map(|a| a.name.clone()));
        }
        let keys = std::mem::take(&mut self.sel_keys);
        self.sel_keys = keys.into_iter().filter(|k| self.key_exists(k)).collect();
        self.prune_selection();
        self.prune_deformer_reference();
        self.prune_machine();
        self.refresh_check();
    }

    fn key_exists(&self, k: &KeyRef) -> bool {
        self.current_anim()
            .and_then(|a| a.tracks.iter().find(|t| t.node == k.node && t.prop == k.prop))
            .is_some_and(|t| t.keys.iter().any(|key| (key.time - k.time).abs() < 0.005))
    }

    /// Saves an undo step, merging rapid changes of the same kind (typing, dragging) into one.
    pub(crate) fn checkpoint(&mut self, kind: &str) {
        if self.last_checkpoint.0 != kind || self.last_checkpoint.1.elapsed().as_secs_f32() > 0.8 {
            self.undo.push((self.boards.active, self.file_text()));
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
            Some(mut s) if s != self.src => {
                if let Some((clip, time)) = self.key_time()
                    && time > 0.0
                    && matches!(kind, "create" | "paste" | "duplicate" | "add-use")
                {
                    let base = self.path.as_ref().and_then(|p| p.parent());
                    if let Ok(next) = motif_script::parse_in(&s, base) {
                        let new: Vec<String> = next
                            .doc
                            .nodes
                            .iter()
                            .filter(|n| {
                                n.visible
                                    && self.scene.doc.node(&n.id).is_none()
                                    && n.parent.is_none_or(|p| self.scene.doc.node(&next.doc.nodes[p].id).is_some())
                            })
                            .map(|n| n.id.clone())
                            .collect();
                        for id in new {
                            let Ok(parsed) = motif_script::parse_in(&s, base) else { return false };
                            let Some(out) = motif_script::forms::key_appearance(&s, &parsed, &clip, &id, time, base) else { return false };
                            s = out;
                        }
                    }
                }
                let parsed = match motif_script::parse_in(&s, self.path.as_ref().and_then(|p| p.parent())) {
                    Ok(scene) => scene,
                    Err(errors) => {
                        self.status = errors.first().map(ToString::to_string).unwrap_or_default();
                        return false;
                    }
                };
                self.checkpoint(kind);
                self.src = s;
                self.draft_pose.clear();
                if motif_script::boards(&self.src).1.len() > 1 {
                    self.reparse();
                } else {
                    self.accept_scene(parsed);
                }
                true
            }
            _ => false,
        }
    }

    /// Discrete commands never merge with a previous or following command, however fast clicked.
    pub(crate) fn apply_once(&mut self, kind: &str, new_src: Option<String>) -> bool {
        self.last_checkpoint.0.clear();
        let changed = self.apply(kind, new_src);
        self.last_checkpoint.0.clear();
        changed
    }

    pub(crate) fn can_edit(&mut self) -> bool {
        if self.errors.is_empty() {
            return true;
        }
        self.status = "Сначала исправьте ошибки в скрипте: правки мышью пишутся в рабочую версию".into();
        false
    }

    /// A selected clip authors the shared current frame in every workspace; without a clip, edits change the base.
    pub(crate) fn edit(&mut self, node: &str, prop: Prop, value: Value) {
        if !self.can_edit() {
            return;
        }
        if prop == Prop::Parameter && self.run.is_some() {
            self.status = "Для записи параметров остановите машину состояний и выберите клип на таймлайне".into();
            return;
        }
        if prop == Prop::Parameter && self.anim.is_none() && self.workspace() == crate::workspace::Workspace::Setup {
            if let Value::Num(value) = value {
                self.parameter_preview(node, value);
            }
            return;
        }
        self.pose_preview.clear();
        self.deformer.form = None;
        self.deformer.corrective = None;
        if let Some(at) = self.key_time() {
            if !self.auto_key {
                self.playing = false;
                self.draft_pose.set(at, node, prop, value);
                self.status = "Пробная поза. Ромб или «Записать позу» сохраняет ключи; автоключ выключен".into();
                return;
            }
        } else if self.workspace() == crate::workspace::Workspace::Animate {
            self.status = "Создайте клип для записи ключей".into();
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
        if self.apply(&kind, new_src) && self.key_time().is_none() {
            // The drawing changed for every clip; a clip that keys this property still plays its own value.
            let keyed: Vec<&str> =
                self.scene.doc.animations.iter().filter(|a| a.tracks.iter().any(|t| t.node == node && t.prop == prop)).map(|a| a.name.as_str()).collect();
            if !keyed.is_empty() {
                self.status = format!("Рисунок изменён. В клипах {} {node}.{} задано ключами — там играет ключ", keyed.join(", "), prop.path_name());
            }
        }
    }

    /// The diamond next to a property: removes the key at the current time, or keys the current value.
    pub(crate) fn toggle_key(&mut self, node: &str, prop: Prop) {
        let Some((anim, t)) = self.key_time() else { return };
        if self.key_state(node, prop) == KeyState::Key && !(self.has_draft_pose() && self.draft_pose.values.contains_key(&(node.into(), prop))) {
            let out = delete_key(&self.src, &self.scene, &anim, node, prop, t);
            self.apply("key", out);
            self.status = format!("Ключ {node}.{} на {} с удалён", prop.path_name(), fmt_num(t));
        } else if let Some(value) = self.display_doc().animated_value(node, prop) {
            let auto_key = self.auto_key;
            let draft = self.draft_pose.clone();
            self.auto_key = true;
            self.edit(node, prop, value);
            self.auto_key = auto_key;
            self.draft_pose = draft;
            self.draft_pose.values.remove(&(node.into(), prop));
        }
    }

    pub(crate) fn has_draft_pose(&self) -> bool {
        self.draft_pose.matches(&self.key_time()) && !self.draft_pose.values.is_empty()
    }

    pub(crate) fn clear_draft_pose(&mut self) {
        self.play_needs_record = false;
        self.draft_pose.clear();
    }

    pub(crate) fn record_draft_pose(&mut self) {
        if !self.can_edit() || !self.has_draft_pose() {
            return;
        }
        let Some((anim, time)) = self.key_time() else { return };
        let mut source = self.src.clone();
        for ((node, prop), value) in &self.draft_pose.values {
            let Ok(scene) = motif_script::parse_in(&source, self.path.as_ref().and_then(|p| p.parent())) else { return };
            let Some(next) = set_key(&source, &scene, &anim, node, *prop, time, *value) else { return };
            source = next;
        }
        if source == self.src {
            self.clear_draft_pose();
            self.status = "Эта поза уже записана".into();
            return;
        }
        if self.apply_once("record-pose", Some(source)) {
            self.status = "Пробная поза записана одним действием".into();
        }
    }

    fn record_transform(&mut self) {
        let Some(id) = self.selected.clone() else { return };
        let Some(node) = self.display_doc().node(&id).cloned() else { return };
        if self.key_time().is_none() {
            return;
        }
        let props = [Prop::X, Prop::Y, Prop::Rotation, Prop::ScaleX, Prop::ScaleY];
        let mut draft = self.draft_pose.clone();
        let auto_key = self.auto_key;
        self.auto_key = true;
        self.last_checkpoint.0.clear();
        self.edit_group = Some("key-transform".into());
        for prop in props {
            self.edit(&id, prop, node.get(prop));
        }
        self.edit_group = None;
        self.last_checkpoint.0.clear();
        self.auto_key = auto_key;
        for prop in props {
            draft.values.remove(&(id.clone(), prop));
        }
        self.draft_pose = draft;
    }

    pub(crate) fn current_anim(&self) -> Option<&motif_core::Animation> {
        self.scene.doc.animation(self.anim.as_deref()?)
    }

    /// The clip the canvas shows posed: in «Рисование» only while it plays, so the drawing
    /// itself is what is drawn and edited there.
    pub(crate) fn posed_anim(&self) -> Option<&motif_core::Animation> {
        self.current_anim().filter(|_| self.workspace() != crate::workspace::Workspace::Drawing || self.playing)
    }

    /// The selected animation and the time within it, rounded the way keys are written. None in
    /// «Рисование»: edits there change the drawing for every clip, keys are written elsewhere.
    pub(crate) fn key_time(&self) -> Option<(String, f64)> {
        if self.workspace() == crate::workspace::Workspace::Drawing {
            return None;
        }
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

    pub(crate) fn workspace(&self) -> crate::workspace::Workspace {
        self.panels.settings.workspace
    }

    pub(crate) fn tool_profile(&self) -> Purpose {
        self.profile.unwrap_or(self.scene.doc.purpose)
    }

    fn set_tool_profile(&mut self, profile: Purpose) {
        self.profile = Some(profile);
        self.status = "Профиль инструментов изменён; сцена и экспортные данные сохранены".into();
    }

    fn set_document_purpose(&mut self, purpose: Purpose) {
        if !self.can_edit() || self.scene.doc.purpose == purpose {
            return;
        }
        if self.apply_once("purpose", motif_script::set_purpose(&self.src, purpose)) {
            self.profile = None;
            self.status = "Назначение артборда сохранено в документе".into();
        }
    }

    pub(crate) fn reset_workspace_document(&mut self) {
        self.deformer = Default::default();
        self.preview = Default::default();
        self.preview_canvas = Default::default();
        self.floating = None;
        self.play_needs_record = false;
        self.profile = None;
        self.clip_times.clear();
        self.draft_pose.clear();
        self.last_anim = None;
        self.run = None;
        self.check = None;
        self.playing = false;
        self.auto_key = true;
    }

    fn remember_clip_time(&mut self) {
        if let Some(name) = &self.anim {
            self.clip_times.insert((self.boards.active, name.clone()), self.time);
            self.last_anim = Some(name.clone());
        }
    }

    pub(crate) fn set_workspace(&mut self, space: crate::workspace::Workspace) {
        if self.drag.is_some()
            || self.key_drag.is_some()
            || self.deformer.grab.is_some()
            || self.point_edit.as_ref().is_some_and(|e| e.live.is_some())
            || !self.pen.is_empty()
        {
            self.status = "Завершите действие на холсте или нажмите Esc перед сменой пространства".into();
            return;
        }
        self.remember_clip_time();
        self.check_from = None;
        self.panels.switch(space);
        self.deformer.form = None;
        self.deformer.corrective = None;
        self.playing = false;
        self.run = None;
        self.check = None;
        self.pose_preview.clear();
        self.point_edit = None;
        self.brush_w.on = None;
        self.live.clear();
        self.sel_keys.clear();
        self.tool = Tool::Select;
        if self.anim.is_none() {
            self.anim = self
                .last_anim
                .clone()
                .filter(|name| self.scene.doc.animation(name).is_some())
                .or_else(|| self.scene.doc.animations.first().map(|a| a.name.clone()));
            if let Some(name) = &self.anim {
                self.time = self.clip_times.get(&(self.boards.active, name.clone())).copied().unwrap_or(0.0);
            }
        }
    }

    pub(crate) fn set_mode_animate(&mut self, on: bool) {
        self.set_workspace(if on { crate::workspace::Workspace::Animate } else { crate::workspace::Workspace::Drawing });
    }

    pub(crate) fn create_animation(&mut self) {
        if !self.can_edit() {
            return;
        }
        self.set_workspace(crate::workspace::Workspace::Animate);
        let (out, name) = motif_script::add_anim(&self.src, &self.scene);
        if self.apply_once("anim", Some(out)) {
            self.select_anim(name.clone());
            self.status = format!("Создан клип «{name}». Общий кадр для рисования, настройки и анимации");
        }
    }

    fn persist_workspace(&mut self) {
        if let Some(store) = &mut self.workspace_store
            && let Err(e) = store.save(&self.panels.settings)
        {
            self.status = format!("Не удалось сохранить раскладку: {e}");
        }
    }

    /// Adds the animations a kit takes its states from, and opens the first in Animate.
    /// Moves the shapes of piece `id` so their edges land on whole pixels of the export.
    fn align_to_pixels(&mut self, id: &str) {
        if self.key_time().is_some() {
            self.status = "Выравнивание по пикселям меняет сцену: выключите Animate".into();
            return;
        }
        let Some(i) = self.scene.doc.index_of(id) else { return };
        let (_, edits) = crate::kit_ui::crisp_edits(&self.scene.doc, i, self.export.scale);
        self.edit_group = Some("pixel-align".into());
        for (node, prop, v) in &edits {
            self.edit(node, *prop, Value::Num(*v));
        }
        self.edit_group = None;
        self.status = format!("Выровнено по пикселям: правок {}", edits.len());
    }

    fn add_ui_states(&mut self) {
        if !self.can_edit() {
            return;
        }
        self.set_mode_animate(true);
        let (out, added) = motif_script::add_states(&self.src, &self.scene, &["Hover", "Press"], 0.25);
        self.apply("states", Some(out));
        self.set_mode_animate(true);
        self.select_anim("Hover".into());
        self.status = if added.is_empty() {
            "Анимации Hover и Press уже есть: записывайте позу ромбом или включите автоключ".into()
        } else {
            format!("Добавлены анимации {}: записывайте свойства в ключи, последний кадр станет состоянием", added.join(", "))
        };
    }

    /// An empty `Value` animation: a slider drawn by it, from value 0 at the start to 1 at the end.
    fn add_value_anim(&mut self) {
        if !self.can_edit() {
            return;
        }
        self.set_mode_animate(true);
        let (out, _) = motif_script::add_states(&self.src, &self.scene, &["Value"], 1.0);
        self.apply("value", Some(out));
        self.set_mode_animate(true);
        self.select_anim("Value".into());
        self.status = "Добавлена анимация Value: начало — значение 0, конец — 1. Ключи рисуют слайдер, «▶ Проверить» тянет его мышью".into();
    }

    pub(crate) fn select_anim(&mut self, name: String) {
        if self.scene.doc.animation(&name).is_none() {
            return;
        }
        self.pose_preview.clear();
        self.deformer.form = None;
        self.deformer.corrective = None;
        self.remember_clip_time();
        self.panels.set(crate::workspace::Panel::Timeline, true);
        self.run = None;
        self.check = None;
        self.tool = Tool::Select;
        self.point_edit = None;
        self.draft_pose.clear();
        if self.anim.as_deref() != Some(name.as_str()) {
            self.time = self.clip_times.get(&(self.boards.active, name.clone())).copied().unwrap_or(0.0);
            self.sel_keys.clear();
        }
        self.last_anim = Some(name.clone());
        self.anim = Some(name);
    }

    fn undo(&mut self) {
        if let Some((board, prev)) = self.undo.pop() {
            self.redo.push((self.boards.active, self.file_text()));
            self.restore(board, &prev);
        }
    }

    fn redo(&mut self) {
        if let Some((board, next)) = self.redo.pop() {
            self.undo.push((self.boards.active, self.file_text()));
            self.restore(board, &next);
        }
    }

    /// Brings back the file as an undo step saved it, editing the board it was edited on.
    fn restore(&mut self, board: usize, file: &str) {
        if let Some(edit) = &mut self.point_edit {
            edit.selected = None;
            edit.grab = None;
            edit.live = None;
        }
        if board != self.boards.active {
            self.selected = None;
            self.also.clear();
            self.anim = None;
            self.run = None;
            self.check = None;
        }
        self.set_file(file, board);
        self.last_checkpoint.0.clear();
        self.reparse();
    }

    pub(crate) fn dirty(&self) -> bool {
        self.file_text() != self.saved_src
    }

    pub(crate) fn display_doc(&self) -> Document {
        let mut doc = match (self.running(), self.posed_anim()) {
            (Some((m, player)), _) => player.pose(&self.faced(m), &self.scene.doc),
            // A UI check owns state playback. The clip selected for authoring is
            // retained in the workspace, but cannot become the normal-state base.
            (None, _) if self.check.is_some() => self.scene.doc.clone(),
            (None, Some(a)) => self.pose_anim(a, self.time),
            (None, None) => self.scene.doc.clone(),
        };
        let mut needs_rig = false;
        if self.run.is_none() && (self.anim.is_none() || !self.pose_preview.is_empty()) && !doc.parameters.is_empty() {
            for p in &mut doc.parameters {
                p.value = self.pose_preview.get(&p.id).copied().unwrap_or(p.value);
            }
            doc.apply_parameters();
            needs_rig = true;
        }
        if self.draft_pose.matches(&self.key_time()) && (self.workspace() != crate::workspace::Workspace::Setup || self.pose_preview.is_empty()) {
            let parameters_changed = self.draft_pose.values.keys().any(|(_, prop)| *prop == Prop::Parameter);
            for ((node, prop), value) in &self.draft_pose.values {
                if *prop == Prop::Parameter {
                    doc.set_animated_value(node, *prop, *value);
                    needs_rig = true;
                }
            }
            if parameters_changed {
                doc.apply_parameters();
                needs_rig = true;
                if let Some(a) = self.posed_anim() {
                    a.apply_node_keys(&mut doc, self.time);
                }
            }
            for ((node, prop), value) in &self.draft_pose.values {
                if *prop != Prop::Parameter {
                    doc.set_animated_value(node, *prop, *value);
                    needs_rig = true;
                }
            }
        }
        if let Some(c) = &self.check {
            doc = c.pose(&self.scene.doc, &doc);
            needs_rig = true;
        }
        if let Some(e) = &self.point_edit
            && let Some(o) = &e.live
            && let Some(i) = doc.index_of(&e.node)
        {
            doc.nodes[i].geometry = o.to_bez();
            needs_rig = true;
            doc.nodes[i].forms = None;
            doc.nodes[i].morph = None;
        }
        for (node, prop, value) in &self.live {
            if let Some(i) = doc.index_of(node) {
                doc.nodes[i].set(*prop, *value);
                needs_rig = true;
            }
        }
        // The weight stroke being drawn bends the shape at once.
        if let Some((node, stroke)) = self.brush_w.live()
            && let Some(skin) = doc.index_of(node).and_then(|i| doc.nodes[i].skin.as_mut())
        {
            skin.paint.push(stroke);
            needs_rig = true;
        }
        if let Some(k) = self.shown_side() {
            if needs_rig {
                doc.rig();
            }
            doc.face(k);
            needs_rig = true;
        }
        if needs_rig {
            doc.rig();
        }
        if !self.web.preview_theme.is_empty()
            && let Ok(config) = motif_web::read(&self.src)
        {
            motif_web::preview_theme(&self.scene, &mut doc, &config, &self.web.preview_theme);
        }
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
        let anim = self.posed_anim().filter(|_| self.onion && !self.playing)?;
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
    pub(crate) fn file_action(&mut self, action: FileAction) {
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
            FileAction::Close => {
                self.recovery.clear();
                self.close_allowed = true;
                self.ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            FileAction::Restore(index) => {
                if let Some(record) = self.recovery_records.get(index).cloned() {
                    self.checkpoint("restore");
                    self.recovery.clear();
                    if record.file.is_some()
                        && let Err(e) = self.recovery.adopt(&record)
                    {
                        self.status = format!("Не удалось перенести копию восстановления: {e}");
                    }
                    self.path = record.path;
                    self.saved_src = record.saved_text;
                    self.set_file(&record.text, record.board);
                    self.disk_mtime = self.path.as_ref().and_then(|p| mtime(p));
                    self.selected = None;
                    self.also.clear();
                    self.view = None;
                    self.anim = None;
                    self.reparse();
                    self.show_recovery = false;
                    self.status = "Копия восстановлена в редакторе. Исходный файл не изменён; сохраните нужную версию.".into();
                    self.last_recovery = Instant::now() - std::time::Duration::from_secs(6);
                }
            }
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
        self.pose_preview.clear();
        self.reset_workspace_document();
        self.recovery.clear();
        self.checkpoint("file");
        self.set_file(template, 0);
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
        if let Err(e) = self.open_path(p) {
            self.status = e;
        }
    }

    /// Opens the scene at `p` in place of the one open; the error says why it did not.
    pub(crate) fn open_path(&mut self, p: PathBuf) -> Result<(), String> {
        match std::fs::read_to_string(&p) {
            Ok(s) => {
                self.pose_preview.clear();
                self.reset_workspace_document();
                self.recovery.clear();
                self.undo.push((self.boards.active, self.file_text()));
                self.set_file(&s, 0);
                self.saved_src = s;
                self.disk_mtime = mtime(&p);
                self.status = format!("Открыт {}", p.display());
                self.path = Some(p);
                self.selected = None;
                self.view = None;
                self.anim = None;
                self.reparse();
                Ok(())
            }
            Err(e) => Err(format!("Не удалось открыть {}: {e}", p.display())),
        }
    }

    pub(crate) fn save(&mut self, save_as: bool) {
        let path = match (&self.path, save_as) {
            (Some(p), false) => p.clone(),
            _ => match rfd::FileDialog::new().add_filter("Сцена Motif", &["msc"]).set_file_name("scene.msc").save_file() {
                Some(p) => p,
                None => return,
            },
        };
        // A file is saved in the current format, its version on the first line. Not an undo step:
        // the drawing is the same.
        if motif_script::format::declared(&self.src).is_none_or(|v| v < motif_script::format::VERSION) {
            self.src = motif_script::format::stamp(&self.src);
            self.reparse();
        }
        // Everything a game binds to gets its hidden id, so a rename later does not break it.
        let stamped = motif_script::ids::stamp(&self.src);
        if stamped != self.src {
            self.src = stamped;
            self.reparse();
        }
        let text = self.file_text();
        let expected = (self.path.as_ref() == Some(&path)).then_some(self.saved_src.as_str());
        let before = expected.map(str::to_string);
        match crate::storage::save_scene(&path, &text, expected) {
            Ok(()) => {
                self.recovery.clear();
                self.saved_src = text;
                self.disk_mtime = mtime(&path);
                self.status = format!("Сохранено: {}", path.display());
                self.path = Some(path.clone());
                self.link_saved(&path, before.as_deref());
                if let Some(link) = self.pixa.link(&path).cloned() {
                    let ctx = self.ctx.clone();
                    self.status = format!("Сохранено. {}", self.godot_to(&link.target, &ctx));
                }
                if let Some(dir) = self.pixa.runtime_link(&path).cloned() {
                    self.status = match motif_pixel::runtime::copy_scene(&path, &self.saved_src, &dir) {
                        Ok((to, _)) => format!("{} Для MotifSprite: → {}", self.status, to.display()),
                        Err(e) => format!("{} В проект Godot не скопировано: {e}", self.status),
                    };
                }
            }
            Err(e) => self.status = format!("Не удалось сохранить {}: {e}", path.display()),
        }
    }

    /// Recovery copies include incomplete scripts and every board, without touching the original.
    fn autosave(&mut self) {
        if self.close_allowed || self.last_recovery.elapsed().as_secs() < 5 {
            return;
        }
        self.last_recovery = Instant::now();
        if !self.dirty() {
            self.recovery.clear();
            return;
        }
        let record = crate::storage::Record::new(self.path.clone(), self.file_text(), self.saved_src.clone(), self.boards.active);
        if let Err(e) = self.recovery.write(&record) {
            self.status = format!("Не удалось записать копию восстановления: {e}");
        }
    }

    fn recovery_window(&mut self, ctx: &egui::Context) {
        if !self.show_recovery {
            return;
        }
        let mut open = true;
        let mut restore = None;
        let mut dropped = Vec::new();
        let asked = self.recovery_drop;
        egui::Window::new(&self.recovery_title).id(egui::Id::unique("recovery")).open(&mut open).default_width(580.0).show(ctx, |ui| {
            ui.label("Копия открывается в редакторе. Исходный файл меняется только после сохранения.");
            if self.recovery_records.is_empty() {
                ui.label("Доступных копий нет.");
            }
            egui::ScrollArea::vertical().max_height(400.0).show(ui, |ui| {
                for (index, record) in self.recovery_records.iter().enumerate() {
                    ui.separator();
                    ui.label(record.path.as_ref().map_or("Без имени".into(), |p| p.display().to_string()));
                    ui.horizontal(|ui| {
                        ui.label(format!("Копия {} · {} байт · холст {}", index + 1, record.text.len(), record.board + 1));
                        if ui.button("Восстановить").clicked() {
                            restore = Some(index);
                        }
                        // Only a recovery copy can go: earlier saves are kept by the history.
                        if record.file.is_some() {
                            if asked == Some(Some(index)) {
                                if ui.button("Точно удалить").on_hover_text("Копия удалится насовсем").clicked() {
                                    dropped.push(index);
                                }
                                if ui.button("Оставить").clicked() {
                                    self.recovery_drop = None;
                                }
                            } else if ui.button("Удалить").on_hover_text("Больше не предлагать эту копию").clicked() {
                                self.recovery_drop = Some(Some(index));
                            }
                        }
                    });
                    ui.collapsing(format!("Текст копии {}", index + 1), |ui| {
                        ui.monospace(record.text.chars().take(1200).collect::<String>());
                    });
                }
            });
            ui.separator();
            let copies = self.recovery_records.iter().filter(|r| r.file.is_some()).count();
            if copies > 1 {
                ui.horizontal(|ui| {
                    if asked == Some(None) {
                        if ui.button(format!("Точно удалить все {copies}")).clicked() {
                            dropped.extend(0..self.recovery_records.len());
                        }
                        if ui.button("Оставить").clicked() {
                            self.recovery_drop = None;
                        }
                    } else if ui.button("Удалить все копии").clicked() {
                        self.recovery_drop = Some(None);
                    }
                });
            }
            ui.small(format!("Автокопии: {}", self.recovery.dir.display()));
        });
        self.show_recovery = open;
        if !dropped.is_empty() {
            for index in dropped.into_iter().rev() {
                crate::storage::Recovery::drop_copy(&self.recovery_records.remove(index));
            }
            self.recovery_drop = None;
            self.status = "Копии восстановления удалены".into();
        }
        if let Some(index) = restore {
            self.recovery_drop = None;
            self.file_action(FileAction::Restore(index));
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
            && s != self.file_text()
        {
            self.undo.push((self.boards.active, self.file_text()));
            let board = self.boards.active;
            self.set_file(&s, board);
            self.saved_src = s;
            self.reparse();
            self.status = "Файл обновлён с диска".into();
        }
    }

    /// The frames the export window would save of `doc`: the animation, or the scene as one frame.
    fn export_render(&self, doc: &Document, anim: Option<&str>) -> Result<Vec<tiny_skia::Pixmap>, String> {
        let render = CpuOptions { scale: self.export.scale, antialias: self.export.antialias, background: !self.export.transparent };
        match anim {
            Some(a) => render_frames_facing(doc, a, &FrameExport { fps: self.export.fps, render }, self.shown_side()),
            None => render_cpu(doc, render).map(|p| vec![p]).ok_or_else(|| "не удалось создать изображение".to_string()),
        }
    }

    pub(crate) fn file_stem(&self) -> String {
        self.path.as_ref().and_then(|p| p.file_stem()).map_or("scene".to_string(), |s| s.to_string_lossy().into_owned())
    }

    /// The boards an export covers, each with the name its files get: the open board, or every
    /// board with «Все холсты». A file with one board is named after the file.
    pub(crate) fn export_boards(&self) -> Result<Vec<(String, Document)>, String> {
        let n = self.board_count();
        if n < 2 {
            return Ok(vec![(self.file_stem(), self.scene.doc.clone())]);
        }
        let ks: Vec<usize> = if self.export.all_boards { (0..n).collect() } else { vec![self.boards.active] };
        let mut out: Vec<(String, Document)> = Vec::new();
        for k in ks {
            let doc = self.board_doc(k).ok_or_else(|| format!("в скрипте холста {} ошибки", k + 1))?;
            let base = motif_script::board_file_name(&doc.artboard.name);
            let name = (1..).map(|i| if i == 1 { base.clone() } else { format!("{base}_{i}") }).find(|c| out.iter().all(|o| o.0 != *c)).expect("free");
            out.push((name, doc));
        }
        Ok(out)
    }

    /// PNG frames: the open animation, or the scene as one frame. With every board, each board
    /// is saved as `BOARD_0000.png`, by the open animation's frames where it has one of that name.
    fn export_frames(&mut self) {
        let Some(dir) = rfd::FileDialog::new().set_title("Папка для PNG-кадров").pick_folder() else { return };
        let several = self.board_count() > 1 && self.export.all_boards;
        let result = self.export_boards().and_then(|boards| {
            let mut count = 0;
            for (board, doc) in &boards {
                let anim = self.anim.as_deref().filter(|a| doc.animation(a).is_some());
                let name = match (anim, several) {
                    (Some(a), true) => format!("{board}_{a}"),
                    (Some(a), false) => a.to_string(),
                    (None, true) => board.clone(),
                    (None, false) if self.board_count() > 1 => board.clone(),
                    (None, false) => "scene".to_string(),
                };
                for (i, px) in self.export_render(doc, anim)?.iter().enumerate() {
                    let path = dir.join(format!("{name}_{i:04}.png"));
                    px.save_png(&path).map_err(|e| format!("{}: {e}", path.display()))?;
                    count += 1;
                }
            }
            Ok((count, boards.len()))
        });
        self.status = match result {
            Ok((n, 1)) => format!("Сохранено кадров: {n} в {}", dir.display()),
            Ok((n, b)) => format!("Сохранено кадров: {n} с холстов: {b} в {}", dir.display()),
            Err(e) => format!("Экспорт не удался: {e}"),
        };
    }

    /// Every UI piece of the scene (of every board, with «Все холсты») in each of its states,
    /// plus a Godot theme over the pictures.
    fn export_kit(&mut self) {
        let Some(dir) = rfd::FileDialog::new().set_title("Папка для UI-набора").pick_folder() else { return };
        let render = CpuOptions { scale: self.export.scale, antialias: self.export.antialias, background: false };
        let theme = self.file_stem();
        let result = self
            .export_boards()
            .and_then(|boards| {
                // Boards without pieces add nothing; with none at all the kit says what is missing.
                let with: Vec<&Document> = boards.iter().map(|(_, d)| d).filter(|d| motif_pixel::kit::has_pieces(d)).collect();
                let with = if with.is_empty() { boards.iter().map(|(_, d)| d).collect() } else { with };
                with.into_iter().map(|doc| motif_pixel::kit::render_kit(doc, render)).collect::<Result<Vec<_>, _>>()
            })
            .map(motif_pixel::kit::merge_boards)
            .and_then(|items| motif_pixel::kit::write_kit(&dir, &theme, &items).map(|files| (items.len(), files.len())));
        self.status = match result {
            Ok((pieces, files)) => format!("UI-набор: деталей {pieces}, файлов {files} в {} (тема {theme}.tres)", dir.display()),
            Err(e) => format!("UI-набор не сохранён: {e}"),
        };
    }

    /// Each board (or the open one) as a Godot scene of controls, over the kit theme of all of
    /// them.
    fn export_screens(&mut self) {
        let Some(dir) = rfd::FileDialog::new().set_title("Папка в проекте Godot").pick_folder() else { return };
        let render = CpuOptions { scale: self.export.scale, antialias: self.export.antialias, background: false };
        let theme = self.file_stem();
        let result = self.export_boards().and_then(|boards| {
            let n = boards.len();
            motif_pixel::screen::write_screens(&dir, &theme, &boards, render).map(|files| (n, files))
        });
        self.status = match result {
            Ok((n, files)) => {
                let scenes: Vec<String> = files
                    .iter()
                    .filter(|f| f.extension().is_some_and(|e| e == "tscn"))
                    .filter_map(|f| f.file_name().map(|s| s.to_string_lossy().into_owned()))
                    .collect();
                let shown = if n == 1 { scenes.join(", ") } else { format!("экранов {}", scenes.len()) };
                format!("Экран для Godot: {shown}, тема {theme}.tres, файлов {} в {}", files.len(), dir.display())
            }
            Err(e) => format!("Экран не сохранён: {e}"),
        };
    }

    /// Every animation of the scene (or of the selected object) for Godot: atlas, SpriteFrames,
    /// a scene with the sprite and, with a state machine, its AnimationTree. A saved scene stays
    /// linked to the folder and is exported there again on every save.
    fn export_godot(&mut self, ctx: &egui::Context) {
        let Some(dir) = rfd::FileDialog::new().set_title("Папка в проекте Godot").pick_folder() else { return };
        let only = self.selected.clone().filter(|id| self.scene.doc.node(id).is_some());
        // The open board by name, so a save exports the same board whichever one is open.
        let board = (self.board_count() > 1 && !self.export.all_boards).then(|| self.scene.doc.artboard.name.clone());
        let only = only.filter(|_| self.board_count() < 2 || board.is_some());
        let target = crate::pixabridge::GodotTarget { dir: dir.clone(), node: only, board };
        if let Some(path) = self.path.clone() {
            self.pixa.set_link(&path, Some(target.clone()));
        }
        self.status = self.godot_to(&target, ctx);
    }

    /// Exports into the target folder and returns the status line; with PixaTool it only starts
    /// the job.
    fn godot_to(&mut self, target: &crate::pixabridge::GodotTarget, ctx: &egui::Context) -> String {
        let render = CpuOptions { scale: self.export.scale, antialias: self.export.antialias, background: false };
        let dir = target.dir.as_path();
        // One board by its name, or all of them.
        let boards: Vec<(String, Document)> = if self.board_count() < 2 {
            vec![(self.file_stem(), self.scene.doc.clone())]
        } else {
            let mut out = Vec::new();
            for k in 0..self.board_count() {
                let Some(doc) = self.board_doc(k) else {
                    return format!("Экспорт в Godot не удался: в скрипте холста {} ошибки", k + 1);
                };
                if target.board.as_ref().is_none_or(|b| *b == doc.artboard.name) {
                    out.push((motif_script::board_file_name(&doc.artboard.name), doc));
                }
            }
            if out.is_empty() {
                return format!("Экспорт в Godot не удался: нет холста «{}»", target.board.as_deref().unwrap_or_default());
            }
            out
        };
        let only = target.node.clone().filter(|id| boards.len() == 1 && boards[0].1.node(id).is_some());
        if self.pixa.godot && self.pixa.exe.is_some() {
            if self.pixa.job.is_some() {
                return "Godot: PixaTool ещё занят прошлой выгрузкой".into();
            }
            let [(board, doc)] = boards.as_slice() else {
                return "Godot через PixaTool выгружает один холст за раз: выберите «Открытый холст»".into();
            };
            let name = only.clone().unwrap_or_else(|| board.clone());
            let opts = motif_pixel::godot::GodotExport { fps: self.export.fps, render, only, pivot: None };
            let machine = doc.machines.first().cloned();
            return match self.pixa.start_godot(doc.clone(), opts, dir.to_path_buf(), name, machine, ctx) {
                Ok(()) => "Godot: снимаю кадры для PixaTool…".into(),
                Err(e) => format!("Экспорт в Godot не удался: {e}"),
            };
        }
        let mut lines = Vec::new();
        let mut total = 0;
        for (board, doc) in &boards {
            let name = only.clone().unwrap_or_else(|| board.clone());
            let opts = motif_pixel::godot::GodotExport { fps: self.export.fps, render, only: only.clone(), pivot: None };
            let result = motif_pixel::godot::bake_sheet(doc, &opts)
                .and_then(|sheet| motif_pixel::godot::write_godot(dir, &name, &sheet, doc.machines.first(), !self.export.antialias).map(|f| (sheet, f.len())));
            match result {
                Ok((sheet, files)) => {
                    total += files;
                    let tree = doc.machines.first().map_or(String::new(), |m| format!(", машина «{}» в AnimationTree", m.name));
                    lines.push(format!("{name}.tscn, анимаций {}, кадров {} в атласе{tree}", sheet.anims.len(), sheet.cells.len()));
                }
                Err(e) => return format!("Экспорт в Godot не удался ({name}): {e}"),
            }
        }
        match lines.as_slice() {
            [one] => format!("Godot: {one} — {total} файлов в {}", dir.display()),
            _ => format!(
                "Godot: сцен {} ({}) — {total} файлов в {}",
                lines.len(),
                boards.iter().map(|b| b.0.as_str()).collect::<Vec<_>>().join(", "),
                dir.display()
            ),
        }
    }

    fn pixelize_frames(&mut self, ctx: &egui::Context) {
        let Some(dir) = rfd::FileDialog::new().set_title("Папка для кадров после PixaTool").pick_folder() else { return };
        let name = self.anim.clone().unwrap_or_else(|| "scene".to_string());
        let frames = self.export_render(&self.scene.doc, self.anim.as_deref());
        self.status = match frames.and_then(|f| self.pixa.start(&f, &name, &dir, ctx).map(|()| f.len())) {
            Ok(n) => format!("PixaTool: обрабатываю {n} кадров…"),
            Err(e) => format!("Пикселизация не удалась: {e}"),
        };
    }

    fn export_palette(&mut self) {
        let colors = match self.export_boards() {
            Ok(boards) => motif_pixel::boards_palette(boards.iter().map(|b| &b.1)),
            Err(e) => {
                self.status = format!("Палитра не сохранена: {e}");
                return;
            }
        };
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
        let split = rect.left() + rect.width() * 0.42;
        let left_rect = Rect::from_min_max(rect.min, pos2(split, rect.top() + 34.0));
        let mut left = ui.new_child(egui::UiBuilder::new().max_rect(left_rect));
        left.set_clip_rect(left_rect);
        left.horizontal_centered(|ui| {
            let file = ui.add(egui::Button::new(RichText::new("Файл").color(MUTED)).frame(false));
            egui::Popup::menu(&file).show(|ui| {
                ui.set_min_width(220.0);
                if ui
                    .add(egui::Button::new("Новое…").shortcut_text("Ctrl+N"))
                    .on_hover_text("Библиотека шаблонов: пустая сцена, UI-набор и экраны игры — меню, пауза, настройки, HUD, диалог, инвентарь, магазин, победа, поражение, выбор уровня, загрузка")
                    .clicked()
                {
                    self.library = Some(crate::library_ui::Library::new());
                }
                if ui.add(egui::Button::new("Новая сцена")).clicked() {
                    self.file_action(FileAction::New(NEW_SCENE));
                }
                if ui
                    .add(egui::Button::new("Новый UI-набор"))
                    .on_hover_text("Заготовка на семи холстах: кнопки, флажок, тумблер, радио-кнопки, поле ввода, выпадающий список, полосы, слайдер, рейтинг, загрузка, панели, вкладки и текст — для экспорта в Godot")
                    .clicked()
                {
                    self.file_action(FileAction::New(NEW_UI_KIT));
                }
                if ui.button("Восстановление документов…").clicked() {
                    self.recovery_records = self.recovery.entries();
                    self.recovery_title = "Восстановление документов".into();
                    self.show_recovery = true;
                }
                if ui.add_enabled(self.path.is_some(), egui::Button::new("Предыдущие сохранения…")).clicked() {
                    self.recovery_records = self.path.as_ref().map_or_else(Vec::new, |p| crate::storage::history(p));
                    self.recovery_title = "Предыдущие сохранения".into();
                    self.show_recovery = true;
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
                self.link_menu(ui);
                ui.separator();
                if ui.add(egui::Button::new("Импорт SVG…")).on_hover_text("Фигуры из SVG станут контурами в новой группе").clicked()
                {
                    self.pick_svg();
                }
                if ui.button("Импорт HTML + CSS…").clicked() { self.html_import.open = true; }
                if ui.add(egui::Button::new("Подложка PNG или PSD…")).on_hover_text("Картинка-референс под фигурами, только в редакторе. У PSD слои включаются по отдельности").clicked()
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
            let text = format!("{}{} · {}", name, if self.dirty() { " •" } else { "" }, self.scene.doc.artboard.name);
            ui.add(egui::Label::new(text).truncate()).on_hover_text(self.path.as_ref().map_or_else(|| "Новая сцена".into(), |p| p.display().to_string()));
        });
        let right = Rect::from_min_max(pos2(split + 8.0, rect.top()), pos2(rect.right(), rect.top() + 34.0));
        let mut rui = ui.new_child(egui::UiBuilder::new().max_rect(right).layout(egui::Layout::right_to_left(egui::Align::Center)));
        rui.set_clip_rect(right);
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
        if self.has_ui() || self.shown_machine().is_some_and(|m| !m.listeners.is_empty()) {
            let on = self.run.is_some() || self.check.is_some();
            let kind = if on { ButtonKind::On } else { ButtonKind::Normal };
            if theme::button(&mut rui, if on { "■ Проверка" } else { "▶ Проверить" }, kind)
                .on_hover_text(
                    "Проверка вживую: детали ui работают под мышью как в Godot (наведение, нажатие, флажки, слайдеры), машина состояний слушает мышь. Справа — список деталей и их значения",
                )
                .clicked()
            {
                self.toggle_check();
            }
        }
        let zoom = self.view.map_or(100.0, |v| v.zoom * 100.0);
        if theme::button(&mut rui, &format!("{zoom:.0}%"), ButtonKind::Normal).on_hover_text("Вписать холст (F); Shift — все холсты (Shift+F)").clicked()
        {
            if rui.input(|i| i.modifiers.shift) {
                self.boards.fit_all = true;
            } else {
                self.view = None;
            }
        }
        let lower = Rect::from_min_max(pos2(rect.left(), rect.top() + 38.0), rect.max);
        let mut lower_ui = ui.new_child(egui::UiBuilder::new().max_rect(lower));
        lower_ui.horizontal_wrapped(|ui| {
            use crate::workspace::{Panel, Workspace};
            let labels = Workspace::ALL.map(Workspace::name);
            if let Some(index) = theme::segmented(ui, &labels, self.workspace().index(), true) {
                self.set_workspace(Workspace::ALL[index]);
            }
            ui.separator();
            let profile = self.tool_profile();
            egui::ComboBox::from_id_salt("tool-profile")
                .width(70.0)
                .selected_text(match profile {
                    Purpose::Any => "Всё",
                    Purpose::Ui => "UI",
                    Purpose::Game => "Игра",
                })
                .show_ui(ui, |ui| {
                    ui.label("Профиль инструментов");
                    for (value, label) in [(Purpose::Any, "Все инструменты"), (Purpose::Ui, "Интерфейсы UI"), (Purpose::Game, "Игровая графика")]
                    {
                        if ui.selectable_label(profile == value, label).clicked() {
                            self.set_tool_profile(value);
                        }
                    }
                    if ui.button("По документу").clicked() {
                        self.profile = None;
                    }
                })
                .response
                .on_hover_text("Фильтр инструментов. Не меняет назначение сцены в файле");
            let response = ui.button("Панели");
            egui::Popup::menu(&response).show(|ui| {
                for panel in Panel::ALL {
                    let mut shown = self.panels.layout().shown(panel);
                    if ui.checkbox(&mut shown, panel.name()).changed() {
                        self.panels.set(panel, shown);
                    }
                }
                ui.separator();
                if ui.button("Сбросить раскладку пространства").clicked() {
                    self.panels.reset();
                }
            });
            if theme::button(
                ui,
                if self.panels.focus.is_some() { "Вернуть панели" } else { "Фокус" },
                if self.panels.focus.is_some() { ButtonKind::On } else { ButtonKind::Normal },
            )
            .on_hover_text("Временно скрыть панели / вернуть раскладку (Ctrl+Shift+Space)")
            .clicked()
            {
                self.panels.toggle_focus();
            }
            if !self.panels.layout().timeline {
                if self.anim.is_some() {
                    if theme::icon_button(ui, if self.playing { Icon::Pause } else { Icon::Play }, "Пуск / пауза").clicked() {
                        self.toggle_playback();
                    }
                    if ui.add(egui::DragValue::new(&mut self.time).speed(1.0 / 60.0).range(0.0..=600.0).suffix(" с").max_decimals(2)).changed() {
                        self.playing = false;
                        self.draft_pose.clear();
                    }
                } else if ui.button("Создать клип").clicked() {
                    self.create_animation();
                }
            }
        });
    }

    // Panel geometry is user state, never document history. Each workspace has its own sizes.
    fn panel_seed(&self, ui: &egui::Ui, panel: crate::workspace::Panel, size: f32) -> (u8, usize, bool) {
        let salt = (panel as u8, self.workspace().index(), self.panels.focus.is_some());
        let id = ui.scope_id().with(salt);
        if !ui.ctx().read_response(id.with("__resize")).is_some_and(|r| r.dragged() || r.drag_stopped()) {
            let rect = Rect::from_min_size(pos2(0.0, 0.0), if panel == crate::workspace::Panel::Timeline { vec2(800.0, size) } else { vec2(size, 600.0) });
            ui.ctx().data_mut(|data| data.insert_persisted(id, egui::containers::panel::PanelState { outer_rect: rect }));
        }
        salt
    }

    fn panel_resized(&mut self, ui: &egui::Ui, panel: crate::workspace::Panel, salt: (u8, usize, bool), rect: Rect) {
        let id = ui.scope_id().with(salt).with("__resize");
        if ui.ctx().read_response(id).is_some_and(|r| r.dragged() || r.drag_stopped()) {
            let layout = self.panels.layout_mut();
            match panel {
                crate::workspace::Panel::Layers => layout.layers_width = rect.width(),
                crate::workspace::Panel::Script => layout.script_width = rect.width(),
                crate::workspace::Panel::Inspector => layout.inspector_width = rect.width(),
                crate::workspace::Panel::Timeline => layout.timeline_height = rect.height(),
                crate::workspace::Panel::Preview => layout.preview_width = rect.width(),
                crate::workspace::Panel::Tools => {}
            }
        }
    }

    fn panel_header(&mut self, ui: &mut egui::Ui, panel: crate::workspace::Panel) {
        ui.horizontal(|ui| {
            if ui.small_button("−").on_hover_text("Свернуть панель").clicked() {
                self.panels.set(panel, false);
            }
            ui.small(panel.name());
        });
    }

    fn collapsed_panel(&mut self, ui: &mut egui::Ui, panel: crate::workspace::Panel) {
        let rect = ui.max_rect();
        let response = ui.interact(rect, ui.scope_id().with("open-panel"), Sense::click()).on_hover_text(format!("Развернуть: {}", panel.name()));
        if response.hovered() {
            ui.painter().rect_filled(rect, 0.0, theme::PANEL_2);
        }
        for (i, ch) in panel.name().to_uppercase().chars().enumerate() {
            ui.painter().text(pos2(rect.center().x, rect.top() + 22.0 + i as f32 * 15.0), Align2::CENTER_CENTER, ch, theme::semibold(11.0), MUTED);
        }
        if response.clicked() {
            self.panels.set(panel, true);
        }
    }

    fn workspace_panels(&mut self, ui: &mut egui::Ui) {
        use crate::workspace::Panel;
        let layout = self.panels.layout().fit_prioritizing(ui.available_width(), ui.available_height(), self.panels.priority);
        let strips = self.panels.focus.is_none();
        let frame = egui::Frame::new().fill(PANEL);
        if layout.timeline {
            let salt = self.panel_seed(ui, Panel::Timeline, layout.timeline_height);
            let max = (ui.available_height() - 220.0).clamp(120.0, 520.0);
            let response = egui::Panel::bottom(salt).resizable(true).default_size(layout.timeline_height).size_range(120.0..=max).frame(frame).show(ui, |ui| {
                self.panel_header(ui, Panel::Timeline);
                self.timeline(ui);
            });
            self.panel_resized(ui, Panel::Timeline, salt, response.response.rect);
        } else if strips {
            egui::Panel::bottom("timeline-closed").exact_size(24.0).frame(frame).show(ui, |ui| {
                if ui.small_button("Развернуть таймлайн / машину состояний").clicked() {
                    self.panels.set(Panel::Timeline, true);
                }
            });
        }
        self.preview_panels(ui, layout, strips);
        if layout.tools {
            egui::Panel::left("tools").exact_size(48.0).frame(frame).show(ui, |ui| self.tool_rail(ui));
        }
        if layout.layers {
            let salt = self.panel_seed(ui, Panel::Layers, layout.layers_width);
            let reserved = 320.0
                + if layout.script {
                    layout.script_width
                } else if strips {
                    28.0
                } else {
                    0.0
                };
            let max = (ui.available_width() - reserved).clamp(160.0, 360.0);
            let response = egui::Panel::left(salt).resizable(true).default_size(layout.layers_width).size_range(160.0..=max).frame(frame).show(ui, |ui| {
                self.panel_header(ui, Panel::Layers);
                self.layers_panel(ui);
            });
            self.panel_resized(ui, Panel::Layers, salt, response.response.rect);
        } else if strips {
            egui::Panel::left("layers-closed").exact_size(28.0).frame(frame).show(ui, |ui| self.collapsed_panel(ui, Panel::Layers));
        }
        if layout.script {
            let salt = self.panel_seed(ui, Panel::Script, layout.script_width);
            let max = (ui.available_width() - 320.0).clamp(240.0, 900.0);
            let response = egui::Panel::left(salt)
                .resizable(true)
                .default_size(layout.script_width)
                .size_range(240.0..=max)
                .frame(frame)
                .show(ui, |ui| self.script_panel(ui));
            self.panel_resized(ui, Panel::Script, salt, response.response.rect);
        } else if strips {
            egui::Panel::left("script-strip").exact_size(28.0).frame(frame).show(ui, |ui| self.script_strip(ui));
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
            if self.workspace() != crate::workspace::Workspace::Animate {
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
                if self.tool_profile() != Purpose::Ui && theme::tool_button(ui, Icon::Bone, self.bone_tool(), "B", tip).clicked() {
                    self.tool = Tool::Create(ShapeKind::Bone);
                }
                let tip = "Перо: клик — угол, протяжка — плавная точка, клик по первой точке замыкает";
                if theme::tool_button(ui, Icon::Pen, self.tool == Tool::Pen, "P", tip).clicked() {
                    self.tool = Tool::Pen;
                }
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
            self.panels.set(crate::workspace::Panel::Script, false);
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
        egui::ScrollArea::vertical().id_salt("props").auto_shrink([false, false]).show(ui, |ui| self.props_contents(ui, None));
    }

    fn props_contents(&mut self, ui: &mut egui::Ui, category: Option<&canvas_menu::InspectorCategory>) {
        use canvas_menu::InspectorCategory as Cat;
        if self.check.is_some() {
            self.check_panel(ui);
            return;
        }
        let show = |part| category.is_none_or(|c| *c == part);
        if category.is_none() {
            self.parameter_work_panel(ui);
            self.parameter_panel(ui);
        }
        if let Some(Cat::Parameter(parameter)) = category {
            self.parameter_category_panel(ui, Some(parameter));
        }
        if show(Cat::Deformer) {
            self.deformer_panel(ui);
        }
        if show(Cat::Character) && self.workspace() == crate::workspace::Workspace::Drawing {
            self.character_panel(ui);
        }
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
        if category.is_none() {
            section(ui, |ui| {
                ui.horizontal(|ui| {
                    let (r, _) = ui.allocate_exact_size(vec2(16.0, 18.0), Sense::hover());
                    theme::paint_icon(ui.painter(), r, theme::shape_icon(node.kind), MUTED);
                    ui.label(RichText::new(node.display_name()).font(theme::semibold(14.0)).color(FG));
                });
                ui.label(RichText::new(format!("{} · id {}", kind_name(node.kind), node.id)).size(11.5).color(FAINT));
                self.export_name_row(ui, &node.id);
            });
        }

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
        if show(Cat::Transform) {
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
        }
        let mut pick_font = false;
        let mut outline_text = false;
        if node.kind == ShapeKind::Text {
            let font = self.scene.prop_spans.get(&(id.clone(), "font".to_string())).map(|s| {
                let t = self.src[s.start..s.end].trim_start_matches("font").trim();
                t.trim_matches('"').to_string()
            });
            if show(Cat::Text) {
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
            }
            if show(Cat::Letters) {
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
        }
        if matches!(node.kind, ShapeKind::Rect | ShapeKind::Star | ShapeKind::Polygon) && show(Cat::Shape) {
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
        if node.kind == ShapeKind::Path && show(Cat::Contour) {
            fold(ui, "Контур", |ui| self.point_controls(ui, &node));
        }
        if show(Cat::Mask) {
            self.mask_ui(ui, &node);
        }
        if show(Cat::Symbol) {
            self.symbol_section(ui, &node);
        }
        // Interface work needs no skeletons or physics; game art needs no kit.
        let game = category.is_some() || self.tool_profile() != Purpose::Ui;
        let interface = category.is_some() || self.tool_profile() != Purpose::Game;
        if game {
            if show(Cat::Bones) {
                self.bone_section(ui, &node);
            }
            if show(Cat::Weights) {
                self.weights_section(ui, &node);
            }
            if show(Cat::Physics) {
                self.physics_section(ui, &node);
            }
            if show(Cat::Game) {
                self.game_section(ui, &node);
            }
        }
        if !node.kind.is_container() {
            if show(Cat::Fill) {
                fold(ui, "Заливка", |ui| {
                    if self.fill_kind_ui(ui, &node) {
                        ui.add_space(6.0);
                        self.paint_row(ui, &id, Prop::Fill, node.fill, &mut changes, &mut diamonds);
                    }
                });
            }
            if show(Cat::Stroke) {
                fold(ui, "Обводка", |ui| {
                    if self.stroke_kind_ui(ui, &node) {
                        ui.add_space(6.0);
                        self.paint_row(ui, &id, Prop::StrokeColor, node.stroke, &mut changes, &mut diamonds);
                    }
                    if node.stroke.is_some() || node.stroke_gradient.is_some() {
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
        }
        // Blur, height and shadow: keyable, so they animate like position or colour.
        let mut fx_changes = Vec::new();
        let mut fx_diamonds = Vec::new();
        if node.kind != ShapeKind::Bone && show(Cat::Effects) {
            fold(ui, "Эффекты", |ui| {
                ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
                let mut fx = |ui: &mut egui::Ui, label: &str, prop: Prop| {
                    let v = node.get(prop).as_num().unwrap_or(0.0);
                    let (range, speed, step) = match prop {
                        Prop::ShadowX | Prop::ShadowY | Prop::InsetX | Prop::InsetY => (-1000.0..=1000.0, 0.2, 10.0),
                        Prop::Noise => (0.0..=1.0, 0.005, 100.0),
                        _ => (0.0..=500.0, 0.2, 10.0),
                    };
                    let out = theme::num_field(ui, half, label, v, speed, range, if step > 10.0 { 2 } else { 1 }, self.key_state(&id, prop));
                    if let Some(nv) = out.changed {
                        fx_changes.push((prop, Value::Num((nv * step).round() / step)));
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
                if crate::style_ui::glow_rows(ui, &node, &mut styles) {
                    ui.horizontal(|ui| fx(ui, "Размер", Prop::Glow));
                }
                if crate::style_ui::inset_rows(ui, &node, &mut styles) {
                    ui.horizontal(|ui| {
                        fx(ui, "Сдвиг X", Prop::InsetX);
                        fx(ui, "Сдвиг Y", Prop::InsetY);
                    });
                    ui.horizontal(|ui| fx(ui, "Мягкость ", Prop::InsetBlur));
                }
                if crate::style_ui::noise_rows(ui, &node, &mut styles) {
                    ui.horizontal(|ui| fx(ui, "Сила", Prop::Noise));
                }
                ui.label(
                        RichText::new(
                            "«Высота» поднимает объект над артбордом: тень уходит от света сцены, длиннее и мягче. Размытие 0 — жёсткая тень для пиксель-арта. Свечение — ореол вокруг, внутренняя тень вдавливает фигуру, шум — зерно поверх заливки. Размер свечения и сила шума анимируются: Hover может зажечь кнопку.",
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
            if show(Cat::Orbit) {
                self.orbit_section(ui, &node, half, &mut changes, &mut diamonds, &mut styles);
            }
            if show(Cat::Body) {
                self.body_section(ui, &node, half, &mut changes, &mut diamonds, &mut styles);
            }
        }
        if show(Cat::Morph) {
            self.morph_section(ui, &node, &mut changes, &mut diamonds, &mut styles);
        }
        if game && show(Cat::Brush) {
            self.brush_section(ui, &node, half, &mut changes, &mut diamonds, &mut styles);
        }
        let mut kit = crate::kit_ui::KitAction::default();
        if interface {
            if show(Cat::Layout) {
                self.layout_section(ui, &node, half, &mut styles);
            }
            if show(Cat::Interface) {
                kit = self.kit_section(ui, &node, half, &mut styles);
            }
        }
        if category.is_none() {
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
        }
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
        if kit.add_states {
            self.add_ui_states();
        }
        if kit.add_value {
            self.add_value_anim();
        }
        if kit.stretch {
            self.stretch = Some(crate::stretch_ui::Stretch::new(&id));
        }
        if kit.crisp {
            self.align_to_pixels(&id);
        }
    }

    fn artboard_info(&mut self, ui: &mut egui::Ui) {
        self.board_section(ui);
        section(ui, |ui| {
            ui.strong("Назначение артборда в файле");
            ui.add_enabled_ui(self.workspace() != crate::workspace::Workspace::Animate, |ui| {
                let values = [Purpose::Any, Purpose::Ui, Purpose::Game];
                let selected = values.iter().position(|p| *p == self.scene.doc.purpose).unwrap_or(0);
                if let Some(i) = theme::segmented(ui, &["Всё", "UI", "Игра"], selected, false) {
                    self.set_document_purpose(values[i]);
                }
            });
            ui.small("Сохраняется в сцене. Профиль сверху только фильтрует инструменты.");
        });
        section(ui, |ui| {
            ui.label(RichText::new("Выберите объект на холсте, в слоях или кликом по его строке в скрипте.").size(12.0).color(FAINT));
            ui.label(RichText::new("Новые фигуры: R, O, S, N — тяните по холсту; P — перо.").size(12.0).color(FAINT));
            ui.label(RichText::new("Перетащите на окно SVG — он станет контурами, PNG или PSD — подложкой.").size(12.0).color(FAINT));
        });
        self.palette_section(ui);
        if self.tool_profile() == Purpose::Ui {
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
        self.sides_section(ui);
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
        self.palette_strip(ui, id, prop);
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
            let mode = match self.key_time() {
                Some((a, _)) => format!("Animate · {a}"),
                None => "Design".into(),
            };
            ui.label(small(mode, FAINT));
            ui.label(small(format!("история: {}", self.undo.len()), FAINT));
            if self.video.busy() {
                self.video.progress_ui(ui, false);
            } else {
                ui.label(small(self.status.clone(), MUTED));
                if let Some(e) = self.video.reveal_ui(ui, Some(&self.status)) {
                    self.status = e;
                }
            }
        });
    }

    // ---------- canvas ----------

    fn canvas_ui(&mut self, ui: &mut egui::Ui, frame: Option<&eframe::Frame>) {
        if self.tool != Tool::Pen && !self.pen.is_empty() {
            self.finish_pen(false);
        }
        if let Some(e) = &self.point_edit
            && (self.selected.as_deref() != Some(e.node.as_str()) || self.tool != Tool::Select)
        {
            self.point_edit = None;
        }
        let rect = ui.available_rect_before_wrap();
        self.canvas_bounds = rect;
        let resp = ui.allocate_rect(rect, Sense::click_and_drag());
        self.pause_for_canvas_press(ui.ctx());
        // The boards of the file on their grid: when the active one moves on it, the view
        // follows, so the boards stay where they were on screen.
        let grid = self.board_grid();
        let origin = grid.origins[self.boards.active.min(grid.origins.len() - 1)];
        if let (Some(old), Some(v)) = (self.boards.origin, self.view.as_mut()) {
            v.pan += vec2((origin.x - old.x) as f32, (origin.y - old.y) as f32) * v.zoom;
        }
        self.boards.origin = Some(origin);
        let others = self.other_boards(&grid);
        if std::mem::take(&mut self.boards.fit_all) {
            self.view = Some(View::fit_rect(rect, self.boards_bounds(&grid)));
        }
        if std::mem::take(&mut self.boards.reveal)
            && let Some(v) = self.view.as_mut()
        {
            // A board opened from the panel or just made: centred when it is off screen,
            // fitted when it is too big for the canvas at this zoom.
            let ab = &self.scene.doc.artboard;
            let size = vec2(ab.width as f32, ab.height as f32) * v.zoom;
            let shown = Rect::from_min_size(rect.min + v.pan, size);
            if !rect.shrink(16.0).contains_rect(shown) {
                *v = if size.x < rect.width() - 96.0 && size.y < rect.height() - 96.0 {
                    View { zoom: v.zoom, pan: (rect.size() - size) / 2.0 }
                } else {
                    View::fit(rect, &self.scene.doc)
                };
            }
        }
        let view = *self.view.get_or_insert_with(|| View::fit(rect, &self.scene.doc));
        let doc = self.display_doc();
        let ghosts = self.onion_doc();
        let backdrop = crate::importer::backdrop_view(&self.scene, &self.backdrop_img);
        let worn: Vec<(&Document, motif_core::kurbo::Affine)> = self.tryon.iter().filter_map(|t| Some((&t.doc, t.placed(&doc)?))).collect();
        let stage = crate::canvas::Stage { doc: ghosts.as_ref().unwrap_or(&doc), backdrop, others: &others, worn: &worn };
        self.canvas.paint(ui, frame, rect, view, stage);
        if self.pixel.on {
            self.paint_pixel_preview(ui, rect, view, &doc);
        }
        let board = artboard_rect(rect, view, &doc);
        let painter = ui.painter_at(rect);
        let mut boards = vec![board];
        boards.extend(others.iter().map(|o| {
            let min = view.to_screen(rect, Point::new(o.offset.x, o.offset.y));
            Rect::from_min_size(min, vec2(o.doc.artboard.width as f32, o.doc.artboard.height as f32) * view.zoom)
        }));
        paint_dots(&painter, rect, &boards, view);
        if self.snap.grid {
            crate::guides::paint_grid(&painter, rect, view, &doc, self.snap.grid_size);
        }
        if self.snap.pixels {
            crate::guides::paint_grid(&painter, rect, view, &doc, 1.0 / self.export.scale.max(0.01));
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
        let live = !pan_gesture && self.drag.is_none() && (self.check.is_some() || self.running().is_some_and(|(m, _)| !m.listeners.is_empty()));
        if live {
            self.hovered = None;
            self.live_pointer(ui, &resp, &doc, doc_pt);
            self.check_pointer(ui, &resp, &doc, doc_pt);
        }
        // The boards' names and size handle, and clicks on other boards.
        let board_ui = !pan_gesture && !live && self.eyedrop.is_none() && self.boards_input(ui, &resp, rect, view, &grid);
        // The pen and point editing take the pointer themselves.
        let deforming = !pan_gesture && !live && self.deformer_input(ui, &resp, rect, view, &doc);
        let special = deforming
            || live
            || board_ui
            || (!pan_gesture
                && self.drag.is_none()
                && if self.eyedrop.is_some() {
                    self.eyedrop_input(ui, &resp, &doc, doc_pt);
                    true
                } else if self.tool == Tool::Pen {
                    self.pen_input(ui, &resp, rect, view);
                    true
                } else if self.brush_w.on.is_some() && self.weight_paint_input(ui, &resp, rect, view, &doc) {
                    true
                } else if self.point_edit.is_some() {
                    self.point_edit_input(ui, &resp, rect, view, &doc)
                } else {
                    false
                });
        // A key handle or the line of a motion path under the pointer.
        let path_hit = pointer
            .filter(|&p| !special && !pan_gesture && self.drag.is_none() && resp.hovered() && hit_handle(p).is_none())
            .filter(|&p| !self.pointer_hits_selection(&doc, view.to_doc(rect, p)))
            .and_then(|p| self.motion_hit(rect, view, p));
        if path_hit.is_some() {
            self.hovered = None;
        }
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
                    None if path_hit.as_ref().is_some_and(|h| h.key) => egui::CursorIcon::Grab,
                    None if path_hit.is_some() => egui::CursorIcon::PointingHand,
                    None if self.hovered.is_some() => egui::CursorIcon::Move,
                    None => egui::CursorIcon::Default,
                }
            };
            ui.ctx().set_cursor_icon(cursor);
        }

        // Drags start where the button went down, not where the movement threshold was passed.
        let press = ui.input(|i| i.pointer.press_origin()).or(pointer);
        if !special
            && (resp.drag_started_by(egui::PointerButton::Primary) || (pan_gesture && resp.drag_started_by(egui::PointerButton::Middle)))
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
            } else if !shift
                && !self.pointer_hits_selection(&doc, dp)
                && let Some(hit) = self.motion_hit(rect, view, p)
            {
                Some(self.motion_drag(hit, dp))
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
        if !live && !pan_gesture {
            self.canvas_context_menu(&resp, &doc, rect, view);
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
            } else if !shift && let Some(hit) = &path_hit {
                self.motion_seek(hit);
            } else if doc_pt.and_then(|p| handles.as_ref().and_then(|_| hit_handle(view.to_screen(rect, p)))).is_none() {
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
        self.paint_bones(&painter, rect, view, &doc);
        self.paint_motion(&painter, rect, view, pointer.filter(|_| self.drag.is_none() && resp.hovered()));
        self.paint_fields(&painter, rect, view, &doc);
        self.paint_marks(&painter, rect, view, &doc);
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
        if self.key_time().is_some() {
            painter.rect_stroke(rect, 0.0, Stroke::new(2.0, ACCENT), StrokeKind::Inside);
            let badge = painter.layout_no_wrap("ANIMATE · АВТОКЛЮЧИ".into(), theme::semibold(11.0), ACCENT_INK);
            let b = Rect::from_min_size(rect.min + vec2(12.0 + ruler, 10.0 + ruler), galley_size(&badge) + vec2(16.0, 6.0));
            painter.rect_filled(b, 5.0, ACCENT);
            painter.galley(b.min + vec2(8.0, 3.0), badge, ACCENT_INK);
        }
        painter.text(
            rect.right_bottom() - vec2(12.0, 10.0),
            Align2::RIGHT_BOTTOM,
            "Колесо — масштаб · пробел + мышь — панорама · F, Shift+F — вписать холст, все",
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
            .vscroll(true)
            .max_height((ctx.content_rect().height() - 96.0).max(240.0))
            .default_width(380.0)
            .anchor(Align2::RIGHT_TOP, vec2(-280.0, 56.0))
            .show(ctx, |ui| {
                self.export_boards_choice(ui);
                self.web_export(ui);
                // An interface scene is exported as a kit first of all; game art has no kit.
                let purpose = self.tool_profile();
                if purpose == Purpose::Ui {
                    self.kit_export(ui);
                } else {
                    self.godot_export(ui);
                }
                self.runtime_export(ui);
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
                let what = match self.anim.as_deref() {
                    _ if self.board_count() > 1 && self.export.all_boards => format!("холстов: {}", self.board_count()),
                    Some(a) => format!("анимация «{a}»"),
                    None => "кадр сцены".to_string(),
                };
                if theme::button(ui, &format!("Сохранить кадры… ({what})"), ButtonKind::Primary).clicked() {
                    self.export_frames();
                }
                ui.add_space(6.0);
                theme::rule(ui);
                ui.add_space(4.0);
                self.video_section(ui);
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

    /// Like Illustrator's «Use Artboards»: the open board or all of them, for every export below.
    fn export_boards_choice(&mut self, ui: &mut egui::Ui) {
        let n = self.board_count();
        if n < 2 {
            return;
        }
        theme::caption(ui, "Холсты");
        ui.horizontal(|ui| {
            let open = format!("Открытый «{}»", self.scene.doc.artboard.name);
            for (all, label) in [(false, open), (true, format!("Все ({n})"))] {
                let kind = if self.export.all_boards == all { ButtonKind::On } else { ButtonKind::Normal };
                if theme::button(ui, &label, kind).clicked() {
                    self.export.all_boards = all;
                }
            }
        });
        let hint = if self.export.all_boards {
            "Кадры — ХОЛСТ_0000.png на каждый холст, видео — ХОЛСТ_Анимация.mp4 с каждого холста, где она есть, UI-набор — детали всех холстов в одной теме, Godot — сцена ХОЛСТ.tscn на каждый холст, палитра — цвета всех холстов."
        } else {
            "Всё ниже сохраняется только с открытого холста. Другой холст открывается кликом по нему."
        };
        ui.label(RichText::new(hint).size(12.0).color(FAINT));
        ui.add_space(6.0);
        theme::rule(ui);
        ui.add_space(4.0);
    }

    fn godot_export(&mut self, ui: &mut egui::Ui) {
        theme::caption(ui, "Анимации для Godot");
        let all = self.board_count() > 1 && self.export.all_boards;
        let what = match self.selected.as_deref().and_then(|id| self.scene.doc.node(id)) {
            _ if all => format!("холстов: {}", self.board_count()),
            Some(n) => format!("«{}»", n.display_name()),
            None if self.board_count() > 1 => format!("холст «{}»", self.scene.doc.artboard.name),
            None => "вся сцена".to_string(),
        };
        let busy = self.pixa.job.is_some();
        let r = ui.add_enabled_ui(!busy, |ui| theme::button(ui, &format!("Выгрузить в Godot… ({what})"), ButtonKind::Primary)).inner;
        if let Some(d) = &self.scene.doc.directions {
            let drawn: Vec<&str> = d.drawn().into_iter().map(|k| d.names()[k]).collect();
            let mirror = if d.mirror { ", левые — отражением правых" } else { "" };
            ui.label(
                RichText::new(format!("Стороны: {}{mirror}. Каждая анимация снимается как ИМЯ_{}…", d.count, drawn.join(", ИМЯ_"))).size(12.0).color(FAINT),
            );
        }
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
            let t = &link.target;
            let what = match (&t.node, &t.board) {
                (Some(id), _) => format!("«{}»", self.scene.doc.node(id).map_or(id.clone(), |n| n.display_name().to_string())),
                (None, Some(b)) => format!("холст «{b}»"),
                (None, None) if self.board_count() > 1 => "все холсты".to_string(),
                (None, None) => "вся сцена".to_string(),
            };
            let mut unlink = false;
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new(format!("Обновляется при каждом сохранении: {what} → {}", crate::pixabridge::short(&t.dir))).size(12.0))
                    .on_hover_text(t.dir.display().to_string());
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
        if theme::button(ui, "Сохранить UI-набор…", if self.tool_profile() == Purpose::Ui { ButtonKind::Primary } else { ButtonKind::Normal })
            .on_hover_text("Каждая деталь с ui или slice — отдельные PNG по состояниям и тема .tres со StyleBoxTexture")
            .clicked()
        {
            self.export_kit();
        }
        if theme::button(ui, "Экран для Godot…", ButtonKind::Normal)
            .on_hover_text("Холст — сцена .tscn из Control-узлов: детали ui — кнопки, полосы, флажки из темы набора, стеки — VBox/HBox, привязки — якоря, тексты — Label, остальное — картинки")
            .clicked()
        {
            self.export_screens();
        }
        ui.label(
            RichText::new("Масштаб и сглаживание — те же, что для кадров. Тему подключают в Godot как theme, детали — через Theme Type Variation. Экран кладёт тему рядом и сам её подключает: меню или HUD открывается в Godot готовой сценой.")
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
                    ("Tab", "Переключить Рисование / Анимацию без изменения сцены"),
                    ("Ctrl+Shift+Space", "Фокус на холсте / вернуть панели"),
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
                    ("F, Shift+F", "Вписать холст; все холсты"),
                    ("G", "Сетка"),
                    ("Alt при сдвиге", "Без умных направляющих"),
                    ("Ctrl+S, Ctrl+O", "Сохранить, открыть"),
                    ("Ctrl+N", "Новое: пустая сцена, UI-набор или шаблон экрана игры"),
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
        // The library window takes the keys: Enter creates, Escape closes.
        if self.library.is_some() {
            return;
        }
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
        if cmd(Key::N) && self.library.is_none() {
            self.library = Some(crate::library_ui::Library::new());
        }
        if cmd(Key::E) && !typing {
            self.panels.toggle(crate::workspace::Panel::Script);
        }
        if i.key_pressed(Key::F1) {
            self.show_syntax = !self.show_syntax;
        }
        // Keep global file commands, but let floating controls own Esc and property shortcuts.
        if typing || self.floating.is_some() || self.html_import.open {
            return;
        }
        if i.modifiers.command && i.modifiers.shift && i.key_pressed(Key::Space) {
            if self.html_import.open
                || self.pending_file.is_some()
                || self.show_export
                || self.show_help
                || self.show_recovery
                || self.show_syntax
                || self.stretch.is_some()
            {
                return;
            }
            self.panels.toggle_focus();
            self.space_used_for_pan = true;
            return;
        }
        if self.preview_canvas.contains_pointer(ctx) {
            if i.key_released(Key::Space)
                && !self.space_used_for_pan
                && !i.modifiers.command
                && !self.preview.follow
                && !self.preview.stale
                && matches!(self.preview.mode, crate::preview::Mode::Clip | crate::preview::Mode::Machine)
            {
                self.preview.playing = !self.preview.playing;
            }
            if i.key_released(Key::Space) {
                self.space_used_for_pan = false;
            }
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
                egui::Event::Cut if self.copy_selection(ctx) => {
                    self.delete_selected_nodes();
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
            } else if !self.space_used_for_pan && self.anim.is_some() && !i.modifiers.command && !i.modifiers.shift {
                self.toggle_playback();
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
                if self.workspace() == crate::workspace::Workspace::Animate && matches!(tool, Tool::Create(_) | Tool::Pen) {
                    self.status = "Для создания фигур выберите «Рисование». Поза и клип сохранятся".into();
                } else {
                    self.tool = tool;
                }
            }
        }
        if plain(Key::Tab) {
            let on = self.workspace() != crate::workspace::Workspace::Animate;
            self.set_mode_animate(on);
        }
        if plain(Key::G) {
            self.snap.grid = !self.snap.grid;
        }
        if plain(Key::F) || (i.modifiers.shift && i.key_pressed(Key::Num1)) {
            self.view = None;
        }
        if i.modifiers.shift && !i.modifiers.command && i.key_pressed(Key::F) {
            self.boards.fit_all = true;
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
            if self.deformer.grab.is_some() {
                self.deformer.grab = None;
                self.live.clear();
            } else if self.deformer.target.is_some() {
                self.deformer.target = None;
                self.deformer.form = None;
                self.deformer.corrective = None;
                self.pose_preview.clear();
            } else if !self.pen.is_empty() {
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
            if plain(Key::K) {
                self.record_transform();
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

fn paint_dots(painter: &egui::Painter, rect: Rect, boards: &[Rect], view: View) {
    let step = 16.0;
    let origin = rect.min + view.pan;
    let start_x = origin.x - ((origin.x - rect.left()) / step).floor() * step;
    let start_y = origin.y - ((origin.y - rect.top()) / step).floor() * step;
    let mut y = start_y;
    while y < rect.bottom() {
        let mut x = start_x;
        while x < rect.right() {
            let p = pos2(x, y);
            if !boards.iter().any(|b| b.expand(2.0).contains(p)) {
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
    if ui.stack().iter().any(|frame| frame.tags().contains("motif-property-category")) {
        section(ui, add);
        return;
    }
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
pub(crate) enum FileAction {
    New(&'static str),
    Open,
    Close,
    Restore(usize),
}

fn updater_path() -> Option<PathBuf> {
    let p = std::env::current_exe().ok()?.parent()?.join("update.bat");
    p.exists().then_some(p)
}

pub(crate) fn mtime(p: &std::path::Path) -> Option<SystemTime> {
    std::fs::metadata(p).and_then(|m| m.modified()).ok()
}

impl eframe::App for App {
    fn on_exit(&mut self) {
        self.persist_workspace();
    }

    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let dt = ctx.input(|i| i.stable_dt);
        self.frame_ms = self.frame_ms * 0.9 + dt * 1000.0 * 0.1;
        if ctx.input(|i| i.viewport().close_requested()) && !self.close_allowed {
            if self.dirty() {
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                self.pending_file = Some(FileAction::Close);
            } else {
                self.recovery.clear();
            }
        }
        self.autosave();
        self.poll_disk();
        self.dropped_files(&ctx);
        self.refresh_backdrop();
        if let Some(status) = self.pixa.poll() {
            self.status = status;
        }
        if let Some(status) = self.video.poll() {
            self.status = status;
        }
        self.poll_link();
        if !ctx.input(|i| i.pointer.any_down()) {
            self.persist_workspace();
        }
        if !self.draft_pose.matches(&self.key_time()) {
            self.draft_pose.clear();
        }
        self.shortcuts(&ctx);
        self.pause_for_canvas_press(&ctx);
        if let Some(c) = &mut self.check {
            c.advance(&self.scene.doc, dt.min(0.1) as f64);
        }
        if self.run.is_some() || self.check.is_some() {
            if self.run.is_some() {
                self.advance_run(dt.min(0.1));
            }
            ctx.request_repaint();
        } else if self.playing {
            self.time += dt.min(0.1) as f64;
            ctx.request_repaint();
        } else if self.path.is_some() || self.dirty() {
            // Keep polling the file on disk for external edits.
            ctx.request_repaint_after(std::time::Duration::from_millis(500));
        }
        let title = format!("Motif — {}{}", self.path.as_ref().map_or("без имени".into(), |p| p.display().to_string()), if self.dirty() { " *" } else { "" });
        ctx.send_viewport_cmd(egui::ViewportCommand::Title(title));

        let panel = egui::Frame::new().fill(PANEL);
        egui::Panel::top("top")
            .exact_size(if ui.available_width() < 900.0 { 110.0 } else { 80.0 })
            .frame(panel.inner_margin(egui::Margin::symmetric(8, 0)))
            .show(ui, |ui| self.top_bar(ui));
        egui::Panel::bottom("status").exact_size(26.0).frame(panel.inner_margin(egui::Margin::symmetric(12, 0))).show(ui, |ui| self.status_bar(ui));
        self.workspace_panels(ui);
        egui::CentralPanel::no_frame().show(ui, |ui| self.canvas_ui(ui, Some(frame)));
        self.export_window(&ctx);
        self.web_window(&ctx);
        self.help_window(&ctx);
        self.unsaved_window(&ctx);
        self.recovery_window(&ctx);
        self.syntax_window(&ctx);
        self.stretch_window(&ctx);
        self.library_window(&ctx);
        self.html_import_window(&ctx);
        self.floating_inspector(&ctx);
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

#[cfg(test)]
pub(crate) mod editing_tests {
    use super::*;
    use crate::compound::{Anchor, Compound};
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn saving_gives_hidden_ids_and_a_rename_keeps_them() {
        let dir = std::env::temp_dir().join(format!("motif-ids-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("ids.msc");
        let old = "motif 1\nartboard \"a\" 40x40\nrect r size 10 fill #123456\nanim Idle 1s loop {\n  r.x 0: 0 | 1: 0\n  event step 0.5\n}\nmachine Hero {\n  input speed 0\n  state Idle\n}\n";
        std::fs::write(&path, old).unwrap();
        let mut app = fixture(old);
        app.path = Some(path.clone());
        app.saved_src = app.file_text();
        app.save(false);
        let saved = std::fs::read_to_string(&path).unwrap();
        let ids = motif_script::parse(&saved).unwrap().doc.ids;
        let speed = ids.get(motif_core::IdKind::Input, "Hero", "speed").expect("the input got an id").to_string();
        for (kind, owner, name) in
            [(motif_core::IdKind::State, "Hero", "Idle"), (motif_core::IdKind::Animation, "", "Idle"), (motif_core::IdKind::Event, "", "step")]
        {
            assert!(ids.get(kind, owner, name).is_some(), "{kind:?} {name}:\n{saved}");
        }
        assert!(!app.dirty() && app.errors.is_empty());
        let renamed = motif_script::edit_machine(&app.src, &app.scene, "Hero", |m| m.inputs[0].name = "скорость".into()).unwrap();
        let ids = motif_script::parse(&renamed).unwrap().doc.ids;
        assert_eq!(ids.get(motif_core::IdKind::Input, "Hero", "скорость"), Some(speed.as_str()));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn saving_writes_the_format_version_first() {
        let dir = std::env::temp_dir().join(format!("motif-format-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("old.msc");
        let old = "let ink = #123456\nartboard \"a\" 40x40\nrect r size 10 fill ink\nartboard \"b\" 40x40\nrect q size 10 fill ink\n";
        std::fs::write(&path, old).unwrap();
        let mut app = fixture(old);
        app.path = Some(path.clone());
        app.saved_src = app.file_text();
        app.save(false);
        let saved = std::fs::read_to_string(&path).unwrap();
        assert_eq!(saved, format!("motif 1\n{old}"));
        assert!(app.errors.is_empty());
        assert!(!app.dirty(), "the saved file is what the editor holds");
        assert_eq!(motif_script::parse_boards(&saved, None).unwrap().len(), 2);
        // Saving again changes nothing.
        app.save(false);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), saved);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn export_name_is_stored_with_the_board_and_checked() {
        let mut app = fixture("artboard \"Нужда: еда\" 24x24\nrect щека \"Щека\" size 8 fill #123456\nrect nose size 4 fill #654321\n");
        app.set_export_name("щека", "cheek").unwrap();
        let svg = motif_web::render(&app.scene, &motif_web::read(&app.src).unwrap(), None).unwrap();
        assert!(svg.text.contains("id=\"Nuzhda-eda-node-cheek\" data-name=\"Щека\""), "{}", svg.text);
        assert!(app.set_export_name("nose", "cheek").is_err(), "two layers cannot share a name");
        assert!(app.set_export_name("nose", "нос").is_err(), "Latin only");
        app.undo();
        assert!(motif_web::read(&app.src).unwrap().assets.is_empty() || motif_web::read(&app.src).unwrap().assets["Нужда: еда"].layers.is_empty());
    }

    #[test]
    fn setup_and_animation_share_the_frame_while_drawing_shows_the_drawing() {
        use crate::workspace::Workspace;
        let source = include_str!("../../../examples/character-portrait.msc");
        let mut app = fixture(source);
        let original = app.file_text();
        let drivers = app.scene.doc.drivers.clone();
        app.set_workspace(Workspace::Animate);
        app.select_anim("Look".into());
        app.time = 0.4;
        assert!(app.auto_key);
        app.edit("shirt", Prop::Fill, Value::Color(motif_core::Color::WHITE));
        let keyed = app.file_text();
        assert_ne!(keyed, original);
        assert_eq!(app.scene.doc.drivers, drivers);
        assert_ne!(app.scene.doc.node("shirt").unwrap().fill, Some(motif_core::Color::WHITE));
        let pose = app.display_doc();
        for space in Workspace::ALL {
            app.set_workspace(space);
            assert_eq!(app.time, 0.4);
            if space == Workspace::Drawing {
                assert_eq!(app.display_doc().node("shirt").unwrap().fill, app.scene.doc.node("shirt").unwrap().fill, "the drawing, not the frame");
            } else {
                assert_eq!(app.display_doc(), pose);
            }
            assert_eq!(app.file_text(), keyed);
        }
        app.set_workspace(Workspace::Animate);
        app.edit("head", Prop::Yaw, Value::Num(42.0));
        assert_eq!(app.display_doc().node("head").unwrap().yaw, 42.0);
        assert_eq!(app.scene.doc.drivers, drivers);
        app.undo();
        assert_eq!(app.file_text(), keyed);
        app.undo();
        assert_eq!(app.file_text(), original);
        app.redo();
        assert_eq!(app.file_text(), keyed);
        app.parameter_preview("turn", 0.8);
        assert_eq!(app.file_text(), keyed);
        assert!((app.display_doc().node("head").unwrap().yaw - 27.2).abs() < 1e-9);
        app.set_workspace(Workspace::Setup);
        assert_eq!(app.display_doc(), pose);
    }

    #[test]
    fn parameter_keys_and_forms_preserve_other_boards_and_roundtrip() {
        let source = format!("{}\nartboard \"Other\" 20x20\nrect keep size 5\n", include_str!("../../../examples/character-portrait.msc"));
        let mut app = fixture(&source);
        let other = motif_script::parse_boards(&source, None).unwrap()[1].clone();
        app.parameter_preview("turn", 1.0);
        app.parameter_form_edit("turn", "head", Prop::Yaw, 1.0, 38.0);
        app.select_anim("Look".into());
        app.auto_key = true;
        app.time = 0.5;
        app.edit("blink", Prop::Parameter, Value::Num(0.7));
        app.toggle_key("blink", Prop::Parameter);
        assert!(app.key_state("blink", Prop::Parameter) == KeyState::Track);
        app.toggle_key("blink", Prop::Parameter);
        assert!(app.key_state("blink", Prop::Parameter) == KeyState::Key);
        let file = app.file_text();
        assert_eq!(motif_script::parse_boards(&file, None).unwrap()[1], other);
        let reopened = fixture(&file);
        assert_eq!(reopened.scene.doc, app.scene.doc);
        app.switch_board(1);
        assert!(app.pose_preview.is_empty());
        assert!(app.scene.doc.parameters.is_empty());
    }

    pub(crate) fn fixture(src: &str) -> App {
        static SEQUENCE: AtomicUsize = AtomicUsize::new(0);
        let recovery = std::env::temp_dir().join(format!("motif-editor-fixture-{}-{}", std::process::id(), SEQUENCE.fetch_add(1, Ordering::Relaxed)));
        // No user settings, recovery directory, GPU surface or external tools are touched.
        let mut app = App::initialize(egui::Context::default(), None, Default::default(), crate::storage::Recovery::new(recovery));
        app.set_file(src, 0);
        app.saved_src = src.into();
        app.reparse();
        app
    }

    #[test]
    fn portrait_parameter_frames_differ_and_copied_hat_renders_identically() {
        let src = include_str!("../../../examples/character-portrait.msc");
        let original = motif_script::parse(src).unwrap();
        let (copy, _) = motif_script::character::copy_variant(src, &original, "hero", "hat", None).unwrap();
        let copied = motif_script::parse(&copy).unwrap();
        let mut previous = None;
        for time in [0.0, 1.0, 1.5, 3.0] {
            let a = original.doc.animation("Look").unwrap().pose(&original.doc, time);
            let b = copied.doc.animation("Look").unwrap().pose(&copied.doc, time);
            let a = motif_render::render_cpu(&a, Default::default()).unwrap();
            let b = motif_render::render_cpu(&b, Default::default()).unwrap();
            assert_eq!(a.data(), b.data(), "copy lost the shared hat mount at {time}");
            if let Some(previous) = previous {
                assert_ne!(a.data(), previous, "controls must change rendered frames");
            }
            previous = Some(a.data().to_vec());
        }
    }
    #[test]
    fn editing_a_hole_and_undo_redo_keep_all_other_parts_and_boards() {
        let src = "// metadata remains\nartboard \"One\" 100x100\npath ring d \"M0 0 L90 0 L90 90 L0 90 Z M20 20 L20 30 L30 40 L40 40 L40 20 Z\" fill #ff0000\nartboard \"Two\" 40x40\nrect keep size 10\n";
        let mut app = fixture(src);
        let original = app.file_text();
        app.select_only(Some("ring".into()));
        app.begin_point_edit();
        let first = Compound::from_bez(&app.scene.doc.node("ring").unwrap().geometry).unwrap().parts[0].to_bez();
        app.point_edit.as_mut().unwrap().selected = Some(Anchor { part: 1, vertex: 1 });
        assert!(app.delete_point());
        let once = app.file_text();
        app.point_edit.as_mut().unwrap().selected = Some(Anchor { part: 1, vertex: 1 });
        app.delete_point();
        let twice = app.file_text();
        assert_ne!(once, twice);
        assert_eq!(Compound::from_bez(&app.scene.doc.node("ring").unwrap().geometry).unwrap().parts[0].to_bez(), first);
        assert!(twice.ends_with("artboard \"Two\" 40x40\nrect keep size 10\n"));
        app.undo();
        assert_eq!(app.file_text(), once);
        assert!(app.point_edit.as_ref().unwrap().selected.is_none());
        app.undo();
        assert_eq!(app.file_text(), original);
        app.redo();
        app.redo();
        assert_eq!(app.file_text(), twice);
    }
    #[test]
    fn batch_layer_style_move_and_delete_are_single_undo_steps() {
        let src = "artboard \"Layers\" 100x100\nrect a size 10\nrect b size 10\ngroup g {\n rect child size 5\n}\nrect top size 10\n";
        let mut app = fixture(src);
        let original = app.file_text();
        app.select_list(vec!["a".into(), "b".into()]);
        app.style_layer("a", "locked", true, "locked");
        assert!(app.scene.doc.node("a").unwrap().locked && app.scene.doc.node("b").unwrap().locked);
        app.undo();
        assert_eq!(app.file_text(), original);
        app.redo();
        let styled = app.file_text();
        app.move_layer("a", "g", motif_script::Place::Into);
        let moved = app.file_text();
        assert_ne!(moved, styled);
        assert_eq!(
            app.scene.doc.nodes.iter().filter(|n| n.parent == app.scene.doc.index_of("g")).map(|n| n.id.as_str()).collect::<Vec<_>>(),
            ["child", "a", "b"]
        );
        app.undo();
        assert_eq!(app.file_text(), styled);
        app.redo();
        assert_eq!(app.file_text(), moved);
        app.select_list(vec!["g".into(), "child".into(), "top".into()]);
        app.delete_selected_nodes();
        let deleted = app.file_text();
        assert!(app.scene.doc.nodes.is_empty());
        app.undo();
        assert_eq!(app.file_text(), moved);
        app.redo();
        assert_eq!(app.file_text(), deleted);
    }
    #[test]
    fn failed_batch_does_not_mutate_document_or_undo_history() {
        let mut app = fixture("artboard \"Safe\" 100x100\nrect a size 10\nfor i in 0..2 {\n rect item{i} size 5\n}\n");
        let before = app.file_text();
        app.select_list(vec!["a".into(), "item0".into()]);
        app.style_layer("a", "hidden", true, "hidden");
        assert_eq!(app.file_text(), before);
        assert!(app.undo.is_empty());
        app.move_layer("a", "item1", motif_script::Place::After);
        assert_eq!(app.file_text(), before);
        assert!(app.undo.is_empty());
    }

    #[test]
    fn canvas_drag_inserts_on_a_hole_and_commits_once_on_release() {
        let mut app = fixture("artboard \"Mouse\" 300x300\npath p d \"M20 20 L180 20 L180 180 L20 180 Z M60 60 L60 140 L140 140 L140 60 Z\" fill #f00\n");
        app.select_only(Some("p".into()));
        app.begin_point_edit();
        let original = app.file_text();
        let frame = |app: &mut App, events, time| {
            let ctx = app.ctx.clone();
            let mut canvas = Rect::NOTHING;
            ctx.run_ui(
                egui::RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(400.0, 400.0))), time: Some(time), events, ..Default::default() },
                |ui| {
                    let (rect, response) = ui.allocate_exact_size(vec2(300.0, 300.0), Sense::click_and_drag());
                    canvas = rect;
                    let doc = app.display_doc();
                    app.point_edit_input(ui, &response, rect, View { zoom: 1.0, pan: egui::Vec2::ZERO }, &doc);
                },
            )
            .drop_without_applying_deltas();
            canvas
        };
        let rect = frame(&mut app, vec![], 0.0);
        let from = rect.min + vec2(100.0, 60.0);
        let to = from + vec2(10.0, -15.0);
        let button = |pos, pressed| egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed, modifiers: Modifiers::NONE };
        frame(&mut app, vec![egui::Event::PointerMoved(from), button(from, true)], 0.1);
        frame(&mut app, vec![egui::Event::PointerMoved(to)], 0.2);
        assert_eq!(app.file_text(), original, "drag preview must not save the inserted point yet");
        frame(&mut app, vec![], 1.5);
        frame(&mut app, vec![button(to, false)], 1.6);
        let edited = app.file_text();
        assert_ne!(edited, original);
        assert_eq!(app.undo.len(), 1);
        let shape = Compound::from_bez(&app.scene.doc.node("p").unwrap().geometry).unwrap();
        assert_eq!(shape.parts[0].outline.verts.len(), 4);
        assert_eq!(shape.parts[1].outline.verts.len(), 5);
        app.undo();
        assert_eq!(app.file_text(), original);
        app.redo();
        assert_eq!(app.file_text(), edited);
    }

    #[test]
    fn layers_ui_shift_ctrl_and_context_delete_apply_to_the_visible_selection() {
        let mut app = fixture("artboard \"Mouse\" 300x300\nrect a size 10\nrect b size 10\nrect c size 10\n");
        let original = app.file_text();
        let frame = |app: &mut App, mut events: Vec<egui::Event>, modifiers| {
            let ctx = app.ctx.clone();
            events.insert(0, egui::Event::ModifiersChanged(modifiers));
            ctx.run_ui(egui::RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(400.0, 600.0))), events, ..Default::default() }, |ui| {
                app.layers_panel(ui)
            })
        };
        let position = |out: &egui::FullOutput, label: &str| {
            out.shapes
                .iter()
                .rev()
                .find_map(|s| match &s.shape {
                    egui::Shape::Text(t) if t.galley.job.text == label => Some(t.pos + vec2(3.0, t.galley.size().y * 0.5)),
                    _ => None,
                })
                .unwrap_or_else(|| panic!("UI label not found: {label}"))
        };
        let click = |app: &mut App, pos, button, modifiers| {
            frame(app, vec![egui::Event::PointerMoved(pos), egui::Event::PointerButton { pos, button, pressed: true, modifiers }], modifiers)
                .drop_without_applying_deltas();
            frame(app, vec![egui::Event::PointerButton { pos, button, pressed: false, modifiers }], modifiers)
        };
        frame(&mut app, vec![], Modifiers::NONE).drop_without_applying_deltas();
        let out = frame(&mut app, vec![], Modifiers::NONE);
        let a = position(&out, "a");
        let b = position(&out, "b");
        let c = position(&out, "c");
        out.drop_without_applying_deltas();
        click(&mut app, a, egui::PointerButton::Primary, Modifiers::NONE).drop_without_applying_deltas();
        click(&mut app, c, egui::PointerButton::Primary, Modifiers { shift: true, ..Modifiers::NONE }).drop_without_applying_deltas();
        assert_eq!(app.selection(), ["c", "b", "a"]);
        click(&mut app, b, egui::PointerButton::Primary, Modifiers { ctrl: true, command: true, ..Modifiers::NONE }).drop_without_applying_deltas();
        assert_eq!(app.selection(), ["c", "a"]);
        click(&mut app, c, egui::PointerButton::Secondary, Modifiers::NONE).drop_without_applying_deltas();
        let out = frame(&mut app, vec![], Modifiers::NONE);
        let delete = position(&out, "Удалить");
        out.drop_without_applying_deltas();
        click(&mut app, delete, egui::PointerButton::Primary, Modifiers::NONE).drop_without_applying_deltas();
        assert_eq!(app.scene.doc.nodes.iter().map(|n| n.id.as_str()).collect::<Vec<_>>(), ["b"]);
        app.undo();
        assert_eq!(app.file_text(), original);
    }

    #[test]
    fn character_changes_and_copy_are_atomic_undo_steps_and_keep_other_boards() {
        use motif_script::character;
        let src = format!("{}\nartboard \"Other\" 30x30\nrect keep size 5\n", include_str!("../../../examples/character-side.msc"));
        let mut app = fixture(&src);
        let original = app.file_text();
        let catalog = character::read(&app.src).unwrap().unwrap();
        let rig = &catalog.characters[0];
        app.select_anim("Walk".into());
        app.time = 0.35;
        app.set_workspace(crate::workspace::Workspace::Animate);
        app.playing = true;
        app.character_choice(&rig.root, &rig.looks[1].slots);
        assert!(app.file_text() == original && app.undo.is_empty(), "animation must protect wardrobe");
        app.set_mode_animate(false);
        app.character_choice(&rig.root, &rig.looks[1].slots);
        app.set_mode_animate(true);
        let clothed = app.file_text();
        assert_ne!(clothed, original);
        assert_eq!(app.undo.len(), 1);
        assert_eq!(app.anim.as_deref(), Some("Walk"));
        assert_eq!(app.time, 0.35);
        assert!(!app.playing);
        app.set_mode_animate(false);
        assert!(clothed.ends_with("artboard \"Other\" 30x30\nrect keep size 5\n"));
        app.character_copy(&rig.root, "head");
        let custom = app.file_text();
        assert_eq!(app.undo.len(), 2);
        assert_eq!(app.anim.as_deref(), Some("Walk"));
        assert!(!app.playing);
        app.undo();
        assert_eq!(app.file_text(), clothed);
        app.undo();
        assert_eq!(app.file_text(), original);
        app.redo();
        app.redo();
        assert_eq!(app.file_text(), custom);
    }

    #[test]
    fn character_ui_selects_a_look_by_mouse_and_returns_to_the_same_clip_time() {
        let mut app = fixture(include_str!("../../../examples/character-side.msc"));
        app.select_anim("Run".into());
        app.time = 0.2;
        app.playing = true;
        let original = app.file_text();
        app.set_mode_animate(false);
        let frame = |app: &mut App, events| {
            let ctx = app.ctx.clone();
            ctx.run_ui(egui::RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(380.0, 800.0))), events, ..Default::default() }, |ui| {
                app.character_panel(ui)
            })
        };
        let position = |out: &egui::FullOutput, label: &str| {
            out.shapes
                .iter()
                .rev()
                .find_map(|s| match &s.shape {
                    egui::Shape::Text(t) if t.galley.job.text == label => Some(t.pos + vec2(3.0, t.galley.size().y * 0.5)),
                    _ => None,
                })
                .unwrap_or_else(|| panic!("UI label missing: {label}"))
        };
        let click = |app: &mut App, pos| {
            frame(
                app,
                vec![
                    egui::Event::PointerMoved(pos),
                    egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE },
                ],
            )
            .drop_without_applying_deltas();
            frame(app, vec![egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE }])
                .drop_without_applying_deltas();
        };
        frame(&mut app, vec![]).drop_without_applying_deltas();
        let out = frame(&mut app, vec![]);
        let combo = position(&out, "Манекен");
        out.drop_without_applying_deltas();
        click(&mut app, combo);
        let out = frame(&mut app, vec![]);
        let traveler = position(&out, "Путник");
        out.drop_without_applying_deltas();
        click(&mut app, traveler);
        assert!(app.scene.doc.node("head_traveler").unwrap().visible);
        assert!(!app.scene.doc.node("head_base").unwrap().visible);
        app.set_mode_animate(true);
        assert_eq!(app.anim.as_deref(), Some("Run"));
        assert_eq!(app.time, 0.2);
        assert!(!app.playing);
        assert_eq!(app.undo.len(), 1);
        app.undo();
        assert_eq!(app.file_text(), original);
    }

    #[test]
    fn a_copied_appearance_renders_identically_and_bad_choices_do_not_change_history() {
        use motif_script::character;
        let mut app = fixture(include_str!("../../../examples/character-side.msc"));
        let original = app.file_text();
        let source = app.scene.clone();
        app.character_choice("hero", &std::collections::BTreeMap::from([("head".into(), "missing".into())]));
        assert_eq!(app.file_text(), original);
        assert!(app.undo.is_empty());
        app.character_copy("hero", "arms");
        character::read(&app.src).unwrap().unwrap().validate(&app.scene).unwrap();
        let options = motif_render::CpuOptions::default();
        for clip in ["Idle", "Walk", "Run", "Jump"] {
            for t in [0.0, 0.2, 0.4, 0.6] {
                let a = source.doc.animation(clip).unwrap().pose(&source.doc, t);
                let b = app.scene.doc.animation(clip).unwrap().pose(&app.scene.doc, t);
                assert_eq!(motif_render::render_cpu(&a, options).unwrap().data(), motif_render::render_cpu(&b, options).unwrap().data(), "{clip} at {t}");
            }
        }
    }

    #[test]
    fn platformer_controls_and_full_copy_preserve_playback_other_boards_and_undo() {
        use motif_script::character;
        let src = format!("{}\nartboard \"Other\" 30x30\nrect keep size 5\n", include_str!("../../../examples/character-platformer.msc"));
        let mut app = fixture(&src);
        let original = app.file_text();
        app.select_anim("Run".into());
        app.time = 0.25;
        app.playing = true;
        app.set_mode_animate(false);
        app.character_look("hero", "Зелёный герой");
        assert_eq!(app.time, 0.25);
        assert!(!app.playing);
        let green = app.file_text();
        let config = character::Settings { parameters: std::collections::BTreeMap::from([("head_size".into(), 1.1)]), ..Default::default() };
        app.character_configure("hero", &config);
        assert_eq!(app.time, 0.25);
        assert_eq!(app.undo.len(), 2);
        assert!(app.file_text().ends_with("artboard \"Other\" 30x30\nrect keep size 5\n"));
        let resized = app.file_text();
        app.character_copy_look("hero");
        let copied = app.file_text();
        app.set_mode_animate(true);
        assert_eq!(app.anim.as_deref(), Some("Run"));
        assert_eq!(app.time, 0.25);
        app.set_mode_animate(false);
        assert_eq!(app.anim.as_deref(), Some("Run"));
        assert_eq!(app.undo.len(), 3);
        app.undo();
        assert_eq!(app.file_text(), resized);
        app.undo();
        assert_eq!(app.file_text(), green);
        app.undo();
        assert_eq!(app.file_text(), original);
        app.redo();
        app.redo();
        app.redo();
        assert_eq!(app.file_text(), copied);
    }

    #[test]
    fn full_costume_copy_is_pixel_identical_and_a_late_failure_is_atomic() {
        const SRC: &str = include_str!("../../../examples/character-platformer.msc");
        let mut app = fixture(SRC);
        let before = app.scene.clone();
        app.character_copy_look("hero");
        for clip in ["Idle", "Walk", "Run", "Jump"] {
            for t in [0.0, 0.25, 0.5] {
                let a = before.doc.animation(clip).unwrap().pose(&before.doc, t);
                let b = app.scene.doc.animation(clip).unwrap().pose(&app.scene.doc, t);
                assert_eq!(
                    motif_render::render_cpu(&a, Default::default()).unwrap().data(),
                    motif_render::render_cpu(&b, Default::default()).unwrap().data(),
                    "{clip} {t}"
                );
            }
        }
        // The foot slot is processed after head, torso and arms: even a late failure rolls back everything.
        let src = format!("{SRC}\nanim Custom 1s {{\n  boot_front.x 0: 0 | 1: 5\n}}\n");
        let mut app = fixture(&src);
        let original = app.file_text();
        app.character_copy_look("hero");
        assert_eq!(app.file_text(), original);
        assert!(app.undo.is_empty());
        app.character_configure(
            "hero",
            &motif_script::character::Settings { parameters: std::collections::BTreeMap::from([("head_size".into(), 9.0)]), ..Default::default() },
        );
        assert_eq!(app.file_text(), original);
        assert!(app.undo.is_empty());
    }
}

#[cfg(test)]
#[path = "workspace_tests.rs"]
mod workspace_tests;

#[path = "canvas_menu.rs"]
mod canvas_menu;

#[path = "preview_ui.rs"]
mod preview_ui;

#[cfg(test)]
#[path = "canvas_interaction_tests.rs"]
mod canvas_interaction_tests;

#[cfg(test)]
#[path = "preview_tests.rs"]
mod preview_tests;

#[path = "deformer_ui.rs"]
mod deformer_ui;

#[cfg(test)]
#[path = "deformer_tests.rs"]
mod deformer_tests;

#[path = "corrective_ui.rs"]
mod corrective_ui;
#[path = "html_ui.rs"]
mod html_ui;
#[cfg(test)]
#[path = "performance_tests.rs"]
mod performance_tests;

#[cfg(test)]
#[path = "check_integration_tests.rs"]
mod check_integration_tests;
