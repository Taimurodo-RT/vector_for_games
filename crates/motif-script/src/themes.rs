//! Themes: a second set of values for the same names.
//!
//! ```text
//! let paper = #fff8ea
//! let ink = #1b1d24
//! style card fill paper stroke ink 2 r 14
//!
//! theme dark {
//!   paper = #1b1d24
//!   ink = #fff8ea
//!   style card fill paper stroke ink 2 r 14 glow 10 #6f93ff
//! }
//! ```
//!
//! A theme gives another value to a color, a number or a gradient declared with `let` on the
//! top level of the scene, and another line to a style. The scene is read in one theme or in
//! none ([`crate::parse_themed`]): the theme's values stand in for the written ones, and
//! everything that names them follows — objects, styles, gradients, animation keys, the board's
//! background. The document, the runtime and the export see an ordinary scene.
//!
//! A theme is written on the top level, above the first `artboard` for every board of the file
//! or inside one board for that board alone. A theme may be written in several blocks; a name
//! is changed once in a theme. A line of a style in a theme is the whole line of the style in
//! that theme.

use crate::boards::board_views;
use crate::edit::{splice, splice_all, with_space_before};
use crate::{Scene, Span};
use std::collections::HashSet;
use std::path::Path;

/// The whole lines that `span` is written on, with the line break after them.
pub(crate) fn whole_line(src: &str, span: Span) -> Span {
    let start = src[..span.start].rfind('\n').map_or(0, |i| i + 1);
    let end = src[span.end..].find('\n').map_or(src.len(), |i| span.end + i + 1);
    Span::new(start, end)
}

/// Where `line` goes to become a line of the theme being shown, and what is put there. `decl`
/// is where the name it changes is declared: the line joins a block of the theme in the same
/// part of the script (the head every board shares, or the board), and a block is made right
/// after the declaration when that part has none.
pub(crate) fn new_entry(src: &str, scene: &Scene, decl: Span, line: &str) -> Option<(Span, String)> {
    let theme = scene.theme.as_deref()?;
    let head = crate::head_end(src);
    let shared = |at: usize| at < head;
    match scene.themes.iter().find(|t| t.name == theme && shared(t.span.start) == shared(decl.start)) {
        Some(t) => {
            let line_start = src[..t.close].rfind('\n').map_or(0, |i| i + 1);
            // The closing brace has a line of its own unless the block was typed in one line.
            if src[line_start..t.close].trim().is_empty() {
                Some((Span::new(line_start, line_start), format!("  {line}\n")))
            } else {
                Some((Span::new(t.close, t.close), format!("\n  {line}\n")))
            }
        }
        None => {
            let at = whole_line(src, decl).end;
            let lead = if at == src.len() && !src.ends_with('\n') { "\n" } else { "" };
            Some((Span::new(at, at), format!("{lead}theme {theme} {{\n  {line}\n}}\n")))
        }
    }
}

/// The names of the themes written in `file`, on any board, in the order written. Reads the
/// text alone, so it answers for a file with errors too.
pub fn theme_names(file: &str) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    for l in file.lines() {
        let mut words = l.split(|c: char| c.is_whitespace() || c == '{').filter(|w| !w.is_empty());
        if words.next() == Some("theme")
            && l.contains('{')
            && let Some(name) = words.next()
            && !names.iter().any(|n| n == name)
        {
            names.push(name.to_string());
        }
    }
    names
}

/// Whether `name` can name a new theme of `file`: a word of the script that no theme has yet.
pub fn theme_name_ok(file: &str, name: &str) -> bool {
    let mut chars = name.chars();
    chars.next().is_some_and(|c| c.is_alphabetic() || c == '_')
        && chars.all(|c| c.is_alphanumeric() || c == '_')
        && !theme_names(file).iter().any(|n| n == name)
}

/// A theme name not used yet: `dark`, then `light`, then `theme3`, `theme4`…
pub fn free_theme_name(file: &str) -> String {
    let taken = theme_names(file);
    let free = |n: &str| !taken.iter().any(|t| t == n);
    ["dark", "light"]
        .into_iter()
        .find(|n| free(n))
        .map(str::to_string)
        .unwrap_or_else(|| (3..).map(|n| format!("theme{n}")).find(|n| free(n)).expect("some number is free"))
}

/// Returns the file with a new, empty theme `name` in the head every board shares: after the
/// palette, the styles and the themes written there, or at the top. An empty theme shows the
/// scene as it is written. `None` when the name cannot be used or a board has errors.
pub fn add_theme(file: &str, base: Option<&Path>, name: &str) -> Option<String> {
    if !theme_name_ok(file, name) {
        return None;
    }
    let views = board_views(file, base)?;
    let scene = &views.first()?.scene;
    let head = crate::head_end(file);
    let ends = scene.palette.iter().map(|s| s.decl.end).chain(scene.gradients.iter().map(|g| g.decl.end)).chain(scene.styles.iter().map(|s| s.decl.end));
    let last = ends.chain(scene.themes.iter().map(|t| t.span.end)).filter(|end| *end <= head).max();
    let at = match last {
        Some(end) => whole_line(file, Span::new(end, end)).end,
        None => crate::palette::top_of_file(file),
    };
    let lead = if at == file.len() && !file.is_empty() && !file.ends_with('\n') { "\n" } else { "" };
    let out = splice(file, Span::new(at, at), &format!("{lead}theme {name} {{\n}}\n"));
    crate::parse_boards(&out, base).ok()?;
    Some(out)
}

/// Every block of the theme `name` in the file, each once: a block of the head is seen by
/// every board.
fn blocks(file: &str, base: Option<&Path>, name: &str) -> Option<Vec<(Span, Span)>> {
    let views = board_views(file, base)?;
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for v in &views {
        for t in v.scene.themes.iter().filter(|t| t.name == name) {
            let at = v.in_file(t.span);
            if seen.insert(at.start) {
                out.push((at, v.in_file(t.name_span)));
            }
        }
    }
    (!out.is_empty()).then_some(out)
}

/// Returns the file with the theme `old` called `new` in every block of it, on every board.
/// `None` when there is no such theme, the new name cannot be used, or a board has errors.
pub fn rename_theme(file: &str, base: Option<&Path>, old: &str, new: &str) -> Option<String> {
    if !theme_name_ok(file, new) {
        return None;
    }
    let edits = blocks(file, base, old)?.into_iter().map(|(_, name)| (name, new.to_string())).collect();
    let out = splice_all(file, edits);
    crate::parse_boards(&out, base).ok()?;
    Some(out)
}

/// Returns the file without the theme `name`: every block of it goes, on every board. The scene
/// as it is written does not change. `None` when there is no such theme or a board has errors.
pub fn delete_theme(file: &str, base: Option<&Path>, name: &str) -> Option<String> {
    let edits = blocks(file, base, name)?.into_iter().map(|(block, _)| (whole_line(file, block), String::new())).collect();
    let out = splice_all(file, edits);
    crate::parse_boards(&out, base).ok()?;
    Some(out)
}

/// Returns the script without the line the theme being shown gives to `name` (a style, or a
/// name of the palette): in this theme it is again as the scene writes it. `None` when no
/// theme is shown or it has no such line.
pub fn reset_in_theme(src: &str, scene: &Scene, name: &str, style: bool) -> Option<String> {
    let theme = scene.theme.as_deref()?;
    let edits: Vec<(Span, String)> = scene
        .themes
        .iter()
        .filter(|t| t.name == theme)
        .flat_map(|t| t.entries.iter().filter(|e| e.name == name && e.style == style).map(move |e| (t, e)))
        .map(|(t, e)| {
            let line = whole_line(src, e.span);
            // A block typed in one line keeps its braces.
            let braces = (line.start..line.end).contains(&t.span.start) || (line.start..line.end).contains(&t.close);
            (if braces { with_space_before(src, e.span) } else { line }, String::new())
        })
        .collect();
    (!edits.is_empty()).then(|| splice_all(src, edits))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{parse, parse_boards, parse_boards_themed, parse_themed};
    use motif_core::{Color, Prop, Value};

    const SET: &str = "\
let paper = #fff8ea
let ink = #1b1d24
let gold = #ffd23f
let radius = 14
let glowing = linear 90 gold #ff5a36
style card fill paper stroke ink 2 r radius
theme dark {
  paper = #1b1d24
  ink = #fff8ea
  radius = 4
  glowing = radial #6f93ff ink
  style card fill paper stroke gold 3 r radius glow 8 gold
}
artboard \"Кнопки\" 300x200 bg paper
rect a at 80 60 size 100 60 style card
rect b at 200 60 size 100 60 style card fill gold
ellipse sun at 80 150 size 40 40 gradient glowing
text t at 200 150 content \"Да\" fill ink
anim Blink 1s loop {
  t.fill 0: ink | 0.5: gold | 1: ink
}
";

    fn hex(s: &str) -> Color {
        Color::from_hex(s).unwrap()
    }

    #[test]
    fn a_theme_gives_other_values_to_the_same_names() {
        let plain = parse(SET).unwrap();
        assert_eq!(plain.theme, None);
        assert_eq!(plain.doc.themes, ["dark"]);
        assert_eq!(plain.doc.artboard.background, hex("#fff8ea"));
        let a = plain.doc.node("a").unwrap();
        assert_eq!(
            (a.fill, a.stroke.map(|c| (c, a.stroke_width)), a.radius, a.glow.is_some()),
            (Some(hex("#fff8ea")), Some((hex("#1b1d24"), 2.0)), 14.0, false)
        );

        let dark = parse_themed(SET, None, Some("dark")).unwrap();
        assert_eq!(dark.theme.as_deref(), Some("dark"));
        // The board, the style, what an object writes itself by a palette name, a gradient, a
        // text and the keys of an animation all follow the theme.
        assert_eq!(dark.doc.artboard.background, hex("#1b1d24"));
        let a = dark.doc.node("a").unwrap();
        assert_eq!((a.fill, a.stroke.map(|c| (c, a.stroke_width)), a.radius), (Some(hex("#1b1d24")), Some((hex("#ffd23f"), 3.0)), 4.0));
        assert_eq!(a.glow.as_ref().map(|g| g.color), Some(hex("#ffd23f")));
        let b = dark.doc.node("b").unwrap();
        assert_eq!((b.fill, b.radius), (Some(hex("#ffd23f")), 4.0), "the object's own fill stays over the theme's style");
        let sun = dark.doc.node("sun").unwrap().gradient.clone().unwrap();
        assert!(sun.radial);
        assert_eq!(sun.colors, [hex("#6f93ff"), hex("#fff8ea")]);
        assert_eq!(dark.doc.node("t").unwrap().fill, Some(hex("#fff8ea")));
        let keys = &dark.doc.animation("Blink").unwrap().tracks.iter().find(|t| t.node == "t" && t.prop == Prop::Fill).unwrap().keys;
        assert_eq!(keys[0].value, Value::Color(hex("#fff8ea")));
        // Nothing but the look differs: the same objects in the same places.
        let places = |s: &Scene| s.doc.nodes.iter().map(|n| (n.id.clone(), n.x, n.y, n.width, n.height)).collect::<Vec<_>>();
        assert_eq!(places(&plain), places(&dark));
        // A theme the scene does not have changes nothing.
        let other = parse_themed(SET, None, Some("sepia")).unwrap();
        assert_eq!((other.theme.clone(), &other.doc), (None, &plain.doc));
    }

    #[test]
    fn the_scene_tells_where_the_theme_writes_each_value() {
        let dark = parse_themed(SET, None, Some("dark")).unwrap();
        let word = |s: Span| &SET[s.start..s.end];
        let paper = dark.palette.iter().find(|s| s.name == "paper").unwrap();
        assert!(paper.themed);
        assert_eq!((word(paper.span), word(paper.decl)), ("#1b1d24", "#fff8ea"));
        let gold = dark.palette.iter().find(|s| s.name == "gold").unwrap();
        assert!(!gold.themed);
        assert_eq!(gold.span, gold.decl);
        let glowing = &dark.gradients[0];
        assert!(glowing.themed);
        assert_eq!((word(glowing.span), word(glowing.decl)), ("radial #6f93ff ink", "linear 90 gold #ff5a36"));
        let card = &dark.styles[0];
        assert!(card.themed);
        assert_eq!(word(card.span), "style card fill paper stroke gold 3 r radius glow 8 gold");
        assert_eq!(word(card.decl), "style card fill paper stroke ink 2 r radius");
        let theme = &dark.themes[0];
        assert_eq!((theme.name.as_str(), word(theme.name_span), &SET[theme.close..theme.close + 1]), ("dark", "dark", "}"));
        assert_eq!(
            theme.entries.iter().map(|e| (e.name.as_str(), e.style)).collect::<Vec<_>>(),
            [("paper", false), ("ink", false), ("radius", false), ("glowing", false), ("card", true)]
        );
        // The plain scene knows the places only the theme reads: a rename reaches them.
        let plain = parse(SET).unwrap();
        let at = SET.find("glow 8 gold").unwrap() + "glow 8 ".len();
        assert!(plain.colors.iter().any(|c| c.span.start == at && c.name.as_deref() == Some("gold")));
    }

    #[test]
    fn mistakes_in_a_theme_are_errors_of_the_script_in_any_theme() {
        let errors = |src: &str, theme: Option<&str>| -> Vec<String> {
            parse_themed(src, None, theme).err().unwrap_or_default().iter().map(|e| e.message.clone()).collect()
        };
        let board = "artboard \"a\" 100x100\nrect r at 0 0 size 10 10 style card\n";
        let head = "let paper = #ffffff\nlet sky = linear 90 #000000 #ffffff\nstyle card fill paper\n";
        for (theme, expected) in [
            ("papper = #000000", "тема «dark» меняет «papper», а такого имени на верхнем уровне сцены нет (let papper = …). Палитра: paper, sky"),
            ("style cord fill paper", "тема «dark» меняет стиль «cord», а такого стиля нет. Стили сцены: card"),
            ("paper = 4", "«paper» в палитре цвет, а тема «dark» пишет число"),
            ("paper = linear 90 #000000 #ffffff", "«paper» в палитре цвет или число, а тема «dark» пишет градиент"),
            ("sky = #000000", "«sky» в палитре градиент, а тема «dark» пишет цвет или число"),
            ("paper = #000000\n  paper = #111111", "в теме «dark» имя «paper» уже записано"),
            ("style card at 0 0", "«at» остаётся на строке объекта: стиль хранит только вид (заливка, обводка, скругление, тени, кисти, шрифт)"),
            ("paper", "в теме пишут другие значения имён палитры и стилей: paper = #1b1d24, sunset = linear 90 #33205e #1b1d24 или style card fill paper"),
        ] {
            let src = format!("{head}theme dark {{\n  {theme}\n}}\n{board}");
            // Whichever theme is shown, the mistake is told, and once.
            for shown in [None, Some("dark")] {
                let told = errors(&src, shown);
                assert_eq!(told.len(), 1, "{theme}: {told:?}");
                assert!(told[0].contains(expected), "{theme}: {told:?}");
            }
        }
        // What a style of a theme gets wrong is told on its line, whichever theme is shown.
        let src = format!("{head}theme dark {{\n  style card fill nope\n}}\n{board}");
        for shown in [None, Some("dark")] {
            let told = parse_themed(&src, None, shown).unwrap_err();
            assert!(told.iter().any(|e| e.message.contains("неизвестное имя «nope»") && e.line == 5), "{told:?}");
        }
        let nested = format!("{head}{board}group g at 0 0 {{\n  theme dark {{\n  }}\n}}\n");
        assert!(errors(&nested, None)[0].contains("тема пишется на верхнем уровне сцены"));
        // An empty theme and a theme in two blocks are fine.
        let two = format!("{head}theme dark {{\n}}\ntheme dark {{\n  paper = #000000\n}}\ntheme light {{\n}}\n{board}");
        let scene = parse_themed(&two, None, Some("dark")).unwrap();
        assert_eq!(scene.doc.themes, ["dark", "light"]);
        assert_eq!(scene.doc.node("r").unwrap().fill, Some(hex("#000000")));
    }

    #[test]
    fn a_color_edited_in_a_theme_is_written_into_the_theme() {
        let dark = parse_themed(SET, None, Some("dark")).unwrap();
        // The theme already changes `paper`: its value there is edited.
        let out = crate::set_swatch(SET, &dark, "paper", hex("#101010")).unwrap();
        assert!(out.contains("  paper = #101010\n") && out.contains("let paper = #fff8ea\n"));
        // It does not change `gold` yet: the theme gets a line, the palette keeps its color.
        let out = crate::set_swatch(SET, &dark, "gold", hex("#00ff00")).unwrap();
        assert!(out.contains("  style card fill paper stroke gold 3 r radius glow 8 gold\n  gold = #00ff00\n}\n"), "{out}");
        assert!(out.contains("let gold = #ffd23f\n"));
        assert_eq!(parse(&out).unwrap().doc, parse(SET).unwrap().doc);
        assert_eq!(parse_themed(&out, None, Some("dark")).unwrap().doc.node("b").unwrap().fill, Some(hex("#00ff00")));
        // Shown as written, the palette itself changes.
        let plain = parse(SET).unwrap();
        assert!(crate::set_swatch(SET, &plain, "gold", hex("#00ff00")).unwrap().contains("let gold = #00ff00\n"));

        // A gradient the same way.
        let mut g = dark.gradients[0].gradient.clone();
        g.colors[0] = hex("#ff00ff");
        let out = crate::set_gradient_swatch(SET, &dark, "glowing", &g).unwrap();
        assert!(out.contains("  glowing = radial #ff00ff ink\n") && out.contains("let glowing = linear 90 gold #ff5a36\n"), "{out}");

        // A theme with no block yet in this part of the file gets one after the declaration.
        let src = "let paper = #ffffff\ntheme dark {\n}\nartboard \"a\" 100x100 bg paper\nlet local = #ff0000\nrect r at 0 0 size 10 10 fill local\n";
        let dark = parse_themed(src, None, Some("dark")).unwrap();
        let out = crate::set_swatch(src, &dark, "local", hex("#0000ff")).unwrap();
        assert!(out.contains("let local = #ff0000\ntheme dark {\n  local = #0000ff\n}\nrect r"), "{out}");
        let out = crate::set_swatch(src, &dark, "paper", hex("#000000")).unwrap();
        assert!(out.starts_with("let paper = #ffffff\ntheme dark {\n  paper = #000000\n}\nartboard"), "{out}");
        // New colors of the palette go after the palette, not into the theme.
        let dark = parse_themed(SET, None, Some("dark")).unwrap();
        let out = crate::add_swatch(SET, &dark, "extra", hex("#123456"));
        assert!(out.contains("let gold = #ffd23f\nlet extra = #123456\n"), "{out}");
    }

    #[test]
    fn a_look_written_into_a_style_in_a_theme_stays_in_the_theme() {
        let src = "let ink = #1b1d24\nstyle card fill #ffffff stroke ink 2\ntheme dark {\n}\nartboard \"a\" 100x100\nrect a at 0 0 size 10 10 style card fill #222222 r 6\nrect b at 20 0 size 10 10 style card\n";
        let dark = parse_themed(src, None, Some("dark")).unwrap();
        let out = crate::write_into_style(src, &dark, "a").unwrap();
        assert!(out.contains("style card fill #ffffff stroke ink 2\ntheme dark {\n  style card fill #222222 stroke ink 2 r 6\n}\n"), "{out}");
        // The line of the object is one for every theme, so it keeps what it had: the scene as
        // written is drawn as before, and in the theme both objects follow the new line.
        assert!(out.contains("rect a at 0 0 size 10 10 style card fill #222222 r 6\n"), "{out}");
        assert_eq!(parse(&out).unwrap().doc, parse(src).unwrap().doc);
        let again = parse_themed(&out, None, Some("dark")).unwrap();
        assert_eq!(again.doc.node("b").unwrap().fill, Some(hex("#222222")));
        assert_eq!(again.doc.node("b").unwrap().radius, again.doc.node("a").unwrap().radius);
        // Nothing more to write: the theme's line already says all the object has.
        assert_eq!(crate::write_into_style(&out, &again, "a"), None);
        // Once the theme has the line, the next look goes into it.
        let more = out.replace("rect b at 20 0 size 10 10 style card", "rect b at 20 0 size 10 10 style card no stroke");
        let scene = parse_themed(&more, None, Some("dark")).unwrap();
        let out = crate::write_into_style(&more, &scene, "b").unwrap();
        assert!(out.contains("  style card fill #222222 r 6\n") && out.contains("style card fill #ffffff stroke ink 2\n"), "{out}");
        assert!(out.contains("rect b at 20 0 size 10 10 style card no stroke\n"), "{out}");
        // «As written» for one name: the theme's line goes.
        let back = reset_in_theme(&out, &parse_themed(&out, None, Some("dark")).unwrap(), "card", true).unwrap();
        assert!(back.contains("theme dark {\n}\n"), "{back}");
        let paper = reset_in_theme(SET, &parse_themed(SET, None, Some("dark")).unwrap(), "paper", false).unwrap();
        assert!(!paper.contains("paper = #1b1d24") && paper.contains("  ink = #fff8ea\n"));
        assert_eq!(reset_in_theme(SET, &parse(SET).unwrap(), "paper", false), None);
    }

    #[test]
    fn names_are_changed_in_the_themes_too() {
        // A color: its line in the theme and the theme's style line follow the new name.
        let out = crate::rename_swatch(SET, None, 0, "gold", "sun_color").unwrap();
        assert!(out.contains("style card fill paper stroke sun_color 3 r radius glow 8 sun_color"), "{out}");
        assert!(!out.contains("gold"));
        let out = crate::rename_swatch(SET, None, 0, "paper", "sheet").unwrap();
        assert!(out.contains("  sheet = #1b1d24\n") && out.contains("  style card fill sheet stroke gold 3"), "{out}");
        assert_eq!(parse_themed(&out, None, Some("dark")).unwrap().doc, parse_themed(SET, None, Some("dark")).unwrap().doc);
        let out = crate::rename_swatch(SET, None, 0, "glowing", "shine").unwrap();
        assert!(out.contains("  shine = radial #6f93ff ink\n") && out.contains("gradient shine\n"), "{out}");
        // A style: the theme's line follows; deleting the style takes the theme's line away.
        let out = crate::rename_style_in_file(SET, None, 0, "card", "plate").unwrap();
        assert!(
            out.contains("  style plate fill paper stroke gold 3") && out.contains("style plate fill paper stroke ink 2") && !out.contains("card"),
            "{out}"
        );
        let out = crate::delete_style_in_file(SET, None, 0, "card").unwrap();
        assert!(!out.contains("style card"), "{out}");
        assert_eq!(parse(&out).unwrap().doc, parse(SET).unwrap().doc);
        // A color replaced everywhere is replaced in what only a theme reads too.
        let out = crate::replace_color(SET, None, 0, &crate::ColorWord::Name("gold".into()), &crate::ColorWord::Name("ink".into())).unwrap();
        assert!(out.file.contains("  style card fill paper stroke ink 3 r radius glow 8 ink\n"), "{}", out.file);
    }

    #[test]
    fn colors_are_collected_without_a_change_in_any_theme() {
        // `#fff8ea` is the paper as written, but in the dark theme the paper is another color:
        // what is written by value keeps its own name. `#ffd23f` is gold in every theme.
        let src = "let paper = #fff8ea\nlet gold = #ffd23f\nlet glowing = linear 90 gold #ff5a36\nlet plain = linear 0 gold paper\ntheme dark {\n  paper = #1b1d24\n  glowing = radial #6f93ff gold\n  style card fill #fff8ea stroke #ffd23f 2\n}\nstyle card fill paper\nartboard \"a\" 200x100 bg paper\nrect a at 0 0 size 10 10 fill #fff8ea stroke #ffd23f 2\nrect b at 20 0 size 10 10 style card\nrect c at 40 0 size 10 10 gradient linear 90 gold #ff5a36\nrect d at 60 0 size 10 10 gradient linear 90 gold #ff5a36\nrect e at 80 0 size 10 10 gradient linear 0 gold paper\n";
        let c = crate::collect_palette(src, None).unwrap();
        assert!(c.file.contains("let color1 = #ff5a36\nlet color2 = #fff8ea\n"), "{}", c.file);
        assert!(c.file.contains("  style card fill color2 stroke gold 2\n"), "{}", c.file);
        assert!(c.file.contains("rect a at 0 0 size 10 10 fill color2 stroke gold 2\n"), "{}", c.file);
        // A gradient the theme changes is not taken for one written alike; one it leaves is.
        assert!(c.file.contains("let glowing = linear 90 gold color1\n") && c.file.contains("let grad1 = linear 90 gold color1\n"), "{}", c.file);
        assert!(c.file.contains("rect c at 40 0 size 10 10 gradient grad1\n") && c.file.contains("rect e at 80 0 size 10 10 gradient plain\n"), "{}", c.file);
        for theme in [None, Some("dark")] {
            assert_eq!(parse_themed(src, None, theme).unwrap().doc, parse_themed(&c.file, None, theme).unwrap().doc, "{theme:?}");
        }
    }

    #[test]
    fn themes_are_added_renamed_and_removed_in_the_whole_file() {
        let file = "// набор\nlet paper = #ffffff\nstyle card fill paper\n\nartboard \"A\" 100x100 bg paper\nrect a at 0 0 size 10 10 style card\nartboard \"B\" 100x100\nlet local = #ff0000\ntheme dark {\n  local = #00ff00\n}\nrect b at 0 0 size 10 10 fill local\n";
        assert_eq!(theme_names(file), ["dark"]);
        assert_eq!(free_theme_name(file), "light");
        assert_eq!(free_theme_name("theme dark {\n}\ntheme light{\n}\n"), "theme3");
        assert!(!theme_name_ok(file, "dark") && !theme_name_ok(file, "1a") && theme_name_ok(file, "sepia"));
        // A new theme goes into the head, after the palette and the styles.
        let out = add_theme(file, None, "sepia").unwrap();
        assert!(out.starts_with("// набор\nlet paper = #ffffff\nstyle card fill paper\ntheme sepia {\n}\n\nartboard \"A\""), "{out}");
        assert_eq!(add_theme(file, None, "dark"), None);
        assert_eq!(add_theme("artboard \"a\" 10x10\n", None, "dark").unwrap(), "theme dark {\n}\nartboard \"a\" 10x10\n");
        // The head's block and the board's block of one theme are one theme.
        let both = add_theme(file, None, "sepia").unwrap().replace("theme dark {", "theme sepia {");
        let boards = parse_boards_themed(&both, None, Some("sepia")).unwrap();
        assert_eq!(boards[1].doc.node("b").unwrap().fill, Some(hex("#00ff00")));
        let out = rename_theme(&both, None, "sepia", "night").unwrap();
        assert_eq!(out.matches("theme night {").count(), 2);
        assert_eq!(rename_theme(&both, None, "sepia", "sepia"), None);
        assert_eq!(rename_theme(&both, None, "nope", "night"), None);
        let out = delete_theme(&both, None, "sepia").unwrap();
        assert!(!out.contains("theme") && !out.contains("local = #00ff00"), "{out}");
        // The scene as written is the same; only the list of its themes is shorter.
        let written = |file: &str| -> Vec<motif_core::Document> {
            let mut docs: Vec<motif_core::Document> = parse_boards(file, None).unwrap().into_iter().map(|s| s.doc).collect();
            docs.iter_mut().for_each(|doc| doc.themes.clear());
            docs
        };
        assert_eq!(written(&out), written(file));
    }

    #[test]
    fn a_file_with_a_theme_is_saved_in_the_format_that_has_themes() {
        assert_eq!(crate::format::needed("theme dark {\n}\nartboard \"a\" 10x10\n"), 4);
        assert_eq!(crate::format::needed("let theme = 4\nartboard \"a\" 10x10\n"), 1);
    }
}
