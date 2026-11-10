use crate::cpu_backend::VelloCpuBackend;
use crate::scene::{PaintCommand, PaintScene, brush_to_peniko, compute_image_transform};
use crate::texture::TextureRegistry;
use craftui_core::{Color, CraftError, Result, TextureId};
use craftui_text::TextEngine;
use kurbo::{Affine, Stroke};
use parley::PositionedLayoutItem;
use std::collections::HashMap;
use std::num::NonZeroUsize;
use vello::AaConfig;
use vello::peniko;
use vello::{RenderParams, Renderer, RendererOptions, Scene};
use wgpu::util::TextureBlitter;

struct CachedGpuImage {
    version: u64,
    brush: peniko::ImageBrush,
}

/// Active GPU rendering pipeline backed by `vello` + `wgpu`.
///
/// Falls back seamlessly to CPU upload-and-blit when GPU compute shaders are unavailable.
pub struct GpuSurfacePipeline {
    pub instance: wgpu::Instance,
    pub adapter: wgpu::Adapter,
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    pub surface: wgpu::Surface<'static>,
    pub config: wgpu::SurfaceConfiguration,
    blitter: TextureBlitter,
    target_texture: wgpu::Texture,
    target_view: wgpu::TextureView,
    vello_renderer: Option<Renderer>,
    cpu_fallback: Option<VelloCpuBackend>,
    cpu_upload_texture: Option<wgpu::Texture>,
    cpu_upload_view: Option<wgpu::TextureView>,
    scene: Scene,
    image_cache: HashMap<TextureId, CachedGpuImage>,
}

impl GpuSurfacePipeline {
    /// Initialize `wgpu` surface + `vello::Renderer`, dropping gracefully to `VelloCpuBackend`
    /// with texture blit if compute shaders are unavailable or `force_cpu` is true.
    pub fn new(
        window: impl Into<wgpu::SurfaceTarget<'static>>,
        width: u32,
        height: u32,
        force_cpu: bool,
    ) -> Result<Self> {
        let backends = wgpu::Backends::from_env().unwrap_or_default();
        let mut desc = wgpu::InstanceDescriptor::new_without_display_handle();
        desc.backends = backends;
        let instance = wgpu::Instance::new(desc);
        let surface = instance
            .create_surface(window)
            .map_err(|e| CraftError::Render(format!("create_surface failed: {e}")))?;
        Self::from_instance_and_surface(instance, surface, width, height, force_cpu)
    }

    /// Initialize GPU/CPU pipeline from an unsafe raw native surface target (e.g. Android `ANativeWindow` or iOS `CAMetalLayer`).
    ///
    /// # Safety
    /// Caller must ensure `target` references a live native window or layer handle that outlives the returned [`GpuSurfacePipeline`].
    pub unsafe fn new_unsafe(
        target: wgpu::SurfaceTargetUnsafe,
        width: u32,
        height: u32,
        force_cpu: bool,
    ) -> Result<Self> {
        let backends = wgpu::Backends::from_env().unwrap_or_default();
        let mut desc = wgpu::InstanceDescriptor::new_without_display_handle();
        desc.backends = backends;
        let instance = wgpu::Instance::new(desc);
        // SAFETY: forwarded from `new_unsafe` caller invariant that `target` outlives the surface.
        let surface = unsafe { instance.create_surface_unsafe(target) }
            .map_err(|e| CraftError::Render(format!("create_surface_unsafe failed: {e}")))?;
        Self::from_instance_and_surface(instance, surface, width, height, force_cpu)
    }

    fn from_instance_and_surface(
        instance: wgpu::Instance,
        surface: wgpu::Surface<'static>,
        width: u32,
        height: u32,
        force_cpu: bool,
    ) -> Result<Self> {
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: Some(&surface),
            force_fallback_adapter: false,
            apply_limit_buckets: false,
        }))
        .map_err(|e| CraftError::Render(format!("no compatible wgpu adapter: {e}")))?;

        let supports_compute = adapter.limits().max_compute_workgroups_per_dimension > 0
            && adapter.limits().max_storage_buffers_per_shader_stage >= 4;

        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("craftui-wgpu-device"),
            required_features: wgpu::Features::empty(),
            required_limits: adapter.limits(),
            memory_hints: wgpu::MemoryHints::default(),
            experimental_features: wgpu::ExperimentalFeatures::disabled(),
            trace: wgpu::Trace::Off,
        }))
        .map_err(|e| CraftError::Render(format!("request_device failed: {e}")))?;

        let caps = surface.get_capabilities(&adapter);
        let format = caps
            .formats
            .iter()
            .copied()
            .find(|f| {
                matches!(
                    f,
                    wgpu::TextureFormat::Bgra8Unorm | wgpu::TextureFormat::Rgba8Unorm
                )
            })
            .unwrap_or(caps.formats[0]);

        let w = width.max(1);
        let h = height.max(1);
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            color_space: wgpu::SurfaceColorSpace::Auto,
            width: w,
            height: h,
            present_mode: wgpu::PresentMode::AutoVsync,
            desired_maximum_frame_latency: 2,
            alpha_mode: wgpu::CompositeAlphaMode::Auto,
            view_formats: vec![],
        };
        surface.configure(&device, &config);

        let (target_texture, target_view) = create_vello_target(&device, w, h);
        let blitter = TextureBlitter::new(&device, format);

        let use_vello_gpu = !force_cpu && supports_compute;
        let vello_renderer = if use_vello_gpu {
            match Renderer::new(
                &device,
                RendererOptions {
                    use_cpu: false,
                    antialiasing_support: vello::AaSupport::area_only(),
                    num_init_threads: NonZeroUsize::new(1),
                    pipeline_cache: None,
                },
            ) {
                Ok(r) => Some(r),
                Err(err) => {
                    tracing::warn!("Vello GPU init failed ({err}); falling back to vello_cpu");
                    None
                }
            }
        } else {
            None
        };

        let (cpu_fallback, cpu_upload_texture, cpu_upload_view) = if vello_renderer.is_none() {
            let (tex, view) = create_cpu_upload_target(&device, w, h);
            (Some(VelloCpuBackend::new(w, h)), Some(tex), Some(view))
        } else {
            (None, None, None)
        };

        Ok(Self {
            instance,
            adapter,
            device,
            queue,
            surface,
            config,
            blitter,
            target_texture,
            target_view,
            vello_renderer,
            cpu_fallback,
            cpu_upload_texture,
            cpu_upload_view,
            scene: Scene::new(),
            image_cache: HashMap::new(),
        })
    }

    pub fn is_using_gpu(&self) -> bool {
        self.vello_renderer.is_some()
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        let w = width.max(1);
        let h = height.max(1);
        if self.config.width == w && self.config.height == h {
            return;
        }
        self.config.width = w;
        self.config.height = h;
        self.surface.configure(&self.device, &self.config);
        let (tex, view) = create_vello_target(&self.device, w, h);
        self.target_texture = tex;
        self.target_view = view;
        if self.cpu_fallback.is_some() {
            let (tex, view) = create_cpu_upload_target(&self.device, w, h);
            self.cpu_upload_texture = Some(tex);
            self.cpu_upload_view = Some(view);
        }
    }

    pub fn render_frame(
        &mut self,
        paint_scene: &PaintScene,
        scale_factor: f32,
        background: Color,
        textures: &TextureRegistry,
        text_engine: &mut TextEngine,
    ) -> Result<()> {
        let w = self.config.width;
        let h = self.config.height;

        let frame = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(f)
            | wgpu::CurrentSurfaceTexture::Suboptimal(f) => f,
            wgpu::CurrentSurfaceTexture::Lost | wgpu::CurrentSurfaceTexture::Outdated => {
                self.surface.configure(&self.device, &self.config);
                match self.surface.get_current_texture() {
                    wgpu::CurrentSurfaceTexture::Success(f)
                    | wgpu::CurrentSurfaceTexture::Suboptimal(f) => f,
                    wgpu::CurrentSurfaceTexture::Timeout
                    | wgpu::CurrentSurfaceTexture::Occluded => {
                        return Ok(());
                    }
                    other => {
                        return Err(CraftError::Render(format!(
                            "surface acquire failed: {other:?}"
                        )));
                    }
                }
            }
            wgpu::CurrentSurfaceTexture::Timeout | wgpu::CurrentSurfaceTexture::Occluded => {
                return Ok(());
            }
            other => {
                return Err(CraftError::Render(format!("surface error: {other:?}")));
            }
        };

        let surface_view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());

        let blit_source_view: &wgpu::TextureView =
            if let Some(ref mut vello_renderer) = self.vello_renderer {
                encode_vello_scene(
                    &mut self.scene,
                    paint_scene,
                    scale_factor,
                    textures,
                    text_engine,
                    &mut self.image_cache,
                );
                vello_renderer
                    .render_to_texture(
                        &self.device,
                        &self.queue,
                        &self.scene,
                        &self.target_view,
                        &RenderParams {
                            base_color: background.to_peniko(),
                            width: w,
                            height: h,
                            antialiasing_method: AaConfig::Area,
                        },
                    )
                    .map_err(|e| CraftError::Render(format!("vello render failed: {e}")))?;
                &self.target_view
            } else if let (Some(cpu), Some(upload_tex), Some(upload_view)) = (
                self.cpu_fallback.as_mut(),
                self.cpu_upload_texture.as_ref(),
                self.cpu_upload_view.as_ref(),
            ) {
                let straight = cpu.render_straight_rgba(
                    paint_scene,
                    w,
                    h,
                    scale_factor,
                    background,
                    textures,
                    text_engine,
                );
                self.queue.write_texture(
                    wgpu::TexelCopyTextureInfo {
                        texture: upload_tex,
                        mip_level: 0,
                        origin: wgpu::Origin3d::ZERO,
                        aspect: wgpu::TextureAspect::All,
                    },
                    &straight,
                    wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(4 * w),
                        rows_per_image: Some(h),
                    },
                    wgpu::Extent3d {
                        width: w,
                        height: h,
                        depth_or_array_layers: 1,
                    },
                );
                upload_view
            } else {
                return Err(CraftError::Render("no active render backend".into()));
            };

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("craftui-blit-encoder"),
            });
        self.blitter
            .copy(&self.device, &mut encoder, blit_source_view, &surface_view);
        self.queue.submit(Some(encoder.finish()));
        self.queue.present(frame);
        Ok(())
    }
}

fn create_vello_target(
    device: &wgpu::Device,
    width: u32,
    height: u32,
) -> (wgpu::Texture, wgpu::TextureView) {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("craftui-vello-target"),
        size: wgpu::Extent3d {
            width: width.max(1),
            height: height.max(1),
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    });
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    (texture, view)
}

fn create_cpu_upload_target(
    device: &wgpu::Device,
    width: u32,
    height: u32,
) -> (wgpu::Texture, wgpu::TextureView) {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("craftui-cpu-upload"),
        size: wgpu::Extent3d {
            width: width.max(1),
            height: height.max(1),
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::COPY_DST | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    });
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    (texture, view)
}

fn encode_vello_scene(
    scene: &mut Scene,
    paint_scene: &PaintScene,
    scale_factor: f32,
    textures: &TextureRegistry,
    text_engine: &mut TextEngine,
    image_cache: &mut HashMap<TextureId, CachedGpuImage>,
) {
    scene.reset();
    let base = Affine::scale(scale_factor as f64);
    let mut layer_depth = 0usize;

    for cmd in &paint_scene.commands {
        match cmd {
            PaintCommand::FillRect {
                rect,
                brush,
                radius,
            } => {
                let p_brush = brush_to_peniko(brush);
                if radius.is_zero() {
                    scene.fill(
                        peniko::Fill::NonZero,
                        base,
                        &p_brush,
                        None,
                        &rect.to_kurbo(),
                    );
                } else {
                    scene.fill(
                        peniko::Fill::NonZero,
                        base,
                        &p_brush,
                        None,
                        &rect.to_rounded_kurbo(*radius),
                    );
                }
            }
            PaintCommand::StrokeRect {
                rect,
                color,
                width,
                radius,
            } => {
                let stroke = Stroke::new(*width as f64);
                if radius.is_zero() {
                    scene.stroke(&stroke, base, color.to_peniko(), None, &rect.to_kurbo());
                } else {
                    scene.stroke(
                        &stroke,
                        base,
                        color.to_peniko(),
                        None,
                        &rect.to_rounded_kurbo(*radius),
                    );
                }
            }
            PaintCommand::DropShadow {
                rect,
                radius,
                std_dev,
                offset_x,
                offset_y,
                color,
            } => {
                let shifted = rect.translate(*offset_x, *offset_y).to_kurbo();
                scene.draw_blurred_rounded_rect(
                    base,
                    shifted,
                    color.to_peniko(),
                    *radius as f64,
                    *std_dev as f64,
                );
            }
            PaintCommand::FillPath { path, brush } => {
                let p_brush = brush_to_peniko(brush);
                scene.fill(peniko::Fill::NonZero, base, &p_brush, None, path);
            }
            PaintCommand::StrokePath { path, color, width } => {
                scene.stroke(
                    &Stroke::new(*width as f64),
                    base,
                    color.to_peniko(),
                    None,
                    path,
                );
            }
            PaintCommand::DrawImage {
                rect,
                texture,
                fit,
                radius,
                opacity,
            } => {
                if let Some(handle) = textures.get(*texture) {
                    let ver = handle.version();
                    let need_refresh = image_cache.get(texture).is_none_or(|c| c.version != ver);
                    let (w, h) = if need_refresh {
                        let (w, h, ver, rgba) = handle.snapshot();
                        if w == 0 || h == 0 {
                            continue;
                        }
                        let brush = peniko::ImageBrush::new(peniko::ImageData {
                            data: peniko::Blob::from(rgba),
                            format: peniko::ImageFormat::Rgba8,
                            alpha_type: peniko::ImageAlphaType::Alpha,
                            width: w,
                            height: h,
                        });
                        image_cache.insert(
                            *texture,
                            CachedGpuImage {
                                version: ver,
                                brush,
                            },
                        );
                        (w, h)
                    } else {
                        handle.dimensions()
                    };
                    if let Some(cached) = image_cache.get(texture) {
                        let (_, img_aff) = compute_image_transform(*rect, w, h, *fit);
                        scene.push_clip_layer(
                            peniko::Fill::NonZero,
                            base,
                            &rect.to_rounded_kurbo(*radius),
                        );
                        let brush = cached.brush.clone().with_alpha(*opacity);
                        scene.draw_image(&brush, base * img_aff);
                        scene.pop_layer();
                    }
                }
            }
            PaintCommand::DrawText {
                origin,
                text,
                style,
                max_width,
            } => {
                if text.is_empty() {
                    continue;
                }
                let layout = text_engine.build_layout(text, style, *max_width);
                let text_xform = base * Affine::translate((origin.x as f64, origin.y as f64));
                for line in layout.lines() {
                    for item in line.items() {
                        if let PositionedLayoutItem::GlyphRun(glyph_run) = item {
                            let run = glyph_run.run();
                            let font = run.font();
                            let color = glyph_run.style().brush;
                            let coords: Vec<i16> = run
                                .normalized_coords()
                                .iter()
                                .map(|c| c.to_bits())
                                .collect();
                            let glyphs = glyph_run.positioned_glyphs().map(|g| vello::Glyph {
                                id: g.id,
                                x: g.x,
                                y: g.y,
                            });
                            scene
                                .draw_glyphs(font)
                                .font_size(run.font_size())
                                .hint(true)
                                .transform(text_xform)
                                .normalized_coords(&coords)
                                .brush(color.to_peniko())
                                .draw(peniko::Fill::NonZero, glyphs);
                        }
                    }
                }
            }
            PaintCommand::PushClip { rect, radius } => {
                if radius.is_zero() {
                    scene.push_clip_layer(peniko::Fill::NonZero, base, &rect.to_kurbo());
                } else {
                    scene.push_clip_layer(
                        peniko::Fill::NonZero,
                        base,
                        &rect.to_rounded_kurbo(*radius),
                    );
                }
                layer_depth += 1;
            }
            PaintCommand::PopClip => {
                if layer_depth > 0 {
                    scene.pop_layer();
                    layer_depth -= 1;
                }
            }
            PaintCommand::PushLayer { opacity } => {
                let huge = kurbo::Rect::new(-1e5, -1e5, 1e5, 1e5);
                scene.push_layer(
                    peniko::Fill::NonZero,
                    peniko::BlendMode::default(),
                    *opacity,
                    base,
                    &huge,
                );
                layer_depth += 1;
            }
            PaintCommand::PopLayer => {
                if layer_depth > 0 {
                    scene.pop_layer();
                    layer_depth -= 1;
                }
            }
            PaintCommand::PlatformViewHole { rect, .. } => {
                scene.fill(
                    peniko::Fill::NonZero,
                    base,
                    Color::TRANSPARENT.to_peniko(),
                    None,
                    &rect.to_kurbo(),
                );
            }
        }
    }

    while layer_depth > 0 {
        scene.pop_layer();
        layer_depth -= 1;
    }
}
