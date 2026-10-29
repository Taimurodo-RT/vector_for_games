use super::*;

pub(super) struct HtmlImport {
    pub open: bool,
    html: String,
    css: String,
    generated: Option<String>,
    warnings: Vec<String>,
    error: Option<String>,
    preview: Option<egui::TextureHandle>,
    elements: usize,
}

impl Default for HtmlImport {
    fn default() -> Self {
        Self { open:false, html:"<button class=\"action\">Начать игру</button>".into(),
            css:".action { width: 220px; height: 56px; background: #7667d9; color: #ffffff; border-radius: 14px; font-size: 20px; }\n.action:hover { background: #9587ef; }\n.action:active { background: #5546b8; }".into(),
            generated:None,warnings:vec![],error:None,preview:None,elements:0 }
    }
}

impl HtmlImport {
    fn invalidate(&mut self) {
        self.generated = None;
        self.preview = None;
        self.error = None;
        self.warnings.clear();
    }
    fn convert(&mut self, ctx: &egui::Context) {
        self.invalidate();
        match motif_html::import(&self.html, &self.css) {
            Ok(result) => match motif_script::parse(&result.source) {
                Ok(scene) => {
                    let scale = (400.0 / scene.doc.artboard.width).min(200.0 / scene.doc.artboard.height).min(1.0);
                    if let Some(px) = motif_render::render_cpu(&scene.doc, motif_render::CpuOptions { scale, ..Default::default() }) {
                        let image = egui::ColorImage::from_rgba_premultiplied([px.width() as usize, px.height() as usize], px.data());
                        self.preview = Some(ctx.load_texture("html-import-preview", image, egui::TextureOptions::LINEAR));
                    }
                    self.generated = Some(result.source);
                    self.warnings = result.warnings;
                    self.elements = result.elements;
                }
                Err(errors) => self.error = Some(errors.iter().map(ToString::to_string).collect::<Vec<_>>().join("\n")),
            },
            Err(error) => self.error = Some(error),
        }
    }
}

impl App {
    pub(super) fn html_import_window(&mut self, ctx: &egui::Context) {
        if !self.html_import.open {
            return;
        }
        let mut state = std::mem::take(&mut self.html_import);
        let mut open = true;
        egui::Window::new("Импорт HTML + CSS").id(egui::Id::unique("html-css-import")).open(&mut open).default_width(760.0).resizable(true).show(ctx, |ui| {
            ui.label("HTML задаёт элементы, CSS — их внешний вид. Результат добавляется отдельным редактируемым UI-холстом.");
            let mut changed = false;
            ui.columns(2, |columns| {
                let (left, right) = columns.split_at_mut(1);
                for (column, title, filter, content) in
                    [(&mut left[0], "HTML", vec!["html", "htm"], &mut state.html), (&mut right[0], "CSS", vec!["css"], &mut state.css)]
                {
                    column.horizontal(|ui| {
                        ui.strong(title);
                        if ui.button("Из файла…").clicked()
                            && let Some(file) = rfd::FileDialog::new().add_filter(title, &filter).pick_file()
                        {
                            match std::fs::read_to_string(&file) {
                                Ok(text) => {
                                    *content = text;
                                    changed = true;
                                }
                                Err(error) => state.error = Some(error.to_string()),
                            }
                        }
                    });
                    egui::ScrollArea::both().id_salt(("html-css-code", title)).max_height(220.0).show(column, |ui| {
                        changed |= ui.add(egui::TextEdit::multiline(content).code_editor().desired_width(f32::INFINITY).desired_rows(10)).changed();
                    });
                }
            });
            if changed {
                state.invalidate();
            }
            ui.horizontal(|ui| {
                if ui.button("Преобразовать").clicked() {
                    state.convert(ctx);
                }
                if ui.add_enabled(state.generated.is_some(), egui::Button::new("Добавить UI-холст")).clicked()
                    && self.add_imported_ui_board(state.generated.as_ref().unwrap())
                {
                    self.view = None;
                    state.open = false;
                }
            });
            if let Some(error) = &state.error {
                ui.colored_label(DANGER, error);
            }
            if let Some(texture) = &state.preview {
                ui.image(texture);
            }
            if state.generated.is_some() {
                ui.label(format!("Элементов: {}. Предупреждений: {}.", state.elements, state.warnings.len()));
            }
            egui::ScrollArea::vertical().id_salt("html-css-warnings").max_height(140.0).show(ui, |ui| {
                for warning in &state.warnings {
                    ui.colored_label(ACCENT, warning);
                }
            });
            ui.small("Преобразуется поддерживаемое подмножество HTML/CSS. Скрипты и внешние ресурсы не выполняются; ограничения перечислены в отчёте.");
        });
        if !open || ctx.input(|i| i.key_pressed(Key::Escape)) {
            state.open = false;
        }
        self.html_import = state;
    }
}

#[cfg(test)]
mod tests {
    use super::super::editing_tests::fixture;
    use super::*;

    #[test]
    fn html_import_adds_an_editable_board_and_undo_preserves_existing_character_and_ui() {
        let mut app = fixture(include_str!("../../../examples/character-anime.msc"));
        let before = app.file_text();
        let old = app.scene.doc.clone();
        app.time = 1.5;
        let imported = motif_html::import(
            "<button id=\"start\">Начать</button>",
            "button { width: 180px; height: 48px; background: #4455aa; } button:hover { background: #6677dd; }",
        )
        .unwrap();
        assert!(app.add_imported_ui_board(&imported.source));
        assert_eq!(app.board_count(), 2);
        assert_eq!(app.scene.doc.purpose, Purpose::Ui);
        assert_eq!(app.time, 0.0);
        assert!(app.scene.doc.nodes.iter().any(|n| n.ui));
        assert!(app.current_anim().is_some());
        assert_eq!(app.undo.len(), 1);
        app.switch_board(0);
        assert_eq!(app.scene.doc, old);
        app.undo();
        assert_eq!(app.file_text(), before);
        assert_eq!(app.board_count(), 1);
    }

    #[test]
    fn conversion_reports_limits_without_mutating_document_and_invalidates_stale_result() {
        let mut state = HtmlImport::default();
        let ctx = egui::Context::default();
        state.css.push_str(" button { unsupported-prop: 1; }");
        state.convert(&ctx);
        assert!(state.error.is_none());
        assert!(state.generated.is_some());
        assert!(!state.warnings.is_empty());
        state.html.push_str("<span>Ещё</span>");
        state.invalidate();
        assert!(state.generated.is_none() && state.preview.is_none());
        let mut app = fixture("artboard 200x200\nrect old size 40\n");
        let before = app.file_text();
        assert!(!app.add_imported_ui_board("not MSC"));
        assert_eq!(app.file_text(), before);
        assert!(app.undo.is_empty());
    }
}
