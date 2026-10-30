//! A Motif scene playing in a game, with no engine and no window: the board, the machine that
//! drives it, its inputs, sides and events, and the frame to draw. Games link it as a library
//! (Forge does); the Godot addon `MotifSprite` is built on it.
//!
//! Time comes only from the game: [`Player::advance`] moves it by the `dt` it is given, and the
//! player never reads a clock. A lone animation plays as a machine of one state, so it gets the
//! same events and simulation.

use motif_core::kurbo::Point;
use motif_core::{Document, Machine, MachinePlayer, State};
use motif_render::CpuOptions;
#[cfg(feature = "mesh")]
use motif_render::mesh::{Cutter, Piece};
use std::sync::Arc;
use tiny_skia::Pixmap;

#[derive(Debug)]
pub struct Player {
    doc: Document,
    /// The machine as the scene has it.
    plain: Machine,
    /// The machine as it plays from the side faced: `Walk_up` instead of `Walk`.
    machine: Machine,
    run: MachinePlayer,
    /// The side of the scene's `directions` faced: down at first; none without `directions`.
    side: Option<usize>,
    /// The pose shown now: what the pointer is tested against.
    shown: Arc<Document>,
    /// Counts the changes of the pose shown: a frame that did not change is not drawn again.
    pose: u64,
}

/// One pose to draw, apart from the player: it can be drawn on another thread while the game
/// goes on.
#[derive(Clone, Debug)]
pub struct Frame {
    doc: Arc<Document>,
    flipped: bool,
}

impl Frame {
    /// The pose drawn at `scale` pixels a unit, premultiplied RGBA; a flipped side turned over.
    pub fn render(&self, scale: f64, background: bool) -> Option<Pixmap> {
        let mut px = motif_render::render_cpu(&self.doc, CpuOptions { scale, antialias: true, background })?;
        if self.flipped {
            let w = px.width() as usize * 4;
            for row in px.data_mut().chunks_exact_mut(w) {
                row.as_chunks_mut::<4>().0.reverse();
            }
        }
        Some(px)
    }

    /// The pose as triangles and small pictures for `scale` pixels a unit, in artboard units,
    /// not turned over: Godot turns a flipped side over when it draws them.
    #[cfg(feature = "mesh")]
    pub fn pieces(&self, cutter: &mut Cutter, scale: f64) -> Vec<Piece> {
        cutter.pieces(&self.doc, scale)
    }

    /// Appends the pose to a Vello scene: `transform` maps artboard units to the target's pixels,
    /// so a game draws all its characters into one scene and renders it in one pass. A flipped
    /// side is turned over inside the artboard. With `background`, the artboard's colour first.
    #[cfg(feature = "vello")]
    pub fn scene(&self, scene: &mut motif_render::vello::Scene, transform: motif_core::kurbo::Affine, background: bool) {
        let turn = if self.flipped {
            motif_core::kurbo::Affine::new([-1.0, 0.0, 0.0, 1.0, self.doc.artboard.width, 0.0])
        } else {
            motif_core::kurbo::Affine::IDENTITY
        };
        motif_render::draw_pose(scene, &self.doc, transform * turn, background);
    }

    pub fn flipped(&self) -> bool {
        self.flipped
    }
}

/// What happened in one [`Player::advance`].
#[derive(Debug, Default, PartialEq)]
pub struct Step {
    /// Animation events passed, in order.
    pub events: Vec<String>,
    /// The state entered, when the machine moved.
    pub state: Option<String>,
}

impl Player {
    /// Board `board` (a name or a number from 1; empty — the first) of scene `src`, played by
    /// machine `machine`, or by animation `animation` alone. With neither, the board's first
    /// machine, else its first animation, else the still board.
    pub fn load(src: &str, base: Option<&std::path::Path>, board: &str, machine: &str, animation: &str) -> Result<Player, String> {
        let boards = motif_script::parse_boards(src, base).map_err(|errs| errs.iter().map(|e| e.to_string()).collect::<Vec<_>>().join("\n"))?;
        let names = || boards.iter().map(|s| format!("«{}»", s.doc.artboard.name)).collect::<Vec<_>>().join(", ");
        let at = match board.parse::<usize>() {
            _ if board.is_empty() => Some(0),
            Ok(n) if (1..=boards.len()).contains(&n) => Some(n - 1),
            _ => boards.iter().position(|s| s.doc.artboard.name == board),
        };
        let doc = at.and_then(|i| boards.get(i)).map(|s| s.doc.clone()).ok_or_else(|| format!("нет холста «{board}»; холсты: {}", names()))?;
        let machine = if !animation.is_empty() {
            doc.animation(animation).ok_or_else(|| {
                let all: Vec<&str> = doc.animations.iter().map(|a| a.name.as_str()).collect();
                format!("нет анимации «{animation}»; анимации: {}", all.join(", "))
            })?;
            Self::alone(animation)
        } else if !machine.is_empty() {
            doc.machine(machine).cloned().ok_or_else(|| {
                let all: Vec<&str> = doc.machines.iter().map(|m| m.name.as_str()).collect();
                format!("нет машины «{machine}»; машины: {}", all.join(", "))
            })?
        } else if let Some(m) = doc.machines.first() {
            m.clone()
        } else {
            doc.animations.first().map_or_else(|| Machine { name: String::new(), ..Self::alone("") }, |a| Self::alone(&a.name))
        };
        let run = MachinePlayer::new(&machine);
        let side = doc.directions.as_ref().map(|_| 0);
        let plain = machine.clone();
        let shown = Arc::new(doc.clone());
        let mut p = Player { doc, plain, machine, run, side, shown, pose: 0 };
        p.turn(side);
        Ok(p)
    }

    /// A machine of one state playing `anim`.
    fn alone(anim: &str) -> Machine {
        Machine {
            name: anim.to_string(),
            inputs: Vec::new(),
            listeners: Vec::new(),
            states: if anim.is_empty() { Vec::new() } else { vec![State { name: anim.to_string(), anim: anim.to_string(), pos: None }] },
            transitions: Vec::new(),
            any_pos: None,
        }
    }

    /// The artboard's width and height.
    pub fn background(&self) -> motif_core::Color {
        self.doc.artboard.background
    }

    pub fn size(&self) -> (f64, f64) {
        (self.doc.artboard.width, self.doc.artboard.height)
    }

    /// Passes the pointer (in artboard units, as the scene is shown) to the machine's `listen` lines.
    pub fn pointer(&mut self, at: Option<Point>, down: bool) {
        if !self.machine.listeners.is_empty() {
            // A flipped side is drawn turned over: the pointer is turned over to meet it.
            let at = at.map(|p| if self.flipped() { Point::new(self.doc.artboard.width - p.x, p.y) } else { p });
            self.run.pointer(&self.machine, &self.shown, at, down);
        }
    }

    /// The names of the scene's sides (down, down_right, right…); empty without `directions`.
    pub fn sides(&self) -> Vec<String> {
        self.doc.directions.as_ref().map_or_else(Vec::new, |d| d.names().iter().map(|n| n.to_string()).collect())
    }

    /// The side faced now, by name; empty without `directions`.
    pub fn side(&self) -> String {
        match (&self.doc.directions, self.side) {
            (Some(d), Some(k)) => d.names()[k].to_string(),
            _ => String::new(),
        }
    }

    /// Whether the side faced is drawn as another one turned over (a left side with `mirror`).
    pub fn flipped(&self) -> bool {
        matches!((&self.doc.directions, self.side), (Some(d), Some(k)) if d.shot(k).1)
    }

    /// Faces the side named `name`. Returns whether the side changed.
    pub fn set_side(&mut self, name: &str) -> Result<bool, String> {
        let d = self.doc.directions.as_ref().ok_or("в сцене нет строки directions")?;
        let k = d.find(name).ok_or_else(|| format!("нет стороны «{name}»; стороны: {}", d.names().join(", ")))?;
        Ok(self.turn(Some(k)))
    }

    /// Faces the side nearest to looking along `(x, y)`, `y` down. Returns whether the side changed.
    pub fn face_toward(&mut self, x: f64, y: f64) -> bool {
        let k = self.doc.directions.as_ref().and_then(|d| d.nearest(x, y));
        k.is_some() && self.turn(k)
    }

    /// Turns to side `k`. The state and its time stay: the walk goes on from the same step, and
    /// no event is sent, since time does not move.
    fn turn(&mut self, k: Option<usize>) -> bool {
        let changed = k != self.side;
        self.side = k;
        self.machine = match k {
            Some(k) => self.plain.facing(&self.doc, k),
            None => self.plain.clone(),
        };
        self.repose();
        // A mirrored side shows the same pose turned over: a new picture all the same.
        self.pose += changed as u64;
        changed
    }

    /// Poses the scene as it is now, seen from the side faced.
    fn repose(&mut self) {
        let mut shown = self.run.pose(&self.machine, &self.doc);
        if let (Some(d), Some(k)) = (&self.doc.directions, self.side) {
            shown.face(d.shot(k).0);
            shown.rig();
        }
        // A held pose, a paused or finished clip: the same picture, nothing to draw again.
        if !same_picture(&mut shown, &self.shown) {
            self.shown = Arc::new(shown);
            self.pose += 1;
        }
    }

    /// Changes whenever the pose shown changes.
    pub fn pose(&self) -> u64 {
        self.pose
    }

    /// The pose shown now, to draw.
    pub fn frame(&self) -> Frame {
        Frame { doc: self.shown.clone(), flipped: self.flipped() }
    }

    /// The animation playing now: the state's, from the side faced (`Walk_up`).
    pub fn animation(&self) -> String {
        self.machine.states.get(self.run.state).map_or(String::new(), |s| s.anim.clone())
    }

    /// How far the state's animation has played, in seconds.
    pub fn time(&self) -> f64 {
        self.run.time
    }

    /// Moves time on by `dt` seconds.
    pub fn advance(&mut self, dt: f64) -> Step {
        if self.machine.states.is_empty() {
            return Step::default();
        }
        let before = self.run.state;
        let taken = self.run.advance(&self.machine, &self.doc, dt.max(0.0));
        self.repose();
        let state = (taken.is_some() || self.run.state != before).then(|| self.state());
        Step { events: self.run.events.clone(), state }
    }

    /// The state now, or the animation playing.
    pub fn state(&self) -> String {
        self.machine.states.get(self.run.state).map_or(String::new(), |s| s.name.clone())
    }

    pub fn inputs(&self) -> Vec<String> {
        self.machine.inputs.iter().map(|i| i.name.clone()).collect()
    }

    pub fn get(&self, input: &str) -> Option<f64> {
        self.machine.input(input).and_then(|i| self.run.values.get(i).copied())
    }

    pub fn set(&mut self, input: &str, value: f64) -> Result<(), String> {
        self.known(input)?;
        self.run.set(&self.machine, input, value);
        Ok(())
    }

    pub fn fire(&mut self, input: &str) -> Result<(), String> {
        self.known(input)?;
        self.run.fire(&self.machine, input);
        Ok(())
    }

    fn known(&self, input: &str) -> Result<(), String> {
        match self.machine.input(input) {
            Some(_) => Ok(()),
            None => Err(format!("у машины «{}» нет входа «{input}»; входы: {}", self.machine.name, self.inputs().join(", "))),
        }
    }

    /// Goes to `state` at once.
    pub fn jump(&mut self, state: &str) -> Result<(), String> {
        let k = self.machine.state(state).ok_or_else(|| {
            let all: Vec<&str> = self.machine.states.iter().map(|s| s.name.as_str()).collect();
            format!("у машины «{}» нет состояния «{state}»; состояния: {}", self.machine.name, all.join(", "))
        })?;
        self.run.jump(k);
        self.repose();
        Ok(())
    }

    pub fn restart(&mut self) {
        self.run = MachinePlayer::new(&self.machine);
        self.repose();
    }

    /// The pose now drawn at `scale` pixels a unit, premultiplied RGBA; a flipped side turned over.
    pub fn render(&self, scale: f64, background: bool) -> Option<Pixmap> {
        self.frame().render(scale, background)
    }
}

/// Whether `new` looks as `old` does. Time runs on in every pose, but only boiling lines are
/// drawn by it: they count as changed when their tick does.
fn same_picture(new: &mut Document, old: &Document) -> bool {
    let time = new.time;
    let boiled = new.nodes.iter().any(|n| n.brush.seed_at(&n.id, time) != n.brush.seed_at(&n.id, old.time));
    new.time = old.time;
    let same = !boiled && *new == *old;
    new.time = time;
    same
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCENE: &str = r#"
artboard "Капля" 100x80
ellipse ball at 50 40 size 20 20 fill #5ee0a0
anim Idle 1s loop {
  ball.x 0: 50 | 1: 50
  event blink 0.5
}
anim Jump 0.5s {
  ball.y 0: 40 | 0.5: 10
}
machine Hero {
  input jump trigger
  input speed 0
  state Idle
  state Jump
  any -> Jump when jump
  Jump -> Idle at end
}
"#;

    #[test]
    fn a_machine_plays_and_reports_states_and_events() {
        let mut p = Player::load(SCENE, None, "", "", "").unwrap();
        assert_eq!((p.size(), p.state()), ((100.0, 80.0), "Idle".to_string()));
        assert_eq!(p.inputs(), ["jump", "speed"]);
        let step = p.advance(0.6);
        assert_eq!(step, Step { events: vec!["blink".into()], state: None });
        p.fire("jump").unwrap();
        assert_eq!(p.advance(0.01).state.as_deref(), Some("Jump"));
        for _ in 0..60 {
            p.advance(0.01);
        }
        assert_eq!(p.state(), "Idle", "back at the end of Jump");
        p.set("speed", 2.0).unwrap();
        assert_eq!(p.get("speed"), Some(2.0));
        assert!(p.set("fly", 1.0).unwrap_err().contains("jump, speed"));
        p.jump("Jump").unwrap();
        assert_eq!(p.state(), "Jump");
        p.restart();
        assert_eq!((p.state(), p.get("speed")), ("Idle".to_string(), Some(0.0)));
        let px = p.render(2.0, false).unwrap();
        assert_eq!((px.width(), px.height()), (200, 160));
    }

    /// Plays `godot-addon/test/sides.txt` and writes the log line of each step, the same way
    /// `godot-addon/test/test.gd` does through the `MotifSprite` node.
    fn sides_log(script: &str) -> String {
        let src = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../examples/directions.msc")).unwrap();
        let mut p = Player::load(&src, None, "", "Walker", "").unwrap();
        let mut log = String::new();
        for line in script.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with('#')) {
            let words: Vec<&str> = line.split_whitespace().collect();
            let num = |i: usize| words[i].parse::<f64>().unwrap();
            let events = match words[0] {
                "advance" => p.advance(num(1)).events,
                "side" => {
                    p.set_side(words[1]).unwrap();
                    Vec::new()
                }
                "facing" => {
                    p.face_toward(num(1), num(2));
                    Vec::new()
                }
                "set" => {
                    p.set(words[1], num(2)).unwrap();
                    Vec::new()
                }
                other => panic!("нет шага {other}"),
            };
            let flip = if p.flipped() { " flipped" } else { "" };
            log += &format!("{line} | {} {} {}{flip} t={:.3} events=[{}]\n", p.state(), p.animation(), p.side(), p.time(), events.join(", "));
        }
        log
    }

    #[test]
    fn turning_keeps_the_step_and_sends_no_events() {
        let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../godot-addon/test/");
        let log = sides_log(&std::fs::read_to_string(format!("{dir}sides.txt")).unwrap());
        if std::env::var_os("MOTIF_BLESS").is_some() {
            std::fs::write(format!("{dir}sides.expected"), &log).unwrap();
        }
        assert_eq!(log, std::fs::read_to_string(format!("{dir}sides.expected")).unwrap());
        // The rules the log shows, said out loud.
        for line in log.lines().filter(|l| !l.starts_with("advance")) {
            assert!(line.ends_with("events=[]"), "a turn sends nothing: {line}");
        }
        let at = |step: &str| log.lines().position(|l| l.starts_with(step)).unwrap();
        let lines: Vec<&str> = log.lines().collect();
        let time = |l: &str| l.split(" t=").nth(1).unwrap().split(' ').next().unwrap().to_string();
        let up = at("side up");
        assert_eq!(time(lines[up]), time(lines[up - 1]), "the walk goes on from the same step");
        assert!(lines[up].contains("Walk Walk_up up"), "{}", lines[up]);
        assert!(lines[at("facing -1 0")].contains("Walk Walk left flipped"), "{log}");
    }

    #[test]
    fn a_flipped_side_is_drawn_and_pointed_at_turned_over() {
        let src = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../examples/directions.msc")).unwrap();
        let mut p = Player::load(&src, None, "", "", "").unwrap();
        assert_eq!((p.side(), p.sides().len()), ("down".to_string(), 8));
        p.set_side("right").unwrap();
        let right = p.render(0.5, false).unwrap();
        assert!(p.set_side("left").unwrap() && p.flipped());
        assert!(!p.set_side("left").unwrap(), "the same side again is no turn");
        let left = p.render(0.5, false).unwrap();
        let w = right.width() as usize;
        for y in (0..right.height() as usize).step_by(7) {
            for x in (0..w).step_by(5) {
                assert_eq!(right.pixels()[y * w + x], left.pixels()[y * w + (w - 1 - x)], "{x},{y}");
            }
        }
        assert!(p.set_side("sideways").unwrap_err().contains("down, down_right"));
        assert!(!p.face_toward(0.0, 0.0), "no look, no turn");
        let mut plain = Player::load(SCENE, None, "", "", "").unwrap();
        assert!(plain.set_side("up").is_err() && plain.side().is_empty() && plain.sides().is_empty());
    }

    #[test]
    fn an_animation_plays_alone_and_mistakes_are_named() {
        let mut p = Player::load(SCENE, None, "1", "", "Jump").unwrap();
        assert_eq!(p.state(), "Jump");
        p.advance(0.5);
        assert!(p.inputs().is_empty());
        assert!(Player::load(SCENE, None, "Меню", "", "").unwrap_err().contains("«Капля»"));
        assert!(Player::load(SCENE, None, "", "Boss", "").unwrap_err().contains("Hero"));
        assert!(Player::load(SCENE, None, "", "", "Run").unwrap_err().contains("Idle, Jump"));
        assert!(Player::load("rect", None, "", "", "").is_err());
    }

    #[test]
    fn a_still_pose_is_not_drawn_again_and_a_turn_always_is() {
        let mut p = Player::load(SCENE, None, "", "", "").unwrap();
        let first = p.pose();
        // Idle keeps the ball where it is: the same picture, frame after frame.
        p.advance(0.1);
        p.advance(0.1);
        assert_eq!(p.pose(), first);
        p.fire("jump").unwrap();
        // The jump starts from where the ball is and lifts it on the next step.
        p.advance(0.1);
        p.advance(0.1);
        assert_eq!(p.state(), "Jump");
        assert_ne!(p.pose(), first);
        let walker = std::fs::read_to_string("../../examples/directions.msc").unwrap();
        let mut w = Player::load(&walker, None, "", "", "").unwrap();
        w.set_side("right").unwrap();
        let right = w.pose();
        // The left side is the right one turned over: the same pose, another picture.
        w.set_side("left").unwrap();
        assert!(w.flipped() && w.pose() != right);
        // Boiling lines redraw with time though nothing moves.
        let boil = "artboard \"Кипение\" 100x80\nrect r at 50 40 size 40 30 fill #5ee0a0 rough 3 40 boil 8\nanim Idle 1s loop {\n  r.x 0: 50 | 1: 50\n}\n";
        let mut b = Player::load(boil, None, "", "", "").unwrap();
        b.advance(0.01);
        let before = b.pose();
        b.advance(0.2);
        assert_ne!(b.pose(), before);
    }
}

/// The frame drawn by Vello on a video card matches the frame drawn on the CPU. Needs an adapter
/// (a software one such as lavapipe will do): without one the check says so and passes, unless
/// `MOTIF_NEED_GPU` is set.
#[cfg(all(test, feature = "vello"))]
mod vello_tests {
    use super::*;
    use motif_core::kurbo::Affine;
    use motif_render::vello::{self, AaConfig, AaSupport, RenderParams, Renderer, RendererOptions, wgpu};

    fn gpu_render(frame: &Frame, scale: f64, width: u32, height: u32) -> Option<Vec<u8>> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env());
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default())).ok()?;
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default())).ok()?;
        let mut renderer = Renderer::new(&device, RendererOptions { antialiasing_support: AaSupport::area_only(), ..RendererOptions::default() }).ok()?;
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let mut scene = vello::Scene::new();
        frame.scene(&mut scene, Affine::scale(scale), true);
        let params = RenderParams { base_color: vello::peniko::Color::from_rgba8(0, 0, 0, 255), width, height, antialiasing_method: AaConfig::Area };
        renderer.render_to_texture(&device, &queue, &scene, &view, &params).ok()?;
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: (width * height * 4) as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo { texture: &texture, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(width * 4), rows_per_image: Some(height) },
            },
            wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
        );
        queue.submit([encoder.finish()]);
        buffer.map_async(wgpu::MapMode::Read, .., |_| {});
        device.poll(wgpu::PollType::wait_indefinitely()).ok()?;
        let data = buffer.slice(..).get_mapped_range().ok()?.to_vec();
        Some(data)
    }

    #[test]
    fn a_frame_on_the_video_card_is_the_frame_on_the_cpu() {
        let src = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../examples/directions.msc")).unwrap();
        let mut p = Player::load(&src, None, "", "", "").unwrap();
        p.set("speed", 1.0).unwrap();
        p.advance(0.3);
        for side in ["right", "left"] {
            p.set_side(side).unwrap();
            let frame = p.frame();
            let (w, h) = p.size();
            // 256 pixels wide: rows copied off the card are whole multiples of 256 bytes.
            let scale = 256.0 / w;
            let cpu = frame.render(scale, true).unwrap();
            let (width, height) = (256, ((h * scale) as u32).min(cpu.height()));
            let Some(gpu) = gpu_render(&frame, scale, width, height) else {
                // CI sets MOTIF_NEED_GPU where it installs a software card: there a skip is a failure.
                assert!(std::env::var_os("MOTIF_NEED_GPU").is_none(), "нет видеокарты, а MOTIF_NEED_GPU требует её");
                eprintln!("нет видеокарты (даже программной): сравнение пропущено");
                return;
            };
            let mut off = 0usize;
            for y in 0..height as usize {
                for x in 0..width as usize {
                    let c = cpu.pixels()[y * cpu.width() as usize + x].demultiply();
                    let g = &gpu[(y * width as usize + x) * 4..][..3];
                    let d = (c.red() as i32 - g[0] as i32).abs().max((c.green() as i32 - g[1] as i32).abs()).max((c.blue() as i32 - g[2] as i32).abs());
                    off += (d > 24) as usize;
                }
            }
            // Only the soft edges may differ: Vello and tiny-skia smooth them each their own way.
            let share = off as f64 / (width * height) as f64;
            eprintln!("{side}: {:.3}% of pixels differ", share * 100.0);
            assert!(share < 0.01, "{side}: {:.2}% of pixels differ", share * 100.0);
        }
    }
}
