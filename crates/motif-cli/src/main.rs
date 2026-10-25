//! `motif` — work with scene scripts without opening the editor.

use motif_pixel::{FrameExport, render_frames, scene_palette, to_jasc};
use motif_render::{CpuOptions, render_cpu};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

const HELP: &str = "\
motif — скрипты сцен Motif без окна редактора

Использование:
  motif check   <сцена.msc>
  motif render  <сцена.msc> [-o кадр.png] [--anim ИМЯ --time СЕК] [параметры рендера]
  motif frames  <сцена.msc> --anim ИМЯ [--fps 12] [-o папка] [параметры рендера]
  motif palette <сцена.msc> [-o палитра.pal]
  motif pixel   <сцена.msc> --anim ИМЯ [--preset X.preset] [--pixatool Pixatool.exe] [--fps 12] [-o папка] [параметры рендера]

Параметры рендера:
  --scale N        пикселей на единицу артборда (по умолчанию 1)
  --width W        итоговая ширина в пикселях вместо --scale
  --no-aa          без сглаживания краёв (удобно перед пикселизацией)
  --transparent    без фона артборда

frames кладёт PNG-кадры ИМЯ_0000.png… в папку: их можно обработать в PixaTool пакетом.
palette сохраняет цвета сцены в формате JASC .pal для PixaTool.
pixel снимает кадры и сам прогоняет их через PixaTool с пресетом, сохранённым в PixaTool кнопкой
«Save Preset». PixaTool ищется в --pixatool, в переменной MOTIF_PIXATOOL и в папке tools рядом с Motif.";

struct Args {
    cmd: String,
    scene: PathBuf,
    out: Option<PathBuf>,
    anim: Option<String>,
    time: f64,
    fps: f64,
    scale: Option<f64>,
    width: Option<f64>,
    antialias: bool,
    background: bool,
    preset: Option<PathBuf>,
    pixatool: Option<PathBuf>,
}

fn parse_args() -> Result<Args, String> {
    let mut it = std::env::args().skip(1);
    let cmd = it.next().ok_or(HELP)?;
    if cmd == "-h" || cmd == "--help" || cmd == "help" {
        return Err(HELP.into());
    }
    let mut a = Args {
        cmd,
        scene: PathBuf::new(),
        out: None,
        anim: None,
        time: 0.0,
        fps: 12.0,
        scale: None,
        width: None,
        antialias: true,
        background: true,
        preset: None,
        pixatool: None,
    };
    let num = |v: Option<String>, flag: &str| v.and_then(|s| s.parse::<f64>().ok()).ok_or(format!("после {flag} нужно число"));
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "-o" | "--out" => a.out = Some(it.next().ok_or("после -o нужен путь")?.into()),
            "--anim" => a.anim = Some(it.next().ok_or("после --anim нужно имя анимации")?),
            "--time" => a.time = num(it.next(), "--time")?,
            "--fps" => a.fps = num(it.next(), "--fps")?,
            "--scale" => a.scale = Some(num(it.next(), "--scale")?),
            "--width" => a.width = Some(num(it.next(), "--width")?),
            "--preset" => a.preset = Some(it.next().ok_or("после --preset нужен файл пресета")?.into()),
            "--pixatool" => a.pixatool = Some(it.next().ok_or("после --pixatool нужен путь к Pixatool.exe")?.into()),
            "--no-aa" => a.antialias = false,
            "--transparent" => a.background = false,
            s if s.starts_with('-') => return Err(format!("неизвестный параметр {s}\n\n{HELP}")),
            s if a.scene.as_os_str().is_empty() => a.scene = s.into(),
            s => return Err(format!("лишний аргумент {s}")),
        }
    }
    if a.scene.as_os_str().is_empty() {
        return Err(format!("не указан файл сцены\n\n{HELP}"));
    }
    Ok(a)
}

fn run() -> Result<(), String> {
    let a = parse_args()?;
    let src = std::fs::read_to_string(&a.scene).map_err(|e| format!("{}: {e}", a.scene.display()))?;
    let scene = motif_script::parse_in(&src, a.scene.parent())
        .map_err(|errs| errs.iter().map(|e| format!("{}: {e}", a.scene.display())).collect::<Vec<_>>().join("\n"))?;
    let doc = scene.doc;
    let scale = match (a.width, a.scale) {
        (Some(w), _) => w / doc.artboard.width,
        (None, Some(s)) => s,
        (None, None) => 1.0,
    };
    let render = CpuOptions { scale, antialias: a.antialias, background: a.background };
    let stem = a.scene.file_stem().and_then(|s| s.to_str()).unwrap_or("scene").to_string();

    match a.cmd.as_str() {
        "check" => {
            println!("{}: ок — объектов {}, анимаций {}", a.scene.display(), doc.nodes.len(), doc.animations.len());
            for anim in &doc.animations {
                println!("  {} — {} с{}", anim.name, motif_script::fmt_num(anim.duration), if anim.looping { ", повтор" } else { "" });
            }
            for m in &doc.machines {
                let inputs: Vec<&str> = m.inputs.iter().map(|i| i.name.as_str()).collect();
                println!("  машина {} — состояний {}, переходов {}, входы: {}", m.name, m.states.len(), m.transitions.len(), inputs.join(", "));
            }
        }
        "render" => {
            let posed = match &a.anim {
                Some(name) => doc.animation(name).ok_or(format!("нет анимации «{name}»"))?.pose(&doc, a.time),
                None => doc,
            };
            let out = a.out.unwrap_or_else(|| PathBuf::from(format!("{stem}.png")));
            let px = render_cpu(&posed, render).ok_or("не удалось создать изображение")?;
            px.save_png(&out).map_err(|e| format!("{}: {e}", out.display()))?;
            println!("{} — {}×{}", out.display(), px.width(), px.height());
        }
        "frames" => {
            let name = a.anim.ok_or("для frames укажите --anim ИМЯ")?;
            let frames = render_frames(&doc, &name, &FrameExport { fps: a.fps, render })?;
            let dir = a.out.unwrap_or_else(|| PathBuf::from(format!("{stem}_{name}")));
            std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
            for (i, px) in frames.iter().enumerate() {
                let path = dir.join(format!("{name}_{i:04}.png"));
                px.save_png(&path).map_err(|e| format!("{}: {e}", path.display()))?;
            }
            println!("{}: {} кадров {}×{} при {} к/с", dir.display(), frames.len(), frames[0].width(), frames[0].height(), motif_script::fmt_num(a.fps));
        }
        "pixel" => {
            let name = a.anim.ok_or("для pixel укажите --anim ИМЯ")?;
            let exe = motif_pixel::pixatool::locate(a.pixatool.as_deref()).ok_or("не найден PixaTool: укажите --pixatool и путь к Pixatool.exe")?;
            let frames = render_frames(&doc, &name, &FrameExport { fps: a.fps, render })?;
            let dir = a.out.unwrap_or_else(|| PathBuf::from(format!("{stem}_{name}_px")));
            std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
            let tmp = std::env::temp_dir().join(format!("motif-pixel-{}", std::process::id()));
            std::fs::create_dir_all(&tmp).map_err(|e| format!("{}: {e}", tmp.display()))?;
            let mut inputs = Vec::new();
            let mut outputs = Vec::new();
            for (i, px) in frames.iter().enumerate() {
                let path = tmp.join(format!("{i:04}.png"));
                px.save_png(&path).map_err(|e| format!("{}: {e}", path.display()))?;
                inputs.push(path);
                outputs.push(std::path::absolute(dir.join(format!("{name}_{i:04}.png"))).map_err(|e| e.to_string())?);
            }
            let preset = a.preset.map(|p| std::path::absolute(p).unwrap_or_default());
            let result = motif_pixel::pixatool::pixelize(&exe, preset.as_deref(), &inputs, &outputs, |n| eprint!("\rPixaTool: {n}/{}", inputs.len()));
            eprintln!();
            std::fs::remove_dir_all(&tmp).ok();
            result?;
            println!("{}: {} кадров через PixaTool", dir.display(), outputs.len());
        }
        "palette" => {
            let colors = scene_palette(&doc);
            let out = a.out.unwrap_or_else(|| PathBuf::from(format!("{stem}.pal")));
            write(&out, to_jasc(&colors))?;
            println!("{} — {} цветов", out.display(), colors.len());
        }
        other => return Err(format!("неизвестная команда «{other}»\n\n{HELP}")),
    }
    Ok(())
}

fn write(path: &Path, text: String) -> Result<(), String> {
    std::fs::write(path, text).map_err(|e| format!("{}: {e}", path.display()))
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}
