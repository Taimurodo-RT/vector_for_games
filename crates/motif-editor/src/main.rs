//! Motif editor: write a scene script, see it live, finish it with the mouse.

mod app;
mod canvas;

fn main() -> eframe::Result {
    let path = std::env::args().nth(1).map(std::path::PathBuf::from);
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_title("Motif").with_inner_size([1440.0, 900.0]).with_min_inner_size([900.0, 560.0]),
        renderer: eframe::Renderer::Wgpu,
        ..Default::default()
    };
    eframe::run_native("Motif", options, Box::new(move |cc| Ok(Box::new(app::App::new(cc, path)))))
}
