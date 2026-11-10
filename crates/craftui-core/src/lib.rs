//! Core geometry, color, style, event, theme, and dispatcher primitives for CraftUI.

pub mod color;
pub mod dispatch;
pub mod error;
pub mod events;
pub mod geometry;
pub mod ids;
pub mod style;
pub mod theme;

pub use color::Color;
pub use dispatch::UiDispatcher;
pub use error::{CraftError, Result};
pub use events::{
    ImeEvent, KeyCode, KeyEvent, Modifiers, PointerButton, PointerEvent, PointerId, PointerPhase,
    ScrollDelta, UiEvent,
};
pub use geometry::{
    Breakpoint, Constraints, CornerRadii, Insets, Point, Rect, Size, WindowMetrics,
};
pub use ids::{DirtyFlags, NodeId, PlatformViewId, ScopeId, TextureId};
pub use style::{
    Alignment, Axis, Border, BoxShadow, Brush, ColorStop, Direction, Fit, Justify, Length, auto,
    dp, pct, px,
};
pub use theme::{ThemeMode, ThemeTokens};
