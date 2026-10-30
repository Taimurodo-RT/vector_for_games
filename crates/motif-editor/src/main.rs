#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

//! Motif editor: write a scene script, see it live, finish it with the mouse.

mod app;
mod boards_ui;
mod body_ui;
mod bone_ui;
mod brush_ui;
mod canvas;
mod character_ui;
mod check_ui;
mod compound;
mod field_ui;
mod guides;
mod highlight;
mod importer;
mod kit_ui;
mod layers;
mod layout_ui;
mod library_ui;
mod link_ui;
mod machine_ui;
mod morph_ui;
mod motion_ui;
mod orbit_ui;
mod paint;
mod palette_ui;
mod parameter_ui;
mod pathedit;
mod physics_ui;
mod pixabridge;
mod preview;
mod psd;
mod rope_ui;
mod runtime_ui;
mod scriptui;
mod selection;
mod sheet;
mod sides_ui;
mod snap;
mod storage;
mod stretch_ui;
mod style_ui;
mod symbol_ui;
mod text_ui;
mod theme;
mod timeline;
mod video_ui;
mod web_ui;
mod weights_ui;
mod workspace;
mod world_ui;

/// The scene to open and the port of the live link: `motif-editor [--link[=ПОРТ]] [сцена.msc]`.
/// A game that starts Motif passes `--link`; `MOTIF_LINK=ПОРТ` does the same.
fn launch(args: impl Iterator<Item = String>) -> (Option<std::path::PathBuf>, Option<u16>) {
    let (mut path, mut link) = (None, std::env::var("MOTIF_LINK").ok().and_then(|p| p.parse().ok()));
    for a in args {
        if a == "--link" {
            link = Some(link.unwrap_or(motif_link::DEFAULT_PORT));
        } else if let Some(port) = a.strip_prefix("--link=") {
            link = port.parse().ok().or(Some(motif_link::DEFAULT_PORT));
        } else if path.is_none() {
            path = Some(std::path::PathBuf::from(a));
        }
    }
    (path, link)
}

fn main() -> eframe::Result {
    let (path, link) = launch(std::env::args().skip(1));
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_title("Motif").with_inner_size([1440.0, 900.0]).with_min_inner_size([900.0, 560.0]),
        renderer: eframe::Renderer::Wgpu,
        ..Default::default()
    };
    eframe::run_native("Motif", options, Box::new(move |cc| Ok(Box::new(app::App::new(cc, path, link)))))
}
