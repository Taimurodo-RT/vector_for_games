//! Drives PixaTool (1.58, an NW.js app) from Motif.
//!
//! PixaTool has no command line. It runs on Chromium, though, so it is started with a debugging
//! port, and a small script ([`pixatool.js`]) is put into its page: the script answers PixaTool's
//! file dialogs with our paths and presses its Import, Export and Load Preset buttons. PixaTool's
//! own files are not changed, and it runs with its own profile, so the copy you use by hand is
//! not affected.

use serde_json::{Value, json};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};
use tungstenite::{Message, WebSocket};

const SCRIPT: &str = include_str!("pixatool.js");

/// How long PixaTool may take to open, and to load or export one picture.
const START_TIMEOUT: Duration = Duration::from_secs(40);
const STEP_TIMEOUT: Duration = Duration::from_secs(20);

/// A running PixaTool under Motif's control. Closed when dropped.
pub struct PixaTool {
    child: Child,
    ws: WebSocket<TcpStream>,
    next_id: u64,
}

/// `Pixatool.exe` near the Motif folder: `tools/*/win64/Pixatool.exe` (or `tools/*/Pixatool.exe`)
/// in the folder of `start` or any folder above it.
pub fn find_exe(start: &Path) -> Option<PathBuf> {
    let name = if cfg!(windows) { "Pixatool.exe" } else { "Pixatool" };
    let arch = if cfg!(windows) { "win64" } else { "linux64" };
    for dir in start.ancestors() {
        let Ok(entries) = std::fs::read_dir(dir.join("tools")) else { continue };
        let mut found: Vec<PathBuf> = entries
            .flatten()
            .flat_map(|e| [e.path().join(arch).join(name), e.path().join(name)])
            .filter(|p| p.is_file() && p.to_string_lossy().to_lowercase().contains("pixatool"))
            .collect();
        found.sort();
        if let Some(p) = found.pop() {
            return Some(p);
        }
    }
    None
}

/// The PixaTool to use: `explicit` if given, else the `MOTIF_PIXATOOL` variable, else one found
/// next to the running program by [`find_exe`].
pub fn locate(explicit: Option<&Path>) -> Option<PathBuf> {
    if let Some(p) = explicit {
        return Some(p.to_path_buf());
    }
    if let Some(p) = std::env::var_os("MOTIF_PIXATOOL").filter(|p| !p.is_empty()) {
        return Some(p.into());
    }
    let exe = std::env::current_exe().ok()?;
    find_exe(exe.parent()?)
}

/// `file:///C:/…` with every byte outside plain ASCII percent-encoded.
pub fn file_url(path: &Path) -> String {
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

impl PixaTool {
    /// Starts PixaTool from `exe` and waits until it can take commands. With `visible` false its
    /// window is moved off screen.
    pub fn start(exe: &Path, visible: bool) -> Result<Self, String> {
        let port = TcpListener::bind("127.0.0.1:0").and_then(|l| l.local_addr()).map_err(|e| format!("нет свободного порта: {e}"))?.port();
        let profile = std::env::temp_dir().join("motif-pixatool-profile");
        let mut cmd = Command::new(exe);
        cmd.arg(format!("--remote-debugging-port={port}"))
            .arg(format!("--user-data-dir={}", profile.display()))
            // Keep it running at full speed while off screen or behind other windows.
            .args(["--disable-background-timer-throttling", "--disable-backgrounding-occluded-windows", "--disable-renderer-backgrounding"])
            .current_dir(exe.parent().unwrap_or(Path::new(".")))
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        if cfg!(target_os = "linux") {
            cmd.arg("--no-sandbox");
        }
        let child = cmd.spawn().map_err(|e| format!("не удалось запустить PixaTool ({}): {e}", exe.display()))?;
        let mut guard = KillOnError(Some(child));
        let start = Instant::now();
        let ws_url = loop {
            if let Some(url) = page_socket(port) {
                break url;
            }
            if start.elapsed() > START_TIMEOUT {
                return Err("PixaTool не открылся за 40 секунд".into());
            }
            if let Some(Ok(Some(status))) = guard.0.as_mut().map(|c| c.try_wait()) {
                return Err(format!("PixaTool закрылся сразу после запуска ({status})"));
            }
            std::thread::sleep(Duration::from_millis(200));
        };
        let stream = TcpStream::connect(("127.0.0.1", port)).map_err(|e| e.to_string())?;
        stream.set_read_timeout(Some(Duration::from_millis(500))).ok();
        let (ws, _) = tungstenite::client(ws_url.as_str(), stream).map_err(|e| format!("PixaTool не отвечает: {e}"))?;
        let mut tool = PixaTool { child: guard.0.take().expect("child"), ws, next_id: 1 };
        // Wait for the Construct runtime and its menu, then install the remote control.
        tool.wait("typeof cr_getC2Runtime === 'function' && cr_getC2Runtime().tickcount > 5 && typeof c2_callFunction === 'function'", START_TIMEOUT)?;
        tool.eval(SCRIPT)?;
        tool.wait("!!__motif.button('Export')", START_TIMEOUT)?;
        if !visible {
            tool.eval("nw.Window.get().moveTo(-4000, -4000)")?;
        }
        Ok(tool)
    }

    /// Loads PixaTool settings saved with its «Save Preset» button.
    pub fn load_preset(&mut self, preset: &Path) -> Result<(), String> {
        if !preset.is_file() {
            return Err(format!("нет файла пресета {}", preset.display()));
        }
        let before = self.eval("__motif.presets")?.as_u64().unwrap_or(0);
        self.press("LoadPreset", &file_url(preset))?;
        self.wait(&format!("__motif.presets > {before}"), STEP_TIMEOUT).map_err(|_| "PixaTool не прочитал пресет".to_string())?;
        // The settings apply on the next frames of PixaTool's loop.
        std::thread::sleep(Duration::from_millis(150));
        Ok(())
    }

    /// Runs one picture through PixaTool's current settings and saves the result.
    pub fn process(&mut self, input: &Path, output: &Path) -> Result<(), String> {
        let before = self.eval("__motif.loaded")?.as_u64().unwrap_or(0);
        self.press("", &file_url(input))?;
        self.wait(&format!("__motif.loaded > {before}"), STEP_TIMEOUT).map_err(|_| format!("PixaTool не открыл {}", input.display()))?;
        std::thread::sleep(Duration::from_millis(100));
        let _ = std::fs::remove_file(output);
        self.press("Export", &output.to_string_lossy())?;
        // PixaTool writes the file in one call; wait until it is there and stops growing.
        let start = Instant::now();
        let mut last = None;
        loop {
            let size = std::fs::metadata(output).ok().map(|m| m.len()).filter(|s| *s > 0);
            if size.is_some() && size == last {
                return Ok(());
            }
            last = size;
            if start.elapsed() > STEP_TIMEOUT {
                return Err(format!("PixaTool не сохранил {}", output.display()));
            }
            std::thread::sleep(Duration::from_millis(60));
        }
    }

    /// Queues the answer to the dialog a button opens, then presses the button.
    fn press(&mut self, button: &str, answer: &str) -> Result<(), String> {
        let ok = self.eval(&format!("__motif.answers.push({}); __motif.click({})", json!(answer), json!(button)))?;
        if ok != Value::Bool(true) {
            return Err(format!("в PixaTool не нашлась кнопка «{}»", if button.is_empty() { "Import" } else { button }));
        }
        Ok(())
    }

    fn wait(&mut self, condition: &str, timeout: Duration) -> Result<(), String> {
        let start = Instant::now();
        let check = format!("(() => {{ try {{ return !!({condition}); }} catch (e) {{ return false; }} }})()");
        while start.elapsed() < timeout {
            if self.eval(&check)? == Value::Bool(true) {
                return Ok(());
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        Err(format!("PixaTool не ответил вовремя ({condition})"))
    }

    /// Evaluates JavaScript in PixaTool's page and returns its value.
    fn eval(&mut self, expression: &str) -> Result<Value, String> {
        let id = self.next_id;
        self.next_id += 1;
        let msg = json!({ "id": id, "method": "Runtime.evaluate", "params": { "expression": expression, "returnByValue": true } });
        self.ws.send(Message::text(msg.to_string())).map_err(|e| format!("связь с PixaTool прервалась: {e}"))?;
        let start = Instant::now();
        loop {
            match self.ws.read() {
                Ok(Message::Text(t)) => {
                    let v: Value = serde_json::from_str(t.as_str()).unwrap_or_default();
                    if v["id"].as_u64() != Some(id) {
                        continue;
                    }
                    if let Some(e) = v["result"]["exceptionDetails"].as_object() {
                        let text = e.get("exception").and_then(|x| x["description"].as_str()).unwrap_or("ошибка скрипта");
                        return Err(format!("PixaTool: {text}"));
                    }
                    return Ok(v["result"]["result"]["value"].clone());
                }
                Ok(_) => {}
                Err(tungstenite::Error::Io(e)) if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => {}
                Err(e) => return Err(format!("связь с PixaTool прервалась: {e}")),
            }
            if start.elapsed() > STEP_TIMEOUT {
                return Err("PixaTool не отвечает".into());
            }
        }
    }
}

impl Drop for PixaTool {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Kills PixaTool if starting it fails half way.
struct KillOnError(Option<Child>);

impl Drop for KillOnError {
    fn drop(&mut self) {
        if let Some(c) = self.0.as_mut() {
            let _ = c.kill();
            let _ = c.wait();
        }
    }
}

/// The debugger address of PixaTool's main page, once it is up.
fn page_socket(port: u16) -> Option<String> {
    let mut s = TcpStream::connect(("127.0.0.1", port)).ok()?;
    s.set_read_timeout(Some(Duration::from_secs(2))).ok();
    write!(s, "GET /json/list HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n").ok()?;
    // The debugger keeps the connection open, so read exactly Content-Length bytes.
    let mut data = Vec::new();
    let mut buf = [0u8; 4096];
    let list: Value = loop {
        let n = s.read(&mut buf).ok()?;
        if n == 0 {
            return None;
        }
        data.extend_from_slice(&buf[..n]);
        let text = String::from_utf8_lossy(&data);
        let Some(end) = text.find("\r\n\r\n") else { continue };
        let len: usize = text[..end]
            .lines()
            .find_map(|l| l.split_once(':').filter(|(k, _)| k.eq_ignore_ascii_case("content-length")))
            .and_then(|(_, v)| v.trim().parse().ok())?;
        if data.len() >= end + 4 + len {
            break serde_json::from_slice(&data[end + 4..end + 4 + len]).ok()?;
        }
    };
    list.as_array()?
        .iter()
        .find(|t| t["type"] == "page" && t["url"].as_str().is_some_and(|u| u.ends_with("index.html")))
        .and_then(|t| t["webSocketDebuggerUrl"].as_str().map(String::from))
}

/// Pixelizes PNG files through PixaTool: each `inputs[i]` is saved as `outputs[i]`.
/// `progress` is called after each picture with the number done.
pub fn pixelize(exe: &Path, preset: Option<&Path>, inputs: &[PathBuf], outputs: &[PathBuf], mut progress: impl FnMut(usize)) -> Result<(), String> {
    let mut tool = PixaTool::start(exe, false)?;
    if let Some(p) = preset {
        tool.load_preset(p)?;
    }
    for (i, (input, output)) in inputs.iter().zip(outputs).enumerate() {
        tool.process(input, output)?;
        progress(i + 1);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_urls_escape_paths() {
        assert_eq!(file_url(Path::new("C:\\Игры\\a b.png")), "file:///C:/%D0%98%D0%B3%D1%80%D1%8B/a%20b.png");
        assert_eq!(file_url(Path::new("/tmp/x.png")), "file:///tmp/x.png");
    }

    #[test]
    fn finds_pixatool_in_a_tools_folder_above() {
        let root = std::env::temp_dir().join(format!("motif-find-{}", std::process::id()));
        let arch = if cfg!(windows) { "win64" } else { "linux64" };
        let name = if cfg!(windows) { "Pixatool.exe" } else { "Pixatool" };
        let exe = root.join("tools").join("NewPixatool_158").join(arch).join(name);
        std::fs::create_dir_all(exe.parent().unwrap()).unwrap();
        std::fs::write(&exe, b"").unwrap();
        std::fs::create_dir_all(root.join("Motif")).unwrap();
        assert_eq!(find_exe(&root.join("Motif")), Some(exe));
        std::fs::remove_dir_all(&root).ok();
    }

    /// Runs only where a PixaTool copy is given: `MOTIF_TEST_PIXATOOL=…/Pixatool cargo test`.
    #[test]
    fn drives_a_real_pixatool() {
        let Some(exe) = std::env::var_os("MOTIF_TEST_PIXATOOL").map(PathBuf::from) else { return };
        let dir = std::env::temp_dir().join(format!("motif-pixatool-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut px = tiny_skia::Pixmap::new(64, 32).unwrap();
        px.fill(tiny_skia::Color::from_rgba8(255, 90, 54, 255));
        let input = dir.join("in.png");
        px.save_png(&input).unwrap();
        let outputs = [dir.join("out1.png"), dir.join("out2.png")];
        let mut done = 0;
        pixelize(&exe, None, &[input.clone(), input.clone()], &outputs, |n| done = n).unwrap();
        assert_eq!(done, 2);
        for o in &outputs {
            let img = tiny_skia::Pixmap::load_png(o).unwrap();
            // PixaTool's default pixel size is 8.
            assert_eq!((img.width(), img.height()), (8, 4));
        }
        // A preset saved in PixaTool with pixel size 4.
        let preset = Path::new(env!("CARGO_MANIFEST_DIR")).join("testdata/px4.preset");
        pixelize(&exe, Some(&preset), &[input], &outputs[..1], |_| {}).unwrap();
        let img = tiny_skia::Pixmap::load_png(&outputs[0]).unwrap();
        assert_eq!((img.width(), img.height()), (16, 8));
        std::fs::remove_dir_all(&dir).ok();
    }
}
