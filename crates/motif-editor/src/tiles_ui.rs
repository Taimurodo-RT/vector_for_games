//! A set of tiles: the words a game finds a board by (`tags`) and the export of every board of
//! the file as one atlas with its description.

use crate::app::App;
use crate::theme::{self, ButtonKind, FAINT, MUTED};
use egui::RichText;
use motif_pixel::tiles::{TileBoard, TileSetOptions, tile_set, write_tile_set};
use motif_render::CpuOptions;
use std::path::{Path, PathBuf};

impl App {
    /// Writes `tags` as the words a game finds the open board by.
    pub(crate) fn set_board_tags(&mut self, tags: &[String]) {
        if !self.can_edit() {
            return;
        }
        let out = motif_script::set_board_tags(&self.src, &self.scene, tags);
        if out != self.src && self.apply_once("board-tags", Some(out)) {
            let name = &self.scene.doc.artboard.name;
            self.status = if self.scene.doc.tags.is_empty() {
                format!("У холста «{name}» больше нет меток")
            } else {
                format!("Метки холста «{name}»: {}. Игра читает их в описании набора тайлов и в договоре сцены", self.scene.doc.tags.join(", "))
            };
        }
    }

    /// The row of the board panel where the words of the board are typed, with commas between.
    pub(crate) fn board_tags_row(&mut self, ui: &mut egui::Ui) {
        let known = self.scene.doc.tags.clone();
        let mut text = self.boards.tags_edit.clone().unwrap_or_else(|| known.join(", "));
        let (mut focus, mut done) = (false, false);
        ui.horizontal(|ui| {
            ui.label(RichText::new("Метки").size(11.5).color(MUTED));
            let field = ui
                .add(egui::TextEdit::singleline(&mut text).hint_text("трава, проходимо").desired_width(ui.available_width()))
                .on_hover_text("Слова через запятую, по которым игра находит этот холст в наборе тайлов: что это за земля, можно ли по ней ходить. На рисунок не влияют. В скрипте это строка tags под строкой artboard");
            focus = field.has_focus();
            done = field.lost_focus();
        });
        self.boards.tags_edit = (focus && !done).then(|| text.clone());
        if done {
            let tags: Vec<String> = text.split(',').map(|t| t.trim().to_string()).filter(|t| !t.is_empty()).collect();
            if tags != known {
                self.set_board_tags(&tags);
            }
        }
    }

    /// Whether some board of the file is a tile: the file may be a set of tiles.
    pub(crate) fn has_tiles(&self) -> bool {
        (0..self.board_count()).any(|k| self.board_doc(k).is_some_and(|d| d.tile.on()))
    }

    /// The part of the export window about a set of tiles; shown while the file has a tile.
    pub(crate) fn tiles_export(&mut self, ui: &mut egui::Ui) {
        if !self.has_tiles() {
            return;
        }
        theme::caption(ui, "Набор тайлов");
        ui.horizontal(|ui| {
            ui.label("Запас вокруг тайла, пикс.");
            ui.add(egui::DragValue::new(&mut self.export.tile_bleed).range(0..=64)).on_hover_text(
                "Сколько пикселей своего продолжения лежит вокруг каждого тайла в атласе. С запасом игра, которая сглаживает или уменьшает атлас, не захватит край соседнего тайла. 0 — тайлы вплотную",
            );
        });
        let n = self.board_count();
        if theme::button(ui, &format!("Сохранить набор тайлов… (холстов: {n})"), ButtonKind::Primary)
            .on_hover_text(
                "Все холсты файла одной картинкой и описание к ней: имя каждого тайла, его место в атласе, метки и стороны, по которым он повторяется",
            )
            .clicked()
        {
            self.tiles_dialog();
        }
        ui.label(
            RichText::new("Атлас ИМЯ.png и описание ИМЯ.json: игра находит тайл по имени или по метке и не размечает картинку вручную. Тайлы стоят в атласе так же, как холсты в редакторе. Масштаб и сглаживание берутся из раздела кадров ниже.")
                .size(12.0)
                .color(FAINT),
        );
        ui.add_space(6.0);
        theme::rule(ui);
        ui.add_space(4.0);
    }

    /// Asks for the folder of the set, and writes it there.
    fn tiles_dialog(&mut self) {
        let mut dialog = rfd::FileDialog::new().set_title("Папка для набора тайлов");
        if let Some(folder) = self.base_dir() {
            dialog = dialog.set_directory(folder);
        }
        if let Some(dir) = dialog.pick_folder() {
            self.tiles_to(&dir);
        }
    }

    /// Writes every board of the file into `dir` as one atlas with its description, named after
    /// the file. Returns the two files.
    pub(crate) fn tiles_to(&mut self, dir: &Path) -> Option<(PathBuf, PathBuf)> {
        let cells = self.board_grid().cells;
        let docs: Result<Vec<motif_core::Document>, String> =
            (0..self.board_count()).map(|k| self.board_doc(k).ok_or_else(|| format!("в скрипте холста {} ошибки", k + 1))).collect();
        let render = CpuOptions { scale: self.export.scale, antialias: self.export.antialias, background: true };
        let opts = TileSetOptions { render, bleed: self.export.tile_bleed };
        let result = docs.and_then(|docs| {
            let boards: Vec<TileBoard<'_>> = docs.iter().zip(&cells).map(|(doc, cell)| TileBoard { doc, cell: Some(*cell) }).collect();
            let set = tile_set(&boards, opts, &self.file_stem())?;
            let files = write_tile_set(&set, dir)?;
            Ok((set.description, files))
        });
        match result {
            Ok((d, files)) => {
                let name = |p: &PathBuf| p.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
                self.status = format!(
                    "Набор тайлов: {} и {} в {} — тайлов {}, атлас {}×{}, тайл {}×{}",
                    name(&files.0),
                    name(&files.1),
                    dir.display(),
                    d.tiles.len(),
                    d.width,
                    d.height,
                    d.tile_width,
                    d.tile_height
                );
                Some(files)
            }
            Err(e) => {
                self.status = format!("Набор тайлов не сохранён: {e}");
                None
            }
        }
    }
}
