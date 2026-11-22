//! Symbols in the panels: a copy's parameters, the way into its symbol, what the copy has of
//! its own, «Сделать символом» for a group, and the list of symbols to place copies from.

use crate::app::{App, fold};
use crate::theme::{self, ACCENT, ButtonKind, FAINT, FG, MUTED};
use egui::{RichText, vec2};
use motif_core::{Node, ShapeKind, Value};

impl App {
    /// The symbol that `id` is a copy of, or that the copy holding `id` is of.
    pub(crate) fn symbol_of(&self, id: &str) -> Option<&str> {
        let doc = &self.scene.doc;
        let mut cur = doc.index_of(id);
        while let Some(i) = cur {
            if let Some(inst) = self.scene.instances.get(&doc.nodes[i].id) {
                return Some(&inst.symbol);
            }
            cur = doc.nodes[i].parent;
        }
        None
    }

    /// For a copy: its symbol and parameters. For a plain group: «Сделать символом».
    pub(crate) fn symbol_section(&mut self, ui: &mut egui::Ui, node: &Node) {
        let generated = self.scene.sites.get(&node.id).is_some_and(|s| s.generated);
        if let Some(inst) = self.scene.instances.get(&node.id).cloned() {
            let mut change: Option<(String, Value)> = None;
            let mut variant_act = None;
            let variants: Vec<String> =
                self.scene.symbols.iter().filter(|s| s.name == inst.symbol).flat_map(|s| s.variants.iter().map(|v| v.name.clone())).collect();
            let variant = inst.variant.as_ref().map(|(v, _)| v.clone());
            let mut enter = false;
            let mut detach = false;
            let copies = self.copies_of(&inst.symbol);
            let inside = self.scene.editing.as_deref() == Some(node.id.as_str());
            // A symbol of an included file: the copy is set here, the symbol is changed there.
            let from = self.symbol_from(&inst.symbol);
            // The words the library finds the symbol by, typed with commas between them.
            let known_tags: Vec<String> = self.scene.symbols.iter().find(|s| s.name == inst.symbol).map(|s| s.tags.clone()).unwrap_or_default();
            let mut tags_text = match &self.tags_edit {
                Some((symbol, text)) if symbol == &inst.symbol => text.clone(),
                _ => known_tags.join(", "),
            };
            let (mut tags_focus, mut tags_done) = (false, false);
            fold(ui, &format!("Символ {}", inst.symbol), |ui| {
                ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
                if inst.params.is_empty() {
                    ui.label(RichText::new("У символа нет параметров. Их пишут в строке symbol: symbol Имя size=16 color=#ffd23f.").size(11.5).color(FAINT));
                }
                // A symbol with variants shows the choice; «+ Вариант» is there for every copy
                // written by hand, as the way to the first variant.
                if !generated || !variants.is_empty() {
                    variant_act = crate::variant_ui::variant_row(ui, &node.id, &variants, variant.as_deref());
                }
                for p in &inst.params {
                    ui.horizontal(|ui| {
                        let w = ui.available_width();
                        if let Some(value) = crate::variant_ui::param_editor(ui, w, &p.name, p.value) {
                            change = Some((p.name.clone(), value));
                        }
                    });
                    if p.variant {
                        ui.label(RichText::new(format!("из варианта {}", variant.as_deref().unwrap_or_default())).size(10.5).color(FAINT));
                    } else if p.span.is_none() {
                        ui.label(RichText::new("по умолчанию").size(10.5).color(FAINT));
                    }
                }
                let note = if generated {
                    "Копия создана циклом: параметры правятся в её строке use в скрипте.".to_string()
                } else if let Some(file) = &from {
                    format!(
                        "Параметры и вариант меняют только эту копию. Сам символ записан в подключённом файле «{file}»: его меняют там, и копия изменится сама."
                    )
                } else {
                    "Параметры меняют только эту копию. Сам символ правится на холсте: «Править символ» или двойной щелчок по копии.".to_string()
                };
                ui.label(RichText::new(note).size(11.5).color(FAINT));
                if from.is_some() {
                    return;
                }
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Теги").size(11.5).color(MUTED));
                    let field = ui
                        .add(egui::TextEdit::singleline(&mut tags_text).hint_text("кнопка, меню").desired_width(ui.available_width()))
                        .on_hover_text("Слова через запятую, по которым символ находят в «Библиотеке». В скрипте это строка tags внутри символа");
                    tags_focus = field.has_focus();
                    tags_done = field.lost_focus();
                });
                if inside {
                    ui.label(RichText::new(format!("Сейчас правится сам символ: правки меняют все его копии ({copies}).")).size(11.5).color(ACCENT));
                } else {
                    enter = theme::button(ui, "Править символ", ButtonKind::Normal)
                        .on_hover_text(format!(
                            "Войти в копию и править символ на холсте: что вы поменяете, нарисуете или удалите, поменяется во всех копиях ({copies}). То же делает двойной щелчок по копии в «Рисовании»"
                        ))
                        .clicked();
                }
                if !generated {
                    detach = theme::button(ui, "Отцепить копию", ButtonKind::Normal)
                        .on_hover_text(
                            "Копия станет обычной группой с тем же рисунком: символ её больше не меняет, а её можно править как угодно. Ключи анимации и правки копии сохраняются. Остальные копии и сам символ не меняются",
                        )
                        .clicked();
                }
            });
            self.tags_edit = (tags_focus || tags_done).then(|| (inst.symbol.clone(), tags_text.clone()));
            if tags_done {
                self.tags_edit = None;
                let tags: Vec<String> = tags_text.split(',').map(|t| t.trim().to_string()).filter(|t| !t.is_empty()).collect();
                if tags != known_tags {
                    self.set_symbol_tags(&inst.symbol, &tags);
                    return;
                }
            }
            if detach {
                self.detach_copy(&node.id);
                return;
            }
            if from.is_none() {
                self.variant_section(ui, &node.id);
            }
            self.copy_changes_section(ui, &node.id);
            if let Some(act) = variant_act {
                self.variant_act(&node.id, act);
                return;
            }
            if enter {
                self.edit_symbol(&node.id);
                return;
            }
            if let Some((name, value)) = change
                && !generated
                && self.can_edit()
            {
                let out = motif_script::set_param(&self.src, &self.scene, &node.id, &name, value);
                self.apply(&format!("{}.{name}", node.id), out);
            }
            return;
        }
        if node.kind != ShapeKind::Group || generated || self.symbol_of(&node.id).is_some() {
            return;
        }
        let mut make = false;
        fold(ui, "Символ", |ui| {
            make = theme::button(ui, "Сделать символом", ButtonKind::Normal)
                .on_hover_text("Группа станет символом, а на её месте останется копия. Новые копии — из панели артборда (клик по пустому месту).")
                .clicked();
            ui.label(RichText::new("Символ рисуется один раз, а копий может быть сколько угодно, каждая со своими параметрами.").size(11.5).color(FAINT));
        });
        if make && self.can_edit() {
            let name = motif_script::free_symbol_name(&self.scene, &node.id);
            let out = motif_script::make_symbol(&self.src, &self.scene, &node.id, &name);
            if self.apply("make-symbol", out) {
                self.status = format!("Группа «{}» стала символом {name}. Новые копии вставляются из панели артборда", node.id);
            }
        }
    }

    /// «Правки этой копии»: what the copy has apart from its symbol (parameters written on its
    /// `use` line, properties of its objects in `override { }`), each with a way back.
    fn copy_changes_section(&mut self, ui: &mut egui::Ui, copy: &str) {
        let changes = motif_script::copy_changes(&self.src, &self.scene, copy);
        let mut reset: Option<Option<usize>> = None;
        fold(ui, "Правки этой копии", |ui| {
            ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
            if changes.is_empty() {
                ui.label(RichText::new("Копия ничем не отличается от символа.").size(11.5).color(FAINT));
                return;
            }
            for (k, c) in changes.iter().enumerate() {
                ui.horizontal(|ui| {
                    let what = match &c.part {
                        Some(part) => format!("{part} · {}", c.name),
                        None => format!("параметр {}", c.name),
                    };
                    ui.label(RichText::new(what).size(12.0).color(MUTED));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if theme::button(ui, "↺", ButtonKind::Normal).on_hover_text("Вернуть как в символе").clicked() {
                            reset = Some(Some(k));
                        }
                        ui.add(egui::Label::new(RichText::new(&c.value).font(theme::mono(11.5)).color(FG)).truncate());
                    });
                });
            }
            if changes.len() > 1 && theme::button(ui, "Сбросить все", ButtonKind::Normal).on_hover_text("Копия снова рисуется так, как задаёт символ").clicked()
            {
                reset = Some(None);
            }
            let note = "Параметры копии и свойства её объектов, изменённые только для неё (блок override). ↺ возвращает значение символа.";
            ui.label(RichText::new(note).size(11.5).color(FAINT));
        });
        if let Some(which) = reset
            && self.can_edit()
        {
            let out = motif_script::reset_copy(&self.src, &self.scene, copy, which);
            if self.apply_once("reset-copy", out) {
                self.status = match which {
                    Some(_) => "Правка копии снята: значение снова как в символе".into(),
                    None => "Все правки копии сняты: она рисуется как символ".into(),
                };
            }
        }
    }

    /// The list of symbols, each with «Вставить» to place a copy in the middle of the artboard.
    pub(crate) fn symbols_library(&mut self, ui: &mut egui::Ui) {
        let mut place = None;
        fold(ui, "Символы", |ui| {
            if self.scene.symbols.is_empty() {
                ui.label(RichText::new("Пока нет. Выделите группу и нажмите «Сделать символом» или напишите в скрипте блок symbol.").size(11.5).color(FAINT));
                return;
            }
            for s in &self.scene.symbols {
                let copies = self.scene.instances.values().filter(|i| i.symbol == s.name).count();
                ui.horizontal(|ui| {
                    ui.label(RichText::new(&s.name).font(theme::semibold(13.0)).color(ACCENT));
                    let from = self.scene.included(s.span).map(|i| format!(", из «{}»", i.path)).unwrap_or_default();
                    ui.label(RichText::new(format!("копий: {copies}{from}")).size(11.5).color(FAINT));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if theme::button(ui, "Вставить", ButtonKind::Normal).clicked() {
                            place = Some(s.name.clone());
                        }
                    });
                });
            }
        });
        if let Some(symbol) = place
            && self.can_edit()
        {
            let ab = &self.scene.doc.artboard;
            let center = ((ab.width / 2.0).round(), (ab.height / 2.0).round());
            let (out, id) = motif_script::add_use(&self.src, &self.scene, &symbol, center);
            if self.apply("add-use", Some(out)) {
                self.select_only(Some(id.clone()));
                self.status = format!("Вставлена копия {id} символа {symbol}");
            }
        }
    }
}
