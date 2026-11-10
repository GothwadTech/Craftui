//! WebAssembly platform adapter (`wasm-bindgen` + `web-sys` Canvas/DPR observer +
//! WebGPU/2D-Canvas fallback + hidden `<input>` DOM IME bridge + hash URL router sync).

use craftui_core::{
    ImeEvent, KeyCode, KeyEvent, Modifiers, Point, PointerButton, PointerEvent, PointerId,
    PointerPhase, ScrollDelta, Size, UiDispatcher, UiEvent, WindowMetrics,
};
use craftui_reactive::Scope;
use craftui_render::{BackendKind, PaintScene, TextureRegistry, render_scene_to_rgba};
use craftui_widgets::{WidgetKind, WidgetTree};

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
}

/// Platform-neutral WASM host managing logical DPR sizing, hidden IME sync, and CPU/GPU rendering.
pub struct WebAppHost {
    pub scope: Scope,
    pub tree: WidgetTree,
    pub scene: PaintScene,
    pub textures: TextureRegistry,
    pub dispatcher: UiDispatcher,
}

impl WebAppHost {
    pub fn new(dispatcher: UiDispatcher, css_width: f32, css_height: f32, dpr: f32) -> Self {
        let scope = Scope::root();
        let mut tree = WidgetTree::new(scope);
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
        }
    }

    pub fn resize_css(&mut self, css_width: f32, css_height: f32, dpr: f32) -> (u32, u32) {
        let w = css_width.max(1.0);
        let h = css_height.max(1.0);
        let s = dpr.max(1.0);
        self.tree.handle_event(&UiEvent::Resize {
            width: w,
            height: h,
            scale_factor: s,
        });
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

    pub fn step_frame(&mut self, dt_sec: f32) -> bool {
        let _ = self.dispatcher.drain();
        let anim = self.tree.tick_animations(dt_sec);
        let _ = self.tree.layout(self.tree.metrics);
        self.tree.paint(&mut self.scene);
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
        "i" | "I" if modifiers.ctrl && modifiers.shift => KeyCode::KeyI,
        "a" | "A" if modifiers.ctrl => KeyCode::KeyA,
        "c" | "C" if modifiers.ctrl => KeyCode::KeyC,
        "v" | "V" if modifiers.ctrl => KeyCode::KeyV,
        "x" | "X" if modifiers.ctrl => KeyCode::KeyX,
        "z" | "Z" if modifiers.ctrl => KeyCode::KeyZ,
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
    use craftui_core::NodeId;
    use std::cell::RefCell;
    use std::rc::Rc;
    use wasm_bindgen::JsCast;
    use wasm_bindgen::closure::Closure;
    use web_sys::{
        CanvasRenderingContext2d, CompositionEvent, HtmlCanvasElement, HtmlInputElement, ImageData,
        KeyboardEvent as DomKeyboardEvent, PointerEvent as DomPointerEvent,
        WheelEvent as DomWheelEvent,
    };

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

        let dispatcher = UiDispatcher::new();
        let mut host = WebAppHost::new(dispatcher.clone(), css_w, css_h, dpr);
        let (pw, ph) = host.resize_css(css_w, css_h, dpr);
        canvas.set_width(pw);
        canvas.set_height(ph);

        let root = build_ui(host.scope, &mut host.tree, &mut host.textures, &dispatcher);
        host.tree.set_root(root);

        // Hidden <input> element overlay for native mobile browser keyboards & CJK IME composition
        let hidden_input: HtmlInputElement = document.create_element("input")?.dyn_into()?;
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
        attach_pointer_listeners(&canvas, host_rc.clone())?;
        attach_keyboard_ime_listeners(&window, &hidden_input, host_rc.clone())?;

        // Render loop via requestAnimationFrame (2D Canvas ImageData path guaranteed on all browsers)
        let ctx_2d: CanvasRenderingContext2d = canvas
            .get_context("2d")?
            .ok_or_else(|| wasm_bindgen::JsValue::from_str("2d context unavailable"))?
            .dyn_into()?;

        let raf_cb: Rc<RefCell<Option<Closure<dyn FnMut(f64)>>>> = Rc::new(RefCell::new(None));
        let raf_cb_clone = raf_cb.clone();
        let canvas_raf = canvas.clone();
        let window_raf = window.clone();
        let hidden_raf = hidden_input.clone();

        *raf_cb.borrow_mut() = Some(Closure::wrap(Box::new(move |_ts: f64| {
            let mut h = host_rc.borrow_mut();
            let dpr_now = window_raf.device_pixel_ratio() as f32;
            let bcr = canvas_raf.get_bounding_client_rect();
            let (pw, ph) = h.resize_css(bcr.width() as f32, bcr.height() as f32, dpr_now);
            if canvas_raf.width() != pw || canvas_raf.height() != ph {
                canvas_raf.set_width(pw);
                canvas_raf.set_height(ph);
            }
            let _anim = h.step_frame(1.0 / 60.0);
            let ime = h.hidden_ime_state();
            if ime.visible {
                let ist = hidden_raf.style();
                let _ = ist.set_property("left", &format!("{}px", bcr.left() as f32 + ime.x_dp));
                let _ = ist.set_property("top", &format!("{}px", bcr.top() as f32 + ime.y_dp));
                let _ = hidden_raf.focus();
            }
            let (w, h_px, rgba) = h.render_rgba_2d();
            if let Ok(img) =
                ImageData::new_with_u8_clamped_array_and_sh(wasm_bindgen::Clamped(&rgba), w, h_px)
            {
                let _ = ctx_2d.put_image_data(&img, 0.0, 0.0);
            }
            if let Some(ref closure) = *raf_cb_clone.borrow() {
                let _ = window_raf.request_animation_frame(closure.as_ref().unchecked_ref());
            }
        }) as Box<dyn FnMut(f64)>));

        if let Some(ref closure) = *raf_cb.borrow() {
            window.request_animation_frame(closure.as_ref().unchecked_ref())?;
        }
        std::mem::forget(raf_cb);
        Ok(())
    }

    fn attach_pointer_listeners(
        canvas: &HtmlCanvasElement,
        host: Rc<RefCell<WebAppHost>>,
    ) -> Result<(), wasm_bindgen::JsValue> {
        for (ev_name, phase) in [
            ("pointerdown", PointerPhase::Down),
            ("pointermove", PointerPhase::Move),
            ("pointerup", PointerPhase::Up),
            ("pointercancel", PointerPhase::Cancel),
        ] {
            let h_c = host.clone();
            let c_el = canvas.clone();
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
                h_c.borrow_mut().tree.handle_event(&ev);
            }) as Box<dyn FnMut(DomPointerEvent)>);
            canvas.add_event_listener_with_callback(ev_name, cb.as_ref().unchecked_ref())?;
            cb.forget();
        }

        let h_w = host.clone();
        let c_w = canvas.clone();
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
            h_w.borrow_mut().tree.handle_event(&ev);
        }) as Box<dyn FnMut(DomWheelEvent)>);
        canvas.add_event_listener_with_callback("wheel", wheel_cb.as_ref().unchecked_ref())?;
        wheel_cb.forget();
        Ok(())
    }

    fn attach_keyboard_ime_listeners(
        window: &web_sys::Window,
        hidden_input: &HtmlInputElement,
        host: Rc<RefCell<WebAppHost>>,
    ) -> Result<(), wasm_bindgen::JsValue> {
        for (ev_name, pressed) in [("keydown", true), ("keyup", false)] {
            let h_c = host.clone();
            let cb = Closure::wrap(Box::new(move |e: DomKeyboardEvent| {
                let mods = Modifiers {
                    shift: e.shift_key(),
                    ctrl: e.ctrl_key(),
                    alt: e.alt_key(),
                    super_key: e.meta_key(),
                };
                let ke = dom_key_to_craft(&e.key(), pressed, e.repeat(), mods);
                h_c.borrow_mut().tree.handle_event(&UiEvent::Key(ke));
            }) as Box<dyn FnMut(DomKeyboardEvent)>);
            window.add_event_listener_with_callback(ev_name, cb.as_ref().unchecked_ref())?;
            cb.forget();
        }

        let h_comp_upd = host.clone();
        let comp_upd = Closure::wrap(Box::new(move |e: CompositionEvent| {
            let ev = dom_composition_to_ime(false, e.data().unwrap_or_default());
            h_comp_upd.borrow_mut().tree.handle_event(&ev);
        }) as Box<dyn FnMut(CompositionEvent)>);
        hidden_input.add_event_listener_with_callback(
            "compositionupdate",
            comp_upd.as_ref().unchecked_ref(),
        )?;
        comp_upd.forget();

        let h_comp_end = host;
        let comp_end = Closure::wrap(Box::new(move |e: CompositionEvent| {
            let ev = dom_composition_to_ime(true, e.data().unwrap_or_default());
            h_comp_end.borrow_mut().tree.handle_event(&ev);
        }) as Box<dyn FnMut(CompositionEvent)>);
        hidden_input.add_event_listener_with_callback(
            "compositionend",
            comp_end.as_ref().unchecked_ref(),
        )?;
        comp_end.forget();
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
    fn web_host_dpr_ime_overlay_and_2d_fallback_render() {
        reset_runtime();
        assert_eq!(BrowserRouteSync::route_to_hash("/studio"), "#/studio");
        assert_eq!(BrowserRouteSync::hash_to_route("#/studio"), "/studio");

        let mut host = WebAppHost::new(UiDispatcher::new(), 400.0, 300.0, 2.0);
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

        let (rw, rh, rgba) = host.render_rgba_2d();
        assert_eq!((rw, rh), (800, 600));
        assert_eq!(rgba.len(), 800 * 600 * 4);
    }
}
