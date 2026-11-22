//! The files a scene includes (`include "kit.msc"`): the section that lists them, the reasons
//! an edit of what they give is refused, and reading the scene again when one of them changes.

use crate::app::{App, fold, mtime};
use crate::theme::{self, ACCENT, ButtonKind, FAINT, MUTED};
use egui::{RichText, vec2};
use std::path::PathBuf;

/// What the section asked for.
enum Act {
    Add,
    Remove(usize),
    Open(PathBuf),
}

/// «цветов: 6, стилей: 3»: the kinds a file gives, those it gives none of left out.
fn counted(parts: &[(&str, usize)]) -> String {
    let said: Vec<String> = parts.iter().filter(|(_, n)| *n > 0).map(|(what, n)| format!("{what}: {n}")).collect();
    if said.is_empty() { "ничего не даёт".to_string() } else { said.join(", ") }
}

impl App {
    /// The included file that writes symbol `name`, as its `include` line names it.
    pub(crate) fn symbol_from(&self, name: &str) -> Option<String> {
        let s = self.scene.symbols.iter().find(|s| s.name == name)?;
        self.scene.included(s.span).map(|i| i.path.clone())
    }

    /// The included file that writes style `name`.
    pub(crate) fn style_from(&self, name: &str) -> Option<String> {
        let s = self.scene.styles.iter().find(|s| s.name == name)?;
        self.scene.included(s.decl).map(|i| i.path.clone())
    }

    /// The included file that writes the color or the gradient `name` of the palette.
    pub(crate) fn swatch_from(&self, name: &str) -> Option<String> {
        let color = self.scene.palette.iter().find(|s| s.name == name).map(|s| s.decl);
        let decl = color.or_else(|| self.scene.gradients.iter().find(|s| s.name == name).map(|s| s.decl))?;
        self.scene.included(decl).map(|i| i.path.clone())
    }

    /// The included file that writes lines of theme `name`.
    pub(crate) fn theme_from(&self, name: &str) -> Option<String> {
        self.scene.themes.iter().filter(|t| t.name == name).find_map(|t| self.scene.included(t.span)).map(|i| i.path.clone())
    }

    /// Says why `what` («Цвет «ink»») is not changed here: `file` writes it.
    pub(crate) fn refuse_included(&mut self, what: &str, file: &str) {
        self.status = format!("{what} записан в подключённом файле «{file}»: его меняют там, а эта сцена перечитает файл сама. Сцена не изменена");
    }

    /// The text of the file with the names the included files give written after it: a new
    /// name of the scene must be free among those too.
    pub(crate) fn names_text(&self) -> String {
        motif_script::with_included_names(&self.file_text(), &self.scene)
    }

    /// The section «Подключённые файлы»: each `include` line with what its file gives.
    pub(crate) fn includes_section(&mut self, ui: &mut egui::Ui) {
        let scene = &self.scene;
        let rows: Vec<(String, PathBuf, String, bool)> = scene
            .includes
            .iter()
            .map(|i| {
                let from = |span: motif_script::Span| scene.included(span).is_some_and(|site| site.span == i.span);
                let gives = counted(&[
                    ("цветов", scene.palette.iter().filter(|s| from(s.decl)).count() + scene.gradients.iter().filter(|s| from(s.decl)).count()),
                    ("стилей", scene.styles.iter().filter(|s| from(s.decl)).count()),
                    ("тем", scene.themes.iter().filter(|t| from(t.span)).count()),
                    ("символов", scene.symbols.iter().filter(|s| from(s.span)).count()),
                ]);
                (i.path.clone(), i.file.clone(), gives, i.files.len() > 1)
            })
            .collect();
        let mut act = None;
        fold(ui, "Подключённые файлы", |ui| {
            ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
            for (k, (path, file, gives, nested)) in rows.iter().enumerate() {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(path).font(theme::semibold(13.0)).color(ACCENT)).on_hover_text(file.display().to_string());
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let hint = "Убрать строку include. Получится, только если сцена не использует ничего из этого файла";
                        if theme::button(ui, "×", ButtonKind::Normal).on_hover_text(hint).clicked() {
                            act = Some(Act::Remove(k));
                        }
                        if theme::button(ui, "Открыть", ButtonKind::Normal).on_hover_text("Открыть файл набора в редакторе вместо этой сцены").clicked()
                        {
                            act = Some(Act::Open(file.clone()));
                        }
                    });
                });
                let more = if *nested { " Вместе с файлами, которые он подключает сам." } else { "" };
                ui.label(RichText::new(format!("{gives}.{more}")).size(11.5).color(MUTED));
            }
            let hint = "Строка include \"файл.msc\" в начале скрипта: сцена получает цвета, градиенты, стили, темы и символы этого файла";
            if theme::button(ui, "Подключить файл…", ButtonKind::Normal).on_hover_text(hint).clicked() {
                act = Some(Act::Add);
            }
            let text = if rows.is_empty() {
                "Набор в отдельном файле: его цвета, стили, темы и символы берут все экраны проекта. Поменяли кнопку в наборе, и она поменялась везде."
            } else {
                "Что даёт файл, меняют в нём самом: здесь это только используется. Когда файл меняется, сцена перечитывает его сама."
            };
            ui.label(RichText::new(text).size(11.5).color(FAINT));
        });
        if let Some(act) = act {
            self.include_act(act);
        }
    }

    fn include_act(&mut self, act: Act) {
        match act {
            Act::Add => {
                let Some(folder) = self.base_dir() else {
                    self.status = "Сначала сохраните сцену в файл: путь к подключаемому файлу пишется от её папки".into();
                    return;
                };
                let Some(file) = rfd::FileDialog::new().add_filter("Сцена Motif", &["msc"]).set_directory(&folder).pick_file() else { return };
                self.include_file(&file);
            }
            Act::Remove(k) => {
                if !self.can_edit() {
                    return;
                }
                let Some(path) = self.scene.includes.get(k).map(|i| i.path.clone()) else { return };
                let out = motif_script::remove_include(&self.src, &self.scene, k);
                if self.apply_once("include-remove", out) {
                    self.status = format!("Файл «{path}» больше не подключён");
                } else {
                    self.status = format!("Сцена использует то, что даёт «{path}», поэтому строка include осталась ({})", self.status);
                }
            }
            Act::Open(file) => {
                if self.dirty() {
                    self.status = "В сцене есть несохранённые правки: сохраните её (Ctrl+S), и файл набора откроется вместо неё".into();
                    return;
                }
                if let Err(e) = self.open_path(file) {
                    self.status = e;
                }
            }
        }
    }

    /// Takes `include` line number `k` away, as «×» in the section does.
    #[cfg(test)]
    pub(crate) fn include_act_remove(&mut self, k: usize) {
        self.include_act(Act::Remove(k));
    }

    /// Writes the `include` line for `file` into the scene.
    pub(crate) fn include_file(&mut self, file: &std::path::Path) {
        if !self.can_edit() {
            return;
        }
        let Some(folder) = self.base_dir() else {
            self.status = "Сначала сохраните сцену в файл: путь к подключаемому файлу пишется от её папки".into();
            return;
        };
        let Some(path) = motif_script::include_path(&folder, file) else {
            self.status = format!("Файл {} на другом диске: положите набор в папку проекта, тогда путь к нему запишется от сцены", file.display());
            return;
        };
        if self.path.as_deref().and_then(|p| std::fs::canonicalize(p).ok()) == std::fs::canonicalize(file).ok() {
            self.status = "Это сама сцена: подключают другой файл".into();
            return;
        }
        if self.scene.includes.iter().any(|i| i.path.replace('\\', "/") == path) {
            self.status = format!("Файл «{path}» уже подключён");
            return;
        }
        let out = motif_script::add_include(&self.src, &self.scene, &path);
        if self.apply_once("include-add", Some(out)) {
            self.status = format!("Подключён «{path}»: его цвета, стили, темы и символы теперь есть в этой сцене. Строка include стоит в начале скрипта");
        } else {
            self.status = format!("«{path}» не подключён. {}", self.status);
        }
    }

    /// Reads the scene again when a file it includes changed on the disk.
    pub(crate) fn poll_includes(&mut self) {
        // A file that was not there may be there now: the scene is tried again quietly.
        if self.errors.iter().any(|e| e.message.contains("не найден файл")) {
            self.reparse();
        }
        let now: Vec<(PathBuf, Option<std::time::SystemTime>)> = self.scene.includes.iter().flat_map(|i| &i.files).map(|f| (f.clone(), mtime(f))).collect();
        if now == self.include_stamps {
            return;
        }
        // A file known before with another time: it changed. A new list is a new scene.
        let changed = now.iter().find(|(f, t)| self.include_stamps.iter().any(|(g, u)| g == f && u != t)).map(|(f, _)| f.clone());
        self.include_stamps = now;
        let Some(file) = changed else { return };
        self.forget_other_boards();
        self.reparse();
        let name = file.file_name().map_or_else(|| file.display().to_string(), |n| n.to_string_lossy().into_owned());
        self.status = if self.errors.is_empty() {
            format!("Подключённый файл «{name}» изменился: сцена перечитана")
        } else {
            format!("Подключённый файл «{name}» изменился, и теперь сцена не читается: ошибка показана под скриптом")
        };
    }
}
