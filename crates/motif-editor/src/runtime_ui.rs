//! «Motif в игре» in the export window: puts the saved scene into a Godot project for the
//! MotifSprite node, which plays it there as vectors, and keeps it copied there on every save.

use crate::app::App;
use crate::theme::{self, ButtonKind, FAINT};
use egui::RichText;
use motif_pixel::runtime;

impl App {
    pub(crate) fn runtime_export(&mut self, ui: &mut egui::Ui) {
        theme::caption(ui, "Motif в игре (MotifSprite)");
        let saved = self.path.clone();
        let r = ui.add_enabled_ui(saved.is_some(), |ui| theme::button(ui, "Поставить в проект Godot…", ButtonKind::Normal)).inner;
        let r = r
            .on_hover_text("Аддон Motif в addons/motif, сцена .msc со шрифтами в выбранную папку проекта и сцена .tscn с узлом MotifSprite, который её играет")
            .on_disabled_hover_text("Сначала сохраните сцену в файл (Ctrl+S)");
        if r.clicked()
            && let Some(scene) = saved.clone()
            && let Some(dir) = rfd::FileDialog::new().set_title("Папка внутри проекта Godot").pick_folder()
        {
            self.install_runtime(&scene, &dir);
        }
        if let Some(scene) = saved
            && let Some(dir) = self.pixa.runtime_link(&scene).cloned()
        {
            let mut unlink = false;
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new(format!("Копируется при каждом сохранении → {}", crate::pixabridge::short(&dir))).size(12.0))
                    .on_hover_text(dir.display().to_string());
                unlink = theme::button(ui, "Отвязать", ButtonKind::Normal).on_hover_text("Больше не копировать сцену в проект при сохранении").clicked();
            });
            if unlink {
                self.pixa.set_runtime_link(&scene, None);
            }
        }
        ui.label(
            RichText::new("Узел MotifSprite играет сцену прямо в игре вектором, без запекания: машина состояний, кости, физика, 3D — как здесь. Из кода: set_input(\"speed\", 1), fire(\"jump\"), сигнал event. После установки каждое Ctrl+S копирует сцену в проект, Godot покажет её при следующем запуске.")
                .size(12.0)
                .color(FAINT),
        );
        ui.add_space(6.0);
        theme::rule(ui);
        ui.add_space(4.0);
    }

    fn install_runtime(&mut self, scene: &std::path::Path, dir: &std::path::Path) {
        let src = self.file_text();
        let addon = runtime::addon_source();
        match runtime::install(scene, &src, dir, addon.as_deref()) {
            Ok(r) => {
                self.pixa.set_runtime_link(scene, Some(dir.to_path_buf()));
                self.status = format!("В проекте Godot — {}", r.report().join(" · "));
            }
            Err(e) => self.status = format!("Не поставлено: {e}"),
        }
    }
}
