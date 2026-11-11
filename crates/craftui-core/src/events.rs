use crate::Point;
use serde::{Deserialize, Serialize};

/// Unique pointer/finger identifier across multi-touch events.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub struct PointerId(pub u64);

/// Mouse / pointer button.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PointerButton {
    Primary,
    Secondary,
    Middle,
    Back,
    Forward,
}

/// Pointer lifecycle phase.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PointerPhase {
    Down,
    Move,
    Up,
    Cancel,
}

/// Unified mouse/touch/stylus pointer event.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct PointerEvent {
    pub id: PointerId,
    pub phase: PointerPhase,
    pub position: Point,
    pub button: Option<PointerButton>,
    pub modifiers: Modifiers,
    pub is_touch: bool,
    pub timestamp_ms: u64,
}

/// Keyboard modifier flags.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub struct Modifiers {
    pub shift: bool,
    pub ctrl: bool,
    pub alt: bool,
    pub super_key: bool,
}

impl Modifiers {
    /// Primary shortcut modifier (`Cmd` on macOS/iOS/browser-Mac, `Ctrl` everywhere else).
    #[inline]
    pub fn command(self) -> bool {
        if cfg!(any(
            target_os = "macos",
            target_os = "ios",
            target_arch = "wasm32"
        )) {
            self.super_key || self.ctrl
        } else {
            self.ctrl
        }
    }

    /// Word navigation / deletion modifier (`Alt`/`Option` on macOS/iOS, `Ctrl` elsewhere).
    #[inline]
    pub fn word_jump(self) -> bool {
        if cfg!(any(target_os = "macos", target_os = "ios")) {
            self.alt
        } else {
            self.ctrl
        }
    }
}

/// Platform-neutral logical key codes.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum KeyCode {
    Character(String),
    Enter,
    Tab,
    Backspace,
    Delete,
    Escape,
    Space,
    ArrowLeft,
    ArrowRight,
    ArrowUp,
    ArrowDown,
    Home,
    End,
    PageUp,
    PageDown,
    KeyI,
    KeyA,
    KeyC,
    KeyV,
    KeyX,
    KeyY,
    KeyZ,
    Unknown,
}

impl KeyCode {
    /// Returns true if this key corresponds to ASCII letter `ch` (case-insensitive),
    /// matching both named `Key*` variants and [`KeyCode::Character`].
    #[inline]
    pub fn matches_ascii_letter(&self, ch: char) -> bool {
        let lower = ch.to_ascii_lowercase();
        match self {
            Self::KeyA => lower == 'a',
            Self::KeyC => lower == 'c',
            Self::KeyI => lower == 'i',
            Self::KeyV => lower == 'v',
            Self::KeyX => lower == 'x',
            Self::KeyY => lower == 'y',
            Self::KeyZ => lower == 'z',
            Self::Character(s) => {
                let mut chars = s.chars();
                match (chars.next(), chars.next()) {
                    (Some(c), None) => c.to_ascii_lowercase() == lower,
                    _ => false,
                }
            }
            _ => false,
        }
    }
}

/// Keyboard press/release event.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeyEvent {
    pub code: KeyCode,
    pub text: Option<String>,
    pub pressed: bool,
    pub repeat: bool,
    pub modifiers: Modifiers,
}

/// Mobile & desktop Input Method Editor (IME) composition event.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ImeEvent {
    Enabled,
    Preedit(String, Option<(usize, usize)>),
    Commit(String),
    Disabled,
}

/// Mouse wheel or trackpad scroll delta.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct ScrollDelta {
    pub x: f32,
    pub y: f32,
    pub position: Point,
}

/// Top-level UI event routed into the widget tree & gesture arena.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum UiEvent {
    Pointer(PointerEvent),
    Key(KeyEvent),
    Ime(ImeEvent),
    Scroll(ScrollDelta),
    Pinch {
        factor: f32,
        center: Point,
    },
    Resize {
        width: f32,
        height: f32,
        scale_factor: f32,
    },
    FocusChanged(bool),
}
