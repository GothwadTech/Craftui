use crate::PaintScene;
use craftui_core::{Brush, Color, Rect};
use kurbo::{Affine, BezPath, Circle, Ellipse, Line, RoundedRect, Shape};

/// Parsed SVG path primitive ready to transform and emit into a [`PaintScene`].
#[derive(Debug, Clone)]
pub struct SvgShape {
    pub path: BezPath,
    pub fill: Option<Color>,
    pub stroke: Option<Color>,
    pub stroke_width: f32,
}

/// Parsed vector icon asset with source `viewBox`.
#[derive(Debug, Clone)]
pub struct ParsedSvgIcon {
    pub view_box: (f64, f64, f64, f64),
    pub shapes: Vec<SvgShape>,
}

impl ParsedSvgIcon {
    /// Parse an icon specification from either a preset icon name, raw SVG path `d`, or `<svg ...>` XML.
    pub fn parse(spec: &str) -> Option<Self> {
        let trimmed = spec.trim();
        if trimmed.is_empty() {
            return None;
        }
        if let Some(preset) = builtin_icon_svg(trimmed) {
            return Self::parse(preset);
        }
        if !trimmed.starts_with('<') {
            let path = BezPath::from_svg(trimmed).ok()?;
            return Some(Self {
                view_box: (0.0, 0.0, 24.0, 24.0),
                shapes: vec![SvgShape {
                    path,
                    fill: Some(Color::WHITE),
                    stroke: None,
                    stroke_width: 0.0,
                }],
            });
        }

        let view_box = extract_attr(trimmed, "viewBox")
            .and_then(parse_view_box)
            .unwrap_or((0.0, 0.0, 24.0, 24.0));

        let root_fill = extract_attr(trimmed, "fill").and_then(parse_svg_paint_color);
        let root_stroke = extract_attr(trimmed, "stroke").and_then(parse_svg_paint_color);
        let root_stroke_w = extract_attr(trimmed, "stroke-width")
            .and_then(|s| s.trim().parse::<f32>().ok())
            .unwrap_or(2.0);

        let mut shapes = Vec::new();
        let mut cursor = 0usize;
        while let Some(rel_start) = trimmed[cursor..].find('<') {
            let tag_start = cursor + rel_start;
            let Some(rel_end) = trimmed[tag_start..].find('>') else {
                break;
            };
            let tag = &trimmed[tag_start + 1..tag_start + rel_end];
            cursor = tag_start + rel_end + 1;

            let tag_trimmed = tag.trim();
            if tag_trimmed.starts_with('/') || tag_trimmed.starts_with("svg") {
                continue;
            }
            let tag_name = tag_trimmed
                .split_whitespace()
                .next()
                .unwrap_or("")
                .trim_end_matches('/');

            let fill_attr = extract_attr(tag_trimmed, "fill");
            let stroke_attr = extract_attr(tag_trimmed, "stroke");
            let sw = extract_attr(tag_trimmed, "stroke-width")
                .and_then(|s| s.trim().parse::<f32>().ok())
                .unwrap_or(root_stroke_w);

            let explicit_none_fill = fill_attr.is_some_and(|v| v.eq_ignore_ascii_case("none"));
            let explicit_none_stroke = stroke_attr.is_some_and(|v| v.eq_ignore_ascii_case("none"));

            let fill = if explicit_none_fill {
                None
            } else {
                fill_attr.and_then(parse_svg_paint_color).or(root_fill).or(
                    if stroke_attr.is_some() || root_stroke.is_some() {
                        None
                    } else {
                        Some(Color::WHITE)
                    },
                )
            };

            let stroke = if explicit_none_stroke {
                None
            } else {
                stroke_attr.and_then(parse_svg_paint_color).or(root_stroke)
            };

            let maybe_path = match tag_name {
                "path" => extract_attr(tag_trimmed, "d").and_then(|d| BezPath::from_svg(d).ok()),
                "circle" => {
                    let cx = extract_attr(tag_trimmed, "cx")
                        .and_then(|s| s.parse::<f64>().ok())
                        .unwrap_or(0.0);
                    let cy = extract_attr(tag_trimmed, "cy")
                        .and_then(|s| s.parse::<f64>().ok())
                        .unwrap_or(0.0);
                    extract_attr(tag_trimmed, "r")
                        .and_then(|s| s.parse::<f64>().ok())
                        .map(|r| Circle::new((cx, cy), r).to_path(0.1))
                }
                "ellipse" => {
                    let cx = extract_attr(tag_trimmed, "cx")
                        .and_then(|s| s.parse::<f64>().ok())
                        .unwrap_or(0.0);
                    let cy = extract_attr(tag_trimmed, "cy")
                        .and_then(|s| s.parse::<f64>().ok())
                        .unwrap_or(0.0);
                    let rx = extract_attr(tag_trimmed, "rx")
                        .and_then(|s| s.parse::<f64>().ok())
                        .unwrap_or(0.0);
                    let ry = extract_attr(tag_trimmed, "ry")
                        .and_then(|s| s.parse::<f64>().ok())
                        .unwrap_or(0.0);
                    Some(Ellipse::new((cx, cy), (rx, ry), 0.0).to_path(0.1))
                }
                "rect" => {
                    let x = extract_attr(tag_trimmed, "x")
                        .and_then(|s| s.parse::<f64>().ok())
                        .unwrap_or(0.0);
                    let y = extract_attr(tag_trimmed, "y")
                        .and_then(|s| s.parse::<f64>().ok())
                        .unwrap_or(0.0);
                    let w = extract_attr(tag_trimmed, "width")
                        .and_then(|s| s.parse::<f64>().ok())
                        .unwrap_or(0.0);
                    let h = extract_attr(tag_trimmed, "height")
                        .and_then(|s| s.parse::<f64>().ok())
                        .unwrap_or(0.0);
                    let rx = extract_attr(tag_trimmed, "rx")
                        .and_then(|s| s.parse::<f64>().ok())
                        .unwrap_or(0.0);
                    Some(RoundedRect::new(x, y, x + w, y + h, rx).to_path(0.1))
                }
                "line" => {
                    let x1 = extract_attr(tag_trimmed, "x1")
                        .and_then(|s| s.parse::<f64>().ok())
                        .unwrap_or(0.0);
                    let y1 = extract_attr(tag_trimmed, "y1")
                        .and_then(|s| s.parse::<f64>().ok())
                        .unwrap_or(0.0);
                    let x2 = extract_attr(tag_trimmed, "x2")
                        .and_then(|s| s.parse::<f64>().ok())
                        .unwrap_or(0.0);
                    let y2 = extract_attr(tag_trimmed, "y2")
                        .and_then(|s| s.parse::<f64>().ok())
                        .unwrap_or(0.0);
                    Some(Line::new((x1, y1), (x2, y2)).to_path(0.1))
                }
                "polyline" | "polygon" => extract_attr(tag_trimmed, "points")
                    .and_then(|pts| points_to_path(pts, tag_name == "polygon")),
                _ => None,
            };

            if let Some(path) = maybe_path {
                shapes.push(SvgShape {
                    path,
                    fill,
                    stroke,
                    stroke_width: sw,
                });
            }
        }

        if shapes.is_empty() {
            return None;
        }
        Some(Self { view_box, shapes })
    }

    /// Emit scaled and centered SVG vector commands into [`PaintScene`].
    /// `tint` overrides generic `currentColor`/white default fills/strokes when provided.
    pub fn paint(&self, scene: &mut PaintScene, target: Rect, tint: Color) {
        let (vx, vy, vw, vh) = self.view_box;
        if vw <= 0.0 || vh <= 0.0 || target.width <= 0.0 || target.height <= 0.0 {
            return;
        }
        let sx = (target.width as f64) / vw;
        let sy = (target.height as f64) / vh;
        let scale = sx.min(sy);
        let dx = target.x as f64 + (target.width as f64 - vw * scale) * 0.5 - vx * scale;
        let dy = target.y as f64 + (target.height as f64 - vh * scale) * 0.5 - vy * scale;
        let xform = Affine::translate((dx, dy)) * Affine::scale(scale);

        for shape in &self.shapes {
            let transformed = xform * &shape.path;
            if let Some(fill_color) = shape.fill {
                let final_color = if fill_color == Color::WHITE {
                    tint
                } else {
                    fill_color
                };
                scene.fill_path(transformed.clone(), Brush::Solid(final_color));
            }
            if let Some(stroke_color) = shape.stroke {
                let final_color = if stroke_color == Color::WHITE {
                    tint
                } else {
                    stroke_color
                };
                let w = ((shape.stroke_width as f64) * scale).max(0.75) as f32;
                scene.stroke_path(transformed, final_color, w);
            }
        }
    }
}

/// Convenience function to parse and paint a named preset, raw `d` path, or `<svg>` string into `rect`.
pub fn paint_svg_icon(scene: &mut PaintScene, rect: Rect, spec: &str, tint: Color) -> bool {
    if let Some(parsed) = ParsedSvgIcon::parse(spec) {
        parsed.paint(scene, rect, tint);
        true
    } else {
        false
    }
}

/// Built-in crisp 24x24 vector icon catalog available by name in `Icon`.
pub fn builtin_icon_svg(name: &str) -> Option<&'static str> {
    Some(match name {
        "check" => {
            r#"<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4"><path d="M5 13l4 4L19 7"/></svg>"#
        }
        "close" | "x" => {
            r#"<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2"><line x1="6" y1="6" x2="18" y2="18"/><line x1="18" y1="6" x2="6" y2="18"/></svg>"#
        }
        "plus" | "add" => {
            r#"<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2"><line x1="12" y1="5" x2="12" y2="19"/><line x1="5" y1="12" x2="19" y2="12"/></svg>"#
        }
        "minus" => {
            r#"<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2"><line x1="5" y1="12" x2="19" y2="12"/></svg>"#
        }
        "chevron_down" => {
            r#"<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2"><path d="M6 9l6 6 6-6"/></svg>"#
        }
        "chevron_up" => {
            r#"<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2"><path d="M6 15l6-6 6 6"/></svg>"#
        }
        "chevron_right" => {
            r#"<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2"><path d="M9 6l6 6-6 6"/></svg>"#
        }
        "chevron_left" => {
            r#"<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2"><path d="M15 6l-6 6 6 6"/></svg>"#
        }
        "search" => {
            r#"<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.0"><circle cx="11" cy="11" r="7"/><line x1="16.5" y1="16.5" x2="21" y2="21"/></svg>"#
        }
        "settings" => {
            r#"<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.0"><circle cx="12" cy="12" r="3.5"/><path d="M12 2v3M12 19v3M4.9 4.9l2.1 2.1M17 17l2.1 2.1M2 12h3M19 12h3M4.9 19.1L7 17M17 7l2.1-2.1"/></svg>"#
        }
        "trash" => {
            r#"<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.0"><path d="M4 7h16M9 7V4h6v3M6 7l1 13h10l1-13M10 11v6M14 11v6"/></svg>"#
        }
        "edit" => {
            r#"<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.0"><path d="M4 20h4l10.5-10.5a2.1 2.1 0 0 0-3-3L5 17v3z"/></svg>"#
        }
        "menu" => {
            r#"<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2"><line x1="4" y1="7" x2="20" y2="7"/><line x1="4" y1="12" x2="20" y2="12"/><line x1="4" y1="17" x2="20" y2="17"/></svg>"#
        }
        "home" => {
            r#"<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.0"><path d="M3 10.5L12 3l9 7.5V20a1 1 0 0 1-1 1h-5v-6h-6v6H4a1 1 0 0 1-1-1z"/></svg>"#
        }
        "folder" => {
            r#"<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.0"><path d="M3 6a2 2 0 0 1 2-2h5l2 2h7a2 2 0 0 1 2 2v10a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z"/></svg>"#
        }
        "file" => {
            r#"<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.0"><path d="M6 3h8l4 4v14H6z"/><path d="M14 3v4h4"/></svg>"#
        }
        "sun" => {
            r#"<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.0"><circle cx="12" cy="12" r="4"/><path d="M12 2v2M12 20v2M4.9 4.9l1.4 1.4M17.7 17.7l1.4 1.4M2 12h2M20 12h2M4.9 19.1l1.4-1.4M17.7 6.3l1.4-1.4"/></svg>"#
        }
        "moon" => {
            r#"<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.0"><path d="M20 14.5A8.5 8.5 0 0 1 9.5 4a8.5 8.5 0 1 0 10.5 10.5z"/></svg>"#
        }
        "info" => {
            r#"<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.0"><circle cx="12" cy="12" r="9"/><line x1="12" y1="11" x2="12" y2="16"/><circle cx="12" cy="8" r="0.8" fill="currentColor"/></svg>"#
        }
        "warning" => {
            r#"<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.0"><path d="M12 3L2 20h20L12 3z"/><line x1="12" y1="9" x2="12" y2="14"/><circle cx="12" cy="17" r="0.8" fill="currentColor"/></svg>"#
        }
        "star" => {
            r#"<svg viewBox="0 0 24 24" fill="currentColor"><path d="M12 2l2.9 6.3L22 9.2l-5 4.9 1.2 6.9L12 17.7 5.8 21 7 14.1 2 9.2l7.1-.9z"/></svg>"#
        }
        "copy" => {
            r#"<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.0"><rect x="9" y="9" width="11" height="11" rx="2"/><path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1"/></svg>"#
        }
        "download" => {
            r#"<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.0"><path d="M12 4v11M7 10l5 5 5-5M4 20h16"/></svg>"#
        }
        "upload" => {
            r#"<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.0"><path d="M12 15V4M7 9l5-5 5 5M4 20h16"/></svg>"#
        }
        "camera" => {
            r#"<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.0"><path d="M23 19a2 2 0 0 1-2 2H3a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2h4l2-3h6l2 3h4a2 2 0 0 1 2 2z"/><circle cx="12" cy="13" r="4"/></svg>"#
        }
        "image" => {
            r#"<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.0"><rect x="3" y="3" width="18" height="18" rx="2"/><circle cx="8.5" cy="8.5" r="1.5"/><polyline points="21 15 16 10 5 21"/></svg>"#
        }
        "user" => {
            r#"<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.0"><path d="M20 21v-2a4 4 0 0 0-4-4H8a4 4 0 0 0-4 4v2"/><circle cx="12" cy="7" r="4"/></svg>"#
        }
        "more_vert" => {
            r#"<svg viewBox="0 0 24 24" fill="currentColor"><circle cx="12" cy="5" r="1.8"/><circle cx="12" cy="12" r="1.8"/><circle cx="12" cy="19" r="1.8"/></svg>"#
        }
        "document" => {
            r#"<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.0"><path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z"/><polyline points="14 2 14 8 20 8"/><line x1="16" y1="13" x2="8" y2="13"/><line x1="16" y1="17" x2="8" y2="17"/></svg>"#
        }
        "tools" => {
            r#"<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.0"><path d="M14.7 6.3a1 1 0 0 0 0 1.4l1.6 1.6a1 1 0 0 0 1.4 0l3.77-3.77a6 6 0 0 1-7.94 7.94l-6.91 6.91a2.12 2.12 0 0 1-3-3l6.91-6.91a6 6 0 0 1 7.94-7.94l-3.76 3.76z"/></svg>"#
        }
        "badge_id" => {
            r#"<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.0"><rect x="3" y="4" width="18" height="16" rx="2"/><circle cx="9" cy="10" r="2"/><line x1="15" y1="8" x2="17" y2="8"/><line x1="15" y1="12" x2="17" y2="12"/><line x1="7" y1="16" x2="17" y2="16"/></svg>"#
        }
        "sort" => {
            r#"<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.0"><path d="M7 16V4M7 4L3 8M7 4L11 8M17 8V20M17 20L21 16M17 20L13 16"/></svg>"#
        }
        "grid" => {
            r#"<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.0"><rect x="3" y="3" width="7" height="7"/><rect x="14" y="3" width="7" height="7"/><rect x="14" y="14" width="7" height="7"/><rect x="3" y="14" width="7" height="7"/></svg>"#
        }
        "list" => {
            r#"<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.0"><line x1="8" y1="6" x2="21" y2="6"/><line x1="8" y1="12" x2="21" y2="12"/><line x1="8" y1="18" x2="21" y2="18"/><line x1="3" y1="6" x2="3.01" y2="6"/><line x1="3" y1="12" x2="3.01" y2="12"/><line x1="3" y1="18" x2="3.01" y2="18"/></svg>"#
        }
        "check_double" => {
            r#"<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.0"><path d="M2 13l4 4L16 7M7 13l4 4L21 7"/></svg>"#
        }
        "aperture" => {
            r#"<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.0"><circle cx="12" cy="12" r="10"/><line x1="14.31" y1="8" x2="20.05" y2="17.94"/><line x1="9.69" y1="8" x2="21.17" y2="8"/><line x1="7.38" y1="12" x2="13.12" y2="2.06"/><line x1="9.69" y1="16" x2="3.95" y2="6.06"/><line x1="14.31" y1="16" x2="2.83" y2="16"/><line x1="16.62" y1="12" x2="10.88" y2="21.94"/></svg>"#
        }
        "share" => {
            r#"<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.0"><circle cx="18" cy="5" r="3"/><circle cx="6" cy="12" r="3"/><circle cx="18" cy="19" r="3"/><line x1="8.59" y1="13.51" x2="15.42" y2="17.49"/><line x1="15.41" y1="6.51" x2="8.59" y2="10.49"/></svg>"#
        }
        "folder_plus" => {
            r#"<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.0"><path d="M3 6a2 2 0 0 1 2-2h5l2 2h7a2 2 0 0 1 2 2v10a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z"/><line x1="12" y1="10" x2="12" y2="16"/><line x1="9" y1="13" x2="15" y2="13"/></svg>"#
        }
        "watermark" => {
            r#"<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.0"><rect x="3" y="4" width="18" height="16" rx="2"/><rect x="7" y="8" width="6" height="5" rx="1" fill="currentColor"/></svg>"#
        }
        "grid_3x3" => {
            r#"<svg viewBox="0 0 24 24" fill="currentColor"><circle cx="5" cy="5" r="1.8"/><circle cx="12" cy="5" r="1.8"/><circle cx="19" cy="5" r="1.8"/><circle cx="5" cy="12" r="1.8"/><circle cx="12" cy="12" r="1.8"/><circle cx="19" cy="12" r="1.8"/><circle cx="5" cy="19" r="1.8"/><circle cx="12" cy="19" r="1.8"/><circle cx="19" cy="19" r="1.8"/></svg>"#
        }
        "pdf" => {
            r#"<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.0"><path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z"/><polyline points="14 2 14 8 20 8"/><path d="M7 14h2a1 1 0 0 0 1-1v-1a1 1 0 0 0-1-1H7v5M13 11v5M13 11h1.5a1.5 1.5 0 0 1 1.5 1.5v2a1.5 1.5 0 0 1-1.5 1.5H13"/></svg>"#
        }
        "copy_pages" | "pages" => {
            r#"<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.0"><rect x="8" y="8" width="12" height="12" rx="2"/><path d="M4 16V4a2 2 0 0 1 2-2h12"/></svg>"#
        }
        "lock" => {
            r#"<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.0"><rect x="5" y="11" width="14" height="10" rx="2"/><path d="M8 11V7a4 4 0 0 1 8 0v4"/></svg>"#
        }
        "unlock" => {
            r#"<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.0"><rect x="5" y="11" width="14" height="10" rx="2"/><path d="M8 11V7a4 4 0 0 1 7.5-2"/></svg>"#
        }
        "hand" | "pan" => {
            r#"<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.0"><path d="M18 11V6a2 2 0 0 0-4 0v4M14 10V4a2 2 0 0 0-4 0v6M10 10.5V6a2 2 0 0 0-4 0v8c0 4.4 3.6 8 8 8h1a8 8 0 0 0 8-8v-3a2 2 0 0 0-4 0v2"/></svg>"#
        }
        "select" | "cursor" => {
            r#"<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.0"><path d="M4 4l7 17 2.5-6.5L20 12 4 4z"/></svg>"#
        }
        "rectangle" | "rect" => {
            r#"<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.0"><rect x="4" y="5" width="16" height="14" rx="2"/></svg>"#
        }
        "diamond" => {
            r#"<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.0"><polygon points="12 3 21 12 12 21 3 12"/></svg>"#
        }
        "ellipse" | "circle" => {
            r#"<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.0"><circle cx="12" cy="12" r="8"/></svg>"#
        }
        "arrow" | "arrow_right" | "arrow_tool" => {
            r#"<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.0"><line x1="4" y1="12" x2="20" y2="12"/><polyline points="14 6 20 12 14 18"/></svg>"#
        }
        "line" => {
            r#"<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.0"><line x1="4" y1="19" x2="20" y2="5"/></svg>"#
        }
        "pen" | "pencil" => {
            r#"<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.0"><path d="M17 3a2.83 2.83 0 1 1 4 4L7.5 20.5 2 22l1.5-5.5L17 3z"/></svg>"#
        }
        "text" | "type" => {
            r#"<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.0"><polyline points="4 7 4 4 20 4 20 7"/><line x1="12" y1="4" x2="12" y2="20"/><line x1="9" y1="20" x2="15" y2="20"/></svg>"#
        }
        "card" | "note" => {
            r#"<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.0"><rect x="3" y="4" width="18" height="16" rx="2"/><line x1="7" y1="9" x2="17" y2="9"/><line x1="7" y1="13" x2="13" y2="13"/></svg>"#
        }
        "eraser" => {
            r#"<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.0"><path d="M19 14l-6.5 6.5a2 2 0 0 1-2.8 0L2.5 13.3a2 2 0 0 1 0-2.8l9.2-9.2a2 2 0 0 1 2.8 0L20.2 7a2 2 0 0 1 0 2.8L19 14z"/><line x1="18" y1="20" x2="6" y2="20"/></svg>"#
        }
        "undo" => {
            r#"<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.0"><path d="M3 7v6h6"/><path d="M21 17a9 9 0 0 0-9-9 9 9 0 0 0-6 2.3L3 13"/></svg>"#
        }
        "redo" => {
            r#"<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.0"><path d="M21 7v6h-6"/><path d="M3 17a9 9 0 0 1 9-9 9 9 0 0 1 6 2.3L21 13"/></svg>"#
        }
        "shield" => {
            r#"<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.0"><path d="M12 22s8-4 8-10V5l-8-3-8 3v7c0 6 8 10 8 10z"/></svg>"#
        }
        "help" | "question" => {
            r#"<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.0"><circle cx="12" cy="12" r="10"/><path d="M9.09 9a3 3 0 0 1 5.83 1c0 2-3 3-3 3"/><circle cx="12" cy="17" r="0.8" fill="currentColor"/></svg>"#
        }
        "map" => {
            r#"<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.0"><polygon points="1 6 8 2 16 6 23 2 23 18 16 22 8 18 1 22 1 6"/><line x1="8" y1="2" x2="8" y2="18"/><line x1="16" y1="6" x2="16" y2="22"/></svg>"#
        }
        "gift" => {
            r#"<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.0"><polyline points="20 12 20 22 4 22 4 12"/><rect x="2" y="7" width="20" height="5"/><line x1="12" y1="22" x2="12" y2="7"/><path d="M12 7H7.5a2.5 2.5 0 0 1 0-5C11 2 12 7 12 7z"/><path d="M12 7h4.5a2.5 2.5 0 0 0 0-5C13 2 12 7 12 7z"/></svg>"#
        }
        "sidebar" => {
            r#"<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.0"><rect x="3" y="3" width="18" height="18" rx="2"/><line x1="15" y1="3" x2="15" y2="21"/></svg>"#
        }
        _ => return None,
    })
}

fn extract_attr<'a>(tag: &'a str, name: &str) -> Option<&'a str> {
    let mut search_from = 0usize;
    while let Some(pos) = tag[search_from..].find(name) {
        let abs = search_from + pos;
        let valid_prefix = abs == 0 || !tag.as_bytes()[abs - 1].is_ascii_alphanumeric();
        let rest = &tag[abs + name.len()..];
        let after_eq = rest.trim_start();
        if valid_prefix && let Some(after) = after_eq.strip_prefix('=') {
            let after_ws = after.trim_start();
            if let Some(quote) = after_ws.chars().next()
                && (quote == '"' || quote == '\'')
            {
                let inner = &after_ws[1..];
                if let Some(end) = inner.find(quote) {
                    return Some(&inner[..end]);
                }
            }
        }
        search_from = abs + name.len();
    }
    None
}

fn parse_view_box(s: &str) -> Option<(f64, f64, f64, f64)> {
    let nums: Vec<f64> = s
        .split(|c: char| c.is_whitespace() || c == ',')
        .filter(|t| !t.is_empty())
        .filter_map(|t| t.parse::<f64>().ok())
        .collect();
    if nums.len() >= 4 {
        Some((nums[0], nums[1], nums[2], nums[3]))
    } else {
        None
    }
}

fn parse_svg_paint_color(s: &str) -> Option<Color> {
    let trimmed = s.trim();
    if trimmed.eq_ignore_ascii_case("none") {
        return None;
    }
    if trimmed.eq_ignore_ascii_case("currentColor") {
        return Some(Color::WHITE);
    }
    if let Some(c) = Color::from_hex(trimmed) {
        return Some(c);
    }
    match trimmed.to_ascii_lowercase().as_str() {
        "black" => Some(Color::BLACK),
        "white" => Some(Color::WHITE),
        "red" => Some(Color::rgb(239, 68, 68)),
        "green" => Some(Color::rgb(34, 197, 94)),
        "blue" => Some(Color::rgb(59, 130, 246)),
        "transparent" => Some(Color::TRANSPARENT),
        _ => None,
    }
}

fn points_to_path(points: &str, close: bool) -> Option<BezPath> {
    let nums: Vec<f64> = points
        .split(|c: char| c.is_whitespace() || c == ',')
        .filter(|t| !t.is_empty())
        .filter_map(|t| t.parse::<f64>().ok())
        .collect();
    if nums.len() < 4 {
        return None;
    }
    let mut path = BezPath::new();
    path.move_to((nums[0], nums[1]));
    for chunk in nums[2..].as_chunks::<2>().0 {
        path.line_to((chunk[0], chunk[1]));
    }
    if close {
        path.close_path();
    }
    Some(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_presets_raw_paths_and_full_svg_documents() {
        let mut scene = PaintScene::new();
        assert!(paint_svg_icon(
            &mut scene,
            Rect::new(0.0, 0.0, 24.0, 24.0),
            "check",
            Color::WHITE
        ));
        assert!(paint_svg_icon(
            &mut scene,
            Rect::new(0.0, 0.0, 24.0, 24.0),
            "M4 12 L20 12 Z",
            Color::WHITE
        ));
        assert!(paint_svg_icon(
            &mut scene,
            Rect::new(0.0, 0.0, 24.0, 24.0),
            r#"<svg viewBox="0 0 16 16"><circle cx="8" cy="8" r="6" fill="currentColor"/></svg>"#,
            Color::rgb(59, 130, 246)
        ));
        assert_eq!(scene.commands.len(), 3);
    }
}
