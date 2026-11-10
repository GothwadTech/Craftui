//! Android platform adapter (`android-activity` `GameActivity` + Vulkan/GLES `wgpu` + JNI/IME/Surface bridges).

use craftui_core::{
    ImeEvent, Insets, KeyCode, KeyEvent, Modifiers, PlatformViewId, Point, PointerButton,
    PointerEvent, PointerId, PointerPhase, Size, TextureId, UiDispatcher, UiEvent, WindowMetrics,
};
use craftui_reactive::Scope;
use craftui_render::{PaintScene, TextureHandle, TextureRegistry};
use craftui_widgets::{Router, WidgetKind, WidgetTree};

/// Embedding strategy for native Android Views (e.g., CameraX / ExoPlayer / WebView)
/// hosted inside CraftUI [`WidgetKind::PlatformView`](craftui_widgets::WidgetKind::PlatformView).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AndroidPlatformViewMode {
    /// Zero-copy Vulkan/GLES `HardwareBuffer` / `SurfaceTexture` external texture composited directly inside Vello.
    ExternalSurfaceTexture { surface_id: u64 },
    /// Transparent `PaintCommand::PlatformViewHole` punch composited over an Android `SurfaceView`.
    HolePunch { view_id: PlatformViewId },
}

/// Bridge trait for importing Android `AHardwareBuffer` / `SurfaceTexture` RGBA frames into CraftUI textures.
pub trait ExternalSurfaceTextureBridge: Send + Sync {
    fn surface_id(&self) -> u64;
    /// Copy or acquire the latest frame dimensions and straight RGBA8 bytes when dirty.
    fn acquire_latest_rgba(&mut self) -> Option<(u32, u32, Vec<u8>)>;
}

/// Platform-neutral Android keycodes translated by [`AndroidHost::handle_android_key_event`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AndroidKeyCode {
    Back,
    Enter,
    NumpadEnter,
    Tab,
    Del,
    ForwardDel,
    Escape,
    DpadLeft,
    DpadRight,
    DpadUp,
    DpadDown,
    Home,
    MoveEnd,
    PageUp,
    PageDown,
    Space,
    Character(char),
    Other,
}

/// Commands emitted toward Android `InputMethodManager` via `GameActivity`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AndroidImeCommand {
    ShowSoftInput,
    HideSoftInput,
    SyncTextState {
        text: String,
        selection_start: usize,
        selection_end: usize,
    },
}

/// Platform-neutral Android lifecycle, safe-area/IME inset, back navigation, and event translator host.
///
/// Used both on-device inside [`android_runtime::run_android_app`] and in deterministic desktop/CI integration tests.
pub struct AndroidHost {
    pub scope: Scope,
    pub tree: WidgetTree,
    pub scene: PaintScene,
    pub textures: TextureRegistry,
    pub dispatcher: UiDispatcher,
    back_handler: Option<Box<dyn FnMut() -> bool>>,
    last_ime_visible: bool,
    last_synced_ime_state: Option<(String, usize, usize)>,
    needs_redraw: bool,
}

impl AndroidHost {
    pub fn new(dispatcher: UiDispatcher, width_px: u32, height_px: u32, scale_factor: f32) -> Self {
        let scope = Scope::root();
        let mut tree = WidgetTree::new(scope);
        let sf = scale_factor.max(1.0);
        tree.metrics = WindowMetrics {
            logical_size: Size::new(width_px as f32 / sf, height_px as f32 / sf),
            scale_factor: sf,
            safe_area: Insets::ZERO,
            keyboard_inset: 0.0,
        };
        Self {
            scope,
            tree,
            scene: PaintScene::new(),
            textures: TextureRegistry::new(),
            dispatcher,
            back_handler: None,
            last_ime_visible: false,
            last_synced_ime_state: None,
            needs_redraw: true,
        }
    }

    pub fn request_redraw(&mut self) {
        self.needs_redraw = true;
    }

    /// Suggested poll timeout for demand-driven Android event loops.
    pub fn next_poll_timeout(&self, animating: bool, has_gpu: bool) -> Option<std::time::Duration> {
        if animating || self.needs_redraw {
            return Some(if has_gpu {
                std::time::Duration::ZERO
            } else {
                std::time::Duration::from_millis(16)
            });
        }
        let now_ms = craftui_core::monotonic_ms();
        let mut min_delay = craftui_reactive::next_timer_delay_ms(now_ms);
        if let Some(caret_delay) = self.tree.next_internal_delay_ms(now_ms) {
            min_delay = Some(min_delay.map_or(caret_delay, |cur| cur.min(caret_delay)));
        }
        min_delay.map(|ms| std::time::Duration::from_millis(ms.max(1)))
    }

    /// Register a hardware Back button handler (e.g. `move || router.pop()`).
    pub fn set_back_handler(&mut self, handler: impl FnMut() -> bool + 'static) {
        self.back_handler = Some(Box::new(handler));
    }

    /// Convenience helper binding a [`Router<R>`] to Android's hardware/gesture Back key.
    pub fn bind_back_router<R: Clone + PartialEq + 'static>(&mut self, router: Router<R>) {
        self.set_back_handler(move || router.pop());
    }

    pub fn update_safe_area_and_ime(
        &mut self,
        window_width_px: i32,
        window_height_px: i32,
        content_left_px: i32,
        content_top_px: i32,
        content_right_px: i32,
        content_bottom_px: i32,
    ) {
        let sf = self.tree.metrics.scale_factor.max(1.0);
        let top = (content_top_px.max(0) as f32) / sf;
        let left = (content_left_px.max(0) as f32) / sf;
        let right = ((window_width_px - content_right_px).max(0) as f32) / sf;
        let bottom_px = (window_height_px - content_bottom_px).max(0) as f32;
        let bottom = bottom_px / sf;
        self.tree.metrics.safe_area = Insets::new(top, right, 0.0, left);
        self.tree.metrics.keyboard_inset = bottom;
        self.needs_redraw = true;
    }

    pub fn push_touch(
        &mut self,
        pointer_id: i32,
        phase: PointerPhase,
        raw_x_px: f32,
        raw_y_px: f32,
        timestamp_ms: u64,
    ) -> bool {
        let sf = self.tree.metrics.scale_factor.max(1.0);
        let pos = Point::new(raw_x_px / sf, raw_y_px / sf);
        let handled = self.tree.handle_event(&UiEvent::Pointer(PointerEvent {
            id: PointerId(pointer_id as u64),
            phase,
            position: pos,
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

    /// Translate and dispatch Android key events, including `Back`, `Enter`, directional D-pad keys, and text input keys.
    pub fn handle_android_key_event(
        &mut self,
        code: AndroidKeyCode,
        pressed: bool,
        repeat: bool,
        modifiers: Modifiers,
    ) -> bool {
        if code == AndroidKeyCode::Back {
            if !pressed {
                return false;
            }
            // 1. Dismiss open overlay / modal / popup menu if active.
            let esc = KeyEvent {
                code: KeyCode::Escape,
                text: None,
                pressed: true,
                repeat: false,
                modifiers,
            };
            if self.tree.handle_event(&UiEvent::Key(esc)) {
                self.needs_redraw = true;
                return true;
            }
            // 2. Pop custom registered Back handler / Router.
            if let Some(ref mut cb) = self.back_handler
                && cb()
            {
                self.needs_redraw = true;
                return true;
            }
            // 3. Fallback to Router<String> in root context if present.
            if let Some(router) = self.scope.use_context::<Router<String>>()
                && router.pop()
            {
                self.needs_redraw = true;
                return true;
            }
            return false;
        }

        let (craft_code, text) = match code {
            AndroidKeyCode::Enter | AndroidKeyCode::NumpadEnter => {
                (KeyCode::Enter, Some("\n".to_string()))
            }
            AndroidKeyCode::Tab => (KeyCode::Tab, None),
            AndroidKeyCode::Del => (KeyCode::Backspace, None),
            AndroidKeyCode::ForwardDel => (KeyCode::Delete, None),
            AndroidKeyCode::Escape => (KeyCode::Escape, None),
            AndroidKeyCode::DpadLeft => (KeyCode::ArrowLeft, None),
            AndroidKeyCode::DpadRight => (KeyCode::ArrowRight, None),
            AndroidKeyCode::DpadUp => (KeyCode::ArrowUp, None),
            AndroidKeyCode::DpadDown => (KeyCode::ArrowDown, None),
            AndroidKeyCode::Home => (KeyCode::Home, None),
            AndroidKeyCode::MoveEnd => (KeyCode::End, None),
            AndroidKeyCode::PageUp => (KeyCode::PageUp, None),
            AndroidKeyCode::PageDown => (KeyCode::PageDown, None),
            AndroidKeyCode::Space => (KeyCode::Space, Some(" ".to_string())),
            AndroidKeyCode::Character(ch) => {
                let s = ch.to_string();
                (KeyCode::Character(s.clone()), Some(s))
            }
            AndroidKeyCode::Back | AndroidKeyCode::Other => (KeyCode::Unknown, None),
        };

        let handled = self.tree.handle_event(&UiEvent::Key(KeyEvent {
            code: craft_code,
            text,
            pressed,
            repeat,
            modifiers,
        }));
        if handled {
            self.needs_redraw = true;
        }
        handled
    }

    pub fn push_ime_commit(&mut self, text: impl Into<String>) -> bool {
        let handled = self
            .tree
            .handle_event(&UiEvent::Ime(ImeEvent::Commit(text.into())));
        if handled {
            self.needs_redraw = true;
        }
        handled
    }

    /// Update or allocate a CraftUI [`TextureHandle`] from an external Android [`ExternalSurfaceTextureBridge`].
    pub fn sync_external_texture(
        &mut self,
        bridge: &mut dyn ExternalSurfaceTextureBridge,
        slot: &mut Option<TextureHandle>,
    ) -> Option<TextureId> {
        let (w, h, rgba) = bridge.acquire_latest_rgba()?;
        if w == 0 || h == 0 || rgba.len() != (w as usize) * (h as usize) * 4 {
            return None;
        }
        self.needs_redraw = true;
        if let Some(existing) = slot.as_ref() {
            existing.buffer.replace_rgba(w, h, rgba);
            Some(existing.id)
        } else {
            let handle = self.textures.create_from_rgba(w, h, rgba);
            let id = handle.id;
            *slot = Some(handle);
            Some(id)
        }
    }

    /// Advance reactive timers, animations, layout, and paint when dirty or animating.
    /// Returns `(animating, did_paint, cmds)`.
    pub fn step_frame_with_redraw(&mut self, dt_sec: f32) -> (bool, bool, Vec<AndroidImeCommand>) {
        let now_ms = craftui_core::monotonic_ms();
        let timers = craftui_reactive::tick_timers();
        let jobs = self.dispatcher.drain();
        let blink = self.tree.tick_blink_timers(now_ms);
        let animating = self.tree.tick_animations(dt_sec);
        let dirty = self.tree.layout(self.tree.metrics);
        let should_paint =
            self.needs_redraw || timers > 0 || jobs > 0 || blink || animating || dirty > 0;
        if should_paint {
            self.tree.paint(&mut self.scene);
            self.needs_redraw = false;
        }

        let mut cmds = Vec::new();
        let focused_input = self.tree.focused.and_then(|id| {
            self.tree.nodes.get(id).and_then(|n| match &n.kind {
                WidgetKind::TextInput { editor, .. } => {
                    let (s, e) = editor
                        .selection_range()
                        .unwrap_or((editor.cursor, editor.cursor));
                    Some((editor.text.clone(), s, e))
                }
                _ => None,
            })
        });

        let ime_needed = focused_input.is_some();
        if ime_needed != self.last_ime_visible {
            self.last_ime_visible = ime_needed;
            if ime_needed {
                cmds.push(AndroidImeCommand::ShowSoftInput);
            } else {
                self.last_synced_ime_state = None;
                cmds.push(AndroidImeCommand::HideSoftInput);
            }
        }
        if let Some((text, selection_start, selection_end)) = focused_input {
            let next_state = (text.clone(), selection_start, selection_end);
            if self.last_synced_ime_state.as_ref() != Some(&next_state) {
                self.last_synced_ime_state = Some(next_state);
                cmds.push(AndroidImeCommand::SyncTextState {
                    text,
                    selection_start,
                    selection_end,
                });
            }
        }

        (animating, should_paint, cmds)
    }

    pub fn step_frame(&mut self, dt_sec: f32) -> (bool, Vec<AndroidImeCommand>) {
        let (animating, _painted, cmds) = self.step_frame_with_redraw(dt_sec);
        (animating, cmds)
    }
}

#[cfg(target_os = "android")]
pub mod android_runtime {
    use super::*;
    use android_activity::input::{
        InputEvent, KeyAction, Keycode, MotionAction, TextInputState, TextSpan,
    };
    use android_activity::{AndroidApp, InputStatus, MainEvent, PollEvent};
    use craftui_core::NodeId;
    use craftui_render::GpuSurfacePipeline;
    use raw_window_handle::{
        AndroidDisplayHandle, AndroidNdkWindowHandle, RawDisplayHandle, RawWindowHandle,
    };
    use std::ptr::NonNull;
    use std::time::{Duration, Instant};

    struct AndroidSurfaceTarget {
        window: RawWindowHandle,
        display: RawDisplayHandle,
    }

    impl AndroidSurfaceTarget {
        fn new(ptr: NonNull<std::ffi::c_void>) -> Self {
            Self {
                window: RawWindowHandle::AndroidNdk(AndroidNdkWindowHandle::new(ptr)),
                display: RawDisplayHandle::Android(AndroidDisplayHandle::new()),
            }
        }
    }

    fn map_activity_keycode(
        kc: Keycode,
        meta: android_activity::input::MetaState,
    ) -> AndroidKeyCode {
        match kc {
            Keycode::Back => AndroidKeyCode::Back,
            Keycode::Enter => AndroidKeyCode::Enter,
            Keycode::NumpadEnter => AndroidKeyCode::NumpadEnter,
            Keycode::Tab => AndroidKeyCode::Tab,
            Keycode::Del => AndroidKeyCode::Del,
            Keycode::ForwardDel => AndroidKeyCode::ForwardDel,
            Keycode::Escape => AndroidKeyCode::Escape,
            Keycode::DpadLeft => AndroidKeyCode::DpadLeft,
            Keycode::DpadRight => AndroidKeyCode::DpadRight,
            Keycode::DpadUp => AndroidKeyCode::DpadUp,
            Keycode::DpadDown => AndroidKeyCode::DpadDown,
            Keycode::MoveHome | Keycode::Home => AndroidKeyCode::Home,
            Keycode::MoveEnd => AndroidKeyCode::MoveEnd,
            Keycode::PageUp => AndroidKeyCode::PageUp,
            Keycode::PageDown => AndroidKeyCode::PageDown,
            Keycode::Space => AndroidKeyCode::Space,
            Keycode::A => AndroidKeyCode::Character(if meta.shift_on() { 'A' } else { 'a' }),
            Keycode::B => AndroidKeyCode::Character(if meta.shift_on() { 'B' } else { 'b' }),
            Keycode::C => AndroidKeyCode::Character(if meta.shift_on() { 'C' } else { 'c' }),
            Keycode::D => AndroidKeyCode::Character(if meta.shift_on() { 'D' } else { 'd' }),
            Keycode::E => AndroidKeyCode::Character(if meta.shift_on() { 'E' } else { 'e' }),
            Keycode::F => AndroidKeyCode::Character(if meta.shift_on() { 'F' } else { 'f' }),
            Keycode::G => AndroidKeyCode::Character(if meta.shift_on() { 'G' } else { 'g' }),
            Keycode::H => AndroidKeyCode::Character(if meta.shift_on() { 'H' } else { 'h' }),
            Keycode::I => AndroidKeyCode::Character(if meta.shift_on() { 'I' } else { 'i' }),
            Keycode::J => AndroidKeyCode::Character(if meta.shift_on() { 'J' } else { 'j' }),
            Keycode::K => AndroidKeyCode::Character(if meta.shift_on() { 'K' } else { 'k' }),
            Keycode::L => AndroidKeyCode::Character(if meta.shift_on() { 'L' } else { 'l' }),
            Keycode::M => AndroidKeyCode::Character(if meta.shift_on() { 'M' } else { 'm' }),
            Keycode::N => AndroidKeyCode::Character(if meta.shift_on() { 'N' } else { 'n' }),
            Keycode::O => AndroidKeyCode::Character(if meta.shift_on() { 'O' } else { 'o' }),
            Keycode::P => AndroidKeyCode::Character(if meta.shift_on() { 'P' } else { 'p' }),
            Keycode::Q => AndroidKeyCode::Character(if meta.shift_on() { 'Q' } else { 'q' }),
            Keycode::R => AndroidKeyCode::Character(if meta.shift_on() { 'R' } else { 'r' }),
            Keycode::S => AndroidKeyCode::Character(if meta.shift_on() { 'S' } else { 's' }),
            Keycode::T => AndroidKeyCode::Character(if meta.shift_on() { 'T' } else { 't' }),
            Keycode::U => AndroidKeyCode::Character(if meta.shift_on() { 'U' } else { 'u' }),
            Keycode::V => AndroidKeyCode::Character(if meta.shift_on() { 'V' } else { 'v' }),
            Keycode::W => AndroidKeyCode::Character(if meta.shift_on() { 'W' } else { 'w' }),
            Keycode::X => AndroidKeyCode::Character(if meta.shift_on() { 'X' } else { 'x' }),
            Keycode::Y => AndroidKeyCode::Character(if meta.shift_on() { 'Y' } else { 'y' }),
            Keycode::Z => AndroidKeyCode::Character(if meta.shift_on() { 'Z' } else { 'z' }),
            Keycode::Keycode0 => AndroidKeyCode::Character('0'),
            Keycode::Keycode1 => AndroidKeyCode::Character('1'),
            Keycode::Keycode2 => AndroidKeyCode::Character('2'),
            Keycode::Keycode3 => AndroidKeyCode::Character('3'),
            Keycode::Keycode4 => AndroidKeyCode::Character('4'),
            Keycode::Keycode5 => AndroidKeyCode::Character('5'),
            Keycode::Keycode6 => AndroidKeyCode::Character('6'),
            Keycode::Keycode7 => AndroidKeyCode::Character('7'),
            Keycode::Keycode8 => AndroidKeyCode::Character('8'),
            Keycode::Keycode9 => AndroidKeyCode::Character('9'),
            _ => AndroidKeyCode::Other,
        }
    }

    pub fn run_android_app<F>(app: AndroidApp, mut build_ui: F)
    where
        F: FnMut(Scope, &mut WidgetTree, &mut TextureRegistry, &UiDispatcher) -> NodeId,
    {
        let waker = app.create_waker();
        let dispatcher = UiDispatcher::with_waker(move || {
            waker.wake();
        });

        let density = app
            .config()
            .density()
            .map(|d| (d as f32 / 160.0).max(1.0))
            .unwrap_or(2.0);

        let mut host = AndroidHost::new(dispatcher.clone(), 1080, 1920, density);
        let root = build_ui(host.scope, &mut host.tree, &mut host.textures, &dispatcher);
        host.tree.set_root(root);

        let mut gpu: Option<GpuSurfacePipeline> = None;
        let mut last_tick = Instant::now();
        let start = Instant::now();
        let mut running = true;
        let mut animating = false;

        while running {
            let timeout = host.next_poll_timeout(animating, gpu.is_some());
            app.poll_events(timeout, |event| match event {
                PollEvent::Wake | PollEvent::Timeout => {
                    host.request_redraw();
                }
                PollEvent::Main(main_ev) => match main_ev {
                    MainEvent::InitWindow { .. } => {
                        if let Some(nw) = app.native_window() {
                            let w = nw.width().max(1) as u32;
                            let h = nw.height().max(1) as u32;
                            host.tree.handle_event(&UiEvent::Resize {
                                width: w as f32 / density,
                                height: h as f32 / density,
                                scale_factor: density,
                            });
                            host.request_redraw();
                            if let Some(nn) = NonNull::new(nw.ptr().as_ptr().cast()) {
                                let tgt = AndroidSurfaceTarget::new(nn);
                                let raw_target = wgpu::SurfaceTargetUnsafe::RawHandle {
                                    raw_display_handle: Some(tgt.display),
                                    raw_window_handle: tgt.window,
                                };
                                // SAFETY: `nw` is a live ANativeWindow until `TerminateWindow` drops `gpu`.
                                gpu = unsafe {
                                    GpuSurfacePipeline::new_unsafe(raw_target, w, h, false)
                                }
                                .ok();
                            }
                        }
                    }
                    MainEvent::TerminateWindow { .. } => {
                        gpu = None;
                    }
                    MainEvent::WindowResized { .. } | MainEvent::ContentRectChanged { .. } => {
                        if let Some(nw) = app.native_window() {
                            let w = nw.width().max(1) as u32;
                            let h = nw.height().max(1) as u32;
                            if let Some(g) = gpu.as_mut() {
                                g.resize(w, h);
                            }
                            let cr = app.content_rect();
                            host.update_safe_area_and_ime(
                                w as i32, h as i32, cr.left, cr.top, cr.right, cr.bottom,
                            );
                            host.tree.handle_event(&UiEvent::Resize {
                                width: w as f32 / density,
                                height: h as f32 / density,
                                scale_factor: density,
                            });
                            host.request_redraw();
                        }
                    }
                    MainEvent::InputAvailable => {
                        if let Ok(mut iter) = app.input_events_iter() {
                            while iter.next(|ev| match ev {
                                InputEvent::MotionEvent(me) => {
                                    let action = me.action();
                                    let ts = start.elapsed().as_millis() as u64;
                                    let ptr_idx = me.pointer_index();
                                    for p in me.pointers() {
                                        let phase = match action {
                                            MotionAction::Down => PointerPhase::Down,
                                            MotionAction::PointerDown
                                                if p.pointer_index() == ptr_idx =>
                                            {
                                                PointerPhase::Down
                                            }
                                            MotionAction::Up => PointerPhase::Up,
                                            MotionAction::PointerUp
                                                if p.pointer_index() == ptr_idx =>
                                            {
                                                PointerPhase::Up
                                            }
                                            MotionAction::Cancel => PointerPhase::Cancel,
                                            _ => PointerPhase::Move,
                                        };
                                        host.push_touch(p.pointer_id(), phase, p.x(), p.y(), ts);
                                    }
                                    InputStatus::Handled
                                }
                                InputEvent::KeyEvent(ke) => {
                                    let pressed = ke.action() == KeyAction::Down;
                                    let repeat = ke.repeat_count() > 0;
                                    let meta = ke.meta_state();
                                    let mods = Modifiers {
                                        shift: meta.shift_on(),
                                        ctrl: meta.ctrl_on(),
                                        alt: meta.alt_on(),
                                        super_key: meta.meta_on(),
                                    };
                                    let code = map_activity_keycode(ke.key_code(), meta);
                                    if host.handle_android_key_event(code, pressed, repeat, mods) {
                                        InputStatus::Handled
                                    } else if code == AndroidKeyCode::Back {
                                        InputStatus::Unhandled
                                    } else {
                                        InputStatus::Handled
                                    }
                                }
                                InputEvent::TextEvent(ts) => {
                                    let state: TextInputState = ts.clone();
                                    if host.tree.handle_event(&UiEvent::Ime(ImeEvent::Commit(state.text)))
                                    {
                                        host.request_redraw();
                                    }
                                    InputStatus::Handled
                                }
                                _ => InputStatus::Unhandled,
                            }) {}
                        }
                    }
                    MainEvent::Destroy => {
                        running = false;
                    }
                    _ => {}
                },
                _ => {}
            });

            if !running {
                break;
            }
            let now = Instant::now();
            let dt = (now - last_tick).as_secs_f32().clamp(0.001, 0.05);
            last_tick = now;

            let (still_anim, did_paint, cmds) = host.step_frame_with_redraw(dt);
            animating = still_anim;
            for cmd in cmds {
                match cmd {
                    AndroidImeCommand::ShowSoftInput => app.show_soft_input(true),
                    AndroidImeCommand::HideSoftInput => app.hide_soft_input(false),
                    AndroidImeCommand::SyncTextState {
                        text,
                        selection_start,
                        selection_end,
                    } => {
                        app.set_text_input_state(TextInputState {
                            text,
                            selection: TextSpan {
                                start: selection_start,
                                end: selection_end,
                            },
                            compose_region: None,
                        });
                    }
                }
            }
            if did_paint && let Some(g) = gpu.as_mut() {
                let bg = host.tree.theme().background;
                let _ = g.render_frame(
                    &host.scene,
                    density,
                    bg,
                    &host.textures,
                    &mut host.tree.text_engine,
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use craftui_core::Length;
    use craftui_layout::LayoutStyle;
    use craftui_reactive::reset_runtime;
    use std::cell::RefCell;
    use std::rc::Rc;

    struct DummyExternalSurface {
        dirty: bool,
    }

    impl ExternalSurfaceTextureBridge for DummyExternalSurface {
        fn surface_id(&self) -> u64 {
            42
        }

        fn acquire_latest_rgba(&mut self) -> Option<(u32, u32, Vec<u8>)> {
            if !self.dirty {
                return None;
            }
            self.dirty = false;
            Some((
                2,
                2,
                vec![
                    255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 255, 255,
                ],
            ))
        }
    }

    #[test]
    fn android_host_safe_area_keyboard_insets_and_ime_commands() {
        reset_runtime();
        let disp = UiDispatcher::new();
        let mut host = AndroidHost::new(disp, 1080, 2400, 3.0);
        // Status bar = 90px top, IME keyboard occupies bottom 750px
        host.update_safe_area_and_ime(1080, 2400, 0, 90, 1080, 1650);
        let eff = host.tree.metrics.effective_safe_area();
        assert!((eff.top - 30.0).abs() < 0.01);
        assert!((eff.bottom - 250.0).abs() < 0.01);

        let inp = host.tree.text_input(host.scope, "", "Search", None);
        let mut ls = LayoutStyle::row().with_size(Length::Dp(300.0), Length::Dp(44.0));
        ls.margin = Insets::ZERO;
        host.tree.set_layout(inp, ls);
        host.tree.set_root(inp);
        let _ = host.step_frame(0.016);

        // Touch down+up focuses the TextInput
        host.push_touch(0, PointerPhase::Down, 150.0, 60.0, 0);
        host.push_touch(0, PointerPhase::Up, 150.0, 60.0, 20);
        let (_, cmds) = host.step_frame(0.016);
        assert!(cmds.contains(&AndroidImeCommand::ShowSoftInput));

        host.push_ime_commit("invoice");
        let (_, cmds2) = host.step_frame(0.016);
        assert!(cmds2.iter().any(|c| matches!(
            c,
            AndroidImeCommand::SyncTextState { text, selection_start: 7, selection_end: 7 } if text == "invoice"
        )));
    }

    #[test]
    fn android_back_router_pop_enter_dpad_and_external_texture_work() {
        reset_runtime();
        let mut host = AndroidHost::new(UiDispatcher::new(), 1080, 1920, 2.0);
        let router = Router::new(host.scope, "home".to_string());
        host.bind_back_router(router);
        router.push("detail".to_string());

        let modal_open = host.scope.signal(true);
        let root = host.tree.column(host.scope);
        let tab_idx = host.scope.signal(0usize);
        let tabs = host
            .tree
            .tabs(host.scope, vec!["One".into(), "Two".into()], tab_idx);
        let pv_id = PlatformViewId::default();
        let pv = host.tree.platform_view(
            host.scope,
            pv_id,
            LayoutStyle::row().with_size(Length::Dp(120.0), Length::Dp(80.0)),
        );
        let modal = host.tree.overlay(
            host.scope,
            modal_open,
            false,
            Some(Rc::new(RefCell::new(move || modal_open.set(false)))),
        );
        host.tree.append_child(root, tabs);
        host.tree.append_child(root, pv);
        host.tree.append_child(root, modal);
        host.tree.set_root(root);
        let _ = host.step_frame(0.016);

        // Back key first dismisses active modal/overlay
        assert!(host.handle_android_key_event(
            AndroidKeyCode::Back,
            true,
            false,
            Modifiers::default()
        ));
        assert!(!modal_open.get());
        assert_eq!(router.current(), "detail");

        // Second Back key pops the Router stack
        assert!(host.handle_android_key_event(
            AndroidKeyCode::Back,
            true,
            false,
            Modifiers::default()
        ));
        assert_eq!(router.current(), "home");

        // Third Back key at root returns false (unhandled so Android finishes Activity)
        assert!(!host.handle_android_key_event(
            AndroidKeyCode::Back,
            true,
            false,
            Modifiers::default()
        ));

        // Tab -> focus Tabs, DpadRight switches active tab
        host.tree.focused = Some(tabs);
        assert!(host.handle_android_key_event(
            AndroidKeyCode::DpadRight,
            true,
            false,
            Modifiers::default()
        ));
        assert_eq!(tab_idx.get(), 1);

        // Demand-driven idle check: once painted and non-animating, poll timeout blocks (`None`)
        let (anim_idle, painted_now, _) = host.step_frame_with_redraw(0.016);
        assert!(!anim_idle && painted_now);
        assert_eq!(host.next_poll_timeout(false, true), None);

        // External SurfaceTexture upload + PlatformViewHole emitted in scene
        let mut bridge = DummyExternalSurface { dirty: true };
        let mut tex_slot = None;
        let tid = host
            .sync_external_texture(&mut bridge, &mut tex_slot)
            .expect("external texture uploaded");
        assert!(host.textures.get(tid).is_some());
        assert!(host.scene.commands.iter().any(|c| matches!(
            c,
            craftui_render::PaintCommand::PlatformViewHole { id, .. } if *id == pv_id
        )));
    }
}
