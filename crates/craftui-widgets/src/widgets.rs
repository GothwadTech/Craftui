use craftui_a11y::A11yMetadata;
use craftui_core::{
    Axis, BlendMode, Border, BoxShadow, Brush, Color, CornerRadii, CursorIcon, Fit, PlatformViewId,
    Point, Rect, Size, TextureId, ThemeTokens, Transform2D, UiEvent,
};
use craftui_reactive::{Computed, ReadSignal, Signal};
use craftui_render::PaintScene;
use craftui_text::{TextEditorState, TextEngine, TextStyle};
use std::cell::RefCell;
use std::rc::Rc;

/// A property value that can be either static or lazily read from reactive signals.
#[derive(Clone)]
pub enum ReactiveValue<T: Clone + 'static> {
    Static(T),
    Dynamic(Rc<dyn Fn() -> T>),
}

impl<T: Clone + 'static> ReactiveValue<T> {
    pub fn get(&self) -> T {
        match self {
            Self::Static(v) => v.clone(),
            Self::Dynamic(f) => f(),
        }
    }

    pub fn is_dynamic(&self) -> bool {
        matches!(self, Self::Dynamic(_))
    }
}

impl<T: Clone + Default + 'static> Default for ReactiveValue<T> {
    fn default() -> Self {
        Self::Static(T::default())
    }
}

impl<T: Clone + 'static> From<T> for ReactiveValue<T> {
    fn from(value: T) -> Self {
        Self::Static(value)
    }
}

impl<T: Clone + 'static> From<Signal<T>> for ReactiveValue<T> {
    fn from(sig: Signal<T>) -> Self {
        Self::Dynamic(Rc::new(move || sig.get()))
    }
}

impl<T: Clone + 'static> From<ReadSignal<T>> for ReactiveValue<T> {
    fn from(sig: ReadSignal<T>) -> Self {
        Self::Dynamic(Rc::new(move || sig.get()))
    }
}

impl<T: Clone + 'static> From<Computed<T>> for ReactiveValue<T> {
    fn from(comp: Computed<T>) -> Self {
        Self::Dynamic(Rc::new(move || comp.get()))
    }
}

impl From<&str> for ReactiveValue<String> {
    fn from(value: &str) -> Self {
        Self::Static(value.to_string())
    }
}

impl<F> From<F> for ReactiveValue<String>
where
    F: Fn() -> String + 'static,
{
    fn from(f: F) -> Self {
        Self::Dynamic(Rc::new(f))
    }
}

/// Visual box decoration shared by containers, buttons, cards, and inputs.
#[derive(Clone)]
pub struct BoxStyle {
    pub background: Option<ReactiveValue<Brush>>,
    pub border: Option<Border>,
    pub border_color: Option<ReactiveValue<Color>>,
    pub shadow: Option<BoxShadow>,
    pub shadows: Vec<BoxShadow>,
    pub backdrop_blur: f32,
    pub backdrop_tint: Color,
    pub transform: Transform2D,
    pub blend_mode: BlendMode,
    pub cursor: Option<CursorIcon>,
    pub radius: CornerRadii,
    pub opacity: f32,
    pub clip: bool,
    pub hit_slop: f32,
}

impl Default for BoxStyle {
    fn default() -> Self {
        Self::new()
    }
}

impl BoxStyle {
    pub fn new() -> Self {
        Self {
            background: None,
            border: None,
            border_color: None,
            shadow: None,
            shadows: Vec::new(),
            backdrop_blur: 0.0,
            backdrop_tint: Color::TRANSPARENT,
            transform: Transform2D::IDENTITY,
            blend_mode: BlendMode::Normal,
            cursor: None,
            radius: CornerRadii::ZERO,
            opacity: 1.0,
            clip: false,
            hit_slop: 0.0,
        }
    }

    pub fn with_shadow(mut self, shadow: BoxShadow) -> Self {
        self.shadows.push(shadow);
        self
    }

    pub fn with_backdrop_blur(mut self, blur_radius: f32, tint: Color) -> Self {
        self.backdrop_blur = blur_radius.max(0.0);
        self.backdrop_tint = tint;
        self
    }

    pub fn with_transform(mut self, transform: Transform2D) -> Self {
        self.transform = transform;
        self
    }

    pub fn with_blend_mode(mut self, blend_mode: BlendMode) -> Self {
        self.blend_mode = blend_mode;
        self
    }

    pub fn with_cursor(mut self, cursor: CursorIcon) -> Self {
        self.cursor = Some(cursor);
        self
    }
}

pub type TapHandler = Rc<RefCell<dyn FnMut()>>;
pub type HoverCallback = Rc<RefCell<dyn FnMut(bool)>>;
pub type BoolCallback = Rc<RefCell<dyn FnMut(bool)>>;
pub type FloatCallback = Rc<RefCell<dyn FnMut(f32)>>;
pub type IndexCallback = Rc<RefCell<dyn FnMut(usize)>>;
pub type TextCallback = Rc<RefCell<dyn FnMut(String)>>;
pub type PinchRotateCallback = Rc<RefCell<dyn FnMut(Point, f32, f32)>>;
pub type CanvasPaintFn = Rc<dyn Fn(Rect, &mut PaintScene)>;
pub type CanvasPointerFn = Rc<RefCell<dyn FnMut(Rect, &craftui_core::PointerEvent)>>;

/// Public extension trait enabling downstream applications to implement custom
/// layout, painting, input handling, and accessibility widgets without editing `craftui-widgets`.
pub trait CustomWidget {
    fn debug_name(&self) -> &'static str {
        "CustomWidget"
    }

    fn measure(
        &mut self,
        constraints: craftui_core::Constraints,
        _text_engine: &mut TextEngine,
        _theme: &ThemeTokens,
    ) -> Size {
        let w = if constraints.max_width.is_finite() {
            constraints.max_width.min(120.0)
        } else {
            120.0
        };
        let h = if constraints.max_height.is_finite() {
            constraints.max_height.min(40.0)
        } else {
            40.0
        };
        Size::new(w.max(constraints.min_width), h.max(constraints.min_height))
    }

    fn paint(
        &mut self,
        rect: Rect,
        scene: &mut PaintScene,
        text_engine: &mut TextEngine,
        theme: &ThemeTokens,
        focused: bool,
    );

    fn handle_event(&mut self, _rect: Rect, _event: &UiEvent) -> bool {
        false
    }

    fn a11y(&self) -> A11yMetadata {
        A11yMetadata::default()
    }
}

/// Concrete widget behavior stored in a retained [`WidgetNode`](crate::WidgetNode).
#[derive(Clone)]
pub enum WidgetKind {
    Container,
    Spacer,
    SafeArea,
    Divider {
        axis: Axis,
        thickness: f32,
        color: Option<ReactiveValue<Color>>,
    },
    Text {
        content: ReactiveValue<String>,
        style: ReactiveValue<TextStyle>,
    },
    Icon {
        svg: ReactiveValue<String>,
        size: f32,
        color: Option<ReactiveValue<Color>>,
    },
    Button {
        label: ReactiveValue<String>,
        disabled: ReactiveValue<bool>,
        foreground: Option<ReactiveValue<Color>>,
        on_tap: Option<TapHandler>,
        hovered: bool,
        pressed: bool,
    },
    Image {
        texture: ReactiveValue<Option<TextureId>>,
        fit: Fit,
        radius: CornerRadii,
        opacity: f32,
    },
    Canvas {
        on_paint: CanvasPaintFn,
        on_pointer: Option<CanvasPointerFn>,
    },
    ScrollView {
        axis: Axis,
        offset: f32,
        velocity: f32,
        content_extent: f32,
    },
    VirtualList {
        axis: Axis,
        columns: usize,
        item_extent: f32,
        item_count: ReactiveValue<usize>,
        offset: f32,
        velocity: f32,
    },
    TextInput {
        editor: TextEditorState,
        placeholder: String,
        style: TextStyle,
        multiline: bool,
        bound_signal: Option<Signal<String>>,
        on_change: Option<TextCallback>,
        on_submit: Option<TextCallback>,
    },
    Checkbox {
        checked: Signal<bool>,
        label: String,
        on_change: Option<BoolCallback>,
    },
    Switch {
        checked: Signal<bool>,
        label: String,
        on_change: Option<BoolCallback>,
    },
    Slider {
        value: Signal<f32>,
        min: f32,
        max: f32,
        label: String,
        on_change: Option<FloatCallback>,
    },
    Tabs {
        tabs: ReactiveValue<Vec<String>>,
        active: Signal<usize>,
        on_change: Option<IndexCallback>,
    },
    Select {
        options: ReactiveValue<Vec<String>>,
        selected: Signal<usize>,
        open: bool,
        dropdown_scroll: f32,
        placeholder: String,
        on_change: Option<IndexCallback>,
    },
    RadioGroup {
        options: ReactiveValue<Vec<String>>,
        selected: Signal<usize>,
        axis: Axis,
        on_change: Option<IndexCallback>,
    },
    PopupMenu {
        open: ReactiveValue<bool>,
        items: ReactiveValue<Vec<String>>,
        anchor: Option<Point>,
        on_select: Option<IndexCallback>,
        on_close: Option<TapHandler>,
    },
    Tooltip {
        text: ReactiveValue<String>,
    },
    ProgressIndicator {
        progress: ReactiveValue<Option<f32>>,
        phase: f32,
    },
    Overlay {
        open: ReactiveValue<bool>,
        on_close: Option<TapHandler>,
        is_bottom_sheet: bool,
    },
    PlatformView {
        id: PlatformViewId,
    },
    Custom(Rc<RefCell<dyn CustomWidget>>),
}
