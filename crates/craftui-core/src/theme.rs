use crate::Color;
use serde::{Deserialize, Serialize};

/// Supported theme variants (PRD FR-6.3 & Design Spec §4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum ThemeMode {
    Light,
    #[default]
    Dark,
    Amoled,
    HighContrast,
}

/// Reactive semantic design tokens.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ThemeTokens {
    pub mode: ThemeMode,
    pub background: Color,
    pub surface: Color,
    pub surface_variant: Color,
    pub border: Color,
    pub primary: Color,
    pub on_primary: Color,
    pub text_primary: Color,
    pub text_secondary: Color,
    pub danger: Color,
    pub radius_md: f32,
    pub spacing_unit: f32,
}

impl Default for ThemeTokens {
    fn default() -> Self {
        Self::dark()
    }
}

impl ThemeTokens {
    pub fn from_mode(mode: ThemeMode) -> Self {
        match mode {
            ThemeMode::Light => Self::light(),
            ThemeMode::Dark => Self::dark(),
            ThemeMode::Amoled => Self::amoled(),
            ThemeMode::HighContrast => Self::high_contrast(),
        }
    }

    pub fn light() -> Self {
        Self {
            mode: ThemeMode::Light,
            background: Color::rgb(248, 249, 250),
            surface: Color::rgb(255, 255, 255),
            surface_variant: Color::rgb(238, 242, 246),
            border: Color::rgb(209, 217, 224),
            primary: Color::rgb(0, 186, 102),
            on_primary: Color::WHITE,
            text_primary: Color::rgb(17, 24, 39),
            text_secondary: Color::rgb(75, 85, 99),
            danger: Color::rgb(220, 38, 38),
            radius_md: 12.0,
            spacing_unit: 8.0,
        }
    }

    pub fn dark() -> Self {
        Self {
            mode: ThemeMode::Dark,
            background: Color::rgb(17, 20, 26),
            surface: Color::rgb(26, 31, 41),
            surface_variant: Color::rgb(37, 44, 58),
            border: Color::rgb(55, 65, 81),
            primary: Color::rgb(0, 186, 102),
            on_primary: Color::rgb(6, 20, 12),
            text_primary: Color::rgb(243, 244, 246),
            text_secondary: Color::rgb(156, 163, 175),
            danger: Color::rgb(248, 113, 113),
            radius_md: 12.0,
            spacing_unit: 8.0,
        }
    }

    pub fn amoled() -> Self {
        Self {
            mode: ThemeMode::Amoled,
            background: Color::BLACK,
            surface: Color::rgb(12, 14, 18),
            surface_variant: Color::rgb(24, 28, 36),
            border: Color::rgb(42, 48, 60),
            primary: Color::rgb(0, 208, 114),
            on_primary: Color::BLACK,
            text_primary: Color::WHITE,
            text_secondary: Color::rgb(160, 168, 180),
            danger: Color::rgb(255, 99, 99),
            radius_md: 12.0,
            spacing_unit: 8.0,
        }
    }

    pub fn high_contrast() -> Self {
        Self {
            mode: ThemeMode::HighContrast,
            background: Color::BLACK,
            surface: Color::rgb(10, 10, 10),
            surface_variant: Color::rgb(28, 28, 28),
            border: Color::WHITE,
            primary: Color::rgb(0, 255, 136),
            on_primary: Color::BLACK,
            text_primary: Color::WHITE,
            text_secondary: Color::rgb(230, 230, 230),
            danger: Color::rgb(255, 82, 82),
            radius_md: 8.0,
            spacing_unit: 8.0,
        }
    }

    /// Smoothly interpolate between two theme palettes for animated theme switching.
    pub fn lerp(&self, target: &Self, t: f32) -> Self {
        let t = t.clamp(0.0, 1.0);
        Self {
            mode: if t < 0.5 { self.mode } else { target.mode },
            background: self.background.lerp(target.background, t),
            surface: self.surface.lerp(target.surface, t),
            surface_variant: self.surface_variant.lerp(target.surface_variant, t),
            border: self.border.lerp(target.border, t),
            primary: self.primary.lerp(target.primary, t),
            on_primary: self.on_primary.lerp(target.on_primary, t),
            text_primary: self.text_primary.lerp(target.text_primary, t),
            text_secondary: self.text_secondary.lerp(target.text_secondary, t),
            danger: self.danger.lerp(target.danger, t),
            radius_md: self.radius_md + (target.radius_md - self.radius_md) * t,
            spacing_unit: self.spacing_unit + (target.spacing_unit - self.spacing_unit) * t,
        }
    }
}
