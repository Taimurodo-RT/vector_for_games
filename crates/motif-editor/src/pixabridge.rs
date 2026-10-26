//! PixaTool in the background: «Пикселизировать кадры…» (frames of the current animation) and
//! the Godot export with every frame pixelized before it goes into the atlas.

use motif_core::{Document, Machine};
use motif_pixel::godot::{self, GodotExport};
use motif_pixel::pixatool;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, channel};

/// Settings kept between runs in `motif.ini` next to the program.
const SETTINGS: &str = "motif.ini";

pub enum Progress {
    /// How many frames PixaTool will get, once the job knows.
    Total(usize),
    Done(usize),
    /// The status line on success.
    Finished(Result<String, String>),
}

pub struct Job {
    rx: Receiver<Progress>,
    pub total: usize,
    pub done: usize,
}

#[derive(Clone, Debug, PartialEq)]
pub struct GodotLink {
    pub scene: PathBuf,
    pub dir: PathBuf,
    pub node: Option<String>,
}

#[derive(Default)]
pub struct Bridge {
    /// Pixatool.exe, picked by hand or found in the tools folder.
    pub exe: Option<PathBuf>,
    /// A preset saved in PixaTool with «Save Preset»; without one PixaTool's own defaults apply.
    pub preset: Option<PathBuf>,
    /// Run the Godot export's frames through PixaTool.
    pub godot: bool,
    /// Scenes exported to Godot, re-exported on every save: the scene file, the folder in the
    /// Godot project and the object exported (the whole scene when `None`).
    pub links: Vec<GodotLink>,
    pub job: Option<Job>,
}

impl Bridge {
    pub fn load() -> Self {
        let text = settings_path().and_then(|p| std::fs::read_to_string(p).ok()).unwrap_or_default();
        let mut b = Bridge::parse(&text);
        if b.exe.is_none() {
            b.exe = pixatool::locate(None).filter(|p| p.is_file());
        }
        b
    }

    /// The settings from `motif.ini` text; programs and presets that are gone are skipped.
    fn parse(text: &str) -> Self {
        let mut b = Bridge::default();
        for line in text.lines() {
            match line.split_once('=') {
                Some(("pixatool", v)) if Path::new(v.trim()).is_file() => b.exe = Some(v.trim().into()),
                Some(("pixatool-preset", v)) if Path::new(v.trim()).is_file() => b.preset = Some(v.trim().into()),
                Some(("godot-pixatool", v)) => b.godot = v.trim() == "1",
                Some(("godot-link", v)) => {
                    let mut parts = v.split('\t');
                    if let (Some(scene), Some(dir)) = (parts.next(), parts.next()) {
                        let node = parts.next().filter(|n| !n.is_empty()).map(String::from);
                        b.links.push(GodotLink { scene: scene.into(), dir: dir.into(), node });
                    }
                }
                _ => {}
            }
        }
        b
    }

    fn text(&self) -> String {
        let mut text = String::new();
        if let Some(p) = &self.exe {
            text += &format!("pixatool={}\n", p.display());
        }
        if let Some(p) = &self.preset {
            text += &format!("pixatool-preset={}\n", p.display());
        }
        if self.godot {
            text += "godot-pixatool=1\n";
        }
        for l in &self.links {
            text += &format!("godot-link={}\t{}\t{}\n", l.scene.display(), l.dir.display(), l.node.as_deref().unwrap_or(""));
        }
        text
    }

    pub fn save(&self) {
        let Some(path) = settings_path() else { return };
        let _ = std::fs::write(path, self.text());
    }

    /// The Godot folder the scene at `scene` is kept up to date in.
    pub fn link(&self, scene: &Path) -> Option<&GodotLink> {
        self.links.iter().find(|l| l.scene == scene)
    }

    /// Keeps `scene` exported into `dir` from now on (one folder per scene), or stops with `None`.
    pub fn set_link(&mut self, scene: &Path, target: Option<(PathBuf, Option<String>)>) {
        self.links.retain(|l| l.scene != scene);
        if let Some((dir, node)) = target {
            self.links.push(GodotLink { scene: scene.to_path_buf(), dir, node });
        }
        self.save();
    }

    /// Saves `frames` to a temporary folder and runs them through PixaTool on another thread,
    /// into `dir/NAME_0000.png`….
    pub fn start(&mut self, frames: &[tiny_skia::Pixmap], name: &str, dir: &Path, ctx: &egui::Context) -> Result<(), String> {
        let exe = self.exe.clone().ok_or("укажите Pixatool.exe")?;
        let tmp = std::env::temp_dir().join(format!("motif-pixel-{}", std::process::id()));
        std::fs::create_dir_all(&tmp).map_err(|e| format!("{}: {e}", tmp.display()))?;
        let mut inputs = Vec::new();
        let mut outputs = Vec::new();
        for (i, px) in frames.iter().enumerate() {
            let path = tmp.join(format!("{i:04}.png"));
            px.save_png(&path).map_err(|e| format!("{}: {e}", path.display()))?;
            inputs.push(path);
            outputs.push(dir.join(format!("{name}_{i:04}.png")));
        }
        let preset = self.preset.clone();
        let (tx, rx) = channel();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let result = pixatool::pixelize(&exe, preset.as_deref(), &inputs, &outputs, |n| {
                let _ = tx.send(Progress::Done(n));
                ctx.request_repaint();
            });
            std::fs::remove_dir_all(&tmp).ok();
            let n = inputs.len();
            let _ = tx.send(Progress::Finished(
                result.map(|()| format!("PixaTool обработал кадров: {n} → {}", outputs[0].parent().unwrap_or(Path::new("")).display())),
            ));
            ctx.request_repaint();
        });
        self.job = Some(Job { rx, total: frames.len(), done: 0 });
        Ok(())
    }

    /// The Godot export with PixaTool between baking and packing, on another thread: the scene
    /// is baked, its distinct frames pixelized and the atlas and scene written into `dir`.
    pub fn start_godot(
        &mut self,
        doc: Document,
        opts: GodotExport,
        dir: PathBuf,
        name: String,
        machine: Option<Machine>,
        ctx: &egui::Context,
    ) -> Result<(), String> {
        let exe = self.exe.clone().ok_or("укажите Pixatool.exe")?;
        let preset = self.preset.clone();
        let (tx, rx) = channel();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let send = |p| {
                let _ = tx.send(p);
                ctx.request_repaint();
            };
            let result = godot::bake_sheet_with(&doc, &opts, |frames| {
                send(Progress::Total(frames.len()));
                pixatool::pixelize_frames(&exe, preset.as_deref(), frames, |n| send(Progress::Done(n)))
            })
            .and_then(|sheet| {
                let files = godot::write_godot(&dir, &name, &sheet, machine.as_ref(), true)?;
                let tree = machine.as_ref().map_or(String::new(), |m| format!(", машина «{}» в AnimationTree", m.name));
                Ok(format!(
                    "Godot через PixaTool: {name}.tscn, анимаций {}, кадров {} по {}×{}{tree} — {} файлов в {}",
                    sheet.anims.len(),
                    sheet.cells.len(),
                    sheet.size.0,
                    sheet.size.1,
                    files.len(),
                    dir.display()
                ))
            });
            send(Progress::Finished(result));
        });
        self.job = Some(Job { rx, total: 0, done: 0 });
        Ok(())
    }

    /// Takes the news from a running job; returns the line for the status bar when it changes.
    pub fn poll(&mut self) -> Option<String> {
        let job = self.job.as_mut()?;
        let mut status = None;
        while let Ok(p) = job.rx.try_recv() {
            match p {
                Progress::Total(n) => {
                    job.total = n;
                    status = Some(format!("PixaTool: {n} кадров…"));
                }
                Progress::Done(n) => {
                    job.done = n;
                    status = Some(format!("PixaTool: кадр {n} из {}", job.total));
                }
                Progress::Finished(r) => {
                    self.job = None;
                    return Some(r.unwrap_or_else(|e| format!("PixaTool: {e}")));
                }
            }
        }
        status
    }
}

fn settings_path() -> Option<PathBuf> {
    Some(std::env::current_exe().ok()?.parent()?.join(SETTINGS))
}

/// The last two parts of a path, enough to recognise it in a narrow window.
pub fn short(path: &Path) -> String {
    let parts: Vec<_> = path.components().rev().take(2).map(|c| c.as_os_str().to_string_lossy().into_owned()).collect();
    parts.into_iter().rev().collect::<Vec<_>>().join("/")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn godot_links_survive_a_restart() {
        let mut b = Bridge { godot: true, ..Default::default() };
        b.links.push(GodotLink { scene: "C:\\Игры\\hero.msc".into(), dir: "C:\\game\\characters\\hero".into(), node: Some("hero".into()) });
        b.links.push(GodotLink { scene: "/tmp/ui.msc".into(), dir: "/tmp/game/ui".into(), node: None });
        let back = Bridge::parse(&b.text());
        assert!(back.godot);
        assert_eq!(back.links, b.links);
    }

    /// Runs only where a PixaTool copy is given: `MOTIF_TEST_PIXATOOL=…/Pixatool cargo test`.
    #[test]
    fn pixelizes_in_the_background() {
        let Some(exe) = std::env::var_os("MOTIF_TEST_PIXATOOL").map(PathBuf::from) else { return };
        let dir = std::env::temp_dir().join(format!("motif-bridge-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut frame = tiny_skia::Pixmap::new(64, 32).unwrap();
        frame.fill(tiny_skia::Color::from_rgba8(59, 91, 255, 255));
        let mut bridge = Bridge { exe: Some(exe), ..Default::default() };
        bridge.start(&[frame.clone(), frame], "Idle", &dir, &egui::Context::default()).unwrap();
        let start = std::time::Instant::now();
        let last = loop {
            let status = bridge.poll();
            if bridge.job.is_none() {
                break status.unwrap();
            }
            assert!(start.elapsed().as_secs() < 60, "PixaTool не закончил");
            std::thread::sleep(std::time::Duration::from_millis(50));
        };
        assert!(last.starts_with("PixaTool обработал кадров: 2"), "{last}");
        assert!(dir.join("Idle_0001.png").is_file());
        std::fs::remove_dir_all(&dir).ok();
    }

    /// Runs only where a PixaTool copy is given: `MOTIF_TEST_PIXATOOL=…/Pixatool cargo test`.
    #[test]
    fn exports_to_godot_through_pixatool() {
        let Some(exe) = std::env::var_os("MOTIF_TEST_PIXATOOL").map(PathBuf::from) else { return };
        let dir = std::env::temp_dir().join(format!("motif-bridge-godot-{}", std::process::id()));
        let src = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/machine.msc")).unwrap();
        let doc = motif_script::parse(&src).unwrap().doc;
        let machine = doc.machines.first().cloned();
        let opts = GodotExport { only: Some("hero".into()), ..GodotExport::default() };
        let mut bridge = Bridge { exe: Some(exe), ..Default::default() };
        bridge.start_godot(doc, opts, dir.clone(), "hero".into(), machine, &egui::Context::default()).unwrap();
        let start = std::time::Instant::now();
        let last = loop {
            let status = bridge.poll();
            if bridge.job.is_none() {
                break status.unwrap();
            }
            assert!(start.elapsed().as_secs() < 120, "PixaTool не закончил");
            std::thread::sleep(std::time::Duration::from_millis(50));
        };
        assert!(last.starts_with("Godot через PixaTool: hero.tscn"), "{last}");
        assert!(dir.join("hero.png").is_file() && dir.join("hero.tscn").is_file());
        std::fs::remove_dir_all(&dir).ok();
    }
}
