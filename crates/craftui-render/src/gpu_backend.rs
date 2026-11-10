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
    target_texture: Option<wgpu::Texture>,
    target_view: Option<wgpu::TextureView>,
    vello_renderer: Option<Renderer>,
    cpu_fallback: Option<VelloCpuBackend>,
    cpu_upload_texture: Option<wgpu::Texture>,
    cpu_upload_view: Option<wgpu::TextureView>,
    scene: Scene,
    image_cache: HashMap<TextureId, CachedGpuImage>,
}

fn default_wgpu_backends() -> wgpu::Backends {
    #[cfg(target_arch = "wasm32")]
    {
        wgpu::Backends::BROWSER_WEBGPU | wgpu::Backends::GL
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        wgpu::Backends::from_env().unwrap_or_default()
    }
}

fn default_instance_flags() -> wgpu::InstanceFlags {
    #[cfg(target_arch = "wasm32")]
    {
        wgpu::InstanceFlags::default()
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        wgpu::InstanceFlags::from_env_or_default()
    }
}

#[derive(Clone, Copy)]
struct AdapterAttempt {
    backends: wgpu::Backends,
    flags: wgpu::InstanceFlags,
    force_fallback_adapter: bool,
}

impl GpuSurfacePipeline {
    /// Synchronously initialize `wgpu` surface + `vello::Renderer` (native platforms only).
    ///
    /// On `wasm32`, use [`GpuSurfacePipeline::new_async`].
    pub fn new(
        window: impl Into<wgpu::SurfaceTarget<'static>>,
        width: u32,
        height: u32,
        force_cpu: bool,
    ) -> Result<Self> {
        #[cfg(target_arch = "wasm32")]
        {
            let _ = (window, width, height, force_cpu);
            Err(CraftError::Render(
                "GpuSurfacePipeline::new is unsupported on wasm32; use GpuSurfacePipeline::new_async"
                    .into(),
            ))
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            pollster::block_on(Self::new_async(window, width, height, force_cpu))
        }
    }

    /// Synchronously initialize `wgpu` surface + `vello::Renderer` with an explicit compositor display handle
    /// (required by OpenGL/EGL on Linux Wayland/X11) and automatic backend fallback retries.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn new_with_display<T>(
        target_factory: impl Fn() -> T,
        display_factory: impl Fn() -> Box<dyn wgpu::wgt::WgpuHasDisplayHandle>,
        width: u32,
        height: u32,
        force_cpu: bool,
    ) -> Result<Self>
    where
        T: Into<wgpu::SurfaceTarget<'static>>,
    {
        pollster::block_on(Self::new_with_display_async(
            target_factory,
            display_factory,
            width,
            height,
            force_cpu,
        ))
    }

    /// Asynchronously initialize `wgpu` surface + `vello::Renderer` with an explicit display handle
    /// and automatic adapter fallback (default -> non-compliant Vulkan -> OpenGL/EGL -> software fallback).
    pub async fn new_with_display_async<T>(
        target_factory: impl Fn() -> T,
        display_factory: impl Fn() -> Box<dyn wgpu::wgt::WgpuHasDisplayHandle>,
        width: u32,
        height: u32,
        force_cpu: bool,
    ) -> Result<Self>
    where
        T: Into<wgpu::SurfaceTarget<'static>>,
    {
        let base_backends = default_wgpu_backends();
        let base_flags = default_instance_flags();
        let env_pinned_backend = {
            #[cfg(not(target_arch = "wasm32"))]
            {
                wgpu::Backends::from_env().is_some()
            }
            #[cfg(target_arch = "wasm32")]
            {
                false
            }
        };

        let attempts = if !env_pinned_backend && cfg!(not(target_arch = "wasm32")) {
            vec![
                AdapterAttempt {
                    backends: wgpu::Backends::PRIMARY,
                    flags: base_flags,
                    force_fallback_adapter: false,
                },
                AdapterAttempt {
                    backends: wgpu::Backends::PRIMARY,
                    flags: base_flags | wgpu::InstanceFlags::ALLOW_UNDERLYING_NONCOMPLIANT_ADAPTER,
                    force_fallback_adapter: false,
                },
                AdapterAttempt {
                    backends: wgpu::Backends::GL,
                    flags: base_flags,
                    force_fallback_adapter: false,
                },
                AdapterAttempt {
                    backends: base_backends,
                    flags: base_flags | wgpu::InstanceFlags::ALLOW_UNDERLYING_NONCOMPLIANT_ADAPTER,
                    force_fallback_adapter: true,
                },
            ]
        } else {
            vec![
                AdapterAttempt {
                    backends: base_backends,
                    flags: base_flags,
                    force_fallback_adapter: false,
                },
                AdapterAttempt {
                    backends: base_backends,
                    flags: base_flags | wgpu::InstanceFlags::ALLOW_UNDERLYING_NONCOMPLIANT_ADAPTER,
                    force_fallback_adapter: false,
                },
                AdapterAttempt {
                    backends: base_backends,
                    flags: base_flags | wgpu::InstanceFlags::ALLOW_UNDERLYING_NONCOMPLIANT_ADAPTER,
                    force_fallback_adapter: true,
                },
            ]
        };

        let mut errors = Vec::new();
        for attempt in attempts {
            let mut desc = wgpu::InstanceDescriptor::new_with_display_handle(display_factory());
            desc.backends = attempt.backends;
            desc.flags = attempt.flags;
            #[cfg(not(target_arch = "wasm32"))]
            {
                desc.backend_options = desc.backend_options.with_env();
            }

            let instance = wgpu::Instance::new(desc);
            let surface = match instance.create_surface(target_factory()) {
                Ok(s) => s,
                Err(e) => {
                    errors.push(format!("{:?} create_surface: {e}", attempt.backends));
                    continue;
                }
            };

            match Self::from_instance_and_surface_with_fallback(
                instance,
                surface,
                width,
                height,
                force_cpu,
                attempt.force_fallback_adapter,
            )
            .await
            {
                Ok(pipeline) => return Ok(pipeline),
                Err(e) => {
                    errors.push(format!(
                        "{:?} (fallback={}): {e}",
                        attempt.backends, attempt.force_fallback_adapter
                    ));
                }
            }
        }

        Err(CraftError::Render(format!(
            "all wgpu surface/adapter attempts failed: {}",
            errors.join(" | ")
        )))
    }

    /// Asynchronously initialize `wgpu` surface + `vello::Renderer` (works cleanly on WebGPU/WebGL2 WASM and native).
    pub async fn new_async(
        window: impl Into<wgpu::SurfaceTarget<'static>>,
        width: u32,
        height: u32,
        force_cpu: bool,
    ) -> Result<Self> {
        let mut desc = wgpu::InstanceDescriptor::new_without_display_handle_from_env();
        desc.backends = default_wgpu_backends();
        let instance = wgpu::Instance::new(desc);
        let surface = instance
            .create_surface(window)
            .map_err(|e| CraftError::Render(format!("create_surface failed: {e}")))?;
        Self::from_instance_and_surface_async(instance, surface, width, height, force_cpu).await
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
        #[cfg(target_arch = "wasm32")]
        {
            let _ = (target, width, height, force_cpu);
            Err(CraftError::Render(
                "GpuSurfacePipeline::new_unsafe is unsupported on wasm32".into(),
            ))
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            let mut desc = wgpu::InstanceDescriptor::new_without_display_handle_from_env();
            desc.backends = default_wgpu_backends();
            let instance = wgpu::Instance::new(desc);
            // SAFETY: forwarded from `new_unsafe` caller invariant that `target` outlives the surface.
            let surface = unsafe { instance.create_surface_unsafe(target) }
                .map_err(|e| CraftError::Render(format!("create_surface_unsafe failed: {e}")))?;
            pollster::block_on(Self::from_instance_and_surface_async(
                instance, surface, width, height, force_cpu,
            ))
        }
    }

    /// Initialize pipeline asynchronously from a pre-constructed `wgpu::Instance` and `wgpu::Surface`.
    pub async fn from_instance_and_surface_async(
        instance: wgpu::Instance,
        surface: wgpu::Surface<'static>,
        width: u32,
        height: u32,
        force_cpu: bool,
    ) -> Result<Self> {
        Self::from_instance_and_surface_with_fallback(
            instance, surface, width, height, force_cpu, false,
        )
        .await
    }

    async fn from_instance_and_surface_with_fallback(
        instance: wgpu::Instance,
        surface: wgpu::Surface<'static>,
        width: u32,
        height: u32,
        force_cpu: bool,
        force_fallback_adapter: bool,
    ) -> Result<Self> {
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: Some(&surface),
                force_fallback_adapter,
                apply_limit_buckets: false,
            })
            .await
            .map_err(|e| CraftError::Render(format!("no compatible wgpu adapter: {e}")))?;

        let adapter_info = adapter.get_info();
        let supports_compute = adapter_info.backend != wgpu::Backend::Gl
            && adapter.limits().max_compute_workgroups_per_dimension > 0
            && adapter.limits().max_storage_buffers_per_shader_stage >= 4;

        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("craftui-wgpu-device"),
                required_features: wgpu::Features::empty(),
                required_limits: adapter.limits(),
                memory_hints: wgpu::MemoryHints::default(),
                experimental_features: wgpu::ExperimentalFeatures::disabled(),
                trace: wgpu::Trace::Off,
            })
            .await
            .map_err(|e| CraftError::Render(format!("request_device failed: {e}")))?;

        let caps = surface.get_capabilities(&adapter);
        let default_fmt = *caps.formats.first().ok_or_else(|| {
            CraftError::Render("wgpu surface reports no supported pixel formats".into())
        })?;
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
            .unwrap_or(default_fmt);

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

        let (target_texture, target_view) = if vello_renderer.is_some() {
            let (tex, view) = create_vello_target(&device, w, h);
            (Some(tex), Some(view))
        } else {
            (None, None)
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
        if self.vello_renderer.is_some() {
            let (tex, view) = create_vello_target(&self.device, w, h);
            self.target_texture = Some(tex);
            self.target_view = Some(view);
        }
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
            if let (Some(vello_renderer), Some(target_view)) =
                (self.vello_renderer.as_mut(), self.target_view.as_ref())
            {
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
                        target_view,
                        &RenderParams {
                            base_color: background.to_peniko(),
                            width: w,
                            height: h,
                            antialiasing_method: AaConfig::Area,
                        },
                    )
                    .map_err(|e| CraftError::Render(format!("vello render failed: {e}")))?;
                target_view
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
                // Punch a clear alpha hole through composited background layers so underlying OS PlatformView shows cleanly.
                let r = rect.to_kurbo();
                scene.push_layer(
                    peniko::Fill::NonZero,
                    peniko::BlendMode::from(peniko::Compose::Clear),
                    1.0,
                    base,
                    &r,
                );
                scene.fill(
                    peniko::Fill::NonZero,
                    base,
                    Color::BLACK.to_peniko(),
                    None,
                    &r,
                );
                scene.pop_layer();
            }
        }
    }

    while layer_depth > 0 {
        scene.pop_layer();
        layer_depth -= 1;
    }
}
