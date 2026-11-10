use crate::PaintScene;
use craftui_core::{Brush, Color, Rect};
use kurbo::{Affine, BezPath, Circle, Line, RoundedRect, Shape};

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
