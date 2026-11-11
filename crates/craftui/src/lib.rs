//! Unified public facade crate for the CraftUI pure-Rust declarative UI framework.
//!
//! Typical applications import `use craftui::prelude::*;` and use either `cui!` + `#[component]`
//! in pure Rust or `.cui` templates compiled AOT via `build.rs` and hot-reloaded in dev.

#[cfg(test)]
extern crate self as craftui;

pub use craftui_a11y as a11y;
pub use craftui_core as core;
#[cfg(feature = "dsl")]
pub use craftui_dsl as dsl;
pub use craftui_layout as layout;
pub use craftui_macros::{component, cui};
pub use craftui_reactive as reactive;
pub use craftui_render as render;
#[cfg(feature = "hot-reload")]
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
    pub use craftui_a11y::{A11yMetadata, A11yRole};
    pub use craftui_core::{
        Alignment, Axis, BlendMode, Border, BoxShadow, Breakpoint, Brush, ClipboardBackend, Color,
        Constraints, CornerRadii, CursorIcon, Direction, DirtyFlags, FileDialogBackend,
        FileDialogRequest, FileFilter, Fit, ImeEvent, Insets, Justify, KeyCode, KeyEvent, Length,
        Modifiers, NodeId, PlatformServices, PlatformViewId, Point, PointerButton, PointerEvent,
        PointerId, PointerPhase, Rect, ScrollDelta, Size, TextureId, ThemeMode, ThemeOverride,
        ThemeTokens, Transform2D, UiDispatcher, UiEvent, WindowMetrics, monotonic_ms,
    };
    pub use craftui_layout::{LayoutMode, LayoutStyle};
    pub use craftui_macros::{component, cui};
    pub use craftui_reactive::{
        Computed, Effect, ReadSignal, Scope, Signal, TaskHandle, batch, next_timer_delay_ms,
        tick_timers, tick_timers_at, untrack,
    };
    pub use craftui_render::{
        BackendKind, PaintCommand, PaintScene, PixelBufferHandle, TextureHandle, TextureRegistry,
        kurbo, render_scene_to_rgba,
    };
    pub use craftui_text::{TextAlign, TextEditorState, TextStyle};
    pub use craftui_widgets::{
        AnimatableProp, AnimatedColor, AnimatedFloat, BoolCallback, BoxStyle, CanvasPaintFn,
        CanvasPointerFn, CustomWidget, Easing, FloatCallback, GestureArena, HoverCallback,
        IndexCallback, InspectorState, PinchRotateCallback, ReactiveValue, RecognizedGesture,
        Router, RouterTransition, ScrollPhysics, Spring, TapHandler, TextCallback,
        TransitionCurve, Tween, View, ViewFn, VirtualItemFactory, WidgetKind, WidgetTree,
    };

    #[cfg(feature = "desktop")]
    pub use craftui_platform_desktop::{
        AppContext, DesktopClipboard, DesktopFileDialog, WindowConfig, WindowLauncher, run_app,
    };
    #[cfg(all(feature = "desktop", feature = "hot-reload"))]
    pub use craftui_platform_desktop::run_hot_reload_cui;
}

#[cfg(test)]
mod tests {
    use super::prelude::*;
    use craftui_reactive::reset_runtime;
    use std::cell::Cell;
    use std::rc::Rc;

    #[component]
    fn sample_panel(cx: Scope, count: Signal<i32>) -> impl View {
        let name = cx.signal("World".to_string());
        let active_tab = cx.signal(0usize);
        let agreed = cx.signal(true);
        let volume = cx.signal(0.4f32);
        let choice = cx.signal(1usize);
        let notes = cx.signal("Line 1".to_string());
        let modal_open = cx.signal(false);

        cui! {
            Column(
                width: pct(100),
                height: pct(100),
                bg: theme.background,
                border_width: 1,
                border_color: theme.border,
                padding: 16,
                gap: 10
            ) {
                Row(width: pct(100), align: Alignment::Center, gap: 8) {
                    Icon(svg: "check", size: 20, color: theme.primary)
                    Text(text: move || format!("Count = {}", count.get()), size: 20, color: theme.text_primary)
                    Tooltip(text: "Interactive count badge")
                }
                Divider(axis: Axis::Horizontal, thickness: 1, color: theme.border)
                Tabs(tabs: vec!["General".into(), "Preview".into()], active: active_tab)
                Row(width: pct(100), gap: 8) {
                    TextInput(placeholder: "Name", value: name)
                    Button(
                        label: "Add",
                        bg: theme.primary,
                        fg: theme.on_primary,
                        on_tap: move || count.update(|v| *v += 1)
                    )
                }
                TextArea(placeholder: "Notes", value: notes)
                Row(width: pct(100), gap: 12) {
                    Checkbox(checked: agreed, label: "Agree")
                    Switch(checked: agreed, label: "Notify")
                    Select(options: vec!["Alpha".into(), "Beta".into()], selected: choice)
                }
                RadioGroup(options: vec!["Compact".into(), "Comfortable".into()], selected: choice)
                Slider(value: volume, min: 0.0, max: 1.0, label: "Volume")
                ProgressIndicator(progress: Some(0.65))
                Modal(open: modal_open, on_close: move || modal_open.set(false)) {
                    Text(text: "Inside Modal", color: theme.text_primary)
                }
            }
        }
    }

    #[test]
    fn facade_cui_macro_supports_all_widgets_and_live_theme_switch() {
        reset_runtime();
        let cx = Scope::root();
        let count = cx.signal(0);
        let mut tree = WidgetTree::new(cx);
        tree.mount(cx, sample_panel(cx, count));

        let metrics = WindowMetrics {
            logical_size: Size::new(480.0, 640.0),
            scale_factor: 1.0,
            ..Default::default()
        };
        tree.layout(metrics);
        let mut scene = PaintScene::new();
        tree.paint(&mut scene);

        let textures = TextureRegistry::new();
        let dark_rgba = render_scene_to_rgba(
            BackendKind::Auto,
            &scene,
            480,
            640,
            1.0,
            tree.theme().background,
            &textures,
            &mut tree.text_engine,
        );
        assert_eq!(dark_rgba.len(), 480 * 640 * 4);
        assert!(dark_rgba.iter().any(|&b| b > 20));

        // Verify reactive `Theme.*` tokens in `cui!` respond live to ThemeMode + ThemeOverride changes
        tree.set_theme_mode(ThemeMode::Light);
        for _ in 0..240 {
            if !tree.tick_animations(0.016) {
                break;
            }
        }
        assert_eq!(tree.theme().mode, ThemeMode::Light);

        let mut light_scene = PaintScene::new();
        tree.paint(&mut light_scene);
        let light_rgba = render_scene_to_rgba(
            BackendKind::Auto,
            &light_scene,
            480,
            640,
            1.0,
            tree.theme().background,
            &textures,
            &mut tree.text_engine,
        );
        assert_ne!(dark_rgba, light_rgba);

        // Verify Scope timer hooks cleanly fire when ticked
        let ticks = Rc::new(Cell::new(0usize));
        let t_c = ticks.clone();
        let handle = cx.interval_at(100, 50, move || t_c.set(t_c.get() + 1));
        tick_timers_at(150);
        tick_timers_at(200);
        assert_eq!(ticks.get(), 2);
        handle.cancel();
        tick_timers_at(300);
        assert_eq!(ticks.get(), 2);
    }
}
