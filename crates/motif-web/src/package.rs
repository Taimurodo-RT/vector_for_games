use crate::{Config, Profile, Svg, config, num, render, xml};
use motif_core::{Prop, Value};
use motif_script::Scene;
use serde_json::{Value as Json, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

pub struct Package {
    pub files: BTreeMap<String, String>,
    pub warnings: Vec<String>,
    pub assets: usize,
}

fn value(v: Value) -> Json {
    match v {
        Value::Num(n) => json!(n),
        Value::Color(c) => json!(c.to_hex()),
        Value::None => Json::Null,
    }
}

fn animation(scene: &Scene, svg: &Svg) -> (Json, String, Vec<String>) {
    let mut list = Vec::new();
    let mut css = String::new();
    let mut warnings = Vec::new();
    let mut names = BTreeSet::new();
    for a in &scene.doc.animations {
        let name = config::slug(&a.name);
        let tracks:Vec<_>=a.tracks.iter().map(|t|json!({"node":t.node,"svg_id":svg.node_ids.get(&t.node),"property":t.prop.path_name(),"keys":t.keys.iter().map(|k|json!({"time":k.time,"value":value(k.value),"ease":k.ease.script()})).collect::<Vec<_>>()})).collect();
        let allowed = |p| matches!(p, Prop::X | Prop::Y | Prop::Rotation | Prop::ScaleX | Prop::ScaleY | Prop::SkewX | Prop::SkewY | Prop::Opacity);
        let supported = a.tracks.iter().all(|t| allowed(t.prop))
            && a.duration > 0.0
            && a.duration <= 60.0
            && names.insert(name.clone())
            && scene
                .doc
                .nodes
                .iter()
                .all(|n| n.ik.is_none() && n.skin.is_none() && n.spring.is_none() && n.jiggle.is_none() && n.swing == 0.0 && n.squash.is_none());
        list.push(json!({"name":a.name,"selector":name,"duration":a.duration,"loop":a.looping,"css":supported,"sampling_fps":if supported {Some(60)} else {None},"tracks":tracks,"events":a.events.iter().map(|e|json!({"name":e.name,"time":e.time})).collect::<Vec<_>>()}));
        if !supported {
            warnings.push(format!("{} / {}: сохранены ключи в graphics.json; CSS для этой анимации не создан", svg.id, a.name));
            continue;
        }
        let mut times: Vec<f64> = (0..=(a.duration * 60.0).ceil() as usize).map(|i| (i as f64 / 60.0).min(a.duration)).collect();
        // Include exact keys and both sides of discontinuities, including Hold/Steps.
        times.extend(a.tracks.iter().flat_map(|t| t.keys.iter().flat_map(|k| [k.time, (k.time - 0.00001).max(0.0)])));
        times.retain(|t| *t >= 0.0 && *t <= a.duration);
        times.sort_by(f64::total_cmp);
        times.dedup_by(|a, b| (*a - *b).abs() < 0.000001);
        let ancestor_chain = |id: &str| {
            let mut chain = Vec::new();
            let mut at = scene.doc.index_of(id);
            while let Some(i) = at {
                chain.push(scene.doc.nodes[i].id.clone());
                at = scene.doc.nodes[i].parent;
            }
            chain
        };
        let mut nodes: BTreeSet<_> = a.tracks.iter().map(|t| t.node.as_str()).collect();
        for n in &scene.doc.nodes {
            if !n.kind.is_container() && a.tracks.iter().any(|t| t.prop == Prop::Opacity && ancestor_chain(&n.id).contains(&t.node)) {
                nodes.insert(&n.id);
            }
        }
        for node in nodes {
            let Some(id) = svg.node_ids.get(node) else { continue };
            let key = format!("{id}-{name}");
            let changes_transform = a.tracks.iter().any(|t| t.node == node && t.prop != Prop::Opacity);
            let ancestors = ancestor_chain(node);
            let changes_opacity =
                scene.doc.node(node).is_some_and(|n| !n.kind.is_container()) && a.tracks.iter().any(|t| ancestors.contains(&t.node) && t.prop == Prop::Opacity);
            if !changes_transform && !changes_opacity {
                continue;
            }
            css.push_str(&format!(
                "[data-motion=\"{name}\"] #{id}{{transform-box:view-box;transform-origin:0 0;animation:{key} {}s linear {} both;}}\n@keyframes {key}{{\n",
                num(a.duration),
                if a.looping { "infinite" } else { "1" }
            ));
            for &time in &times {
                let Some(original) = scene.doc.node(node) else { continue };
                let mut n = original.clone();
                for track in a.tracks.iter().filter(|t| t.node == node) {
                    if let Some(v) = track.sample(time) {
                        n.set(track.prop, v);
                    }
                }
                css.push_str(&format!("{}%{{", motif_script::fmt_num_with(time / a.duration * 100.0, 6)));
                if changes_transform {
                    css.push_str(&format!("transform:matrix({});", crate::svg::matrix(n.local_transform()).replace(' ', ",")));
                }
                if changes_opacity {
                    let opacity = ancestors
                        .iter()
                        .map(|id| {
                            a.tracks
                                .iter()
                                .find(|t| t.node == *id && t.prop == Prop::Opacity)
                                .and_then(|t| t.sample(time))
                                .and_then(Value::as_num)
                                .unwrap_or_else(|| scene.doc.node(id).unwrap().opacity)
                        })
                        .product::<f64>()
                        .clamp(0.0, 1.0);
                    css.push_str(&format!("fill-opacity:{};stroke-opacity:{};", num(opacity), num(opacity)));
                }
                css.push_str("}\n");
            }
            css.push_str("}\n");
        }
    }
    (json!(list), css, warnings)
}

pub fn build(scenes: &[Scene], config: &Config, profile: Option<Profile>) -> Result<Package, String> {
    if scenes.is_empty() {
        return Err("Нет холстов для экспорта".into());
    }
    config::validate(config)?;
    let mut out = Package { files: BTreeMap::new(), warnings: Vec::new(), assets: scenes.len() };
    let mut ids = BTreeSet::new();
    let mut paths = BTreeSet::new();
    let mut prefixes = BTreeSet::new();
    let mut manifest = Vec::new();
    let mut css = String::new();
    let mut previews = Vec::new();
    let mut palette = BTreeMap::new();
    for scene in scenes {
        let svg = render(scene, config, profile)?;
        let variant = config.assets.get(&scene.doc.artboard.name).map(|a| a.variant.clone()).unwrap_or_default();
        if !ids.insert((svg.id.clone(), variant)) || !paths.insert(svg.file.to_ascii_lowercase()) {
            return Err(format!("Повторяющийся id или путь: {} / {}", svg.id, svg.file));
        }
        if !prefixes.insert(config::slug(&svg.file.trim_end_matches(".svg").replace('/', "-"))) {
            return Err(format!("Пути создают одинаковый префикс SVG id: {}", svg.file));
        }
        for s in &scene.palette {
            if config.tokens.contains_key(&s.name)
                && let Some(old) = palette.insert(s.name.clone(), s.color.to_hex())
                && old != s.color.to_hex()
            {
                return Err(format!("У токена {} разные цвета на холстах; задайте разные имена", s.name));
            }
        }
        let asset = config.assets.get(&scene.doc.artboard.name).cloned().unwrap_or_default();
        let (animations, anim_css, warnings) = animation(scene, &svg);
        css.push_str(&anim_css);
        out.warnings.extend(warnings);
        out.warnings.extend(svg.warnings.iter().map(|w| format!("{}: {w}", svg.id)));
        let nodes:Vec<_>=scene.doc.nodes.iter().map(|n|json!({"id":n.id,"svg_id":svg.node_ids[&n.id],"name":n.name,"parent":n.parent.map(|i|&scene.doc.nodes[i].id),"pivot":[n.pivot.x,n.pivot.y],"slice":n.slice,"transform":n.local_transform().as_coeffs()})).collect();
        let p = profile.unwrap_or(asset.profile);
        let item = json!({"id":svg.id,"file":svg.file,"size":[scene.doc.artboard.width,scene.doc.artboard.height],"variant":asset.variant,"stage":asset.stage,"keywords":asset.keywords,"profile":p.name(),"pivot":asset.pivot,"hotspot":asset.hotspot,"slices":asset.slices,"manual_fix":asset.manual_fix,"metadata":asset.metadata,"anim":animations,"nodes":nodes,"bytes":svg.text.len(),"warnings":svg.warnings});
        manifest.push(item.clone());
        previews.push((item, svg.text.clone()));
        out.files.insert(svg.file, svg.text);
    }
    let mut bindings = BTreeMap::new();
    for (name, binding) in &config.tokens {
        let Some(color) = palette.get(name) else {
            out.warnings.push(format!("Токен {name} не найден в палитре выбранных холстов"));
            continue;
        };
        if let Some(old) = bindings.insert(binding.clone(), color.clone())
            && old != *color
        {
            return Err(format!("Два разных цвета связаны с {binding}"));
        }
    }
    let declarations = |colors: &BTreeMap<String, String>| -> Result<String, String> {
        let mut values = bindings.clone();
        let mut overrides = BTreeMap::new();
        for (name, color) in colors {
            let binding = config.tokens.get(name).ok_or_else(|| format!("У цвета темы {name} нет CSS-токена"))?;
            if let Some(old) = overrides.insert(binding.clone(), color.clone())
                && old != *color
            {
                return Err(format!("Тема задаёт разные значения для {binding}"));
            }
            values.insert(binding.clone(), color.clone());
        }
        Ok(values.iter().map(|(b, c)| format!("{}:{c};", if b == "currentColor" { "color" } else { b })).collect())
    };
    let mut tokens = format!(":root,[data-theme=\"default\"]{{{}}}\n", declarations(&BTreeMap::new())?);
    for (name, colors) in &config.themes {
        tokens.push_str(&format!("[data-theme=\"{name}\"]{{{}}}\n", declarations(colors)?));
    }
    css.push_str("@media(prefers-reduced-motion:reduce){[data-motion] [id]{animation:none!important;}}\n");
    out.files.insert("tokens.css".into(), tokens.clone());
    out.files.insert("animations.css".into(), css.clone());
    out.files.insert("graphics.json".into(),serde_json::to_string_pretty(&json!({"schema":"motif-web/1","coordinate_system":"SVG viewBox; slices: top,right,bottom,left; node pivots are local","assets":manifest,"tokens":config.tokens,"themes":config.themes,"warnings":out.warnings})).map_err(|e|e.to_string())?);
    out.files.insert("index.html".into(), gallery(&previews, config, &tokens, &css));
    Ok(out)
}

/// Inline SVG is intentional: currentColor and CSS variables cannot inherit through <img>.
/// Every preview gets isolated SVG IDs; fragment references and animation selectors follow.
pub fn gallery(previews: &[(Json, String)], config: &Config, tokens: &str, css: &str) -> String {
    let mut cards = String::new();
    let mut motion_css = String::new();
    for (i, (meta, svg)) in previews.iter().enumerate() {
        let mut copies = String::new();
        let width = meta["size"][0].as_f64().unwrap_or(24.0);
        let height = meta["size"][1].as_f64().unwrap_or(24.0);
        let modes = if meta["profile"] == "tile" { 9 } else { 2 };
        for j in 0..modes {
            let suffix = format!("-preview-{i}-{j}");
            let mut copy = svg.clone();
            let mut style = css.to_string();
            // Collect every generated id, including defs and title, without touching text.
            let ids: Vec<String> = svg.split(" id=\"").skip(1).filter_map(|s| s.split('"').next().map(str::to_string)).collect();
            for id in &ids {
                copy = copy
                    .replace(&format!("id=\"{id}\""), &format!("id=\"{id}{suffix}\""))
                    .replace(&format!("url(#{id})"), &format!("url(#{id}{suffix})"))
                    .replace(&format!("aria-labelledby=\"{id}\""), &format!("aria-labelledby=\"{id}{suffix}\""));
                style = style.replace(&format!("#{id}{{"), &format!("#{id}{suffix}{{"));
            }
            motion_css.push_str(&style);
            let scale = if modes == 2 { j + 1 } else { 1 };
            copies.push_str(&format!(
                "<div class=\"sample\" style=\"width:{}px;height:{}px\">{copy}</div>",
                num(width * scale as f64),
                num(height * scale as f64)
            ));
        }
        let options = meta["anim"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter(|a| a["css"] == true)
                    .map(|a| format!("<option value=\"{}\">{}</option>", xml(a["selector"].as_str().unwrap_or("")), xml(a["name"].as_str().unwrap_or(""))))
                    .collect::<String>()
            })
            .unwrap_or_default();
        let motion = if options.is_empty() {
            String::new()
        } else {
            format!("<label>Анимация <select class=\"motion\"><option value=\"\">Статика</option>{options}</select></label>")
        };
        let slice = if let Some(s) = meta["slices"].as_array() {
            let slices = s.iter().map(|v| v.to_string()).collect::<Vec<_>>().join(" ");
            // In an image document CSS tokens use the SVG fallbacks. Label this explicitly.
            format!(
                "<div class=\"slice\" style=\"border-image-source:url('{}');border-image-slice:{} fill;border-width:{}px {}px {}px {}px\">9-slice — тяни за угол</div><small>9-slice: цвета по умолчанию из SVG</small>",
                xml(meta["file"].as_str().unwrap_or("")),
                slices,
                s[0],
                s[1],
                s[2],
                s[3]
            )
        } else {
            String::new()
        };
        let notes = meta["warnings"].as_array().map(|a| a.iter().filter_map(|v| v.as_str()).map(xml).collect::<Vec<_>>().join(" · ")).unwrap_or_default();
        cards.push_str(&format!("<article><h2>{}</h2><p>{} · {} байт · {}</p>{motion}<div class=\"samples {}\">{copies}</div>{slice}<p class=\"notes\">{notes}</p><a href=\"{}\">SVG</a></article>",xml(meta["id"].as_str().unwrap_or("")),xml(meta["profile"].as_str().unwrap_or("")),meta["bytes"],if modes==9 {"сетка 3×3"}else{"100% / 200%"},if modes==9 {"tiles"}else{""},xml(meta["file"].as_str().unwrap_or(""))));
    }
    let themes = config.themes.keys().map(|t| format!("<option>{}</option>", xml(t))).collect::<String>();
    format!(
        "<!doctype html><html lang=\"ru\"><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width,initial-scale=1\"><title>Motif · Web assets</title><style>{tokens}\n{motion_css}\n{}</style><body data-theme=\"default\"><header><h1>Motif · Web assets</h1><p>SVG в контексте. Масштаб браузера 100%. Технические предупреждения — в graphics.json.</p><label>Тема <select id=\"theme\"><option>default</option>{themes}</select></label> <label>Фон <select id=\"surface\"><option value=\"paper\">Светлый</option><option value=\"dark\">Тёмный</option><option value=\"checker\">Шахматный</option></select></label> <a href=\"graphics.json\">Манифест</a></header><main>{cards}</main><script>{}</script></body></html>",
        include_str!("gallery.css"),
        include_str!("gallery.js")
    )
}

/// Build first, then write. A bad board cannot leave a partial logical export behind.
/// Each individual file is replaced atomically; IO failures are reported, never hidden.
pub fn export(package: &Package, dir: &Path) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let root = dir.canonicalize().map_err(|e| e.to_string())?;
    for name in package.files.keys() {
        let dest = root.join(name);
        let parent = dest.parent().ok_or("Нет папки назначения")?;
        let mut ancestor = parent;
        while !ancestor.exists() {
            ancestor = ancestor.parent().ok_or("Нет родителя")?;
        }
        if !ancestor.canonicalize().map_err(|e| e.to_string())?.starts_with(&root) {
            return Err(format!("Путь выходит из папки экспорта: {name}"));
        }
        if dest.is_symlink() {
            return Err(format!("Экспорт не перезаписывает ссылки: {name}"));
        }
    }
    for (name, content) in &package.files {
        let dest = root.join(name);
        let parent = dest.parent().unwrap();
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        let tmp = parent.join(format!(".motif-{}-{}.tmp", std::process::id(), config::slug(name)));
        let mut file = std::fs::OpenOptions::new().write(true).create_new(true).open(&tmp).map_err(|e| format!("{}: {e}", tmp.display()))?;
        use std::io::Write;
        if let Err(e) = file.write_all(content.as_bytes()).and_then(|()| file.sync_all()) {
            let _ = std::fs::remove_file(&tmp);
            return Err(e.to_string());
        }
        drop(file);
        if let Err(e) = std::fs::rename(&tmp, &dest) {
            let _ = std::fs::remove_file(&tmp);
            return Err(format!("{}: {e}", dest.display()));
        }
    }
    Ok(())
}
