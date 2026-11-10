//! Unified public facade crate for the CraftUI pure-Rust declarative UI framework.
//!
//! Typical applications import `use craftui::prelude::*;` and use either `cui!` + `#[component]`
//! in pure Rust or `.cui` templates compiled AOT via `build.rs` and hot-reloaded in dev.

#[cfg(test)]
extern crate self as craftui;

pub use craftui_a11y as a11y;
pub use craftui_core as core;
pub use craftui_dsl as dsl;
pub use craftui_layout as layout;
pub use craftui_macros::{component, cui};
pub use craftui_reactive as reactive;
pub use craftui_render as render;
pub use craftui_runtime as runtime;
pub use craftui_text as text;
pub use craftui_widgets as widgets;

#[cfg(feature = "desktop")]
pub use craftui_platform_desktop as platform_desktop;

#[cfg(feature = "android")]
pub use craftui_platform_android as platform_android;

#[cfg(feature = "ios")]
pub use craftui_platform_ios as platform_ios;

#[cfg(feature = "web")]
pub use craftui_platform_web as platform_web;

/// Numeric conversion helper enabling ergonomic `pct(100)`, `dp(16)`, `px(8)` in `cui!` calls.
pub trait Scalar: Copy {
    fn into_f32(self) -> f32;
}

impl Scalar for f32 {
    #[inline]
    fn into_f32(self) -> f32 {
        self
    }
}
impl Scalar for f64 {
    #[inline]
    fn into_f32(self) -> f32 {
        self as f32
    }
}
impl Scalar for i32 {
    #[inline]
    fn into_f32(self) -> f32 {
        self as f32
    }
}
impl Scalar for u32 {
    #[inline]
    fn into_f32(self) -> f32 {
        self as f32
    }
}
impl Scalar for usize {
    #[inline]
    fn into_f32(self) -> f32 {
        self as f32
    }
}

#[inline]
pub fn dp(v: impl Scalar) -> craftui_core::Length {
    craftui_core::Length::Dp(v.into_f32())
}

#[inline]
pub fn px(v: impl Scalar) -> craftui_core::Length {
    craftui_core::Length::Px(v.into_f32())
}

#[inline]
pub fn pct(v: impl Scalar) -> craftui_core::Length {
    craftui_core::Length::Pct(v.into_f32())
}

#[inline]
pub const fn auto() -> craftui_core::Length {
    craftui_core::Length::Auto
}

pub mod prelude {
    pub use crate::{auto, dp, pct, px};
    pub use craftui_core::{
        Alignment, Axis, Border, BoxShadow, Breakpoint, Brush, Color, Constraints, CornerRadii,
        Direction, DirtyFlags, Fit, ImeEvent, Insets, Justify, KeyCode, KeyEvent, Length,
        Modifiers, NodeId, PlatformViewId, Point, PointerButton, PointerEvent, PointerId,
        PointerPhase, Rect, ScrollDelta, Size, TextureId, ThemeMode, ThemeTokens, UiDispatcher,
        UiEvent, WindowMetrics,
    };
    pub use craftui_layout::{LayoutMode, LayoutStyle};
    pub use craftui_macros::{component, cui};
    pub use craftui_reactive::{Computed, Effect, ReadSignal, Scope, Signal, batch, untrack};
    pub use craftui_render::{
        BackendKind, PaintCommand, PaintScene, PixelBufferHandle, TextureHandle, TextureRegistry,
        kurbo, render_scene_to_rgba,
    };
    pub use craftui_text::{TextAlign, TextEditorState, TextStyle};
    pub use craftui_widgets::{
        BoxStyle, Easing, GestureArena, InspectorState, ReactiveValue, RecognizedGesture, Router,
        ScrollPhysics, Spring, Tween, View, ViewFn, WidgetKind, WidgetTree,
    };

    #[cfg(feature = "desktop")]
    pub use craftui_platform_desktop::{AppContext, WindowConfig, run_app, run_hot_reload_cui};
}

#[cfg(test)]
mod tests {
    use super::prelude::*;
    use craftui_reactive::reset_runtime;

    #[component]
    fn sample_panel(cx: Scope, count: Signal<i32>) -> impl View {
        cui! {
            Column(width: pct(100), height: pct(100), bg: theme.background, padding: 16, gap: 12) {
                Text(text: move || format!("Count = {}", count.get()), size: 22)
                Button(
                    label: "Add",
                    bg: theme.primary,
                    on_tap: move || count.update(|v| *v += 1)
                )
            }
        }
    }

    #[test]
    fn facade_cui_macro_and_component_render_snapshot() {
        reset_runtime();
        let cx = Scope::root();
        let count = cx.signal(0);
        let mut tree = WidgetTree::new(cx);
        tree.mount(cx, sample_panel(cx, count));

        let metrics = WindowMetrics {
            logical_size: Size::new(320.0, 200.0),
            scale_factor: 1.0,
            ..Default::default()
        };
        tree.layout(metrics);
        let mut scene = PaintScene::new();
        tree.paint(&mut scene);

        let textures = TextureRegistry::new();
        let rgba = render_scene_to_rgba(
            BackendKind::VelloCpu,
            &scene,
            320,
            200,
            1.0,
            tree.theme().background,
            &textures,
            &mut tree.text_engine,
        );
        assert_eq!(rgba.len(), 320 * 200 * 4);
        assert!(rgba.iter().any(|&b| b > 20));
    }
}
