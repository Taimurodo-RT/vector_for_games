//! Safe edits of a symbol that has copies: «Отцепить копию» turns a copy into a group of its
//! own, and the removal of an object of a symbol that something refers to (animation tracks,
//! edits of copies, variants, on any board) first shows what goes with it.

use crate::app::App;
use crate::theme::{self, ButtonKind, FAINT, FG, MUTED};
use egui::{Align2, RichText, vec2};
use motif_script::PartUses;

/// A removal of objects of a symbol that waits for an answer.
pub(crate) struct SymbolDelete {
    /// The copy the symbol is edited through.
    pub copy: String,
    /// The objects to remove, as the scene names them (`copy-object`).
    pub ids: Vec<String>,
    pub uses: PartUses,
}

/// `2 дорожки`, `5 дорожек`.
fn count(n: usize, one: &str, few: &str, many: &str) -> String {
    let word = match (n % 10, n % 100) {
        (1, r) if r != 11 => one,
        (2..=4, r) if !(12..=14).contains(&r) => few,
        _ => many,
    };
    format!("{n} {word}")
}

/// What one board loses, in words: «копий 2 · 2 дорожки анимации (Spin) · 1 правка копии».
fn board_line(b: &motif_script::BoardUse) -> String {
    let mut parts = vec![format!("копий {}", b.copies)];
    let tracks: usize = b.tracks.iter().map(|(_, n)| n).sum();
    if tracks > 0 {
        let clips: Vec<&str> = b.tracks.iter().map(|(name, _)| name.as_str()).collect();
        parts.push(format!("{} анимации ({})", count(tracks, "дорожка", "дорожки", "дорожек"), clips.join(", ")));
    }
    if b.edits > 0 {
        parts.push(format!("{} копии", count(b.edits, "правка", "правки", "правок")));
    }
    if b.links > 0 {
        parts.push(count(b.links, "связь (драйвер, слушатель машины)", "связи (драйверы, слушатели машин)", "связей (драйверы, слушатели машин)"));
    }
    parts.join(" · ")
}

impl App {
    /// «Отцепить копию»: copy `copy` becomes an ordinary group with the same picture; its symbol
    /// no longer changes it.
    pub(crate) fn detach_copy(&mut self, copy: &str) {
        if !self.can_edit() {
            return;
        }
        let Some(symbol) = self.scene.instances.get(copy).map(|i| i.symbol.clone()) else { return };
        // Out of the symbol first: the copy is read as a copy again.
        if self.scene.editing.is_some() {
            self.isolate(None);
        }
        let base = self.base_dir();
        match motif_script::detach_copy(&self.src, &self.scene, copy, base.as_deref()) {
            Ok(out) => {
                if self.apply_once("detach", Some(out)) {
                    self.select_only(Some(copy.to_string()));
                    self.status =
                        format!("Копия «{copy}» отцеплена: теперь это обычная группа, символ {symbol} её больше не меняет. Её ключи и правки сохранены");
                }
            }
            Err(why) => self.status = why,
        }
    }

    /// Whether the selected objects are objects of the symbol being edited that something else
    /// refers to. Then their removal waits for an answer in a window and `true` is returned.
    pub(crate) fn ask_symbol_delete(&mut self) -> bool {
        let Some(copy) = self.scene.editing.clone() else { return false };
        let ids = self.selection();
        if ids.is_empty() || !ids.iter().all(|id| motif_script::part_of(&self.scene, &copy, id).is_some()) {
            return false;
        }
        let base = self.base_dir();
        let Some(uses) = motif_script::symbol_part_uses(&self.file_text(), base.as_deref(), self.boards.active, &copy, &ids) else { return false };
        if !uses.referred() {
            return false;
        }
        self.status =
            format!("На {} символа {} есть ссылки: подтвердите удаление в окне", if uses.parts.len() == 1 { "деталь" } else { "детали" }, uses.symbol);
        self.symbol_delete = Some(SymbolDelete { copy, ids, uses });
        true
    }

    /// Answers the waiting removal: `yes` removes the objects from the symbol with everything
    /// that names them, on every board, as one step back.
    pub(crate) fn answer_symbol_delete(&mut self, yes: bool) {
        let Some(SymbolDelete { copy, ids, uses }) = self.symbol_delete.take() else { return };
        if !yes {
            self.status = "Удаление отменено: символ не изменён".into();
            return;
        }
        let base = self.base_dir();
        match motif_script::delete_symbol_parts(&self.file_text(), base.as_deref(), self.boards.active, &copy, &ids) {
            Ok(out) => {
                if self.apply_file("delete", Some(out)) {
                    self.select_only(None);
                    let tracks: usize = uses.boards.iter().flat_map(|b| &b.tracks).map(|(_, n)| n).sum();
                    let edits: usize = uses.boards.iter().map(|b| b.edits).sum();
                    self.status = format!(
                        "Из символа {} удалено: {}. Копий: {}, вместе с деталью ушло дорожек: {tracks}, правок копий: {edits}",
                        uses.symbol,
                        uses.parts.join(", "),
                        uses.copies()
                    );
                }
            }
            Err(why) => self.status = format!("Не удалено: {why}"),
        }
    }

    /// The window of the waiting removal: what refers to the objects, board by board.
    pub(crate) fn symbol_delete_window(&mut self, ctx: &egui::Context) {
        let Some(pending) = &self.symbol_delete else { return };
        // The symbol is no longer open: there is nothing to answer.
        if self.scene.editing.as_deref() != Some(pending.copy.as_str()) {
            self.symbol_delete = None;
            return;
        }
        let uses = pending.uses.clone();
        let mut answer = None;
        egui::Window::new(format!("Удалить из символа {}?", uses.symbol))
            .id(egui::Id::unique("symbol-delete"))
            .collapsible(false)
            .resizable(false)
            .anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0))
            .show(ctx, |ui| {
                ui.set_max_width(460.0);
                let what = match uses.parts.as_slice() {
                    [one] => format!("Деталь «{one}» есть в каждой копии символа. Вместе с ней уйдёт то, что на неё ссылается:"),
                    many => format!("Детали {} есть в каждой копии символа. Вместе с ними уйдёт то, что на них ссылается:", many.join(", ")),
                };
                ui.label(RichText::new(what).size(13.0).color(FG));
                ui.add_space(4.0);
                for b in &uses.boards {
                    ui.horizontal_wrapped(|ui| {
                        ui.label(RichText::new(format!("Холст «{}»:", b.name)).size(12.5).color(MUTED));
                        ui.label(RichText::new(board_line(b)).size(12.5).color(FG));
                    });
                }
                if !uses.variants.is_empty() {
                    let names = uses.variants.join(", ");
                    let text = if uses.variants.len() == 1 {
                        format!("Она уйдёт и из варианта {names}.")
                    } else {
                        format!("Она уйдёт и из вариантов {names}.")
                    };
                    ui.label(RichText::new(text).size(12.5).color(FG));
                }
                ui.add_space(4.0);
                ui.label(RichText::new("Одно удаление — один шаг отмены: Ctrl+Z вернёт и деталь, и всё перечисленное.").size(11.5).color(FAINT));
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if theme::button(ui, "Удалить во всех копиях", ButtonKind::Primary).clicked() {
                        answer = Some(true);
                    }
                    if theme::button(ui, "Отмена", ButtonKind::Normal).on_hover_text("Esc").clicked() {
                        answer = Some(false);
                    }
                });
            });
        if let Some(yes) = answer {
            self.answer_symbol_delete(yes);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_are_said_in_russian() {
        assert_eq!(count(1, "дорожка", "дорожки", "дорожек"), "1 дорожка");
        assert_eq!(count(3, "дорожка", "дорожки", "дорожек"), "3 дорожки");
        assert_eq!(count(11, "дорожка", "дорожки", "дорожек"), "11 дорожек");
        assert_eq!(count(25, "дорожка", "дорожки", "дорожек"), "25 дорожек");
        assert_eq!(count(21, "дорожка", "дорожки", "дорожек"), "21 дорожка");
        let b = motif_script::BoardUse { board: 0, name: "a".into(), copies: 2, tracks: vec![("Spin".into(), 2)], edits: 1, links: 0 };
        assert_eq!(board_line(&b), "копий 2 · 2 дорожки анимации (Spin) · 1 правка копии");
    }
}
