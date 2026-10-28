//! Motif in Godot 4 without baking: the `MotifSprite` node reads a `.msc` scene and plays it in
//! the game with the same core as the editor, so bones, soft bodies, morphs, 3D turns, the state
//! machine and its events behave exactly as they do in Motif. Each frame the pose is drawn on the
//! CPU at the size the node has on screen, so it stays sharp when the camera zooms in.

use godot::classes::canvas_item_material::BlendMode;
use godot::classes::image::Format;
use godot::classes::{CanvasItemMaterial, FileAccess, INode2D, Image, ImageTexture, Input, Node2D};
use godot::global::MouseButton;
use godot::prelude::*;

mod player;

pub use player::{Player, Step};

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
            pointer: true,
            facing: Vector2::DOWN,
            side: GString::new(),
            player: None,
            error: None,
            texture: None,
            drawn: (0, 0),
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
        if self.dirty {
            self.base_mut().queue_redraw();
        }
    }

    fn draw(&mut self) {
        self.dirty = false;
        let Some(player) = &self.player else { return };
        let scale = self.screen_scale() * self.resolution.clamp(0.25, 4.0);
        let Some(px) = player.render(scale, self.background) else { return };
        let (w, h) = (px.width() as i32, px.height() as i32);
        let data = PackedByteArray::from(px.data());
        let Some(image) = Image::create_from_data(w, h, false, Format::RGBA8, &data) else { return };
        match &mut self.texture {
            Some(t) if self.drawn == (w, h) => t.update(&image),
            _ => self.texture = ImageTexture::create_from_image(&image),
        }
        self.drawn = (w, h);
        let rect = Rect2::new(self.origin(), self.size());
        if let Some(t) = self.texture.clone() {
            self.base_mut().draw_texture_rect(&t, rect, false);
        }
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

    /// Reads the scene file again (after it was edited in Motif).
    #[func]
    fn reload(&mut self) {
        self.player = None;
        self.error = None;
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

/// A file of the project (`res://…`), packed or not.
fn read_res(path: &std::path::Path) -> Option<Vec<u8>> {
    // `res://scenes` joined with a font name gets a backslash on Windows.
    let path = GString::from(path.to_str()?.replace('\\', "/").as_str());
    FileAccess::file_exists(&path).then(|| FileAccess::get_file_as_bytes(&path).to_vec())
}

impl MotifSprite {
    fn load(&self) -> Result<Player, String> {
        let path = self.scene.to_string();
        if !FileAccess::file_exists(&self.scene) {
            return Err(format!("нет файла {path}"));
        }
        let src = FileAccess::get_file_as_string(&self.scene).to_string();
        // Fonts next to the scene are read through Godot, so they are found in an exported
        // game's pack too.
        motif_script::fonts::set_reader(read_res);
        let base = path.rfind('/').map(|i| std::path::PathBuf::from(&path[..i]));
        Player::load(&src, base.as_deref(), &self.board.to_string(), &self.machine.to_string(), &self.animation.to_string())
    }

    /// Moves time on by `dt`, sending the events passed and the state entered.
    fn step(&mut self, dt: f64) {
        let Some(player) = &mut self.player else { return };
        let step = player.advance(dt);
        for name in &step.events {
            self.base_mut().emit_signal("event", &[name.to_variant()]);
        }
        if let Some(state) = step.state {
            self.base_mut().emit_signal("state_changed", &[state.to_variant()]);
        }
        self.dirty = true;
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
