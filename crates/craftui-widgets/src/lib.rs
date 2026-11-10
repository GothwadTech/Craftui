//! Retained widget tree, standard widget primitives, gestures, animations, router, and inspector.

pub mod anim;
pub mod gesture;
pub mod inspector;
pub mod router;
pub mod tree;
pub mod widgets;

pub use anim::{Easing, Spring, Tween};
pub use gesture::{GestureArena, RecognizedGesture, ScrollPhysics, VelocityTracker};
pub use inspector::InspectorState;
pub use router::Router;
pub use tree::{VirtualItemFactory, WidgetNode, WidgetTree};
pub use widgets::{
    BoxStyle, CanvasPaintFn, CanvasPointerFn, ReactiveValue, TapHandler, TextCallback, WidgetKind,
};

/// Composable declarative view mountable onto a [`WidgetTree`].
pub trait View {
    fn build(self, cx: craftui_reactive::Scope, tree: &mut WidgetTree) -> craftui_core::NodeId;
}

impl View for craftui_core::NodeId {
    fn build(self, _cx: craftui_reactive::Scope, _tree: &mut WidgetTree) -> craftui_core::NodeId {
        self
    }
}

/// Closure-backed [`View`] returned by `cui!` macro invocations and `#[component]` functions.
pub struct ViewFn<F>(pub F);

impl<F> ViewFn<F> {
    pub fn new(f: F) -> Self {
        Self(f)
    }
}

impl<F> View for ViewFn<F>
where
    F: FnOnce(craftui_reactive::Scope, &mut WidgetTree) -> craftui_core::NodeId,
{
    fn build(self, cx: craftui_reactive::Scope, tree: &mut WidgetTree) -> craftui_core::NodeId {
        (self.0)(cx, tree)
    }
}

impl WidgetTree {
    pub fn mount(&mut self, cx: craftui_reactive::Scope, view: impl View) -> craftui_core::NodeId {
        let id = view.build(cx, self);
        self.set_root(id);
        id
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use craftui_core::{
        Axis, KeyCode, KeyEvent, Length, Modifiers, PointerButton, PointerEvent, PointerId,
        PointerPhase, Size, ThemeMode, UiEvent, WindowMetrics,
    };
    use craftui_layout::LayoutStyle;
    use craftui_reactive::{Scope, reset_runtime};
    use craftui_render::{BackendKind, PaintScene, TextureRegistry, render_scene_to_rgba};
    use craftui_text::TextStyle;
    use std::cell::Cell;
    use std::rc::Rc;

    #[test]
    fn button_click_updates_signal_and_repaints() {
        reset_runtime();
        let cx = Scope::root();
        let count = cx.signal(0i32);
        let mut tree = WidgetTree::new(cx);

        let col = tree.column(cx);
        tree.set_layout(
            col,
            LayoutStyle::column().with_size(Length::Dp(300.0), Length::Dp(200.0)),
        );
        let label = tree.text(
            cx,
            move || format!("Count: {}", count.get()),
            TextStyle::default().with_size(18.0),
        );
        let btn = tree.button(cx, "Increment", move || {
            count.update(|c| *c += 1);
        });
        tree.append_child(col, label);
        tree.append_child(col, btn);
        tree.set_root(col);

        let metrics = WindowMetrics {
            logical_size: Size::new(300.0, 200.0),
            ..Default::default()
        };
        tree.layout(metrics);
        let btn_center = tree.rect(btn).center();

        tree.handle_event(&UiEvent::Pointer(PointerEvent {
            id: PointerId(1),
            phase: PointerPhase::Down,
            position: btn_center,
            button: Some(PointerButton::Primary),
            modifiers: Modifiers::default(),
            is_touch: false,
            timestamp_ms: 10,
        }));
        tree.handle_event(&UiEvent::Pointer(PointerEvent {
            id: PointerId(1),
            phase: PointerPhase::Up,
            position: btn_center,
            button: Some(PointerButton::Primary),
            modifiers: Modifiers::default(),
            is_touch: false,
            timestamp_ms: 40,
        }));

        assert_eq!(count.get(), 1);
        let dirty = tree.layout(metrics);
        assert!(dirty > 0);
        let mut scene = PaintScene::new();
        tree.paint(&mut scene);
        let textures = TextureRegistry::new();
        let rgba = render_scene_to_rgba(
            BackendKind::VelloCpu,
            &scene,
            300,
            200,
            1.0,
            tree.theme().background,
            &textures,
            &mut tree.text_engine,
        );
        assert_eq!(rgba.len(), 300 * 200 * 4);
    }

    #[test]
    fn virtual_list_recycles_10000_items_windowed() {
        reset_runtime();
        let cx = Scope::root();
        let mut tree = WidgetTree::new(cx);
        let built = Rc::new(Cell::new(0usize));
        let built_c = built.clone();

        let vlist = tree.virtual_list(
            cx,
            Axis::Vertical,
            1,
            40.0,
            10_000usize,
            LayoutStyle::column().with_size(Length::Dp(300.0), Length::Dp(200.0)),
            Rc::new(move |scope, idx, t| {
                built_c.set(built_c.get() + 1);
                t.text(scope, format!("Item {idx}"), TextStyle::default())
            }),
        );
        tree.set_root(vlist);
        tree.layout(WindowMetrics {
            logical_size: Size::new(300.0, 200.0),
            ..Default::default()
        });

        // 200px viewport / 40px row = 5 visible rows + overscan -> far less than 10,000!
        assert!(built.get() > 0 && built.get() < 20);
        assert!(tree.nodes.len() < 25);
    }

    #[test]
    fn text_input_and_slider_and_theme_interpolation() {
        reset_runtime();
        let cx = Scope::root();
        let val = cx.signal(0.0f32);
        let text_sig = cx.signal(String::new());
        let mut tree = WidgetTree::new(cx);
        let col = tree.column(cx);
        let input = tree.text_input(cx, "", "Search...", Some(text_sig));
        let slider = tree.slider(cx, val, -100.0, 100.0, "Brightness");
        tree.append_child(col, input);
        tree.append_child(col, slider);
        tree.set_root(col);
        tree.layout(WindowMetrics {
            logical_size: Size::new(400.0, 200.0),
            ..Default::default()
        });

        let ic = tree.rect(input).center();
        tree.handle_event(&UiEvent::Pointer(PointerEvent {
            id: PointerId(1),
            phase: PointerPhase::Down,
            position: ic,
            button: Some(PointerButton::Primary),
            modifiers: Modifiers::default(),
            is_touch: false,
            timestamp_ms: 0,
        }));
        tree.handle_event(&UiEvent::Pointer(PointerEvent {
            id: PointerId(1),
            phase: PointerPhase::Up,
            position: ic,
            button: Some(PointerButton::Primary),
            modifiers: Modifiers::default(),
            is_touch: false,
            timestamp_ms: 10,
        }));
        tree.handle_event(&UiEvent::Key(KeyEvent {
            code: KeyCode::Character("A".into()),
            text: Some("A".into()),
            pressed: true,
            repeat: false,
            modifiers: Modifiers::default(),
        }));
        assert_eq!(text_sig.get(), "A");

        tree.set_theme_mode(ThemeMode::Light);
        for _ in 0..120 {
            if !tree.tick_animations(1.0 / 60.0) {
                break;
            }
        }
        assert_eq!(tree.theme().mode, ThemeMode::Light);
    }
}
