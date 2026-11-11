struct Globals {
    target_size: vec2<f32>,
    scale_factor: f32,
    surface_is_srgb: f32,
};

@group(0) @binding(0) var<uniform> globals: Globals;
@group(0) @binding(1) var atlas_tex: texture_2d<f32>;
@group(0) @binding(2) var linear_sampler: sampler;
@group(1) @binding(0) var image_tex: texture_2d<f32>;

struct VsInput {
    @builtin(vertex_index) vertex_index: u32,
    // Row-major 2x2 [a, c, b, d] so x' = a*x + c*y + tx, y' = b*x + d*y + ty
    @location(0) xform_0: vec4<f32>,
    // [tx, ty, kind (0=rect, 1=shadow, 2=mask, 3=image, 4=hole), unused]
    @location(1) xform_1: vec4<f32>,
    // [x, y, width, height] in local space
    @location(2) rect: vec4<f32>,
    // [top_left, top_right, bottom_right, bottom_left]
    @location(3) radii: vec4<f32>,
    @location(4) color0: vec4<f32>,
    @location(5) color1: vec4<f32>,
    // Gradient: [sx, sy, ex, ey] / [cx, cy, r, 0]; Shadow: [std_dev, spread, ox, oy]
    @location(6) grad_pts: vec4<f32>,
    // [stroke_width, mode, expand, opacity]
    @location(7) params: vec4<f32>,
    // [u0, v0, u1, v1]
    @location(8) uv_rect: vec4<f32>,
    // [cx, cy, cw, ch] in logical screen space
    @location(9) clip_rect: vec4<f32>,
    // [tl, tr, br, bl]
    @location(10) clip_radii: vec4<f32>,
};

struct VsOut {
    @builtin(position) clip_pos: vec4<f32>,
    @location(0) local_pos: vec2<f32>,
    @location(1) screen_pos: vec2<f32>,
    @location(2) tex_uv: vec2<f32>,
    @location(3) rect: vec4<f32>,
    @location(4) radii: vec4<f32>,
    @location(5) color0: vec4<f32>,
    @location(6) color1: vec4<f32>,
    @location(7) grad_pts: vec4<f32>,
    @location(8) params: vec4<f32>,
    @location(9) clip_rect: vec4<f32>,
    @location(10) clip_radii: vec4<f32>,
    @location(11) @interpolate(flat) kind: f32,
};

@vertex
fn vs_main(in: VsInput) -> VsOut {
    let corner_x = f32(in.vertex_index & 1u);
    let corner_y = f32((in.vertex_index >> 1u) & 1u);
    let expand = in.params.z;

    let min_p = in.rect.xy - vec2<f32>(expand);
    let span = max(in.rect.zw, vec2<f32>(0.0)) + vec2<f32>(2.0 * expand);
    let local_pos = min_p + vec2<f32>(corner_x, corner_y) * span;

    let screen_pos = vec2<f32>(
        dot(in.xform_0.xy, local_pos) + in.xform_1.x,
        dot(in.xform_0.zw, local_pos) + in.xform_1.y,
    );

    let phys_pos = screen_pos * globals.scale_factor;
    let ndc_x = (phys_pos.x / max(globals.target_size.x, 1.0)) * 2.0 - 1.0;
    let ndc_y = 1.0 - (phys_pos.y / max(globals.target_size.y, 1.0)) * 2.0;

    let safe_wh = max(in.rect.zw, vec2<f32>(1e-4));
    let uv_frac = (local_pos - in.rect.xy) / safe_wh;
    let tex_uv = mix(in.uv_rect.xy, in.uv_rect.zw, uv_frac);

    var out: VsOut;
    out.clip_pos = vec4<f32>(ndc_x, ndc_y, 0.0, 1.0);
    out.local_pos = local_pos;
    out.screen_pos = screen_pos;
    out.tex_uv = tex_uv;
    out.rect = in.rect;
    out.radii = in.radii;
    out.color0 = in.color0;
    out.color1 = in.color1;
    out.grad_pts = in.grad_pts;
    out.params = in.params;
    out.clip_rect = in.clip_rect;
    out.clip_radii = in.clip_radii;
    out.kind = in.xform_1.z;
    return out;
}

fn sd_rounded_box(p_abs_center: vec2<f32>, rect: vec4<f32>, radii: vec4<f32>) -> f32 {
    let half_size = rect.zw * 0.5;
    let p = p_abs_center - (rect.xy + half_size);
    let top_r = select(radii.x, radii.y, p.x >= 0.0);
    let bot_r = select(radii.w, radii.z, p.x >= 0.0);
    let r = min(select(bot_r, top_r, p.y < 0.0), min(half_size.x, half_size.y));
    let q = abs(p) - half_size + vec2<f32>(r);
    return min(max(q.x, q.y), 0.0) + length(max(q, vec2<f32>(0.0))) - r;
}

fn erf_approx(x: f32) -> f32 {
    let s = sign(x);
    let a = abs(x);
    let t = 1.0 + (0.278393 + (0.230389 + (0.000972 + 0.078108 * a) * a) * a) * a;
    let t2 = t * t;
    return s * (1.0 - 1.0 / (t2 * t2));
}

fn gaussian_rounded_rect_shadow(p: vec2<f32>, rect: vec4<f32>, radii: vec4<f32>, sigma: f32) -> f32 {
    let half_size = max(rect.zw * 0.5, vec2<f32>(0.0));
    if (half_size.x <= 0.0 || half_size.y <= 0.0) {
        return 0.0;
    }
    let rel = p - (rect.xy + half_size);
    let inv_s2 = 1.0 / (1.41421356 * max(sigma, 0.05));
    let max_r = min(half_size.x, half_size.y);

    // 7-point Gaussian quadrature vertically across [-2.4*sigma, +2.4*sigma]
    var sum = 0.0;
    var w_sum = 0.0;
    for (var i = -3; i <= 3; i = i + 1) {
        let fi = f32(i);
        let t = fi * 0.8;
        let wy = exp(-0.5 * t * t);
        w_sum = w_sum + wy;

        let py = rel.y - t * sigma;
        let abs_py = abs(py);
        if (abs_py <= half_size.y) {
            let is_top = py < 0.0;
            let rl = min(select(radii.w, radii.x, is_top), max_r);
            let rr = min(select(radii.z, radii.y, is_top), max_r);
            let y_edge = half_size.y - abs_py;

            var x_left = -half_size.x;
            if (y_edge < rl && rl > 0.0) {
                let dy = rl - y_edge;
                x_left = x_left + (rl - sqrt(max(0.0, rl * rl - dy * dy)));
            }
            var x_right = half_size.x;
            if (y_edge < rr && rr > 0.0) {
                let dy = rr - y_edge;
                x_right = x_right - (rr - sqrt(max(0.0, rr * rr - dy * dy)));
            }
            if (x_right > x_left) {
                let cx = 0.5 * (erf_approx((rel.x - x_left) * inv_s2) - erf_approx((rel.x - x_right) * inv_s2));
                sum = sum + wy * max(cx, 0.0);
            }
        }
    }
    return clamp(sum / max(w_sum, 1e-5), 0.0, 1.0);
}

fn eval_paint_color(
    local_pos: vec2<f32>,
    color0: vec4<f32>,
    color1: vec4<f32>,
    grad_pts: vec4<f32>,
    mode: f32,
) -> vec4<f32> {
    if (mode < 0.5) {
        return color0;
    }
    var t = 0.0;
    let is_radial = abs(mode - 2.0) < 0.5 || mode >= 3.5;
    if (is_radial) {
        let r = max(grad_pts.z, 1e-4);
        t = clamp(length(local_pos - grad_pts.xy) / r, 0.0, 1.0);
    } else {
        let dir = grad_pts.zw - grad_pts.xy;
        let len2 = max(dot(dir, dir), 1e-6);
        t = clamp(dot(local_pos - grad_pts.xy, dir) / len2, 0.0, 1.0);
    }
    if (mode < 2.5) {
        return mix(color0, color1, t);
    }
    // N-stop gradient strip encoded in color1: (u0, u1, v_mid, 0)
    let ramp_u = mix(color1.x, color1.y, t);
    let sample_rgba = textureSampleLevel(atlas_tex, linear_sampler, vec2<f32>(ramp_u, color1.z), 0.0);
    let a = max(sample_rgba.a, 1e-5);
    return vec4<f32>(sample_rgba.rgb / a, sample_rgba.a);
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    // Evaluate derivatives unconditionally in uniform control flow for crisp AA
    let sd_shape = sd_rounded_box(in.local_pos, in.rect, in.radii);
    let aa_shape = clamp(fwidth(sd_shape) * 0.75, 0.35 / max(globals.scale_factor, 1.0), 2.5);

    let sd_clip = sd_rounded_box(in.screen_pos, in.clip_rect, in.clip_radii);
    let aa_clip = clamp(fwidth(sd_clip) * 0.75, 0.35 / max(globals.scale_factor, 1.0), 2.0);
    let clip_cov = clamp(0.5 - sd_clip / aa_clip, 0.0, 1.0);

    // Sample textures unconditionally before branches (uniform control flow)
    let atlas_sample = textureSample(atlas_tex, linear_sampler, in.tex_uv);
    let image_sample = textureSample(image_tex, linear_sampler, clamp(in.tex_uv, vec2<f32>(0.0), vec2<f32>(1.0)));

    if (clip_cov <= 0.001) {
        discard;
    }

    let kind = i32(round(in.kind));
    let opacity = in.params.w;

    if (kind == 4) {
        // PlatformViewHole cutout (only inside rect)
        if (sd_shape > 0.0) {
            discard;
        }
        return vec4<f32>(0.0, 0.0, 0.0, 0.0);
    }

    if (kind == 0) {
        // Rect fill or stroke
        let stroke_w = in.params.x;
        var cov = clamp(0.5 - sd_shape / aa_shape, 0.0, 1.0);
        if (stroke_w > 0.0) {
            let half_sw = stroke_w * 0.5;
            let ring_dist = abs(sd_shape) - half_sw;
            cov = clamp(0.5 - ring_dist / aa_shape, 0.0, 1.0);
        }
        let col = eval_paint_color(in.local_pos, in.color0, in.color1, in.grad_pts, in.params.y);
        let final_a = cov * clip_cov * col.a * opacity;
        if (final_a <= 0.001) {
            discard;
        }
        return vec4<f32>(col.rgb * final_a, final_a);
    }

    if (kind == 1) {
        // Gaussian rounded-box shadow (mode < 0.5: outer; mode >= 0.5: inset)
        let std_dev = max(in.grad_pts.x, 0.0);
        let spread = in.grad_pts.y;
        let off = in.grad_pts.zw;
        let is_inset = in.params.y >= 0.5;

        var cov = 0.0;
        if (is_inset) {
            let container_cov = clamp(0.5 - sd_shape / aa_shape, 0.0, 1.0);
            if (container_cov <= 0.001) {
                discard;
            }
            let cut_rect = vec4<f32>(
                in.rect.x + off.x + spread,
                in.rect.y + off.y + spread,
                max(in.rect.z - 2.0 * spread, 0.0),
                max(in.rect.w - 2.0 * spread, 0.0),
            );
            let cut_radii = max(in.radii - vec4<f32>(spread), vec4<f32>(0.0));
            let inner_alpha = select(
                clamp(0.5 - sd_rounded_box(in.local_pos, cut_rect, cut_radii) / aa_shape, 0.0, 1.0),
                gaussian_rounded_rect_shadow(in.local_pos, cut_rect, cut_radii, std_dev),
                std_dev >= 0.25,
            );
            cov = container_cov * (1.0 - inner_alpha);
        } else {
            // Note: rect passed by CPU already includes offset + spread adjustment for outer shadow
            if (std_dev < 0.25) {
                cov = clamp(0.5 - sd_shape / aa_shape, 0.0, 1.0);
            } else {
                cov = gaussian_rounded_rect_shadow(in.local_pos, in.rect, in.radii, std_dev);
            }
        }
        let final_a = cov * clip_cov * in.color0.a * opacity;
        if (final_a <= 0.001) {
            discard;
        }
        return vec4<f32>(in.color0.rgb * final_a, final_a);
    }

    if (kind == 2) {
        // Atlas glyph / rasterized vector mask
        if (in.tex_uv.x < 0.0 || in.tex_uv.x > 1.0 || in.tex_uv.y < 0.0 || in.tex_uv.y > 1.0) {
            discard;
        }
        let cov = atlas_sample.a;
        let col = eval_paint_color(in.local_pos, in.color0, in.color1, in.grad_pts, in.params.y);
        let final_a = cov * clip_cov * col.a * opacity;
        if (final_a <= 0.001) {
            discard;
        }
        return vec4<f32>(col.rgb * final_a, final_a);
    }

    // kind == 3: Textured image quad clipped cleanly with rounded rect SDF
    if (in.tex_uv.x < 0.0 || in.tex_uv.x > 1.0 || in.tex_uv.y < 0.0 || in.tex_uv.y > 1.0) {
        discard;
    }
    let cov = clamp(0.5 - sd_shape / aa_shape, 0.0, 1.0);
    let premul_a = image_sample.a * cov * clip_cov * opacity;
    if (premul_a <= 0.001) {
        discard;
    }
    return vec4<f32>(image_sample.rgb * cov * clip_cov * opacity, premul_a);
}
