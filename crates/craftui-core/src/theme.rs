use crate::Color;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Supported theme variants (PRD FR-6.3 & Design Spec §4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum ThemeMode {
    Light,
    #[default]
    Dark,
    Amoled,
    HighContrast,
}

/// Extensible theme customization & brand palette overrides.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct ThemeOverride {
    pub background: Option<Color>,
    pub surface: Option<Color>,
    pub surface_variant: Option<Color>,
    pub border: Option<Color>,
    pub primary: Option<Color>,
    pub on_primary: Option<Color>,
    pub text_primary: Option<Color>,
    pub text_secondary: Option<Color>,
    pub danger: Option<Color>,
    pub radius_md: Option<f32>,
    pub spacing_unit: Option<f32>,
    /// Arbitrary application-defined color tokens (e.g., `brand_accent`, `warning`, `info`).
    pub custom_colors: BTreeMap<String, Color>,
    /// Arbitrary application-defined scalar tokens.
    pub custom_floats: BTreeMap<String, f32>,
}

impl ThemeOverride {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_primary(mut self, primary: Color, on_primary: Color) -> Self {
        self.primary = Some(primary);
        self.on_primary = Some(on_primary);
        self
    }

    pub fn with_surface_palette(
        mut self,
        background: Color,
        surface: Color,
        surface_variant: Color,
        border: Color,
    ) -> Self {
        self.background = Some(background);
        self.surface = Some(surface);
        self.surface_variant = Some(surface_variant);
        self.border = Some(border);
        self
    }

    pub fn with_color(mut self, name: impl Into<String>, color: Color) -> Self {
        self.custom_colors.insert(name.into(), color);
        self
    }

    pub fn with_float(mut self, name: impl Into<String>, value: f32) -> Self {
        self.custom_floats.insert(name.into(), value);
        self
    }

    pub fn color(&self, name: &str) -> Option<Color> {
        self.custom_colors.get(name).copied()
    }

    pub fn float(&self, name: &str) -> Option<f32> {
        self.custom_floats.get(name).copied()
    }
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
            background: Color::rgb(248, 250, 252),
            surface: Color::rgb(255, 255, 255),
            surface_variant: Color::rgb(241, 245, 249),
            border: Color::rgb(203, 213, 225),
            primary: Color::rgb(37, 99, 235),
            on_primary: Color::WHITE,
            text_primary: Color::rgb(15, 23, 42),
            text_secondary: Color::rgb(71, 85, 105),
            danger: Color::rgb(220, 38, 38),
            radius_md: 12.0,
            spacing_unit: 8.0,
        }
    }

    pub fn dark() -> Self {
        Self {
            mode: ThemeMode::Dark,
            background: Color::rgb(15, 23, 42),
            surface: Color::rgb(30, 41, 59),
            surface_variant: Color::rgb(51, 65, 85),
            border: Color::rgb(71, 85, 105),
            primary: Color::rgb(59, 130, 246),
            on_primary: Color::WHITE,
            text_primary: Color::rgb(248, 250, 252),
            text_secondary: Color::rgb(148, 163, 184),
            danger: Color::rgb(248, 113, 113),
            radius_md: 12.0,
            spacing_unit: 8.0,
        }
    }

    pub fn amoled() -> Self {
        Self {
            mode: ThemeMode::Amoled,
            background: Color::BLACK,
            surface: Color::rgb(14, 17, 23),
            surface_variant: Color::rgb(26, 32, 44),
            border: Color::rgb(45, 55, 72),
            primary: Color::rgb(96, 165, 250),
            on_primary: Color::rgb(10, 14, 23),
            text_primary: Color::WHITE,
            text_secondary: Color::rgb(160, 174, 192),
            danger: Color::rgb(252, 129, 129),
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
            primary: Color::rgb(56, 189, 248),
            on_primary: Color::BLACK,
            text_primary: Color::WHITE,
            text_secondary: Color::rgb(230, 230, 230),
            danger: Color::rgb(255, 82, 82),
            radius_md: 8.0,
            spacing_unit: 8.0,
        }
    }

    /// Return a copy of this theme with custom accent colors applied.
    pub fn with_primary(mut self, primary: Color, on_primary: Color) -> Self {
        self.primary = primary;
        self.on_primary = on_primary;
        self
    }

    /// Return a copy of this theme with selective overrides from a [`ThemeOverride`].
    pub fn applied(mut self, override_spec: &ThemeOverride) -> Self {
        if let Some(c) = override_spec.background {
            self.background = c;
        }
        if let Some(c) = override_spec.surface {
            self.surface = c;
        }
        if let Some(c) = override_spec.surface_variant {
            self.surface_variant = c;
        }
        if let Some(c) = override_spec.border {
            self.border = c;
        }
        if let Some(c) = override_spec.primary {
            self.primary = c;
        }
        if let Some(c) = override_spec.on_primary {
            self.on_primary = c;
        }
        if let Some(c) = override_spec.text_primary {
            self.text_primary = c;
        }
        if let Some(c) = override_spec.text_secondary {
            self.text_secondary = c;
        }
        if let Some(c) = override_spec.danger {
            self.danger = c;
        }
        if let Some(v) = override_spec.radius_md {
            self.radius_md = v;
        }
        if let Some(v) = override_spec.spacing_unit {
            self.spacing_unit = v;
        }
        self
    }

    /// Lookup a standard semantic color token by name.
    pub fn token_color(&self, name: &str) -> Option<Color> {
        match name {
            "background" => Some(self.background),
            "surface" => Some(self.surface),
            "surface_variant" => Some(self.surface_variant),
            "border" => Some(self.border),
            "primary" => Some(self.primary),
            "on_primary" => Some(self.on_primary),
            "text_primary" => Some(self.text_primary),
            "text_secondary" => Some(self.text_secondary),
            "danger" => Some(self.danger),
            _ => None,
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

    /// Create Material 3 dynamic tokens from a seed color.
    pub fn material3(seed: Color, mode: ThemeMode) -> Self {
        let is_dark = matches!(mode, ThemeMode::Dark | ThemeMode::Amoled);
        Material3Tokens::from_seed(seed, is_dark).to_theme_tokens()
    }
}

/// Material 3 standard elevation levels and dynamic drop shadows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum M3Elevation {
    #[default]
    Level0,
    Level1,
    Level2,
    Level3,
    Level4,
    Level5,
}

impl M3Elevation {
    /// Return the standard Material 3 elevation drop shadow.
    pub fn to_shadow(self, ambient_tint: Color) -> crate::BoxShadow {
        match self {
            Self::Level0 => crate::BoxShadow::drop(0.0, 0.0, 0.0, 0.0, Color::TRANSPARENT),
            Self::Level1 => crate::BoxShadow::drop(0.0, 1.0, 3.0, 0.0, ambient_tint.with_alpha(0.14)),
            Self::Level2 => crate::BoxShadow::drop(0.0, 2.0, 6.0, 0.0, ambient_tint.with_alpha(0.18)),
            Self::Level3 => crate::BoxShadow::drop(0.0, 4.0, 10.0, 0.0, ambient_tint.with_alpha(0.22)),
            Self::Level4 => crate::BoxShadow::drop(0.0, 6.0, 14.0, 0.0, ambient_tint.with_alpha(0.26)),
            Self::Level5 => crate::BoxShadow::drop(0.0, 8.0, 20.0, 0.0, ambient_tint.with_alpha(0.30)),
        }
    }
}

/// Material 3 complete Color Roles generated dynamically via Material You seed algorithm.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Material3Tokens {
    pub primary: Color,
    pub on_primary: Color,
    pub primary_container: Color,
    pub on_primary_container: Color,
    pub secondary: Color,
    pub on_secondary: Color,
    pub secondary_container: Color,
    pub on_secondary_container: Color,
    pub tertiary: Color,
    pub on_tertiary: Color,
    pub surface: Color,
    pub on_surface: Color,
    pub surface_variant: Color,
    pub on_surface_variant: Color,
    pub surface_container: Color,
    pub outline: Color,
    pub outline_variant: Color,
    pub error: Color,
    pub on_error: Color,
    pub is_dark: bool,
}

impl Material3Tokens {
    /// Generate a complete Material 3 dynamic color scheme from a single seed color.
    pub fn from_seed(seed: Color, dark: bool) -> Self {
        let (r, g, b) = (seed.r, seed.g, seed.b);
        if dark {
            Self {
                primary: seed.lerp(Color::WHITE, 0.45),
                on_primary: seed.lerp(Color::BLACK, 0.85),
                primary_container: seed.lerp(Color::BLACK, 0.55),
                on_primary_container: seed.lerp(Color::WHITE, 0.8),
                secondary: Color::rgb(r.saturating_add(20), g.saturating_add(20), b.saturating_add(30)).lerp(Color::WHITE, 0.35),
                on_secondary: Color::BLACK,
                secondary_container: seed.lerp(Color::rgb(20, 25, 35), 0.6),
                on_secondary_container: Color::rgb(220, 230, 245),
                tertiary: Color::rgb(b, r, g).lerp(Color::WHITE, 0.4),
                on_tertiary: Color::BLACK,
                surface: Color::rgb(15, 20, 30),
                on_surface: Color::rgb(235, 240, 250),
                surface_variant: Color::rgb(35, 45, 60),
                on_surface_variant: Color::rgb(180, 195, 215),
                surface_container: Color::rgb(25, 32, 45),
                outline: Color::rgb(90, 105, 125),
                outline_variant: Color::rgb(55, 65, 80),
                error: Color::rgb(255, 180, 171),
                on_error: Color::rgb(105, 0, 5),
                is_dark: true,
            }
        } else {
            Self {
                primary: seed,
                on_primary: Color::WHITE,
                primary_container: seed.lerp(Color::WHITE, 0.75),
                on_primary_container: seed.lerp(Color::BLACK, 0.7),
                secondary: Color::rgb(r.saturating_add(15), g.saturating_add(15), b.saturating_add(20)),
                on_secondary: Color::WHITE,
                secondary_container: seed.lerp(Color::WHITE, 0.82),
                on_secondary_container: Color::rgb(20, 30, 50),
                tertiary: Color::rgb(b, r, g),
                on_tertiary: Color::WHITE,
                surface: Color::rgb(250, 252, 255),
                on_surface: Color::rgb(25, 28, 32),
                surface_variant: Color::rgb(225, 230, 240),
                on_surface_variant: Color::rgb(70, 75, 85),
                surface_container: Color::rgb(240, 244, 250),
                outline: Color::rgb(120, 125, 135),
                outline_variant: Color::rgb(195, 200, 210),
                error: Color::rgb(186, 26, 26),
                on_error: Color::WHITE,
                is_dark: false,
            }
        }
    }

    /// Convert this Material 3 palette to standard CraftUI ThemeTokens.
    pub fn to_theme_tokens(&self) -> ThemeTokens {
        ThemeTokens {
            mode: if self.is_dark { ThemeMode::Dark } else { ThemeMode::Light },
            background: self.surface,
            surface: self.surface_container,
            surface_variant: self.surface_variant,
            border: self.outline_variant,
            primary: self.primary,
            on_primary: self.on_primary,
            text_primary: self.on_surface,
            text_secondary: self.on_surface_variant,
            danger: self.error,
            radius_md: 16.0,
            spacing_unit: 8.0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn theme_override_applies_built_in_and_custom_tokens() {
        let ov = ThemeOverride::new()
            .with_primary(Color::rgb(124, 58, 237), Color::WHITE)
            .with_color("warning", Color::rgb(245, 158, 11))
            .with_float("card_elevation", 6.0);
        let themed = ThemeTokens::light().applied(&ov);
        assert_eq!(themed.primary, Color::rgb(124, 58, 237));
        assert_eq!(ov.color("warning"), Some(Color::rgb(245, 158, 11)));
        assert_eq!(ov.float("card_elevation"), Some(6.0));
        assert_eq!(themed.token_color("primary"), Some(themed.primary));
    }

    #[test]
    fn material3_seed_palette_generation() {
        let seed = Color::rgb(103, 80, 164); // Standard Material 3 purple seed

        let light = Material3Tokens::from_seed(seed, false);
        let dark = Material3Tokens::from_seed(seed, true);

        // Dark mode primary should be lighter (tone 80) than light mode primary (tone 40)
        let light_brightness = (light.primary.r as u32 + light.primary.g as u32 + light.primary.b as u32) / 3;
        let dark_brightness = (dark.primary.r as u32 + dark.primary.g as u32 + dark.primary.b as u32) / 3;
        assert!(dark_brightness > light_brightness, "M3 dark mode primary is higher tone than light mode");

        // Surface tones
        assert_eq!(dark.surface, Color::rgb(15, 20, 30));
        assert_eq!(light.surface, Color::rgb(250, 252, 255));

        // High contrast on_primary
        assert_eq!(light.on_primary, Color::WHITE);
        assert_eq!(dark.on_primary, seed.lerp(Color::BLACK, 0.85));
    }

    #[test]
    fn material3_elevations_produce_correct_shadows() {
        let tint = Color::rgba(0, 0, 0, 40);
        let s0 = M3Elevation::Level0.to_shadow(tint);
        assert_eq!(s0.blur_radius, 0.0);
        assert_eq!(s0.offset_y, 0.0);

        let s1 = M3Elevation::Level1.to_shadow(tint);
        let s5 = M3Elevation::Level5.to_shadow(tint);

        assert!(s5.blur_radius > s1.blur_radius);
        assert!(s5.offset_y > s1.offset_y);
    }

    #[test]
    fn material3_theme_tokens_generation() {
        let seed = Color::rgb(0, 128, 255);
        let tokens = ThemeTokens::material3(seed, ThemeMode::Dark);

        assert_eq!(tokens.background, Color::rgb(15, 20, 30));
        assert_eq!(tokens.radius_md, 16.0);
        assert_eq!(tokens.spacing_unit, 8.0);
    }
}
