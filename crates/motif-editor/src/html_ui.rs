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
    shown: bool,
}

impl Default for HtmlImport {
    fn default() -> Self {
        Self { open:false, html:"<button class=\"action\">Начать игру</button>".into(),
            css:".action { width: 220px; height: 56px; background: #7667d9; color: #ffffff; border-radius: 14px; font-size: 20px; }\n.action:hover { background: #9587ef; }\n.action:active { background: #5546b8; }".into(),
            generated:None,warnings:vec![],error:None,preview:None,elements:0,shown:false }
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
            self.html_import.shown = false;
            return;
        }
        let mut state = std::mem::take(&mut self.html_import);
        let mut focus_target = None;
        let mut paste_target = None;
        if !state.shown {
            focus_target = Some(html_field_id("HTML"));
            state.shown = true;
        }
        let mut open = true;
        let child_popup_was_open = egui::Popup::is_any_open(ctx);
        egui::Window::new("Импорт HTML + CSS").id(egui::Id::unique("html-css-import")).open(&mut open).default_width(760.0).resizable(true).show(ctx, |ui| {
            ui.label("HTML задаёт элементы, CSS — их внешний вид. Результат добавляется отдельным редактируемым UI-холстом.");
            let mut changed = false;
            ui.columns(2, |columns| {
                let (left, right) = columns.split_at_mut(1);
                for (column, title, filter, content) in
                    [(&mut left[0], "HTML", vec!["html", "htm"], &mut state.html), (&mut right[0], "CSS", vec!["css"], &mut state.css)]
                {
                    let field_id = html_field_id(title);
                    column.horizontal(|ui| {
                        ui.strong(title);
                        if ui.button(format!("Вставить {title}")).clicked() {
                            paste_target = Some(field_id);
                        }
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
                        let response = ui.add(egui::TextEdit::multiline(content).id(field_id).code_editor().desired_width(f32::INFINITY).desired_rows(10));
                        changed |= response.changed();
                        response.context_menu(|ui| {
                            if ui.button("Вставить").clicked() {
                                paste_target = Some(field_id);
                                ui.close();
                            }
                        });
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
        // TextEdit processes outside clicks while it is drawn. Restore the requested focus
        // afterwards so the same click on a paste button cannot immediately surrender it.
        if let Some(field) = paste_target {
            request_paste(ctx, field);
        } else if let Some(field) = focus_target {
            ctx.memory_mut(|m| m.request_focus(field));
        }
        if !open || !child_popup_was_open && ctx.input(|i| i.key_pressed(Key::Escape)) {
            state.open = false;
        }
        self.html_import = state;
    }
}

fn html_field_id(title: &str) -> egui::Id {
    egui::Id::unique(("html-css-field", title))
}

fn request_paste(ctx: &egui::Context, field: egui::Id) {
    ctx.memory_mut(|m| m.request_focus(field));
    ctx.send_viewport_cmd(egui::ViewportCommand::RequestPaste);
}

#[cfg(test)]
mod tests {
    use super::super::editing_tests::fixture;
    use super::*;

    fn input_frame(app: &mut App, mut events: Vec<egui::Event>) -> egui::FullOutput {
        let ctx = app.ctx.clone();
        let modifiers = events.iter().rev().find_map(|e| if let egui::Event::Key { modifiers, .. } = e { Some(*modifiers) } else { None }).unwrap_or_default();
        events.insert(0, egui::Event::ModifiersChanged(modifiers));
        ctx.run_ui(egui::RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1600.0, 850.0))), events, ..Default::default() }, |ui| {
            app.shortcuts(&ctx);
            egui::Panel::top("test-main-top").exact_size(80.0).show(ui, |ui| app.top_bar(ui));
            app.workspace_panels(ui);
            egui::CentralPanel::no_frame().show(ui, |ui| app.canvas_ui(ui, None));
            app.html_import_window(&ctx);
        })
    }

    fn field_point(app: &mut App, text: &str) -> Pos2 {
        for _ in 0..3 {
            input_frame(app, vec![]).drop_without_applying_deltas();
        }
        let output = input_frame(app, vec![]);
        let point = output
            .shapes
            .iter()
            .find_map(|shape| match &shape.shape {
                egui::Shape::Text(t) if t.galley.job.text == text => Some(t.pos + vec2(5.0, 8.0)),
                _ => None,
            })
            .unwrap_or_else(|| panic!("missing field {text}"));
        output.drop_without_applying_deltas();
        point
    }

    fn focus_at(app: &mut App, p: Pos2) {
        for pressed in [true, false] {
            input_frame(
                app,
                vec![
                    egui::Event::PointerMoved(p),
                    egui::Event::PointerButton { pos: p, button: egui::PointerButton::Primary, pressed, modifiers: Modifiers::NONE },
                ],
            )
            .drop_without_applying_deltas();
        }
    }

    fn select_all_event() -> egui::Event {
        egui::Event::Key {
            key: Key::A,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Modifiers { ctrl: true, command: true, ..Default::default() },
        }
    }

    fn ctrl_v_paste(text: &str) -> Vec<egui::Event> {
        let command = Modifiers { ctrl: true, command: true, ..Default::default() };
        vec![egui::Event::Key { key: Key::V, physical_key: None, pressed: true, repeat: false, modifiers: command }, egui::Event::Paste(text.into())]
    }

    fn control_point(app: &mut App, text: &str) -> Pos2 {
        let out = input_frame(app, vec![]);
        let point = out
            .shapes
            .iter()
            .find_map(|s| match &s.shape {
                egui::Shape::Text(t) if t.galley.job.text == text => Some(t.pos + t.galley.size() * 0.5),
                _ => None,
            })
            .unwrap_or_else(|| panic!("missing {text}"));
        out.drop_without_applying_deltas();
        point
    }

    fn click_control(app: &mut App, text: &str) -> egui::FullOutput {
        let at = control_point(app, text);
        input_frame(
            app,
            vec![
                egui::Event::PointerMoved(at),
                egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE },
            ],
        )
        .drop_without_applying_deltas();
        input_frame(
            app,
            vec![
                egui::Event::PointerMoved(at),
                egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE },
            ],
        )
    }

    fn requested_paste(out: &egui::FullOutput) -> bool {
        out.viewport_output.values().any(|output| output.commands.iter().any(|command| matches!(command, egui::ViewportCommand::RequestPaste)))
    }

    #[test]
    fn html_and_css_accept_paste_without_changing_the_scene() {
        let mut app = fixture("artboard 200x200\nrect old size 40\n");
        app.html_import.open = true;
        let scene = app.src.clone();
        let html = app.html_import.html.clone();
        let css = app.html_import.css.clone();
        let p = field_point(&mut app, &html);
        focus_at(&mut app, p);
        assert!(app.ctx.text_edit_focused());
        input_frame(&mut app, vec![egui::Event::Paste("<span>Вставка HTML</span>".into())]).drop_without_applying_deltas();
        assert!(app.html_import.html.contains("Вставка HTML"));
        assert_eq!(app.html_import.css, css);
        assert_eq!(app.src, scene);
        let html_after = app.html_import.html.clone();
        let p = field_point(&mut app, &css);
        focus_at(&mut app, p);
        input_frame(&mut app, vec![select_all_event()]).drop_without_applying_deltas();
        input_frame(&mut app, ctrl_v_paste("button { color: #123456; }")).drop_without_applying_deltas();
        assert_eq!(app.html_import.css, "button { color: #123456; }");
        assert_eq!(app.src, scene);
        assert_eq!(app.html_import.html, html_after);
    }

    #[test]
    fn pasted_multiline_html_and_css_preserve_tabs_newlines_and_selection_replacement() {
        let mut app = fixture("artboard 200x200\nrect old size 40\n");
        app.html_import.open = true;
        let original = app.src.clone();
        let html = "<div class=\"card\">\n\t<button id=\"play\">Играть &amp; исследовать</button>\n</div>\n";
        let css = ".card {\r\n\tdisplay: flex;\r\n\tpadding: 12px;\r\n}\r\n#play {\r\n\tbackground: #7667d9;\r\n\tcolor: #ffffff;\r\n}\r\n";
        for (title, replacement) in [("HTML", html), ("CSS", css)] {
            let initial = if title == "HTML" { app.html_import.html.clone() } else { app.html_import.css.clone() };
            let point = field_point(&mut app, &initial);
            focus_at(&mut app, point);
            input_frame(&mut app, vec![select_all_event()]).drop_without_applying_deltas();
            input_frame(&mut app, ctrl_v_paste(replacement)).drop_without_applying_deltas();
            assert_eq!(if title == "HTML" { &app.html_import.html } else { &app.html_import.css }, replacement);
            assert_eq!(app.ctx.memory(|memory| memory.focused()), Some(html_field_id(title)));
        }
        assert_eq!(app.html_import.html, html);
        assert_eq!(app.html_import.css, css);
        assert_eq!(app.src, original);
        app.html_import.convert(&app.ctx.clone());
        assert!(app.html_import.error.is_none());
        let parsed = motif_script::parse(app.html_import.generated.as_ref().unwrap()).unwrap();
        assert!(parsed.doc.nodes.iter().any(|n| n.ui));
        assert!(parsed.doc.nodes.iter().any(|n| n.text.as_ref().is_some_and(|t| t.content == "Играть & исследовать")));
        assert_eq!(app.src, original);
        assert!(app.undo.is_empty());
    }

    #[test]
    fn code_editor_tab_indents_and_shift_tab_dedents_without_changing_focus_or_workspace() {
        let mut app = fixture("artboard 200x200\nrect old size 40\n");
        app.html_import.open = true;
        let original = app.src.clone();
        let workspace = app.workspace();
        for title in ["HTML", "CSS"] {
            let initial = if title == "HTML" { app.html_import.html.clone() } else { app.html_import.css.clone() };
            let point = field_point(&mut app, &initial);
            focus_at(&mut app, point);
            input_frame(&mut app, vec![select_all_event()]).drop_without_applying_deltas();
            input_frame(&mut app, ctrl_v_paste("code")).drop_without_applying_deltas();
            input_frame(&mut app, vec![egui::Event::Key { key: Key::Home, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE }])
                .drop_without_applying_deltas();
            input_frame(&mut app, vec![egui::Event::Key { key: Key::Tab, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE }])
                .drop_without_applying_deltas();
            assert_eq!(if title == "HTML" { &app.html_import.html } else { &app.html_import.css }, "\tcode");
            assert_eq!(app.ctx.memory(|memory| memory.focused()), Some(html_field_id(title)));
            input_frame(
                &mut app,
                vec![egui::Event::Key {
                    key: Key::Tab,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: Modifiers { shift: true, ..Default::default() },
                }],
            )
            .drop_without_applying_deltas();
            assert_eq!(if title == "HTML" { &app.html_import.html } else { &app.html_import.css }, "code");
            assert_eq!(app.ctx.memory(|memory| memory.focused()), Some(html_field_id(title)));
            assert_eq!(app.workspace(), workspace);
        }
        assert_eq!(app.src, original);
        assert!(app.undo.is_empty());
    }

    #[test]
    fn paste_buttons_target_their_editor_and_right_click_offers_paste() {
        let mut app = fixture("artboard 200x200\nrect old size 40\n");
        app.html_import.open = true;
        let before = app.src.clone();
        for title in ["HTML", "CSS"] {
            for _ in 0..3 {
                input_frame(&mut app, vec![]).drop_without_applying_deltas();
            }
            let out = click_control(&mut app, &format!("Вставить {title}"));
            assert!(requested_paste(&out));
            out.drop_without_applying_deltas();
            assert_eq!(app.ctx.memory(|m| m.focused()), Some(html_field_id(title)));
            input_frame(&mut app, vec![select_all_event()]).drop_without_applying_deltas();
            input_frame(&mut app, vec![egui::Event::Paste(format!("paste-to-{title}"))]).drop_without_applying_deltas();
            assert_eq!(if title == "HTML" { &app.html_import.html } else { &app.html_import.css }, &format!("paste-to-{title}"));
        }
        let text = app.html_import.html.clone();
        let at = field_point(&mut app, &text);
        for pressed in [true, false] {
            input_frame(
                &mut app,
                vec![
                    egui::Event::PointerMoved(at),
                    egui::Event::PointerButton { pos: at, button: egui::PointerButton::Secondary, pressed, modifiers: Modifiers::NONE },
                ],
            )
            .drop_without_applying_deltas();
        }
        let out = click_control(&mut app, "Вставить");
        assert!(requested_paste(&out));
        out.drop_without_applying_deltas();
        input_frame(&mut app, vec![egui::Event::Paste("-context-menu".into())]).drop_without_applying_deltas();
        assert!(app.html_import.html.contains("-context-menu"));
        assert_eq!(app.src, before);
        assert!(app.undo.is_empty());
    }

    #[test]
    fn open_import_dialog_owns_clipboard_and_canvas_shortcuts_without_a_text_focus() {
        let mut app = fixture("artboard 200x200\nrect old size 40\n");
        app.html_import.open = true;
        let original = app.src.clone();
        app.select_only(Some("old".into()));
        input_frame(&mut app, vec![]).drop_without_applying_deltas();
        app.ctx.memory_mut(|m| m.surrender_focus(html_field_id("HTML")));
        input_frame(&mut app, vec![egui::Event::Paste("rect pasted size 20\n".into()), egui::Event::Cut]).drop_without_applying_deltas();
        assert_eq!(app.src, original);
        assert!(app.scene.doc.node("pasted").is_none());
        assert!(app.scene.doc.node("old").is_some());
        assert!(app.undo.is_empty());
    }

    #[test]
    fn closing_import_restores_scene_key_and_script_paste_routes() {
        let mut app = fixture("artboard 200x200\nrect old size 40\n");
        input_frame(&mut app, vec![egui::Event::Paste("rect pasted size 20\n".into())]).drop_without_applying_deltas();
        assert!(app.scene.doc.node("pasted").is_some());

        let mut app = fixture("artboard 200x200\nrect box size 20\nanim Move 1s {\nbox.x 0: 0 | 1: 20\n}\n");
        app.sel_keys = vec![KeyRef { node: "box".into(), prop: Prop::X, time: 0.0 }];
        let ctx = app.ctx.clone();
        app.copy_keys(&ctx);
        let copied = app.key_clip.0.clone();
        app.sel_keys.clear();
        app.time = 0.5;
        input_frame(&mut app, vec![egui::Event::Paste(copied)]).drop_without_applying_deltas();
        assert!(app.current_anim().unwrap().tracks[0].keys.iter().any(|k| k.time == 0.5));

        let original = "artboard 200x200\nrect before size 40\n";
        let replacement = "artboard 240x240\nrect replaced size 30\n";
        let mut app = fixture(original);
        app.panels.set(crate::workspace::Panel::Script, true);
        let p = field_point(&mut app, original);
        focus_at(&mut app, p);
        assert!(app.ctx.text_edit_focused());
        input_frame(&mut app, vec![select_all_event()]).drop_without_applying_deltas();
        input_frame(&mut app, ctrl_v_paste(replacement)).drop_without_applying_deltas();
        assert_eq!(app.src, replacement);
        assert!(app.scene.doc.node("replaced").is_some());
        assert!(app.scene.doc.node("before").is_none());
    }

    #[test]
    fn copy_and_cut_in_import_editors_leave_object_clipboard_and_scene_unchanged() {
        let mut app = fixture("artboard 200x200\nrect old size 40\n");
        app.html_import.open = true;
        app.select_only(Some("old".into()));
        let before = app.src.clone();
        let original = app.html_import.html.clone();
        let p = field_point(&mut app, &original);
        focus_at(&mut app, p);
        input_frame(&mut app, vec![select_all_event()]).drop_without_applying_deltas();
        let output = input_frame(&mut app, vec![egui::Event::Copy]);
        assert!(output.platform_output.commands.iter().any(|command| matches!(command,egui::OutputCommand::CopyText(text) if text==&original)));
        output.drop_without_applying_deltas();
        input_frame(&mut app, vec![egui::Event::Cut]).drop_without_applying_deltas();
        assert!(app.html_import.html.is_empty());
        assert_eq!(app.src, before);
        assert!(app.clipboard.1.is_empty());
        assert!(app.undo.is_empty());
    }

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
