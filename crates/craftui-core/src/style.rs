use crate::{Color, CornerRadii, Point};
use serde::{Deserialize, Serialize};

/// Dimension specification for layout styling.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub enum Length {
    #[default]
    Auto,
    Px(f32),
    Dp(f32),
    Pct(f32),
}

#[inline]
pub const fn auto() -> Length {
    Length::Auto
}

#[inline]
pub const fn px(v: f32) -> Length {
    Length::Px(v)
}

#[inline]
pub const fn dp(v: f32) -> Length {
    Length::Dp(v)
}

#[inline]
pub const fn pct(v: f32) -> Length {
    Length::Pct(v)
}

/// Gradient color stop.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ColorStop {
    pub offset: f32,
    pub color: Color,
}

/// Paint brush for filling or stroking vector geometry.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Brush {
    Solid(Color),
    LinearGradient {
        start: Point,
        end: Point,
        stops: Vec<ColorStop>,
    },
    RadialGradient {
        center: Point,
        radius: f32,
        stops: Vec<ColorStop>,
    },
}

impl From<Color> for Brush {
    fn from(color: Color) -> Self {
        Self::Solid(color)
    }
}

/// Border styling around a widget box.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Border {
    pub width: f32,
    pub color: Color,
    pub radius: CornerRadii,
}

impl Default for Border {
    fn default() -> Self {
        Self {
            width: 0.0,
            color: Color::TRANSPARENT,
            radius: CornerRadii::ZERO,
        }
    }
}

/// Drop shadow specification.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct BoxShadow {
    pub offset_x: f32,
    pub offset_y: f32,
    pub blur_radius: f32,
    pub color: Color,
}

/// Image / media box-fit behavior.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Fit {
    #[default]
    Contain,
    Cover,
    Fill,
    None,
}

/// Cross-axis alignment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Alignment {
    #[default]
    Start,
    Center,
    End,
    Stretch,
}

/// Main-axis content justification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Justify {
    #[default]
    Start,
    Center,
    End,
    SpaceBetween,
    SpaceAround,
    SpaceEvenly,
}

/// Text & layout reading direction (PRD FR-3.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Direction {
    #[default]
    Ltr,
    Rtl,
}

/// Scroll / layout axis.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Axis {
    #[default]
    Vertical,
    Horizontal,
}
