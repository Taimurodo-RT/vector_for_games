//! Variants of a symbol in the panel of a copy: which variant the copy is, a new variant from
//! the look of the copy, and what the variant holds (its values and the objects it hides).

use crate::app::{App, fold};
use crate::theme::{self, ACCENT, ButtonKind, FAINT, FG, KeyState, LINE, MUTED};
use egui::{Color32, RichText, Sense, Stroke, StrokeKind, vec2};
use motif_core::{Color, Value};

/// What was asked for in the panel.
pub(crate) enum VariantAct {
    /// The copy becomes this variant, or the symbol itself.
    Take(Option<String>),
    /// A new variant from the look of the copy.
    New,
    Rename(String),
    Delete,
    /// The variant sets the parameter, or stops setting it.
    Value(String, Option<Value>),
    /// The variant hides the object of the symbol, or shows it.
    Visible(String, bool),
}

/// The editor of one parameter value: a number field or a color swatch with its code. Hands
/// back the new value.
pub(crate) fn param_editor(ui: &mut egui::Ui, width: f32, name: &str, value: Value) -> Option<Value> {
    let mut change = None;
    match value {
        Value::Num(v) => {
            let out = theme::num_field(ui, width, name, v, 0.5, f64::MIN..=f64::MAX, 2, KeyState::Hidden);
            if let Some(nv) = out.changed {
                change = Some(Value::Num((nv * 100.0).round() / 100.0));
            }
        }
        Value::Color(c) => {
            let (sw, resp) = ui.allocate_exact_size(vec2(28.0, 28.0), Sense::click());
            let mut c32 = Color32::from_rgba_unmultiplied(c.r, c.g, c.b, c.a);
            ui.painter().rect_filled(sw, 6.0, c32);
            ui.painter().rect_stroke(sw, 6.0, Stroke::new(1.0, LINE), StrokeKind::Inside);
            theme::color_popup(&resp).show(|ui| {
                if egui::color_picker::color_picker_color32(ui, &mut c32, egui::color_picker::Alpha::OnlyBlend) {
                    let [r, g, b, a] = c32.to_srgba_unmultiplied();
                    change = Some(Value::Color(Color { r, g, b, a }));
                }
            });
            ui.label(RichText::new(name).size(12.0).color(MUTED));
            ui.label(RichText::new(c.to_hex()).font(theme::mono(11.5)).color(FG));
        }
        Value::None => {}
    }
    change
}

/// The row «Вариант» of a copy: the variant it is, chosen from the variants of its symbol, and
/// «+ Вариант» that keeps the look of the copy as a new one.
pub(crate) fn variant_row(ui: &mut egui::Ui, copy: &str, variants: &[String], current: Option<&str>) -> Option<VariantAct> {
    let mut act = None;
    ui.horizontal(|ui| {
        ui.label(RichText::new("Вариант").size(12.0).color(MUTED));
        // The choice takes the rest of the row and cuts a long name: a row wider than the panel
        // would widen every fold below it.
        let mut chosen = current.unwrap_or_default().to_string();
        let width = (ui.available_width() - 8.0).max(60.0);
        egui::ComboBox::from_id_salt(("copy-variant", copy))
            .width(width)
            .truncate()
            .selected_text(if chosen.is_empty() { "как в символе" } else { chosen.as_str() })
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut chosen, String::new(), "как в символе");
                for name in variants {
                    ui.selectable_value(&mut chosen, name.clone(), name);
                }
            });
        if chosen != current.unwrap_or_default() {
            act = Some(VariantAct::Take((!chosen.is_empty()).then_some(chosen)));
        }
    });
    if theme::button(ui, "+ Вариант", ButtonKind::Normal)
        .on_hover_text(
            "Новый вариант символа из этой копии: её параметры переходят в вариант, а копия становится этим вариантом и выглядит как раньше. Тот же вид потом выбирают у других копий",
        )
        .clicked()
    {
        act = Some(VariantAct::New);
    }
    act
}

impl App {
    /// «Вариант имя»: what the variant of copy `copy` holds. Its values and the objects it hides
    /// are edited here for every copy that is this variant.
    pub(crate) fn variant_section(&mut self, ui: &mut egui::Ui, copy: &str) {
        let Some(inst) = self.scene.instances.get(copy).cloned() else { return };
        let Some((name, _)) = inst.variant.clone() else { return };
        let Some(sym) = self.scene.symbols.iter().find(|s| s.name == inst.symbol).cloned() else { return };
        let Some(variant) = sym.variants.iter().find(|v| v.name == name).cloned() else { return };
        let copies = self.scene.instances.values().filter(|i| i.symbol == inst.symbol && i.variant.as_ref().is_some_and(|(v, _)| *v == name)).count();
        // The objects of the symbol, as this copy has them.
        let parts: Vec<String> = self.scene.doc.nodes.iter().filter_map(|n| motif_script::part_of(&self.scene, copy, &n.id)).map(str::to_string).collect();
        let src = self.src.clone();
        let mut act: Option<VariantAct> = None;
        fold(ui, &format!("Вариант {name}"), |ui| {
            ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
            ui.horizontal(|ui| {
                ui.label(RichText::new("Имя").size(12.0).color(MUTED));
                if let Some(new) = crate::machine_ui::name_edit(ui, ("variant-name", &inst.symbol, &name), &name, 118.0) {
                    act = Some(VariantAct::Rename(new));
                }
                if theme::button(ui, "×", ButtonKind::Normal)
                    .on_hover_text(format!("Удалить вариант. Копии с ним ({copies}) снова рисуются как сам символ; что записано у самой копии, остаётся"))
                    .clicked()
                {
                    act = Some(VariantAct::Delete);
                }
            });
            ui.label(RichText::new("Значения параметров").size(11.5).color(FAINT));
            if variant.values.is_empty() {
                ui.label(RichText::new("Пока как в символе.").size(11.5).color(FAINT));
            }
            for v in &variant.values {
                let written = &src[v.span.start..v.span.end];
                // A value written as a number or a color is edited in place; a name of the
                // palette or a formula stays as written.
                let literal = written.starts_with('#') || written.parse::<f64>().is_ok();
                ui.horizontal(|ui| {
                    let w = ui.available_width() - 34.0;
                    match v.value.filter(|_| literal) {
                        Some(value) => {
                            if let Some(new) = param_editor(ui, w, &v.name, value) {
                                act = Some(VariantAct::Value(v.name.clone(), Some(new)));
                            }
                        }
                        None => {
                            ui.label(RichText::new(&v.name).size(12.0).color(MUTED));
                            ui.label(RichText::new(written).font(theme::mono(11.5)).color(FG));
                        }
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if theme::button(ui, "×", ButtonKind::Normal).on_hover_text("Вариант больше не задаёт этот параметр: будет значение символа").clicked()
                        {
                            act = Some(VariantAct::Value(v.name.clone(), None));
                        }
                    });
                });
            }
            let free: Vec<&str> = sym.params.iter().map(|(p, _)| p.as_str()).filter(|p| !variant.values.iter().any(|v| v.name == *p)).collect();
            if !free.is_empty() {
                let mut add = String::new();
                egui::ComboBox::from_id_salt(("variant-add", &inst.symbol, &name))
                    .width(ui.available_width() - 16.0)
                    .selected_text("Добавить параметр…")
                    .show_ui(ui, |ui| {
                        for p in &free {
                            ui.selectable_value(&mut add, p.to_string(), *p);
                        }
                    });
                // The variant starts from the value the copy shows now.
                if let Some(p) = inst.params.iter().find(|p| p.name == add) {
                    act = Some(VariantAct::Value(add, Some(p.value)));
                }
            }
            ui.label(RichText::new("Видимые детали").size(11.5).color(FAINT));
            if parts.is_empty() {
                ui.label(RichText::new("В символе нет объектов.").size(11.5).color(FAINT));
            }
            for part in &parts {
                // As the symbol has it, unless the variant says otherwise.
                let in_symbol = !sym.hidden.contains(part);
                let listed = |ids: &[(String, motif_script::Span)]| ids.iter().any(|(id, _)| id == part);
                let mut visible = listed(&variant.show) || (in_symbol && !listed(&variant.hide));
                ui.horizontal(|ui| {
                    if ui.checkbox(&mut visible, RichText::new(part).size(12.5).color(FG)).changed() {
                        act = Some(VariantAct::Visible(part.clone(), visible));
                    }
                    if !in_symbol {
                        ui.label(RichText::new("скрыта в символе").size(10.5).color(FAINT));
                    }
                });
            }
            let note = format!(
                "Вариант — значения параметров символа и то, какие его детали видны. Что вы меняете здесь, меняется у всех копий с этим вариантом ({copies}). Параметр, заданный у самой копии, сильнее варианта."
            );
            ui.label(RichText::new(note).size(11.5).color(if copies > 1 { ACCENT } else { FAINT }));
        });
        if let Some(act) = act {
            self.variant_act(copy, act);
        }
    }

    /// Does what the panel of copy `copy` asked for about variants.
    pub(crate) fn variant_act(&mut self, copy: &str, act: VariantAct) {
        if !self.can_edit() {
            return;
        }
        let Some(inst) = self.scene.instances.get(copy).cloned() else { return };
        if self.scene.sites.get(copy).is_some_and(|s| s.generated) && matches!(act, VariantAct::Take(_) | VariantAct::New) {
            self.status = "Копия создана циклом: вариант пишется в её строке use в скрипте".into();
            return;
        }
        // The variants of a symbol of an included file are lines of that file.
        if !matches!(act, VariantAct::Take(_))
            && let Some(file) = self.symbol_from(&inst.symbol)
        {
            self.refuse_included(&format!("Символ {} с его вариантами", inst.symbol), &file);
            return;
        }
        let current = inst.variant.as_ref().map(|(v, _)| v.clone());
        let base = self.path.as_ref().and_then(|p| p.parent()).map(std::path::Path::to_path_buf);
        let base = base.as_deref();
        match act {
            VariantAct::Take(name) => {
                let out = motif_script::set_variant(&self.src, &self.scene, copy, name.as_deref());
                if self.apply_once("variant-take", out) {
                    self.status = match name {
                        Some(name) => format!("Копия «{copy}» теперь вариант «{name}» символа {}", inst.symbol),
                        None => format!("Копия «{copy}» снова рисуется как сам символ {}", inst.symbol),
                    };
                }
            }
            VariantAct::New => {
                let name = motif_script::free_variant_name(&self.scene, &inst.symbol);
                let out = motif_script::variant_from_copy(&self.src, &self.scene, copy, &name);
                if self.apply_once("variant-new", out) {
                    self.status = format!("Вид копии «{copy}» сохранён как вариант «{name}» символа {}. Имя меняется ниже, в разделе варианта", inst.symbol);
                }
            }
            VariantAct::Rename(new) => {
                let (Some(old), new) = (current, new.trim().to_string()) else { return };
                let taken = self.scene.symbols.iter().any(|s| s.name == inst.symbol && s.variants.iter().any(|v| v.name == new));
                if !motif_script::variant_name_ok(&new) || taken {
                    self.status = format!("Имя «{new}» не подходит: нужно одно слово из букв, цифр, «_» и «-», которого ещё нет среди вариантов символа");
                    return;
                }
                let out = motif_script::rename_variant(&self.file_text(), base, self.boards.active, &inst.symbol, &old, &new);
                if self.apply_file("variant-rename", out) {
                    self.status = format!("Вариант «{old}» теперь «{new}»");
                }
            }
            VariantAct::Delete => {
                let Some(name) = current else { return };
                let out = motif_script::delete_variant(&self.file_text(), base, self.boards.active, &inst.symbol, &name);
                if self.apply_file("variant-delete", out) {
                    self.status = format!("Вариант «{name}» удалён: копии с ним рисуются как сам символ {}", inst.symbol);
                }
            }
            VariantAct::Value(param, value) => {
                let Some(name) = current else { return };
                let out = motif_script::set_variant_value(&self.src, &self.scene, &inst.symbol, &name, &param, value);
                // Dragging a number is one step back; adding and removing a value are steps of their own.
                let kind = format!("variant-value:{name}.{param}");
                let gone = value.is_none();
                let had = self
                    .scene
                    .symbols
                    .iter()
                    .any(|s| s.name == inst.symbol && s.variants.iter().any(|v| v.name == name && v.values.iter().any(|p| p.name == param)));
                let done = if gone || !had { self.apply_once(&kind, out) } else { self.apply(&kind, out) };
                if done && gone {
                    self.status = format!("Вариант «{name}» больше не задаёт «{param}»");
                } else if done && !had {
                    self.status = format!("Вариант «{name}» теперь задаёт «{param}»");
                }
            }
            VariantAct::Visible(part, visible) => {
                let Some(name) = current else { return };
                let out = motif_script::set_variant_visible(&self.src, &self.scene, &inst.symbol, &name, &part, visible);
                if self.apply_once("variant-visible", out) {
                    self.status = if visible {
                        format!("Вариант «{name}» показывает «{part}» во всех своих копиях")
                    } else {
                        format!("Вариант «{name}» скрывает «{part}» во всех своих копиях")
                    };
                }
            }
        }
    }
}
