//! iOS platform adapter (`UIKit` lifecycle C-ABI entrypoints, `CAMetalLayer` + `CADisplayLink`
//! frame pacing, Dynamic Island / Home Indicator SafeArea, `UITextInput` IME, and `CVPixelBuffer` external textures).

use craftui_core::{
    ImeEvent, Insets, KeyCode, KeyEvent, Modifiers, NodeId, PlatformViewId, Point, PointerButton,
    PointerEvent, PointerId, PointerPhase, Size, TextureId, UiDispatcher, UiEvent, WindowMetrics,
};
use craftui_reactive::Scope;
use craftui_render::{PaintScene, TextureHandle, TextureRegistry};
use craftui_widgets::{WidgetKind, WidgetTree};
use std::ffi::c_void;

/// iOS native view hosting strategy inside CraftUI `PlatformView`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IosPlatformViewMode {
    /// Zero-copy `CVPixelBufferRef` -> `MTLTexture` / `TextureHandle` composited directly inside Vello.
    ExternalCvPixelBuffer { cv_pixel_buffer_ptr: usize },
    /// Transparent UIKit `UIView` overlay cutout coordinated via `PaintCommand::PlatformViewHole`.
    HolePunchUiView { view_id: PlatformViewId },
}

/// Pixel format reported by a CoreVideo [`CvPixelBufferBridge`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CvPixelBufferFormat {
    Rgba8,
    /// `kCVPixelFormatType_32BGRA` (native iOS `AVCaptureVideoDataOutput` / `CAMetalLayer` format).
    Bgra8,
}

/// CPU/IOSurface frame payload acquired from a CoreVideo `CVPixelBufferRef`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CvPixelBufferFrame {
    pub width: u32,
    pub height: u32,
    pub bytes_per_row: usize,
    pub format: CvPixelBufferFormat,
    pub bytes: Vec<u8>,
}

/// Trait bridging native `CVPixelBufferRef` (Swift/ObjC camera, video, or ARKit feeds) into CraftUI.
pub trait CvPixelBufferBridge: Send + Sync {
    fn acquire_latest_frame(&mut self) -> Option<CvPixelBufferFrame>;
}

/// Convert packed or strided `BGRA8` pixels from `kCVPixelFormatType_32BGRA` into packed straight `RGBA8`.
pub fn bgra_frame_to_packed_rgba(frame: &CvPixelBufferFrame) -> Vec<u8> {
    let w = frame.width as usize;
    let h = frame.height as usize;
    let row_bytes = w.saturating_mul(4);
    let mut out = vec![0u8; row_bytes.saturating_mul(h)];
    let stride = frame.bytes_per_row.max(row_bytes);

    for y in 0..h {
        let src_start = y * stride;
        let dst_start = y * row_bytes;
        if src_start + row_bytes > frame.bytes.len() {
            break;
        }
        let src_row = &frame.bytes[src_start..src_start + row_bytes];
        let dst_row = &mut out[dst_start..dst_start + row_bytes];
        match frame.format {
            CvPixelBufferFormat::Rgba8 => dst_row.copy_from_slice(src_row),
            CvPixelBufferFormat::Bgra8 => {
                for (dst_px, src_px) in dst_row
                    .as_chunks_mut::<4>()
                    .0
                    .iter_mut()
                    .zip(src_row.as_chunks::<4>().0)
                {
                    dst_px[0] = src_px[2];
                    dst_px[1] = src_px[1];
                    dst_px[2] = src_px[0];
                    dst_px[3] = src_px[3];
                }
            }
        }
    }
    out
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

/// C-ABI callback invoked by UIKit launcher to populate the root CraftUI [`WidgetTree`].
pub type IosBuildUiCallback = extern "C" fn(host: *mut IosAppHost, user_data: *mut c_void);

/// iOS host controller bridging `CADisplayLink` ticks, UIKit touches, SafeArea, and Metal rendering.
pub struct IosAppHost {
    pub scope: Scope,
    pub tree: WidgetTree,
    pub scene: PaintScene,
    pub textures: TextureRegistry,
    pub dispatcher: UiDispatcher,
    last_first_responder: bool,
    last_synced_text: Option<String>,
    needs_redraw: bool,
    pub last_ime_commands: Vec<IosImeCommand>,
    #[cfg(all(target_os = "ios", any(feature = "gpu-sdf", feature = "gpu-vello")))]
    gpu: Option<craftui_render::GpuSurfacePipeline>,
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
            last_synced_text: None,
            needs_redraw: true,
            last_ime_commands: Vec::new(),
            #[cfg(all(target_os = "ios", any(feature = "gpu-sdf", feature = "gpu-vello")))]
            gpu: None,
        }
    }

    pub fn request_redraw(&mut self) {
        self.needs_redraw = true;
    }

    /// Returns whether `CADisplayLink` should remain active for ongoing animations, pending redraws, or timers.
    pub fn needs_display_link(&self, animating: bool) -> bool {
        if animating || self.needs_redraw {
            return true;
        }
        let now_ms = craftui_core::monotonic_ms();
        craftui_reactive::next_timer_delay_ms(now_ms).is_some()
            || self.tree.next_internal_delay_ms(now_ms).is_some()
    }

    /// Build a CraftUI root view inside Rust before handing control to the UIKit CADisplayLink loop.
    pub fn mount_root<F>(&mut self, build_ui: F)
    where
        F: FnOnce(Scope, &mut WidgetTree, &mut TextureRegistry, &UiDispatcher) -> NodeId,
    {
        let root = build_ui(
            self.scope,
            &mut self.tree,
            &mut self.textures,
            &self.dispatcher,
        );
        self.tree.set_root(root);
        self.needs_redraw = true;
    }

    pub fn set_safe_area(&mut self, safe_area: IosSafeAreaInsets) {
        let next: Insets = safe_area.into();
        if self.tree.metrics.safe_area != next {
            self.tree.metrics.safe_area = next;
            self.needs_redraw = true;
        }
    }

    pub fn set_keyboard_frame(&mut self, frame: IosKeyboardFrame) {
        let next = if frame.visible {
            frame.overlap_height_pt.max(0.0)
        } else {
            0.0
        };
        if (self.tree.metrics.keyboard_inset - next).abs() > 0.01 {
            self.tree.metrics.keyboard_inset = next;
            self.needs_redraw = true;
        }
    }

    pub fn resize_pt(&mut self, width_pt: f32, height_pt: f32, scale_factor: f32) {
        let w = width_pt.max(1.0);
        let h = height_pt.max(1.0);
        let s = scale_factor.max(1.0);
        if (self.tree.metrics.logical_size.width - w).abs() > 0.01
            || (self.tree.metrics.logical_size.height - h).abs() > 0.01
            || (self.tree.metrics.scale_factor - s).abs() > 0.001
        {
            self.tree.handle_event(&UiEvent::Resize {
                width: w,
                height: h,
                scale_factor: s,
            });
            self.needs_redraw = true;
        }
        #[cfg(all(target_os = "ios", any(feature = "gpu-sdf", feature = "gpu-vello")))]
        if let Some(gpu) = self.gpu.as_mut() {
            let (pw, ph) = self.tree.metrics.physical_size();
            gpu.resize(pw, ph);
        }
    }

    pub fn push_ui_touch(
        &mut self,
        touch_id: u64,
        phase: PointerPhase,
        x_pt: f32,
        y_pt: f32,
        timestamp_ms: u64,
    ) -> bool {
        let handled = self.tree.handle_event(&UiEvent::Pointer(PointerEvent {
            id: PointerId(touch_id),
            phase,
            position: Point::new(x_pt, y_pt),
            button: Some(PointerButton::Primary),
            modifiers: Modifiers::default(),
            is_touch: true,
            timestamp_ms,
        }));
        if handled {
            self.needs_redraw = true;
        }
        handled
    }

    pub fn push_marked_text(&mut self, marked: &str, cursor_end: usize) -> bool {
        let handled = self.tree.handle_event(&UiEvent::Ime(ImeEvent::Preedit(
            marked.to_string(),
            Some((cursor_end, cursor_end)),
        )));
        if handled {
            self.needs_redraw = true;
        }
        handled
    }

    pub fn push_insert_text(&mut self, committed: &str) -> bool {
        let handled = self
            .tree
            .handle_event(&UiEvent::Ime(ImeEvent::Commit(committed.to_string())));
        if handled {
            self.needs_redraw = true;
        }
        handled
    }

    pub fn push_delete_backward(&mut self) -> bool {
        let handled = self.tree.handle_event(&UiEvent::Key(KeyEvent {
            code: KeyCode::Backspace,
            text: None,
            pressed: true,
            repeat: false,
            modifiers: Modifiers::default(),
        }));
        if handled {
            self.needs_redraw = true;
        }
        handled
    }

    /// Pull the latest frame from a [`CvPixelBufferBridge`] into a CraftUI [`TextureHandle`].
    pub fn sync_cv_pixel_buffer(
        &mut self,
        bridge: &mut dyn CvPixelBufferBridge,
        slot: &mut Option<TextureHandle>,
    ) -> Option<TextureId> {
        let frame = bridge.acquire_latest_frame()?;
        if frame.width == 0 || frame.height == 0 {
            return None;
        }
        let rgba = bgra_frame_to_packed_rgba(&frame);
        self.needs_redraw = true;
        if let Some(existing) = slot.as_ref() {
            existing
                .buffer
                .replace_rgba(frame.width, frame.height, rgba);
            Some(existing.id)
        } else {
            let handle = self
                .textures
                .create_from_rgba(frame.width, frame.height, rgba);
            let id = handle.id;
            *slot = Some(handle);
            Some(id)
        }
    }

    pub fn display_link_tick_with_redraw(
        &mut self,
        dt_sec: f32,
        now_ms: u64,
    ) -> (bool, bool, Vec<IosImeCommand>) {
        let timers = craftui_reactive::tick_timers_at(now_ms);
        let jobs = self.dispatcher.drain();
        let blink = self.tree.tick_blink_timers(now_ms);
        let anim_tick = self.tree.tick_animations(dt_sec);
        let dirty = self.tree.layout(self.tree.metrics);
        let animating = anim_tick || self.tree.has_active_animations();
        let should_paint =
            self.needs_redraw || timers > 0 || jobs > 0 || blink || anim_tick || dirty > 0;
        if should_paint {
            self.tree.paint(&mut self.scene);
            self.needs_redraw = false;
        }

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
            if needs_kb {
                cmds.push(IosImeCommand::BecomeFirstResponder);
            } else {
                self.last_synced_text = None;
                cmds.push(IosImeCommand::ResignFirstResponder);
            }
        }
        if let Some(txt) = focused_text
            && self.last_synced_text.as_ref() != Some(&txt)
        {
            self.last_synced_text = Some(txt.clone());
            cmds.push(IosImeCommand::SyncText(txt));
        }

        #[cfg(all(target_os = "ios", any(feature = "gpu-sdf", feature = "gpu-vello")))]
        if should_paint && let Some(gpu) = self.gpu.as_mut() {
            let bg = self.tree.theme().background;
            let sf = self.tree.metrics.scale_factor;
            let _ = gpu.render_frame(
                &self.scene,
                sf,
                bg,
                &self.textures,
                &mut self.tree.text_engine,
            );
        }

        self.last_ime_commands = cmds.clone();
        (animating, should_paint, cmds)
    }

    pub fn display_link_tick(&mut self, dt_sec: f32) -> (bool, Vec<IosImeCommand>) {
        let (animating, _did_paint, cmds) =
            self.display_link_tick_with_redraw(dt_sec, craftui_core::monotonic_ms());
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

#[cfg(all(target_os = "ios", any(feature = "gpu-sdf", feature = "gpu-vello")))]
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

    /// Attach a live Metal [`craftui_render::GpuSurfacePipeline`] to an [`IosAppHost`].
    ///
    /// # Safety
    /// `desc.ca_metal_layer` must remain valid until the host is destroyed or replaced.
    pub unsafe fn attach_metal_pipeline(
        host: &mut IosAppHost,
        desc: IosMetalLayerDescriptor,
    ) -> craftui_core::Result<()> {
        host.resize_pt(desc.width_pt, desc.height_pt, desc.contents_scale);
        let (pw, ph) = host.tree.metrics.physical_size();
        let target = wgpu::SurfaceTargetUnsafe::CoreAnimationLayer(desc.ca_metal_layer);
        // SAFETY: forwarded from caller invariant that `desc.ca_metal_layer` outlives `host.gpu`.
        let pipeline =
            unsafe { craftui_render::GpuSurfacePipeline::new_unsafe(target, pw, ph, false) }?;
        host.gpu = Some(pipeline);
        host.needs_redraw = true;
        Ok(())
    }
}

// === C-ABI / UIKit Entrypoints callable directly from Swift/ObjC `UIViewController` ===

/// Allocate and return a new heap-boxed [`IosAppHost`] for UIKit lifecycle ownership.
#[unsafe(no_mangle)]
pub extern "C" fn craftui_ios_app_create(
    width_pt: f32,
    height_pt: f32,
    scale_factor: f32,
) -> *mut IosAppHost {
    let host = IosAppHost::new(UiDispatcher::new(), width_pt, height_pt, scale_factor);
    Box::into_raw(Box::new(host))
}

/// Launch a CraftUI iOS host attached to a `CAMetalLayer` descriptor and optional C builder callback.
///
/// # Safety
/// `layer.ca_metal_layer` (on iOS) must be valid or null if running offscreen; `user_data` is passed
/// verbatim to `build` if provided.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn craftui_ios_app_launch(
    layer: IosMetalLayerDescriptor,
    build: Option<IosBuildUiCallback>,
    user_data: *mut c_void,
) -> *mut IosAppHost {
    let raw = craftui_ios_app_create(layer.width_pt, layer.height_pt, layer.contents_scale);
    if let Some(build_fn) = build {
        build_fn(raw, user_data);
    }
    #[cfg(all(target_os = "ios", any(feature = "gpu-sdf", feature = "gpu-vello")))]
    if !layer.ca_metal_layer.is_null() {
        // SAFETY: `raw` was just allocated by `craftui_ios_app_create` and `layer.ca_metal_layer` invariant is upheld by caller.
        let host = unsafe { &mut *raw };
        let _ = unsafe { ios_metal::attach_metal_pipeline(host, layer) };
    }
    raw
}

/// Dispose and free an [`IosAppHost`] created by [`craftui_ios_app_create`].
///
/// # Safety
/// `app` must be null or a valid pointer previously returned by [`craftui_ios_app_create`] and not yet destroyed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn craftui_ios_app_destroy(app: *mut IosAppHost) {
    if !app.is_null() {
        // SAFETY: Caller transfers unique ownership of `app` back for deallocation.
        drop(unsafe { Box::from_raw(app) });
    }
}

/// Notify UIKit view size / screen scale changes.
///
/// # Safety
/// `app` must be a valid non-null [`IosAppHost`] pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn craftui_ios_app_resize(
    app: *mut IosAppHost,
    width_pt: f32,
    height_pt: f32,
    scale_factor: f32,
) {
    if let Some(host) = unsafe { app.as_mut() } {
        host.resize_pt(width_pt, height_pt, scale_factor);
    }
}

/// Update iOS notch / Dynamic Island / Home Indicator safe-area insets in points.
///
/// # Safety
/// `app` must be a valid non-null [`IosAppHost`] pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn craftui_ios_app_set_safe_area(
    app: *mut IosAppHost,
    top: f32,
    right: f32,
    bottom: f32,
    left: f32,
) {
    if let Some(host) = unsafe { app.as_mut() } {
        host.set_safe_area(IosSafeAreaInsets {
            top,
            right,
            bottom,
            left,
        });
    }
}

/// Update software keyboard overlap from `keyboardWillChangeFrameNotification`.
///
/// # Safety
/// `app` must be a valid non-null [`IosAppHost`] pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn craftui_ios_app_set_keyboard(
    app: *mut IosAppHost,
    overlap_height_pt: f32,
    visible: bool,
) {
    if let Some(host) = unsafe { app.as_mut() } {
        host.set_keyboard_frame(IosKeyboardFrame {
            overlap_height_pt,
            visible,
        });
    }
}

/// Dispatch a UIKit `UITouch` phase (`0=Down, 1=Move, 2=Up, 3=Cancel`) in logical points.
///
/// # Safety
/// `app` must be a valid non-null [`IosAppHost`] pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn craftui_ios_app_touch(
    app: *mut IosAppHost,
    touch_id: u64,
    phase: u8,
    x_pt: f32,
    y_pt: f32,
    timestamp_ms: u64,
) -> bool {
    let Some(host) = (unsafe { app.as_mut() }) else {
        return false;
    };
    let p = match phase {
        0 => PointerPhase::Down,
        1 => PointerPhase::Move,
        2 => PointerPhase::Up,
        _ => PointerPhase::Cancel,
    };
    host.push_ui_touch(touch_id, p, x_pt, y_pt, timestamp_ms)
}

/// Commit UTF-8 text from UIKit `UIKeyInput::insertText`.
///
/// # Safety
/// `app` must be valid and if `utf8_len > 0`, `utf8_ptr` must point to `utf8_len` valid UTF-8 bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn craftui_ios_app_insert_text(
    app: *mut IosAppHost,
    utf8_ptr: *const u8,
    utf8_len: usize,
) -> bool {
    let Some(host) = (unsafe { app.as_mut() }) else {
        return false;
    };
    if utf8_ptr.is_null() || utf8_len == 0 {
        return false;
    }
    // SAFETY: Caller guarantees `utf8_ptr..utf8_ptr+utf8_len` is valid for reading during this call.
    let slice = unsafe { std::slice::from_raw_parts(utf8_ptr, utf8_len) };
    let text = String::from_utf8_lossy(slice).into_owned();
    host.push_insert_text(&text)
}

/// Handle `UIKeyInput::deleteBackward`.
///
/// # Safety
/// `app` must be a valid non-null [`IosAppHost`] pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn craftui_ios_app_delete_backward(app: *mut IosAppHost) -> bool {
    let Some(host) = (unsafe { app.as_mut() }) else {
        return false;
    };
    host.push_delete_backward()
}

/// Advance one `CADisplayLink` frame. Returns a bitmask: bit 0 = animating, bit 1 = wants keyboard.
///
/// # Safety
/// `app` must be a valid non-null [`IosAppHost`] pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn craftui_ios_app_tick(app: *mut IosAppHost, dt_sec: f32) -> u8 {
    let Some(host) = (unsafe { app.as_mut() }) else {
        return 0;
    };
    let (anim, cmds) = host.display_link_tick(dt_sec);
    let wants_kb = cmds.iter().any(|c| {
        matches!(
            c,
            IosImeCommand::BecomeFirstResponder | IosImeCommand::SyncText(_)
        )
    });
    (u8::from(anim)) | (u8::from(wants_kb) << 1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use craftui_core::Length;
    use craftui_layout::LayoutStyle;
    use craftui_reactive::reset_runtime;

    struct DummyCvPixelBuffer {
        frame: Option<CvPixelBufferFrame>,
    }

    impl CvPixelBufferBridge for DummyCvPixelBuffer {
        fn acquire_latest_frame(&mut self) -> Option<CvPixelBufferFrame> {
            self.frame.take()
        }
    }

    extern "C" fn build_sample_ios_scene(host: *mut IosAppHost, user_data: *mut c_void) {
        // SAFETY: invoked synchronously inside launch test with valid `host` and `*mut bool` signal handle.
        let host = unsafe { &mut *host };
        let clicked_ptr = user_data as *mut std::sync::atomic::AtomicBool;
        host.mount_root(|cx, tree, _, _| {
            let root = tree.column(cx);
            let txt = tree.text_input(cx, "", "Type here", None);
            let btn = tree.button(cx, "Submit", move || {
                if !clicked_ptr.is_null() {
                    // SAFETY: test atomic bool outlives C-ABI invocation.
                    unsafe { (*clicked_ptr).store(true, std::sync::atomic::Ordering::SeqCst) };
                }
            });
            tree.set_layout(
                txt,
                LayoutStyle::row().with_size(Length::Dp(200.0), Length::Dp(40.0)),
            );
            tree.set_layout(
                btn,
                LayoutStyle::row().with_size(Length::Dp(120.0), Length::Dp(44.0)),
            );
            tree.append_child(root, txt);
            tree.append_child(root, btn);
            root
        });
    }

    #[test]
    fn ios_safe_area_keyboard_and_touch_flow() {
        reset_runtime();
        let mut host = IosAppHost::new(UiDispatcher::new(), 393.0, 852.0, 3.0);
        // Dynamic Island + Home indicator safe area
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
            .button(host.scope, "Confirm", move || clicked.set(true));
        let mut ls = LayoutStyle::row().with_size(Length::Dp(200.0), Length::Dp(48.0));
        ls.margin = Insets::new(80.0, 0.0, 0.0, 20.0);
        host.tree.set_layout(btn, ls);
        host.tree.set_root(btn);
        let _ = host.display_link_tick(0.016);

        let c = host.tree.rect(btn).center();
        host.push_ui_touch(1, PointerPhase::Down, c.x, c.y, 10);
        host.push_ui_touch(1, PointerPhase::Up, c.x, c.y, 35);
        assert!(clicked.get());

        // CVPixelBuffer BGRA -> RGBA external texture sync
        let mut bridge = DummyCvPixelBuffer {
            frame: Some(CvPixelBufferFrame {
                width: 2,
                height: 1,
                bytes_per_row: 8,
                format: CvPixelBufferFormat::Bgra8,
                bytes: vec![10, 20, 200, 255, 0, 180, 0, 255], // BGRx -> RGBx: (200, 20, 10)
            }),
        };
        let mut tex_slot = None;
        let tid = host
            .sync_cv_pixel_buffer(&mut bridge, &mut tex_slot)
            .expect("CVPixelBuffer converted and registered");
        let handle = host.textures.get(tid).unwrap();
        let (_, _, _, rgba) = handle.snapshot();
        assert_eq!(&rgba[0..4], &[200, 20, 10, 255]);
    }

    #[test]
    fn ios_c_abi_lifecycle_entrypoints_work_end_to_end() {
        reset_runtime();
        let clicked = std::sync::atomic::AtomicBool::new(false);
        let layer = IosMetalLayerDescriptor {
            ca_metal_layer: std::ptr::null_mut(),
            width_pt: 390.0,
            height_pt: 844.0,
            contents_scale: 3.0,
        };
        // SAFETY: Passing valid atomic pointer and C builder function within test lifetime.
        let app = unsafe {
            craftui_ios_app_launch(
                layer,
                Some(build_sample_ios_scene),
                (&clicked as *const std::sync::atomic::AtomicBool) as *mut c_void,
            )
        };
        assert!(!app.is_null());
        unsafe {
            craftui_ios_app_set_safe_area(app, 47.0, 0.0, 34.0, 0.0);
            craftui_ios_app_set_keyboard(app, 0.0, false);
            craftui_ios_app_resize(app, 400.0, 860.0, 3.0);
            let _ = craftui_ios_app_tick(app, 0.016);
            // Tap first TextInput (centered around y=20)
            let _ = craftui_ios_app_touch(app, 1, 0, 50.0, 20.0, 10);
            assert!(craftui_ios_app_touch(app, 1, 2, 50.0, 20.0, 30));
            let flags = craftui_ios_app_tick(app, 0.016);
            assert_ne!(flags & 0b10, 0, "expected keyboard requested");
            // Subsequent idle ticks neither force Metal redraw nor re-emit duplicate SyncText
            let host_ref = &mut *app;
            let now_ms = craftui_core::monotonic_ms();
            host_ref.tree.reset_caret_blink(now_ms);
            let (anim_idle, painted_idle, cmds_idle) =
                host_ref.display_link_tick_with_redraw(0.016, now_ms + 50);
            assert!(!anim_idle && !painted_idle && cmds_idle.is_empty());
            let msg = b"hello";
            assert!(craftui_ios_app_insert_text(app, msg.as_ptr(), msg.len()));
            // Tap Button below TextInput (y around 60)
            assert!(craftui_ios_app_touch(app, 2, 0, 40.0, 60.0, 50));
            assert!(craftui_ios_app_touch(app, 2, 2, 40.0, 60.0, 70));
            assert!(clicked.load(std::sync::atomic::Ordering::SeqCst));
            craftui_ios_app_destroy(app);
        }
    }
}
