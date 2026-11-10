use craftui_core::{
    Axis, Border, BoxShadow, Brush, CornerRadii, Fit, PlatformViewId, Rect, TextureId,
};
use craftui_reactive::{Computed, ReadSignal, Signal};
use craftui_render::PaintScene;
use craftui_text::{TextEditorState, TextStyle};
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
#[derive(Clone, Default)]
pub struct BoxStyle {
    pub background: Option<ReactiveValue<Brush>>,
    pub border: Option<Border>,
    pub shadow: Option<BoxShadow>,
    pub radius: CornerRadii,
    pub opacity: f32,
    pub clip: bool,
    pub hit_slop: f32,
}

impl BoxStyle {
    pub fn new() -> Self {
        Self {
            opacity: 1.0,
            ..Default::default()
        }
    }
}

pub type TapHandler = Rc<RefCell<dyn FnMut()>>;
pub type TextCallback = Rc<RefCell<dyn FnMut(String)>>;
pub type CanvasPaintFn = Rc<dyn Fn(Rect, &mut PaintScene)>;
pub type CanvasPointerFn = Rc<RefCell<dyn FnMut(Rect, &craftui_core::PointerEvent)>>;

/// Concrete widget behavior stored in a retained [`WidgetNode`](crate::WidgetNode).
#[derive(Clone)]
pub enum WidgetKind {
    Container,
    Spacer,
    SafeArea,
    Text {
        content: ReactiveValue<String>,
        style: ReactiveValue<TextStyle>,
    },
    Button {
        label: ReactiveValue<String>,
        disabled: ReactiveValue<bool>,
        foreground: Option<ReactiveValue<craftui_core::Color>>,
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
        bound_signal: Option<Signal<String>>,
        on_change: Option<TextCallback>,
        on_submit: Option<TextCallback>,
    },
    Checkbox {
        checked: Signal<bool>,
        label: String,
    },
    Switch {
        checked: Signal<bool>,
        label: String,
    },
    Slider {
        value: Signal<f32>,
        min: f32,
        max: f32,
        label: String,
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
}
