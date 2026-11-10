use craftui_core::{Brush, Color, CornerRadii, Fit, PlatformViewId, Point, Rect, TextureId};
use craftui_text::TextStyle;
use kurbo::{Affine, BezPath};

/// Backend-agnostic 2D paint command recorded from the retained widget tree.
#[derive(Debug, Clone)]
pub enum PaintCommand {
    FillRect {
        rect: Rect,
        brush: Brush,
        radius: CornerRadii,
    },
    StrokeRect {
        rect: Rect,
        color: Color,
        width: f32,
        radius: CornerRadii,
    },
    DropShadow {
        rect: Rect,
        radius: f32,
        std_dev: f32,
        offset_x: f32,
        offset_y: f32,
        color: Color,
    },
    FillPath {
        path: BezPath,
        brush: Brush,
    },
    StrokePath {
        path: BezPath,
        color: Color,
        width: f32,
    },
    DrawImage {
        rect: Rect,
        texture: TextureId,
        fit: Fit,
        radius: CornerRadii,
        opacity: f32,
    },
    DrawText {
        origin: Point,
        text: String,
        style: TextStyle,
        max_width: Option<f32>,
    },
    PushClip {
        rect: Rect,
        radius: CornerRadii,
    },
    PopClip,
    PushLayer {
        opacity: f32,
    },
    PopLayer,
    PlatformViewHole {
        id: PlatformViewId,
        rect: Rect,
    },
}

/// Recorded vector scene for a single frame.
#[derive(Debug, Clone, Default)]
pub struct PaintScene {
    pub commands: Vec<PaintCommand>,
}

impl PaintScene {
    pub fn new() -> Self {
        Self {
            commands: Vec::new(),
        }
    }

    pub fn clear(&mut self) {
        self.commands.clear();
    }

    pub fn push(&mut self, cmd: PaintCommand) {
        self.commands.push(cmd);
    }

    pub fn fill_rect(&mut self, rect: Rect, brush: impl Into<Brush>, radius: CornerRadii) {
        self.commands.push(PaintCommand::FillRect {
            rect,
            brush: brush.into(),
            radius,
        });
    }

    pub fn stroke_rect(&mut self, rect: Rect, color: Color, width: f32, radius: CornerRadii) {
        self.commands.push(PaintCommand::StrokeRect {
            rect,
            color,
            width,
            radius,
        });
    }

    pub fn draw_text(
        &mut self,
        origin: Point,
        text: impl Into<String>,
        style: TextStyle,
        max_width: Option<f32>,
    ) {
        self.commands.push(PaintCommand::DrawText {
            origin,
            text: text.into(),
            style,
            max_width,
        });
    }
}

/// Compute destination rectangle & affine transform for fitting an image `(img_w, img_h)` into `bounds`.
pub fn compute_image_transform(bounds: Rect, img_w: u32, img_h: u32, fit: Fit) -> (Rect, Affine) {
    let iw = img_w.max(1) as f64;
    let ih = img_h.max(1) as f64;
    let bw = bounds.width.max(1.0) as f64;
    let bh = bounds.height.max(1.0) as f64;

    let (sx, sy) = match fit {
        Fit::Fill => (bw / iw, bh / ih),
        Fit::Contain => {
            let s = (bw / iw).min(bh / ih);
            (s, s)
        }
        Fit::Cover => {
            let s = (bw / iw).max(bh / ih);
            (s, s)
        }
        Fit::None => (1.0, 1.0),
    };

    let drawn_w = iw * sx;
    let drawn_h = ih * sy;
    let ox = bounds.x as f64 + (bw - drawn_w) * 0.5;
    let oy = bounds.y as f64 + (bh - drawn_h) * 0.5;

    let draw_rect = Rect::new(ox as f32, oy as f32, drawn_w as f32, drawn_h as f32);
    let affine = Affine::translate((ox, oy)) * Affine::scale_non_uniform(sx, sy);
    (draw_rect, affine)
}

/// Convert a [`craftui_core::Brush`] into a [`peniko::Brush`].
pub fn brush_to_peniko(brush: &Brush) -> peniko::Brush {
    match brush {
        Brush::Solid(c) => peniko::Brush::Solid(c.to_peniko()),
        Brush::LinearGradient { start, end, stops } => {
            let mut color_stops = peniko::ColorStops::new();
            for s in stops {
                color_stops.push(peniko::ColorStop::from((s.offset, s.color.to_peniko())));
            }
            peniko::Brush::Gradient(peniko::Gradient {
                kind: peniko::LinearGradientPosition {
                    start: start.to_kurbo(),
                    end: end.to_kurbo(),
                }
                .into(),
                stops: color_stops,
                extend: peniko::Extend::Pad,
                ..Default::default()
            })
        }
        Brush::RadialGradient {
            center,
            radius,
            stops,
        } => {
            let mut color_stops = peniko::ColorStops::new();
            for s in stops {
                color_stops.push(peniko::ColorStop::from((s.offset, s.color.to_peniko())));
            }
            peniko::Brush::Gradient(peniko::Gradient {
                kind: peniko::RadialGradientPosition {
                    start_center: center.to_kurbo(),
                    start_radius: 0.0,
                    end_center: center.to_kurbo(),
                    end_radius: *radius,
                }
                .into(),
                stops: color_stops,
                extend: peniko::Extend::Pad,
                ..Default::default()
            })
        }
    }
}
