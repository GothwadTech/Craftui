use crate::scene::{PaintCommand, PaintScene, brush_to_peniko, compute_image_transform};
use crate::texture::TextureRegistry;
use craftui_core::{Brush, Color, CornerRadii, Rect, TextureId};
use craftui_text::TextEngine;
use kurbo::{Affine, BezPath, Shape, Stroke};
use parley::PositionedLayoutItem;
use std::collections::HashMap;
use std::sync::Arc;
use vello_cpu::{
    Image as VelloCpuImage, ImageSource, Level, PaintType, Pixmap, RenderContext, RenderSettings,
    Resources,
};

struct CachedCpuPixmap {
    version: u64,
    pixmap: Arc<Pixmap>,
}

/// CPU software vector renderer backed by `vello_cpu`.
///
/// Automatically used when GPU compute shaders are unavailable or in headless tests.
pub struct VelloCpuBackend {
    ctx: RenderContext,
    resources: Resources,
    target: Pixmap,
    cached_images: HashMap<TextureId, CachedCpuPixmap>,
}

impl Default for VelloCpuBackend {
    fn default() -> Self {
        Self::new(1, 1)
    }
}

impl VelloCpuBackend {
    pub fn new(width: u32, height: u32) -> Self {
        let w = width.clamp(1, u16::MAX as u32) as u16;
        let h = height.clamp(1, u16::MAX as u32) as u16;
        let settings = RenderSettings {
            level: Level::new(),
            num_threads: 0,
        };
        Self {
            ctx: RenderContext::new_with(w, h, settings),
            resources: Resources::new(),
            target: Pixmap::new(w, h),
            cached_images: HashMap::new(),
        }
    }

    fn ensure_size(&mut self, width: u32, height: u32) -> (u16, u16) {
        let w = width.clamp(1, u16::MAX as u32) as u16;
        let h = height.clamp(1, u16::MAX as u32) as u16;
        if self.ctx.width() != w || self.ctx.height() != h {
            self.ctx.reset_and_resize(w, h);
            self.target = Pixmap::new(w, h);
        } else {
            self.ctx.reset();
        }
        (w, h)
    }

    fn get_or_upload_pixmap(
        &mut self,
        id: TextureId,
        textures: &TextureRegistry,
    ) -> Option<Arc<Pixmap>> {
        let handle = textures.get(id)?;
        let ver = handle.version();
        if let Some(entry) = self.cached_images.get(&id)
            && entry.version == ver
        {
            return Some(entry.pixmap.clone());
        }
        let (w, h, ver, rgba) = handle.snapshot();
        if w == 0 || h == 0 {
            return None;
        }
        let pw = w.min(u16::MAX as u32) as u16;
        let ph = h.min(u16::MAX as u32) as u16;
        let mut pixmap = Pixmap::new(pw, ph);
        let dst = pixmap.data_as_u8_slice_mut();
        let copy_len = dst.len().min(rgba.len());
        // Convert straight RGBA8 to premultiplied RGBA8 for vello_cpu
        let (dst_chunks, _) = dst[..copy_len].as_chunks_mut::<4>();
        let (src_chunks, _) = rgba[..copy_len].as_chunks::<4>();
        for (chunk_dst, chunk_src) in dst_chunks.iter_mut().zip(src_chunks.iter()) {
            let a = chunk_src[3] as u16;
            chunk_dst[0] = ((chunk_src[0] as u16 * a) / 255) as u8;
            chunk_dst[1] = ((chunk_src[1] as u16 * a) / 255) as u8;
            chunk_dst[2] = ((chunk_src[2] as u16 * a) / 255) as u8;
            chunk_dst[3] = chunk_src[3];
        }
        pixmap.recompute_may_have_transparency();
        let arc = Arc::new(pixmap);
        self.cached_images.insert(
            id,
            CachedCpuPixmap {
                version: ver,
                pixmap: arc.clone(),
            },
        );
        Some(arc)
    }

    /// Rasterize `scene` into premultiplied RGBA8 pixels (also returning straight RGBA8 if desired).
    #[allow(clippy::too_many_arguments)]
    pub fn render_premul(
        &mut self,
        scene: &PaintScene,
        width: u32,
        height: u32,
        scale_factor: f32,
        background: Color,
        textures: &TextureRegistry,
        text_engine: &mut TextEngine,
    ) -> &[u8] {
        let (w, h) = self.ensure_size(width, height);
        let base_transform = Affine::scale(scale_factor as f64);

        self.ctx.set_transform(Affine::IDENTITY);
        self.ctx.set_paint(background.to_peniko());
        self.ctx
            .fill_rect(&kurbo::Rect::new(0.0, 0.0, w as f64, h as f64));

        self.ctx.set_transform(base_transform);

        let mut clip_stack_depth = 0usize;
        let mut layer_stack_depth = 0usize;

        for cmd in &scene.commands {
            match cmd {
                PaintCommand::FillRect {
                    rect,
                    brush,
                    radius,
                } => {
                    self.ctx.set_transform(base_transform);
                    self.ctx.reset_paint_transform();
                    apply_paint(&mut self.ctx, brush);
                    if radius.is_zero() {
                        self.ctx.fill_rect(&rect.to_kurbo());
                    } else {
                        let path = rect.to_rounded_kurbo(*radius).to_path(0.1);
                        self.ctx.fill_path(&path);
                    }
                }
                PaintCommand::StrokeRect {
                    rect,
                    color,
                    width,
                    radius,
                } => {
                    self.ctx.set_transform(base_transform);
                    self.ctx.reset_paint_transform();
                    self.ctx.set_paint(color.to_peniko());
                    self.ctx.set_stroke(Stroke::new(*width as f64));
                    if radius.is_zero() {
                        self.ctx.stroke_rect(&rect.to_kurbo());
                    } else {
                        let path = rect.to_rounded_kurbo(*radius).to_path(0.1);
                        self.ctx.stroke_path(&path);
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
                    self.ctx.set_transform(base_transform);
                    self.ctx.reset_paint_transform();
                    self.ctx.set_paint(color.to_peniko());
                    let shifted = rect.translate(*offset_x, *offset_y).to_kurbo();
                    self.ctx
                        .fill_blurred_rounded_rect(&shifted, *radius, *std_dev, false);
                }
                PaintCommand::FillPath { path, brush } => {
                    self.ctx.set_transform(base_transform);
                    self.ctx.reset_paint_transform();
                    apply_paint(&mut self.ctx, brush);
                    self.ctx.fill_path(path);
                }
                PaintCommand::StrokePath { path, color, width } => {
                    self.ctx.set_transform(base_transform);
                    self.ctx.reset_paint_transform();
                    self.ctx.set_paint(color.to_peniko());
                    self.ctx.set_stroke(Stroke::new(*width as f64));
                    self.ctx.stroke_path(path);
                }
                PaintCommand::DrawImage {
                    rect,
                    texture,
                    fit,
                    radius,
                    opacity,
                } => {
                    if let Some(pixmap) = self.get_or_upload_pixmap(*texture, textures) {
                        let img_w = pixmap.width() as u32;
                        let img_h = pixmap.height() as u32;
                        let (_, img_affine) = compute_image_transform(*rect, img_w, img_h, *fit);
                        let clip_path = rect.to_rounded_kurbo(*radius).to_path(0.1);
                        self.ctx.set_transform(base_transform);
                        self.ctx.push_clip_layer(&clip_path);
                        self.ctx.set_paint_transform(img_affine);
                        let img_paint = VelloCpuImage {
                            image: ImageSource::Pixmap(pixmap),
                            sampler: peniko::ImageSampler::default().with_alpha(*opacity),
                        };
                        self.ctx.set_paint(img_paint);
                        self.ctx.fill_rect(&rect.to_kurbo());
                        self.ctx.reset_paint_transform();
                        self.ctx.pop_layer();
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
                    let layout = text_engine.build_layout_cached(text, style, *max_width);
                    let text_xform =
                        base_transform * Affine::translate((origin.x as f64, origin.y as f64));
                    self.ctx.set_transform(text_xform);
                    self.ctx.reset_paint_transform();
                    self.ctx.set_paint(style.color.to_peniko());
                    let mut coords_buf: Vec<i16> = Vec::new();
                    for line in layout.lines() {
                        for item in line.items() {
                            if let PositionedLayoutItem::GlyphRun(glyph_run) = item {
                                let run = glyph_run.run();
                                let font = run.font();
                                let font_size = run.font_size();
                                let norm = run.normalized_coords();
                                let coords_slice: &[i16] = if norm.is_empty() {
                                    &[]
                                } else {
                                    coords_buf.clear();
                                    coords_buf.extend(norm.iter().map(|c| c.to_bits()));
                                    &coords_buf
                                };
                                let glyphs =
                                    glyph_run.positioned_glyphs().map(|g| vello_cpu::Glyph {
                                        id: g.id,
                                        x: g.x,
                                        y: g.y,
                                    });
                                let _ = self
                                    .ctx
                                    .glyph_run(&mut self.resources, font)
                                    .font_size(font_size)
                                    .hint(true)
                                    .normalized_coords(coords_slice)
                                    .fill_glyphs(glyphs);
                            }
                        }
                    }
                    self.ctx.set_transform(base_transform);
                }
                PaintCommand::PushClip { rect, radius } => {
                    self.ctx.set_transform(base_transform);
                    if radius.is_zero() {
                        self.ctx.push_clip_rect(&rect.to_kurbo());
                        clip_stack_depth += 1;
                    } else {
                        let path = rect.to_rounded_kurbo(*radius).to_path(0.1);
                        self.ctx.push_clip_layer(&path);
                        layer_stack_depth += 1;
                    }
                }
                PaintCommand::PopClip => {
                    if clip_stack_depth > 0 {
                        self.ctx.pop_clip();
                        clip_stack_depth -= 1;
                    } else if layer_stack_depth > 0 {
                        self.ctx.pop_layer();
                        layer_stack_depth -= 1;
                    }
                }
                PaintCommand::PushLayer { opacity } => {
                    self.ctx.set_transform(base_transform);
                    self.ctx.push_opacity_layer(*opacity);
                    layer_stack_depth += 1;
                }
                PaintCommand::PopLayer => {
                    if layer_stack_depth > 0 {
                        self.ctx.pop_layer();
                        layer_stack_depth -= 1;
                    }
                }
                PaintCommand::PlatformViewHole { rect, .. } => {
                    // Punch a transparent cutout when compositing over an OS-native view
                    let r = rect.to_kurbo();
                    let clip_path = r.to_path(0.1);
                    self.ctx.set_transform(base_transform);
                    self.ctx.push_layer(
                        Some(&clip_path),
                        Some(vello_cpu::peniko::BlendMode::from(
                            vello_cpu::peniko::Compose::Clear,
                        )),
                        None,
                        None,
                        None,
                    );
                    self.ctx.set_paint(Color::BLACK.to_peniko());
                    self.ctx.fill_rect(&r);
                    self.ctx.pop_layer();
                }
            }
        }

        while clip_stack_depth > 0 {
            self.ctx.pop_clip();
            clip_stack_depth -= 1;
        }
        while layer_stack_depth > 0 {
            self.ctx.pop_layer();
            layer_stack_depth -= 1;
        }

        self.ctx.flush();
        self.ctx.render(&mut self.target, &mut self.resources);
        self.target.data_as_u8_slice()
    }

    /// Render scene and unpremultiply into straight RGBA8 bytes.
    #[allow(clippy::too_many_arguments)]
    pub fn render_straight_rgba(
        &mut self,
        scene: &PaintScene,
        width: u32,
        height: u32,
        scale_factor: f32,
        background: Color,
        textures: &TextureRegistry,
        text_engine: &mut TextEngine,
    ) -> Vec<u8> {
        let premul = self.render_premul(
            scene,
            width,
            height,
            scale_factor,
            background,
            textures,
            text_engine,
        );
        let mut out = premul.to_vec();
        for px in out.as_chunks_mut::<4>().0 {
            let a = px[3];
            if a > 0 && a < 255 {
                let af = 255.0 / a as f32;
                px[0] = ((px[0] as f32 * af).round() as u16).min(255) as u8;
                px[1] = ((px[1] as f32 * af).round() as u16).min(255) as u8;
                px[2] = ((px[2] as f32 * af).round() as u16).min(255) as u8;
            }
        }
        out
    }
}

fn apply_paint(ctx: &mut RenderContext, brush: &Brush) {
    match brush_to_peniko(brush) {
        peniko::Brush::Solid(c) => ctx.set_paint(PaintType::Solid(c)),
        peniko::Brush::Gradient(g) => ctx.set_paint(PaintType::Gradient(g)),
        peniko::Brush::Image(_) => {}
    }
}

/// Ultra-lightweight `tiny-skia` software fallback renderer (for minimal targets or fallback tests).
pub struct TinySkiaBackend;

impl TinySkiaBackend {
    pub fn render_straight_rgba(
        scene: &PaintScene,
        width: u32,
        height: u32,
        scale_factor: f32,
        background: Color,
        textures: &TextureRegistry,
    ) -> Vec<u8> {
        let w = width.max(1);
        let h = height.max(1);
        let mut pixmap = tiny_skia::Pixmap::new(w, h).expect("tiny-skia pixmap");
        pixmap.fill(tiny_skia::Color::from_rgba8(
            background.r,
            background.g,
            background.b,
            background.a,
        ));
        let transform = tiny_skia::Transform::from_scale(scale_factor, scale_factor);

        for cmd in &scene.commands {
            match cmd {
                PaintCommand::FillRect {
                    rect,
                    brush,
                    radius,
                } => {
                    let color = match brush {
                        Brush::Solid(c) => *c,
                        Brush::LinearGradient { stops, .. }
                        | Brush::RadialGradient { stops, .. } => {
                            stops.first().map(|s| s.color).unwrap_or(Color::WHITE)
                        }
                    };
                    if let Some(path) = rounded_rect_to_tiny_path(*rect, *radius) {
                        let mut paint = tiny_skia::Paint::default();
                        paint.set_color_rgba8(color.r, color.g, color.b, color.a);
                        paint.anti_alias = true;
                        pixmap.fill_path(
                            &path,
                            &paint,
                            tiny_skia::FillRule::Winding,
                            transform,
                            None,
                        );
                    }
                }
                PaintCommand::StrokeRect {
                    rect,
                    color,
                    width: stroke_w,
                    radius,
                } => {
                    if let Some(path) = rounded_rect_to_tiny_path(*rect, *radius) {
                        let mut paint = tiny_skia::Paint::default();
                        paint.set_color_rgba8(color.r, color.g, color.b, color.a);
                        paint.anti_alias = true;
                        let stroke = tiny_skia::Stroke {
                            width: *stroke_w,
                            ..Default::default()
                        };
                        pixmap.stroke_path(&path, &paint, &stroke, transform, None);
                    }
                }
                PaintCommand::FillPath { path, brush } => {
                    if let Some(ts_path) = kurbo_to_tiny_path(path) {
                        let color = match brush {
                            Brush::Solid(c) => *c,
                            Brush::LinearGradient { stops, .. }
                            | Brush::RadialGradient { stops, .. } => {
                                stops.first().map(|s| s.color).unwrap_or(Color::WHITE)
                            }
                        };
                        let mut paint = tiny_skia::Paint::default();
                        paint.set_color_rgba8(color.r, color.g, color.b, color.a);
                        paint.anti_alias = true;
                        pixmap.fill_path(
                            &ts_path,
                            &paint,
                            tiny_skia::FillRule::Winding,
                            transform,
                            None,
                        );
                    }
                }
                PaintCommand::StrokePath {
                    path,
                    color,
                    width: stroke_w,
                } => {
                    if let Some(ts_path) = kurbo_to_tiny_path(path) {
                        let mut paint = tiny_skia::Paint::default();
                        paint.set_color_rgba8(color.r, color.g, color.b, color.a);
                        paint.anti_alias = true;
                        let stroke = tiny_skia::Stroke {
                            width: *stroke_w,
                            ..Default::default()
                        };
                        pixmap.stroke_path(&ts_path, &paint, &stroke, transform, None);
                    }
                }
                PaintCommand::DrawImage {
                    rect, texture, fit, ..
                } => {
                    if let Some(handle) = textures.get(*texture) {
                        let (iw, ih, _, rgba) = handle.snapshot();
                        if iw > 0 && ih > 0 {
                            let mut premul = rgba;
                            for px in premul.as_chunks_mut::<4>().0 {
                                let a = px[3] as u16;
                                px[0] = ((px[0] as u16 * a) / 255) as u8;
                                px[1] = ((px[1] as u16 * a) / 255) as u8;
                                px[2] = ((px[2] as u16 * a) / 255) as u8;
                            }
                            if let Some(size) = tiny_skia::IntSize::from_wh(iw, ih)
                                && let Some(src) = tiny_skia::Pixmap::from_vec(premul, size)
                            {
                                let (_, aff) = compute_image_transform(*rect, iw, ih, *fit);
                                let c = aff.as_coeffs();
                                let img_xform = tiny_skia::Transform::from_row(
                                    c[0] as f32 * scale_factor,
                                    c[1] as f32 * scale_factor,
                                    c[2] as f32 * scale_factor,
                                    c[3] as f32 * scale_factor,
                                    c[4] as f32 * scale_factor,
                                    c[5] as f32 * scale_factor,
                                );
                                pixmap.draw_pixmap(
                                    0,
                                    0,
                                    src.as_ref(),
                                    &tiny_skia::PixmapPaint::default(),
                                    img_xform,
                                    None,
                                );
                            }
                        }
                    }
                }
                PaintCommand::PlatformViewHole { rect, .. } => {
                    if let Some(path) = rounded_rect_to_tiny_path(*rect, CornerRadii::ZERO) {
                        let mut paint = tiny_skia::Paint::default();
                        paint.set_color_rgba8(0, 0, 0, 255);
                        paint.blend_mode = tiny_skia::BlendMode::Clear;
                        pixmap.fill_path(
                            &path,
                            &paint,
                            tiny_skia::FillRule::Winding,
                            transform,
                            None,
                        );
                    }
                }
                _ => {}
            }
        }

        pixmap.take()
    }
}

fn rounded_rect_to_tiny_path(rect: Rect, radius: CornerRadii) -> Option<tiny_skia::Path> {
    if radius.is_zero() {
        let r = tiny_skia::Rect::from_xywh(rect.x, rect.y, rect.width, rect.height)?;
        return Some(tiny_skia::PathBuilder::from_rect(r));
    }
    let bez = rect.to_rounded_kurbo(radius).to_path(0.25);
    kurbo_to_tiny_path(&bez)
}

fn kurbo_to_tiny_path(path: &BezPath) -> Option<tiny_skia::Path> {
    let mut pb = tiny_skia::PathBuilder::new();
    for el in path.elements() {
        match *el {
            kurbo::PathEl::MoveTo(p) => pb.move_to(p.x as f32, p.y as f32),
            kurbo::PathEl::LineTo(p) => pb.line_to(p.x as f32, p.y as f32),
            kurbo::PathEl::QuadTo(p1, p2) => {
                pb.quad_to(p1.x as f32, p1.y as f32, p2.x as f32, p2.y as f32)
            }
            kurbo::PathEl::CurveTo(p1, p2, p3) => pb.cubic_to(
                p1.x as f32,
                p1.y as f32,
                p2.x as f32,
                p2.y as f32,
                p3.x as f32,
                p3.y as f32,
            ),
            kurbo::PathEl::ClosePath => pb.close(),
        }
    }
    pb.finish()
}
