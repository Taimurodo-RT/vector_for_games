#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

//! Motif editor: write a scene script, see it live, finish it with the mouse.

mod app;
mod body_ui;
mod bone_ui;
mod canvas;
mod field_ui;
mod guides;
mod highlight;
mod importer;
mod kit_ui;
mod machine_ui;
mod morph_ui;
mod orbit_ui;
mod paint;
mod pathedit;
mod physics_ui;
mod pixabridge;
mod rope_ui;
mod scriptui;
mod selection;
mod snap;
mod style_ui;
mod symbol_ui;
mod text_ui;
mod theme;
mod timeline;
mod world_ui;

fn main() -> eframe::Result {
    let path = std::env::args().nth(1).map(std::path::PathBuf::from);
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_title("Motif").with_inner_size([1440.0, 900.0]).with_min_inner_size([900.0, 560.0]),
        renderer: eframe::Renderer::Wgpu,
        ..Default::default()
    };
    eframe::run_native("Motif", options, Box::new(move |cc| Ok(Box::new(app::App::new(cc, path)))))
}
