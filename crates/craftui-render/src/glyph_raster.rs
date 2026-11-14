//! CPU outline rasterizer (`skrifa` + `ab_glyph_rasterizer`) and separable box blur helpers
//! shared by the pure-WGSL `gpu-sdf` glyph/path atlas and the `TinySkia` / `VelloCpu` backends.

use ab_glyph_rasterizer::{Point as AbPoint, Rasterizer};
use kurbo::{BezPath, PathEl};
use skrifa::GlyphId;
use skrifa::instance::{LocationRef, NormalizedCoord, Size};
use skrifa::outline::{DrawSettings, OutlinePen};
use skrifa::{FontRef, MetadataProvider};

#[derive(Debug, Clone)]
pub struct RasterizedMask {
    pub width: u32,
    pub height: u32,
    pub offset_x: i32,
    pub offset_y: i32,
    pub coverage: Vec<u8>,
}

#[derive(Clone, Copy)]
enum PathOp {
    MoveTo(f32, f32),
    LineTo(f32, f32),
    QuadTo(f32, f32, f32, f32),
    CubicTo(f32, f32, f32, f32, f32, f32),
    Close,
}

#[derive(Default)]
struct OutlineRecorder {
    ops: Vec<PathOp>,
    min_x: f32,
    min_y: f32,
    max_x: f32,
    max_y: f32,
    has_pts: bool,
}

impl OutlineRecorder {
    fn include(&mut self, x: f32, y: f32) {
        if !x.is_finite() || !y.is_finite() {
            return;
        }
        if !self.has_pts {
            self.min_x = x;
            self.max_x = x;
            self.min_y = y;
            self.max_y = y;
            self.has_pts = true;
        } else {
            self.min_x = self.min_x.min(x);
            self.max_x = self.max_x.max(x);
            self.min_y = self.min_y.min(y);
            self.max_y = self.max_y.max(y);
        }
    }

    fn rasterize_with_subpixel(self, sub_x: f32, sub_y: f32) -> Option<RasterizedMask> {
        if !self.has_pts || self.ops.is_empty() {
            return None;
        }
        let bw = self.max_x - self.min_x;
        let bh = self.max_y - self.min_y;
        if bw <= 0.01 && bh <= 0.01 {
            return None;
        }
        let x0 = (self.min_x + sub_x).floor() as i32 - 1;
        let y0 = (self.min_y + sub_y).floor() as i32 - 1;
        let x1 = (self.max_x + sub_x).ceil() as i32 + 1;
        let y1 = (self.max_y + sub_y).ceil() as i32 + 1;
        let width = (x1 - x0).max(1) as usize;
        let height = (y1 - y0).max(1) as usize;
        if width > 8192 || height > 8192 || width.saturating_mul(height) > 16 * 1024 * 1024 {
            return None;
        }

        let ox = sub_x - x0 as f32;
        let oy = sub_y - y0 as f32;
        let mut rast = Rasterizer::new(width, height);
        let mut start = AbPoint { x: 0.0, y: 0.0 };
        let mut cur = start;
        let mut in_contour = false;

        for op in self.ops {
            match op {
                PathOp::MoveTo(x, y) => {
                    if in_contour && (cur.x != start.x || cur.y != start.y) {
                        rast.draw_line(cur, start);
                    }
                    let p = ab_glyph_rasterizer::point(x + ox, y + oy);
                    start = p;
                    cur = p;
                    in_contour = true;
                }
                PathOp::LineTo(x, y) => {
                    let p = ab_glyph_rasterizer::point(x + ox, y + oy);
                    rast.draw_line(cur, p);
                    cur = p;
                }
                PathOp::QuadTo(cx, cy, x, y) => {
                    let c = ab_glyph_rasterizer::point(cx + ox, cy + oy);
                    let p = ab_glyph_rasterizer::point(x + ox, y + oy);
                    rast.draw_quad(cur, c, p);
                    cur = p;
                }
                PathOp::CubicTo(c0x, c0y, c1x, c1y, x, y) => {
                    let c0 = ab_glyph_rasterizer::point(c0x + ox, c0y + oy);
                    let c1 = ab_glyph_rasterizer::point(c1x + ox, c1y + oy);
                    let p = ab_glyph_rasterizer::point(x + ox, y + oy);
                    rast.draw_cubic(cur, c0, c1, p);
                    cur = p;
                }
                PathOp::Close => {
                    if in_contour && (cur.x != start.x || cur.y != start.y) {
                        rast.draw_line(cur, start);
                    }
                    cur = start;
                    in_contour = false;
                }
            }
        }
        if in_contour && (cur.x != start.x || cur.y != start.y) {
            rast.draw_line(cur, start);
        }

        let mut coverage = vec![0u8; width * height];
        let mut non_zero = false;
        rast.for_each_pixel(|idx, alpha| {
            let v = (alpha.clamp(0.0, 1.0) * 255.0).round() as u8;
            if v > 0 {
                non_zero = true;
            }
            coverage[idx] = v;
        });

        if !non_zero {
            return None;
        }

        Some(RasterizedMask {
            width: width as u32,
            height: height as u32,
            offset_x: x0,
            offset_y: y0,
            coverage,
        })
    }
}

impl OutlinePen for OutlineRecorder {
    fn move_to(&mut self, x: f32, y: f32) {
        let y = -y;
        self.include(x, y);
        self.ops.push(PathOp::MoveTo(x, y));
    }

    fn line_to(&mut self, x: f32, y: f32) {
        let y = -y;
        self.include(x, y);
        self.ops.push(PathOp::LineTo(x, y));
    }

    fn quad_to(&mut self, cx0: f32, cy0: f32, x: f32, y: f32) {
        let cy0 = -cy0;
        let y = -y;
        self.include(cx0, cy0);
        self.include(x, y);
        self.ops.push(PathOp::QuadTo(cx0, cy0, x, y));
    }

    fn curve_to(&mut self, cx0: f32, cy0: f32, cx1: f32, cy1: f32, x: f32, y: f32) {
        let cy0 = -cy0;
        let cy1 = -cy1;
        let y = -y;
        self.include(cx0, cy0);
        self.include(cx1, cy1);
        self.include(x, y);
        self.ops.push(PathOp::CubicTo(cx0, cy0, cx1, cy1, x, y));
    }

    fn close(&mut self) {
        self.ops.push(PathOp::Close);
    }
}

/// Rasterize an outline glyph from `parley::FontData` using `skrifa` + `ab_glyph_rasterizer`.
pub fn rasterize_glyph(
    font_data: &[u8],
    font_index: u32,
    glyph_id: u32,
    font_size_px: f32,
    sub_x: f32,
    sub_y: f32,
    normalized_coords: &[i16],
) -> Option<RasterizedMask> {
    let font = FontRef::from_index(font_data, font_index).ok()?;
    let gid = GlyphId::new(glyph_id);
    let glyph = font.outline_glyphs().get(gid)?;
    let coords: Vec<NormalizedCoord> = normalized_coords
        .iter()
        .map(|&b| NormalizedCoord::from_bits(b))
        .collect();
    let settings = DrawSettings::unhinted(Size::new(font_size_px), LocationRef::new(&coords));
    let mut rec = OutlineRecorder::default();
    if glyph.draw(settings, &mut rec).is_err() {
        return None;
    }
    rec.rasterize_with_subpixel(sub_x, sub_y)
}

/// Rasterize a closed or stroked [`kurbo::BezPath`] at `scale` into an integer-aligned coverage mask.
pub fn rasterize_bez_path(path: &BezPath, scale: f32) -> Option<RasterizedMask> {
    let s = scale.max(0.01);
    let mut rec = OutlineRecorder::default();
    for el in path.elements() {
        match *el {
            PathEl::MoveTo(p) => {
                let x = p.x as f32 * s;
                let y = p.y as f32 * s;
                rec.include(x, y);
                rec.ops.push(PathOp::MoveTo(x, y));
            }
            PathEl::LineTo(p) => {
                let x = p.x as f32 * s;
                let y = p.y as f32 * s;
                rec.include(x, y);
                rec.ops.push(PathOp::LineTo(x, y));
            }
            PathEl::QuadTo(c, p) => {
                let cx = c.x as f32 * s;
                let cy = c.y as f32 * s;
                let x = p.x as f32 * s;
                let y = p.y as f32 * s;
                rec.include(cx, cy);
                rec.include(x, y);
                rec.ops.push(PathOp::QuadTo(cx, cy, x, y));
            }
            PathEl::CurveTo(c0, c1, p) => {
                let c0x = c0.x as f32 * s;
                let c0y = c0.y as f32 * s;
                let c1x = c1.x as f32 * s;
                let c1y = c1.y as f32 * s;
                let x = p.x as f32 * s;
                let y = p.y as f32 * s;
                rec.include(c0x, c0y);
                rec.include(c1x, c1y);
                rec.include(x, y);
                rec.ops.push(PathOp::CubicTo(c0x, c0y, c1x, c1y, x, y));
            }
            PathEl::ClosePath => rec.ops.push(PathOp::Close),
        }
    }
    rec.rasterize_with_subpixel(0.0, 0.0)
}

/// Fast 3-pass separable O(N) RGBA8 box blur (approximating Gaussian blur for CPU backdrop/shadow passes).
pub fn separable_box_blur_rgba(buf: &mut [u8], width: usize, height: usize, radius: usize) {
    if width == 0 || height == 0 || radius == 0 || buf.len() < width * height * 4 {
        return;
    }
    let r = radius.min(64);
    let pass_r = (r / 2).max(1);
    let mut tmp = vec![0u8; width * height * 4];
    for _ in 0..3 {
        box_blur_h(buf, &mut tmp, width, height, pass_r);
        box_blur_v(&tmp, buf, width, height, pass_r);
    }
}

fn box_blur_h(src: &[u8], dst: &mut [u8], width: usize, height: usize, r: usize) {
    let win = (2 * r + 1) as u32;
    let w_max = (width - 1) as isize;
    for y in 0..height {
        let row_off = y * width * 4;
        let mut acc = [0u32; 4];
        for dx in -(r as isize)..=(r as isize) {
            let sx = dx.clamp(0, w_max) as usize;
            let i = row_off + sx * 4;
            acc[0] += src[i] as u32;
            acc[1] += src[i + 1] as u32;
            acc[2] += src[i + 2] as u32;
            acc[3] += src[i + 3] as u32;
        }
        for x in 0..width {
            let out = row_off + x * 4;
            dst[out] = (acc[0] / win) as u8;
            dst[out + 1] = (acc[1] / win) as u8;
            dst[out + 2] = (acc[2] / win) as u8;
            dst[out + 3] = (acc[3] / win) as u8;

            let rem_x = (x as isize - r as isize).clamp(0, w_max) as usize;
            let add_x = (x as isize + r as isize + 1).clamp(0, w_max) as usize;
            let ir = row_off + rem_x * 4;
            let ia = row_off + add_x * 4;
            for c in 0..4 {
                acc[c] = acc[c] + src[ia + c] as u32 - src[ir + c] as u32;
            }
        }
    }
}

fn box_blur_v(src: &[u8], dst: &mut [u8], width: usize, height: usize, r: usize) {
    let win = (2 * r + 1) as u32;
    let h_max = (height - 1) as isize;
    let stride = width * 4;
    for x in 0..width {
        let mut acc = [0u32; 4];
        for dy in -(r as isize)..=(r as isize) {
            let sy = dy.clamp(0, h_max) as usize;
            let i = sy * stride + x * 4;
            acc[0] += src[i] as u32;
            acc[1] += src[i + 1] as u32;
            acc[2] += src[i + 2] as u32;
            acc[3] += src[i + 3] as u32;
        }
        for y in 0..height {
            let out = y * stride + x * 4;
            dst[out] = (acc[0] / win) as u8;
            dst[out + 1] = (acc[1] / win) as u8;
            dst[out + 2] = (acc[2] / win) as u8;
            dst[out + 3] = (acc[3] / win) as u8;

            let rem_y = (y as isize - r as isize).clamp(0, h_max) as usize;
            let add_y = (y as isize + r as isize + 1).clamp(0, h_max) as usize;
            let ir = rem_y * stride + x * 4;
            let ia = add_y * stride + x * 4;
            for c in 0..4 {
                acc[c] = acc[c] + src[ia + c] as u32 - src[ir + c] as u32;
            }
        }
    }
}
