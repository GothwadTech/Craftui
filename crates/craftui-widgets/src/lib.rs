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
    BoolCallback, BoxStyle, CanvasPaintFn, CanvasPointerFn, CustomWidget, FloatCallback,
    HoverCallback, IndexCallback, PinchRotateCallback, ReactiveValue, TapHandler, TextCallback,
    WidgetKind,
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
    use std::cell::{Cell, RefCell};
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

    struct CounterBadgeWidget {
        hits: Rc<Cell<usize>>,
    }

    impl CustomWidget for CounterBadgeWidget {
        fn debug_name(&self) -> &'static str {
            "CounterBadgeWidget"
        }

        fn measure(
            &mut self,
            _constraints: craftui_core::Constraints,
            _text_engine: &mut craftui_text::TextEngine,
            _theme: &craftui_core::ThemeTokens,
        ) -> Size {
            Size::new(90.0, 32.0)
        }

        fn paint(
            &mut self,
            rect: craftui_core::Rect,
            scene: &mut PaintScene,
            _text_engine: &mut craftui_text::TextEngine,
            theme: &craftui_core::ThemeTokens,
            focused: bool,
        ) {
            scene.fill_rect(
                rect,
                if focused {
                    theme.primary
                } else {
                    theme.surface_variant
                },
                craftui_core::CornerRadii::all(8.0),
            );
        }

        fn handle_event(&mut self, _rect: craftui_core::Rect, event: &UiEvent) -> bool {
            if let UiEvent::Pointer(pe) = event
                && matches!(pe.phase, PointerPhase::Down)
            {
                self.hits.set(self.hits.get() + 1);
                return true;
            }
            false
        }
    }

    #[test]
    fn custom_widget_new_controls_gestures_and_keyboard_traversal() {
        reset_runtime();
        let cx = Scope::root();
        let mut tree = WidgetTree::new(cx);
        let metrics = WindowMetrics {
            logical_size: Size::new(500.0, 500.0),
            ..Default::default()
        };

        let root = tree.column(cx);
        let tab_idx = cx.signal(0usize);
        let sel_idx = cx.signal(0usize);
        let rad_idx = cx.signal(0usize);
        let area_sig = cx.signal("line1".to_string());
        let modal_open = cx.signal(false);

        let tabs = tree.tabs(cx, vec!["Overview".into(), "Logs".into()], tab_idx);
        let select = tree.select(
            cx,
            vec!["Alpha".into(), "Beta".into(), "Gamma".into()],
            sel_idx,
            "Choose...",
        );
        let radio = tree.radio_group(
            cx,
            vec!["Small".into(), "Large".into()],
            rad_idx,
            Axis::Horizontal,
        );
        let textarea = tree.text_area(cx, "", "Notes...", Some(area_sig));
        let div = tree.divider(cx, Axis::Horizontal, 2.0);
        let ico = tree.icon(cx, "check", 24.0, None);
        let badge_hits = Rc::new(Cell::new(0usize));
        let badge = tree.custom_widget(
            cx,
            CounterBadgeWidget {
                hits: badge_hits.clone(),
            },
            LayoutStyle::row(),
        );
        let modal = tree.overlay(
            cx,
            modal_open,
            false,
            Some(Rc::new(RefCell::new(move || modal_open.set(false)))),
        );
        let modal_btn = tree.button(cx, "Confirm", move || modal_open.set(false));
        tree.append_child(modal, modal_btn);

        for child in [tabs, select, radio, textarea, div, ico, badge, modal] {
            tree.append_child(root, child);
        }
        tree.set_root(root);
        tree.layout(metrics);

        // Verify gestures: DoubleTap, LongPress, Hover, PinchRotate
        let dbl_hits = Rc::new(Cell::new(0usize));
        let dbl_c = dbl_hits.clone();
        tree.set_on_double_tap(badge, move || dbl_c.set(dbl_c.get() + 1));
        let lp_hits = Rc::new(Cell::new(0usize));
        let lp_c = lp_hits.clone();
        tree.set_on_long_press(badge, move || lp_c.set(lp_c.get() + 1));
        let hovered = Rc::new(Cell::new(false));
        let hov_c = hovered.clone();
        tree.set_on_hover(badge, move |h| hov_c.set(h));
        let pinched = Rc::new(Cell::new(false));
        let pin_c = pinched.clone();
        tree.set_on_pinch_rotate(root, move |_, _, _| pin_c.set(true));

        let bc = tree.rect(badge).center();
        let make_pt = |phase, ts| PointerEvent {
            id: PointerId(1),
            phase,
            position: bc,
            button: Some(PointerButton::Primary),
            modifiers: Modifiers::default(),
            is_touch: true,
            timestamp_ms: ts,
        };
        tree.handle_event(&UiEvent::Pointer(make_pt(PointerPhase::Down, 10)));
        tree.handle_event(&UiEvent::Pointer(make_pt(PointerPhase::Up, 30)));
        tree.handle_event(&UiEvent::Pointer(make_pt(PointerPhase::Down, 80)));
        tree.handle_event(&UiEvent::Pointer(make_pt(PointerPhase::Up, 110)));
        assert_eq!(badge_hits.get(), 2);
        assert_eq!(dbl_hits.get(), 1);
        assert!(hovered.get());

        // LongPress
        tree.handle_event(&UiEvent::Pointer(make_pt(PointerPhase::Down, 500)));
        tree.handle_event(&UiEvent::Pointer(make_pt(PointerPhase::Move, 1100)));
        tree.handle_event(&UiEvent::Pointer(make_pt(PointerPhase::Up, 1150)));
        assert_eq!(lp_hits.get(), 1);

        // Pinch
        tree.handle_event(&UiEvent::Pinch {
            factor: 1.25,
            center: bc,
        });
        assert!(pinched.get());

        // Tab / Shift+Tab focus traversal
        let key_evt = |code, shift| KeyEvent {
            code,
            text: None,
            pressed: true,
            repeat: false,
            modifiers: Modifiers {
                shift,
                ..Default::default()
            },
        };
        tree.focused = None;
        tree.handle_event(&UiEvent::Key(key_evt(KeyCode::Tab, false)));
        assert_eq!(tree.focused, Some(tabs));
        tree.handle_event(&UiEvent::Key(key_evt(KeyCode::ArrowRight, false)));
        assert_eq!(tab_idx.get(), 1);
        tree.handle_event(&UiEvent::Key(key_evt(KeyCode::Tab, false)));
        assert_eq!(tree.focused, Some(select));
        tree.handle_event(&UiEvent::Key(key_evt(KeyCode::Tab, false)));
        assert_eq!(tree.focused, Some(radio));
        tree.handle_event(&UiEvent::Key(key_evt(KeyCode::Tab, false)));
        assert_eq!(tree.focused, Some(textarea));

        // TextArea newline with Enter + Up/Down
        tree.handle_event(&UiEvent::Key(key_evt(KeyCode::Enter, false)));
        tree.handle_event(&UiEvent::Key(KeyEvent {
            code: KeyCode::Character("B".into()),
            text: Some("B".into()),
            pressed: true,
            repeat: false,
            modifiers: Modifiers::default(),
        }));
        assert_eq!(area_sig.get(), "line1\nB");

        // Clipboard via PlatformServices
        tree.services.write_clipboard("+C");
        tree.handle_event(&UiEvent::Key(KeyEvent {
            code: KeyCode::KeyV,
            text: None,
            pressed: true,
            repeat: false,
            modifiers: Modifiers {
                ctrl: true,
                ..Default::default()
            },
        }));
        assert_eq!(area_sig.get(), "line1\nB+C");

        // Modal open traps Tab focus & Escape closes it
        modal_open.set(true);
        tree.layout(metrics);
        tree.handle_event(&UiEvent::Key(key_evt(KeyCode::Tab, false)));
        assert_eq!(tree.focused, Some(modal_btn));
        tree.handle_event(&UiEvent::Key(key_evt(KeyCode::Escape, false)));
        assert!(!modal_open.get());

        // Render full scene verifying SVG Icon, CustomWidget, Select, Radio, Tabs, Divider all paint
        let mut scene = PaintScene::new();
        tree.paint(&mut scene);
        let textures = TextureRegistry::new();
        let rgba = render_scene_to_rgba(
            BackendKind::VelloCpu,
            &scene,
            500,
            500,
            1.0,
            tree.theme().background,
            &textures,
            &mut tree.text_engine,
        );
        assert_eq!(rgba.len(), 500 * 500 * 4);
    }

    #[test]
    fn reactive_for_reuses_keyed_nodes_on_reorder() {
        reset_runtime();
        let cx = Scope::root();
        let mut tree = WidgetTree::new(cx);
        let list_sig = cx.signal(vec![(1u32, "One"), (2u32, "Two"), (3u32, "Three")]);
        let build_count = Rc::new(Cell::new(0usize));
        let bc = build_count.clone();

        let host = tree.reactive_for(
            cx,
            LayoutStyle::column(),
            move || list_sig.get(),
            |_, (k, _)| *k,
            move |scope, (_k, lbl), _, t| {
                bc.set(bc.get() + 1);
                t.text(scope, lbl, TextStyle::default())
            },
        );
        tree.set_root(host);
        let metrics = WindowMetrics::default();
        tree.layout(metrics);
        assert_eq!(build_count.get(), 3);
        let initial_children = tree.nodes.get(host).unwrap().children.clone();

        // Reorder and drop #2, add #4: #1 and #3 keep exact same NodeId, only #4 built!
        list_sig.set(vec![(3u32, "Three"), (1u32, "One"), (4u32, "Four")]);
        tree.layout(metrics);
        assert_eq!(build_count.get(), 4);
        let new_children = tree.nodes.get(host).unwrap().children.clone();
        assert_eq!(new_children[0], initial_children[2]);
        assert_eq!(new_children[1], initial_children[0]);
        assert!(!tree.nodes.contains_key(initial_children[1]));
    }
}
