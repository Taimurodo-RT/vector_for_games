//! Symbols in the panels: a copy's parameters, «Сделать символом» for a group, and the list of
//! symbols to place copies from.

use crate::app::{App, section};
use crate::theme::{self, ACCENT, ButtonKind, FAINT, FG, KeyState, LINE, MUTED};
use egui::{Color32, RichText, Sense, Stroke, StrokeKind, vec2};
use motif_core::{Color, Node, ShapeKind, Value};

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
            section(ui, |ui| {
                theme::caption(ui, &format!("Символ {}", inst.symbol));
                ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
                if inst.params.is_empty() {
                    ui.label(RichText::new("У символа нет параметров. Их пишут в строке symbol: symbol Имя size=16 color=#ffd23f.").size(11.5).color(FAINT));
                }
                for p in &inst.params {
                    let written = p.span.is_some();
                    match p.value {
                        Value::Num(v) => {
                            let w = ui.available_width();
                            let out = theme::num_field(ui, w, &p.name, v, 0.5, f64::MIN..=f64::MAX, 2, KeyState::Hidden);
                            if let Some(nv) = out.changed {
                                change = Some((p.name.clone(), Value::Num((nv * 100.0).round() / 100.0)));
                            }
                        }
                        Value::Color(c) => {
                            ui.horizontal(|ui| {
                                let (sw, resp) = ui.allocate_exact_size(vec2(28.0, 28.0), Sense::click());
                                let mut c32 = Color32::from_rgba_unmultiplied(c.r, c.g, c.b, c.a);
                                ui.painter().rect_filled(sw, 6.0, c32);
                                ui.painter().rect_stroke(sw, 6.0, Stroke::new(1.0, LINE), StrokeKind::Inside);
                                theme::color_popup(&resp).show(|ui| {
                                    if egui::color_picker::color_picker_color32(ui, &mut c32, egui::color_picker::Alpha::OnlyBlend) {
                                        let [r, g, b, a] = c32.to_srgba_unmultiplied();
                                        change = Some((p.name.clone(), Value::Color(Color { r, g, b, a })));
                                    }
                                });
                                ui.label(RichText::new(&p.name).size(12.0).color(MUTED));
                                ui.label(RichText::new(c.to_hex()).font(theme::mono(11.5)).color(FG));
                            });
                        }
                        Value::None => {}
                    }
                    if !written {
                        ui.label(RichText::new("по умолчанию").size(10.5).color(FAINT));
                    }
                }
                let note = if generated {
                    "Копия создана циклом: параметры правятся в её строке use в скрипте."
                } else {
                    "Параметры меняют только эту копию. Чтобы поменять все копии, правьте блок symbol в скрипте."
                };
                ui.label(RichText::new(note).size(11.5).color(FAINT));
            });
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
        section(ui, |ui| {
            theme::caption(ui, "Символ");
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

    /// The list of symbols, each with «Вставить» to place a copy in the middle of the artboard.
    pub(crate) fn symbols_library(&mut self, ui: &mut egui::Ui) {
        let mut place = None;
        section(ui, |ui| {
            theme::caption(ui, "Символы");
            if self.scene.symbols.is_empty() {
                ui.label(RichText::new("Пока нет. Выделите группу и нажмите «Сделать символом» или напишите в скрипте блок symbol.").size(11.5).color(FAINT));
                return;
            }
            for s in &self.scene.symbols {
                let copies = self.scene.instances.values().filter(|i| i.symbol == s.name).count();
                ui.horizontal(|ui| {
                    ui.label(RichText::new(&s.name).font(theme::semibold(13.0)).color(ACCENT));
                    ui.label(RichText::new(format!("копий: {copies}")).size(11.5).color(FAINT));
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
