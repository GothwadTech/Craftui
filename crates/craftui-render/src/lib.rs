//! Hybrid GPU (`vello`/`wgpu`) and software CPU (`vello_cpu`/`tiny-skia`) 2D vector renderer.

pub mod cpu_backend;
pub mod gpu_backend;
pub mod scene;
pub mod texture;

pub use cpu_backend::{TinySkiaBackend, VelloCpuBackend};
pub use gpu_backend::GpuSurfacePipeline;
pub use kurbo;
pub use scene::{PaintCommand, PaintScene};
pub use texture::{PixelBufferHandle, TextureHandle, TextureRegistry};

use craftui_core::{Color, Result};
use craftui_text::TextEngine;
use std::path::Path;

/// Renderer backend selection mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BackendKind {
    /// Prefer `vello` GPU compute shaders, automatically dropping to `vello_cpu` if unsupported.
    #[default]
    Auto,
    /// Force `vello_cpu` software rasterization (deterministic across headless CI / old GPUs).
    VelloCpu,
    /// Force `tiny-skia` software rasterization.
    TinySkia,
}

/// Convenience helper for headless rendering and snapshot testing without opening a window.
#[allow(clippy::too_many_arguments)]
pub fn render_scene_to_rgba(
    backend: BackendKind,
    scene: &PaintScene,
    width: u32,
    height: u32,
    scale_factor: f32,
    background: Color,
    textures: &TextureRegistry,
    text_engine: &mut TextEngine,
) -> Vec<u8> {
    match backend {
        BackendKind::Auto | BackendKind::VelloCpu => {
            let mut cpu = VelloCpuBackend::new(width, height);
            cpu.render_straight_rgba(
                scene,
                width,
                height,
                scale_factor,
                background,
                textures,
                text_engine,
            )
        }
        BackendKind::TinySkia => TinySkiaBackend::render_straight_rgba(
            scene,
            width,
            height,
            scale_factor,
            background,
            textures,
        ),
    }
}

/// Write straight RGBA8 pixels to a PNG file on disk.
pub fn write_rgba_png(path: impl AsRef<Path>, width: u32, height: u32, rgba: &[u8]) -> Result<()> {
    let file = std::fs::File::create(path)?;
    let w = std::io::BufWriter::new(file);
    let mut encoder = png::Encoder::new(w, width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder
        .write_header()
        .map_err(|e| craftui_core::CraftError::Render(e.to_string()))?;
    writer
        .write_image_data(rgba)
        .map_err(|e| craftui_core::CraftError::Render(e.to_string()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use craftui_core::{Brush, ColorStop, CornerRadii, Fit, Point, Rect};
    use craftui_text::TextStyle;

    #[test]
    fn cpu_and_tinyskia_render_rounded_rect_text_and_pixelbuffer() {
        let mut textures = TextureRegistry::new();
        let tex = textures.create_from_rgba(
            2,
            2,
            vec![
                255, 0, 0, 255, 0, 255, 0, 255, // row 0
                0, 0, 255, 255, 255, 255, 0, 255, // row 1
            ],
        );

        let mut scene = PaintScene::new();
        scene.fill_rect(
            Rect::new(10.0, 10.0, 80.0, 40.0),
            Brush::LinearGradient {
                start: Point::new(10.0, 10.0),
                end: Point::new(90.0, 50.0),
                stops: vec![
                    ColorStop {
                        offset: 0.0,
                        color: Color::rgb(0, 186, 102),
                    },
                    ColorStop {
                        offset: 1.0,
                        color: Color::rgb(16, 120, 220),
                    },
                ],
            },
            CornerRadii::all(8.0),
        );
        scene.push(PaintCommand::DrawImage {
            rect: Rect::new(100.0, 10.0, 32.0, 32.0),
            texture: tex.id,
            fit: Fit::Fill,
            radius: CornerRadii::all(4.0),
            opacity: 1.0,
        });
        scene.draw_text(
            Point::new(16.0, 22.0),
            "CraftUI",
            TextStyle::default()
                .with_size(14.0)
                .with_color(Color::WHITE),
            Some(120.0),
        );

        let mut text = TextEngine::new();
        let rgba = render_scene_to_rgba(
            BackendKind::VelloCpu,
            &scene,
            160,
            80,
            1.0,
            Color::rgb(17, 20, 26),
            &textures,
            &mut text,
        );
        assert_eq!(rgba.len(), 160 * 80 * 4);
        // Background corner pixel should match background
        assert_eq!(&rgba[0..4], &[17, 20, 26, 255]);
        // Pixel inside image top-left (102, 12) should be red-dominant
        let idx = (12 * 160 + 102) * 4;
        assert!(rgba[idx] > 200 && rgba[idx + 1] < 50);

        // Damage update on pixel buffer
        tex.buffer.with_pixels_mut(|_, _, px| {
            px[0] = 0;
            px[1] = 255;
        });
        tex.buffer.commit_damage(Rect::new(0.0, 0.0, 1.0, 1.0));
        let rgba2 = render_scene_to_rgba(
            BackendKind::VelloCpu,
            &scene,
            160,
            80,
            1.0,
            Color::rgb(17, 20, 26),
            &textures,
            &mut text,
        );
        assert!(rgba2[idx + 1] > 200);

        // TinySkia fallback check
        let tiny = render_scene_to_rgba(
            BackendKind::TinySkia,
            &scene,
            160,
            80,
            1.0,
            Color::rgb(17, 20, 26),
            &textures,
            &mut text,
        );
        assert_eq!(tiny.len(), 160 * 80 * 4);
    }
}
