//! Font files for `text`, read once and kept while they do not change on disk.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::SystemTime;

type Cache = HashMap<PathBuf, (Option<SystemTime>, Arc<Vec<u8>>)>;

fn cache() -> &'static Mutex<Cache> {
    static CACHE: OnceLock<Mutex<Cache>> = OnceLock::new();
    CACHE.get_or_init(Default::default)
}

/// The bytes of a font: the built-in one (`None`, `regular`, `bold`) or a file, relative to `base`.
pub fn load(font: Option<&str>, base: Option<&Path>) -> Result<Arc<Vec<u8>>, String> {
    let file = match font {
        None | Some("regular") => return Ok(builtin(false)),
        Some("bold") => return Ok(builtin(true)),
        Some(f) => f,
    };
    let path = match base {
        Some(b) if Path::new(file).is_relative() => b.join(file),
        _ => PathBuf::from(file),
    };
    let mtime = std::fs::metadata(&path).and_then(|m| m.modified()).ok();
    let mut cache = cache().lock().map_err(|_| "кэш шрифтов недоступен".to_string())?;
    if let Some((t, bytes)) = cache.get(&path)
        && *t == mtime
    {
        return Ok(bytes.clone());
    }
    let bytes = Arc::new(std::fs::read(&path).map_err(|e| format!("шрифт {}: {e}", path.display()))?);
    cache.insert(path, (mtime, bytes.clone()));
    Ok(bytes)
}

/// The file name a font is saved under next to an exported theme.
pub fn file_name(font: Option<&str>) -> String {
    match font {
        None | Some("regular") => "IBMPlexSans-Regular.ttf".into(),
        Some("bold") => "IBMPlexSans-SemiBold.ttf".into(),
        Some(f) => Path::new(f).file_name().map_or_else(|| f.to_string(), |n| n.to_string_lossy().into_owned()),
    }
}

fn builtin(bold: bool) -> Arc<Vec<u8>> {
    static REGULAR: OnceLock<Arc<Vec<u8>>> = OnceLock::new();
    static BOLD: OnceLock<Arc<Vec<u8>>> = OnceLock::new();
    if bold {
        BOLD.get_or_init(|| Arc::new(motif_core::text::BOLD_FONT.to_vec())).clone()
    } else {
        REGULAR.get_or_init(|| Arc::new(motif_core::text::DEFAULT_FONT.to_vec())).clone()
    }
}
