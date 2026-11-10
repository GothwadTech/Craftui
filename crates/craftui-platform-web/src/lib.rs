//! WebAssembly platform adapter (`wasm-bindgen` + `web-sys` Canvas/DPR observer +
//! async WebGPU/WebGL2 + 2D Canvas fallback + hidden `<input>` DOM `input`/IME bridge +
//! `window.location.hash` router sync + OS/browser clipboard + monotonic timers).

use craftui_core::{
    ClipboardBackend, ImeEvent, KeyCode, KeyEvent, Modifiers, PlatformServices, Point,
    PointerButton, PointerEvent, PointerId, PointerPhase, ScrollDelta, Size, UiDispatcher, UiEvent,
    WindowMetrics,
};
use craftui_reactive::Scope;
use craftui_render::{BackendKind, PaintScene, TextureRegistry, render_scene_to_rgba};
use craftui_widgets::{Router, WidgetKind, WidgetTree};
use std::rc::Rc;
use std::sync::{Arc, Mutex};

/// Configuration for bootstrapping a CraftUI WASM application inside an HTML canvas element.
#[derive(Debug, Clone)]
pub struct WebBridgeConfig {
    pub canvas_id: String,
    pub sync_url_hash: bool,
    pub force_2d_fallback: bool,
}

impl Default for WebBridgeConfig {
    fn default() -> Self {
        Self {
            canvas_id: "craftui-canvas".to_string(),
            sync_url_hash: true,
            force_2d_fallback: false,
        }
    }
}

/// Synchronizes hidden `<input>` accessibility/IME overlay coordinates with the focused `TextInput`.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct HiddenImeInputState {
    pub visible: bool,
    pub x_dp: f32,
    pub y_dp: f32,
    pub width_dp: f32,
    pub height_dp: f32,
    pub value: String,
}

/// Type-erased router binding connecting CraftUI [`Router<R>`] state to browser URL hashes.
#[derive(Clone)]
pub struct RouteBridge {
    current_route: Rc<dyn Fn() -> String>,
    apply_route: Rc<dyn Fn(&str) -> bool>,
}

impl RouteBridge {
    /// Construct a [`RouteBridge`] for any custom route enum/struct `R`.
    pub fn new<R: Clone + PartialEq + 'static>(
        router: Router<R>,
        to_path: fn(&R) -> String,
        from_path: fn(&str) -> R,
    ) -> Self {
        Self {
            current_route: Rc::new(move || to_path(&router.current())),
            apply_route: Rc::new(move |path: &str| {
                let target = from_path(path);
                if router.current() != target {
                    router.push(target);
                    true
                } else {
                    false
                }
            }),
        }
    }

    pub fn current_route(&self) -> String {
        (self.current_route)()
    }

    pub fn navigate(&self, route: &str) -> bool {
        (self.apply_route)(route)
    }
}

/// Maps CraftUI router route strings to/from browser `window.location.hash` fragments (`#/route`).
pub struct BrowserRouteSync;

impl BrowserRouteSync {
    pub fn route_to_hash(route: &str) -> String {
        let trimmed = route.trim_start_matches('#').trim_start_matches('/');
        format!("#/{trimmed}")
    }

    pub fn hash_to_route(hash: &str) -> String {
        let trimmed = hash.trim_start_matches('#').trim_start_matches('/');
        if trimmed.is_empty() {
            "/".to_string()
        } else {
            format!("/{trimmed}")
        }
    }

    /// Connect a typed [`Router<R>`] in `scope` so WASM `hashchange` / `window.location.hash` stay synchronized.
    pub fn connect_router<R: Clone + PartialEq + 'static>(
        scope: Scope,
        router: Router<R>,
        to_path: fn(&R) -> String,
        from_path: fn(&str) -> R,
    ) {
        scope.provide_context(RouteBridge::new(router, to_path, from_path));
    }

    /// Connect a `Router<String>` directly in `scope` using raw path strings (e.g., `"/"`, `"/settings"`).
    pub fn connect_string_router(scope: Scope, router: Router<String>) {
        scope.provide_context(RouteBridge::new(
            router,
            |s| s.clone(),
            |path| path.to_string(),
        ));
    }
}

/// Browser clipboard bridge backed by `navigator.clipboard` (on `wasm32`) and a synchronous cache.
#[derive(Clone, Default)]
pub struct WebClipboard {
    cache: Arc<Mutex<String>>,
}

impl WebClipboard {
    pub fn new(cache: Arc<Mutex<String>>) -> Self {
        Self { cache }
    }
}

impl ClipboardBackend for WebClipboard {
    fn get_text(&mut self) -> Option<String> {
        self.cache.lock().ok().map(|g| g.clone())
    }

    fn set_text(&mut self, text: &str) -> bool {
        if let Ok(mut guard) = self.cache.lock() {
            guard.clear();
            guard.push_str(text);
        }
        #[cfg(target_arch = "wasm32")]
        if let Some(window) = web_sys::window() {
            let _ = window.navigator().clipboard().write_text(text);
        }
        true
    }
}

/// Platform-neutral WASM host managing logical DPR sizing, hidden IME sync, router hash sync, and rendering.
pub struct WebAppHost {
    pub scope: Scope,
    pub tree: WidgetTree,
    pub scene: PaintScene,
    pub textures: TextureRegistry,
    pub dispatcher: UiDispatcher,
    route_bridge: Option<RouteBridge>,
    last_synced_hash: Option<String>,
    pub clipboard_cache: Arc<Mutex<String>>,
    needs_redraw: bool,
}

impl WebAppHost {
    pub fn new(dispatcher: UiDispatcher, css_width: f32, css_height: f32, dpr: f32) -> Self {
        let scope = Scope::root();
        let mut tree = WidgetTree::new(scope);
        let clipboard_cache = Arc::new(Mutex::new(String::new()));
        tree.set_platform_services(
            PlatformServices::in_memory()
                .with_clipboard(WebClipboard::new(clipboard_cache.clone())),
        );
        tree.metrics = WindowMetrics {
            logical_size: Size::new(css_width.max(1.0), css_height.max(1.0)),
            scale_factor: dpr.max(1.0),
            ..Default::default()
        };
        Self {
            scope,
            tree,
            scene: PaintScene::new(),
            textures: TextureRegistry::new(),
            dispatcher,
            route_bridge: None,
            last_synced_hash: None,
            clipboard_cache,
            needs_redraw: true,
        }
    }

    pub fn request_redraw(&mut self) {
        self.needs_redraw = true;
    }

    pub fn handle_ui_event(&mut self, ev: &UiEvent) -> bool {
        let handled = self.tree.handle_event(ev);
        if handled {
            self.needs_redraw = true;
        }
        handled
    }

    /// Resolve a [`RouteBridge`] from explicit attachment or `Scope` context (`RouteBridge` or `Router<String>`).
    fn active_route_bridge(&mut self) -> Option<RouteBridge> {
        if let Some(ref rb) = self.route_bridge {
            return Some(rb.clone());
        }
        if let Some(rb) = self.scope.use_context::<RouteBridge>() {
            self.route_bridge = Some(rb.clone());
            return Some(rb);
        }
        if let Some(router) = self.scope.use_context::<Router<String>>() {
            let rb = RouteBridge::new(router, |s| s.clone(), |p| p.to_string());
            self.route_bridge = Some(rb.clone());
            return Some(rb);
        }
        None
    }

    pub fn attach_route_bridge(&mut self, bridge: RouteBridge) {
        self.route_bridge = Some(bridge);
    }

    /// Apply a browser URL hash string (from initial load or `hashchange`) to the attached router.
    pub fn apply_url_hash(&mut self, hash: &str) -> bool {
        let Some(bridge) = self.active_route_bridge() else {
            return false;
        };
        if hash.trim().is_empty() {
            let cur_hash = BrowserRouteSync::route_to_hash(&bridge.current_route());
            self.last_synced_hash = Some(cur_hash);
            return false;
        }
        let route = BrowserRouteSync::hash_to_route(hash);
        let changed = bridge.navigate(&route);
        if changed {
            self.needs_redraw = true;
        }
        self.last_synced_hash = Some(BrowserRouteSync::route_to_hash(&bridge.current_route()));
        changed
    }

    /// Check whether the attached router moved to a new route and return the new `#/route` hash to apply.
    pub fn take_pending_hash_update(&mut self) -> Option<String> {
        let bridge = self.active_route_bridge()?;
        let target_hash = BrowserRouteSync::route_to_hash(&bridge.current_route());
        if self.last_synced_hash.as_deref() != Some(target_hash.as_str()) {
            self.last_synced_hash = Some(target_hash.clone());
            Some(target_hash)
        } else {
            None
        }
    }

    pub fn resize_css(&mut self, css_width: f32, css_height: f32, dpr: f32) -> (u32, u32) {
        let w = css_width.max(1.0);
        let h = css_height.max(1.0);
        let s = dpr.max(1.0);
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
        self.tree.metrics.physical_size()
    }

    pub fn hidden_ime_state(&self) -> HiddenImeInputState {
        let Some(focused) = self.tree.focused else {
            return HiddenImeInputState::default();
        };
        let Some(node) = self.tree.nodes.get(focused) else {
            return HiddenImeInputState::default();
        };
        match &node.kind {
            WidgetKind::TextInput { editor, .. } => {
                let r = self.tree.rect(focused);
                HiddenImeInputState {
                    visible: true,
                    x_dp: r.x,
                    y_dp: r.y,
                    width_dp: r.width,
                    height_dp: r.height,
                    value: editor.text.clone(),
                }
            }
            _ => HiddenImeInputState::default(),
        }
    }

    /// Advance one UI frame at `now_ms`, painting only when dirty, animating, or blink toggled.
    pub fn step_frame_with_redraw(&mut self, dt_sec: f32, now_ms: u64) -> (bool, bool) {
        let timers = craftui_reactive::tick_timers_at(now_ms);
        let jobs = self.dispatcher.drain();
        let blink = self.tree.tick_blink_timers(now_ms);
        let anim = self.tree.tick_animations(dt_sec);
        let dirty = self.tree.layout(self.tree.metrics);
        let should_paint =
            self.needs_redraw || timers > 0 || jobs > 0 || blink || anim || dirty > 0;
        if should_paint {
            self.tree.paint(&mut self.scene);
            self.needs_redraw = false;
        }
        (anim, should_paint)
    }

    pub fn step_frame(&mut self, dt_sec: f32) -> bool {
        let (anim, _) = self.step_frame_with_redraw(dt_sec, craftui_core::monotonic_ms());
        anim
    }

    /// Render the current scene to RGBA pixels for 2D `<canvas>` `ImageData` fallback or headless testing.
    pub fn render_rgba_2d(&mut self) -> (u32, u32, Vec<u8>) {
        let (pw, ph) = self.tree.metrics.physical_size();
        let bg = self.tree.theme().background;
        let sf = self.tree.metrics.scale_factor;
        let rgba = render_scene_to_rgba(
            BackendKind::VelloCpu,
            &self.scene,
            pw,
            ph,
            sf,
            bg,
            &self.textures,
            &mut self.tree.text_engine,
        );
        (pw, ph, rgba)
    }
}

/// Convert browser `KeyboardEvent.key` string into a CraftUI [`KeyEvent`].
pub fn dom_key_to_craft(key: &str, pressed: bool, repeat: bool, modifiers: Modifiers) -> KeyEvent {
    let cmd = modifiers.command();
    let code = match key {
        "Enter" => KeyCode::Enter,
        "Tab" => KeyCode::Tab,
        "Backspace" => KeyCode::Backspace,
        "Delete" => KeyCode::Delete,
        "Escape" => KeyCode::Escape,
        " " => KeyCode::Space,
        "ArrowLeft" => KeyCode::ArrowLeft,
        "ArrowRight" => KeyCode::ArrowRight,
        "ArrowUp" => KeyCode::ArrowUp,
        "ArrowDown" => KeyCode::ArrowDown,
        "Home" => KeyCode::Home,
        "End" => KeyCode::End,
        "PageUp" => KeyCode::PageUp,
        "PageDown" => KeyCode::PageDown,
        "i" | "I" if cmd && modifiers.shift => KeyCode::KeyI,
        "a" | "A" if cmd => KeyCode::KeyA,
        "c" | "C" if cmd => KeyCode::KeyC,
        "v" | "V" if cmd => KeyCode::KeyV,
        "x" | "X" if cmd => KeyCode::KeyX,
        "y" | "Y" if modifiers.ctrl => KeyCode::KeyY,
        "z" | "Z" if cmd => KeyCode::KeyZ,
        s if s.chars().count() == 1 => KeyCode::Character(s.to_string()),
        _ => KeyCode::Unknown,
    };
    let text = if key.chars().count() == 1 {
        Some(key.to_string())
    } else {
        None
    };
    KeyEvent {
        code,
        text,
        pressed,
        repeat,
        modifiers,
    }
}

/// Convert mobile/virtual-keyboard DOM `InputEvent` updates on `<input>` into CraftUI key/IME events.
pub fn dom_input_to_events(
    prev: &str,
    current: &str,
    input_type: &str,
    data: Option<&str>,
) -> Vec<UiEvent> {
    let mut events = Vec::new();
    match input_type {
        "deleteContentBackward" | "deleteWordBackward" | "deleteSoftLineBackward" => {
            events.push(UiEvent::Key(KeyEvent {
                code: KeyCode::Backspace,
                text: None,
                pressed: true,
                repeat: false,
                modifiers: Modifiers::default(),
            }));
            return events;
        }
        "deleteContentForward" | "deleteWordForward" | "deleteSoftLineForward" => {
            events.push(UiEvent::Key(KeyEvent {
                code: KeyCode::Delete,
                text: None,
                pressed: true,
                repeat: false,
                modifiers: Modifiers::default(),
            }));
            return events;
        }
        "insertLineBreak" | "insertParagraph" => {
            events.push(UiEvent::Key(KeyEvent {
                code: KeyCode::Enter,
                text: Some("\n".to_string()),
                pressed: true,
                repeat: false,
                modifiers: Modifiers::default(),
            }));
            return events;
        }
        _ => {}
    }

    if let Some(text) = data
        && !text.is_empty()
        && matches!(
            input_type,
            "insertText" | "insertFromPaste" | "insertCompositionText" | ""
        )
    {
        events.push(UiEvent::Ime(ImeEvent::Commit(text.to_string())));
        return events;
    }

    if prev == current {
        return events;
    }

    // Fallback diff between `prev` and `current` for Android Chrome Gboard batch mutations
    let prev_chars: Vec<char> = prev.chars().collect();
    let curr_chars: Vec<char> = current.chars().collect();
    let common_prefix = prev_chars
        .iter()
        .zip(curr_chars.iter())
        .take_while(|(a, b)| a == b)
        .count();
    let removed = prev_chars.len().saturating_sub(common_prefix);
    for _ in 0..removed {
        events.push(UiEvent::Key(KeyEvent {
            code: KeyCode::Backspace,
            text: None,
            pressed: true,
            repeat: false,
            modifiers: Modifiers::default(),
        }));
    }
    if curr_chars.len() > common_prefix {
        let inserted: String = curr_chars[common_prefix..].iter().collect();
        events.push(UiEvent::Ime(ImeEvent::Commit(inserted)));
    }
    events
}

pub fn dom_button_to_craft(button: i16) -> PointerButton {
    match button {
        2 => PointerButton::Secondary,
        1 => PointerButton::Middle,
        3 => PointerButton::Back,
        4 => PointerButton::Forward,
        _ => PointerButton::Primary,
    }
}

#[allow(clippy::too_many_arguments)]
pub fn dom_pointer_event(
    pointer_id: i32,
    phase: PointerPhase,
    client_x: f32,
    client_y: f32,
    rect_left: f32,
    rect_top: f32,
    button: i16,
    is_touch: bool,
    modifiers: Modifiers,
    timestamp_ms: u64,
) -> UiEvent {
    UiEvent::Pointer(PointerEvent {
        id: PointerId(pointer_id as u64),
        phase,
        position: Point::new(client_x - rect_left, client_y - rect_top),
        button: Some(dom_button_to_craft(button)),
        modifiers,
        is_touch,
        timestamp_ms,
    })
}

pub fn dom_wheel_event(
    client_x: f32,
    client_y: f32,
    rect_left: f32,
    rect_top: f32,
    delta_x: f32,
    delta_y: f32,
) -> UiEvent {
    UiEvent::Scroll(ScrollDelta {
        x: -delta_x,
        y: -delta_y,
        position: Point::new(client_x - rect_left, client_y - rect_top),
    })
}

pub fn dom_composition_to_ime(phase_end: bool, data: String) -> UiEvent {
    if phase_end {
        UiEvent::Ime(ImeEvent::Commit(data))
    } else {
        let len = data.len();
        UiEvent::Ime(ImeEvent::Preedit(data, Some((len, len))))
    }
}

#[cfg(target_arch = "wasm32")]
pub mod wasm_runner {
    use super::*;
    use craftui_core::{NodeId, set_monotonic_ms_provider};
    use craftui_render::GpuSurfacePipeline;
    use std::cell::{Cell, RefCell};
    use std::rc::{Rc, Weak};
    use wasm_bindgen::JsCast;
    use wasm_bindgen::closure::Closure;
    use web_sys::{
        CanvasRenderingContext2d, CompositionEvent, HashChangeEvent, HtmlCanvasElement,
        HtmlInputElement, ImageData, InputEvent as DomInputEvent,
        KeyboardEvent as DomKeyboardEvent, PointerEvent as DomPointerEvent,
        WheelEvent as DomWheelEvent,
    };

    thread_local! {
        static ACTIVE_SCHEDULERS: RefCell<Vec<Weak<WebScheduler>>> = const { RefCell::new(Vec::new()) };
    }

    fn browser_now_ms() -> u64 {
        web_sys::window()
            .and_then(|w| w.performance())
            .map(|p| p.now().max(0.0) as u64)
            .unwrap_or(0)
    }

    enum WebRenderBackend {
        Pending,
        Gpu(GpuSurfacePipeline),
        Canvas2d(CanvasRenderingContext2d),
    }

    struct WebScheduler {
        window: web_sys::Window,
        raf_scheduled: Cell<bool>,
        timeout_id: Cell<Option<i32>>,
        raf_cb: RefCell<Option<Closure<dyn FnMut(f64)>>>,
        timer_cb: RefCell<Option<Closure<dyn FnMut()>>>,
    }

    impl WebScheduler {
        fn new(window: web_sys::Window) -> Rc<Self> {
            Rc::new(Self {
                window,
                raf_scheduled: Cell::new(false),
                timeout_id: Cell::new(None),
                raf_cb: RefCell::new(None),
                timer_cb: RefCell::new(None),
            })
        }

        fn request_frame(&self) {
            if let Some(tid) = self.timeout_id.take() {
                self.window.clear_timeout_with_handle(tid);
            }
            if self.raf_scheduled.get() {
                return;
            }
            if let Some(ref closure) = *self.raf_cb.borrow() {
                self.raf_scheduled.set(true);
                let _ = self
                    .window
                    .request_animation_frame(closure.as_ref().unchecked_ref());
            }
        }

        fn schedule_wake_after(&self, delay_ms: u64) {
            if self.raf_scheduled.get() {
                return;
            }
            if let Some(tid) = self.timeout_id.take() {
                self.window.clear_timeout_with_handle(tid);
            }
            if let Some(ref tcb) = *self.timer_cb.borrow() {
                let ms = delay_ms.clamp(1, i32::MAX as u64) as i32;
                if let Ok(id) = self
                    .window
                    .set_timeout_with_callback_and_timeout_and_arguments_0(
                        tcb.as_ref().unchecked_ref(),
                        ms,
                    )
                {
                    self.timeout_id.set(Some(id));
                }
            }
        }
    }

    fn obtain_2d_context(
        canvas_slot: &Rc<RefCell<HtmlCanvasElement>>,
        host: &Rc<RefCell<WebAppHost>>,
        scheduler: &Rc<WebScheduler>,
    ) -> Option<CanvasRenderingContext2d> {
        let existing = canvas_slot.borrow().clone();
        if let Ok(Some(ctx)) = existing.get_context("2d")
            && let Ok(c2d) = ctx.dyn_into::<CanvasRenderingContext2d>()
        {
            return Some(c2d);
        }
        // If failed WebGPU/WebGL2 attempt locked the `<canvas>` context type, replace with a fresh clone.
        if let Ok(cloned_node) = existing.clone_node_with_deep(false)
            && let Ok(fresh_canvas) = cloned_node.dyn_into::<HtmlCanvasElement>()
        {
            fresh_canvas.set_width(existing.width());
            fresh_canvas.set_height(existing.height());
            let _ = existing.replace_with_with_node_1(&fresh_canvas);
            let _ = attach_pointer_listeners(&fresh_canvas, host.clone(), scheduler.clone());
            *canvas_slot.borrow_mut() = fresh_canvas.clone();
            if let Ok(Some(ctx)) = fresh_canvas.get_context("2d") {
                return ctx.dyn_into::<CanvasRenderingContext2d>().ok();
            }
        }
        None
    }

    /// Mount and start a reactive CraftUI application inside an HTML5 `<canvas>` element.
    pub fn start_web_app<F>(
        config: WebBridgeConfig,
        build_ui: F,
    ) -> Result<(), wasm_bindgen::JsValue>
    where
        F: FnOnce(Scope, &mut WidgetTree, &mut TextureRegistry, &UiDispatcher) -> NodeId + 'static,
    {
        let window =
            web_sys::window().ok_or_else(|| wasm_bindgen::JsValue::from_str("no window"))?;
        let document = window
            .document()
            .ok_or_else(|| wasm_bindgen::JsValue::from_str("no document"))?;
        let el = document
            .get_element_by_id(&config.canvas_id)
            .ok_or_else(|| wasm_bindgen::JsValue::from_str("canvas element not found"))?;
        let canvas: HtmlCanvasElement = el.dyn_into()?;

        let dpr = window.device_pixel_ratio() as f32;
        let rect = canvas.get_bounding_client_rect();
        let css_w = (rect.width() as f32).max(320.0);
        let css_h = (rect.height() as f32).max(240.0);

        set_monotonic_ms_provider(browser_now_ms);
        let scheduler = WebScheduler::new(window.clone());
        let sched_weak = Rc::downgrade(&scheduler);
        ACTIVE_SCHEDULERS.with(|list| {
            let mut list = list.borrow_mut();
            list.retain(|w| w.strong_count() > 0);
            list.push(sched_weak);
        });
        let dispatcher = UiDispatcher::with_waker(|| {
            ACTIVE_SCHEDULERS.with(|list| {
                for sched in list.borrow().iter().filter_map(|w| w.upgrade()) {
                    sched.request_frame();
                }
            });
        });
        let mut host = WebAppHost::new(dispatcher.clone(), css_w, css_h, dpr);
        let (pw, ph) = host.resize_css(css_w, css_h, dpr);
        canvas.set_width(pw);
        canvas.set_height(ph);

        let root = build_ui(host.scope, &mut host.tree, &mut host.textures, &dispatcher);
        host.tree.set_root(root);

        // Apply initial browser URL hash route before first frame if hash routing is enabled.
        if config.sync_url_hash
            && let Ok(hash) = window.location().hash()
        {
            host.apply_url_hash(&hash);
        }

        // Hidden <input> element overlay for native mobile browser keyboards & CJK IME composition
        let hidden_input: HtmlInputElement = document.create_element("input")?.dyn_into()?;
        hidden_input.set_type("text");
        let st = hidden_input.style();
        let _ = st.set_property("position", "fixed");
        let _ = st.set_property("opacity", "0");
        let _ = st.set_property("pointer-events", "none");
        let _ = st.set_property("width", "1px");
        let _ = st.set_property("height", "1px");
        if let Some(body) = document.body() {
            let _ = body.append_child(&hidden_input);
        }

        let host_rc = Rc::new(RefCell::new(host));
        let canvas_slot = Rc::new(RefCell::new(canvas.clone()));
        let last_hidden_val = Rc::new(RefCell::new(String::new()));
        let is_composing = Rc::new(Cell::new(false));

        attach_pointer_listeners(&canvas, host_rc.clone(), scheduler.clone())?;
        attach_keyboard_ime_listeners(
            &window,
            &hidden_input,
            host_rc.clone(),
            last_hidden_val.clone(),
            is_composing.clone(),
            scheduler.clone(),
        )?;

        if config.sync_url_hash {
            let h_hash = host_rc.clone();
            let sched_hash = scheduler.clone();
            let hash_cb = Closure::wrap(Box::new(move |_ev: HashChangeEvent| {
                if let Some(win) = web_sys::window()
                    && let Ok(hash) = win.location().hash()
                    && h_hash.borrow_mut().apply_url_hash(&hash)
                {
                    sched_hash.request_frame();
                }
            }) as Box<dyn FnMut(HashChangeEvent)>);
            window
                .add_event_listener_with_callback("hashchange", hash_cb.as_ref().unchecked_ref())?;
            hash_cb.forget();
        }

        {
            let h_res = host_rc.clone();
            let sched_res = scheduler.clone();
            let resize_cb = Closure::wrap(Box::new(move |_ev: web_sys::Event| {
                h_res.borrow_mut().request_redraw();
                sched_res.request_frame();
            }) as Box<dyn FnMut(web_sys::Event)>);
            window
                .add_event_listener_with_callback("resize", resize_cb.as_ref().unchecked_ref())?;
            resize_cb.forget();
        }

        let backend_slot: Rc<RefCell<WebRenderBackend>> =
            Rc::new(RefCell::new(WebRenderBackend::Pending));
        if config.force_2d_fallback {
            if let Some(c2d) = obtain_2d_context(&canvas_slot, &host_rc, &scheduler) {
                *backend_slot.borrow_mut() = WebRenderBackend::Canvas2d(c2d);
            }
        } else {
            let backend_async = backend_slot.clone();
            let canvas_async = canvas_slot.clone();
            let host_async = host_rc.clone();
            let sched_async = scheduler.clone();
            let canvas_for_wgpu = canvas.clone();
            wasm_bindgen_futures::spawn_local(async move {
                match GpuSurfacePipeline::new_async(
                    wgpu::SurfaceTarget::Canvas(canvas_for_wgpu),
                    pw,
                    ph,
                    false,
                )
                .await
                {
                    Ok(gpu) => {
                        tracing::info!(
                            "CraftUI Web renderer initialized ({})",
                            if gpu.is_using_gpu() {
                                "WebGPU/Vello GPU"
                            } else {
                                "WebGL2/CPUFallback"
                            }
                        );
                        *backend_async.borrow_mut() = WebRenderBackend::Gpu(gpu);
                    }
                    Err(err) => {
                        tracing::warn!(
                            "WebGPU/WebGL2 async init failed ({err}); falling back to 2D canvas"
                        );
                        if let Some(c2d) = obtain_2d_context(&canvas_async, &host_async, &sched_async) {
                            *backend_async.borrow_mut() = WebRenderBackend::Canvas2d(c2d);
                        }
                    }
                }
                host_async.borrow_mut().request_redraw();
                sched_async.request_frame();
            });
        }

        {
            let sched_timer = scheduler.clone();
            let h_timer = host_rc.clone();
            *scheduler.timer_cb.borrow_mut() = Some(Closure::wrap(Box::new(move || {
                sched_timer.timeout_id.set(None);
                h_timer.borrow_mut().request_redraw();
                sched_timer.request_frame();
            }) as Box<dyn FnMut()>));
        }

        let sched_raf = scheduler.clone();
        let canvas_raf = canvas_slot;
        let window_raf = window.clone();
        let doc_raf = document;
        let hidden_raf = hidden_input;
        let sync_hash = config.sync_url_hash;
        let mut last_ts_ms = window.performance().map(|p| p.now()).unwrap_or(0.0);

        *scheduler.raf_cb.borrow_mut() = Some(Closure::wrap(Box::new(move |ts: f64| {
            sched_raf.raf_scheduled.set(false);
            let dt_ms = (ts - last_ts_ms).max(1.0);
            last_ts_ms = ts;
            let dt_sec = ((dt_ms as f32) / 1000.0).clamp(0.001, 0.05);
            let now_ms = window_raf
                .performance()
                .map(|p| p.now() as u64)
                .unwrap_or(ts.max(0.0) as u64);

            let mut h = host_rc.borrow_mut();
            let dpr_now = window_raf.device_pixel_ratio() as f32;
            let cur_canvas = canvas_raf.borrow().clone();
            let bcr = cur_canvas.get_bounding_client_rect();
            let (pw, ph) = h.resize_css(bcr.width() as f32, bcr.height() as f32, dpr_now);
            if cur_canvas.width() != pw || cur_canvas.height() != ph {
                cur_canvas.set_width(pw);
                cur_canvas.set_height(ph);
                h.request_redraw();
            }

            let (anim, did_paint) = h.step_frame_with_redraw(dt_sec, now_ms);

            if sync_hash && let Some(new_hash) = h.take_pending_hash_update() {
                let _ = window_raf.location().set_hash(&new_hash);
            }

            let ime = h.hidden_ime_state();
            if ime.visible {
                let ist = hidden_raf.style();
                let _ = ist.set_property("left", &format!("{}px", bcr.left() as f32 + ime.x_dp));
                let _ = ist.set_property("top", &format!("{}px", bcr.top() as f32 + ime.y_dp));
                if !is_composing.get() && *last_hidden_val.borrow() != ime.value {
                    hidden_raf.set_value(&ime.value);
                    *last_hidden_val.borrow_mut() = ime.value.clone();
                }
                let hidden_node: &web_sys::Element = hidden_raf.as_ref();
                let already_focused = doc_raf
                    .active_element()
                    .is_some_and(|ae| &ae == hidden_node);
                if !already_focused {
                    let _ = hidden_raf.focus();
                }
            }

            if did_paint {
                match &mut *backend_slot.borrow_mut() {
                    WebRenderBackend::Pending => {}
                    WebRenderBackend::Gpu(gpu) => {
                        gpu.resize(pw, ph);
                        let bg = h.tree.theme().background;
                        let host_ref = &mut *h;
                        let _ = gpu.render_frame(
                            &host_ref.scene,
                            dpr_now,
                            bg,
                            &host_ref.textures,
                            &mut host_ref.tree.text_engine,
                        );
                    }
                    WebRenderBackend::Canvas2d(ctx_2d) => {
                        let (w, h_px, rgba) = h.render_rgba_2d();
                        if let Ok(img) = ImageData::new_with_u8_clamped_array_and_sh(
                            wasm_bindgen::Clamped(&rgba),
                            w,
                            h_px,
                        ) {
                            let _ = ctx_2d.put_image_data(&img, 0.0, 0.0);
                        }
                    }
                }
            }

            if anim {
                sched_raf.request_frame();
            } else {
                let mut min_delay = craftui_reactive::next_timer_delay_ms(now_ms);
                if let Some(d) = h.tree.next_internal_delay_ms(now_ms) {
                    min_delay = Some(min_delay.map_or(d, |cur| cur.min(d)));
                }
                if let Some(delay) = min_delay {
                    sched_raf.schedule_wake_after(delay);
                }
            }
        }) as Box<dyn FnMut(f64)>));

        scheduler.request_frame();
        std::mem::forget(scheduler);
        Ok(())
    }

    fn attach_pointer_listeners(
        canvas: &HtmlCanvasElement,
        host: Rc<RefCell<WebAppHost>>,
        scheduler: Rc<WebScheduler>,
    ) -> Result<(), wasm_bindgen::JsValue> {
        for (ev_name, phase) in [
            ("pointerdown", PointerPhase::Down),
            ("pointermove", PointerPhase::Move),
            ("pointerup", PointerPhase::Up),
            ("pointercancel", PointerPhase::Cancel),
        ] {
            let h_c = host.clone();
            let c_el = canvas.clone();
            let sched_c = scheduler.clone();
            let cb = Closure::wrap(Box::new(move |e: DomPointerEvent| {
                let bcr = c_el.get_bounding_client_rect();
                let mods = Modifiers {
                    shift: e.shift_key(),
                    ctrl: e.ctrl_key(),
                    alt: e.alt_key(),
                    super_key: e.meta_key(),
                };
                let ev = dom_pointer_event(
                    e.pointer_id(),
                    phase,
                    e.client_x() as f32,
                    e.client_y() as f32,
                    bcr.left() as f32,
                    bcr.top() as f32,
                    e.button(),
                    e.pointer_type() == "touch",
                    mods,
                    e.time_stamp() as u64,
                );
                if h_c.borrow_mut().handle_ui_event(&ev) {
                    sched_c.request_frame();
                }
            }) as Box<dyn FnMut(DomPointerEvent)>);
            canvas.add_event_listener_with_callback(ev_name, cb.as_ref().unchecked_ref())?;
            cb.forget();
        }

        let h_w = host;
        let c_w = canvas.clone();
        let sched_w = scheduler;
        let wheel_cb = Closure::wrap(Box::new(move |e: DomWheelEvent| {
            let bcr = c_w.get_bounding_client_rect();
            let ev = dom_wheel_event(
                e.client_x() as f32,
                e.client_y() as f32,
                bcr.left() as f32,
                bcr.top() as f32,
                e.delta_x() as f32,
                e.delta_y() as f32,
            );
            if h_w.borrow_mut().handle_ui_event(&ev) {
                sched_w.request_frame();
            }
        }) as Box<dyn FnMut(DomWheelEvent)>);
        canvas.add_event_listener_with_callback("wheel", wheel_cb.as_ref().unchecked_ref())?;
        wheel_cb.forget();
        Ok(())
    }

    fn attach_keyboard_ime_listeners(
        window: &web_sys::Window,
        hidden_input: &HtmlInputElement,
        host: Rc<RefCell<WebAppHost>>,
        last_hidden_val: Rc<RefCell<String>>,
        is_composing: Rc<Cell<bool>>,
        scheduler: Rc<WebScheduler>,
    ) -> Result<(), wasm_bindgen::JsValue> {
        for (ev_name, pressed) in [("keydown", true), ("keyup", false)] {
            let h_c = host.clone();
            let sched_k = scheduler.clone();
            let cb = Closure::wrap(Box::new(move |e: DomKeyboardEvent| {
                let mods = Modifiers {
                    shift: e.shift_key(),
                    ctrl: e.ctrl_key(),
                    alt: e.alt_key(),
                    super_key: e.meta_key(),
                };
                let ke = dom_key_to_craft(&e.key(), pressed, e.repeat(), mods);
                if h_c.borrow_mut().handle_ui_event(&UiEvent::Key(ke)) {
                    sched_k.request_frame();
                }
            }) as Box<dyn FnMut(DomKeyboardEvent)>);
            window.add_event_listener_with_callback(ev_name, cb.as_ref().unchecked_ref())?;
            cb.forget();
        }

        let comp_flag_start = is_composing.clone();
        let comp_start = Closure::wrap(Box::new(move |_e: CompositionEvent| {
            comp_flag_start.set(true);
        }) as Box<dyn FnMut(CompositionEvent)>);
        hidden_input.add_event_listener_with_callback(
            "compositionstart",
            comp_start.as_ref().unchecked_ref(),
        )?;
        comp_start.forget();

        let h_comp_upd = host.clone();
        let sched_comp_upd = scheduler.clone();
        let comp_upd = Closure::wrap(Box::new(move |e: CompositionEvent| {
            let ev = dom_composition_to_ime(false, e.data().unwrap_or_default());
            if h_comp_upd.borrow_mut().handle_ui_event(&ev) {
                sched_comp_upd.request_frame();
            }
        }) as Box<dyn FnMut(CompositionEvent)>);
        hidden_input.add_event_listener_with_callback(
            "compositionupdate",
            comp_upd.as_ref().unchecked_ref(),
        )?;
        comp_upd.forget();

        let h_comp_end = host.clone();
        let comp_flag_end = is_composing.clone();
        let hidden_comp = hidden_input.clone();
        let last_val_comp = last_hidden_val.clone();
        let sched_comp_end = scheduler.clone();
        let comp_end = Closure::wrap(Box::new(move |e: CompositionEvent| {
            comp_flag_end.set(false);
            let ev = dom_composition_to_ime(true, e.data().unwrap_or_default());
            let mut guard = h_comp_end.borrow_mut();
            let changed = guard.handle_ui_event(&ev);
            let synced = guard.hidden_ime_state().value;
            hidden_comp.set_value(&synced);
            *last_val_comp.borrow_mut() = synced;
            if changed {
                sched_comp_end.request_frame();
            }
        }) as Box<dyn FnMut(CompositionEvent)>);
        hidden_input.add_event_listener_with_callback(
            "compositionend",
            comp_end.as_ref().unchecked_ref(),
        )?;
        comp_end.forget();

        // Mobile virtual keyboard DOM `input` event bridge on `<input>`
        let h_input = host;
        let hidden_inp = hidden_input.clone();
        let comp_flag_inp = is_composing;
        let last_val_inp = last_hidden_val;
        let sched_inp = scheduler;
        let on_input = Closure::wrap(Box::new(move |e: web_sys::Event| {
            if comp_flag_inp.get() {
                return;
            }
            let Ok(ie) = e.dyn_into::<DomInputEvent>() else {
                return;
            };
            if ie.is_composing() {
                return;
            }
            let current = hidden_inp.value();
            let prev = last_val_inp.borrow().clone();
            let input_type = ie.input_type();
            let data = ie.data();
            let events = dom_input_to_events(&prev, &current, &input_type, data.as_deref());
            *last_val_inp.borrow_mut() = current;
            let mut guard = h_input.borrow_mut();
            let mut changed = false;
            for ev in events {
                changed |= guard.handle_ui_event(&ev);
            }
            if changed {
                sched_inp.request_frame();
            }
        }) as Box<dyn FnMut(web_sys::Event)>);
        hidden_input
            .add_event_listener_with_callback("input", on_input.as_ref().unchecked_ref())?;
        on_input.forget();

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use craftui_core::Length;
    use craftui_layout::LayoutStyle;
    use craftui_reactive::reset_runtime;

    #[test]
    fn web_host_dpr_ime_overlay_hash_router_dom_input_and_2d_fallback_render() {
        reset_runtime();
        assert_eq!(BrowserRouteSync::route_to_hash("/settings"), "#/settings");
        assert_eq!(BrowserRouteSync::hash_to_route("#/settings"), "/settings");

        let mut host = WebAppHost::new(UiDispatcher::new(), 400.0, 300.0, 2.0);
        let router = Router::new(host.scope, "/".to_string());
        BrowserRouteSync::connect_string_router(host.scope, router);
        assert_eq!(host.take_pending_hash_update().as_deref(), Some("#/"));
        assert!(host.apply_url_hash("#/dashboard"));
        assert_eq!(router.current(), "/dashboard");
        router.push("/profile".to_string());
        assert_eq!(
            host.take_pending_hash_update().as_deref(),
            Some("#/profile")
        );

        let (pw, ph) = host.resize_css(400.0, 300.0, 2.0);
        assert_eq!((pw, ph), (800, 600));

        let sig = host.scope.signal("hello".to_string());
        let inp = host.tree.text_input(host.scope, "", "Search", Some(sig));
        host.tree.set_layout(
            inp,
            LayoutStyle::row().with_size(Length::Dp(240.0), Length::Dp(40.0)),
        );
        host.tree.set_root(inp);
        let _ = host.step_frame(0.016);

        let c = host.tree.rect(inp).center();
        let down = dom_pointer_event(
            1,
            PointerPhase::Down,
            c.x,
            c.y,
            0.0,
            0.0,
            0,
            false,
            Modifiers::default(),
            0,
        );
        let up = dom_pointer_event(
            1,
            PointerPhase::Up,
            c.x,
            c.y,
            0.0,
            0.0,
            0,
            false,
            Modifiers::default(),
            20,
        );
        host.tree.handle_event(&down);
        host.tree.handle_event(&up);

        let ime = host.hidden_ime_state();
        assert!(ime.visible);
        assert_eq!(ime.value, "hello");

        // Mobile virtual keyboard DOM `input` insert & backspace events
        for ev in dom_input_to_events("hello", "hello!", "insertText", Some("!")) {
            host.tree.handle_event(&ev);
        }
        assert_eq!(sig.get(), "hello!");
        for ev in dom_input_to_events("hello!", "hello", "deleteContentBackward", None) {
            host.tree.handle_event(&ev);
        }
        assert_eq!(sig.get(), "hello");

        let (rw, rh, rgba) = host.render_rgba_2d();
        assert_eq!((rw, rh), (800, 600));
        assert_eq!(rgba.len(), 800 * 600 * 4);

        // Demand-driven Web frame check: idle focused TextInput does not animate continuously,
        // and only triggers redraw at blink interval boundaries.
        host.tree.reset_caret_blink(1000);
        let (anim0, _painted0) = host.step_frame_with_redraw(0.016, 1000);
        assert!(!anim0);
        let (anim1, painted1) = host.step_frame_with_redraw(0.016, 1100);
        assert!(!anim1 && !painted1);
        let (anim2, painted2) = host.step_frame_with_redraw(0.016, 1600);
        assert!(!anim2 && painted2);

        let ky = dom_key_to_craft(
            "y",
            true,
            false,
            Modifiers {
                ctrl: true,
                ..Default::default()
            },
        );
        assert_eq!(ky.code, KeyCode::KeyY);
    }
}
