//! Several artboards in one file, like in Illustrator: each board is the part of the script from
//! its `artboard` line to the next, and what comes before the first board is shared by all. The
//! board being edited, after the shared head, is the script in `App::src`; the others wait in
//! `Boards::texts`. Boards sit on a grid, each at the top-left corner of its cell;
//! a board's name above it drags it to another cell, the corner handle resizes it.

use crate::app::App;
use crate::canvas::View;
use crate::theme::{self, ACCENT, FG, MUTED, SEL};
use egui::{Align2, Color32, Pos2, Rect, RichText, Stroke, StrokeKind, vec2};
use motif_core::Document;
use motif_core::kurbo::Point;
use motif_script::{BoardGrid, BoardSize, Scene, fmt_num};
use std::cell::RefCell;

/// The boards of the open file.
#[derive(Default)]
pub(crate) struct Boards {
    /// The text of every board, in file order. The active one is stale while it is active: its
    /// current text is `App::src`.
    pub texts: Vec<String>,
    /// What comes before the first board, shared by all of them: the active script starts
    /// with it too. Empty while the file has one board.
    pub head: String,
    pub active: usize,
    /// The other boards parsed for drawing, by the text they were parsed from.
    parsed: RefCell<Vec<(String, Option<Scene>)>>,
    pub drag: Option<BoardDrag>,
    /// Where the active board was on the shared grid last frame: when the layout moves it, the
    /// view follows so the other boards stay put on screen.
    pub origin: Option<Point>,
    /// Bring the active board into view on the next frame (it was opened from the panel or
    /// just made, and may be off screen).
    pub reveal: bool,
    /// Fit every board into the view on the next frame (Shift+F).
    pub fit_all: bool,
    /// The board name being typed in the board panel.
    name_edit: Option<String>,
}

pub(crate) enum BoardDrag {
    /// A board's name dragged to another cell of the grid.
    Move { board: usize },
    /// The corner handle of the active board, and the size so far.
    Resize { size: (f64, f64) },
}

/// A board drawn next to the active one: its pose and where it sits relative to the active board.
pub(crate) struct OtherBoard {
    pub doc: Document,
    pub offset: Point,
}

impl App {
    /// The whole file: the shared head and every board's text, the active one as edited.
    pub(crate) fn file_text(&self) -> String {
        if self.boards.texts.len() < 2 {
            return self.src.clone();
        }
        // The active script is the head followed by the board; the head may have been edited.
        let at = motif_script::head_end(&self.src);
        let mut out = self.src[..at].to_string();
        for (i, t) in self.boards.texts.iter().enumerate() {
            out.push_str(if i == self.boards.active { &self.src[at..] } else { t });
        }
        out
    }

    /// Loads `text` as the file, making board `active` (or the last one) the one being edited.
    pub(crate) fn set_file(&mut self, text: &str, active: usize) {
        let (head, ranges) = motif_script::boards(text);
        let texts: Vec<String> = ranges.into_iter().map(|r| text[r].to_string()).collect();
        self.boards.active = active.min(texts.len() - 1);
        self.boards.head = text[head].to_string();
        self.src = format!("{}{}", self.boards.head, texts[self.boards.active]);
        self.boards.texts = texts;
    }

    /// After the active script was edited: when it now holds more than one `artboard` line, it
    /// is split into boards and the first of them stays active.
    pub(crate) fn split_active(&mut self) {
        if motif_script::boards(&self.src).1.len() < 2 {
            return;
        }
        let (file, active) = (self.file_text(), self.boards.active);
        self.set_file(&file, active);
    }

    pub(crate) fn board_count(&self) -> usize {
        self.boards.texts.len().max(1)
    }

    /// Board `k` parsed, for drawing, layout and export; the active board is `App::scene`.
    fn with_other_scene<R>(&self, k: usize, f: impl FnOnce(&Scene) -> R) -> Option<R> {
        // The head as it is being edited in the open board, so the other boards follow it live.
        let head = &self.src[..motif_script::head_end(&self.src)];
        let text = format!("{head}{}", self.boards.texts.get(k)?);
        let mut cache = self.boards.parsed.borrow_mut();
        if cache.len() != self.boards.texts.len() {
            cache.resize(self.boards.texts.len(), (String::new(), None));
        }
        if cache[k].0 != text || (cache[k].1.is_none() && cache[k].0.is_empty()) {
            let scene = motif_script::parse_in(&text, self.path.as_ref().and_then(|p| p.parent())).ok();
            cache[k] = (text, scene);
        }
        cache[k].1.as_ref().map(f)
    }

    /// The size, cell and name of every board.
    pub(crate) fn board_specs(&self) -> Vec<(BoardSize, String)> {
        let spec = |s: &Scene| ((s.doc.artboard.width, s.doc.artboard.height, s.board_cell), s.doc.artboard.name.clone());
        (0..self.board_count())
            .map(|k| {
                let found = if k == self.boards.active { Some(spec(&self.scene)) } else { self.with_other_scene(k, spec) };
                found.unwrap_or_else(|| ((480.0, 360.0, None), format!("Холст {} (ошибка)", k + 1)))
            })
            .collect()
    }

    /// Board `k` as it is now, at rest; `None` while its script has errors.
    pub(crate) fn board_doc(&self, k: usize) -> Option<Document> {
        if k == self.boards.active || self.board_count() < 2 {
            return Some(self.scene.doc.clone());
        }
        self.with_other_scene(k, |s| s.doc.clone())
    }

    /// Where all boards lie, in the active board's space.
    pub(crate) fn boards_bounds(&self, grid: &BoardGrid) -> egui::Rect {
        let here = grid.origins[self.boards.active.min(grid.origins.len() - 1)];
        let mut out = egui::Rect::NOTHING;
        for ((size, _), o) in self.board_specs().iter().zip(&grid.origins) {
            let min = egui::pos2((o.x - here.x) as f32, (o.y - here.y) as f32);
            out = out.union(egui::Rect::from_min_size(min, egui::vec2(size.0 as f32, size.1 as f32)));
        }
        out
    }

    pub(crate) fn board_grid(&self) -> BoardGrid {
        let sizes: Vec<BoardSize> = self.board_specs().into_iter().map(|s| s.0).collect();
        motif_script::board_layout(&sizes)
    }

    /// The other boards as they look at rest, placed relative to the active one.
    pub(crate) fn other_boards(&self, grid: &BoardGrid) -> Vec<OtherBoard> {
        let here = grid.origins[self.boards.active.min(grid.origins.len() - 1)];
        (0..self.board_count())
            .filter(|&k| k != self.boards.active)
            .filter_map(|k| {
                let mut doc = self.with_other_scene(k, |s| s.doc.clone())?;
                doc.rig();
                let o = grid.origins[k];
                Some(OtherBoard { doc, offset: Point::new(o.x - here.x, o.y - here.y) })
            })
            .collect()
    }

    /// Makes board `k` the one being edited.
    pub(crate) fn switch_board(&mut self, k: usize) {
        if k == self.boards.active || k >= self.boards.texts.len() {
            return;
        }
        let file = self.file_text();
        self.set_file(&file, k);
        self.selected = None;
        self.also.clear();
        self.anim = None;
        self.last_anim = None;
        self.pose_preview.clear();
        self.reset_workspace_document();
        self.run = None;
        self.check = None;
        self.playing = false;
        self.time = 0.0;
        self.sel_keys.clear();
        self.point_edit = None;
        self.machine = None;
        self.machine_pick = None;
        self.brush_w.on = None;
        self.reparse();
        self.status = format!("Холст «{}»", self.scene.doc.artboard.name);
    }

    /// Replaces the texts of the boards as one undo step; `active` becomes the edited board.
    pub(crate) fn apply_boards(&mut self, kind: &str, texts: Vec<String>, active: usize) {
        if self.workspace() == crate::workspace::Workspace::Animate {
            self.status = "Для изменения состава холстов выберите «Рисование» или «Настройка»".into();
            return;
        }
        self.checkpoint(kind);
        let switching = active != self.boards.active;
        let file = format!("{}{}", self.boards.head, texts.concat());
        self.set_file(&file, active);
        if switching {
            self.selected = None;
            self.also.clear();
            self.anim = None;
            self.run = None;
            self.check = None;
        }
        self.reparse();
    }

    /// The texts of all boards with the active one as edited, each ending with a line break.
    /// The shared head, as edited, is in `Boards::head` afterwards.
    fn current_texts(&mut self) -> Vec<String> {
        let file = self.file_text();
        self.set_file(&file, self.boards.active);
        let mut texts = self.boards.texts.clone();
        for t in &mut texts {
            if !t.ends_with('\n') {
                t.push('\n');
            }
        }
        texts
    }

    /// A free board name: «Холст 2», «Холст 3»…
    fn free_board_name(&self, base: &str) -> String {
        let names: Vec<String> = self.board_specs().into_iter().map(|s| s.1).collect();
        if !names.iter().any(|n| n == base) {
            return base.to_string();
        }
        (2..).map(|i| format!("{base} {i}")).find(|n| !names.contains(n)).expect("some number is free")
    }

    /// Adds an empty board the size of the active one after the last board and opens it.
    pub(crate) fn add_board(&mut self) {
        let ab = self.scene.doc.artboard.clone();
        let mut texts = self.current_texts();
        let name = self.free_board_name("Холст");
        let cell = self.free_cell();
        texts.push(format!("\n{}", motif_script::new_board(&name, ab.width, ab.height, cell, Some(&self.scene.doc))));
        let k = texts.len() - 1;
        self.apply_boards("board-add", texts, k);
        self.boards.reveal = true;
        self.status = format!("Новый холст «{name}»: рисуйте на нём, имя над ним тянет его по сетке");
    }

    /// A copy of board `k`, everything on it included, in the next free cell.
    pub(crate) fn duplicate_board(&mut self, k: usize) {
        let mut texts = self.current_texts();
        let Some(src) = texts.get(k).cloned() else { return };
        let name = self.board_specs().get(k).map_or_else(|| "Холст".to_string(), |s| s.1.clone());
        let name = self.free_board_name(&format!("{name} копия"));
        let copy = motif_script::rename_artboard(&src, &name).unwrap_or(src);
        let copy = motif_script::set_board_cell(&copy, Some(self.free_cell())).unwrap_or(copy);
        texts.push(format!("\n{}", copy.trim_start_matches('\n')));
        let at = texts.len() - 1;
        self.apply_boards("board-dup", texts, at);
        self.boards.reveal = true;
        self.status = format!("Холст скопирован: «{name}»");
    }

    /// Removes board `k` and everything on it; the last board stays.
    pub(crate) fn delete_board(&mut self, k: usize) {
        if self.board_count() < 2 {
            self.status = "Это единственный холст: его нельзя удалить".into();
            return;
        }
        let mut texts = self.current_texts();
        let name = self.board_specs().get(k).map(|s| s.1.clone()).unwrap_or_default();
        texts.remove(k);
        let active = match self.boards.active {
            a if a == k => k.min(texts.len() - 1),
            a if a > k => a - 1,
            a => a,
        };
        self.apply_boards("board-delete", texts, active);
        self.status = format!("Холст «{name}» удалён (Ctrl+Z вернёт)");
    }

    pub(crate) fn rename_board(&mut self, name: &str) {
        if !self.can_edit() {
            return;
        }
        let out = motif_script::rename_artboard(&self.src, name);
        self.apply("board-name", out);
    }

    pub(crate) fn resize_board(&mut self, w: f64, h: f64) {
        if !self.can_edit() {
            return;
        }
        let out = motif_script::set_artboard_size(&self.src, w, h);
        self.apply("board-size", out);
    }

    /// The first cell no board takes, row by row within the current columns.
    fn free_cell(&self) -> (u32, u32) {
        let grid = self.board_grid();
        let cols = grid.cells.iter().map(|c| c.0 + 1).max().unwrap_or(1).max(((grid.cells.len() + 1) as f64).sqrt().ceil() as u32);
        (0..).map(|i| (i % cols, i / cols)).find(|c| !grid.cells.contains(c)).expect("cells never run out")
    }

    /// Moves board `k` to `cell`; a board already there takes the cell `k` left. Every board's
    /// cell is written down, so the grid stays as arranged.
    fn move_board(&mut self, k: usize, cell: (u32, u32)) {
        let grid = self.board_grid();
        let from = grid.cells[k];
        if from == cell {
            return;
        }
        let mut cells = grid.cells.clone();
        if let Some(other) = cells.iter().position(|c| *c == cell) {
            cells[other] = from;
        }
        cells[k] = cell;
        let texts: Vec<String> = self.current_texts().into_iter().zip(&cells).map(|(t, c)| motif_script::set_board_cell(&t, Some(*c)).unwrap_or(t)).collect();
        let active = self.boards.active;
        self.apply_boards("board-move", texts, active);
        self.status = format!("Холст в ячейке {}, {}", cell.0 + 1, cell.1 + 1);
    }

    /// The names above the boards, picking a board by clicking it, dragging a name to another
    /// cell and the size handle of the active board. `true` while the pointer is theirs, so the
    /// canvas leaves it alone.
    pub(crate) fn boards_input(&mut self, ui: &egui::Ui, resp: &egui::Response, rect: Rect, view: View, grid: &BoardGrid) -> bool {
        let specs = self.board_specs();
        let here = grid.origins[self.boards.active.min(grid.origins.len() - 1)];
        // Board k on screen, in the active board's view.
        let screen = |k: usize| {
            let o = grid.origins[k];
            let min = view.to_screen(rect, Point::new(o.x - here.x, o.y - here.y));
            Rect::from_min_size(min, vec2(specs[k].0.0 as f32, specs[k].0.1 as f32) * view.zoom)
        };
        let label = |k: usize| {
            let r = screen(k);
            let text = format!("{}  {}×{}", specs[k].1, fmt_num(specs[k].0.0), fmt_num(specs[k].0.1));
            let galley = ui.painter().layout_no_wrap(text, theme::ui_font(12.0), MUTED);
            (Rect::from_min_size(r.left_top() - vec2(0.0, galley.size().y + 6.0), galley.size() + vec2(4.0, 2.0)), galley)
        };
        let painter = ui.painter_at(rect);
        let pointer = resp.hover_pos().or(resp.interact_pointer_pos());
        let active = self.boards.active;
        let handle = Rect::from_center_size(screen(active).right_bottom() + vec2(5.0, 5.0), vec2(9.0, 9.0));
        let many = specs.len() > 1;

        // Labels and outlines.
        for k in 0..specs.len() {
            let (lr, galley) = label(k);
            let hot = many && self.drag.is_none() && pointer.is_some_and(|p| lr.expand(3.0).contains(p));
            let color = if k == active || hot { FG } else { MUTED };
            painter.galley(lr.min, galley, color);
            if k == active && many {
                painter.rect_stroke(screen(k).expand(1.5), 0.0, Stroke::new(1.5, ACCENT.gamma_multiply(0.7)), StrokeKind::Outside);
            }
        }
        let size_hot = self.drag.is_none() && pointer.is_some_and(|p| handle.expand(3.0).contains(p));
        painter.rect(handle, 1.5, if size_hot { ACCENT } else { Color32::WHITE }, Stroke::new(1.0, Color32::from_black_alpha(160)), StrokeKind::Middle);

        // A drag of a name or the handle in progress.
        if let Some(d) = &mut self.boards.drag {
            let stage = |p: Pos2| {
                let q = view.to_doc(rect, p);
                Point::new(q.x + here.x, q.y + here.y)
            };
            match d {
                BoardDrag::Move { board } => {
                    let k = *board;
                    if let Some(p) = pointer {
                        let o = grid.cell_origin(grid.cell_at(stage(p)));
                        let at = view.to_screen(rect, Point::new(o.x - here.x, o.y - here.y));
                        let ghost = Rect::from_min_size(at, vec2(specs[k].0.0 as f32, specs[k].0.1 as f32) * view.zoom);
                        painter.rect_filled(ghost, 0.0, SEL.gamma_multiply(0.12));
                        painter.rect_stroke(ghost, 0.0, Stroke::new(1.5, SEL), StrokeKind::Outside);
                        ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
                    }
                    if !ui.input(|i| i.pointer.primary_down()) {
                        self.boards.drag = None;
                        if let Some(p) = pointer {
                            let cell = grid.cell_at(stage(p));
                            self.move_board(k, cell);
                        }
                    }
                }
                BoardDrag::Resize { size } => {
                    if let Some(p) = pointer {
                        let q = view.to_doc(rect, p);
                        *size = (q.x.round().max(8.0), q.y.round().max(8.0));
                        let r = Rect::from_min_size(screen(active).min, vec2(size.0 as f32, size.1 as f32) * view.zoom);
                        painter.rect_stroke(r, 0.0, Stroke::new(1.5, ACCENT), StrokeKind::Outside);
                        painter.text(
                            r.right_bottom() + vec2(10.0, 10.0),
                            Align2::LEFT_TOP,
                            format!("{}×{}", fmt_num(size.0), fmt_num(size.1)),
                            theme::mono(12.0),
                            FG,
                        );
                        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeNwSe);
                    }
                    if !ui.input(|i| i.pointer.primary_down()) {
                        let (w, h) = *size;
                        self.boards.drag = None;
                        self.resize_board(w, h);
                    }
                }
            }
            return true;
        }
        if self.drag.is_some() {
            return false;
        }
        let Some(p) = pointer.filter(|_| resp.hovered()) else { return false };
        // Drags count from where the button went down, not where they passed the threshold.
        let p = if resp.drag_started() { ui.input(|i| i.pointer.press_origin()).unwrap_or(p) } else { p };
        let press = resp.drag_started() || resp.clicked();
        if handle.expand(3.0).contains(p) {
            ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeNwSe);
            if resp.drag_started() {
                self.boards.drag = Some(BoardDrag::Resize { size: (specs[active].0.0, specs[active].0.1) });
            }
            return true;
        }
        if !many {
            return false;
        }
        if let Some(k) = (0..specs.len()).find(|&k| label(k).0.expand(3.0).contains(p)) {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
            if resp.drag_started() {
                self.boards.drag = Some(BoardDrag::Move { board: k });
            } else if resp.clicked() {
                self.switch_board(k);
            }
            return true;
        }
        // Inside another board: a click opens it.
        if !screen(active).contains(p)
            && let Some(k) = (0..specs.len()).find(|&k| k != active && screen(k).contains(p))
        {
            if press {
                self.switch_board(k);
            }
            return true;
        }
        false
    }
}

impl App {
    /// The board panel (nothing selected): name, size, and making, copying and removing boards.
    pub(crate) fn board_section(&mut self, ui: &mut egui::Ui) {
        let ab = self.scene.doc.artboard.clone();
        let (mut name_done, mut size, mut add, mut dup, mut del) = (None, None, false, false, false);
        crate::app::section(ui, |ui| {
            ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
            let text = self.boards.name_edit.get_or_insert_with(|| ab.name.clone());
            let out = ui.add(egui::TextEdit::singleline(text).font(theme::semibold(14.0)).desired_width(ui.available_width()).hint_text("Имя холста"));
            if out.lost_focus() {
                if !ui.input(|i| i.key_pressed(egui::Key::Escape)) && text.trim() != ab.name && !text.trim().is_empty() {
                    name_done = Some(text.trim().to_string());
                }
                self.boards.name_edit = None;
            } else if !out.has_focus() {
                *text = ab.name.clone();
            }
            let w = (ui.available_width() - 6.0) / 2.0;
            ui.horizontal(|ui| {
                if let Some(v) = theme::num_field(ui, w, "Ш", ab.width, 1.0, 1.0..=8192.0, 0, theme::KeyState::Hidden).changed {
                    size = Some((v.round(), ab.height));
                }
                if let Some(v) = theme::num_field(ui, w, "В", ab.height, 1.0, 1.0..=8192.0, 0, theme::KeyState::Hidden).changed {
                    size = Some((ab.width, v.round()));
                }
            });
            ui.horizontal_wrapped(|ui| {
                add = theme::button(ui, "Новый холст", theme::ButtonKind::Normal).clicked();
                dup = theme::button(ui, "Копия", theme::ButtonKind::Normal).clicked();
                if self.board_count() > 1 {
                    del = theme::button(ui, "Удалить", theme::ButtonKind::Normal).clicked();
                }
            });
            let many = self.board_count() > 1;
            let hint = if many {
                "Размер растёт от левого верхнего угла (уголок справа снизу). Имя над холстом тянет его в другую ячейку сетки; клик по холсту открывает его."
            } else {
                "Размер растёт от левого верхнего угла (уголок справа снизу). «Новый холст» добавит рядом ещё один: элементы набора, кадры, варианты."
            };
            ui.label(RichText::new(hint).size(11.5).color(theme::FAINT));
        });
        if !self.can_edit() && (name_done.is_some() || size.is_some() || add || dup || del) {
            return;
        }
        if let Some(n) = name_done {
            self.rename_board(&n);
        }
        if let Some((w, h)) = size {
            self.resize_board(w, h);
        }
        let k = self.boards.active;
        if add {
            self.add_board();
        } else if dup {
            self.duplicate_board(k);
        } else if del {
            self.delete_board(k);
        }
    }
}
