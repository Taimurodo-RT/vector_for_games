//! Draws the artboard into the central panel: Vello on the GPU, with a tiny-skia fallback.

use eframe::egui_wgpu::{self, wgpu};
use egui::{Color32, ColorImage, Rect, TextureHandle, TextureId, TextureOptions};
use motif_core::Document;
use motif_core::kurbo::{Affine, Vec2};
use motif_render::vello::{self, AaConfig, AaSupport, RenderParams, Renderer, RendererOptions};
use motif_render::{BackdropView, CpuOptions, render_cpu_with};

/// Maps artboard units to points on screen.
#[derive(Clone, Copy, Debug)]
pub struct View {
    pub zoom: f32,
    /// Screen position (points) of the artboard origin, relative to the canvas rect.
    pub pan: egui::Vec2,
}

impl View {
    pub fn to_screen(self, canvas: Rect, p: motif_core::kurbo::Point) -> egui::Pos2 {
        canvas.min + self.pan + egui::vec2(p.x as f32, p.y as f32) * self.zoom
    }

    pub fn to_doc(self, canvas: Rect, p: egui::Pos2) -> motif_core::kurbo::Point {
        let v = (p - canvas.min - self.pan) / self.zoom;
        motif_core::kurbo::Point::new(v.x as f64, v.y as f64)
    }

    pub fn fit(canvas: Rect, doc: &Document) -> View {
        let margin = 48.0;
        let (w, h) = (doc.artboard.width as f32, doc.artboard.height as f32);
        let zoom = ((canvas.width() - 2.0 * margin) / w).min((canvas.height() - 2.0 * margin) / h).clamp(0.05, 16.0);
        View { zoom, pan: egui::vec2((canvas.width() - w * zoom) / 2.0, (canvas.height() - h * zoom) / 2.0) }
    }
}

pub const STAGE: Color32 = Color32::from_rgb(10, 12, 16);

struct Gpu {
    renderer: Renderer,
    texture: Option<(wgpu::Texture, wgpu::TextureView, TextureId, [u32; 2])>,
}

pub struct Canvas {
    gpu: Option<Gpu>,
    gpu_failed: Option<String>,
    cpu: Option<TextureHandle>,
}

impl Canvas {
    pub fn new() -> Self {
        Canvas { gpu: None, gpu_failed: None, cpu: None }
    }

    /// Short description of the active backend for the status bar.
    pub fn backend(&self) -> String {
        match (&self.gpu, &self.gpu_failed) {
            (Some(_), _) => "Vello на GPU (wgpu)".into(),
            (None, Some(e)) => format!("CPU (tiny-skia): GPU недоступен — {e}"),
            (None, None) => "CPU (tiny-skia)".into(),
        }
    }

    /// Paints `doc` into `rect` and returns nothing; overlays are drawn by the caller afterwards.
    pub fn paint(&mut self, ui: &mut egui::Ui, frame: &eframe::Frame, rect: Rect, view: View, doc: &Document, backdrop: Option<BackdropView>) {
        if self.gpu.is_none()
            && self.gpu_failed.is_none()
            && let Some(rs) = frame.wgpu_render_state()
        {
            let opts = RendererOptions { antialiasing_support: AaSupport::area_only(), ..RendererOptions::default() };
            match Renderer::new(&rs.device, opts) {
                Ok(renderer) => self.gpu = Some(Gpu { renderer, texture: None }),
                Err(e) => self.gpu_failed = Some(e.to_string()),
            }
        }
        if let (Some(gpu), Some(rs)) = (self.gpu.as_mut(), frame.wgpu_render_state()) {
            match paint_gpu(gpu, rs, ui, rect, view, doc, backdrop) {
                Ok(()) => return,
                Err(e) => {
                    self.gpu = None;
                    self.gpu_failed = Some(e);
                }
            }
        }
        self.paint_cpu(ui, rect, view, doc, backdrop);
    }

    fn paint_cpu(&mut self, ui: &mut egui::Ui, rect: Rect, view: View, doc: &Document, backdrop: Option<BackdropView>) {
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 0.0, STAGE);
        let ppp = ui.ctx().pixels_per_point();
        // Cap the raster size so a huge zoom does not allocate gigabytes.
        let scale = (view.zoom * ppp).min(4096.0 / doc.artboard.width.max(doc.artboard.height) as f32) as f64;
        let Some(px) = render_cpu_with(doc, CpuOptions { scale, antialias: true, background: true }, backdrop) else { return };
        let image = ColorImage::from_rgba_premultiplied([px.width() as usize, px.height() as usize], px.data());
        let tex = match &mut self.cpu {
            Some(t) => {
                t.set(image, TextureOptions::LINEAR);
                t.id()
            }
            None => {
                let t = ui.ctx().load_texture("motif-canvas", image, TextureOptions::LINEAR);
                let id = t.id();
                self.cpu = Some(t);
                id
            }
        };
        let board = artboard_rect(rect, view, doc);
        painter.image(tex, board, Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), Color32::WHITE);
    }
}

pub fn artboard_rect(canvas: Rect, view: View, doc: &Document) -> Rect {
    Rect::from_min_size(canvas.min + view.pan, egui::vec2(doc.artboard.width as f32, doc.artboard.height as f32) * view.zoom)
}

fn paint_gpu(
    gpu: &mut Gpu,
    rs: &egui_wgpu::RenderState,
    ui: &mut egui::Ui,
    rect: Rect,
    view: View,
    doc: &Document,
    backdrop: Option<BackdropView>,
) -> Result<(), String> {
    let ppp = ui.ctx().pixels_per_point();
    let size = [((rect.width() * ppp).round() as u32).max(1), ((rect.height() * ppp).round() as u32).max(1)];
    let needs_new = gpu.texture.as_ref().is_none_or(|t| t.3 != size);
    if needs_new {
        let texture = rs.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("motif-canvas"),
            size: wgpu::Extent3d { width: size[0], height: size[1], depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let tv = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let mut egui_renderer = rs.renderer.write();
        let id = match gpu.texture.take() {
            Some((_, _, id, _)) => {
                egui_renderer.update_egui_texture_from_wgpu_texture(&rs.device, &tv, wgpu::FilterMode::Linear, id);
                id
            }
            None => egui_renderer.register_native_texture(&rs.device, &tv, wgpu::FilterMode::Linear),
        };
        gpu.texture = Some((texture, tv, id, size));
    }
    let (_, tv, id, _) = gpu.texture.as_ref().expect("texture was just created");

    let mut scene = vello::Scene::new();
    let to_px = Affine::scale(ppp as f64) * Affine::translate(Vec2::new(view.pan.x as f64, view.pan.y as f64)) * Affine::scale(view.zoom as f64);
    motif_render::build_scene(&mut scene, doc, to_px, backdrop);
    let params = RenderParams {
        base_color: vello::peniko::Color::from_rgba8(STAGE.r(), STAGE.g(), STAGE.b(), 255),
        width: size[0],
        height: size[1],
        antialiasing_method: AaConfig::Area,
    };
    gpu.renderer.render_to_texture(&rs.device, &rs.queue, &scene, tv, &params).map_err(|e| e.to_string())?;
    ui.painter_at(rect).image(*id, rect, Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), Color32::WHITE);
    Ok(())
}
