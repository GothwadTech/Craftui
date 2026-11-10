//! Android platform adapter (`android-activity` `GameActivity` + Vulkan/GLES `wgpu` + JNI/Surface bridges).

use craftui_core::{
    ImeEvent, Insets, Modifiers, PlatformViewId, Point, PointerButton, PointerEvent, PointerId,
    PointerPhase, Size, UiDispatcher, UiEvent, WindowMetrics,
};
use craftui_reactive::Scope;
use craftui_render::{PaintScene, TextureRegistry};
use craftui_widgets::{WidgetKind, WidgetTree};

/// Embedding strategy for native Android Views (e.g., CameraX / ExoPlayer / WebView)
/// hosted inside CraftUI [`WidgetKind::PlatformView`](craftui_widgets::WidgetKind::PlatformView).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AndroidPlatformViewMode {
    /// Zero-overlay Vulkan/GLES `HardwareBuffer` / `SurfaceTexture` external texture composited directly inside Vello.
    ExternalSurfaceTexture { surface_id: u64 },
    /// Transparent `PaintCommand::PlatformViewHole` punch composited over an Android `SurfaceView`.
    HolePunch { view_id: PlatformViewId },
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

/// Platform-neutral Android lifecycle, safe-area/IME inset, and event translator host.
///
/// Used both on-device inside [`run_android_app`] and in deterministic desktop/CI integration tests.
pub struct AndroidHost {
    pub scope: Scope,
    pub tree: WidgetTree,
    pub scene: PaintScene,
    pub textures: TextureRegistry,
    pub dispatcher: UiDispatcher,
    last_ime_visible: bool,
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
            last_ime_visible: false,
        }
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
        self.tree.handle_event(&UiEvent::Pointer(PointerEvent {
            id: PointerId(pointer_id as u64),
            phase,
            position: pos,
            button: Some(PointerButton::Primary),
            modifiers: Modifiers::default(),
            is_touch: true,
            timestamp_ms,
        }))
    }

    pub fn push_ime_commit(&mut self, text: impl Into<String>) -> bool {
        self.tree
            .handle_event(&UiEvent::Ime(ImeEvent::Commit(text.into())))
    }

    pub fn step_frame(&mut self, dt_sec: f32) -> (bool, Vec<AndroidImeCommand>) {
        let _ = self.dispatcher.drain();
        let animating = self.tree.tick_animations(dt_sec);
        let _dirty = self.tree.layout(self.tree.metrics);
        self.tree.paint(&mut self.scene);

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
            cmds.push(if ime_needed {
                AndroidImeCommand::ShowSoftInput
            } else {
                AndroidImeCommand::HideSoftInput
            });
        }
        if let Some((text, selection_start, selection_end)) = focused_input {
            cmds.push(AndroidImeCommand::SyncTextState {
                text,
                selection_start,
                selection_end,
            });
        }

        (animating, cmds)
    }
}

#[cfg(target_os = "android")]
pub mod android_runtime {
    use super::*;
    use android_activity::input::{InputEvent, KeyAction, MotionAction, TextSpan};
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

        while running {
            let timeout = Some(Duration::from_millis(16));
            app.poll_events(timeout, |event| match event {
                PollEvent::Wake => {}
                PollEvent::Timeout => {}
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
                                    if pressed
                                        && ke.key_code() == android_activity::input::Keycode::Del
                                    {
                                        host.tree.handle_event(&UiEvent::Key(
                                            craftui_core::KeyEvent {
                                                code: craftui_core::KeyCode::Backspace,
                                                text: None,
                                                pressed: true,
                                                repeat: false,
                                                modifiers: Modifiers::default(),
                                            },
                                        ));
                                    }
                                    InputStatus::Handled
                                }
                                InputEvent::TextEvent(ts) => {
                                    let state: android_activity::input::TextInputState = ts.clone();
                                    let _ = TextSpan {
                                        start: state.selection.start,
                                        end: state.selection.end,
                                    };
                                    host.tree
                                        .handle_event(&UiEvent::Ime(ImeEvent::Commit(state.text)));
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

            let (_anim, cmds) = host.step_frame(dt);
            for cmd in cmds {
                match cmd {
                    AndroidImeCommand::ShowSoftInput => app.show_soft_input(true),
                    AndroidImeCommand::HideSoftInput => app.hide_soft_input(false),
                    AndroidImeCommand::SyncTextState { .. } => {}
                }
            }
            if let Some(g) = gpu.as_mut() {
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
            AndroidImeCommand::SyncTextState { text, .. } if text == "invoice"
        )));
    }
}
