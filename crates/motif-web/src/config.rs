use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Kept in the shared script header: old Motif versions treat it as a comment.
pub const MARKER: &str = "// @motif-web ";

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    /// Palette name -> currentColor or a CSS custom property name (including --).
    pub tokens: BTreeMap<String, String>,
    /// Theme name -> palette name -> hex value. The authored palette is the default theme.
    pub themes: BTreeMap<String, BTreeMap<String, String>>,
    /// Exact artboard name -> asset settings.
    pub assets: BTreeMap<String, Asset>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct Asset {
    pub id: String,
    /// Relative .svg path; empty uses the board name.
    pub file: String,
    pub profile: Profile,
    pub variant: String,
    pub stage: String,
    pub keywords: Vec<String>,
    pub pivot: Option<[f64; 2]>,
    pub hotspot: Option<[f64; 2]>,
    /// Top, right, bottom, left, in artboard units.
    pub slices: Option<[f64; 4]>,
    pub manual_fix: bool,
    /// Project-specific metadata (slots, rig conventions, tile masks, etc.).
    pub metadata: serde_json::Value,
    pub layers: BTreeMap<String, Layer>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct Layer {
    pub id: String,
    pub class: String,
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum Profile {
    #[default]
    General,
    Icon16,
    Icon24,
    Icon48,
    Tile,
    Illustration,
}

impl Profile {
    pub const ALL: [Self; 6] = [Self::General, Self::Icon16, Self::Icon24, Self::Icon48, Self::Tile, Self::Illustration];
    pub fn name(self) -> &'static str {
        match self {
            Self::General => "general",
            Self::Icon16 => "icon16",
            Self::Icon24 => "icon24",
            Self::Icon48 => "icon48",
            Self::Tile => "tile",
            Self::Illustration => "illustration",
        }
    }
    pub fn parse(s: &str) -> Result<Self, String> {
        Self::ALL.into_iter().find(|p| p.name() == s).ok_or_else(|| format!("Неизвестный SVG-профиль: {s}"))
    }
    pub fn icon_size(self) -> Option<f64> {
        match self {
            Self::Icon16 => Some(16.0),
            Self::Icon24 => Some(24.0),
            Self::Icon48 => Some(48.0),
            _ => None,
        }
    }
}

pub fn read(src: &str) -> Result<Config, String> {
    let mut found = None;
    for (i, line) in src.lines().enumerate() {
        if let Some(json) = line.trim_start().strip_prefix(MARKER) {
            if found.is_some() {
                return Err("В файле больше одного блока @motif-web".into());
            }
            if i > 0 && src.lines().take(i).any(|l| l.trim_start().starts_with("artboard ")) {
                return Err("@motif-web должен стоять до первого artboard".into());
            }
            found = Some(serde_json::from_str(json).map_err(|e| format!("@motif-web, строка {}: {e}", i + 1))?);
        }
    }
    let c = found.unwrap_or_default();
    validate(&c)?;
    Ok(c)
}

pub fn write(src: &str, c: &Config) -> Result<String, String> {
    validate(c)?;
    let header = format!("{MARKER}{}\n", serde_json::to_string(c).map_err(|e| e.to_string())?);
    let mut out = header;
    for line in src.split_inclusive('\n') {
        if !line.trim_start().starts_with(MARKER) {
            out.push_str(line);
        }
    }
    Ok(out)
}

pub fn ident(s: &str) -> bool {
    !s.is_empty()
        && s.bytes().next().is_some_and(|b| b.is_ascii_alphabetic() || b == b'_')
        && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

pub fn slug(s: &str) -> String {
    let mut out = String::new();
    for ch in s.chars() {
        if ch.is_ascii_alphanumeric() || ch == '_' || ch == '-' {
            out.push(ch);
        } else if ch == '.' || ch.is_whitespace() {
            out.push('-');
        } else {
            out.push_str(&format!("u{:x}", ch as u32));
        }
    }
    if !out.starts_with(|c: char| c.is_ascii_alphabetic() || c == '_') {
        out.insert_str(0, "asset-");
    }
    out
}

pub fn file_ok(s: &str) -> bool {
    s.ends_with(".svg")
        && s.split('/').all(|p| {
            !p.is_empty()
                && p != "."
                && p != ".."
                && !p.ends_with('.')
                && p.bytes().all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
                && ![
                    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8", "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5",
                    "LPT6", "LPT7", "LPT8", "LPT9",
                ]
                .contains(&p.split('.').next().unwrap_or("").to_ascii_uppercase().as_str())
        })
}

pub fn validate(c: &Config) -> Result<(), String> {
    for binding in c.tokens.values() {
        if binding != "currentColor" && !(binding.starts_with("--") && ident(&binding[2..])) {
            return Err(format!("Токен: ожидалось currentColor или --имя, получено {binding:?}"));
        }
    }
    for (name, colors) in &c.themes {
        if !ident(name) || name == "default" {
            return Err(format!("Недопустимое имя темы: {name}"));
        }
        for v in colors.values() {
            if motif_core::Color::from_hex(v).is_none() {
                return Err(format!("Цвет темы должен быть HEX: {v}"));
            }
        }
    }
    for a in c.assets.values() {
        if !a.id.is_empty() && !a.id.bytes().all(|b| b.is_ascii_alphanumeric() || b"_.:-".contains(&b)) {
            return Err(format!("Недопустимый id ассета: {}", a.id));
        }
        if !a.file.is_empty() && !file_ok(&a.file) {
            return Err(format!("Небезопасный относительный путь SVG: {}", a.file));
        }
        if a.pivot.iter().chain(a.hotspot.iter()).flatten().any(|n| !n.is_finite()) || a.slices.iter().flatten().any(|n| !n.is_finite() || *n < 0.0) {
            return Err("Координаты должны быть конечными, slice — неотрицательным".into());
        }
        for l in a.layers.values() {
            if !l.id.is_empty() && !ident(&l.id) {
                return Err(format!("Недопустимый id слоя: {}", l.id));
            }
            if l.class.split_whitespace().any(|s| !ident(s)) {
                return Err(format!("Недопустимый CSS class: {}", l.class));
            }
        }
    }
    Ok(())
}

/// Check authored metadata independently of whether the scene can be exported to SVG.
/// A physics/3D document still needs to store asset IDs, themes and game attachment points.
pub fn validate_metadata(scene: &motif_script::Scene, config: &Config) -> Result<(), String> {
    validate(config)?;
    let Some(asset) = config.assets.get(&scene.doc.artboard.name) else { return Ok(()) };
    for layer in asset.layers.keys() {
        if scene.doc.node(layer).is_none() {
            return Err(format!("В метаданных указан отсутствующий слой {layer}"));
        }
    }
    let mut ids = std::collections::BTreeSet::new();
    for n in &scene.doc.nodes {
        let id = asset.layers.get(&n.id).map(|l| l.id.clone()).filter(|s| !s.is_empty()).unwrap_or_else(|| slug(&n.id));
        if !ids.insert(id.clone()) {
            return Err(format!("Повторяющийся SVG id слоя: {id}"));
        }
    }
    if let Some(s) = asset.slices
        && (s[1] + s[3] > scene.doc.artboard.width || s[0] + s[2] > scene.doc.artboard.height)
    {
        return Err("Slice выходит за размер холста".into());
    }
    Ok(())
}
