use serde::{Deserialize, Serialize};

/// 2D point in logical pixels (`dp`).
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct Point {
    pub x: f32,
    pub y: f32,
}

impl Point {
    pub const ZERO: Self = Self { x: 0.0, y: 0.0 };

    #[inline]
    pub const fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }

    #[inline]
    pub fn distance_to(self, other: Self) -> f32 {
        let dx = self.x - other.x;
        let dy = self.y - other.y;
        (dx * dx + dy * dy).sqrt()
    }

    #[inline]
    pub fn lerp(self, other: Self, t: f32) -> Self {
        Self {
            x: self.x + (other.x - self.x) * t,
            y: self.y + (other.y - self.y) * t,
        }
    }

    #[inline]
    pub fn to_kurbo(self) -> kurbo::Point {
        kurbo::Point::new(self.x as f64, self.y as f64)
    }
}

impl From<kurbo::Point> for Point {
    fn from(p: kurbo::Point) -> Self {
        Self::new(p.x as f32, p.y as f32)
    }
}

/// 2D size in logical pixels.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct Size {
    pub width: f32,
    pub height: f32,
}

impl Size {
    pub const ZERO: Self = Self {
        width: 0.0,
        height: 0.0,
    };

    #[inline]
    pub const fn new(width: f32, height: f32) -> Self {
        Self { width, height }
    }

    #[inline]
    pub fn is_empty(self) -> bool {
        self.width <= 0.0 || self.height <= 0.0
    }

    #[inline]
    pub fn to_kurbo(self) -> kurbo::Size {
        kurbo::Size::new(self.width as f64, self.height as f64)
    }
}

/// Axis-aligned rectangle in logical pixels.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl Rect {
    pub const ZERO: Self = Self {
        x: 0.0,
        y: 0.0,
        width: 0.0,
        height: 0.0,
    };

    #[inline]
    pub const fn new(x: f32, y: f32, width: f32, height: f32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    #[inline]
    pub fn from_origin_size(origin: Point, size: Size) -> Self {
        Self::new(origin.x, origin.y, size.width, size.height)
    }

    #[inline]
    pub fn origin(self) -> Point {
        Point::new(self.x, self.y)
    }

    #[inline]
    pub fn size(self) -> Size {
        Size::new(self.width, self.height)
    }

    #[inline]
    pub fn max_x(self) -> f32 {
        self.x + self.width
    }

    #[inline]
    pub fn max_y(self) -> f32 {
        self.y + self.height
    }

    #[inline]
    pub fn center(self) -> Point {
        Point::new(self.x + self.width * 0.5, self.y + self.height * 0.5)
    }

    #[inline]
    pub fn contains(self, p: Point) -> bool {
        p.x >= self.x && p.x <= self.max_x() && p.y >= self.y && p.y <= self.max_y()
    }

    #[inline]
    pub fn inflate(self, dx: f32, dy: f32) -> Self {
        Self::new(
            self.x - dx,
            self.y - dy,
            (self.width + dx * 2.0).max(0.0),
            (self.height + dy * 2.0).max(0.0),
        )
    }

    #[inline]
    pub fn inset(self, insets: Insets) -> Self {
        Self::new(
            self.x + insets.left,
            self.y + insets.top,
            (self.width - insets.left - insets.right).max(0.0),
            (self.height - insets.top - insets.bottom).max(0.0),
        )
    }

    #[inline]
    pub fn intersects(self, other: Self) -> bool {
        self.x < other.max_x()
            && self.max_x() > other.x
            && self.y < other.max_y()
            && self.max_y() > other.y
    }

    #[inline]
    pub fn translate(self, dx: f32, dy: f32) -> Self {
        Self::new(self.x + dx, self.y + dy, self.width, self.height)
    }

    #[inline]
    pub fn to_kurbo(self) -> kurbo::Rect {
        kurbo::Rect::new(
            self.x as f64,
            self.y as f64,
            self.max_x() as f64,
            self.max_y() as f64,
        )
    }

    #[inline]
    pub fn to_rounded_kurbo(self, radii: CornerRadii) -> kurbo::RoundedRect {
        kurbo::RoundedRect::from_rect(
            self.to_kurbo(),
            kurbo::RoundedRectRadii::new(
                radii.top_left as f64,
                radii.top_right as f64,
                radii.bottom_right as f64,
                radii.bottom_left as f64,
            ),
        )
    }
}

/// Edge insets (padding, margin, or safe area).
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct Insets {
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
    pub left: f32,
}

impl Insets {
    pub const ZERO: Self = Self {
        top: 0.0,
        right: 0.0,
        bottom: 0.0,
        left: 0.0,
    };

    #[inline]
    pub const fn all(v: f32) -> Self {
        Self {
            top: v,
            right: v,
            bottom: v,
            left: v,
        }
    }

    #[inline]
    pub const fn symmetric(horizontal: f32, vertical: f32) -> Self {
        Self {
            top: vertical,
            right: horizontal,
            bottom: vertical,
            left: horizontal,
        }
    }

    #[inline]
    pub const fn new(top: f32, right: f32, bottom: f32, left: f32) -> Self {
        Self {
            top,
            right,
            bottom,
            left,
        }
    }

    #[inline]
    pub fn max_combine(self, other: Self) -> Self {
        Self {
            top: self.top.max(other.top),
            right: self.right.max(other.right),
            bottom: self.bottom.max(other.bottom),
            left: self.left.max(other.left),
        }
    }
}

/// Per-corner border radii.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct CornerRadii {
    pub top_left: f32,
    pub top_right: f32,
    pub bottom_right: f32,
    pub bottom_left: f32,
}

impl CornerRadii {
    pub const ZERO: Self = Self::all(0.0);

    #[inline]
    pub const fn all(radius: f32) -> Self {
        Self {
            top_left: radius,
            top_right: radius,
            bottom_right: radius,
            bottom_left: radius,
        }
    }

    #[inline]
    pub const fn new(top_left: f32, top_right: f32, bottom_right: f32, bottom_left: f32) -> Self {
        Self {
            top_left,
            top_right,
            bottom_right,
            bottom_left,
        }
    }

    #[inline]
    pub fn is_zero(self) -> bool {
        self.top_left == 0.0
            && self.top_right == 0.0
            && self.bottom_right == 0.0
            && self.bottom_left == 0.0
    }
}

impl From<f32> for CornerRadii {
    fn from(r: f32) -> Self {
        Self::all(r)
    }
}

/// Box layout constraints.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Constraints {
    pub min_width: f32,
    pub max_width: f32,
    pub min_height: f32,
    pub max_height: f32,
}

impl Default for Constraints {
    fn default() -> Self {
        Self {
            min_width: 0.0,
            max_width: f32::INFINITY,
            min_height: 0.0,
            max_height: f32::INFINITY,
        }
    }
}

impl Constraints {
    pub const fn tight(size: Size) -> Self {
        Self {
            min_width: size.width,
            max_width: size.width,
            min_height: size.height,
            max_height: size.height,
        }
    }

    pub fn constrain(self, size: Size) -> Size {
        Size::new(
            size.width.clamp(self.min_width, self.max_width),
            size.height.clamp(self.min_height, self.max_height),
        )
    }
}

/// Responsive viewport size breakpoint (PRD FR-3.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Breakpoint {
    /// Phones and narrow panes (< 600dp).
    Compact,
    /// Tablets and small windows (600dp..=1024dp).
    Medium,
    /// Desktops and wide tablets (> 1024dp).
    Expanded,
}

impl Breakpoint {
    pub fn from_width(width_dp: f32) -> Self {
        if width_dp < 600.0 {
            Self::Compact
        } else if width_dp <= 1024.0 {
            Self::Medium
        } else {
            Self::Expanded
        }
    }
}

/// Current window and platform environment metrics.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct WindowMetrics {
    pub logical_size: Size,
    pub scale_factor: f32,
    pub safe_area: Insets,
    pub keyboard_inset: f32,
}

impl Default for WindowMetrics {
    fn default() -> Self {
        Self {
            logical_size: Size::new(1024.0, 768.0),
            scale_factor: 1.0,
            safe_area: Insets::ZERO,
            keyboard_inset: 0.0,
        }
    }
}

impl WindowMetrics {
    pub fn physical_size(self) -> (u32, u32) {
        (
            (self.logical_size.width * self.scale_factor)
                .round()
                .max(1.0) as u32,
            (self.logical_size.height * self.scale_factor)
                .round()
                .max(1.0) as u32,
        )
    }

    pub fn effective_safe_area(self) -> Insets {
        Insets {
            bottom: self.safe_area.bottom.max(self.keyboard_inset),
            ..self.safe_area
        }
    }

    pub fn breakpoint(self) -> Breakpoint {
        Breakpoint::from_width(self.logical_size.width)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rect_operations() {
        let r = Rect::new(10.0, 20.0, 100.0, 50.0);
        assert!(r.contains(Point::new(50.0, 40.0)));
        assert!(!r.contains(Point::new(5.0, 40.0)));
        let inset = r.inset(Insets::all(5.0));
        assert_eq!(inset, Rect::new(15.0, 25.0, 90.0, 40.0));
    }

    #[test]
    fn breakpoints() {
        assert_eq!(Breakpoint::from_width(390.0), Breakpoint::Compact);
        assert_eq!(Breakpoint::from_width(768.0), Breakpoint::Medium);
        assert_eq!(Breakpoint::from_width(1440.0), Breakpoint::Expanded);
    }
}
