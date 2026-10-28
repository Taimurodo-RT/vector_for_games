//! Puts a scene into a Godot project for the `MotifSprite` node, which plays it there without
//! baking: the Motif addon into `addons/motif`, the `.msc` and the fonts it names into a folder of
//! the project, a scene with the node set up, and `*.msc` into the export filters so the game
//! carries its scenes when it is exported.

use std::path::{Path, PathBuf};

/// What [`install`] did.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Installed {
    /// The Godot project: the folder with `project.godot`.
    pub project: PathBuf,
    /// The scene file in the project, and how Godot names it (`res://…`).
    pub scene: PathBuf,
    pub res: String,
    /// Fonts copied next to the scene.
    pub fonts: usize,
    /// The addon was copied or updated (false: it was there already, the same).
    pub addon: bool,
    /// The `.tscn` made with the node, when there was none.
    pub tscn: Option<PathBuf>,
    /// Export presets that got `*.msc` in their filter.
    pub presets: usize,
    /// Something the person should know: the addon has no library, Godot holds the old one…
    pub warnings: Vec<String>,
}

/// The Godot project around `dir`: the nearest folder up with `project.godot`.
pub fn project_root(dir: &Path) -> Option<PathBuf> {
    dir.ancestors().find(|d| d.join("project.godot").is_file()).map(Path::to_path_buf)
}

/// The addon shipped with Motif: `MOTIF_GODOT_ADDON`, else `godot-addon/addons/motif` next to
/// the program, else in a folder above it (running from the repository's `target`).
pub fn addon_source() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("MOTIF_GODOT_ADDON").filter(|p| !p.is_empty()) {
        return Some(p.into());
    }
    let exe = std::env::current_exe().ok()?;
    exe.ancestors().skip(1).take(4).map(|d| d.join("godot-addon/addons/motif")).find(|d| d.join("motif.gdextension").is_file())
}

/// The fonts scene `src` (saved at `scene`) names, checked before anything is copied: each as
/// written, and the file it is read from. A font outside the scene's folder, or missing, is an error.
pub fn scene_fonts(scene: &Path, src: &str) -> Result<Vec<(String, PathBuf)>, String> {
    let from_dir = scene.parent().unwrap_or(Path::new("."));
    let mut out = Vec::new();
    for font in motif_script::fonts::named(src)? {
        let rel = Path::new(&font);
        if rel.is_absolute() || rel.components().any(|c| matches!(c, std::path::Component::ParentDir)) {
            // Outside the scene's folder: the game would look for it at a path it does not have.
            return Err(format!("шрифт «{font}» лежит не в папке сцены: положите его рядом с .msc (например в fonts/), чтобы он поехал в игру"));
        }
        let file = from_dir.join(rel);
        if !file.is_file() {
            return Err(format!("нет шрифта {}", file.display()));
        }
        out.push((font, file));
    }
    Ok(out)
}

/// Copies the scene `src` (saved at `scene`) into `dir` with the fonts it names, at the same
/// paths relative to it. Returns the copied scene and how many fonts went along. When a font is
/// wrong nothing is written, and a scene copied there before stays as it was.
pub fn copy_scene(scene: &Path, src: &str, dir: &Path) -> Result<(PathBuf, usize), String> {
    let name = scene.file_name().ok_or("у сцены нет имени файла")?;
    let fonts = scene_fonts(scene, src)?;
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let mut copied = 0;
    for (font, from) in fonts {
        let to = dir.join(&font);
        if to == from {
            continue;
        }
        if let Some(parent) = to.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
        }
        std::fs::copy(&from, &to).map_err(|e| format!("шрифт {}: {e}", from.display()))?;
        copied += 1;
    }
    // The scene last, and whole: Godot may be reading it.
    let to = dir.join(name);
    let part = dir.join(format!(".{}.part", name.to_string_lossy()));
    std::fs::write(&part, src).map_err(|e| format!("{}: {e}", part.display()))?;
    std::fs::rename(&part, &to).map_err(|e| {
        let _ = std::fs::remove_file(&part);
        format!("{}: {e}", to.display())
    })?;
    Ok((to, copied))
}

/// Puts scene `src` (saved at `scene`) into the folder `dir` of a Godot project, with the addon
/// from `addon` (see [`addon_source`]), a `NAME.tscn` holding a `MotifSprite` that plays it (not
/// touched when it exists), and `*.msc` in the project's export filters.
pub fn install(scene: &Path, src: &str, dir: &Path, addon: Option<&Path>) -> Result<Installed, String> {
    let project = project_root(dir).ok_or_else(|| format!("{} — не проект Godot: выберите папку внутри проекта, где лежит project.godot", dir.display()))?;
    // A scene that cannot go is found out before anything in the project changes.
    scene_fonts(scene, src)?;
    let mut out = Installed { project: project.clone(), ..Default::default() };
    match addon {
        Some(a) => {
            let (changed, warnings) = copy_addon(a, &project.join("addons/motif"))?;
            out.addon = changed;
            out.warnings.extend(warnings);
        }
        None if project.join("addons/motif/motif.gdextension").is_file() => {}
        None => return Err("не найден аддон Motif (папка godot-addon рядом с Motif)".into()),
    }
    let (copied, fonts) = copy_scene(scene, src, dir)?;
    out.fonts = fonts;
    out.res = res_path(&project, &copied)?;
    out.scene = copied;
    let stem = scene.file_stem().and_then(|s| s.to_str()).unwrap_or("scene");
    let tscn = dir.join(format!("{stem}.tscn"));
    if !tscn.exists() {
        let node: String = stem.chars().map(|c| if c.is_alphanumeric() || c == '_' { c } else { '_' }).collect();
        let text = format!("[gd_scene format=3]\n\n[node name=\"{node}\" type=\"MotifSprite\"]\nscene = \"{}\"\n", out.res);
        std::fs::write(&tscn, text).map_err(|e| format!("{}: {e}", tscn.display()))?;
        out.tscn = Some(tscn);
    }
    out.presets = add_export_filter(&project.join("export_presets.cfg"), &["*.msc", "*.ttf", "*.otf"])?;
    Ok(out)
}

/// `res://` path of `file` inside `project`.
fn res_path(project: &Path, file: &Path) -> Result<String, String> {
    let rel = file.strip_prefix(project).map_err(|_| format!("{} не внутри проекта {}", file.display(), project.display()))?;
    let parts: Vec<String> = rel.components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect();
    Ok(format!("res://{}", parts.join("/")))
}

/// Copies the addon folder over `to`, file by file, leaving files that are already the same.
/// Returns whether anything changed, and what could not be replaced.
fn copy_addon(from: &Path, to: &Path) -> Result<(bool, Vec<String>), String> {
    if !from.join("motif.gdextension").is_file() {
        return Err(format!("{}: это не аддон Motif (нет motif.gdextension)", from.display()));
    }
    let mut changed = false;
    let mut warnings = Vec::new();
    let mut libraries = 0;
    let mut stack = vec![PathBuf::new()];
    while let Some(rel) = stack.pop() {
        let dir = from.join(&rel);
        let entries = std::fs::read_dir(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        std::fs::create_dir_all(to.join(&rel)).map_err(|e| format!("{}: {e}", to.join(&rel).display()))?;
        for entry in entries.flatten() {
            let rel = rel.join(entry.file_name());
            let (src, dst) = (from.join(&rel), to.join(&rel));
            if src.is_dir() {
                stack.push(rel);
                continue;
            }
            let lib = matches!(src.extension().and_then(|e| e.to_str()), Some("dll" | "so" | "dylib"));
            libraries += usize::from(lib);
            let bytes = std::fs::read(&src).map_err(|e| format!("{}: {e}", src.display()))?;
            if std::fs::read(&dst).is_ok_and(|old| old == bytes) {
                continue;
            }
            match std::fs::write(&dst, &bytes) {
                Ok(()) => changed = true,
                // Windows keeps a loaded library locked while Godot is open.
                Err(e) if lib && dst.exists() => warnings.push(format!("{} не обновлён ({e}): закройте Godot и поставьте ещё раз", dst.display())),
                Err(e) => return Err(format!("{}: {e}", dst.display())),
            }
        }
    }
    if libraries == 0 {
        warnings.push("в аддоне нет библиотеки (bin/motif_godot.dll): возьмите аддон из сборки Motif или соберите cargo build --release -p motif-godot".into());
    }
    Ok((changed, warnings))
}

/// Adds `patterns` to the non-resource filter of every preset in `export_presets.cfg`; returns how
/// many presets changed. No file — nothing to do: Godot writes it with the first preset.
fn add_export_filter(cfg: &Path, patterns: &[&str]) -> Result<usize, String> {
    let Ok(text) = std::fs::read_to_string(cfg) else { return Ok(0) };
    let mut changed = 0;
    let lines: Vec<String> = text
        .lines()
        .map(|line| {
            let Some(value) = line.strip_prefix("include_filter=") else { return line.to_string() };
            let value = value.trim().trim_matches('"');
            let mut items: Vec<&str> = value.split(',').map(str::trim).filter(|s| !s.is_empty()).collect();
            let before = items.len();
            for p in patterns {
                if !items.contains(p) {
                    items.push(p);
                }
            }
            if items.len() == before {
                return line.to_string();
            }
            changed += 1;
            format!("include_filter=\"{}\"", items.join(", "))
        })
        .collect();
    if changed > 0 {
        let mut out = lines.join("\n");
        if text.ends_with('\n') {
            out.push('\n');
        }
        std::fs::write(cfg, out).map_err(|e| format!("{}: {e}", cfg.display()))?;
    }
    Ok(changed)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("motif-runtime-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn fonts_are_found_as_written() {
        let src = "text a content \"x\" font \"fonts/Inter.ttf\" font-size 16\ntext b font \"bold\"\ntext c font \"fonts/Inter.ttf\"\ntext d font  \"мой.otf\"\n// myfont \"no.ttf\"";
        assert_eq!(motif_script::fonts::named(src).unwrap(), ["fonts/Inter.ttf", "мой.otf"]);
        // Comments and texts that mention a font are not fonts.
        let src = "artboard 100x80\nellipse body at 50 40 size 20 20 fill #55aa88\n// font \"fonts/unused.ttf\" was removed\ntext t content \"font x.ttf\" font-size 12\n";
        assert_eq!(motif_script::fonts::named(src).unwrap(), Vec::<String>::new());
        let d = dir("comment");
        std::fs::write(d.join("a.msc"), src).unwrap();
        let (to, n) = copy_scene(&d.join("a.msc"), src, &d.join("game")).unwrap();
        assert_eq!((std::fs::read_to_string(to).unwrap().as_str(), n), (src, 0));
        std::fs::remove_dir_all(d).ok();
    }

    #[test]
    fn a_refused_scene_leaves_the_copy_in_the_game() {
        let d = dir("refused");
        let (art, game) = (d.join("art"), d.join("game/characters"));
        std::fs::create_dir_all(&art).unwrap();
        std::fs::create_dir_all(d.join("shared")).unwrap();
        std::fs::write(d.join("shared/Inter.ttf"), b"font").unwrap();
        let good = "artboard 100x80\nellipse body at 50 40 size 20 20 fill #55aa88\n";
        copy_scene(&art.join("hero.msc"), good, &game).unwrap();
        // A font beside the art folder, and one that is not there: refused, the copy kept.
        for font in ["../shared/Inter.ttf", "fonts/missing.ttf"] {
            let bad = format!("{good}text label at 10 20 content \"Test\" font \"{font}\" font-size 16\n");
            assert!(copy_scene(&art.join("hero.msc"), &bad, &game).is_err(), "{font}");
            assert_eq!(std::fs::read_to_string(game.join("hero.msc")).unwrap(), good, "{font}");
        }
        let names: Vec<_> = std::fs::read_dir(&game).unwrap().flatten().map(|e| e.file_name()).collect();
        assert_eq!(names, [std::ffi::OsString::from("hero.msc")], "no half-written files");
        std::fs::remove_dir_all(d).ok();
    }

    #[test]
    fn a_scene_goes_into_a_project_with_the_addon() {
        let d = dir("install");
        // Motif's side: the scene with a font, and the addon.
        let art = d.join("art");
        std::fs::create_dir_all(art.join("fonts")).unwrap();
        std::fs::write(art.join("fonts/Inter.ttf"), b"font").unwrap();
        let src = "artboard \"Кнопка\" 100x40\ntext t content \"Ok\" font \"fonts/Inter.ttf\"\n";
        std::fs::write(art.join("button.msc"), src).unwrap();
        let addon = d.join("addon");
        std::fs::create_dir_all(addon.join("bin")).unwrap();
        std::fs::write(addon.join("motif.gdextension"), b"[configuration]").unwrap();
        std::fs::write(addon.join("bin/motif_godot.dll"), b"lib").unwrap();
        // Godot's side: a project with an export preset.
        let game = d.join("game");
        std::fs::create_dir_all(game.join("ui")).unwrap();
        std::fs::write(game.join("project.godot"), "config_version=5\n").unwrap();
        std::fs::write(game.join("export_presets.cfg"), "[preset.0]\nname=\"Windows\"\ninclude_filter=\"*.json\"\n\n[preset.1]\ninclude_filter=\"\"\n")
            .unwrap();

        assert!(install(&art.join("button.msc"), src, &d, Some(&addon)).unwrap_err().contains("project.godot"));
        let r = install(&art.join("button.msc"), src, &game.join("ui"), Some(&addon)).unwrap();
        assert_eq!((r.project.clone(), r.res.as_str(), r.fonts, r.addon, r.presets), (game.clone(), "res://ui/button.msc", 1, true, 2));
        assert!(r.warnings.is_empty(), "{:?}", r.warnings);
        assert_eq!(std::fs::read(game.join("ui/fonts/Inter.ttf")).unwrap(), b"font");
        assert_eq!(std::fs::read(game.join("addons/motif/bin/motif_godot.dll")).unwrap(), b"lib");
        let tscn = std::fs::read_to_string(game.join("ui/button.tscn")).unwrap();
        assert!(tscn.contains("type=\"MotifSprite\"") && tscn.contains("scene = \"res://ui/button.msc\""), "{tscn}");
        let cfg = std::fs::read_to_string(game.join("export_presets.cfg")).unwrap();
        assert!(cfg.contains("include_filter=\"*.json, *.msc, *.ttf, *.otf\"") && cfg.contains("include_filter=\"*.msc, *.ttf, *.otf\""), "{cfg}");

        // Again: the addon and filters are the same, the scene made by hand in Godot is kept.
        std::fs::write(game.join("ui/button.tscn"), "мой").unwrap();
        let again = install(&art.join("button.msc"), src, &game.join("ui"), Some(&addon)).unwrap();
        assert_eq!((again.addon, again.tscn, again.presets), (false, None, 0));
        assert_eq!(std::fs::read_to_string(game.join("ui/button.tscn")).unwrap(), "мой");

        // Refused before the project changes: the scene installed before stays.
        let broken = format!("{src}text u content \"?\" font \"fonts/Gone.ttf\"\n");
        assert!(install(&art.join("button.msc"), &broken, &game.join("ui"), Some(&addon)).unwrap_err().contains("Gone.ttf"));
        assert_eq!(std::fs::read_to_string(game.join("ui/button.msc")).unwrap(), src);
        // A font outside the scene's folder would not reach the game.
        let outside = "text t font \"../Inter.ttf\"";
        assert!(copy_scene(&art.join("button.msc"), outside, &game.join("ui")).unwrap_err().contains("рядом с .msc"));
        std::fs::remove_dir_all(d).ok();
    }
}
