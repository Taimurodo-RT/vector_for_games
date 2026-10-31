//! Motif in Godot 4 without baking: the `MotifSprite` node reads a `.msc` scene and plays it in
//! the game with the same core as the editor, so bones, soft bodies, morphs, 3D turns, the state
//! machine and its events behave exactly as they do in Motif. Each frame the pose goes to the video
//! card as triangles (or, with `gpu` off, is drawn on the CPU at the size the node has on
//! screen), so it stays sharp when the camera zooms in.

use godot::classes::canvas_item_material::BlendMode;
use godot::classes::image::Format;
use godot::classes::{CanvasItemMaterial, FileAccess, INode2D, Image, ImageTexture, Input, Node2D, RenderingServer};
use godot::global::MouseButton;
use godot::prelude::*;

#[cfg(test)]
mod player_tests;
mod pool;

pub use motif_runtime::{Frame, Player, Step};

use motif_render::mesh::{Cutter, Piece};
use std::sync::{Arc, Mutex};

struct MotifExtension;

#[gdextension]
unsafe impl ExtensionLibrary for MotifExtension {}

/// A Motif scene playing in the game: one artboard of a `.msc` file, its state machine (or one
/// animation), drawn as vectors at the size it is shown.
#[derive(GodotClass)]
#[class(tool, base = Node2D)]
pub struct MotifSprite {
    /// The `.msc` file.
    #[export(file = "*.msc")]
    #[var(set = set_scene)]
    scene: GString,
    /// The artboard to play, by name; empty — the first one.
    #[export]
    #[var(set = set_board)]
    board: GString,
    /// The state machine that drives the scene; empty — the first one, unless `animation` is set.
    #[export]
    #[var(set = set_machine)]
    machine: GString,
    /// One animation played instead of a machine.
    #[export]
    #[var(set = set_animation)]
    animation: GString,
    #[export]
    playing: bool,
    /// How fast time runs: 1 — as in Motif.
    #[export(range = (0.0, 4.0, 0.05, or_greater))]
    speed: f64,
    /// Draw the artboard around the node's position rather than from its top-left corner.
    #[export]
    #[var(set = set_centered)]
    centered: bool,
    /// Pixels drawn per screen pixel: above 1 for smoother edges, below 1 to save time.
    #[export(range = (0.25, 4.0, 0.25))]
    resolution: f64,
    /// Paint the artboard's background colour behind the scene.
    #[export]
    background: bool,
    /// Draw on the video card: shapes go to Godot as triangles, and only blurs, shadows, masks,
    /// gradients and blend modes are drawn on the processor, each as a small picture. Off — the
    /// whole frame is drawn on the processor, exactly as `get_image` and Motif draw it.
    #[export]
    gpu: bool,
    /// Feed the mouse to the machine's `listen` lines (hover, press, click, x, y).
    #[export]
    pointer: bool,
    /// Where the character looks, for a scene with `directions`: any vector, the nearest side is
    /// shown. (1, 0) — right, (0, -1) — up, away from the viewer. The walk goes on from the same step.
    #[export]
    #[var(set = set_facing)]
    facing: Vector2,
    /// The side shown, by name: down, down_right, right, up_right, up, up_left, left, down_left.
    #[var(get = get_side, set = set_side)]
    #[allow(dead_code)] // The player keeps the side: this field only gives the property its name.
    side: GString,
    player: Option<Player>,
    error: Option<String>,
    texture: Option<Gd<ImageTexture>>,
    /// The size of the last drawn image, to reuse its texture.
    drawn: (i32, i32),
    /// What the texture shows: the pose, its scale and background.
    shot: Option<Shot>,
    /// The frame being drawn on the drawing threads, and what it will show.
    pending: Option<(Shot, pool::Pending<Picture>)>,
    /// The pieces shown on the video card, and the textures of their pictures.
    pieces: Vec<Piece>,
    patches: Vec<Gd<ImageTexture>>,
    /// Remembers the shapes cut, so a shape that only moves is not cut again.
    cutter: Arc<Mutex<Cutter>>,
    dirty: bool,
    base: Base<Node2D>,
}

#[godot_api]
impl INode2D for MotifSprite {
    fn init(base: Base<Node2D>) -> Self {
        MotifSprite {
            scene: GString::new(),
            board: GString::new(),
            machine: GString::new(),
            animation: GString::new(),
            playing: true,
            speed: 1.0,
            centered: true,
            resolution: 1.0,
            background: false,
            gpu: true,
            pointer: true,
            facing: Vector2::DOWN,
            side: GString::new(),
            player: None,
            error: None,
            texture: None,
            drawn: (0, 0),
            shot: None,
            pending: None,
            pieces: Vec::new(),
            patches: Vec::new(),
            cutter: Arc::default(),
            dirty: true,
            base,
        }
    }

    fn ready(&mut self) {
        // tiny-skia draws premultiplied colour: blended as such, edges stay clean.
        let mut material = CanvasItemMaterial::new_gd();
        material.set_blend_mode(BlendMode::PREMULT_ALPHA);
        self.base_mut().set_material(&material);
        if self.player.is_none() && self.error.is_none() {
            self.reload();
        }
    }

    fn process(&mut self, delta: f64) {
        if self.player.is_none() && self.error.is_none() && !self.scene.is_empty() {
            self.reload();
        }
        let pointer = self.pointer.then(|| self.pointer_at());
        let Some(player) = &mut self.player else { return };
        if let Some((at, down)) = pointer {
            player.pointer(Some(at), down);
        }
        if self.playing {
            self.step(delta * self.speed);
        }
        self.prepare();
        if self.dirty {
            self.base_mut().queue_redraw();
        }
    }

    fn draw(&mut self) {
        self.dirty = false;
        let Some(want) = self.wanted() else { return };
        if self.shot != Some(want) {
            let picture = match self.pending.take() {
                Some((shot, pending)) if shot == want => pending.wait(),
                _ => self.player.as_ref().map(|p| self.work(p.frame(), want)()),
            };
            self.shot = Some(want);
            match picture {
                Some(Picture::Pieces(pieces)) => self.keep(pieces),
                Some(Picture::Pixels(px)) => self.upload(px),
                None => self.upload(None),
            }
        }
        let rect = Rect2::new(self.origin(), self.size());
        if !want.gpu {
            if let Some(t) = self.texture.clone() {
                self.base_mut().draw_texture_rect(&t, rect, false);
            }
            return;
        }
        let flipped = self.player.as_ref().is_some_and(|p| p.flipped());
        if want.background
            && let Some(p) = &self.player
        {
            let c = p.background();
            let color = Color::from_rgba8(c.r, c.g, c.b, c.a);
            // Premultiplied, as the material blends.
            let a = color.a;
            self.base_mut().draw_rect(rect, Color::from_rgba(color.r * a, color.g * a, color.b * a, a));
        }
        // Artboard units from here on; a flipped side turned over around the artboard.
        let o = self.origin();
        let place = if flipped {
            Transform2D::from_cols(Vector2::new(-1.0, 0.0), Vector2::new(0.0, 1.0), Vector2::new(o.x + rect.size.x, o.y))
        } else {
            Transform2D::from_cols(Vector2::new(1.0, 0.0), Vector2::new(0.0, 1.0), o)
        };
        self.base_mut().draw_set_transform_matrix(place);
        let item = self.base().get_canvas_item();
        let mut server = RenderingServer::singleton();
        let mut patch = 0;
        for piece in &self.pieces {
            match piece {
                Piece::Mesh(m) => {
                    let points: PackedVector2Array = m.points.iter().map(|p| Vector2::new(p[0], p[1])).collect();
                    let colors: PackedColorArray = m.colors.iter().map(|c| Color::from_rgba(c[0], c[1], c[2], c[3])).collect();
                    let indices = PackedInt32Array::from(m.indices.as_slice());
                    server.canvas_item_add_triangle_array(item, &indices, &points, &colors);
                }
                Piece::Patch(p) => {
                    let Some(texture) = self.patches.get(patch) else { continue };
                    patch += 1;
                    let size = Vector2::new(p.pixmap.width() as f32, p.pixmap.height() as f32) / p.scale as f32;
                    let at = Rect2::new(Vector2::new(p.at.x as f32, p.at.y as f32), size);
                    server.canvas_item_add_texture_rect(item, at, texture.get_rid());
                }
            }
        }
        self.base_mut().draw_set_transform_matrix(Transform2D::IDENTITY);
    }

    fn get_configuration_warnings(&self) -> PackedStringArray {
        let mut out = PackedStringArray::new();
        if self.scene.is_empty() {
            out.push("Укажите файл сцены .msc в свойстве Scene");
        } else if let Some(e) = &self.error {
            out.push(e.as_str());
        }
        out
    }
}

#[godot_api]
impl MotifSprite {
    /// An animation event passed while playing: a step, a hit, the moment to play a sound.
    #[signal]
    fn event(name: GString);

    /// The same event with its data (`event hit 0.3 damage=2` gives `{"damage": 2.0}`), sent
    /// right after `event` for every event, an empty dictionary when it has none.
    #[signal]
    fn event_data(name: GString, data: VarDictionary);

    /// The machine moved to another state.
    #[signal]
    fn state_changed(state: GString);

    /// The character turned to another side (`facing` or `side` changed it).
    #[signal]
    fn side_changed(side: GString);

    #[func]
    fn set_facing(&mut self, facing: Vector2) {
        if facing == Vector2::ZERO {
            return;
        }
        self.facing = facing;
        let turned = self.player.as_mut().is_some_and(|p| p.face_toward(facing.x as f64, facing.y as f64));
        self.turned(turned);
    }

    #[func]
    fn get_side(&self) -> GString {
        self.player.as_ref().map(|p| GString::from(p.side().as_str())).unwrap_or_default()
    }

    #[func]
    fn set_side(&mut self, side: GString) {
        let Some(p) = &mut self.player else { return };
        match p.set_side(&side.to_string()) {
            Ok(turned) => {
                // `facing` follows, so the side is kept when the scene is saved or reloaded.
                let sides = p.sides().len().max(1) as f32;
                let k = p.sides().iter().position(|n| *n == side.to_string()).unwrap_or(0) as f32;
                let a = k / sides * std::f32::consts::TAU;
                self.facing = Vector2::new(a.sin(), a.cos());
                self.turned(turned);
            }
            Err(e) => {
                godot_warn!("MotifSprite: {e}");
            }
        }
    }

    /// The scene's sides, from facing the viewer round clockwise; empty without `directions`.
    #[func]
    fn get_sides(&self) -> PackedStringArray {
        self.player.as_ref().map(|p| p.sides().iter().map(|s| GString::from(s.as_str())).collect()).unwrap_or_default()
    }

    /// Whether the side shown is another side turned over (a left side of mirrored `directions`).
    #[func]
    fn is_flipped(&self) -> bool {
        self.player.as_ref().is_some_and(|p| p.flipped())
    }

    /// The animation playing now, as seen from the side: «Walk_up» while the state plays «Walk».
    #[func]
    fn get_current_animation(&self) -> GString {
        self.player.as_ref().map(|p| GString::from(p.animation().as_str())).unwrap_or_default()
    }

    /// How far the state's animation has played, in seconds.
    #[func]
    fn get_time(&self) -> f64 {
        self.player.as_ref().map_or(0.0, |p| p.time())
    }

    /// Moves time on by `seconds` at once, playing or paused, with its events and states sent:
    /// for a step frame by frame, or for a game that keeps its own time.
    #[func]
    fn advance(&mut self, seconds: f64) {
        self.step(seconds);
        self.base_mut().queue_redraw();
    }

    #[func]
    fn set_scene(&mut self, scene: GString) {
        self.scene = scene;
        self.reload();
    }

    #[func]
    fn set_board(&mut self, board: GString) {
        self.board = board;
        self.reload();
    }

    #[func]
    fn set_machine(&mut self, machine: GString) {
        self.machine = machine;
        self.reload();
    }

    #[func]
    fn set_animation(&mut self, animation: GString) {
        self.animation = animation;
        self.reload();
    }

    #[func]
    fn set_centered(&mut self, centered: bool) {
        self.centered = centered;
        self.redraw();
    }

    /// Sets a machine input: a number, or a flag as true/false.
    #[func]
    fn set_input(&mut self, name: GString, value: Variant) {
        let value = match value.get_type() {
            VariantType::FLOAT => value.to::<f64>(),
            VariantType::INT => value.to::<i64>() as f64,
            VariantType::BOOL => f64::from(u8::from(value.to::<bool>())),
            _ => {
                godot_warn!("MotifSprite: вход «{name}» — число или true/false, а не {value}");
                return;
            }
        };
        if let Some(p) = &mut self.player
            && let Err(e) = p.set(&name.to_string(), value)
        {
            godot_warn!("MotifSprite: {e}");
        }
    }

    /// A machine input's value now (a flag or a trigger is 0 or 1).
    #[func]
    fn get_input(&self, name: GString) -> f64 {
        self.player.as_ref().and_then(|p| p.get(&name.to_string())).unwrap_or(0.0)
    }

    /// Fires a trigger input once.
    #[func]
    fn fire(&mut self, name: GString) {
        if let Some(p) = &mut self.player
            && let Err(e) = p.fire(&name.to_string())
        {
            godot_warn!("MotifSprite: {e}");
        }
    }

    /// The state the machine is in, or the animation playing.
    #[func]
    fn get_state(&self) -> GString {
        self.player.as_ref().map(|p| GString::from(p.state().as_str())).unwrap_or_default()
    }

    /// Goes to a state of the machine at once, without a blend.
    #[func]
    fn jump_to(&mut self, state: GString) {
        if let Some(p) = &mut self.player
            && let Err(e) = p.jump(&state.to_string())
        {
            godot_warn!("MotifSprite: {e}");
        }
        self.redraw();
    }

    /// Starts the scene over: the first state, the inputs at their defaults, time 0.
    #[func]
    fn restart(&mut self) {
        if let Some(p) = &mut self.player {
            p.restart();
        }
        self.redraw();
    }

    /// The machine's inputs, by name.
    #[func]
    fn get_inputs(&self) -> PackedStringArray {
        self.player.as_ref().map(|p| p.inputs().iter().map(|s| GString::from(s.as_str())).collect()).unwrap_or_default()
    }

    /// The frame shown now as an image, `scale` pixels per artboard unit: for a screenshot, an
    /// icon or a texture of its own.
    #[func]
    fn get_image(&self, scale: f64) -> Option<Gd<Image>> {
        let px = self.player.as_ref()?.render(scale.clamp(0.01, 16.0), self.background)?;
        let mut data = px.data().to_vec();
        // Godot keeps straight colour in images: undo the premultiplication.
        for p in data.as_chunks_mut::<4>().0 {
            let a = p[3] as u32;
            if a > 0 && a < 255 {
                for c in &mut p[..3] {
                    *c = ((*c as u32 * 255 + a / 2) / a).min(255) as u8;
                }
            }
        }
        Image::create_from_data(px.width() as i32, px.height() as i32, false, Format::RGBA8, &PackedByteArray::from(data.as_slice()))
    }

    /// The artboard's size in pixels at scale 1.
    #[func]
    fn get_size(&self) -> Vector2 {
        self.size()
    }

    /// Reads the scene file again and plays on from where it is: the state, its time, the
    /// inputs, springs and particles go on with the change. A scene with errors is not taken:
    /// the old one plays on and the errors are warned about. Returns whether it was taken.
    #[func]
    fn hot_reload(&mut self) -> bool {
        if self.player.is_none() {
            self.reload();
            return self.player.is_some();
        }
        let taken = self.read().and_then(|(src, base)| self.player.as_mut().map_or(Ok(()), |p| p.reload(&src, base.as_deref())));
        match &taken {
            // Shapes may have changed under the same names: cut them again.
            Ok(()) => {
                self.shot = None;
                self.pending = None;
                self.pieces.clear();
                self.cutter = Arc::default();
            }
            Err(e) => {
                godot_warn!("MotifSprite {}: {e}", self.scene);
            }
        }
        self.redraw();
        taken.is_ok()
    }

    /// Reads the scene file again (after it was edited in Motif).
    #[func]
    fn reload(&mut self) {
        self.player = None;
        self.error = None;
        self.shot = None;
        self.pending = None;
        self.pieces.clear();
        self.cutter = Arc::default();
        if !self.scene.is_empty() {
            match self.load() {
                Ok(mut p) => {
                    p.face_toward(self.facing.x as f64, self.facing.y as f64);
                    self.player = Some(p);
                }
                Err(e) => {
                    godot_warn!("MotifSprite {}: {e}", self.scene);
                    self.error = Some(e);
                }
            }
        }
        self.base_mut().update_configuration_warnings();
        self.redraw();
    }
}

/// What a picture of the scene shows.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Shot {
    pose: u64,
    scale: f64,
    background: bool,
    gpu: bool,
}

/// A frame drawn: the whole picture, or pieces for the video card.
enum Picture {
    Pixels(Option<tiny_skia::Pixmap>),
    Pieces(Vec<Piece>),
}

/// A file of the project (`res://…`), packed or not.
fn read_res(path: &std::path::Path) -> Option<Vec<u8>> {
    // `res://scenes` joined with a font name gets a backslash on Windows.
    let path = GString::from(path.to_str()?.replace('\\', "/").as_str());
    FileAccess::file_exists(&path).then(|| FileAccess::get_file_as_bytes(&path).to_vec())
}

/// Says once when the library and the addon folder around it come from different Motif builds
/// (a hand-copied addon, a library left from before): the build packs `version.txt` beside it.
fn check_build() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        let Some(lib) = option_env!("MOTIF_BUILD") else { return };
        let file = GString::from("res://addons/motif/version.txt");
        if !FileAccess::file_exists(&file) {
            return;
        }
        let addon = FileAccess::get_file_as_string(&file).to_string();
        if addon.trim() != lib {
            godot_warn!(
                "MotifSprite: библиотека аддона из сборки Motif {lib}, а addons/motif — из {}: поставьте аддон заново из Motif («Поставить в проект Godot…»)",
                addon.trim()
            );
        }
    });
}

impl MotifSprite {
    fn load(&self) -> Result<Player, String> {
        let (src, base) = self.read()?;
        Player::load(&src, base.as_deref(), &self.board.to_string(), &self.machine.to_string(), &self.animation.to_string())
    }

    /// The scene's text and the folder it is in.
    fn read(&self) -> Result<(String, Option<std::path::PathBuf>), String> {
        check_build();
        let path = self.scene.to_string();
        if !FileAccess::file_exists(&self.scene) {
            return Err(format!("нет файла {path}"));
        }
        let src = FileAccess::get_file_as_string(&self.scene).to_string();
        // Fonts next to the scene are read through Godot, so they are found in an exported
        // game's pack too.
        motif_script::fonts::set_reader(read_res);
        Ok((src, path.rfind('/').map(|i| std::path::PathBuf::from(&path[..i]))))
    }

    /// What the node should show now.
    fn wanted(&self) -> Option<Shot> {
        let pose = self.player.as_ref()?.pose();
        Some(Shot { pose, scale: self.screen_scale() * self.resolution.clamp(0.25, 4.0), background: self.background, gpu: self.gpu })
    }

    /// The work of drawing `frame` as `shot` asks: a whole picture, or pieces for the video card.
    fn work(&self, frame: Frame, shot: Shot) -> impl FnOnce() -> Picture + Send + 'static {
        let cutter = self.cutter.clone();
        move || {
            if shot.gpu {
                let Ok(mut cutter) = cutter.lock() else { return Picture::Pieces(Vec::new()) };
                Picture::Pieces(frame.pieces(&mut cutter, shot.scale))
            } else {
                Picture::Pixels(frame.render(shot.scale, shot.background))
            }
        }
    }

    /// Keeps the pieces to draw and puts their pictures into textures, reusing those of the
    /// same size.
    fn keep(&mut self, pieces: Vec<Piece>) {
        let mut k = 0;
        for piece in &pieces {
            let Piece::Patch(p) = piece else { continue };
            let (w, h) = (p.pixmap.width() as i32, p.pixmap.height() as i32);
            let Some(image) = Image::create_from_data(w, h, false, Format::RGBA8, &PackedByteArray::from(p.pixmap.data())) else { continue };
            match self.patches.get_mut(k) {
                Some(t) if t.get_width() == w && t.get_height() == h => t.update(&image),
                Some(t) => {
                    if let Some(new) = ImageTexture::create_from_image(&image) {
                        *t = new;
                    }
                }
                None => {
                    if let Some(new) = ImageTexture::create_from_image(&image) {
                        self.patches.push(new);
                    }
                }
            }
            k += 1;
        }
        self.patches.truncate(k);
        self.pieces = pieces;
    }

    /// Starts drawing the frame on the drawing threads when the picture shown is out of date: a
    /// new pose, a zoom, another background. All the sprites' frames are drawn at once while
    /// the game goes on, and each is taken in its `draw`.
    fn prepare(&mut self) {
        let Some(want) = self.wanted() else { return };
        if self.shot == Some(want) || self.pending.as_ref().is_some_and(|(s, _)| *s == want) || !self.base().is_visible_in_tree() {
            return;
        }
        let Some(player) = &self.player else { return };
        let work = self.work(player.frame(), want);
        self.pending = Some((want, pool::start(work)));
        self.dirty = true;
    }

    /// Puts the picture into the texture drawn; nothing is drawn without one.
    fn upload(&mut self, px: Option<tiny_skia::Pixmap>) {
        let image = px.and_then(|px| {
            let (w, h) = (px.width() as i32, px.height() as i32);
            Image::create_from_data(w, h, false, Format::RGBA8, &PackedByteArray::from(px.data())).map(|i| (i, (w, h)))
        });
        let Some((image, size)) = image else {
            self.texture = None;
            return;
        };
        match &mut self.texture {
            Some(t) if self.drawn == size => t.update(&image),
            _ => self.texture = ImageTexture::create_from_image(&image),
        }
        self.drawn = size;
    }

    /// Moves time on by `dt`, sending the events passed and the state entered.
    fn step(&mut self, dt: f64) {
        let Some(player) = &mut self.player else { return };
        let step = player.advance(dt);
        for e in &step.events {
            self.base_mut().emit_signal("event", &[e.name.to_variant()]);
            let mut data = VarDictionary::new();
            for (k, v) in &e.data {
                let v = match v {
                    motif_runtime::EventValue::Number(n) => n.to_variant(),
                    motif_runtime::EventValue::Bool(b) => b.to_variant(),
                    motif_runtime::EventValue::Text(t) => t.to_variant(),
                };
                data.set(k.as_str(), &v);
            }
            self.base_mut().emit_signal("event_data", &[e.name.to_variant(), data.to_variant()]);
        }
        if let Some(state) = step.state {
            self.base_mut().emit_signal("state_changed", &[state.to_variant()]);
        }
    }

    /// After a turn: the side is announced and drawn.
    fn turned(&mut self, turned: bool) {
        if turned {
            let side = self.get_side();
            self.base_mut().emit_signal("side_changed", &[side.to_variant()]);
            self.redraw();
        }
    }

    fn redraw(&mut self) {
        self.dirty = true;
        self.base_mut().queue_redraw();
    }

    fn size(&self) -> Vector2 {
        self.player.as_ref().map_or(Vector2::ZERO, |p| {
            let (w, h) = p.size();
            Vector2::new(w as f32, h as f32)
        })
    }

    /// Where the artboard's top-left corner is drawn, in the node's own coordinates.
    fn origin(&self) -> Vector2 {
        if self.centered { -self.size() / 2.0 } else { Vector2::ZERO }
    }

    /// How many screen pixels one artboard unit takes: the node's scale, its parents' and the camera's.
    fn screen_scale(&self) -> f64 {
        let t = self.base().get_global_transform_with_canvas();
        let s = t.a.length().max(t.b.length()) as f64;
        if s.is_finite() && s > 0.0 { s.clamp(0.05, 16.0) } else { 1.0 }
    }

    /// The mouse in artboard units, and whether its left button is held.
    fn pointer_at(&self) -> (motif_core::kurbo::Point, bool) {
        let at = self.base().get_local_mouse_position() - self.origin();
        let down = Input::singleton().is_mouse_button_pressed(MouseButton::LEFT);
        (motif_core::kurbo::Point::new(at.x as f64, at.y as f64), down)
    }
}
