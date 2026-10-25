//! «Пикселизировать кадры…»: frames of the current animation through PixaTool, in the background.

use motif_pixel::pixatool;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, channel};

/// Settings kept between runs in `motif.ini` next to the program.
const SETTINGS: &str = "motif.ini";

pub enum Progress {
    Done(usize),
    Finished(Result<(), String>),
}

pub struct Job {
    rx: Receiver<Progress>,
    pub total: usize,
    pub done: usize,
    pub dir: PathBuf,
}

#[derive(Default)]
pub struct Bridge {
    /// Pixatool.exe, picked by hand or found in the tools folder.
    pub exe: Option<PathBuf>,
    /// A preset saved in PixaTool with «Save Preset»; without one PixaTool's own defaults apply.
    pub preset: Option<PathBuf>,
    pub job: Option<Job>,
}

impl Bridge {
    pub fn load() -> Self {
        let mut b = Bridge::default();
        if let Some(text) = settings_path().and_then(|p| std::fs::read_to_string(p).ok()) {
            for line in text.lines() {
                match line.split_once('=') {
                    Some(("pixatool", v)) if Path::new(v.trim()).is_file() => b.exe = Some(v.trim().into()),
                    Some(("pixatool-preset", v)) if Path::new(v.trim()).is_file() => b.preset = Some(v.trim().into()),
                    _ => {}
                }
            }
        }
        if b.exe.is_none() {
            b.exe = pixatool::locate(None).filter(|p| p.is_file());
        }
        b
    }

    pub fn save(&self) {
        let Some(path) = settings_path() else { return };
        let mut text = String::new();
        if let Some(p) = &self.exe {
            text += &format!("pixatool={}\n", p.display());
        }
        if let Some(p) = &self.preset {
            text += &format!("pixatool-preset={}\n", p.display());
        }
        let _ = std::fs::write(path, text);
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
            let _ = tx.send(Progress::Finished(result));
            ctx.request_repaint();
        });
        self.job = Some(Job { rx, total: frames.len(), done: 0, dir: dir.to_path_buf() });
        Ok(())
    }

    /// Takes the news from a running job; returns the line for the status bar when it changes.
    pub fn poll(&mut self) -> Option<String> {
        let job = self.job.as_mut()?;
        let mut status = None;
        while let Ok(p) = job.rx.try_recv() {
            match p {
                Progress::Done(n) => {
                    job.done = n;
                    status = Some(format!("PixaTool: кадр {n} из {}", job.total));
                }
                Progress::Finished(r) => {
                    let msg = match r {
                        Ok(()) => format!("PixaTool обработал кадров: {} → {}", job.total, job.dir.display()),
                        Err(e) => format!("PixaTool: {e}"),
                    };
                    self.job = None;
                    return Some(msg);
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
}
