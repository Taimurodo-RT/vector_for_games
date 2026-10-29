//! File transactions and recovery copies. They never rewrite a document during autosave.
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

static SEQUENCE: AtomicU64 = AtomicU64::new(0);
fn stamp() -> String {
    format!(
        "{:020}-{:010}-{:06}",
        SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis(),
        std::process::id(),
        SEQUENCE.fetch_add(1, Ordering::Relaxed)
    )
}

/// Stage next to the target, flush, then atomically replace. A failed rename preserves the target.
pub fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), String> {
    atomic_write_with(path, bytes, &mut |a, b| fs::rename(a, b))
}
fn atomic_write_with(path: &Path, bytes: &[u8], rename: &mut dyn FnMut(&Path, &Path) -> std::io::Result<()>) -> Result<(), String> {
    let parent = path.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(Path::new("."));
    let temp = parent.join(format!(".motif-{}.tmp", stamp()));
    let result = (|| {
        let mut f = fs::OpenOptions::new().write(true).create_new(true).open(&temp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
        drop(f);
        rename(&temp, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result.map_err(|e| format!("{}: {e}", path.display()))
}

pub fn history_dir(path: &Path) -> PathBuf {
    path.parent().unwrap_or(Path::new(".")).join(".motif-history").join(path.file_name().unwrap_or_default())
}

/// Compare with the file we loaded before writing; never silently overwrite an external edit.
pub fn save_scene(path: &Path, text: &str, expected: Option<&str>) -> Result<(), String> {
    let before = match fs::read_to_string(path) {
        Ok(s) => Some(s),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => return Err(e.to_string()),
    };
    if let Some(expected) = expected
        && before.as_deref() != Some(expected)
        && before.as_deref() != Some(text)
    {
        return Err("Файл изменён или удалён другой программой. Используйте «Сохранить как…», чтобы сохранить обе версии.".into());
    }
    if before.as_deref() == Some(text) {
        return Ok(());
    }
    if let Some(old) = &before {
        let history = history_dir(path);
        fs::create_dir_all(&history).map_err(|e| format!("Не удалось сохранить предыдущую версию: {e}"))?;
        atomic_write(&history.join(format!("{}.msc", stamp())), old.as_bytes())?;
    }
    // Catch edits made while the backup was written too.
    let now = match fs::read_to_string(path) {
        Ok(s) => Some(s),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => return Err(e.to_string()),
    };
    if now != before {
        return Err("Файл изменился во время сохранения. Повторите через «Сохранить как…».".into());
    }
    atomic_write(path, text.as_bytes())?;
    // Cleanup is best effort, only after the new document is safe. Never follow directories.
    if let Ok(entries) = fs::read_dir(history_dir(path)) {
        let mut files: Vec<_> = entries
            .flatten()
            .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|x| x == "msc"))
            .collect();
        files.sort();
        for p in files.iter().take(files.len().saturating_sub(10)) {
            let _ = fs::remove_file(p);
        }
    }
    Ok(())
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Record {
    pub schema: u32,
    pub path: Option<PathBuf>,
    pub text: String,
    pub saved_text: String,
    pub board: usize,
    pub created: u64,
}
impl Record {
    pub fn new(path: Option<PathBuf>, text: String, saved_text: String, board: usize) -> Self {
        Self { schema: 1, path, text, saved_text, board, created: SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs() }
    }
}

pub fn recovery_dir() -> PathBuf {
    std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("XDG_STATE_HOME").map(PathBuf::from))
        .or_else(|| std::env::var_os("HOME").map(|p| PathBuf::from(p).join(".local/state")))
        .unwrap_or_else(std::env::temp_dir)
        .join("Motif/recovery")
}

pub struct Recovery {
    pub dir: PathBuf,
    current: PathBuf,
    last: Option<(Option<PathBuf>, String, usize)>,
}
impl Recovery {
    pub fn new(dir: PathBuf) -> Self {
        Self { current: dir.join(format!("{}.json", stamp())), dir, last: None }
    }
    pub fn write(&mut self, record: &Record) -> Result<(), String> {
        let key = (record.path.clone(), record.text.clone(), record.board);
        if self.last.as_ref() == Some(&key) {
            return Ok(());
        }
        fs::create_dir_all(&self.dir).map_err(|e| e.to_string())?;
        let bytes = serde_json::to_vec(record).map_err(|e| e.to_string())?;
        if bytes.len() > 64 * 1024 * 1024 {
            return Err("Копия превышает 64 МиБ. Сохраните документ вручную.".into());
        }
        atomic_write(&self.current, &bytes)?;
        self.last = Some(key);
        Ok(())
    }
    pub fn clear(&mut self) {
        let _ = fs::remove_file(&self.current);
        self.last = None;
    }
    pub fn entries(&self) -> Vec<Record> {
        let Ok(entries) = fs::read_dir(&self.dir) else {
            return Vec::new();
        };
        let mut out: Vec<Record> = entries
            .flatten()
            .filter(|e| e.path() != self.current)
            .filter(|e| e.file_type().is_ok_and(|t| t.is_file()) && e.path().extension().is_some_and(|x| x == "json"))
            .filter(|e| e.metadata().is_ok_and(|m| m.len() <= 64 * 1024 * 1024))
            .filter_map(|e| fs::read(e.path()).ok().and_then(|b| serde_json::from_slice::<Record>(&b).ok()))
            .filter(|r| r.schema == 1 && r.text != r.saved_text && !r.path.as_ref().is_some_and(|p| fs::read_to_string(p).is_ok_and(|s| s == r.text)))
            .collect();
        out.sort_by_key(|r| std::cmp::Reverse(r.created));
        out
    }
}

pub fn history(path: &Path) -> Vec<Record> {
    let Ok(entries) = fs::read_dir(history_dir(path)) else {
        return Vec::new();
    };
    let base = fs::read_to_string(path).unwrap_or_default();
    let mut files: Vec<_> = entries
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "msc"))
        .collect();
    files.sort();
    files.into_iter().rev().filter_map(|p| fs::read_to_string(p).ok()).map(|text| Record::new(Some(path.to_path_buf()), text, base.clone(), 0)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Temp(PathBuf);
    impl Temp {
        fn new() -> Self {
            let p = std::env::temp_dir().join(format!("motif-storage-{}", stamp()));
            fs::create_dir_all(&p).unwrap();
            Self(p)
        }
    }
    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    #[test]
    fn failed_replace_preserves_original_and_cleans_temp() {
        let d = Temp::new();
        let p = d.0.join("scene.msc");
        fs::write(&p, "original").unwrap();
        assert!(atomic_write_with(&p, b"next", &mut |_, _| Err(std::io::Error::other("locked"))).is_err());
        assert_eq!(fs::read_to_string(p).unwrap(), "original");
        assert_eq!(fs::read_dir(&d.0).unwrap().count(), 1);
    }
    #[test]
    fn external_edit_or_delete_is_never_overwritten() {
        let d = Temp::new();
        let p = d.0.join("scene.msc");
        fs::write(&p, "external").unwrap();
        assert!(save_scene(&p, "ours", Some("loaded")).is_err());
        assert_eq!(fs::read_to_string(&p).unwrap(), "external");
        fs::remove_file(&p).unwrap();
        assert!(save_scene(&p, "ours", Some("loaded")).is_err());
        assert!(!p.exists());
    }
    #[test]
    fn saves_are_atomic_and_keep_ten_previous_versions() {
        let d = Temp::new();
        let p = d.0.join("сцена.msc");
        save_scene(&p, "0", None).unwrap();
        for i in 1..=14 {
            save_scene(&p, &i.to_string(), Some(&(i - 1).to_string())).unwrap();
        }
        let versions = history(&p);
        assert_eq!(versions.len(), 10);
        assert_eq!(versions[0].text, "13");
        assert_eq!(versions[9].text, "4");
        assert_eq!(fs::read_to_string(&p).unwrap(), "14");
    }
    #[test]
    fn backup_failure_preserves_document() {
        let d = Temp::new();
        let p = d.0.join("scene.msc");
        fs::write(&p, "old").unwrap();
        fs::write(d.0.join(".motif-history"), "occupied").unwrap();
        assert!(save_scene(&p, "new", Some("old")).is_err());
        assert_eq!(fs::read_to_string(p).unwrap(), "old");
    }
    #[test]
    fn crash_recovery_preserves_invalid_script_all_boards_and_original() {
        let d = Temp::new();
        let p = d.0.join("scene.msc");
        fs::write(&p, "original").unwrap();
        let text = "// @motif-web {}\nartboard \"one\" 16x16\nartboard \"two\" 16x16\nunfinished {";
        let mut writer = Recovery::new(d.0.join("recovery"));
        writer.write(&Record::new(Some(p.clone()), text.into(), "original".into(), 1)).unwrap();
        fs::write(writer.dir.join("bad.json"), "{").unwrap();
        let reader = Recovery::new(writer.dir.clone());
        let records = reader.entries();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].text, text);
        assert_eq!(records[0].board, 1);
        assert_eq!(fs::read_to_string(&p).unwrap(), "original");
        fs::write(&p, text).unwrap();
        assert!(reader.entries().is_empty());
    }
    #[test]
    fn independent_windows_keep_separate_recovery_records() {
        let d = Temp::new();
        let mut a = Recovery::new(d.0.clone());
        let mut b = Recovery::new(d.0.clone());
        a.write(&Record::new(None, "first".into(), "".into(), 0)).unwrap();
        b.write(&Record::new(None, "second".into(), "".into(), 0)).unwrap();
        a.clear();
        assert_eq!(Recovery::new(d.0.clone()).entries()[0].text, "second");
    }
}
