//! Desktop platform runner (`winit` + `wgpu`/`vello` + `accesskit_winit` + `notify` hot-reload)
//! supporting Linux (Wayland/X11), macOS, and Windows.

use accesskit_winit::Adapter as AccessKitAdapter;
use craftui_a11y::accesskit;
use craftui_core::{
    CraftError, ImeEvent, KeyCode, KeyEvent, Modifiers, NodeId, Point, PointerButton, PointerEvent,
    PointerId, PointerPhase, Result, ScrollDelta, Size, UiDispatcher, UiEvent, WindowMetrics,
};
use craftui_reactive::Scope;
use craftui_render::{GpuSurfacePipeline, PaintScene, TextureRegistry};
use craftui_runtime::HotReloadSession;
use craftui_widgets::WidgetTree;
use notify::{Event as NotifyEvent, RecursiveMode, Watcher};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::{
    ElementState, Ime as WinitIme, MouseButton, MouseScrollDelta, TouchPhase, WindowEvent,
};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::keyboard::{Key, KeyCode as WinitKeyCode, ModifiersState, NamedKey, PhysicalKey};
use winit::window::{Window, WindowAttributes, WindowId};

/// Desktop window configuration.
#[derive(Debug, Clone)]
pub struct WindowConfig {
    pub title: String,
    pub width: f32,
    pub height: f32,
    pub force_cpu: bool,
    pub resizable: bool,
}

impl Default for WindowConfig {
    fn default() -> Self {
        Self {
            title: "CraftUI Application".to_string(),
            width: 1024.0,
            height: 768.0,
            force_cpu: false,
            resizable: true,
        }
    }
}

/// Custom event loop messages delivered from background worker threads, AccessKit, or `.cui` watcher.
#[derive(Debug)]
pub enum DesktopUserEvent {
    WakeUi,
    AccessKit(accesskit_winit::Event),
    FileChanged,
}

impl From<accesskit_winit::Event> for DesktopUserEvent {
    fn from(ev: accesskit_winit::Event) -> Self {
        Self::AccessKit(ev)
    }
}

/// Platform services provided during app setup.
pub struct AppContext {
    pub dispatcher: UiDispatcher,
    pub textures: TextureRegistry,
}

type BuildUiFn = Box<dyn FnOnce(Scope, &mut WidgetTree, &mut AppContext) -> NodeId>;

/// Run a CraftUI application in a desktop `winit` window.
pub fn run_app<F>(config: WindowConfig, build_ui: F) -> Result<()>
where
    F: FnOnce(Scope, &mut WidgetTree, &mut AppContext) -> NodeId + 'static,
{
    let event_loop = EventLoop::<DesktopUserEvent>::with_user_event()
        .build()
        .map_err(|e| CraftError::Platform(format!("failed to create winit event loop: {e}")))?;
    let proxy = event_loop.create_proxy();

    let proxy_wake = proxy.clone();
    let dispatcher = UiDispatcher::with_waker(move || {
        let _ = proxy_wake.send_event(DesktopUserEvent::WakeUi);
    });

    let mut runner = DesktopRunner::new(config, proxy, dispatcher, Some(Box::new(build_ui)), None);
    event_loop
        .run_app(&mut runner)
        .map_err(|e| CraftError::Platform(format!("event loop exited with error: {e}")))
}

struct HotReloadSpec {
    path: PathBuf,
    component: Option<String>,
    session: HotReloadSession,
    _watcher: Option<notify::RecommendedWatcher>,
}

/// Run a live hot-reloading `.cui` preview window using `notify` + `craftui-runtime`.
pub fn run_hot_reload_cui(
    config: WindowConfig,
    cui_path: PathBuf,
    component: Option<String>,
) -> Result<()> {
    let event_loop = EventLoop::<DesktopUserEvent>::with_user_event()
        .build()
        .map_err(|e| CraftError::Platform(format!("failed to create winit event loop: {e}")))?;
    let proxy = event_loop.create_proxy();

    let proxy_wake = proxy.clone();
    let dispatcher = UiDispatcher::with_waker(move || {
        let _ = proxy_wake.send_event(DesktopUserEvent::WakeUi);
    });

    let watch_proxy = proxy.clone();
    let mut watcher = notify::recommended_watcher(move |res: notify::Result<NotifyEvent>| {
        if res.is_ok() {
            let _ = watch_proxy.send_event(DesktopUserEvent::FileChanged);
        }
    })
    .map_err(|e| CraftError::Platform(format!("file watcher init failed: {e}")))?;

    let watch_target = cui_path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| std::path::Path::new("."));
    watcher
        .watch(watch_target, RecursiveMode::Recursive)
        .map_err(|e| CraftError::Platform(format!("watch failed: {e}")))?;

    let root_scope = Scope::root();
    let mut tree = WidgetTree::new(root_scope);
    let mut session = HotReloadSession::new(root_scope);
    let source = std::fs::read_to_string(&cui_path)
        .map_err(|e| CraftError::Platform(format!("failed reading {}: {e}", cui_path.display())))?;
    let _ = session.reload(&source, component.as_deref(), &mut tree)?;

    let spec = HotReloadSpec {
        path: cui_path,
        component,
        session,
        _watcher: Some(watcher),
    };

    let mut runner = DesktopRunner::new_with_tree(
        config,
        proxy,
        dispatcher,
        root_scope,
        tree,
        None,
        Some(spec),
    );
    event_loop
        .run_app(&mut runner)
        .map_err(|e| CraftError::Platform(format!("event loop exited with error: {e}")))
}

struct DesktopRunner {
    config: WindowConfig,
    proxy: EventLoopProxy<DesktopUserEvent>,
    ctx: AppContext,
    root_scope: Scope,
    tree: WidgetTree,
    scene: PaintScene,
    builder: Option<BuildUiFn>,
    hot_reload: Option<HotReloadSpec>,
    window: Option<Arc<Window>>,
    a11y: Option<AccessKitAdapter>,
    gpu: Option<GpuSurfacePipeline>,
    cursor_pos: Point,
    modifiers: Modifiers,
    mouse_down: bool,
    start_instant: Instant,
    last_frame_instant: Instant,
    last_focused_was_input: bool,
}

impl DesktopRunner {
    fn new(
        config: WindowConfig,
        proxy: EventLoopProxy<DesktopUserEvent>,
        dispatcher: UiDispatcher,
        builder: Option<BuildUiFn>,
        hot_reload: Option<HotReloadSpec>,
    ) -> Self {
        let root_scope = Scope::root();
        let tree = WidgetTree::new(root_scope);
        Self::new_with_tree(
            config, proxy, dispatcher, root_scope, tree, builder, hot_reload,
        )
    }

    fn new_with_tree(
        config: WindowConfig,
        proxy: EventLoopProxy<DesktopUserEvent>,
        dispatcher: UiDispatcher,
        root_scope: Scope,
        mut tree: WidgetTree,
        builder: Option<BuildUiFn>,
        hot_reload: Option<HotReloadSpec>,
    ) -> Self {
        tree.metrics = WindowMetrics {
            logical_size: Size::new(config.width, config.height),
            scale_factor: 1.0,
            ..Default::default()
        };
        Self {
            config,
            proxy,
            ctx: AppContext {
                dispatcher,
                textures: TextureRegistry::new(),
            },
            root_scope,
            tree,
            scene: PaintScene::new(),
            builder,
            hot_reload,
            window: None,
            a11y: None,
            gpu: None,
            cursor_pos: Point::ZERO,
            modifiers: Modifiers::default(),
            mouse_down: false,
            start_instant: Instant::now(),
            last_frame_instant: Instant::now(),
            last_focused_was_input: false,
        }
    }

    fn timestamp_ms(&self) -> u64 {
        self.start_instant.elapsed().as_millis() as u64
    }

    fn render_and_present(&mut self) {
        let now = Instant::now();
        let dt = (now - self.last_frame_instant)
            .as_secs_f32()
            .clamp(0.0001, 0.1);
        self.last_frame_instant = now;

        let frame_start = Instant::now();
        let _ = self.ctx.dispatcher.drain();
        let animating = self.tree.tick_animations(dt);

        let layout_start = Instant::now();
        let dirty_count = self.tree.layout(self.tree.metrics);
        let layout_ms = layout_start.elapsed().as_secs_f32() * 1000.0;

        self.tree.paint(&mut self.scene);

        let bg = self.tree.theme().background;
        let scale = self.tree.metrics.scale_factor;
        let tex_reg = &self.ctx.textures;
        if let Some(gpu) = self.gpu.as_mut()
            && let Err(e) =
                gpu.render_frame(&self.scene, scale, bg, tex_reg, &mut self.tree.text_engine)
        {
            tracing::warn!("desktop frame render error: {e}");
        }

        if let Some(a11y) = self.a11y.as_mut() {
            let tree_ref = &mut self.tree;
            a11y.update_if_active(|| {
                tree_ref
                    .build_a11y_update()
                    .unwrap_or_else(|| accesskit::TreeUpdate {
                        nodes: vec![],
                        tree: None,
                        tree_id: accesskit::TreeId::ROOT,
                        focus: accesskit::NodeId(0),
                    })
            });
        }

        let focused_is_input = self
            .tree
            .focused
            .and_then(|id| self.tree.nodes.get(id))
            .is_some_and(|n| {
                matches!(
                    n.kind,
                    craftui_widgets::widgets::WidgetKind::TextInput { .. }
                )
            });
        if focused_is_input != self.last_focused_was_input {
            self.last_focused_was_input = focused_is_input;
            if let Some(win) = self.window.as_ref() {
                win.set_ime_allowed(focused_is_input);
            }
        }

        let frame_ms = frame_start.elapsed().as_secs_f32() * 1000.0;
        let node_count = self.tree.nodes.len();
        self.tree
            .inspector
            .record_frame(frame_ms, layout_ms, node_count, dirty_count);

        if animating && let Some(win) = self.window.as_ref() {
            win.request_redraw();
        }
    }

    fn reload_cui_from_disk(&mut self) {
        let Some(spec) = self.hot_reload.as_mut() else {
            return;
        };
        if let Ok(source) = std::fs::read_to_string(&spec.path)
            && source != spec.session.last_source
        {
            match spec
                .session
                .reload(&source, spec.component.as_deref(), &mut self.tree)
            {
                Ok(_) => {
                    tracing::info!(
                        "Hot-reloaded {} in {:.2}ms",
                        spec.path.display(),
                        spec.session.last_reload_duration.as_secs_f64() * 1000.0
                    );
                    if let Some(win) = self.window.as_ref() {
                        win.request_redraw();
                    }
                }
                Err(e) => {
                    tracing::warn!("Hot-reload diagnostic: {e}");
                }
            }
        }
    }
}

impl ApplicationHandler<DesktopUserEvent> for DesktopRunner {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        event_loop.set_control_flow(ControlFlow::Wait);

        // Create window invisible first so AccessKit adapter can attach cleanly (accesskit_winit contract).
        let attrs = WindowAttributes::default()
            .with_title(self.config.title.clone())
            .with_inner_size(LogicalSize::new(self.config.width, self.config.height))
            .with_resizable(self.config.resizable)
            .with_visible(false);

        let window = match event_loop.create_window(attrs) {
            Ok(w) => Arc::new(w),
            Err(e) => {
                tracing::error!("create_window failed: {e}");
                event_loop.exit();
                return;
            }
        };

        let a11y = AccessKitAdapter::with_event_loop_proxy(event_loop, &window, self.proxy.clone());
        window.set_visible(true);

        let scale = window.scale_factor() as f32;
        let phys = window.inner_size();
        self.tree.metrics = WindowMetrics {
            logical_size: Size::new(phys.width as f32 / scale, phys.height as f32 / scale),
            scale_factor: scale,
            ..Default::default()
        };

        let force_cpu = self.config.force_cpu
            || std::env::var("CRAFTUI_FORCE_CPU")
                .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
                .unwrap_or(false);

        match GpuSurfacePipeline::new(Arc::clone(&window), phys.width, phys.height, force_cpu) {
            Ok(gpu) => {
                tracing::info!(
                    "CraftUI Desktop Renderer initialized ({})",
                    if gpu.is_using_gpu() {
                        "Vello GPU"
                    } else {
                        "Vello CPU Fallback"
                    }
                );
                self.gpu = Some(gpu);
            }
            Err(e) => {
                tracing::error!("GpuSurfacePipeline initialization failed: {e}");
            }
        }

        if let Some(build_fn) = self.builder.take() {
            let root = build_fn(self.root_scope, &mut self.tree, &mut self.ctx);
            self.tree.set_root(root);
        }

        self.window = Some(window.clone());
        self.a11y = Some(a11y);
        window.request_redraw();
    }

    fn user_event(&mut self, _event_loop: &ActiveEventLoop, event: DesktopUserEvent) {
        match event {
            DesktopUserEvent::WakeUi => {
                let _ = self.ctx.dispatcher.drain();
                if let Some(win) = self.window.as_ref() {
                    win.request_redraw();
                }
            }
            DesktopUserEvent::AccessKit(ev) => match &ev.window_event {
                accesskit_winit::WindowEvent::ActionRequested(req) => {
                    if self.tree.handle_a11y_action(req)
                        && let Some(win) = self.window.as_ref()
                    {
                        win.request_redraw();
                    }
                }
                accesskit_winit::WindowEvent::InitialTreeRequested => {
                    if let Some(a11y) = self.a11y.as_mut()
                        && let Some(update) = self.tree.build_a11y_update()
                    {
                        a11y.update_if_active(|| update);
                    }
                }
                accesskit_winit::WindowEvent::AccessibilityDeactivated => {}
            },
            DesktopUserEvent::FileChanged => {
                self.reload_cui_from_disk();
            }
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        if let (Some(win), Some(a11y)) = (self.window.as_ref(), self.a11y.as_mut()) {
            a11y.process_event(win, &event);
        }

        match event {
            WindowEvent::CloseRequested => {
                event_loop.exit();
            }
            WindowEvent::Resized(size) => {
                if let Some(gpu) = self.gpu.as_mut() {
                    gpu.resize(size.width, size.height);
                }
                let sf = self.tree.metrics.scale_factor.max(1.0);
                self.tree.handle_event(&UiEvent::Resize {
                    width: size.width as f32 / sf,
                    height: size.height as f32 / sf,
                    scale_factor: sf,
                });
                if let Some(win) = self.window.as_ref() {
                    win.request_redraw();
                }
            }
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                let sf = scale_factor as f32;
                let phys = self
                    .window
                    .as_ref()
                    .map(|w| w.inner_size())
                    .unwrap_or_default();
                self.tree.handle_event(&UiEvent::Resize {
                    width: phys.width as f32 / sf,
                    height: phys.height as f32 / sf,
                    scale_factor: sf,
                });
                if let Some(win) = self.window.as_ref() {
                    win.request_redraw();
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                let sf = self.tree.metrics.scale_factor.max(1.0);
                self.cursor_pos = Point::new(position.x as f32 / sf, position.y as f32 / sf);
                let ts = self.timestamp_ms();
                if self.tree.handle_event(&UiEvent::Pointer(PointerEvent {
                    id: PointerId(1),
                    phase: PointerPhase::Move,
                    position: self.cursor_pos,
                    button: if self.mouse_down {
                        Some(PointerButton::Primary)
                    } else {
                        None
                    },
                    modifiers: self.modifiers,
                    is_touch: false,
                    timestamp_ms: ts,
                })) && let Some(win) = self.window.as_ref()
                {
                    win.request_redraw();
                }
            }
            WindowEvent::MouseInput { state, button, .. } => {
                let phase = match state {
                    ElementState::Pressed => {
                        self.mouse_down = true;
                        PointerPhase::Down
                    }
                    ElementState::Released => {
                        self.mouse_down = false;
                        PointerPhase::Up
                    }
                };
                let ts = self.timestamp_ms();
                if self.tree.handle_event(&UiEvent::Pointer(PointerEvent {
                    id: PointerId(1),
                    phase,
                    position: self.cursor_pos,
                    button: Some(translate_mouse_button(button)),
                    modifiers: self.modifiers,
                    is_touch: false,
                    timestamp_ms: ts,
                })) && let Some(win) = self.window.as_ref()
                {
                    win.request_redraw();
                }
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let sf = self.tree.metrics.scale_factor.max(1.0);
                let (dx, dy) = translate_scroll_delta(delta, sf);
                if self.tree.handle_event(&UiEvent::Scroll(ScrollDelta {
                    x: dx,
                    y: dy,
                    position: self.cursor_pos,
                })) && let Some(win) = self.window.as_ref()
                {
                    win.request_redraw();
                }
            }
            WindowEvent::Touch(touch) => {
                let sf = self.tree.metrics.scale_factor.max(1.0);
                let pos = Point::new(touch.location.x as f32 / sf, touch.location.y as f32 / sf);
                let phase = match touch.phase {
                    TouchPhase::Started => PointerPhase::Down,
                    TouchPhase::Moved => PointerPhase::Move,
                    TouchPhase::Ended => PointerPhase::Up,
                    TouchPhase::Cancelled => PointerPhase::Cancel,
                };
                let ts = self.timestamp_ms();
                if self.tree.handle_event(&UiEvent::Pointer(PointerEvent {
                    id: PointerId(touch.id + 100),
                    phase,
                    position: pos,
                    button: Some(PointerButton::Primary),
                    modifiers: self.modifiers,
                    is_touch: true,
                    timestamp_ms: ts,
                })) && let Some(win) = self.window.as_ref()
                {
                    win.request_redraw();
                }
            }
            WindowEvent::ModifiersChanged(mods) => {
                self.modifiers = translate_modifiers(mods.state());
            }
            WindowEvent::KeyboardInput { event: key_ev, .. } => {
                let ke = translate_winit_key(&key_ev, self.modifiers);
                if self.tree.handle_event(&UiEvent::Key(ke))
                    && let Some(win) = self.window.as_ref()
                {
                    win.request_redraw();
                }
            }
            WindowEvent::Ime(ime) => {
                let ev = match ime {
                    WinitIme::Enabled => ImeEvent::Enabled,
                    WinitIme::Preedit(s, cur) => ImeEvent::Preedit(s, cur),
                    WinitIme::Commit(s) => ImeEvent::Commit(s),
                    WinitIme::Disabled => ImeEvent::Disabled,
                };
                if self.tree.handle_event(&UiEvent::Ime(ev))
                    && let Some(win) = self.window.as_ref()
                {
                    win.request_redraw();
                }
            }
            WindowEvent::PinchGesture { delta, .. } => {
                if self.tree.handle_event(&UiEvent::Pinch {
                    factor: 1.0 + delta as f32,
                    center: self.cursor_pos,
                }) && let Some(win) = self.window.as_ref()
                {
                    win.request_redraw();
                }
            }
            WindowEvent::Focused(focused) => {
                if self.tree.handle_event(&UiEvent::FocusChanged(focused))
                    && let Some(win) = self.window.as_ref()
                {
                    win.request_redraw();
                }
            }
            WindowEvent::RedrawRequested => {
                self.render_and_present();
            }
            _ => {}
        }
    }
}

pub fn translate_modifiers(state: ModifiersState) -> Modifiers {
    Modifiers {
        shift: state.shift_key(),
        ctrl: state.control_key(),
        alt: state.alt_key(),
        super_key: state.super_key(),
    }
}

pub fn translate_mouse_button(button: MouseButton) -> PointerButton {
    match button {
        MouseButton::Left => PointerButton::Primary,
        MouseButton::Right => PointerButton::Secondary,
        MouseButton::Middle => PointerButton::Middle,
        MouseButton::Back => PointerButton::Back,
        MouseButton::Forward => PointerButton::Forward,
        MouseButton::Other(_) => PointerButton::Primary,
    }
}

pub fn translate_scroll_delta(delta: MouseScrollDelta, scale_factor: f32) -> (f32, f32) {
    match delta {
        MouseScrollDelta::LineDelta(x, y) => (x * 36.0, y * 36.0),
        MouseScrollDelta::PixelDelta(pos) => {
            (pos.x as f32 / scale_factor, pos.y as f32 / scale_factor)
        }
    }
}

pub fn translate_winit_key(key_ev: &winit::event::KeyEvent, modifiers: Modifiers) -> KeyEvent {
    let code = match key_ev.physical_key {
        PhysicalKey::Code(WinitKeyCode::KeyI) => KeyCode::KeyI,
        PhysicalKey::Code(WinitKeyCode::KeyA) => KeyCode::KeyA,
        PhysicalKey::Code(WinitKeyCode::KeyC) => KeyCode::KeyC,
        PhysicalKey::Code(WinitKeyCode::KeyV) => KeyCode::KeyV,
        PhysicalKey::Code(WinitKeyCode::KeyX) => KeyCode::KeyX,
        PhysicalKey::Code(WinitKeyCode::KeyZ) => KeyCode::KeyZ,
        _ => match &key_ev.logical_key {
            Key::Named(NamedKey::Enter) => KeyCode::Enter,
            Key::Named(NamedKey::Tab) => KeyCode::Tab,
            Key::Named(NamedKey::Backspace) => KeyCode::Backspace,
            Key::Named(NamedKey::Delete) => KeyCode::Delete,
            Key::Named(NamedKey::Escape) => KeyCode::Escape,
            Key::Named(NamedKey::Space) => KeyCode::Space,
            Key::Named(NamedKey::ArrowLeft) => KeyCode::ArrowLeft,
            Key::Named(NamedKey::ArrowRight) => KeyCode::ArrowRight,
            Key::Named(NamedKey::ArrowUp) => KeyCode::ArrowUp,
            Key::Named(NamedKey::ArrowDown) => KeyCode::ArrowDown,
            Key::Named(NamedKey::Home) => KeyCode::Home,
            Key::Named(NamedKey::End) => KeyCode::End,
            Key::Named(NamedKey::PageUp) => KeyCode::PageUp,
            Key::Named(NamedKey::PageDown) => KeyCode::PageDown,
            Key::Character(s) => KeyCode::Character(s.to_string()),
            _ => KeyCode::Unknown,
        },
    };
    let text = key_ev.text.as_ref().map(|t| t.to_string());
    KeyEvent {
        code,
        text,
        pressed: key_ev.state == ElementState::Pressed,
        repeat: key_ev.repeat,
        modifiers,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn translates_mouse_buttons_and_scroll_deltas() {
        assert_eq!(
            translate_mouse_button(MouseButton::Left),
            PointerButton::Primary
        );
        assert_eq!(
            translate_mouse_button(MouseButton::Right),
            PointerButton::Secondary
        );

        let (dx, dy) = translate_scroll_delta(MouseScrollDelta::LineDelta(0.0, -2.0), 1.0);
        assert_eq!(dx, 0.0);
        assert_eq!(dy, -72.0);

        let mods = translate_modifiers(ModifiersState::CONTROL | ModifiersState::SHIFT);
        assert!(mods.ctrl && mods.shift && !mods.alt);
    }
}
