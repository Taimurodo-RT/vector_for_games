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
    /// The Motif build of the addon before and after (`version.txt`), when it says.
    pub build_was: Option<String>,
    pub build: Option<String>,
    /// The project had the addon before.
    pub had_addon: bool,
    /// The `.tscn` made with the node, when there was none.
    pub tscn: Option<PathBuf>,
    /// Export presets that got `*.msc` in their filter.
    pub presets: usize,
    /// Something the person should know: the addon has no library for this system…
    pub warnings: Vec<String>,
}

impl Installed {
    /// What changed in the project, one line each, as the editor and `motif runtime` say it.
    pub fn report(&self) -> Vec<String> {
        let build = |b: &Option<String>| b.as_ref().map_or(String::new(), |b| format!(" (сборка {b})"));
        let mut out = vec![format!("сцена: {}", self.res)];
        if self.fonts > 0 {
            out.push(format!("шрифтов обновлено: {}", self.fonts));
        }
        if let Some(t) = &self.tscn {
            out.push(format!("сцена с узлом MotifSprite: {}", t.file_name().map_or(String::new(), |n| n.to_string_lossy().into_owned())));
        }
        out.push(match (self.addon, &self.build_was, &self.build) {
            (false, _, b) => format!("аддон уже стоит{}", build(b)),
            (true, Some(was), Some(now)) if was != now => format!("аддон обновлён: сборка {was} → {now}"),
            (true, _, b) if !self.had_addon => format!("аддон поставлен в addons/motif{}", build(b)),
            (true, _, b) => format!("аддон обновлён{}", build(b)),
        });
        if self.presets > 0 {
            out.push(format!("*.msc и шрифты добавлены в фильтры экспорта: {} пресетов", self.presets));
        }
        out.extend(self.warnings.iter().map(|w| format!("внимание: {w}")));
        out
    }
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
/// paths relative to it. Returns the copied scene and how many fonts changed. When a font is
/// wrong or a file cannot be written, nothing changes: a scene copied there before stays as it was.
pub fn copy_scene(scene: &Path, src: &str, dir: &Path) -> Result<(PathBuf, usize), String> {
    let mut plan = Plan::default();
    let staged = stage_scene(&mut plan, scene, src, dir);
    let (to, fonts) = match staged {
        Ok(s) => s,
        Err(e) => {
            plan.abandon();
            return Err(e);
        }
    };
    plan.commit(&mut |a, b| std::fs::rename(a, b))?;
    Ok((to, fonts))
}

/// Stages the fonts and then the scene into `plan`; returns where the scene goes and how many
/// fonts change.
fn stage_scene(plan: &mut Plan, scene: &Path, src: &str, dir: &Path) -> Result<(PathBuf, usize), String> {
    let name = scene.file_name().ok_or("у сцены нет имени файла")?;
    let fonts = scene_fonts(scene, src)?;
    plan.dir(dir)?;
    let mut changed = 0;
    for (font, from) in fonts {
        let to = dir.join(&font);
        if to == from {
            continue;
        }
        let bytes = std::fs::read(&from).map_err(|e| format!("шрифт {}: {e}", from.display()))?;
        changed += usize::from(plan.file(&to, &bytes)?);
    }
    let to = dir.join(name);
    plan.file(&to, src.as_bytes())?;
    Ok((to, changed))
}

/// The build of Motif an addon folder comes from (`version.txt`, written when Motif is packed).
pub fn addon_build(addon: &Path) -> Option<String> {
    std::fs::read_to_string(addon.join("version.txt")).ok().map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

/// The library file the addon needs on the system Motif runs on.
fn own_library() -> &'static str {
    if cfg!(windows) {
        "bin/motif_godot.dll"
    } else if cfg!(target_os = "macos") {
        "bin/libmotif_godot.dylib"
    } else {
        "bin/libmotif_godot.so"
    }
}

/// Refuses a project MotifSprite cannot run in: one of Godot 3 (`config_version` below 5) or of
/// Godot 4 before 4.3 (`config/features`). A project file without them is let through.
pub fn check_project(project: &Path) -> Result<(), String> {
    let file = project.join("project.godot");
    let text = std::fs::read_to_string(&file).map_err(|e| format!("{}: {e}", file.display()))?;
    let value = |key: &str| text.lines().find_map(|l| l.trim().strip_prefix(key)).map(str::trim);
    if let Some(v) = value("config_version=").and_then(|v| v.parse::<u32>().ok())
        && v < 5
    {
        return Err("это проект Godot 3, а узлу MotifSprite нужен Godot 4.3 или новее".into());
    }
    let version = value("config/features=").and_then(|f| {
        f.split('"').find_map(|w| {
            let (major, minor) = w.split_once('.')?;
            Some((major.parse::<u32>().ok()?, minor.parse::<u32>().ok()?))
        })
    });
    if let Some((major, minor)) = version
        && (major, minor) < (4, 3)
    {
        return Err(format!(
            "проект сделан в Godot {major}.{minor}, а узлу MotifSprite нужен Godot 4.3 или новее: откройте проект в Godot 4.3+, сохраните и поставьте снова"
        ));
    }
    Ok(())
}

/// Puts scene `src` (saved at `scene`) into the folder `dir` of a Godot project, with the addon
/// from `addon` (see [`addon_source`]), a `NAME.tscn` holding a `MotifSprite` that plays it (not
/// touched when it exists), and `*.msc` in the project's export filters.
///
/// Everything is checked and written beside its place first, then moved in at once. If any step
/// fails (a font is missing, Godot holds the library) the project keeps its old set: the old
/// addon, scene and fonts, and the person's own files are not touched.
pub fn install(scene: &Path, src: &str, dir: &Path, addon: Option<&Path>) -> Result<Installed, String> {
    install_with(scene, src, dir, addon, &mut |a, b| std::fs::rename(a, b))
}

fn install_with(scene: &Path, src: &str, dir: &Path, addon: Option<&Path>, rename: &mut Rename<'_>) -> Result<Installed, String> {
    let project = project_root(dir).ok_or_else(|| format!("{} — не проект Godot: выберите папку внутри проекта, где лежит project.godot", dir.display()))?;
    check_project(&project)?;
    let mut plan = Plan::default();
    let staged = stage_install(&mut plan, &project, scene, src, dir, addon);
    let out = match staged {
        Ok(out) => out,
        Err(e) => {
            plan.abandon();
            return Err(e);
        }
    };
    plan.commit(rename).map_err(|e| {
        if e.contains("addons") {
            format!("{e}. Похоже, Godot держит библиотеку аддона: закройте Godot и поставьте ещё раз — в проекте ничего не изменилось")
        } else {
            format!("{e} — в проекте ничего не изменилось")
        }
    })?;
    Ok(out)
}

fn stage_install(plan: &mut Plan, project: &Path, scene: &Path, src: &str, dir: &Path, addon: Option<&Path>) -> Result<Installed, String> {
    let mut out = Installed { project: project.to_path_buf(), ..Default::default() };
    let target = project.join("addons/motif");
    out.build_was = addon_build(&target);
    out.had_addon = target.join("motif.gdextension").is_file();
    match addon {
        Some(a) => {
            if !a.join("motif.gdextension").is_file() {
                return Err(format!("{}: это не аддон Motif (нет motif.gdextension)", a.display()));
            }
            if !a.join(own_library()).is_file() {
                out.warnings.push(format!(
                    "в аддоне нет библиотеки для этой системы ({}): возьмите аддон из сборки Motif или соберите cargo build --release -p motif-godot",
                    own_library()
                ));
            }
            plan.dir(&project.join("addons"))?;
            out.addon = plan.tree(a, &target)?;
            out.build = addon_build(a);
        }
        None if target.join("motif.gdextension").is_file() => out.build = out.build_was.clone(),
        None => return Err("не найден аддон Motif (папка godot-addon рядом с Motif)".into()),
    }
    let (to, fonts) = stage_scene(plan, scene, src, dir)?;
    out.fonts = fonts;
    out.res = res_path(project, &to)?;
    out.scene = to;
    let stem = scene.file_stem().and_then(|s| s.to_str()).unwrap_or("scene");
    let tscn = dir.join(format!("{stem}.tscn"));
    if !tscn.exists() {
        let node: String = stem.chars().map(|c| if c.is_alphanumeric() || c == '_' { c } else { '_' }).collect();
        let text = format!("[gd_scene format=3]\n\n[node name=\"{node}\" type=\"MotifSprite\"]\nscene = \"{}\"\n", out.res);
        plan.file(&tscn, text.as_bytes())?;
        out.tscn = Some(tscn);
    }
    let cfg = project.join("export_presets.cfg");
    if let Some((text, n)) = export_filter(&cfg, &["*.msc", "*.ttf", "*.otf"]) {
        plan.file(&cfg, text.as_bytes())?;
        out.presets = n;
    }
    Ok(out)
}

/// `res://` path of `file` inside `project`.
fn res_path(project: &Path, file: &Path) -> Result<String, String> {
    let rel = file.strip_prefix(project).map_err(|_| format!("{} не внутри проекта {}", file.display(), project.display()))?;
    let parts: Vec<String> = rel.components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect();
    Ok(format!("res://{}", parts.join("/")))
}

type Rename<'a> = dyn FnMut(&Path, &Path) -> std::io::Result<()> + 'a;

/// Files and folders put into a project together. Each is first written beside its place as
/// `.NAME.part`; [`Plan::commit`] then moves them in one after another, keeping what they
/// replace as `.NAME.old` until all are in. When a move fails, the ones before it are put back.
#[derive(Default)]
struct Plan {
    /// Staged copy and where it goes.
    steps: Vec<(PathBuf, PathBuf)>,
    /// Folders made while staging, removed again if nothing goes in.
    made: Vec<PathBuf>,
}

/// `.NAME.tag` beside `path`: Godot leaves hidden files and folders alone.
fn beside(path: &Path, tag: &str) -> PathBuf {
    let name = path.file_name().map_or_else(String::new, |n| n.to_string_lossy().into_owned());
    path.with_file_name(format!(".{name}.{tag}"))
}

fn remove_any(path: &Path) {
    if path.is_dir() {
        let _ = std::fs::remove_dir_all(path);
    } else {
        let _ = std::fs::remove_file(path);
    }
}

/// Every file under `dir`, by its path inside it, with its bytes.
fn files(dir: &Path) -> std::io::Result<Vec<(PathBuf, Vec<u8>)>> {
    let mut out = Vec::new();
    let mut stack = vec![PathBuf::new()];
    while let Some(rel) = stack.pop() {
        for entry in std::fs::read_dir(dir.join(&rel))? {
            let rel = rel.join(entry?.file_name());
            let path = dir.join(&rel);
            if path.is_dir() {
                stack.push(rel);
            } else {
                out.push((rel, std::fs::read(&path)?));
            }
        }
    }
    out.sort();
    Ok(out)
}

impl Plan {
    /// Makes folder `dir` (and those above it) if it is not there.
    fn dir(&mut self, dir: &Path) -> Result<(), String> {
        let missing: Vec<PathBuf> = dir.ancestors().take_while(|d| !d.exists()).map(Path::to_path_buf).collect();
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        self.made.extend(missing.into_iter().rev());
        Ok(())
    }

    /// Stages `bytes` for `target`; false when the file is already the same.
    fn file(&mut self, target: &Path, bytes: &[u8]) -> Result<bool, String> {
        if std::fs::read(target).is_ok_and(|old| old == bytes) {
            return Ok(false);
        }
        if let Some(parent) = target.parent() {
            self.dir(parent)?;
        }
        let part = beside(target, "part");
        std::fs::write(&part, bytes).map_err(|e| format!("{}: {e}", part.display()))?;
        self.steps.push((part, target.to_path_buf()));
        Ok(true)
    }

    /// Stages a copy of folder `from` for `target`; false when the folder is already the same.
    fn tree(&mut self, from: &Path, target: &Path) -> Result<bool, String> {
        let new = files(from).map_err(|e| format!("{}: {e}", from.display()))?;
        if files(target).is_ok_and(|old| old == new) {
            return Ok(false);
        }
        let part = beside(target, "part");
        remove_any(&part);
        self.steps.push((part.clone(), target.to_path_buf()));
        for (rel, bytes) in new {
            let to = part.join(&rel);
            if let Some(parent) = to.parent() {
                std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
            }
            std::fs::write(&to, bytes).map_err(|e| format!("{}: {e}", to.display()))?;
        }
        Ok(true)
    }

    /// Removes what was staged: nothing in the project changed.
    fn abandon(self) {
        for (part, _) in &self.steps {
            remove_any(part);
        }
        for d in self.made.iter().rev() {
            let _ = std::fs::remove_dir(d);
        }
    }

    /// Moves every staged copy into its place with `rename`. On a failure the places already
    /// filled get their old contents back and the error is returned.
    fn commit(self, rename: &mut Rename<'_>) -> Result<(), String> {
        let mut done: Vec<(PathBuf, Option<PathBuf>)> = Vec::new();
        let mut failed = None;
        for (part, target) in &self.steps {
            let old = target.exists().then(|| beside(target, "old"));
            if let Some(old) = &old {
                remove_any(old);
                if let Err(e) = rename(target, old) {
                    failed = Some(format!("{}: {e}", target.display()));
                    break;
                }
            }
            if let Err(e) = rename(part, target) {
                if let Some(old) = &old {
                    let _ = std::fs::rename(old, target);
                }
                failed = Some(format!("{}: {e}", target.display()));
                break;
            }
            done.push((target.clone(), old));
        }
        match failed {
            None => {
                for old in done.iter().filter_map(|(_, old)| old.as_ref()) {
                    // A library Godot still holds may refuse to go: it is hidden, and the next
                    // install tries again.
                    remove_any(old);
                }
                Ok(())
            }
            Some(e) => {
                for (target, old) in done.into_iter().rev() {
                    remove_any(&target);
                    if let Some(old) = old {
                        let _ = std::fs::rename(&old, &target);
                    }
                }
                self.abandon();
                Err(e)
            }
        }
    }
}

/// `export_presets.cfg` with `patterns` added to the non-resource filter of every preset, and
/// how many presets changed; None when there is no file (Godot writes it with the first preset)
/// or nothing to add.
fn export_filter(cfg: &Path, patterns: &[&str]) -> Option<(String, usize)> {
    let text = std::fs::read_to_string(cfg).ok()?;
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
    if changed == 0 {
        return None;
    }
    let mut out = lines.join("\n");
    if text.ends_with('\n') {
        out.push('\n');
    }
    Some((out, changed))
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

    /// An addon of Motif build `build`, with the library for this system.
    fn addon(d: &Path, build: &str) -> PathBuf {
        let a = d.join(format!("addon-{build}"));
        std::fs::create_dir_all(a.join("bin")).unwrap();
        std::fs::write(a.join("motif.gdextension"), b"[configuration]").unwrap();
        std::fs::write(a.join(own_library()), b"lib").unwrap();
        std::fs::write(a.join("version.txt"), format!("{build}\n")).unwrap();
        a
    }

    /// Every file under `dir` with its bytes: what "the project did not change" is checked against.
    fn snapshot(dir: &Path) -> Vec<(PathBuf, Vec<u8>)> {
        files(dir).unwrap()
    }

    /// A project with an older addon, a scene and a font installed, and the person's own files.
    fn installed_project(d: &Path) -> (PathBuf, PathBuf, String) {
        let art = d.join("art");
        std::fs::create_dir_all(art.join("fonts")).unwrap();
        std::fs::write(art.join("fonts/Inter.ttf"), b"font 1").unwrap();
        let src = "artboard 100x40\ntext t content \"Ok\" font \"fonts/Inter.ttf\"\n".to_string();
        let game = d.join("game");
        std::fs::create_dir_all(game.join("ui")).unwrap();
        std::fs::write(game.join("project.godot"), "config_version=5\nconfig/features=PackedStringArray(\"4.3\", \"Forward Plus\")\n").unwrap();
        std::fs::write(game.join("export_presets.cfg"), "[preset.0]\ninclude_filter=\"\"\n").unwrap();
        install(&art.join("button.msc"), &src, &game.join("ui"), Some(&addon(d, "old"))).unwrap();
        std::fs::write(game.join("ui/button.tscn"), "моя сцена с правками").unwrap();
        std::fs::write(game.join("main.gd"), "extends Node").unwrap();
        (art, game, src)
    }

    #[test]
    fn a_failure_halfway_puts_the_old_set_back() {
        let d = dir("halfway");
        let (art, game, src) = installed_project(&d);
        let before = snapshot(&game);
        // New fonts, a new scene and a new addon: four things to move in, each a rename or two.
        std::fs::write(art.join("fonts/Inter.ttf"), b"font 2").unwrap();
        let src2 = format!("{src}ellipse dot at 10 10 size 4 4\n");
        let new = addon(&d, "new");
        let total = {
            let probe = dir("halfway-probe");
            let (art, game, _) = installed_project(&probe);
            std::fs::write(art.join("fonts/Inter.ttf"), b"font 2").unwrap();
            let mut n = 0;
            install_with(&art.join("button.msc"), &src2, &game.join("ui"), Some(&addon(&probe, "new")), &mut |a, b| {
                n += 1;
                std::fs::rename(a, b)
            })
            .unwrap();
            std::fs::remove_dir_all(probe).ok();
            n
        };
        assert!(total >= 6, "{total} moves");
        // Fail at every move in turn: the old library held by Godot, a file in the way…
        for fail_at in 1..=total {
            let mut n = 0;
            let e = install_with(&art.join("button.msc"), &src2, &game.join("ui"), Some(&new), &mut |a, b| {
                n += 1;
                if n == fail_at { Err(std::io::Error::other("занято другим процессом")) } else { std::fs::rename(a, b) }
            })
            .unwrap_err();
            assert!(e.contains("ничего не изменилось"), "{fail_at}: {e}");
            assert_eq!(snapshot(&game), before, "move {fail_at} of {total}: the project is as it was");
        }
        // The same install with nothing in the way goes through, and the person's files stay.
        let r = install(&art.join("button.msc"), &src2, &game.join("ui"), Some(&new)).unwrap();
        assert!(r.report().contains(&"аддон обновлён: сборка old → new".to_string()), "{:?}", r.report());
        assert_eq!(addon_build(&game.join("addons/motif")).as_deref(), Some("new"));
        assert_eq!(std::fs::read(game.join("ui/fonts/Inter.ttf")).unwrap(), b"font 2");
        assert_eq!(std::fs::read_to_string(game.join("ui/button.tscn")).unwrap(), "моя сцена с правками");
        assert_eq!(std::fs::read_to_string(game.join("main.gd")).unwrap(), "extends Node");
        let hidden: Vec<_> = files(&game).unwrap().into_iter().filter(|(p, _)| p.to_string_lossy().contains("/.")).collect();
        assert!(hidden.is_empty(), "nothing staged is left: {:?}", hidden.iter().map(|h| &h.0).collect::<Vec<_>>());
        std::fs::remove_dir_all(d).ok();
    }

    #[test]
    fn a_project_godot_cannot_run_is_refused_before_it_changes() {
        let d = dir("version");
        let (art, game, src) = installed_project(&d);
        let before = snapshot(&game);
        for (project, says) in [("config_version=4\n", "Godot 3"), ("config_version=5\nconfig/features=PackedStringArray(\"4.2\", \"Mobile\")\n", "Godot 4.2")]
        {
            std::fs::write(game.join("project.godot"), project).unwrap();
            let e = install(&art.join("button.msc"), &format!("{src}// new\n"), &game.join("ui"), Some(&addon(&d, "new"))).unwrap_err();
            assert!(e.contains(says) && e.contains("4.3"), "{e}");
            let mut now = snapshot(&game);
            now.retain(|(p, _)| p != Path::new("project.godot"));
            let mut was = before.clone();
            was.retain(|(p, _)| p != Path::new("project.godot"));
            assert_eq!(now, was, "{says}");
        }
        for ok in ["config_version=5\nconfig/features=PackedStringArray(\"4.4\", \"Forward Plus\")\n", "config_version=5\n"] {
            std::fs::write(game.join("project.godot"), ok).unwrap();
            assert!(check_project(&game).is_ok(), "{ok}");
        }
        // An addon without the library for this system is put in, with a word about it.
        let bare = d.join("bare");
        std::fs::create_dir_all(&bare).unwrap();
        std::fs::write(bare.join("motif.gdextension"), b"[configuration]").unwrap();
        let r = install(&art.join("button.msc"), &src, &game.join("ui"), Some(&bare)).unwrap();
        assert!(r.warnings.iter().any(|w| w.contains(own_library())), "{:?}", r.warnings);
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
        let addon = addon(&d, "2026.10.28-a");
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
        assert_eq!(std::fs::read(game.join("addons/motif").join(own_library())).unwrap(), b"lib");
        assert_eq!(r.report()[..3], ["сцена: res://ui/button.msc", "шрифтов обновлено: 1", "сцена с узлом MotifSprite: button.tscn"]);
        assert_eq!(r.report()[3], "аддон поставлен в addons/motif (сборка 2026.10.28-a)");
        let tscn = std::fs::read_to_string(game.join("ui/button.tscn")).unwrap();
        assert!(tscn.contains("type=\"MotifSprite\"") && tscn.contains("scene = \"res://ui/button.msc\""), "{tscn}");
        let cfg = std::fs::read_to_string(game.join("export_presets.cfg")).unwrap();
        assert!(cfg.contains("include_filter=\"*.json, *.msc, *.ttf, *.otf\"") && cfg.contains("include_filter=\"*.msc, *.ttf, *.otf\""), "{cfg}");

        // Again: the addon and filters are the same, the scene made by hand in Godot is kept.
        std::fs::write(game.join("ui/button.tscn"), "мой").unwrap();
        let again = install(&art.join("button.msc"), src, &game.join("ui"), Some(&addon)).unwrap();
        assert_eq!((again.addon, again.tscn.clone(), again.presets, again.fonts), (false, None, 0, 0));
        assert_eq!(again.report(), ["сцена: res://ui/button.msc", "аддон уже стоит (сборка 2026.10.28-a)"]);
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
