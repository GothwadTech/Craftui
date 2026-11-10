use crate::anim::Spring;
use crate::gesture::{GestureArena, RecognizedGesture, ScrollPhysics};
use crate::inspector::InspectorState;
use crate::widgets::{BoxStyle, CanvasPointerFn, ReactiveValue, TapHandler, WidgetKind};
use craftui_a11y::{A11yAction, A11yBridge, A11yMetadata, A11yRole, A11ySnapshotNode};
use craftui_core::{
    Alignment, Axis, Brush, Color, CornerRadii, Direction, DirtyFlags, Fit, Insets, KeyCode,
    KeyEvent, Length, NodeId, PlatformViewId, Point, PointerEvent, Rect, Size, TextureId,
    ThemeMode, ThemeTokens, UiEvent, WindowMetrics,
};
use craftui_layout::{LayoutEngine, LayoutStyle};
use craftui_reactive::{Scope, Signal, drain_dirty_nodes, mark_node_dirty};
use craftui_render::{PaintCommand, PaintScene};
use craftui_text::{TextEditorState, TextEngine, TextStyle};
use slotmap::{SecondaryMap, SlotMap};
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

/// Single node in the retained UI tree.
pub struct WidgetNode {
    pub id: NodeId,
    pub debug_name: &'static str,
    pub kind: WidgetKind,
    pub layout: LayoutStyle,
    pub box_style: BoxStyle,
    pub a11y: A11yMetadata,
    pub parent: Option<NodeId>,
    pub children: Vec<NodeId>,
    pub scope: Scope,
    pub dirty: DirtyFlags,
    pub on_tap: Option<TapHandler>,
    /// Optional reactive visibility predicate evaluated during layout.
    pub visible_when: Option<ReactiveValue<bool>>,
    cached_text: String,
    cached_style: TextStyle,
}

impl WidgetNode {
    fn new(
        id: NodeId,
        debug_name: &'static str,
        kind: WidgetKind,
        layout: LayoutStyle,
        scope: Scope,
    ) -> Self {
        Self {
            id,
            debug_name,
            kind,
            layout,
            box_style: BoxStyle::new(),
            a11y: A11yMetadata::default(),
            parent: None,
            children: Vec::new(),
            scope,
            dirty: DirtyFlags::ALL,
            on_tap: None,
            visible_when: None,
            cached_text: String::new(),
            cached_style: TextStyle::default(),
        }
    }
}

/// Callback type used by `VirtualList` and `VirtualGrid` to materialize visible items lazily.
pub type VirtualItemFactory = Rc<dyn Fn(Scope, usize, &mut WidgetTree) -> NodeId>;

type DynamicSlotSync = Box<dyn FnMut(NodeId, &mut WidgetTree) -> bool>;

struct VirtualListHost {
    factory: VirtualItemFactory,
    active_items: HashMap<usize, (NodeId, Scope)>,
    last_range: Option<(usize, usize)>,
}

/// Central retained widget tree managing layout, text, reactivity, gestures, overlays, and painting.
pub struct WidgetTree {
    pub nodes: SlotMap<NodeId, WidgetNode>,
    pub root: Option<NodeId>,
    pub layout_engine: LayoutEngine,
    pub text_engine: TextEngine,
    pub gesture_arena: GestureArena,
    pub scroll_physics: ScrollPhysics,
    pub a11y_bridge: A11yBridge,
    pub inspector: InspectorState,
    pub root_scope: Scope,
    pub theme_signal: Signal<ThemeTokens>,
    pub direction: Direction,
    pub focused: Option<NodeId>,
    pub hovered: Option<NodeId>,
    pub metrics: WindowMetrics,
    pub clipboard_text: String,
    virtual_hosts: SecondaryMap<NodeId, VirtualListHost>,
    dynamic_hosts: SecondaryMap<NodeId, DynamicSlotSync>,
    theme_spring: Option<(ThemeTokens, ThemeTokens, Spring)>,
    caret_blink_sec: f32,
}

impl WidgetTree {
    pub fn new(root_scope: Scope) -> Self {
        let initial_theme = ThemeTokens::dark();
        let theme_signal = root_scope.signal(initial_theme);
        root_scope.provide_context(theme_signal);
        Self {
            nodes: SlotMap::with_key(),
            root: None,
            layout_engine: LayoutEngine::new(),
            text_engine: TextEngine::new(),
            gesture_arena: GestureArena::new(),
            scroll_physics: ScrollPhysics::default(),
            a11y_bridge: A11yBridge::new("CraftUI App"),
            inspector: InspectorState::default(),
            root_scope,
            theme_signal,
            direction: Direction::Ltr,
            focused: None,
            hovered: None,
            metrics: WindowMetrics::default(),
            clipboard_text: String::new(),
            virtual_hosts: SecondaryMap::new(),
            dynamic_hosts: SecondaryMap::new(),
            theme_spring: None,
            caret_blink_sec: 0.0,
        }
    }

    pub fn theme(&self) -> ThemeTokens {
        self.theme_signal.get()
    }

    /// Animate smoothly between theme palettes (Light, Dark, AMOLED, HighContrast).
    pub fn set_theme_mode(&mut self, mode: ThemeMode) {
        let from = self.theme_signal.get();
        let to = ThemeTokens::from_mode(mode);
        let mut spring = Spring::new(0.0, 260.0, 28.0);
        spring.set_target(1.0);
        self.theme_spring = Some((from, to, spring));
    }

    pub fn set_root(&mut self, root: NodeId) {
        self.root = Some(root);
    }

    pub fn spawn_node(
        &mut self,
        debug_name: &'static str,
        kind: WidgetKind,
        layout: LayoutStyle,
        scope: Scope,
    ) -> NodeId {
        let id = self
            .nodes
            .insert_with_key(|id| WidgetNode::new(id, debug_name, kind, layout.clone(), scope));
        self.layout_engine.upsert_node(id, layout);
        self.setup_reactive_bindings(id);
        id
    }

    fn setup_reactive_bindings(&mut self, id: NodeId) {
        let Some(node) = self.nodes.get(id) else {
            return;
        };
        let scope = node.scope;

        if let Some(bg) = node.box_style.background.clone()
            && bg.is_dynamic()
        {
            scope.effect(move || {
                let _ = bg.get();
                mark_node_dirty(id, DirtyFlags::PAINT);
            });
        }

        match &node.kind {
            WidgetKind::Text { content, style } => {
                let content = content.clone();
                let style = style.clone();
                if content.is_dynamic() || style.is_dynamic() {
                    scope.effect(move || {
                        let _ = content.get();
                        let _ = style.get();
                        mark_node_dirty(
                            id,
                            DirtyFlags::LAYOUT
                                | DirtyFlags::PAINT
                                | DirtyFlags::TEXT
                                | DirtyFlags::A11Y,
                        );
                    });
                }
            }
            WidgetKind::Button {
                label,
                disabled,
                foreground,
                ..
            } => {
                let label = label.clone();
                let disabled = disabled.clone();
                let fg_dyn = foreground.clone().filter(|f| f.is_dynamic());
                if label.is_dynamic() || disabled.is_dynamic() || fg_dyn.is_some() {
                    scope.effect(move || {
                        let _ = label.get();
                        let _ = disabled.get();
                        if let Some(ref f) = fg_dyn {
                            let _ = f.get();
                        }
                        mark_node_dirty(
                            id,
                            DirtyFlags::LAYOUT | DirtyFlags::PAINT | DirtyFlags::A11Y,
                        );
                    });
                }
            }
            WidgetKind::Image { texture, .. } => {
                let texture = texture.clone();
                if texture.is_dynamic() {
                    scope.effect(move || {
                        let _ = texture.get();
                        mark_node_dirty(id, DirtyFlags::PAINT);
                    });
                }
            }
            WidgetKind::VirtualList { item_count, .. } => {
                let count = item_count.clone();
                if count.is_dynamic() {
                    scope.effect(move || {
                        let _ = count.get();
                        mark_node_dirty(id, DirtyFlags::LAYOUT | DirtyFlags::PAINT);
                    });
                }
            }
            WidgetKind::Checkbox { checked, .. } | WidgetKind::Switch { checked, .. } => {
                checked.bind_node(id, DirtyFlags::PAINT | DirtyFlags::A11Y);
            }
            WidgetKind::Slider { value, .. } => {
                value.bind_node(id, DirtyFlags::PAINT | DirtyFlags::A11Y);
            }
            WidgetKind::ProgressIndicator { progress, .. } => {
                let prog = progress.clone();
                if prog.is_dynamic() {
                    scope.effect(move || {
                        let _ = prog.get();
                        mark_node_dirty(id, DirtyFlags::PAINT | DirtyFlags::A11Y);
                    });
                }
            }
            WidgetKind::Overlay { open, .. } => {
                let open = open.clone();
                if open.is_dynamic() {
                    scope.effect(move || {
                        let _ = open.get();
                        mark_node_dirty(
                            id,
                            DirtyFlags::LAYOUT | DirtyFlags::PAINT | DirtyFlags::A11Y,
                        );
                    });
                }
            }
            _ => {}
        }

        // Theme updates re-paint widgets without full layout recalculation (Design Spec §4).
        self.theme_signal.bind_node(id, DirtyFlags::PAINT);
    }

    pub fn append_child(&mut self, parent: NodeId, child: NodeId) {
        if let Some(c) = self.nodes.get_mut(child) {
            c.parent = Some(parent);
        }
        if let Some(p) = self.nodes.get_mut(parent) {
            p.children.push(child);
            p.dirty |= DirtyFlags::LAYOUT | DirtyFlags::PAINT;
            let children = p.children.clone();
            self.layout_engine.set_children(parent, &children);
        }
    }

    pub fn set_children(&mut self, parent: NodeId, children: Vec<NodeId>) {
        for &c in &children {
            if let Some(cn) = self.nodes.get_mut(c) {
                cn.parent = Some(parent);
            }
        }
        if let Some(p) = self.nodes.get_mut(parent) {
            p.children = children.clone();
            p.dirty |= DirtyFlags::LAYOUT | DirtyFlags::PAINT;
        }
        self.layout_engine.set_children(parent, &children);
    }

    /// Remove a subtree cleanly and dispose its reactive scope.
    pub fn remove_subtree(&mut self, node_id: NodeId) {
        if let Some(parent) = self.nodes.get(node_id).and_then(|n| n.parent)
            && let Some(p) = self.nodes.get_mut(parent)
        {
            p.children.retain(|&c| c != node_id);
            let children = p.children.clone();
            self.layout_engine.set_children(parent, &children);
        }
        self.remove_subtree_inner(node_id);
    }

    fn remove_subtree_inner(&mut self, node_id: NodeId) {
        let Some(node) = self.nodes.remove(node_id) else {
            return;
        };
        for child in node.children {
            self.remove_subtree_inner(child);
        }
        self.virtual_hosts.remove(node_id);
        self.dynamic_hosts.remove(node_id);
        self.layout_engine.remove_node(node_id);
        if node.scope != self.root_scope {
            node.scope.dispose();
        }
        if self.focused == Some(node_id) {
            self.focused = None;
        }
        if self.hovered == Some(node_id) {
            self.hovered = None;
        }
    }

    // === Ergonomic Widget Builders ===

    pub fn container(&mut self, scope: Scope, layout: LayoutStyle, style: BoxStyle) -> NodeId {
        let id = self.spawn_node("Container", WidgetKind::Container, layout, scope);
        if let Some(n) = self.nodes.get_mut(id) {
            let is_dyn = style.background.as_ref().is_some_and(|b| b.is_dynamic());
            if let Some(bg) = style.background.clone()
                && is_dyn
            {
                scope.effect(move || {
                    let _ = bg.get();
                    mark_node_dirty(id, DirtyFlags::PAINT);
                });
            }
            n.box_style = style;
        }
        id
    }

    pub fn row(&mut self, scope: Scope) -> NodeId {
        self.container(scope, LayoutStyle::row(), BoxStyle::new())
    }

    pub fn column(&mut self, scope: Scope) -> NodeId {
        self.container(scope, LayoutStyle::column(), BoxStyle::new())
    }

    pub fn grid(&mut self, scope: Scope, columns: u16, gap: f32) -> NodeId {
        self.container(
            scope,
            LayoutStyle::grid(columns).with_gap(gap),
            BoxStyle::new(),
        )
    }

    pub fn stack(&mut self, scope: Scope) -> NodeId {
        self.container(scope, LayoutStyle::stack(), BoxStyle::new())
    }

    pub fn spacer(&mut self, scope: Scope) -> NodeId {
        self.spawn_node(
            "Spacer",
            WidgetKind::Spacer,
            LayoutStyle::column().with_flex_grow(1.0),
            scope,
        )
    }

    pub fn safe_area(&mut self, scope: Scope) -> NodeId {
        self.spawn_node(
            "SafeArea",
            WidgetKind::SafeArea,
            LayoutStyle::column()
                .with_size(Length::Pct(100.0), Length::Pct(100.0))
                .with_safe_area(true),
            scope,
        )
    }

    pub fn text(
        &mut self,
        scope: Scope,
        content: impl Into<ReactiveValue<String>>,
        style: impl Into<ReactiveValue<TextStyle>>,
    ) -> NodeId {
        let id = self.spawn_node(
            "Text",
            WidgetKind::Text {
                content: content.into(),
                style: style.into(),
            },
            LayoutStyle::column(),
            scope,
        );
        if let Some(n) = self.nodes.get_mut(id) {
            n.a11y.role = A11yRole::Label;
        }
        id
    }

    pub fn button(
        &mut self,
        scope: Scope,
        label: impl Into<ReactiveValue<String>>,
        on_tap: impl FnMut() + 'static,
    ) -> NodeId {
        let label = label.into();
        let label_text = label.get();
        let id = self.spawn_node(
            "Button",
            WidgetKind::Button {
                label,
                disabled: ReactiveValue::Static(false),
                foreground: None,
                on_tap: Some(Rc::new(RefCell::new(on_tap))),
                hovered: false,
                pressed: false,
            },
            LayoutStyle::row()
                .with_padding(Insets::symmetric(18.0, 10.0))
                .with_align(Alignment::Center),
            scope,
        );
        if let Some(n) = self.nodes.get_mut(id) {
            n.box_style.radius = CornerRadii::all(10.0);
            n.a11y.role = A11yRole::Button;
            n.a11y.label = Some(label_text);
            n.a11y.focusable = true;
        }
        id
    }

    pub fn image(
        &mut self,
        scope: Scope,
        texture: impl Into<ReactiveValue<Option<TextureId>>>,
        fit: Fit,
        radius: CornerRadii,
    ) -> NodeId {
        let id = self.spawn_node(
            "ImageView",
            WidgetKind::Image {
                texture: texture.into(),
                fit,
                radius,
                opacity: 1.0,
            },
            LayoutStyle::column(),
            scope,
        );
        if let Some(n) = self.nodes.get_mut(id) {
            n.a11y.role = A11yRole::Image;
        }
        id
    }

    pub fn canvas(
        &mut self,
        scope: Scope,
        layout: LayoutStyle,
        on_paint: impl Fn(Rect, &mut PaintScene) + 'static,
        on_pointer: Option<CanvasPointerFn>,
    ) -> NodeId {
        let id = self.spawn_node(
            "Canvas",
            WidgetKind::Canvas {
                on_paint: Rc::new(on_paint),
                on_pointer,
            },
            layout,
            scope,
        );
        if let Some(n) = self.nodes.get_mut(id) {
            n.a11y.role = A11yRole::Canvas;
        }
        id
    }

    pub fn scroll_view(&mut self, scope: Scope, axis: Axis, mut layout: LayoutStyle) -> NodeId {
        layout.clip_overflow = true;
        let id = self.spawn_node(
            "ScrollView",
            WidgetKind::ScrollView {
                axis,
                offset: 0.0,
                velocity: 0.0,
                content_extent: 0.0,
            },
            layout,
            scope,
        );
        if let Some(n) = self.nodes.get_mut(id) {
            n.box_style.clip = true;
            n.a11y.role = A11yRole::ScrollView;
        }
        id
    }

    #[allow(clippy::too_many_arguments)]
    pub fn virtual_list(
        &mut self,
        scope: Scope,
        axis: Axis,
        columns: usize,
        item_extent: f32,
        item_count: impl Into<ReactiveValue<usize>>,
        mut layout: LayoutStyle,
        factory: VirtualItemFactory,
    ) -> NodeId {
        layout.clip_overflow = true;
        let id = self.spawn_node(
            "VirtualList",
            WidgetKind::VirtualList {
                axis,
                columns: columns.max(1),
                item_extent: item_extent.max(8.0),
                item_count: item_count.into(),
                offset: 0.0,
                velocity: 0.0,
            },
            layout,
            scope,
        );
        if let Some(n) = self.nodes.get_mut(id) {
            n.box_style.clip = true;
            n.a11y.role = A11yRole::List;
        }
        self.virtual_hosts.insert(
            id,
            VirtualListHost {
                factory,
                active_items: HashMap::new(),
                last_range: None,
            },
        );
        id
    }

    pub fn text_input(
        &mut self,
        scope: Scope,
        initial: impl Into<String>,
        placeholder: impl Into<String>,
        bound_signal: Option<Signal<String>>,
    ) -> NodeId {
        let initial_s = bound_signal
            .map(|s| s.get_untracked())
            .unwrap_or_else(|| initial.into());
        let id = self.spawn_node(
            "TextInput",
            WidgetKind::TextInput {
                editor: TextEditorState::new(initial_s.clone()),
                placeholder: placeholder.into(),
                style: TextStyle::default().with_size(14.0),
                bound_signal,
                on_change: None,
                on_submit: None,
            },
            LayoutStyle::row()
                .with_height(Length::Dp(40.0))
                .with_padding(Insets::symmetric(12.0, 8.0)),
            scope,
        );
        if let Some(n) = self.nodes.get_mut(id) {
            n.box_style.radius = CornerRadii::all(8.0);
            n.a11y.role = A11yRole::TextInput;
            n.a11y.value = Some(initial_s);
            n.a11y.focusable = true;
        }
        id
    }

    pub fn checkbox(
        &mut self,
        scope: Scope,
        checked: Signal<bool>,
        label: impl Into<String>,
    ) -> NodeId {
        let lbl = label.into();
        let id = self.spawn_node(
            "Checkbox",
            WidgetKind::Checkbox {
                checked,
                label: lbl.clone(),
            },
            LayoutStyle::row()
                .with_height(Length::Dp(32.0))
                .with_gap(8.0),
            scope,
        );
        if let Some(n) = self.nodes.get_mut(id) {
            n.a11y.role = A11yRole::CheckBox;
            n.a11y.label = Some(lbl);
            n.a11y.checked = Some(checked.get_untracked());
            n.a11y.focusable = true;
        }
        id
    }

    pub fn switch(
        &mut self,
        scope: Scope,
        checked: Signal<bool>,
        label: impl Into<String>,
    ) -> NodeId {
        let lbl = label.into();
        let id = self.spawn_node(
            "Switch",
            WidgetKind::Switch {
                checked,
                label: lbl.clone(),
            },
            LayoutStyle::row()
                .with_height(Length::Dp(32.0))
                .with_gap(10.0),
            scope,
        );
        if let Some(n) = self.nodes.get_mut(id) {
            n.a11y.role = A11yRole::Switch;
            n.a11y.label = Some(lbl);
            n.a11y.checked = Some(checked.get_untracked());
            n.a11y.focusable = true;
        }
        id
    }

    pub fn slider(
        &mut self,
        scope: Scope,
        value: Signal<f32>,
        min: f32,
        max: f32,
        label: impl Into<String>,
    ) -> NodeId {
        let lbl = label.into();
        let default_h = if lbl.is_empty() { 32.0 } else { 48.0 };
        let id = self.spawn_node(
            "Slider",
            WidgetKind::Slider {
                value,
                min,
                max,
                label: lbl.clone(),
            },
            LayoutStyle::row()
                .with_height(Length::Dp(default_h))
                .with_padding(Insets::symmetric(4.0, 0.0)),
            scope,
        );
        if let Some(n) = self.nodes.get_mut(id) {
            n.box_style.hit_slop = 10.0;
            n.a11y.role = A11yRole::Slider;
            n.a11y.label = Some(lbl);
            n.a11y.numeric_value = Some(value.get_untracked() as f64);
            n.a11y.min_value = Some(min as f64);
            n.a11y.max_value = Some(max as f64);
            n.a11y.focusable = true;
        }
        id
    }

    pub fn progress_indicator(
        &mut self,
        scope: Scope,
        progress: impl Into<ReactiveValue<Option<f32>>>,
    ) -> NodeId {
        let id = self.spawn_node(
            "ProgressIndicator",
            WidgetKind::ProgressIndicator {
                progress: progress.into(),
                phase: 0.0,
            },
            LayoutStyle::row().with_height(Length::Dp(8.0)),
            scope,
        );
        if let Some(n) = self.nodes.get_mut(id) {
            n.a11y.role = A11yRole::ProgressIndicator;
        }
        id
    }

    pub fn overlay(
        &mut self,
        scope: Scope,
        open: impl Into<ReactiveValue<bool>>,
        is_bottom_sheet: bool,
        on_close: Option<TapHandler>,
    ) -> NodeId {
        let mut layout = LayoutStyle::column().with_size(Length::Pct(100.0), Length::Pct(100.0));
        layout.absolute = true;
        layout.use_inset = true;
        layout.inset = Insets::ZERO;
        layout.justify_content = if is_bottom_sheet {
            craftui_core::Justify::End
        } else {
            craftui_core::Justify::Center
        };
        layout.align_items = Alignment::Center;
        let id = self.spawn_node(
            if is_bottom_sheet {
                "BottomSheet"
            } else {
                "Modal"
            },
            WidgetKind::Overlay {
                open: open.into(),
                on_close,
                is_bottom_sheet,
            },
            layout,
            scope,
        );
        if let Some(n) = self.nodes.get_mut(id) {
            n.a11y.role = A11yRole::Dialog;
        }
        id
    }

    pub fn platform_view(
        &mut self,
        scope: Scope,
        view_id: PlatformViewId,
        layout: LayoutStyle,
    ) -> NodeId {
        self.spawn_node(
            "PlatformView",
            WidgetKind::PlatformView { id: view_id },
            layout,
            scope,
        )
    }

    pub fn set_layout(&mut self, node: NodeId, layout: LayoutStyle) {
        if let Some(n) = self.nodes.get_mut(node) {
            n.layout = layout.clone();
            n.dirty |= DirtyFlags::LAYOUT | DirtyFlags::PAINT;
            self.layout_engine.upsert_node(node, layout);
            self.layout_engine.mark_dirty(node);
        }
    }

    pub fn set_box_style(&mut self, node: NodeId, box_style: BoxStyle) {
        if let Some(n) = self.nodes.get_mut(node) {
            if let Some(bg) = box_style.background.clone()
                && bg.is_dynamic()
            {
                let scope = n.scope;
                scope.effect(move || {
                    let _ = bg.get();
                    mark_node_dirty(node, DirtyFlags::PAINT);
                });
            }
            n.box_style = box_style;
            n.dirty |= DirtyFlags::PAINT;
        }
    }

    pub fn set_button_foreground(&mut self, node: NodeId, fg: impl Into<ReactiveValue<Color>>) {
        let rv = fg.into();
        if let Some(n) = self.nodes.get_mut(node)
            && let WidgetKind::Button { foreground, .. } = &mut n.kind
        {
            if rv.is_dynamic() {
                let rv_c = rv.clone();
                n.scope.effect(move || {
                    let _ = rv_c.get();
                    mark_node_dirty(node, DirtyFlags::PAINT);
                });
            }
            *foreground = Some(rv);
            n.dirty |= DirtyFlags::PAINT;
        }
    }

    pub fn set_image_radius(&mut self, node: NodeId, rad: CornerRadii) {
        if let Some(n) = self.nodes.get_mut(node)
            && let WidgetKind::Image { radius, .. } = &mut n.kind
        {
            *radius = rad;
            n.dirty |= DirtyFlags::PAINT;
        }
    }

    pub fn set_on_tap(&mut self, node: NodeId, cb: impl FnMut() + 'static) {
        if let Some(n) = self.nodes.get_mut(node) {
            n.on_tap = Some(Rc::new(RefCell::new(cb)));
        }
    }

    pub fn set_on_change(&mut self, node: NodeId, cb: impl FnMut(String) + 'static) {
        if let Some(n) = self.nodes.get_mut(node)
            && let WidgetKind::TextInput { on_change, .. } = &mut n.kind
        {
            *on_change = Some(Rc::new(RefCell::new(cb)));
        }
    }

    pub fn set_on_submit(&mut self, node: NodeId, cb: impl FnMut(String) + 'static) {
        if let Some(n) = self.nodes.get_mut(node)
            && let WidgetKind::TextInput { on_submit, .. } = &mut n.kind
        {
            *on_submit = Some(Rc::new(RefCell::new(cb)));
        }
    }

    /// Reactively toggle whether `node` participates in layout and painting.
    pub fn set_visible_when(&mut self, node: NodeId, pred: impl Fn() -> bool + 'static) {
        let rv = ReactiveValue::Dynamic(Rc::new(pred));
        if let Some(n) = self.nodes.get_mut(node) {
            let scope = n.scope;
            n.visible_when = Some(rv.clone());
            n.dirty |= DirtyFlags::LAYOUT | DirtyFlags::PAINT;
            scope.effect(move || {
                let _ = rv.get();
                mark_node_dirty(node, DirtyFlags::LAYOUT | DirtyFlags::PAINT);
            });
        }
    }

    /// Mount a reactive dynamic container whose subtree automatically rebuilds whenever `key_fn` changes.
    pub fn dynamic_slot<K: Clone + PartialEq + 'static>(
        &mut self,
        scope: Scope,
        layout: LayoutStyle,
        key_fn: impl Fn() -> K + 'static,
        builder: impl Fn(Scope, &K, &mut WidgetTree) -> NodeId + 'static,
    ) -> NodeId {
        let host = self.container(scope, layout, BoxStyle::new());
        let key_rc = Rc::new(key_fn);
        let key_for_eff = key_rc.clone();
        scope.effect(move || {
            let _ = key_for_eff();
            mark_node_dirty(host, DirtyFlags::LAYOUT | DirtyFlags::PAINT);
        });

        let mut last_key: Option<K> = None;
        let mut child_scope_opt: Option<Scope> = None;
        let sync_fn: DynamicSlotSync = Box::new(move |host_id, tree| {
            let next_key = key_rc();
            if last_key.as_ref() == Some(&next_key) {
                return false;
            }
            last_key = Some(next_key.clone());

            let old_children = tree
                .nodes
                .get(host_id)
                .map(|n| n.children.clone())
                .unwrap_or_default();
            for oc in old_children {
                tree.remove_subtree(oc);
            }
            if let Some(old_sc) = child_scope_opt.take() {
                old_sc.dispose();
            }

            let new_scope = scope.child_scope();
            child_scope_opt = Some(new_scope);
            let child = builder(new_scope, &next_key, tree);
            tree.set_children(host_id, vec![child]);
            true
        });

        self.dynamic_hosts.insert(host, sync_fn);
        self.sync_dynamic_slots();
        host
    }

    fn sync_dynamic_slots(&mut self) -> bool {
        let mut changed = false;
        let keys: Vec<NodeId> = self.dynamic_hosts.keys().collect();
        for host_id in keys {
            if !self.nodes.contains_key(host_id) {
                continue;
            }
            if let Some(mut sync_fn) = self.dynamic_hosts.remove(host_id) {
                if sync_fn(host_id, self) {
                    changed = true;
                }
                if self.nodes.contains_key(host_id) {
                    self.dynamic_hosts.insert(host_id, sync_fn);
                }
            }
        }
        changed
    }

    pub fn rect(&self, node: NodeId) -> Rect {
        self.layout_engine.rect(node)
    }

    // === Frame Step: Animations, Dirty Sync, Layout, Virtualization, Paint, A11y ===

    /// Advance spring animations, inertial scroll flings, and return whether another animation frame is needed.
    pub fn tick_animations(&mut self, dt_sec: f32) -> bool {
        let mut active = false;

        if let Some((from, to, mut spring)) = self.theme_spring.take() {
            let still_running = spring.step(dt_sec);
            let lerped = from.lerp(&to, spring.value);
            self.theme_signal.set(lerped);
            if still_running {
                self.theme_spring = Some((from, to, spring));
                active = true;
            }
        }

        self.caret_blink_sec = (self.caret_blink_sec + dt_sec) % 1.0;
        if let Some(focused) = self.focused
            && let Some(n) = self.nodes.get(focused)
            && matches!(n.kind, WidgetKind::TextInput { .. })
        {
            active = true;
        }

        let ids: Vec<NodeId> = self.nodes.keys().collect();
        for id in ids {
            let rect = self.layout_engine.rect(id);
            let physics = self.scroll_physics;
            if let Some(node) = self.nodes.get_mut(id) {
                match &mut node.kind {
                    WidgetKind::ScrollView {
                        axis,
                        offset,
                        velocity,
                        content_extent,
                    } => {
                        if velocity.abs() > 0.0 {
                            let (delta, running) = physics.step_fling(velocity, dt_sec);
                            let viewport = match axis {
                                Axis::Vertical => rect.height,
                                Axis::Horizontal => rect.width,
                            };
                            let max_off = (*content_extent - viewport).max(0.0);
                            *offset = (*offset - delta).clamp(0.0, max_off);
                            if *offset <= 0.0 || *offset >= max_off {
                                *velocity = 0.0;
                            }
                            node.dirty |= DirtyFlags::PAINT;
                            if running && *velocity != 0.0 {
                                active = true;
                            }
                        }
                    }
                    WidgetKind::VirtualList {
                        axis,
                        columns,
                        item_extent,
                        item_count,
                        offset,
                        velocity,
                    } => {
                        if velocity.abs() > 0.0 {
                            let (delta, running) = physics.step_fling(velocity, dt_sec);
                            let total_rows = item_count.get().div_ceil((*columns).max(1));
                            let total_extent = total_rows as f32 * *item_extent;
                            let viewport = match axis {
                                Axis::Vertical => rect.height,
                                Axis::Horizontal => rect.width,
                            };
                            let max_off = (total_extent - viewport).max(0.0);
                            *offset = (*offset - delta).clamp(0.0, max_off);
                            node.dirty |= DirtyFlags::LAYOUT | DirtyFlags::PAINT;
                            if running {
                                active = true;
                            }
                        }
                    }
                    WidgetKind::ProgressIndicator { progress, phase }
                        if progress.get().is_none() =>
                    {
                        *phase = (*phase + dt_sec * 1.4) % 1.0;
                        node.dirty |= DirtyFlags::PAINT;
                        active = true;
                    }
                    _ => {}
                }
            }
        }

        active
    }

    /// Pull dirty flags from `craftui-reactive` and update layout + virtualized windows.
    pub fn layout(&mut self, metrics: WindowMetrics) -> usize {
        self.metrics = metrics;
        let dirty_updates = drain_dirty_nodes();
        let dirty_count = dirty_updates.len();

        for (id, flags) in dirty_updates {
            if let Some(node) = self.nodes.get_mut(id) {
                node.dirty |= flags;
                if flags.contains(DirtyFlags::LAYOUT) {
                    self.layout_engine.mark_dirty(id);
                }
            }
        }

        // Rebuild any reactive dynamic_slot subtrees before syncing properties/solving layout
        self.sync_dynamic_slots();

        // Sync dynamic properties before layout
        let all_ids: Vec<NodeId> = self.nodes.keys().collect();
        for id in all_ids {
            if let Some(node) = self.nodes.get_mut(id) {
                let mut layout_changed = false;
                if let Some(ref vis) = node.visible_when {
                    let hidden = !vis.get();
                    if node.layout.hidden != hidden {
                        node.layout.hidden = hidden;
                        layout_changed = true;
                    }
                }
                match &mut node.kind {
                    WidgetKind::Text { content, style } => {
                        if node.dirty.contains(DirtyFlags::TEXT)
                            || node.dirty.contains(DirtyFlags::LAYOUT)
                        {
                            node.cached_text = content.get();
                            node.cached_style = style.get();
                            node.a11y.label = Some(node.cached_text.clone());
                        }
                    }
                    WidgetKind::Button { label, .. } => {
                        let txt = label.get();
                        node.cached_text = txt.clone();
                        node.a11y.label = Some(txt);
                    }
                    WidgetKind::Checkbox { checked, .. } | WidgetKind::Switch { checked, .. } => {
                        node.a11y.checked = Some(checked.get_untracked());
                    }
                    WidgetKind::Slider { value, .. } => {
                        node.a11y.numeric_value = Some(value.get_untracked() as f64);
                    }
                    WidgetKind::Overlay { open, .. } => {
                        let should_show = open.get();
                        let hidden = !should_show;
                        if node.layout.hidden != hidden {
                            node.layout.hidden = hidden;
                            layout_changed = true;
                        }
                    }
                    _ => {}
                }
                if layout_changed {
                    let st = node.layout.clone();
                    self.layout_engine.upsert_node(id, st);
                    self.layout_engine.mark_dirty(id);
                }
            }
        }

        let Some(root) = self.root else {
            return dirty_count;
        };

        // Pass 1: solve layout
        self.solve_taffy(root, metrics);

        // Pass 2: recycle VirtualList / VirtualGrid visible items & recompute if children changed
        if self.sync_virtual_lists() {
            self.solve_taffy(root, metrics);
        }

        // Update ScrollView content_extent
        let scroll_ids: Vec<NodeId> = self.nodes.keys().collect();
        for id in scroll_ids {
            let is_scroll = self
                .nodes
                .get(id)
                .is_some_and(|n| matches!(n.kind, WidgetKind::ScrollView { .. }));
            if is_scroll {
                let rect = self.layout_engine.rect(id);
                let mut stack = self
                    .nodes
                    .get(id)
                    .map(|n| n.children.clone())
                    .unwrap_or_default();
                let mut max_bottom = 0.0f32;
                let mut max_right = 0.0f32;
                while let Some(c) = stack.pop() {
                    let cr = self.layout_engine.rect(c);
                    max_bottom = max_bottom.max(cr.max_y() - rect.y);
                    max_right = max_right.max(cr.max_x() - rect.x);
                    if let Some(cn) = self.nodes.get(c) {
                        stack.extend(cn.children.iter().copied());
                    }
                }
                if let Some(n) = self.nodes.get_mut(id)
                    && let WidgetKind::ScrollView {
                        axis,
                        offset,
                        content_extent,
                        ..
                    } = &mut n.kind
                {
                    *content_extent = match axis {
                        Axis::Vertical => max_bottom,
                        Axis::Horizontal => max_right,
                    };
                    let view = match axis {
                        Axis::Vertical => rect.height,
                        Axis::Horizontal => rect.width,
                    };
                    let max_off = (*content_extent - view).max(0.0);
                    *offset = offset.clamp(0.0, max_off);
                }
            }
        }

        dirty_count
    }

    fn solve_taffy(&mut self, root: NodeId, metrics: WindowMetrics) {
        let nodes = &self.nodes;
        let text_engine = &mut self.text_engine;
        let theme = self.theme_signal.get_untracked();
        let safe_area = metrics.effective_safe_area();

        self.layout_engine.compute_layout(
            root,
            metrics.logical_size.width,
            metrics.logical_size.height,
            safe_area,
            metrics.scale_factor,
            self.direction,
            |node_id, constraints| {
                let Some(node) = nodes.get(node_id) else {
                    return Size::ZERO;
                };
                match &node.kind {
                    WidgetKind::Text { content, style } => {
                        let txt = if node.cached_text.is_empty() {
                            content.get()
                        } else {
                            node.cached_text.clone()
                        };
                        let mut st = style.get();
                        if st.color == Color::TRANSPARENT {
                            st.color = theme.text_primary;
                        }
                        let max_w = if constraints.max_width.is_finite() {
                            Some(constraints.max_width)
                        } else {
                            None
                        };
                        text_engine.measure(&txt, &st, max_w)
                    }
                    WidgetKind::Button { label, .. } => {
                        let txt = label.get();
                        let st = TextStyle::default().with_size(15.0).with_weight(600.0);
                        text_engine.measure(&txt, &st, None)
                    }
                    WidgetKind::Checkbox { label, .. } => {
                        let s =
                            text_engine.measure(label, &TextStyle::default().with_size(14.0), None);
                        Size::new(26.0 + s.width, s.height.max(22.0))
                    }
                    WidgetKind::Switch { label, .. } => {
                        let s =
                            text_engine.measure(label, &TextStyle::default().with_size(14.0), None);
                        Size::new(50.0 + s.width, s.height.max(26.0))
                    }
                    WidgetKind::Slider { label, .. } => {
                        Size::new(160.0, if label.is_empty() { 32.0 } else { 48.0 })
                    }
                    WidgetKind::TextInput { .. } => Size::new(160.0, 40.0),
                    WidgetKind::ProgressIndicator { .. } => Size::new(120.0, 8.0),
                    _ => Size::ZERO,
                }
            },
        );
    }

    fn sync_virtual_lists(&mut self) -> bool {
        let mut changed = false;
        let v_ids: Vec<NodeId> = self.virtual_hosts.keys().collect();

        for host_id in v_ids {
            let rect = self.layout_engine.rect(host_id);
            let Some(node) = self.nodes.get(host_id) else {
                continue;
            };
            let WidgetKind::VirtualList {
                axis,
                columns,
                item_extent,
                item_count,
                offset,
                ..
            } = &node.kind
            else {
                continue;
            };

            let axis = *axis;
            let total_count = item_count.get();
            let cols = (*columns).max(1);
            let extent = *item_extent;
            let off = *offset;
            let viewport = match axis {
                Axis::Vertical => rect.height.max(extent),
                Axis::Horizontal => rect.width.max(extent),
            };
            let overscan_rows = 2usize;
            let first_row =
                ((off / extent).floor() as isize - overscan_rows as isize).max(0) as usize;
            let last_row = (((off + viewport) / extent).ceil() as usize + overscan_rows)
                .min(total_count.div_ceil(cols).saturating_sub(1));

            let first_idx = (first_row * cols).min(total_count);
            let last_idx = ((last_row + 1) * cols).min(total_count);
            let new_range = if total_count == 0 {
                None
            } else {
                Some((first_idx, last_idx))
            };

            let needs_rebuild = self
                .virtual_hosts
                .get(host_id)
                .is_some_and(|h| h.last_range != new_range);
            if !needs_rebuild {
                continue;
            }
            changed = true;

            let Some(mut host) = self.virtual_hosts.remove(host_id) else {
                continue;
            };
            host.last_range = new_range;

            // Dispose out-of-window items
            let mut to_remove = Vec::new();
            for (&idx, &(item_node, item_scope)) in &host.active_items {
                let keep = new_range.is_some_and(|(s, e)| idx >= s && idx < e);
                if !keep {
                    to_remove.push((idx, item_node, item_scope));
                }
            }
            for (idx, item_node, _item_scope) in to_remove {
                host.active_items.remove(&idx);
                self.remove_subtree(item_node);
            }

            let host_scope = self
                .nodes
                .get(host_id)
                .map(|n| n.scope)
                .unwrap_or(self.root_scope);
            let mut ordered_children = Vec::new();

            if let Some((start, end)) = new_range {
                let col_w = if cols > 1 {
                    (rect.width / cols as f32).max(20.0)
                } else {
                    rect.width.max(20.0)
                };
                for idx in start..end {
                    let row = idx / cols;
                    let col = idx % cols;
                    let item_node = if let Some(&(existing, _)) = host.active_items.get(&idx) {
                        existing
                    } else {
                        let item_scope = host_scope.child_scope();
                        let created = (host.factory)(item_scope, idx, self);
                        host.active_items.insert(idx, (created, item_scope));
                        created
                    };

                    // Position each recycled item absolutely at its virtualized slot
                    let mut st = self
                        .nodes
                        .get(item_node)
                        .map(|n| n.layout.clone())
                        .unwrap_or_default();
                    st.absolute = true;
                    st.use_inset = true;
                    match axis {
                        Axis::Vertical => {
                            st.inset = Insets::new(
                                row as f32 * extent - off,
                                0.0,
                                0.0,
                                col as f32 * col_w,
                            );
                            st.width = Length::Dp(col_w);
                            st.height = Length::Dp(extent);
                        }
                        Axis::Horizontal => {
                            st.inset = Insets::new(0.0, 0.0, 0.0, idx as f32 * extent - off);
                            st.width = Length::Dp(extent);
                            st.height = Length::Dp(rect.height.max(20.0));
                        }
                    }
                    self.set_layout(item_node, st);
                    ordered_children.push(item_node);
                }
            }

            self.set_children(host_id, ordered_children);
            self.virtual_hosts.insert(host_id, host);
        }

        changed
    }

    /// Record [`PaintScene`] commands from the retained widget tree.
    pub fn paint(&mut self, scene: &mut PaintScene) {
        scene.clear();
        let Some(root) = self.root else {
            return;
        };
        let theme = self.theme_signal.get_untracked();
        self.paint_node_recursive(root, Point::ZERO, &theme, scene);

        if self.inspector.enabled {
            let mut boxes = Vec::new();
            for (id, node) in &self.nodes {
                let r = self.layout_engine.rect(id);
                if !r.size().is_empty() {
                    boxes.push((r, node.debug_name));
                }
            }
            self.inspector
                .paint_overlay(scene, &boxes, self.metrics.logical_size.width);
        }
    }

    fn paint_node_recursive(
        &mut self,
        id: NodeId,
        scroll_offset: Point,
        theme: &ThemeTokens,
        scene: &mut PaintScene,
    ) {
        let Some(node) = self.nodes.get(id) else {
            return;
        };
        if node.layout.hidden {
            return;
        }
        let rect = self
            .layout_engine
            .rect(id)
            .translate(scroll_offset.x, scroll_offset.y);
        let box_style = node.box_style.clone();
        let children = node.children.clone();
        let kind = node.kind.clone();
        let is_focused = self.focused == Some(id);

        if let WidgetKind::Overlay { open, .. } = &kind
            && !open.get()
        {
            return;
        }

        if box_style.opacity < 0.999 {
            scene.push(PaintCommand::PushLayer {
                opacity: box_style.opacity,
            });
        }

        if let Some(shadow) = box_style.shadow {
            scene.push(PaintCommand::DropShadow {
                rect,
                radius: box_style.radius.top_left,
                std_dev: shadow.blur_radius,
                offset_x: shadow.offset_x,
                offset_y: shadow.offset_y,
                color: shadow.color,
            });
        }

        if let Some(ref bg) = box_style.background {
            scene.fill_rect(rect, bg.get(), box_style.radius);
        }

        let mut child_offset = scroll_offset;

        match kind {
            WidgetKind::Container | WidgetKind::Spacer | WidgetKind::SafeArea => {}
            WidgetKind::Text { content, style } => {
                let txt = content.get();
                let mut st = style.get();
                if st.color == Color::TRANSPARENT {
                    st.color = theme.text_primary;
                }
                scene.draw_text(rect.origin(), txt, st, Some(rect.width.max(1.0)));
            }
            WidgetKind::Button {
                label,
                disabled,
                foreground,
                hovered,
                pressed,
                ..
            } => {
                let is_disabled = disabled.get();
                let mut bg = match box_style.background {
                    Some(ref b) => match b.get() {
                        Brush::Solid(c) => c,
                        _ => theme.primary,
                    },
                    None => theme.primary,
                };
                if is_disabled {
                    bg = theme.surface_variant;
                } else if pressed {
                    bg = bg.lerp(Color::BLACK, 0.22);
                } else if hovered {
                    bg = bg.lerp(Color::WHITE, 0.12);
                }
                scene.fill_rect(rect, bg, box_style.radius);
                if is_focused {
                    scene.stroke_rect(rect.inflate(2.0, 2.0), Color::WHITE, 1.5, box_style.radius);
                }
                let txt = label.get();
                let fg = if is_disabled {
                    theme.text_secondary
                } else if let Some(ref custom_fg) = foreground {
                    custom_fg.get()
                } else {
                    theme.on_primary
                };
                let tstyle = TextStyle::default()
                    .with_size(15.0)
                    .with_weight(600.0)
                    .with_color(fg);
                let measured = self.text_engine.measure(&txt, &tstyle, None);
                let tx = rect.x + (rect.width - measured.width) * 0.5;
                let ty = rect.y + (rect.height - measured.height) * 0.5;
                scene.draw_text(Point::new(tx, ty), txt, tstyle, None);
            }
            WidgetKind::Image {
                texture,
                fit,
                radius,
                opacity,
            } => {
                if let Some(tex_id) = texture.get() {
                    scene.push(PaintCommand::DrawImage {
                        rect,
                        texture: tex_id,
                        fit,
                        radius,
                        opacity,
                    });
                } else {
                    scene.fill_rect(rect, theme.surface_variant, radius);
                }
            }
            WidgetKind::Canvas { on_paint, .. } => {
                scene.push(PaintCommand::PushClip {
                    rect,
                    radius: box_style.radius,
                });
                on_paint(rect, scene);
                scene.push(PaintCommand::PopClip);
            }
            WidgetKind::ScrollView {
                axis,
                offset,
                content_extent,
                ..
            } => {
                match axis {
                    Axis::Vertical => child_offset.y -= offset,
                    Axis::Horizontal => child_offset.x -= offset,
                }
                scene.push(PaintCommand::PushClip {
                    rect,
                    radius: box_style.radius,
                });
                for child in children.iter().copied() {
                    self.paint_node_recursive(child, child_offset, theme, scene);
                }
                // Scrollbar indicator
                let view = match axis {
                    Axis::Vertical => rect.height,
                    Axis::Horizontal => rect.width,
                };
                if content_extent > view + 1.0 {
                    let ratio = (view / content_extent).clamp(0.08, 1.0);
                    let pos_ratio = (offset / (content_extent - view)).clamp(0.0, 1.0);
                    match axis {
                        Axis::Vertical => {
                            let thumb_h = view * ratio;
                            let thumb_y = rect.y + (view - thumb_h) * pos_ratio;
                            scene.fill_rect(
                                Rect::new(rect.max_x() - 6.0, thumb_y, 4.0, thumb_h),
                                theme.text_secondary.with_alpha(0.55),
                                CornerRadii::all(2.0),
                            );
                        }
                        Axis::Horizontal => {
                            let thumb_w = view * ratio;
                            let thumb_x = rect.x + (view - thumb_w) * pos_ratio;
                            scene.fill_rect(
                                Rect::new(thumb_x, rect.max_y() - 6.0, thumb_w, 4.0),
                                theme.text_secondary.with_alpha(0.55),
                                CornerRadii::all(2.0),
                            );
                        }
                    }
                }
                scene.push(PaintCommand::PopClip);
                if box_style.opacity < 0.999 {
                    scene.push(PaintCommand::PopLayer);
                }
                return;
            }
            WidgetKind::VirtualList { .. } => {
                scene.push(PaintCommand::PushClip {
                    rect,
                    radius: box_style.radius,
                });
                for child in children.iter().copied() {
                    self.paint_node_recursive(child, scroll_offset, theme, scene);
                }
                scene.push(PaintCommand::PopClip);
                if box_style.opacity < 0.999 {
                    scene.push(PaintCommand::PopLayer);
                }
                return;
            }
            WidgetKind::TextInput {
                editor,
                placeholder,
                style,
                ..
            } => {
                scene.fill_rect(rect, theme.surface, box_style.radius);
                let border_color = if is_focused {
                    theme.primary
                } else {
                    theme.border
                };
                scene.stroke_rect(
                    rect,
                    border_color,
                    if is_focused { 1.8 } else { 1.0 },
                    box_style.radius,
                );

                let inner = rect.inset(Insets::symmetric(12.0, 8.0));
                scene.push(PaintCommand::PushClip {
                    rect: inner,
                    radius: CornerRadii::ZERO,
                });
                let disp = editor.display_text().into_owned();
                let mut draw_style = style.clone();
                if disp.is_empty() {
                    draw_style.color = theme.text_secondary;
                    scene.draw_text(inner.origin(), placeholder, draw_style, None);
                } else {
                    draw_style.color = theme.text_primary;
                    let layout = self.text_engine.build_layout(&disp, &draw_style, None);
                    if let Some((s, e)) = editor.selection_range() {
                        for sr in self.text_engine.selection_rects(&layout, s, e) {
                            scene.fill_rect(
                                sr.translate(inner.x, inner.y),
                                theme.primary.with_alpha(0.32),
                                CornerRadii::ZERO,
                            );
                        }
                    }
                    scene.draw_text(inner.origin(), &disp, draw_style, None);
                    if is_focused && self.caret_blink_sec < 0.55 {
                        let cr = self.text_engine.cursor_rect(&layout, editor.cursor);
                        scene.fill_rect(
                            Rect::new(inner.x + cr.x, inner.y + cr.y, 2.0, cr.height),
                            theme.primary,
                            CornerRadii::ZERO,
                        );
                    }
                }
                scene.push(PaintCommand::PopClip);
            }
            WidgetKind::Checkbox { checked, label } => {
                let is_on = checked.get_untracked();
                let box_rect = Rect::new(rect.x, rect.y + (rect.height - 20.0) * 0.5, 20.0, 20.0);
                let fill = if is_on { theme.primary } else { theme.surface };
                scene.fill_rect(box_rect, fill, CornerRadii::all(5.0));
                scene.stroke_rect(
                    box_rect,
                    if is_on { theme.primary } else { theme.border },
                    1.5,
                    CornerRadii::all(5.0),
                );
                if is_on {
                    let mut path = kurbo::BezPath::new();
                    path.move_to((box_rect.x as f64 + 4.5, box_rect.y as f64 + 10.5));
                    path.line_to((box_rect.x as f64 + 8.5, box_rect.y as f64 + 14.5));
                    path.line_to((box_rect.x as f64 + 15.5, box_rect.y as f64 + 5.5));
                    scene.push(PaintCommand::StrokePath {
                        path,
                        color: theme.on_primary,
                        width: 2.2,
                    });
                }
                scene.draw_text(
                    Point::new(rect.x + 28.0, rect.y + (rect.height - 16.0) * 0.5),
                    label,
                    TextStyle::default()
                        .with_size(14.0)
                        .with_color(theme.text_primary),
                    None,
                );
            }
            WidgetKind::Switch { checked, label } => {
                let is_on = checked.get_untracked();
                let track = Rect::new(rect.x, rect.y + (rect.height - 24.0) * 0.5, 44.0, 24.0);
                scene.fill_rect(
                    track,
                    if is_on {
                        theme.primary
                    } else {
                        theme.surface_variant
                    },
                    CornerRadii::all(12.0),
                );
                let knob_x = if is_on { track.x + 22.0 } else { track.x + 3.0 };
                scene.fill_rect(
                    Rect::new(knob_x, track.y + 3.0, 18.0, 18.0),
                    Color::WHITE,
                    CornerRadii::all(9.0),
                );
                if !label.is_empty() {
                    scene.draw_text(
                        Point::new(rect.x + 54.0, rect.y + (rect.height - 16.0) * 0.5),
                        label,
                        TextStyle::default()
                            .with_size(14.0)
                            .with_color(theme.text_primary),
                        None,
                    );
                }
            }
            WidgetKind::Slider {
                value,
                min,
                max,
                label,
            } => {
                let v = value.get_untracked();
                let frac = if (max - min).abs() > 1e-6 {
                    ((v - min) / (max - min)).clamp(0.0, 1.0)
                } else {
                    0.0
                };
                let cy = if !label.is_empty() && rect.height >= 40.0 {
                    let lbl_style = TextStyle::default()
                        .with_size(13.0)
                        .with_weight(500.0)
                        .with_color(theme.text_secondary);
                    scene.draw_text(
                        Point::new(rect.x + 8.0, rect.y + 2.0),
                        &label,
                        lbl_style,
                        None,
                    );
                    let val_txt = if (v - v.round()).abs() < 0.05 {
                        format!("{:.0}", v)
                    } else {
                        format!("{:.1}", v)
                    };
                    let val_style = TextStyle::default()
                        .with_size(13.0)
                        .with_weight(600.0)
                        .with_color(theme.text_primary);
                    let val_w = self.text_engine.measure(&val_txt, &val_style, None).width;
                    scene.draw_text(
                        Point::new((rect.max_x() - 8.0 - val_w).max(rect.x + 8.0), rect.y + 2.0),
                        val_txt,
                        val_style,
                        None,
                    );
                    rect.max_y() - 12.0
                } else {
                    rect.y + rect.height * 0.5
                };
                let track = Rect::new(rect.x + 8.0, cy - 3.0, (rect.width - 16.0).max(8.0), 6.0);
                scene.fill_rect(track, theme.surface_variant, CornerRadii::all(3.0));
                let active_w = track.width * frac;
                scene.fill_rect(
                    Rect::new(track.x, track.y, active_w, track.height),
                    theme.primary,
                    CornerRadii::all(3.0),
                );
                let kx = track.x + active_w - 9.0;
                scene.fill_rect(
                    Rect::new(kx, cy - 9.0, 18.0, 18.0),
                    Color::WHITE,
                    CornerRadii::all(9.0),
                );
                scene.stroke_rect(
                    Rect::new(kx, cy - 9.0, 18.0, 18.0),
                    theme.primary,
                    2.0,
                    CornerRadii::all(9.0),
                );
            }
            WidgetKind::ProgressIndicator { progress, phase } => {
                scene.fill_rect(
                    rect,
                    theme.surface_variant,
                    CornerRadii::all(rect.height * 0.5),
                );
                match progress.get() {
                    Some(p) => {
                        let w = rect.width * p.clamp(0.0, 1.0);
                        scene.fill_rect(
                            Rect::new(rect.x, rect.y, w, rect.height),
                            theme.primary,
                            CornerRadii::all(rect.height * 0.5),
                        );
                    }
                    None => {
                        let bar_w = rect.width * 0.35;
                        let start_x = rect.x + (rect.width - bar_w) * phase;
                        scene.fill_rect(
                            Rect::new(start_x, rect.y, bar_w, rect.height),
                            theme.primary,
                            CornerRadii::all(rect.height * 0.5),
                        );
                    }
                }
            }
            WidgetKind::Overlay { .. } => {
                scene.fill_rect(rect, Color::rgba(0, 0, 0, 145), CornerRadii::ZERO);
            }
            WidgetKind::PlatformView { id: pv_id } => {
                scene.push(PaintCommand::PlatformViewHole { id: pv_id, rect });
            }
        }

        if let Some(border) = box_style.border
            && border.width > 0.0
        {
            scene.stroke_rect(rect, border.color, border.width, border.radius);
        }

        if box_style.clip {
            scene.push(PaintCommand::PushClip {
                rect,
                radius: box_style.radius,
            });
        }
        for child in children {
            self.paint_node_recursive(child, child_offset, theme, scene);
        }
        if box_style.clip {
            scene.push(PaintCommand::PopClip);
        }
        if box_style.opacity < 0.999 {
            scene.push(PaintCommand::PopLayer);
        }

        if let Some(n) = self.nodes.get_mut(id) {
            n.dirty = DirtyFlags::NONE;
        }
    }

    // === Event Routing & Hit-Testing ===

    /// Hit-test the deepest interactive/visible node under `point`, respecting scroll offsets and `hit_slop`.
    pub fn hit_test(&self, point: Point) -> Option<NodeId> {
        let root = self.root?;
        self.hit_test_recursive(root, point, Point::ZERO)
    }

    fn hit_test_recursive(&self, id: NodeId, point: Point, scroll_offset: Point) -> Option<NodeId> {
        let node = self.nodes.get(id)?;
        if node.layout.hidden {
            return None;
        }
        if let WidgetKind::Overlay { open, .. } = &node.kind
            && !open.get()
        {
            return None;
        }

        let rect = self
            .layout_engine
            .rect(id)
            .translate(scroll_offset.x, scroll_offset.y)
            .inflate(node.box_style.hit_slop, node.box_style.hit_slop);

        if node.box_style.clip && !rect.contains(point) {
            return None;
        }

        let mut child_offset = scroll_offset;
        if let WidgetKind::ScrollView { axis, offset, .. } = &node.kind {
            match axis {
                Axis::Vertical => child_offset.y -= *offset,
                Axis::Horizontal => child_offset.x -= *offset,
            }
        }

        // Topmost Z-order children first
        for &child in node.children.iter().rev() {
            if let Some(hit) = self.hit_test_recursive(child, point, child_offset) {
                return Some(hit);
            }
        }

        if rect.contains(point) && self.is_interactive(node) {
            Some(id)
        } else {
            None
        }
    }

    fn is_interactive(&self, node: &WidgetNode) -> bool {
        node.on_tap.is_some()
            || matches!(
                node.kind,
                WidgetKind::Button { .. }
                    | WidgetKind::Canvas {
                        on_pointer: Some(_),
                        ..
                    }
                    | WidgetKind::ScrollView { .. }
                    | WidgetKind::VirtualList { .. }
                    | WidgetKind::TextInput { .. }
                    | WidgetKind::Checkbox { .. }
                    | WidgetKind::Switch { .. }
                    | WidgetKind::Slider { .. }
                    | WidgetKind::Overlay { .. }
            )
    }

    /// Dispatch a [`UiEvent`] through the gesture arena, scroll physics, keyboard, and IME.
    /// Returns `true` if the UI state/scene should be re-rendered.
    pub fn handle_event(&mut self, event: &UiEvent) -> bool {
        match event {
            UiEvent::Pointer(pe) => self.handle_pointer(pe),
            UiEvent::Scroll(delta) => self.handle_scroll(delta.position, delta.x, delta.y),
            UiEvent::Key(ke) => self.handle_key(ke),
            UiEvent::Ime(ime) => self.handle_ime(ime),
            UiEvent::Resize {
                width,
                height,
                scale_factor,
            } => {
                self.metrics.logical_size = Size::new(*width, *height);
                self.metrics.scale_factor = *scale_factor;
                true
            }
            UiEvent::Pinch { .. } | UiEvent::FocusChanged(_) => false,
        }
    }

    fn handle_pointer(&mut self, pe: &PointerEvent) -> bool {
        let mut changed = false;
        let hit = self.hit_test(pe.position);

        // Update hovered state
        if hit != self.hovered {
            if let Some(prev) = self.hovered
                && let Some(WidgetKind::Button { hovered, .. }) =
                    self.nodes.get_mut(prev).map(|n| &mut n.kind)
                && *hovered
            {
                *hovered = false;
                changed = true;
            }
            self.hovered = hit;
            if let Some(curr) = hit
                && let Some(WidgetKind::Button { hovered, .. }) =
                    self.nodes.get_mut(curr).map(|n| &mut n.kind)
                && !*hovered
            {
                *hovered = true;
                changed = true;
            }
            if self.inspector.enabled {
                self.inspector.hovered_node = hit.map(|h| {
                    let name = self.nodes.get(h).map(|n| n.debug_name).unwrap_or("Node");
                    (name.to_string(), self.layout_engine.rect(h))
                });
                changed = true;
            }
        }

        // Route raw pointer directly to captured Canvas if applicable
        let target = self.gesture_arena.active_capture(pe.id).or(hit);
        if let Some(t) = target {
            let rect = self.layout_engine.rect(t);
            let maybe_cb = self.nodes.get(t).and_then(|n| match &n.kind {
                WidgetKind::Canvas { on_pointer, .. } => on_pointer.clone(),
                _ => None,
            });
            if let Some(cb) = maybe_cb
                && let Ok(mut func) = cb.try_borrow_mut()
            {
                func(rect, pe);
                changed = true;
            }
        }

        // Button pressed feedback
        if matches!(pe.phase, craftui_core::PointerPhase::Down)
            && let Some(h) = hit
            && let Some(WidgetKind::Button { pressed, .. }) =
                self.nodes.get_mut(h).map(|n| &mut n.kind)
        {
            *pressed = true;
            changed = true;
        }
        if matches!(
            pe.phase,
            craftui_core::PointerPhase::Up | craftui_core::PointerPhase::Cancel
        ) {
            for (_, node) in &mut self.nodes {
                if let WidgetKind::Button { pressed, .. } = &mut node.kind
                    && *pressed
                {
                    *pressed = false;
                    changed = true;
                }
            }
        }

        let gestures = self.gesture_arena.process_pointer(pe, hit);
        for g in gestures {
            match g {
                RecognizedGesture::Tap { node, position } => {
                    self.focused = Some(node);
                    self.activate_tap(node, position);
                    changed = true;
                }
                RecognizedGesture::DragStart { node, position }
                | RecognizedGesture::DragUpdate { node, position, .. } => {
                    if let RecognizedGesture::DragUpdate { delta, .. } = g {
                        self.handle_drag(node, position, delta);
                    } else {
                        self.handle_drag(node, position, Point::ZERO);
                    }
                    changed = true;
                }
                RecognizedGesture::DragEnd { node, velocity } => {
                    self.handle_drag_end(node, velocity);
                    changed = true;
                }
                _ => {}
            }
        }

        changed
    }

    fn activate_tap(&mut self, node_id: NodeId, pos: Point) {
        let rect = self.layout_engine.rect(node_id);

        // Generic node on_tap handler
        let extra_tap = self.nodes.get(node_id).and_then(|n| n.on_tap.clone());
        if let Some(cb) = extra_tap
            && let Ok(mut f) = cb.try_borrow_mut()
        {
            f();
        }

        let mut action: Option<TapHandler> = None;
        if let Some(node) = self.nodes.get_mut(node_id) {
            match &mut node.kind {
                WidgetKind::Button {
                    disabled, on_tap, ..
                } => {
                    if !disabled.get() {
                        action = on_tap.clone();
                    }
                }
                WidgetKind::Checkbox { checked, .. } | WidgetKind::Switch { checked, .. } => {
                    checked.update(|v| *v = !*v);
                }
                WidgetKind::Slider {
                    value, min, max, ..
                } => {
                    let track_x = rect.x + 8.0;
                    let track_w = (rect.width - 16.0).max(1.0);
                    let t = ((pos.x - track_x) / track_w).clamp(0.0, 1.0);
                    value.set(*min + (*max - *min) * t);
                }
                WidgetKind::TextInput { editor, style, .. } => {
                    let inner_x = pos.x - (rect.x + 12.0);
                    let inner_y = pos.y - (rect.y + 8.0);
                    let layout = self
                        .text_engine
                        .build_layout(&editor.display_text(), style, None);
                    let byte_idx = self.text_engine.hit_test_point(&layout, inner_x, inner_y);
                    editor.set_cursor_at(byte_idx, false);
                }
                WidgetKind::Overlay { on_close, .. } => {
                    action = on_close.clone();
                }
                _ => {}
            }
        }

        if let Some(cb) = action
            && let Ok(mut f) = cb.try_borrow_mut()
        {
            f();
        }
    }

    fn handle_drag(&mut self, node_id: NodeId, pos: Point, delta: Point) {
        let rect = self.layout_engine.rect(node_id);
        // Walk up if child doesn't directly handle drag (e.g., dragging inside a ScrollView/VirtualList)
        let mut cursor = Some(node_id);
        while let Some(nid) = cursor {
            let parent = self.nodes.get(nid).and_then(|n| n.parent);
            if let Some(node) = self.nodes.get_mut(nid) {
                match &mut node.kind {
                    WidgetKind::Slider {
                        value, min, max, ..
                    } => {
                        let r = self.layout_engine.rect(nid);
                        let track_x = r.x + 8.0;
                        let track_w = (r.width - 16.0).max(1.0);
                        let t = ((pos.x - track_x) / track_w).clamp(0.0, 1.0);
                        value.set(*min + (*max - *min) * t);
                        return;
                    }
                    WidgetKind::ScrollView {
                        axis,
                        offset,
                        velocity,
                        content_extent,
                    } => {
                        *velocity = 0.0;
                        let view = match axis {
                            Axis::Vertical => rect.height,
                            Axis::Horizontal => rect.width,
                        };
                        let max_off = (*content_extent - view).max(0.0);
                        let d = match axis {
                            Axis::Vertical => -delta.y,
                            Axis::Horizontal => -delta.x,
                        };
                        *offset = (*offset + d).clamp(0.0, max_off);
                        node.dirty |= DirtyFlags::PAINT;
                        return;
                    }
                    WidgetKind::VirtualList {
                        axis,
                        columns,
                        item_extent,
                        item_count,
                        offset,
                        velocity,
                    } => {
                        *velocity = 0.0;
                        let r = self.layout_engine.rect(nid);
                        let view = match axis {
                            Axis::Vertical => r.height,
                            Axis::Horizontal => r.width,
                        };
                        let total_rows = item_count.get().div_ceil((*columns).max(1));
                        let max_off = (total_rows as f32 * *item_extent - view).max(0.0);
                        let d = match axis {
                            Axis::Vertical => -delta.y,
                            Axis::Horizontal => -delta.x,
                        };
                        *offset = (*offset + d).clamp(0.0, max_off);
                        node.dirty |= DirtyFlags::LAYOUT | DirtyFlags::PAINT;
                        return;
                    }
                    _ => {}
                }
            }
            cursor = parent;
        }
    }

    fn handle_drag_end(&mut self, node_id: NodeId, velocity: (f32, f32)) {
        let mut cursor = Some(node_id);
        while let Some(nid) = cursor {
            let parent = self.nodes.get(nid).and_then(|n| n.parent);
            if let Some(node) = self.nodes.get_mut(nid) {
                match &mut node.kind {
                    WidgetKind::ScrollView {
                        axis,
                        velocity: vel,
                        ..
                    }
                    | WidgetKind::VirtualList {
                        axis,
                        velocity: vel,
                        ..
                    } => {
                        *vel = match axis {
                            Axis::Vertical => velocity.1,
                            Axis::Horizontal => velocity.0,
                        };
                        return;
                    }
                    _ => {}
                }
            }
            cursor = parent;
        }
    }

    fn handle_scroll(&mut self, pos: Point, dx: f32, dy: f32) -> bool {
        let Some(mut cursor) = self.hit_test(pos).or(self.root) else {
            return false;
        };
        loop {
            let rect = self.layout_engine.rect(cursor);
            let parent = self.nodes.get(cursor).and_then(|n| n.parent);
            if let Some(node) = self.nodes.get_mut(cursor) {
                match &mut node.kind {
                    WidgetKind::ScrollView {
                        axis,
                        offset,
                        velocity,
                        content_extent,
                    } => {
                        *velocity = 0.0;
                        let view = match axis {
                            Axis::Vertical => rect.height,
                            Axis::Horizontal => rect.width,
                        };
                        let max_off = (*content_extent - view).max(0.0);
                        let d = match axis {
                            Axis::Vertical => -dy,
                            Axis::Horizontal => {
                                if dx.abs() > 0.01 {
                                    -dx
                                } else {
                                    -dy
                                }
                            }
                        };
                        *offset = (*offset + d).clamp(0.0, max_off);
                        node.dirty |= DirtyFlags::PAINT;
                        return true;
                    }
                    WidgetKind::VirtualList {
                        axis,
                        columns,
                        item_extent,
                        item_count,
                        offset,
                        velocity,
                    } => {
                        *velocity = 0.0;
                        let view = match axis {
                            Axis::Vertical => rect.height,
                            Axis::Horizontal => rect.width,
                        };
                        let total_rows = item_count.get().div_ceil((*columns).max(1));
                        let max_off = (total_rows as f32 * *item_extent - view).max(0.0);
                        let d = match axis {
                            Axis::Vertical => -dy,
                            Axis::Horizontal => {
                                if dx.abs() > 0.01 {
                                    -dx
                                } else {
                                    -dy
                                }
                            }
                        };
                        *offset = (*offset + d).clamp(0.0, max_off);
                        node.dirty |= DirtyFlags::LAYOUT | DirtyFlags::PAINT;
                        return true;
                    }
                    _ => {}
                }
            }
            let Some(p) = parent else {
                break;
            };
            cursor = p;
        }
        false
    }

    fn handle_key(&mut self, ke: &KeyEvent) -> bool {
        if !ke.pressed {
            return false;
        }

        // Ctrl+Shift+I toggles Live Inspector Overlay (PRD FR-9.2)
        if ke.modifiers.ctrl && ke.modifiers.shift && matches!(ke.code, KeyCode::KeyI) {
            self.inspector.enabled = !self.inspector.enabled;
            return true;
        }

        let Some(focused) = self.focused else {
            return false;
        };

        let mut text_changed = None;
        let mut submitted = None;
        let mut activated = false;

        if let Some(node) = self.nodes.get_mut(focused) {
            match &mut node.kind {
                WidgetKind::TextInput {
                    editor,
                    bound_signal,
                    ..
                } => {
                    let prev = editor.text.clone();
                    match &ke.code {
                        KeyCode::Backspace => editor.backspace(),
                        KeyCode::Delete => editor.delete_forward(),
                        KeyCode::ArrowLeft => editor.move_left(ke.modifiers.shift),
                        KeyCode::ArrowRight => editor.move_right(ke.modifiers.shift),
                        KeyCode::Home => editor.move_home(ke.modifiers.shift),
                        KeyCode::End => editor.move_end(ke.modifiers.shift),
                        KeyCode::KeyA if ke.modifiers.ctrl => editor.select_all(),
                        KeyCode::KeyC if ke.modifiers.ctrl => {
                            self.clipboard_text = editor.selected_text().to_string();
                        }
                        KeyCode::KeyX if ke.modifiers.ctrl => {
                            self.clipboard_text = editor.selected_text().to_string();
                            editor.delete_selection();
                        }
                        KeyCode::KeyV if ke.modifiers.ctrl => {
                            if !self.clipboard_text.is_empty() {
                                editor.insert_str(&self.clipboard_text);
                            }
                        }
                        KeyCode::Enter => {
                            submitted = Some(editor.text.clone());
                        }
                        _ => {
                            if let Some(ref txt) = ke.text
                                && !ke.modifiers.ctrl
                                && !ke.modifiers.alt
                                && !txt.chars().all(|c| c.is_control())
                            {
                                editor.insert_str(txt);
                            }
                        }
                    }
                    if editor.text != prev {
                        if let Some(sig) = bound_signal {
                            sig.set(editor.text.clone());
                        }
                        node.a11y.value = Some(editor.text.clone());
                        text_changed = Some(editor.text.clone());
                    }
                    node.dirty |= DirtyFlags::PAINT | DirtyFlags::A11Y;
                }
                WidgetKind::Button { .. }
                | WidgetKind::Checkbox { .. }
                | WidgetKind::Switch { .. } => {
                    if matches!(ke.code, KeyCode::Enter | KeyCode::Space) {
                        activated = true;
                    }
                }
                WidgetKind::Slider {
                    value, min, max, ..
                } => {
                    let step = (*max - *min) * 0.05;
                    match ke.code {
                        KeyCode::ArrowRight | KeyCode::ArrowUp => {
                            value.update(|v| *v = (*v + step).clamp(*min, *max));
                            return true;
                        }
                        KeyCode::ArrowLeft | KeyCode::ArrowDown => {
                            value.update(|v| *v = (*v - step).clamp(*min, *max));
                            return true;
                        }
                        _ => {}
                    }
                }
                _ => {}
            }
        }

        if activated {
            let center = self.layout_engine.rect(focused).center();
            self.activate_tap(focused, center);
        }

        if let Some(txt) = text_changed {
            let cb = self.nodes.get(focused).and_then(|n| match &n.kind {
                WidgetKind::TextInput { on_change, .. } => on_change.clone(),
                _ => None,
            });
            if let Some(func) = cb
                && let Ok(mut f) = func.try_borrow_mut()
            {
                f(txt);
            }
        }

        if let Some(txt) = submitted {
            let cb = self.nodes.get(focused).and_then(|n| match &n.kind {
                WidgetKind::TextInput { on_submit, .. } => on_submit.clone(),
                _ => None,
            });
            if let Some(func) = cb
                && let Ok(mut f) = func.try_borrow_mut()
            {
                f(txt);
            }
        }

        true
    }

    fn handle_ime(&mut self, ime: &craftui_core::ImeEvent) -> bool {
        let Some(focused) = self.focused else {
            return false;
        };
        let Some(node) = self.nodes.get_mut(focused) else {
            return false;
        };
        let WidgetKind::TextInput {
            editor,
            bound_signal,
            ..
        } = &mut node.kind
        else {
            return false;
        };
        match ime {
            craftui_core::ImeEvent::Preedit(s, cur) => {
                editor.preedit = if s.is_empty() { None } else { Some(s.clone()) };
                editor.preedit_cursor = *cur;
            }
            craftui_core::ImeEvent::Commit(s) => {
                editor.preedit = None;
                editor.preedit_cursor = None;
                editor.insert_str(s);
                if let Some(sig) = bound_signal {
                    sig.set(editor.text.clone());
                }
            }
            craftui_core::ImeEvent::Disabled => {
                editor.preedit = None;
                editor.preedit_cursor = None;
            }
            craftui_core::ImeEvent::Enabled => {}
        }
        node.dirty |= DirtyFlags::PAINT;
        true
    }

    /// Collect AccessKit tree update for screen readers.
    pub fn build_a11y_update(&mut self) -> Option<craftui_a11y::accesskit::TreeUpdate> {
        let root = self.root?;
        let mut snaps = Vec::new();
        self.collect_a11y_recursive(root, &mut snaps, &mut HashSet::new());
        Some(
            self.a11y_bridge
                .build_tree_update(root, self.focused, &snaps),
        )
    }

    fn collect_a11y_recursive(
        &self,
        id: NodeId,
        out: &mut Vec<A11ySnapshotNode>,
        seen: &mut HashSet<NodeId>,
    ) {
        if !seen.insert(id) {
            return;
        }
        let Some(node) = self.nodes.get(id) else {
            return;
        };
        if node.layout.hidden {
            return;
        }
        let bounds = self.layout_engine.rect(id);
        out.push(A11ySnapshotNode {
            id,
            bounds,
            meta: node.a11y.clone(),
            children: node.children.clone(),
        });
        for &c in &node.children {
            self.collect_a11y_recursive(c, out, seen);
        }
    }

    pub fn handle_a11y_action(&mut self, req: &craftui_a11y::accesskit::ActionRequest) -> bool {
        let Some(action) = self.a11y_bridge.translate_action(req) else {
            return false;
        };
        match action {
            A11yAction::Click(id) => {
                let c = self.layout_engine.rect(id).center();
                self.activate_tap(id, c);
                true
            }
            A11yAction::Focus(id) => {
                self.focused = Some(id);
                true
            }
            A11yAction::SetValue(id, v) => {
                if let Some(n) = self.nodes.get_mut(id)
                    && let WidgetKind::TextInput {
                        editor,
                        bound_signal,
                        ..
                    } = &mut n.kind
                {
                    editor.text = v.clone();
                    editor.cursor = v.len();
                    editor.anchor = v.len();
                    if let Some(sig) = bound_signal {
                        sig.set(v);
                    }
                    return true;
                }
                false
            }
            A11yAction::SetNumericValue(id, v) => {
                if let Some(n) = self.nodes.get_mut(id)
                    && let WidgetKind::Slider {
                        value, min, max, ..
                    } = &mut n.kind
                {
                    value.set((v as f32).clamp(*min, *max));
                    return true;
                }
                false
            }
            A11yAction::Increment(id) => {
                if let Some(n) = self.nodes.get_mut(id)
                    && let WidgetKind::Slider {
                        value, min, max, ..
                    } = &mut n.kind
                {
                    let step = (*max - *min) * 0.05;
                    value.update(|v| *v = (*v + step).clamp(*min, *max));
                    return true;
                }
                false
            }
            A11yAction::Decrement(id) => {
                if let Some(n) = self.nodes.get_mut(id)
                    && let WidgetKind::Slider {
                        value, min, max, ..
                    } = &mut n.kind
                {
                    let step = (*max - *min) * 0.05;
                    value.update(|v| *v = (*v - step).clamp(*min, *max));
                    return true;
                }
                false
            }
            A11yAction::ScrollIntoView(_) => false,
        }
    }
}
