//! Native web export settings. Stored in the script, undoable with the rest of the scene.
use crate::app::App;
use crate::theme::{self, ButtonKind};
use motif_web::{Config, Profile};

#[derive(Default)]
pub(crate) struct WebState {
    draft: Option<Config>,
    base: Option<Config>,
    board: String,
    error: String,
    pub preview_theme: String,
    advanced: String,
}

fn point(ui: &mut egui::Ui, label: &str, p: &mut Option<[f64; 2]>) {
    ui.horizontal(|ui| {
        let mut on = p.is_some();
        if ui.checkbox(&mut on, label).changed() {
            *p = if on { Some([0.0, 0.0]) } else { None };
        }
        if let Some(p) = p {
            for v in p {
                ui.add(egui::DragValue::new(v).speed(0.5));
            }
        }
    });
}

impl App {
    pub(crate) fn web_export(&mut self, ui: &mut egui::Ui) {
        theme::caption(ui, "SVG / Web assets");
        ui.horizontal(|ui| {
            if theme::button(ui, "SVG холста…", ButtonKind::Normal).clicked() {
                let result = (|| {
                    if !self.errors.is_empty() {
                        return Err("Исправьте ошибки скрипта перед экспортом".into());
                    }
                    let config = motif_web::read(&self.src)?;
                    let svg = motif_web::render(&self.scene, &config, None)?;
                    let Some(dest) =
                        rfd::FileDialog::new().add_filter("SVG", &["svg"]).set_file_name(svg.file.rsplit('/').next().unwrap_or("asset.svg")).save_file()
                    else {
                        return Ok("Экспорт отменён".into());
                    };
                    std::fs::write(&dest, &svg.text).map_err(|e| e.to_string())?;
                    Ok(format!("SVG: {}{}", dest.display(), if svg.warnings.is_empty() { String::new() } else { format!(" · {}", svg.warnings.join(" · ")) }))
                })();
                self.status = result.unwrap_or_else(|e: String| format!("SVG: {e}"));
            }
            if theme::button(ui, "Web-пакет…", ButtonKind::Normal).clicked() {
                let result = (|| {
                    let src = if self.export.all_boards { self.file_text() } else { self.src.clone() };
                    let config = motif_web::read(&src)?;
                    let scenes = motif_script::parse_boards(&src, self.path.as_deref().and_then(|p| p.parent()))
                        .map_err(|e| e.iter().map(ToString::to_string).collect::<Vec<_>>().join("\n"))?;
                    let package = motif_web::build(&scenes, &config, None)?;
                    let Some(dir) = rfd::FileDialog::new().set_title("Папка web-пакета: SVG, CSS, graphics.json, index.html").pick_folder() else {
                        return Ok("Экспорт отменён".into());
                    };
                    motif_web::export(&package, &dir)?;
                    Ok(format!("Web: {} · ассетов {} · предупреждений {} (graphics.json)", dir.display(), package.assets, package.warnings.len()))
                })();
                self.status = result.unwrap_or_else(|e: String| format!("Web: {e}"));
            }
        });
        if theme::button(ui, "Токены, темы и метаданные…", ButtonKind::Normal).clicked() {
            match motif_web::read(&self.src) {
                Ok(config) => {
                    self.web.advanced = serde_json::to_string_pretty(&config).unwrap_or_default();
                    self.web.base = Some(config.clone());
                    self.web.draft = Some(config);
                    self.web.board = self.scene.doc.artboard.name.clone();
                    self.web.error.clear();
                }
                Err(e) => self.status = e,
            }
        }
        if let Ok(config) = motif_web::read(&self.src) {
            egui::ComboBox::from_label("Тема холста (просмотр)")
                .selected_text(if self.web.preview_theme.is_empty() { "Исходная" } else { &self.web.preview_theme })
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut self.web.preview_theme, String::new(), "Исходная");
                    for name in config.themes.keys() {
                        ui.selectable_value(&mut self.web.preview_theme, name.clone(), name);
                    }
                });
        }
        ui.label(egui::RichText::new("Прозрачный SVG; анимации web — в CSS и манифесте. index.html показывает темы, 100%/200%, тайлы и 9-slice.").small());
        theme::rule(ui);
    }

    pub(crate) fn web_window(&mut self, ctx: &egui::Context) {
        let Some(mut config) = self.web.draft.take() else { return };
        let mut open = true;
        let mut apply = false;
        egui::Window::new("Web assets · настройки в .msc").open(&mut open).default_width(560.0).default_height(650.0).show(ctx, |ui| {
            ui.label("Применить создаёт один шаг Undo. Затем сохраните .msc обычным Ctrl+S.");
            egui::ScrollArea::vertical().max_height(540.0).show(ui, |ui| {
                ui.collapsing("Цветовые токены", |ui| {
                    ui.label("Пусто = HEX; currentColor = цвет текста; --имя = CSS-переменная.");
                    for swatch in &self.scene.palette {
                        ui.horizontal(|ui| {
                            ui.label(format!("{} {}", swatch.name, swatch.color));
                            let mut binding = config.tokens.get(&swatch.name).cloned().unwrap_or_default();
                            if ui.text_edit_singleline(&mut binding).changed() {
                                if binding.is_empty() {
                                    config.tokens.remove(&swatch.name);
                                } else {
                                    config.tokens.insert(swatch.name.clone(), binding);
                                }
                            }
                        });
                    }
                });
                ui.collapsing("Темы", |ui| {
                    if ui.button("Добавить тему dark").clicked() {
                        config.themes.entry("dark".into()).or_insert_with(|| {
                            self.scene.palette.iter().filter(|s| config.tokens.contains_key(&s.name)).map(|s| (s.name.clone(), s.color.to_hex())).collect()
                        });
                    }
                    let mut remove = None;
                    for (name, colors) in &mut config.themes {
                        ui.horizontal(|ui| {
                            ui.strong(name);
                            if ui.small_button("Удалить").clicked() {
                                remove = Some(name.clone());
                            }
                        });
                        for token in config.tokens.keys() {
                            ui.horizontal(|ui| {
                                ui.label(token);
                                let mut color = colors.get(token).cloned().unwrap_or_else(|| {
                                    self.scene.palette.iter().find(|s| s.name == *token).map(|s| s.color.to_hex()).unwrap_or_else(|| "#000000".into())
                                });
                                if ui.text_edit_singleline(&mut color).changed() {
                                    colors.insert(token.clone(), color);
                                }
                            });
                        }
                    }
                    if let Some(name) = remove {
                        config.themes.remove(&name);
                    }
                });
                ui.separator();
                ui.strong(format!("Холст: {}", self.web.board));
                let asset = config.assets.entry(self.web.board.clone()).or_default();
                egui::Grid::new("web-asset-fields").num_columns(2).show(ui, |ui| {
                    for (label, value) in
                        [("ID ассета", &mut asset.id), ("Путь SVG", &mut asset.file), ("Вариант", &mut asset.variant), ("Стадия", &mut asset.stage)]
                    {
                        ui.label(label);
                        ui.text_edit_singleline(value);
                        ui.end_row();
                    }
                    ui.label("Профиль");
                    egui::ComboBox::from_id_salt("web-profile").selected_text(asset.profile.name()).show_ui(ui, |ui| {
                        for p in Profile::ALL {
                            ui.selectable_value(&mut asset.profile, p, p.name());
                        }
                    });
                    ui.end_row();
                    ui.label("Ключевые слова");
                    let mut words = asset.keywords.join(", ");
                    if ui.text_edit_singleline(&mut words).changed() {
                        asset.keywords = words.split(',').map(str::trim).filter(|s| !s.is_empty()).map(String::from).collect();
                    }
                    ui.end_row();
                });
                point(ui, "Pivot (холст)", &mut asset.pivot);
                point(ui, "Hotspot (холст)", &mut asset.hotspot);
                ui.horizontal(|ui| {
                    let mut on = asset.slices.is_some();
                    if ui.checkbox(&mut on, "9-slice T/R/B/L").changed() {
                        asset.slices = if on { Some([4.0; 4]) } else { None };
                    }
                    if let Some(s) = &mut asset.slices {
                        for v in s {
                            ui.add(egui::DragValue::new(v).range(0.0..=10000.0));
                        }
                    }
                });
                ui.checkbox(&mut asset.manual_fix, "Требует ручной правки");
                ui.collapsing("ID и CSS class слоёв", |ui| {
                    for n in &self.scene.doc.nodes {
                        ui.push_id(&n.id, |ui| {
                            ui.horizontal(|ui| {
                                ui.label(&n.id);
                                let layer = asset.layers.entry(n.id.clone()).or_default();
                                ui.add(egui::TextEdit::singleline(&mut layer.id).hint_text("id (необязательно)").desired_width(140.0));
                                ui.add(egui::TextEdit::singleline(&mut layer.class).hint_text("base accent anim-body…").desired_width(180.0));
                            });
                        });
                    }
                    asset.layers.retain(|_, l| !l.id.is_empty() || !l.class.is_empty());
                });
                ui.collapsing("JSON — все холсты и произвольные имена тем", |ui| {
                    if ui.button("Обновить JSON из формы").clicked() {
                        self.web.advanced = serde_json::to_string_pretty(&config).unwrap_or_default();
                    }
                    ui.add(egui::TextEdit::multiline(&mut self.web.advanced).font(egui::TextStyle::Monospace).desired_rows(12).desired_width(f32::INFINITY));
                    if ui.button("Загрузить JSON в форму").clicked() {
                        match serde_json::from_str::<Config>(&self.web.advanced) {
                            Ok(c) => {
                                config = c;
                                self.web.error.clear();
                            }
                            Err(e) => self.web.error = e.to_string(),
                        }
                    }
                });
            });
            if !self.web.error.is_empty() {
                ui.colored_label(egui::Color32::LIGHT_RED, &self.web.error);
            }
            apply = ui.button("Применить к .msc").clicked();
        });
        if apply {
            let result = (|| {
                if motif_web::read(&self.src).ok() != self.web.base || self.scene.doc.artboard.name != self.web.board {
                    return Err("Сцена или её настройки изменились. Закройте окно и откройте заново.".into());
                }
                let src = motif_web::write(&self.src, &config)?;
                // Export preflight catches stale layer IDs and bad slice coordinates before storing.
                motif_web::render(&self.scene, &config, None)?;
                Ok(src)
            })();
            match result {
                Ok(src) => {
                    self.apply("web-settings", Some(src));
                    self.status = "Web-настройки применены; Ctrl+S сохранит их в .msc".into();
                    open = false;
                }
                Err(e) => self.web.error = e,
            }
        }
        if open {
            self.web.draft = Some(config);
        }
    }
}
