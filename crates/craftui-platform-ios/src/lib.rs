//! iOS platform adapter (`UIKit` lifecycle hooks, `CAMetalLayer` + `CADisplayLink` frame pacing,
//! Dynamic Island / Home Indicator SafeArea, `UITextInput` IME, and `CVPixelBuffer` external textures).

use craftui_core::{
    ImeEvent, Insets, Modifiers, PlatformViewId, Point, PointerButton, PointerEvent, PointerId,
    PointerPhase, Size, UiDispatcher, UiEvent, WindowMetrics,
};
use craftui_reactive::Scope;
use craftui_render::{PaintScene, TextureRegistry};
use craftui_widgets::{WidgetKind, WidgetTree};
use std::ffi::c_void;

/// iOS native view hosting strategy inside CraftUI `PlatformView`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IosPlatformViewMode {
    /// Zero-copy `CVPixelBufferRef` -> `MTLTexture` imported directly into Vello (camera preview / video).
    ExternalCvPixelBuffer { cv_pixel_buffer_ptr: usize },
    /// Transparent UIKit `UIView` overlay cutout coordinated via `PaintCommand::PlatformViewHole`.
    HolePunchUiView { view_id: PlatformViewId },
}

/// iOS window & notch / Dynamic Island safe area insets in logical points (`pt`).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct IosSafeAreaInsets {
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
    pub left: f32,
}

impl From<IosSafeAreaInsets> for Insets {
    fn from(v: IosSafeAreaInsets) -> Self {
        Insets::new(v.top, v.right, v.bottom, v.left)
    }
}

/// `UIResponder.keyboardWillChangeFrameNotification` transition state.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct IosKeyboardFrame {
    pub overlap_height_pt: f32,
    pub visible: bool,
}

/// Desired UIKit keyboard state emitted after each iOS frame step.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IosImeCommand {
    BecomeFirstResponder,
    ResignFirstResponder,
    SyncText(String),
}

/// iOS host controller bridging `CADisplayLink` ticks, UIKit touches, SafeArea, and Metal rendering.
pub struct IosAppHost {
    pub scope: Scope,
    pub tree: WidgetTree,
    pub scene: PaintScene,
    pub textures: TextureRegistry,
    pub dispatcher: UiDispatcher,
    last_first_responder: bool,
}

impl IosAppHost {
    pub fn new(dispatcher: UiDispatcher, width_pt: f32, height_pt: f32, scale_factor: f32) -> Self {
        let scope = Scope::root();
        let mut tree = WidgetTree::new(scope);
        tree.metrics = WindowMetrics {
            logical_size: Size::new(width_pt.max(1.0), height_pt.max(1.0)),
            scale_factor: scale_factor.max(1.0),
            safe_area: Insets::ZERO,
            keyboard_inset: 0.0,
        };
        Self {
            scope,
            tree,
            scene: PaintScene::new(),
            textures: TextureRegistry::new(),
            dispatcher,
            last_first_responder: false,
        }
    }

    pub fn set_safe_area(&mut self, safe_area: IosSafeAreaInsets) {
        self.tree.metrics.safe_area = safe_area.into();
    }

    pub fn set_keyboard_frame(&mut self, frame: IosKeyboardFrame) {
        self.tree.metrics.keyboard_inset = if frame.visible {
            frame.overlap_height_pt.max(0.0)
        } else {
            0.0
        };
    }

    pub fn resize_pt(&mut self, width_pt: f32, height_pt: f32, scale_factor: f32) {
        self.tree.handle_event(&UiEvent::Resize {
            width: width_pt.max(1.0),
            height: height_pt.max(1.0),
            scale_factor: scale_factor.max(1.0),
        });
    }

    pub fn push_ui_touch(
        &mut self,
        touch_id: u64,
        phase: PointerPhase,
        x_pt: f32,
        y_pt: f32,
        timestamp_ms: u64,
    ) -> bool {
        self.tree.handle_event(&UiEvent::Pointer(PointerEvent {
            id: PointerId(touch_id),
            phase,
            position: Point::new(x_pt, y_pt),
            button: Some(PointerButton::Primary),
            modifiers: Modifiers::default(),
            is_touch: true,
            timestamp_ms,
        }))
    }

    pub fn push_marked_text(&mut self, marked: &str, cursor_end: usize) -> bool {
        self.tree.handle_event(&UiEvent::Ime(ImeEvent::Preedit(
            marked.to_string(),
            Some((cursor_end, cursor_end)),
        )))
    }

    pub fn push_insert_text(&mut self, committed: &str) -> bool {
        self.tree
            .handle_event(&UiEvent::Ime(ImeEvent::Commit(committed.to_string())))
    }

    pub fn display_link_tick(&mut self, dt_sec: f32) -> (bool, Vec<IosImeCommand>) {
        let _ = self.dispatcher.drain();
        let animating = self.tree.tick_animations(dt_sec);
        let _ = self.tree.layout(self.tree.metrics);
        self.tree.paint(&mut self.scene);

        let mut cmds = Vec::new();
        let focused_text = self.tree.focused.and_then(|id| {
            self.tree.nodes.get(id).and_then(|n| match &n.kind {
                WidgetKind::TextInput { editor, .. } => Some(editor.text.clone()),
                _ => None,
            })
        });
        let needs_kb = focused_text.is_some();
        if needs_kb != self.last_first_responder {
            self.last_first_responder = needs_kb;
            cmds.push(if needs_kb {
                IosImeCommand::BecomeFirstResponder
            } else {
                IosImeCommand::ResignFirstResponder
            });
        }
        if let Some(txt) = focused_text {
            cmds.push(IosImeCommand::SyncText(txt));
        }

        (animating, cmds)
    }
}

/// FFI-safe `CAMetalLayer` descriptor passed from Swift/ObjC `UIViewController`.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct IosMetalLayerDescriptor {
    pub ca_metal_layer: *mut c_void,
    pub width_pt: f32,
    pub height_pt: f32,
    pub contents_scale: f32,
}

#[cfg(target_os = "ios")]
pub mod ios_metal {
    use super::*;

    /// Initialize a `wgpu::Surface` targeting a `CAMetalLayer` pointer from UIKit.
    ///
    /// # Safety
    /// `desc.ca_metal_layer` must point to a valid retained `CALayer` / `CAMetalLayer`.
    pub unsafe fn create_metal_surface(
        instance: &wgpu::Instance,
        desc: IosMetalLayerDescriptor,
    ) -> craftui_core::Result<wgpu::Surface<'static>> {
        let target = wgpu::SurfaceTargetUnsafe::CoreAnimationLayer(desc.ca_metal_layer);
        // SAFETY: forwarded from caller invariant that `desc.ca_metal_layer` is a live `CAMetalLayer`.
        unsafe { instance.create_surface_unsafe(target) }.map_err(|e| {
            craftui_core::CraftError::Platform(format!("iOS CAMetalLayer surface error: {e}"))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use craftui_core::Length;
    use craftui_layout::LayoutStyle;
    use craftui_reactive::reset_runtime;

    #[test]
    fn ios_safe_area_keyboard_and_touch_flow() {
        reset_runtime();
        let mut host = IosAppHost::new(UiDispatcher::new(), 393.0, 852.0, 3.0);
        // iPhone Dynamic Island + Home indicator safe area
        host.set_safe_area(IosSafeAreaInsets {
            top: 59.0,
            right: 0.0,
            bottom: 34.0,
            left: 0.0,
        });
        host.set_keyboard_frame(IosKeyboardFrame {
            overlap_height_pt: 312.0,
            visible: true,
        });
        let eff = host.tree.metrics.effective_safe_area();
        assert_eq!(eff.top, 59.0);
        assert_eq!(eff.bottom, 312.0);

        let clicked = host.scope.signal(false);
        let btn = host
            .tree
            .button(host.scope, "Scan", move || clicked.set(true));
        let mut ls = LayoutStyle::row().with_size(Length::Dp(200.0), Length::Dp(48.0));
        ls.margin = Insets::new(80.0, 0.0, 0.0, 20.0);
        host.tree.set_layout(btn, ls);
        host.tree.set_root(btn);
        let _ = host.display_link_tick(0.016);

        let c = host.tree.rect(btn).center();
        host.push_ui_touch(1, PointerPhase::Down, c.x, c.y, 10);
        host.push_ui_touch(1, PointerPhase::Up, c.x, c.y, 35);
        assert!(clicked.get());
    }
}
