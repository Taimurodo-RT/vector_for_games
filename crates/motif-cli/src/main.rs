//! `motif` — work with scene scripts without opening the editor.

use motif_core::Document;
use motif_pixel::{FrameExport, boards_palette, render_frames, to_jasc};
use motif_render::{CpuOptions, render_cpu};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

mod html_import;
mod inspect;

const HELP: &str = "\
motif — скрипты сцен Motif без окна редактора

Использование:
  motif check   <сцена.msc>
  motif import-html <фрагмент.html> [--css стиль.css] [-o сцена.msc]
  motif inspect <сцена.msc> [--board ИМЯ] [--anim ИМЯ --time СЕК] [--node ID] [-o отчёт.json]
  motif svg     <сцена.msc> [-o рисунок.svg|папка] [--board ИМЯ] [--profile general|icon16|icon24|icon48|tile|illustration]
  motif web     <сцена.msc> [-o папка] [--board ИМЯ] [--profile ПРОФИЛЬ] [--strict]
  motif render  <сцена.msc> [-o кадр.png] [--anim ИМЯ --time СЕК] [--side СТОРОНА] [параметры рендера]
  motif frames  <сцена.msc> --anim ИМЯ [--fps 12] [--side СТОРОНА] [-o папка] [параметры рендера]
  motif video   <сцена.msc> --anim ИМЯ [--fps 30] [--side СТОРОНА] [-o видео.mp4|.webm|.gif|.png] [--ffmpeg путь] [параметры рендера]
  motif video   <сцена.msc> --machine ИМЯ --take \"1 speed=1, 2.5 jump\" [--length СЕК] [-o видео.mp4] …
  motif palette <сцена.msc> [-o палитра.pal]
  motif kit     <сцена.msc> [-o папка] [параметры рендера]
  motif screen  <сцена.msc> [-o папка] [параметры рендера]
  motif godot   <сцена.msc> [--node ID] [--pivot X Y] [--machine ИМЯ] [--pixelize] [--preset X.preset] [--fps 12] [-o папка] [параметры рендера]
  motif pixel   <сцена.msc> --anim ИМЯ [--preset X.preset] [--pixatool Pixatool.exe] [--fps 12] [-o папка] [параметры рендера]
  motif runtime <сцена.msc> -o папка-в-проекте-godot [--addon папка-аддона]

Холсты: в файле с несколькими строками artboard каждая открывает свой холст. У всех команд --board ИМЯ (или номер
с 1) берёт один из них, без него команды работают со всеми: render сохраняет СЦЕНА_ХОЛСТ.png
(-o — папка), kit собирает детали всех холстов в одну тему, godot выгружает каждый холст своей
сценой ХОЛСТ.tscn в одну папку, palette собирает цвета всех холстов; frames и pixel берут холст,
где есть анимация --anim (video тоже).

Параметры рендера:
  --scale N        пикселей на единицу артборда (по умолчанию 1)
  --width W        итоговая ширина в пикселях вместо --scale
  --no-aa          без сглаживания краёв (удобно перед пикселизацией)
  --transparent    без фона артборда

frames кладёт PNG-кадры ИМЯ_0000.png… в папку: их можно обработать в PixaTool пакетом.
video сохраняет анимацию одним файлом, формат — по расширению -o (по умолчанию СЦЕНА_ИМЯ.mp4):
GIF и APNG (.png) Motif пишет сам, MP4 (H.264) и WebM (VP9, с прозрачностью) — через ffmpeg, который
ищется в --ffmpeg, рядом с Motif и в PATH. Повторяющаяся анимация в GIF и APNG крутится по кругу; где
прозрачности нет (MP4), её место занимает цвет фона артборда. С --machine и --take video снимает прогон машины
состояний: в моменты из --take входы меняются (число — speed=1, флаг — on или on=0, триггер — просто имя),
и машина переходит между состояниями, как в игре. Прогон длится --length секунд или секунду после последнего момента.
kit сохраняет элементы интерфейса (ui, slice) в каждом состоянии (анимации Hover, Press, Disabled, Focus)
и тему Godot ИМЯ.tres, где они стоят стилями 9-slice.
screen делает из каждого холста сцену Godot ХОЛСТ.tscn из Control-узлов: детали интерфейса — их типы
из темы набора (она пишется рядом), стеки — VBoxContainer/HBoxContainer, привязки — якоря, тексты — Label,
остальное — картинки TextureRect.
godot запекает все анимации в один атлас ИМЯ.png, пишет SpriteFrames ИМЯ_frames.tres и сцену ИМЯ.tscn
с AnimatedSprite2D. Если в сцене есть машина состояний, в сцене будут ещё AnimationPlayer и AnimationTree
с теми же состояниями и переходами, а входы машины станут переменными скрипта ИМЯ.gd. --node берёт
только один объект (персонаж без фона), --pivot задаёт точку привязки спрайта в координатах артборда.
С --pixelize (или --preset) кадры перед упаковкой в атлас проходят через PixaTool.
palette сохраняет цвета сцены в формате JASC .pal для PixaTool.
runtime кладёт сцену в проект Godot для узла MotifSprite, который играет её вектором без запекания: аддон
Motif в addons/motif (из --addon, переменной MOTIF_GODOT_ADDON или папки godot-addon рядом с Motif), .msc и
его шрифты в папку -o, сцену ИМЯ.tscn с настроенным узлом (если её ещё нет) и *.msc в фильтры экспорта.
Стороны: в сцене со строкой directions 8 turn ID… godot снимает каждую анимацию со всех сторон
(Walk_down, Walk_right…, с mirror — без левых, их отражает скрипт ИМЯ.gd), а --side right у render,
frames и video снимает одну сторону: down, down_right, right, up_right, up, up_left, left, down_left.
pixel снимает кадры и сам прогоняет их через PixaTool с пресетом, сохранённым в PixaTool кнопкой
«Save Preset». PixaTool ищется в --pixatool, в переменной MOTIF_PIXATOOL и в папке tools рядом с Motif.";

struct Args {
    cmd: String,
    scene: PathBuf,
    out: Option<PathBuf>,
    anim: Option<String>,
    time: f64,
    fps: f64,
    fps_set: bool,
    ffmpeg: Option<PathBuf>,
    addon: Option<PathBuf>,
    scale: Option<f64>,
    width: Option<f64>,
    antialias: bool,
    background: bool,
    preset: Option<PathBuf>,
    pixatool: Option<PathBuf>,
    node: Option<String>,
    pivot: Option<(f64, f64)>,
    pixelize: bool,
    machine: Option<String>,
    board: Option<String>,
    take: Option<String>,
    length: Option<f64>,
    side: Option<String>,
    web_profile: Option<motif_web::Profile>,
    strict: bool,
    css: Option<PathBuf>,
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
        fps_set: false,
        ffmpeg: None,
        addon: None,
        scale: None,
        width: None,
        antialias: true,
        background: true,
        preset: None,
        pixatool: None,
        node: None,
        pivot: None,
        pixelize: false,
        machine: None,
        board: None,
        take: None,
        length: None,
        side: None,
        web_profile: None,
        strict: false,
        css: None,
    };
    let num = |v: Option<String>, flag: &str| v.and_then(|s| s.parse::<f64>().ok()).ok_or(format!("после {flag} нужно число"));
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "-o" | "--out" => a.out = Some(it.next().ok_or("после -o нужен путь")?.into()),
            "--anim" => a.anim = Some(it.next().ok_or("после --anim нужно имя анимации")?),
            "--time" => a.time = num(it.next(), "--time")?,
            "--fps" => {
                a.fps = num(it.next(), "--fps")?;
                a.fps_set = true;
            }
            "--addon" => a.addon = Some(it.next().ok_or("после --addon нужна папка аддона (godot-addon/addons/motif)")?.into()),
            "--ffmpeg" => a.ffmpeg = Some(it.next().ok_or("после --ffmpeg нужен путь к ffmpeg")?.into()),
            "--scale" => a.scale = Some(num(it.next(), "--scale")?),
            "--width" => a.width = Some(num(it.next(), "--width")?),
            "--preset" => a.preset = Some(it.next().ok_or("после --preset нужен файл пресета")?.into()),
            "--pixatool" => a.pixatool = Some(it.next().ok_or("после --pixatool нужен путь к Pixatool.exe")?.into()),
            "--node" => a.node = Some(it.next().ok_or("после --node нужен id объекта")?),
            "--board" => a.board = Some(it.next().ok_or("после --board нужно имя холста или его номер")?),
            "--take" => a.take = Some(it.next().ok_or("после --take нужны моменты: \"1 speed=1, 2.5 jump\"")?),
            "--length" => a.length = Some(num(it.next(), "--length")?),
            "--side" => a.side = Some(it.next().ok_or("после --side нужна сторона: down, right, up…")?),
            "--machine" => a.machine = Some(it.next().ok_or("после --machine нужно имя машины")?),
            "--pivot" => a.pivot = Some((num(it.next(), "--pivot")?, num(it.next(), "--pivot")?)),
            "--no-aa" => a.antialias = false,
            "--profile" => a.web_profile = Some(motif_web::Profile::parse(&it.next().ok_or("после --profile нужен профиль")?)?),
            "--strict" => a.strict = true,
            "--css" => a.css = Some(it.next().ok_or("после --css нужен файл стилей")?.into()),
            "--pixelize" => a.pixelize = true,
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
    if a.cmd == "import-html" {
        return html_import::run(&a, &src);
    }
    let scenes = motif_script::parse_boards(&src, a.scene.parent())
        .map_err(|errs| errs.iter().map(|e| format!("{}: {e}", a.scene.display())).collect::<Vec<_>>().join("\n"))?;
    let total = scenes.len();
    if a.cmd == "inspect" {
        return inspect::run(&a, &src, &scenes);
    }
    if a.cmd == "svg" || a.cmd == "web" {
        return export_web(&a, &src, scenes);
    }
    let boards = pick_boards(scenes.into_iter().map(|s| s.doc).collect(), a.board.as_deref())?;
    let several = boards.len() > 1;
    let stem = a.scene.file_stem().and_then(|s| s.to_str()).unwrap_or("scene").to_string();
    let render_for = |doc: &Document| {
        let scale = match (a.width, a.scale) {
            (Some(w), _) => w / doc.artboard.width,
            (None, Some(s)) => s,
            (None, None) => 1.0,
        };
        CpuOptions { scale, antialias: a.antialias, background: a.background }
    };
    // The name a board's files get: the file's own name while it has one board.
    let board_name = |doc: &Document| if total > 1 { motif_script::board_file_name(&doc.artboard.name) } else { stem.clone() };

    match a.cmd.as_str() {
        "check" => {
            if total > 1 {
                println!("{}: ок — холстов {total}", a.scene.display());
            }
            for doc in &boards {
                if total > 1 {
                    let (w, h) = (motif_script::fmt_num(doc.artboard.width), motif_script::fmt_num(doc.artboard.height));
                    println!("холст «{}» {w}×{h} — объектов {}, анимаций {}", doc.artboard.name, doc.nodes.len(), doc.animations.len());
                } else {
                    println!("{}: ок — объектов {}, анимаций {}", a.scene.display(), doc.nodes.len(), doc.animations.len());
                }
                for anim in &doc.animations {
                    let events = anim.events.iter().map(|e| format!("{} {}", e.name, motif_script::fmt_num(e.time))).collect::<Vec<_>>();
                    let events = if events.is_empty() { String::new() } else { format!(", события: {}", events.join(", ")) };
                    println!("  {} — {} с{}{events}", anim.name, motif_script::fmt_num(anim.duration), if anim.looping { ", повтор" } else { "" });
                }
                for m in &doc.machines {
                    let inputs: Vec<&str> = m.inputs.iter().map(|i| i.name.as_str()).collect();
                    let mouse = if m.listeners.is_empty() { String::new() } else { format!(", связей с мышью {}", m.listeners.len()) };
                    println!("  машина {} — состояний {}, переходов {}, входы: {}{mouse}", m.name, m.states.len(), m.transitions.len(), inputs.join(", "));
                }
            }
        }
        "render" => {
            for doc in &boards {
                let face = a.side.as_deref().map(|s| motif_pixel::side(doc, s)).transpose()?;
                // From a side, `Walk_up` plays instead of `Walk` where the scene has it.
                let anim = |name: &str| match (face, &doc.directions) {
                    (Some(k), Some(d)) => d.anim_for(doc, name, k),
                    _ => doc.animation(name),
                };
                let mut posed = match &a.anim {
                    Some(name) if several => anim(name).map_or_else(|| doc.clone(), |an| an.pose(doc, a.time)),
                    Some(name) => anim(name).ok_or(format!("нет анимации «{name}»"))?.pose(doc, a.time),
                    None => doc.clone(),
                };
                if let Some(k) = face {
                    posed.rig();
                    posed.face(k);
                    posed.rig();
                }
                // Several boards: -o is the folder they go to.
                let out = match &a.out {
                    Some(o) if several => o.join(format!("{stem}_{}.png", board_name(doc))),
                    Some(o) => o.clone(),
                    None if total > 1 => PathBuf::from(format!("{stem}_{}.png", board_name(doc))),
                    None => PathBuf::from(format!("{stem}.png")),
                };
                if let Some(dir) = out.parent().filter(|d| !d.as_os_str().is_empty()) {
                    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
                }
                let px = render_cpu(&posed, render_for(doc)).ok_or("не удалось создать изображение")?;
                px.save_png(&out).map_err(|e| format!("{}: {e}", out.display()))?;
                println!("{} — {}×{}", out.display(), px.width(), px.height());
            }
        }
        "frames" => {
            let name = a.anim.clone().ok_or("для frames укажите --anim ИМЯ")?;
            let doc = board_with_anim(&boards, &name)?;
            let face = a.side.as_deref().map(|s| motif_pixel::side(doc, s)).transpose()?;
            let frames = motif_pixel::render_frames_facing(doc, &name, &FrameExport { fps: a.fps, render: render_for(doc) }, face)?;
            let dir = a.out.clone().unwrap_or_else(|| PathBuf::from(format!("{stem}_{name}")));
            std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
            for (i, px) in frames.iter().enumerate() {
                let path = dir.join(format!("{name}_{i:04}.png"));
                px.save_png(&path).map_err(|e| format!("{}: {e}", path.display()))?;
            }
            println!("{}: {} кадров {}×{} при {} к/с", dir.display(), frames.len(), frames[0].width(), frames[0].height(), motif_script::fmt_num(a.fps));
        }
        "video" if a.take.is_some() || (a.anim.is_none() && a.machine.is_some()) => {
            let name = a.machine.clone().ok_or("для прогона укажите --machine ИМЯ")?;
            let with: Vec<&Document> = boards.iter().filter(|d| d.machine(&name).is_some()).collect();
            let doc = match with.as_slice() {
                [doc] => *doc,
                [] => return Err(format!("нет машины «{name}»")),
                _ => return Err(format!("машина «{name}» есть на нескольких холстах: укажите --board")),
            };
            let m = doc.machine(&name).expect("found");
            let take = motif_core::Take::parse(m, a.take.as_deref().unwrap_or(""), a.length)?;
            let face = a.side.as_deref().map(|s| motif_pixel::side(doc, s)).transpose()?;
            let fps = if a.fps_set { a.fps } else { 30.0 };
            let out = a.out.clone().unwrap_or_else(|| PathBuf::from(format!("{stem}_{name}.mp4")));
            let r = motif_pixel::video::export_take(
                doc,
                m,
                &take,
                face,
                render_for(doc),
                motif_pixel::video::VideoOptions { fps, ffmpeg: a.ffmpeg.clone(), ..Default::default() },
                &out,
            )?;
            println!(
                "{} — {}, прогон «{name}» {} с: {} кадров {}×{}",
                out.display(),
                r.format.label(),
                motif_script::fmt_num(take.length),
                r.frames,
                r.width,
                r.height
            );
        }
        "video" => {
            let name = a.anim.clone().ok_or("для video укажите --anim ИМЯ (или --machine и --take для прогона машины)")?;
            let doc = board_with_anim(&boards, &name)?;
            // Video plays smoothly at 30 frames a second; sprite frames keep their 12.
            let fps = if a.fps_set { a.fps } else { 30.0 };
            let out = a.out.clone().unwrap_or_else(|| PathBuf::from(format!("{stem}_{name}.mp4")));
            let face = a.side.as_deref().map(|s| motif_pixel::side(doc, s)).transpose()?;
            let r = motif_pixel::video::export_animation(
                doc,
                &name,
                face,
                render_for(doc),
                motif_pixel::video::VideoOptions { fps, ffmpeg: a.ffmpeg.clone(), ..Default::default() },
                &out,
            )?;
            println!("{} — {}, {} кадров {}×{} при {} к/с", out.display(), r.format.label(), r.frames, r.width, r.height, motif_script::fmt_num(fps));
        }
        "pixel" => {
            let name = a.anim.clone().ok_or("для pixel укажите --anim ИМЯ")?;
            let doc = board_with_anim(&boards, &name)?;
            let exe = motif_pixel::pixatool::locate(a.pixatool.as_deref()).ok_or("не найден PixaTool: укажите --pixatool и путь к Pixatool.exe")?;
            let frames = render_frames(doc, &name, &FrameExport { fps: a.fps, render: render_for(doc) })?;
            let dir = a.out.clone().unwrap_or_else(|| PathBuf::from(format!("{stem}_{name}_px")));
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
            let preset = a.preset.clone().map(|p| std::path::absolute(p).unwrap_or_default());
            let result = motif_pixel::pixatool::pixelize(&exe, preset.as_deref(), &inputs, &outputs, |n| eprint!("\rPixaTool: {n}/{}", inputs.len()));
            eprintln!();
            std::fs::remove_dir_all(&tmp).ok();
            result?;
            println!("{}: {} кадров через PixaTool", dir.display(), outputs.len());
        }
        "kit" => {
            // The pieces of every board go into one theme; boards without pieces add nothing.
            let with: Vec<&Document> = boards.iter().filter(|d| motif_pixel::kit::has_pieces(d)).collect();
            let with = if with.is_empty() { boards.iter().collect() } else { with };
            let kits = with.iter().map(|doc| motif_pixel::kit::render_kit(doc, render_for(doc))).collect::<Result<Vec<_>, _>>()?;
            let items = motif_pixel::kit::merge_boards(kits);
            let dir = a.out.clone().unwrap_or_else(|| PathBuf::from(format!("{stem}_ui")));
            let files = motif_pixel::kit::write_kit(&dir, &stem, &items)?;
            for item in &items {
                let pictures: Vec<&str> = item.pictures.iter().map(|p| p.name.as_str()).collect();
                let size = item.pictures.first().map_or("без картинок".to_string(), |p| format!("{}×{}, ", p.px.width(), p.px.height()));
                let font = item.font.as_ref().map_or(String::new(), |f| format!(", текст {} {}px", f.file, f.size));
                println!("  {} ({} от {}) — {size}{}{font}", item.id, motif_pixel::kit::type_name(&item.id), item.base, pictures.join(", "));
            }
            println!("{}: {} файлов, тема {stem}.tres", dir.display(), files.len());
        }
        "screen" => {
            let dir = a.out.clone().unwrap_or_else(|| PathBuf::from(format!("{stem}_ui")));
            let named: Vec<(String, Document)> = boards.iter().map(|d| (d.artboard.name.clone(), d.clone())).collect();
            let opts = render_for(&boards[0]);
            let files = motif_pixel::screen::write_screens(&dir, &stem, &named, opts)?;
            for f in files.iter().filter(|f| f.extension().is_some_and(|e| e == "tscn")) {
                println!("  {}", f.display());
            }
            println!("{}: {} файлов", dir.display(), files.len());
        }
        "godot" => {
            // With --node or --machine only the boards that have them; each board is a scene of its own.
            let boards: Vec<&Document> = boards
                .iter()
                .filter(|d| a.node.as_ref().is_none_or(|n| d.node(n).is_some()))
                .filter(|d| a.machine.as_ref().is_none_or(|m| d.machine(m).is_some()))
                .collect();
            match (boards.len(), &a.node, &a.machine) {
                (0, Some(n), _) => return Err(format!("нет объекта «{n}»")),
                (0, _, Some(m)) => return Err(format!("нет машины «{m}»")),
                _ => {}
            }
            let dir = a.out.clone().unwrap_or_else(|| match (boards.len(), &a.node) {
                (1, Some(n)) => PathBuf::from(format!("{n}_godot")),
                _ => PathBuf::from(format!("{stem}_godot")),
            });
            for doc in boards {
                let name = a.node.clone().unwrap_or_else(|| board_name(doc));
                godot(&a, doc, render_for(doc), &dir, &name)?;
            }
        }
        "runtime" => {
            let dir = a.out.clone().ok_or("для runtime укажите -o папку внутри проекта Godot")?;
            let addon = a.addon.clone().or_else(motif_pixel::runtime::addon_source);
            let r = motif_pixel::runtime::install(&a.scene, &src, &dir, addon.as_deref())?;
            println!("{} → проект Godot {}", a.scene.display(), r.project.display());
            for line in r.report() {
                println!("  {line}");
            }
        }
        "palette" => {
            let colors = boards_palette(&boards);
            let out = a.out.clone().unwrap_or_else(|| PathBuf::from(format!("{stem}.pal")));
            write(&out, to_jasc(&colors))?;
            println!("{} — {} цветов", out.display(), colors.len());
        }
        other => return Err(format!("неизвестная команда «{other}»\n\n{HELP}")),
    }
    Ok(())
}

fn export_web(a: &Args, src: &str, mut scenes: Vec<motif_script::Scene>) -> Result<(), String> {
    if a.anim.is_some() || a.time != 0.0 || a.side.is_some() || a.machine.is_some() || a.scale.is_some() || a.width.is_some() {
        return Err("SVG/Web экспортирует исходный векторный рисунок. Анимации web сохраняются отдельно в CSS и graphics.json; параметры растрового рендера здесь не применяются.".into());
    }
    if let Some(board) = &a.board {
        let index = board
            .parse::<usize>()
            .ok()
            .filter(|i| *i > 0 && *i <= scenes.len())
            .map(|i| i - 1)
            .or_else(|| scenes.iter().position(|s| s.doc.artboard.name == *board))
            .ok_or_else(|| format!("Нет холста {board}"))?;
        scenes = vec![scenes.remove(index)];
    }
    let config = motif_web::read(src)?;
    let package = motif_web::build(&scenes, &config, a.web_profile)?;
    for warning in &package.warnings {
        eprintln!("Внимание: {warning}");
    }
    if a.strict && !package.warnings.is_empty() {
        return Err("--strict: экспорт остановлен из-за предупреждений, файлы не записаны".into());
    }
    let stem = a.scene.file_stem().and_then(|s| s.to_str()).unwrap_or("scene");
    if a.cmd == "svg" && scenes.len() == 1 {
        let svg = motif_web::render(&scenes[0], &config, a.web_profile)?;
        let dest = a.out.clone().unwrap_or_else(|| PathBuf::from(&svg.file));
        if dest.extension().and_then(|s| s.to_str()) != Some("svg") {
            return Err("Для одного SVG укажите -o файл.svg; для пакета используйте motif web".into());
        }
        if let Some(p) = dest.parent().filter(|p| !p.as_os_str().is_empty()) {
            std::fs::create_dir_all(p).map_err(|e| e.to_string())?;
        }
        std::fs::write(&dest, svg.text).map_err(|e| e.to_string())?;
        println!("{}", dest.display());
    } else {
        let dir = a.out.clone().unwrap_or_else(|| PathBuf::from(format!("{stem}-web")));
        if a.cmd == "svg" {
            let mut only = package;
            only.files.retain(|name, _| name.ends_with(".svg"));
            motif_web::export(&only, &dir)?;
        } else {
            motif_web::export(&package, &dir)?;
        }
        println!("{} — холстов {}", dir.display(), scenes.len());
    }
    Ok(())
}

/// The boards `--board` names (its name or number from 1), or all of them.
fn pick_boards(boards: Vec<Document>, pick: Option<&str>) -> Result<Vec<Document>, String> {
    let Some(pick) = pick else { return Ok(boards) };
    let names = || boards.iter().map(|d| format!("«{}»", d.artboard.name)).collect::<Vec<_>>().join(", ");
    let at = match pick.parse::<usize>() {
        Ok(n) if (1..=boards.len()).contains(&n) => Some(n - 1),
        _ => boards.iter().position(|d| d.artboard.name == pick),
    };
    let at = at.ok_or_else(|| format!("нет холста «{pick}»; холсты: {}", names()))?;
    Ok(vec![boards.into_iter().nth(at).expect("index checked")])
}

/// The one board with animation `name`: frames come from a single board.
fn board_with_anim<'a>(boards: &'a [Document], name: &str) -> Result<&'a Document, String> {
    let with: Vec<&Document> = boards.iter().filter(|d| d.animation(name).is_some()).collect();
    match with.as_slice() {
        [doc] => Ok(doc),
        [] => Err(format!("нет анимации «{name}»")),
        many => {
            let names: Vec<String> = many.iter().map(|d| format!("«{}»", d.artboard.name)).collect();
            Err(format!("анимация «{name}» есть на холстах {}: укажите --board", names.join(", ")))
        }
    }
}

/// One board for Godot: atlas, SpriteFrames and a scene named `name` in `dir`.
fn godot(a: &Args, doc: &Document, render: CpuOptions, dir: &Path, name: &str) -> Result<(), String> {
    let opts = motif_pixel::godot::GodotExport { fps: a.fps, render, only: a.node.clone(), pivot: a.pivot.map(|(x, y)| motif_core::kurbo::Point::new(x, y)) };
    let machine = match &a.machine {
        Some(m) => Some(doc.machine(m).ok_or(format!("нет машины «{m}»"))?),
        None => doc.machines.first(),
    };
    let pixelize = a.pixelize || a.preset.is_some();
    let sheet = if pixelize {
        let exe = motif_pixel::pixatool::locate(a.pixatool.as_deref()).ok_or("не найден PixaTool: укажите --pixatool и путь к Pixatool.exe")?;
        let preset = a.preset.clone().map(|p| std::path::absolute(p).unwrap_or_default());
        let sheet = motif_pixel::godot::bake_sheet_with(doc, &opts, |frames| {
            let n = frames.len();
            motif_pixel::pixatool::pixelize_frames(&exe, preset.as_deref(), frames, |k| eprint!("\rPixaTool: {k}/{n}"))
        });
        eprintln!();
        sheet?
    } else {
        motif_pixel::godot::bake_sheet(doc, &opts)?
    };
    let files = motif_pixel::godot::write_godot(dir, name, &sheet, machine, pixelize || !a.antialias)?;
    for an in &sheet.anims {
        println!("  {} — кадров {}{}", an.name, an.frames.len(), if an.looping { ", повтор" } else { "" });
    }
    let (w, h) = (sheet.atlas.width(), sheet.atlas.height());
    let tree = machine.map_or(String::new(), |m| format!(", машина {} в AnimationTree", m.name));
    println!(
        "{}/{name}.tscn: {} файлов, атлас {w}×{h}, разных кадров {} по {}×{}{tree}",
        dir.display(),
        files.len(),
        sheet.cells.len(),
        sheet.size.0,
        sheet.size.1
    );
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
