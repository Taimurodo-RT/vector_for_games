//! Taking work out of a project: the scene written as one file, without its `include` lines,
//! and a symbol saved as a file of its own with everything it needs. The open scene does not
//! change in either case.

use crate::app::App;
use std::path::{Path, PathBuf};

/// Whether `a` and `b` name one file.
fn same_file(a: &Path, b: &Path) -> bool {
    a == b || std::fs::canonicalize(a).ok().is_some_and(|a| std::fs::canonicalize(b).ok() == Some(a))
}

/// «, шрифты скопированы рядом (2)» and what to do about the ones that were not.
fn fonts_said(all: usize, left: &[String]) -> String {
    let mut out = String::new();
    if all > left.len() {
        out.push_str(&format!(". Шрифты скопированы рядом с файлом: {}", all - left.len()));
    }
    if !left.is_empty() {
        out.push_str(&format!(". Шрифты не скопированы, положите их рядом с новым файлом сами: {}", left.join(", ")));
    }
    out
}

impl App {
    /// Asks where to write the scene as one file, and writes it.
    pub(crate) fn bundle_dialog(&mut self) {
        let stem = self.path.as_deref().and_then(|p| p.file_stem()).map_or_else(|| "scene".to_string(), |s| s.to_string_lossy().into_owned());
        let mut dialog = rfd::FileDialog::new().add_filter("Сцена Motif", &["msc"]).set_file_name(format!("{stem}-one.msc"));
        if let Some(folder) = self.base_dir() {
            dialog = dialog.set_directory(folder);
        }
        if let Some(out) = dialog.save_file() {
            self.bundle_to(&out);
        }
    }

    /// Writes the scene to `out` as one file: the lines of the files it includes stand in it.
    pub(crate) fn bundle_to(&mut self, out: &Path) {
        if !self.can_edit() {
            return;
        }
        if self.path.as_deref().is_some_and(|p| same_file(p, out)) {
            self.status = "Это сама сцена: файл без подключений пишется под другим именем, а сцена остаётся как есть".into();
            return;
        }
        let base = self.base_dir();
        let bundled = match motif_script::bundle_scene(&self.file_text(), base.as_deref()) {
            Ok(b) => b,
            Err(e) => {
                self.status = format!("Сцена не собрана в один файл: {e}");
                return;
            }
        };
        if let Err(e) = std::fs::write(out, &bundled.text) {
            self.status = format!("Не удалось записать {}: {e}", out.display());
            return;
        }
        let fonts: Vec<(String, String)> = bundled.fonts.iter().map(|f| (f.clone(), f.clone())).collect();
        let left = match (&base, out.parent()) {
            (Some(from), Some(to)) => motif_script::copy_fonts(&fonts, from, to),
            _ => Vec::new(),
        };
        let with = if bundled.files.is_empty() {
            "В сцене нет подключённых файлов, она записана как есть".to_string()
        } else {
            format!("В неё вставлены строки файлов: {}", bundled.files.join(", "))
        };
        self.status = format!("Сцена записана одним файлом: {}. {with}{}. Открытая сцена не изменилась", out.display(), fonts_said(fonts.len(), &left));
    }

    /// Asks where to save symbol `symbol` as a file of its own, and saves it. The symbol is of
    /// the open scene, or of the library file `source`.
    pub(crate) fn extract_dialog(&mut self, source: Option<&Path>, symbol: &str) {
        let mut dialog = rfd::FileDialog::new().add_filter("Сцена Motif", &["msc"]).set_file_name(motif_script::file_name_for(symbol).to_string_lossy());
        if let Some(folder) = self.base_dir() {
            dialog = dialog.set_directory(folder);
        }
        if let Some(out) = dialog.save_file() {
            self.extract_to(source, symbol, &out);
        }
    }

    /// Saves symbol `symbol` to `out` with the palette names, styles, symbols and fonts it uses.
    pub(crate) fn extract_to(&mut self, source: Option<&Path>, symbol: &str, out: &Path) {
        let taken: Vec<PathBuf> = self.path.iter().cloned().chain(source.map(Path::to_path_buf)).collect();
        if taken.iter().any(|p| same_file(p, out)) {
            self.status = "В этом файле символ и записан: сохраните его под другим именем или в другой папке".into();
            return;
        }
        let (text, base) = match source {
            None => {
                if !self.can_edit() {
                    return;
                }
                (self.file_text(), self.base_dir())
            }
            Some(file) => match std::fs::read_to_string(file) {
                Ok(text) => (text, file.parent().map(Path::to_path_buf)),
                Err(e) => {
                    self.status = format!("Не удалось открыть {}: {e}", file.display());
                    return;
                }
            },
        };
        let e = match motif_script::extract_symbol(&text, base.as_deref(), symbol) {
            Ok(e) => e,
            Err(e) => {
                self.status = format!("Символ {symbol} не сохранён: {e}");
                return;
            }
        };
        if let Err(err) = std::fs::write(out, &e.text) {
            self.status = format!("Не удалось записать {}: {err}", out.display());
            return;
        }
        let left = match (&base, out.parent()) {
            (Some(from), Some(to)) => motif_script::copy_fonts(&e.fonts, from, to),
            _ => e.fonts.iter().map(|(f, _)| f.clone()).collect(),
        };
        let mut with = Vec::new();
        for (what, names) in [("цвета и числа", &e.palette), ("стили", &e.styles), ("символы", &e.symbols), ("темы", &e.themes)] {
            if !names.is_empty() {
                with.push(format!("{what} {}", names.join(", ")));
            }
        }
        let with = if with.is_empty() { String::new() } else { format!(" С ним взято: {}.", with.join("; ")) };
        self.status = format!(
            "Символ {symbol} сохранён в файл {}.{with} Положите файл в папку другого проекта, и «Библиотека» покажет символ{}",
            out.display(),
            fonts_said(e.fonts.len(), &left)
        );
    }
}
