//! Pinned, deterministic UA defaults for the supported HTML subset, not a site reset.
//! System colours/native widgets are approximated with one portable light palette.
//! Chromium html.css (2026-10-28):
//! https://github.com/chromium/chromium/blob/0441415b72b922e1c14ad3320fd230d1608ea651/third_party/blink/renderer/core/html/resources/html.css
//! Gecko forms.css/html.css (2026-10-14):
//! https://github.com/mozilla-firefox/firefox/blob/d760e8dae63ede3b6a50bc1fcc2a04c7d1b40a47/layout/style/res/forms.css
//! https://github.com/mozilla-firefox/firefox/blob/d760e8dae63ede3b6a50bc1fcc2a04c7d1b40a47/layout/style/res/html.css
//! This subset omits OS themes, viewport flow, margin collapsing and browser text
//! wrapping. Explicit body margins apply to a body element; component fragments
//! do not receive an artificial page or a site-wide reset.
use crate::{Element, css, style::Style};
use std::collections::BTreeMap;
pub(crate) const SELECT_ARROW_SPACE: f64 = 14.0;

/// Browser engine baseline used before author HTML/CSS declarations.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum BrowserProfile {
    #[default]
    Chrome,
    Firefox,
}
impl BrowserProfile {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Chrome => "chrome",
            Self::Firefox => "firefox",
        }
    }
    pub const fn name(self) -> &'static str {
        match self {
            Self::Chrome => "Chrome / Chromium",
            Self::Firefox => "Firefox / Gecko",
        }
    }
    pub const fn source_revision(self) -> &'static str {
        match self {
            Self::Chrome => "0441415b72b922e1c14ad3320fd230d1608ea651",
            Self::Firefox => "d760e8dae63ede3b6a50bc1fcc2a04c7d1b40a47",
        }
    }
}

/// Import configuration. The default preserves the original `import` entry point.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ImportOptions {
    pub browser: BrowserProfile,
}

/// UA shorthands are expanded before author cascade values replace them. This is
/// essential for `p { margin:0 }` to override the UA margin-block longhands.
pub(crate) fn declarations(node: &Element, parent: Option<&Style>, browser: BrowserProfile, warnings: &mut Vec<String>) -> BTreeMap<String, String> {
    let mut source = String::new();
    let block = matches!(
        node.tag.as_str(),
        "root"
            | "html"
            | "body"
            | "main"
            | "section"
            | "article"
            | "aside"
            | "nav"
            | "header"
            | "footer"
            | "form"
            | "div"
            | "p"
            | "pre"
            | "ul"
            | "ol"
            | "li"
            | "h1"
            | "h2"
            | "h3"
            | "h4"
            | "h5"
            | "h6"
            | "hr"
    );
    source.push_str(if block { "display:block;" } else { "display:inline;" });
    match node.tag.as_str() {
        "root" => source.push_str("font-family:serif;font-size:medium;color:#000000;"),
        "body" => source.push_str("margin:8px;"),
        "p" => source.push_str("margin:1em 0;"),
        "h1" => source.push_str("font-size:2em;font-weight:bold;margin:.67em 0;"),
        "h2" => source.push_str("font-size:1.5em;font-weight:bold;margin:.83em 0;"),
        "h3" => source.push_str("font-size:1.17em;font-weight:bold;margin:1em 0;"),
        "h4" => source.push_str("font-size:1em;font-weight:bold;margin:1.33em 0;"),
        "h5" => source.push_str("font-size:.83em;font-weight:bold;margin:1.67em 0;"),
        "h6" => source.push_str("font-size:.67em;font-weight:bold;margin:2.33em 0;"),
        "strong" | "b" => source.push_str("font-weight:bold;"),
        "i" | "em" => source.push_str("font-style:italic;"),
        "u" | "ins" => source.push_str("text-decoration:underline;"),
        "s" | "del" => source.push_str("text-decoration:line-through;"),
        "small" => source.push_str("font-size:smaller;"),
        "sub" => source.push_str("font-size:smaller;vertical-align:sub;"),
        "sup" => source.push_str("font-size:smaller;vertical-align:super;"),
        "pre" => source.push_str("font-family:monospace;white-space:pre;margin:1em 0;"),
        "code" | "kbd" | "samp" => source.push_str("font-family:monospace;"),
        "ul" | "ol" => {
            source.push_str(if parent.is_some_and(|p| p.list_depth > 0) { "margin:0;padding-left:40px;" } else { "margin:1em 0;padding-left:40px;" })
        }
        "a" if node.attrs.contains_key("href") => source.push_str("color:#0000ee;text-decoration:underline;"),
        "button" => button(&mut source, browser),
        "input" => {
            let kind = node.attrs.get("type").map_or("text", String::as_str).to_ascii_lowercase();
            match kind.as_str() {
                "hidden"=>source.push_str("display:none;"),
                "button"|"submit"|"reset"=>button(&mut source,browser),
                "checkbox"|"radio"=>{
                    // Native appearance paints an outline even when computed CSS
                    // says border:0. Keep an editable, visible approximation.
                    source.push_str("display:inline-block;width:13px;height:13px;box-sizing:border-box;padding:0;border:1px solid #767676;background-color:#ffffff;");
                    source.push_str(if kind=="radio"{"margin:3px 3px 0 5px;"}else{"margin:3px 3px 3px 4px;"});
                },
                "range"=>source.push_str("display:inline-block;width:129px;height:16px;margin:2px;box-sizing:border-box;padding:0;border:0;"),
                _=>source.push_str("display:inline-block;font-family:sans-serif;font-size:13.333333px;font-weight:normal;font-style:normal;line-height:normal;padding:1px 2px;border:2px solid #767676;background-color:#ffffff;color:#000000;box-sizing:content-box;"),
            }
        }
        "textarea" => {
            source.push_str("display:inline-block;font-family:monospace;font-weight:normal;font-style:normal;line-height:normal;padding:2px;background-color:#ffffff;color:#000000;box-sizing:content-box;white-space:pre;");
            source.push_str(match browser {
                BrowserProfile::Chrome => "font-size:13.333333px;border:1px solid #767676;",
                BrowserProfile::Firefox => "font-size:medium;border:2px solid #767676;",
            });
        }
        "select" => {
            source.push_str("display:inline-block;font-family:sans-serif;font-size:13.333333px;font-weight:normal;font-style:normal;line-height:normal;background-color:#ffffff;color:#000000;box-sizing:border-box;");
            source.push_str(match browser {
                BrowserProfile::Chrome => "padding:0;border:1px solid #767676;",
                BrowserProfile::Firefox => "padding:1px 4px;border:2px solid #767676;",
            });
        }
        "hr" => source.push_str("margin:.5em 0;border:1px solid #808080;height:0;"),
        _ => {}
    }
    css::declarations(&source, warnings).into_iter().map(|(key, value, _)| (key, value)).collect()
}
fn button(source: &mut String, browser: BrowserProfile) {
    source.push_str("display:inline-block;font-family:sans-serif;font-size:13.333333px;font-weight:normal;font-style:normal;line-height:normal;box-sizing:border-box;border:2px solid #767676;background-color:#f0f0f0;color:#000000;text-align:center;");
    source.push_str(match browser {
        BrowserProfile::Chrome => "padding:1px 6px;",
        BrowserProfile::Firefox => "padding:1px 4px;",
    });
}

/// Native form metrics which are not expressible by UA CSS alone (size/cols/rows,
/// editable arrows and handles). These are deterministic portable approximations.
pub(crate) fn intrinsic(node: &Element, style: &mut Style, browser: BrowserProfile) {
    let attr =
        |name: &str, default: usize| node.attrs.get(name).and_then(|v| v.parse::<usize>().ok()).filter(|v| *v > 0 && *v <= 1000).unwrap_or(default) as f64;
    match node.tag.as_str() {
        "input" => match node.attrs.get("type").map_or("text", String::as_str).to_ascii_lowercase().as_str() {
            "button" | "submit" | "reset" | "checkbox" | "radio" | "range" | "hidden" => {}
            _ => {
                if style.width.is_none() && style.width_percent.is_none() {
                    style.width = Some((attr("size", 20) * 8.0 + 9.0) * style.font_size / 13.333333);
                }
                if style.height.is_none() && style.height_percent.is_none() {
                    style.height = Some(style.font_size * 1.125);
                }
            }
        },
        "textarea" => {
            if style.width.is_none() && style.width_percent.is_none() {
                style.width = Some(attr("cols", 20) * style.font_size * 0.6 + 2.0);
            }
            if style.height.is_none() && style.height_percent.is_none() {
                style.height = Some(attr("rows", 2) * style.font_size * 1.125);
            }
        }
        "select" => {
            // Space for the separately editable native-arrow approximation.
            style.padding[1] += SELECT_ARROW_SPACE;
            if style.height.is_none() && style.height_percent.is_none() {
                // Edge/Windows light resolves an additional two pixels of native
                // select metrics beyond its CSS line/border box (19px at 13.333px).
                let native_allowance = if browser == BrowserProfile::Chrome { 2.0 } else { 0.0 };
                style.height = Some(style.font_size * 1.125 + style.padding[0] + style.padding[2] + 2.0 * style.border_width + native_allowance);
            }
        }
        _ => {}
    }
}

/// Native-control behaviour seeded before author declarations, so explicit text
/// alignment and border radius always take precedence.
pub(crate) fn prepare(node: &Element, style: &mut Style) {
    let kind = node.attrs.get("type").map_or("text", String::as_str).to_ascii_lowercase();
    if node.tag == "button" || node.tag == "input" && matches!(kind.as_str(), "button" | "submit" | "reset") {
        style.align_center = true;
        style.justify_center = true;
    }
    if node.tag == "input" {
        match kind.as_str() {
            "radio" => style.radius = 6.5,
            "checkbox" => style.radius = 2.0,
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn styles(html: &str, css: &str, browser: BrowserProfile) -> (Vec<Element>, Vec<Style>) {
        let mut warnings = Vec::new();
        let (mut nodes, _) = crate::parse_html(html, &mut warnings).unwrap();
        crate::semantic::prepare(&mut nodes, &mut warnings).unwrap();
        let sheet = css::parse_stylesheet(css, &mut warnings).unwrap();
        let styles = crate::computed_with_browser(&nodes, &sheet.rules, css::State::Base, &mut warnings, browser);
        assert!(!warnings.iter().any(|w| w.contains("неподдерживаемое значение") || w.contains("свойство не поддерживается")), "{warnings:?}");
        (nodes, styles)
    }
    fn at<'a>(nodes: &[Element], styles: &'a [Style], id: &str) -> &'a Style {
        &styles[nodes.iter().position(|n| n.attrs.get("id").is_some_and(|v| v == id)).unwrap()]
    }
    #[test]
    fn default_entry_point_is_chrome_and_profile_is_recorded_in_source() {
        let html = "<button id='play'>Play</button>";
        let default = crate::import(html, "").unwrap();
        let chrome = crate::import_with_options(html, "", ImportOptions { browser: BrowserProfile::Chrome }).unwrap();
        let firefox = crate::import_with_options(html, "", ImportOptions { browser: BrowserProfile::Firefox }).unwrap();
        assert_eq!(default, chrome);
        assert!(chrome.source.contains("chrome") && chrome.source.contains(BrowserProfile::Chrome.source_revision()));
        assert!(firefox.source.contains("firefox") && firefox.source.contains(BrowserProfile::Firefox.source_revision()));
        let a = motif_script::parse(&chrome.source).unwrap().doc;
        let b = motif_script::parse(&firefox.source).unwrap().doc;
        assert!((a.node("play_bg").unwrap().width - b.node("play_bg").unwrap().width - 4.0).abs() < 1e-6);
    }
    #[test]
    fn source_based_control_differences_keep_native_parts_usable() {
        let html = "<button id='b'>B</button><input id='c' type='checkbox'><input id='r' type='radio'><textarea id='t'></textarea><select id='s'><option>One</option></select>";
        let (nodes, chrome) = styles(html, "", BrowserProfile::Chrome);
        let (_, firefox) = styles(html, "", BrowserProfile::Firefox);
        assert_eq!(at(&nodes, &chrome, "b").padding, [1.0, 6.0, 1.0, 6.0]);
        assert_eq!(at(&nodes, &firefox, "b").padding, [1.0, 4.0, 1.0, 4.0]);
        assert_eq!(at(&nodes, &chrome, "t").border_width, 1.0);
        assert_eq!(at(&nodes, &firefox, "t").border_width, 2.0);
        assert_eq!(at(&nodes, &chrome, "s").background, "#ffffff");
        assert!((at(&nodes, &chrome, "s").height.unwrap() - 19.0).abs() < 0.01);
        assert_eq!(at(&nodes, &chrome, "c").margin, [3.0, 3.0, 3.0, 4.0]);
        assert_eq!(at(&nodes, &chrome, "r").margin, [3.0, 3.0, 0.0, 5.0]);
        for profile in [&chrome, &firefox] {
            for id in ["c", "r"] {
                let s = at(&nodes, profile, id);
                assert_eq!(s.width, Some(13.0));
                assert_eq!(s.height, Some(13.0));
                assert!(s.border_width > 0.0);
            }
        }
    }
    #[test]
    fn author_shorthands_override_ua_longhands_and_em_margins_use_author_font_size() {
        for browser in [BrowserProfile::Chrome, BrowserProfile::Firefox] {
            let (nodes, s) = styles(
                "<p id='p'>P</p><ul id='u'><li>List</li></ul><h1 id='h'>Title</h1><button id='b'>B</button>",
                "p{margin:0}ul{padding:0}h1{font-size:20px}button{width:160px;border:0;padding:0;border-radius:0;text-align:left}",
                browser,
            );
            assert_eq!(at(&nodes, &s, "p").margin, [0.0; 4]);
            assert_eq!(at(&nodes, &s, "u").padding, [0.0; 4]);
            assert!((at(&nodes, &s, "h").margin[0] - 13.4).abs() < 1e-9);
            let b = at(&nodes, &s, "b");
            assert_eq!(b.padding, [0.0; 4]);
            assert_eq!(b.border_width, 0.0);
            assert_eq!(b.radius, 0.0);
            assert!(!b.align_center);
            let mut boxes = vec![crate::BoxNode::default(); nodes.len()];
            crate::layout::measure(0, &nodes, &s, &mut boxes);
            crate::layout::position(0, 0.0, 0.0, &nodes, &s, &mut boxes);
            let index = nodes.iter().position(|n| n.attrs.get("id").is_some_and(|v| v == "b")).unwrap();
            let text = nodes[index].children.iter().copied().find(|c| nodes[*c].tag == "#text").unwrap();
            assert!((boxes[text].x - boxes[index].x).abs() < 1e-9);
        }
    }
    #[test]
    fn font_inherit_shorthand_respects_source_order_and_parent_values() {
        for browser in [BrowserProfile::Chrome, BrowserProfile::Firefox] {
            let (nodes, s) = styles(
                "<div id='parent'><button id='a'>A</button><button id='b'>B</button></div>",
                "#parent{font-size:20px;font-weight:bold;font-style:italic;font-family:serif;line-height:1.7}#a{font:inherit;font-size:22px}#b{font-size:22px;font:inherit}",
                browser,
            );
            let a = at(&nodes, &s, "a");
            let b = at(&nodes, &s, "b");
            assert_eq!(a.font_size, 22.0);
            assert_eq!(b.font_size, 20.0);
            for s in [a, b] {
                assert_eq!(s.font_family, crate::font::Family::Serif);
                assert!(s.bold && s.italic);
                assert_eq!(s.line_height, 1.7);
            }
        }
    }
    #[test]
    fn monospace_medium_defaults_and_explicit_parent_sizes_match_the_browser_cases() {
        for browser in [BrowserProfile::Chrome, BrowserProfile::Firefox] {
            for (css, expected) in [
                ("", 13.0),
                ("pre{font-size:inherit}", 13.0),
                ("div{font-size:20px}", 20.0),
                ("div{font-size:16px}", 16.0),
                ("div{font-size:2em}", 26.0),
                ("pre{font-size:16px}", 16.0),
            ] {
                let (nodes, s) = styles("<div><pre id='pre'>code</pre></div>", css, browser);
                let pre = at(&nodes, &s, "pre");
                assert_eq!(pre.font_family, crate::font::Family::Mono);
                assert_eq!(pre.font_size, expected, "{browser:?}:{css}");
                assert_eq!(pre.margin[0], expected);
            }
            let (nodes, s) = styles("<body id='body'><ul id='outer'><li><ol id='inner'><li>Item</li></ol></li></ul></body>", "", browser);
            assert_eq!(at(&nodes, &s, "body").margin, [8.0; 4]);
            assert_eq!(at(&nodes, &s, "outer").margin[0], 16.0);
            assert_eq!(at(&nodes, &s, "inner").margin, [0.0; 4]);
        }
    }
    #[test]
    fn author_styles_produce_the_same_normal_hover_and_loop_in_both_profiles() {
        let html = "<button id='play'>Play</button>";
        let css = "button{width:160px;height:40px;padding:4px;border:2px solid #123456;border-radius:3px;font-size:16px;font-family:Arial;line-height:20px}button:hover{color:#ff0000;animation:move .4s linear infinite alternate}@keyframes move{from{transform:translateX(0)}to{transform:translateX(10px)}}";
        let a = crate::import_with_options(html, css, ImportOptions { browser: BrowserProfile::Chrome }).unwrap();
        let b = crate::import_with_options(html, css, ImportOptions { browser: BrowserProfile::Firefox }).unwrap();
        let a = motif_script::parse(&a.source).unwrap().doc;
        let b = motif_script::parse(&b.source).unwrap().doc;
        assert_eq!(a, b);
        for name in ["Normal", "Hover", "HoverLoop"] {
            let aa = a.animation(name).unwrap();
            let bb = b.animation(name).unwrap();
            for t in [0.0, 0.1, 0.2] {
                assert_eq!(aa.pose(&a, t), bb.pose(&b, t));
            }
        }
    }
    #[test]
    fn browser_control_metrics_are_preserved_in_all_state_and_keyframe_samples() {
        let html = "<button id='play'>Play</button>";
        let css = "button:hover{animation:move .4s linear infinite alternate}@keyframes move{from{transform:translateX(0)}to{transform:translateX(10px)}}";
        let a = crate::import_with_options(html, css, ImportOptions { browser: BrowserProfile::Chrome }).unwrap();
        let b = crate::import_with_options(html, css, ImportOptions { browser: BrowserProfile::Firefox }).unwrap();
        let a = motif_script::parse(&a.source).unwrap().doc;
        let b = motif_script::parse(&b.source).unwrap().doc;
        for name in ["Normal", "Hover", "HoverLoop"] {
            for t in [0.0, 0.1, 0.3] {
                let pa = a.animation(name).unwrap().pose(&a, t);
                let pb = b.animation(name).unwrap().pose(&b, t);
                assert!((pa.node("play_bg").unwrap().width - pb.node("play_bg").unwrap().width - 4.0).abs() < 1e-6, "{name}:{t}");
            }
        }
    }

    #[test]
    fn auto_sized_select_reserves_native_arrow_space_without_overlapping_its_caption() {
        for browser in [BrowserProfile::Chrome, BrowserProfile::Firefox] {
            for css in ["", "select{width:240px;padding:2px 18px}", "select{padding-right:0}"] {
                let result =
                    crate::import_with_options("<select id='choice'><option>Обычная сложность</option></select>", css, ImportOptions { browser }).unwrap();
                let doc = motif_script::parse(&result.source).unwrap().doc;
                let control = doc.index_of("choice").unwrap();
                let caption = doc.nodes.iter().position(|n| n.parent == Some(control) && n.text.is_some()).unwrap();
                let arrow = doc.index_of("choice_arrow").unwrap();
                let text_bounds = doc.world_bounds(caption).unwrap();
                let arrow_bounds = doc.world_bounds(arrow).unwrap();
                assert!(text_bounds.x1 < arrow_bounds.x0, "{browser:?} {css}: {text_bounds:?} vs {arrow_bounds:?}");
            }
        }
    }
}
