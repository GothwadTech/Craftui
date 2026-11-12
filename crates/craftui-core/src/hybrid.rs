//! Tauri 2.0-style Hybrid Web + CUI runtime bridge, IPC command router, and Tailwind/HTML utility parser.

use crate::{BoxShadow, Color, CornerRadii, Insets, Length};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// Injected JavaScript runtime shim providing `window.craftui` and standard `window.__TAURI__` interoperability.
pub const CRAFTUI_TAURI_IPC_SCRIPT: &str = r#"
(function() {
    if (window.craftui) return;
    var _callbacks = {};
    var _listeners = {};

    window.craftui = {
        invoke: function(cmd, args) {
            return new Promise(function(resolve, reject) {
                var id = "req_" + Math.random().toString(36).substring(2, 11);
                _callbacks[id] = { resolve: resolve, reject: reject };
                var payload = JSON.stringify({ id: id, cmd: cmd, args: args || {} });
                if (window.__CRAFTUI_POST__) {
                    window.__CRAFTUI_POST__(payload);
                } else if (window.craftui_native && window.craftui_native.postMessage) {
                    window.craftui_native.postMessage(payload);
                } else if (window.webkit && window.webkit.messageHandlers && window.webkit.messageHandlers.craftui) {
                    window.webkit.messageHandlers.craftui.postMessage(payload);
                }
            });
        },
        listen: function(event, callback) {
            if (!_listeners[event]) _listeners[event] = [];
            _listeners[event].push(callback);
            return function unlisten() {
                var idx = _listeners[event].indexOf(callback);
                if (idx >= 0) _listeners[event].splice(idx, 1);
            };
        },
        emit: function(event, payload) {
            var id = "emit_" + Math.random().toString(36).substring(2, 11);
            var msg = JSON.stringify({ id: id, cmd: "__emit__", args: { event: event, payload: payload } });
            if (window.__CRAFTUI_POST__) window.__CRAFTUI_POST__(msg);
        },
        __on_response: function(resJson) {
            var res = JSON.parse(resJson);
            var cb = _callbacks[res.id];
            if (cb) {
                delete _callbacks[res.id];
                if (res.error) cb.reject(new Error(res.error));
                else cb.resolve(res.result);
            }
        },
        __on_event: function(event, payloadJson) {
            var list = _listeners[event] || [];
            var payload = JSON.parse(payloadJson);
            for (var i = 0; i < list.length; i++) {
                try { list[i](payload); } catch (e) { console.error(e); }
            }
        }
    };

    // Official Tauri 2.0 compatibility layer so unmodified Tauri frontends run inside CraftUI
    window.__TAURI__ = {
        core: {
            invoke: window.craftui.invoke
        },
        event: {
            listen: window.craftui.listen,
            emit: window.craftui.emit
        }
    };
})();
"#;

/// Standard JSON RPC request dispatched from JavaScript (`window.craftui.invoke`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IpcMessage {
    pub id: String,
    pub cmd: String,
    #[serde(default)]
    pub args: serde_json::Value,
}

/// JSON RPC response returned from Rust back to JavaScript.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IpcResponse {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

type IpcHandlerFn =
    Box<dyn Fn(serde_json::Value) -> Result<serde_json::Value, String> + Send + Sync>;
type IpcListenerFn = Box<dyn Fn(serde_json::Value) + Send + Sync>;

/// Bidirectional IPC Command & Event Router compatible with Tauri 2.0.
#[derive(Default)]
pub struct IpcRouter {
    handlers: HashMap<String, IpcHandlerFn>,
    listeners: HashMap<String, Vec<IpcListenerFn>>,
    emitted_events: Arc<Mutex<Vec<(String, serde_json::Value)>>>,
}

impl IpcRouter {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a command handler that can be invoked from JS (`window.craftui.invoke("cmd", args)`).
    pub fn register<F>(&mut self, cmd: impl Into<String>, handler: F)
    where
        F: Fn(serde_json::Value) -> Result<serde_json::Value, String> + Send + Sync + 'static,
    {
        self.handlers.insert(cmd.into(), Box::new(handler));
    }

    /// Register a Rust listener for events emitted from JavaScript (`window.craftui.emit("event", payload)`).
    pub fn listen<F>(&mut self, event: impl Into<String>, callback: F)
    where
        F: Fn(serde_json::Value) + Send + Sync + 'static,
    {
        self.listeners
            .entry(event.into())
            .or_default()
            .push(Box::new(callback));
    }

    /// Emit an event from Rust to JavaScript listeners (`window.craftui.listen("event", ...)`).
    pub fn emit(&self, event: impl Into<String>, payload: serde_json::Value) {
        let ev = event.into();
        if let Ok(mut list) = self.emitted_events.lock() {
            list.push((ev.clone(), payload.clone()));
        }
        if let Some(callbacks) = self.listeners.get(&ev) {
            for cb in callbacks {
                cb(payload.clone());
            }
        }
    }

    /// Drain all queued events to be sent to the Web frontend.
    pub fn drain_emitted(&self) -> Vec<(String, serde_json::Value)> {
        if let Ok(mut list) = self.emitted_events.lock() {
            std::mem::take(&mut *list)
        } else {
            Vec::new()
        }
    }

    /// Handle raw incoming JSON request from the Web frontend and return a JSON response string.
    pub fn handle_invoke_json(&self, raw_json: &str) -> String {
        let msg: IpcMessage = match serde_json::from_str(raw_json) {
            Ok(m) => m,
            Err(e) => {
                let err_res = IpcResponse {
                    id: "error".into(),
                    result: None,
                    error: Some(format!("Invalid IPC JSON: {}", e)),
                };
                return serde_json::to_string(&err_res).unwrap_or_default();
            }
        };

        if msg.cmd == "__emit__" {
            if let Some(ev) = msg.args.get("event").and_then(|v| v.as_str()) {
                let payload = msg
                    .args
                    .get("payload")
                    .cloned()
                    .unwrap_or(serde_json::Value::Null);
                self.emit(ev, payload);
            }
            let res = IpcResponse {
                id: msg.id,
                result: Some(serde_json::json!({ "status": "ok" })),
                error: None,
            };
            return serde_json::to_string(&res).unwrap_or_default();
        }

        let resp = match self.handlers.get(&msg.cmd) {
            Some(handler) => match handler(msg.args) {
                Ok(val) => IpcResponse {
                    id: msg.id,
                    result: Some(val),
                    error: None,
                },
                Err(err) => IpcResponse {
                    id: msg.id,
                    result: None,
                    error: Some(err),
                },
            },
            None => IpcResponse {
                id: msg.id,
                result: None,
                error: Some(format!("Unknown CraftUI IPC command: '{}'", msg.cmd)),
            },
        };

        serde_json::to_string(&resp).unwrap_or_default()
    }
}

/// Parsed Tailwind CSS utility classes into native CraftUI layout and styling properties.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct TailwindTokens {
    pub is_row: bool,
    pub is_column: bool,
    pub align_center: bool,
    pub align_start: bool,
    pub align_end: bool,
    pub justify_between: bool,
    pub justify_center: bool,
    pub justify_around: bool,
    pub justify_evenly: bool,
    pub justify_start: bool,
    pub justify_end: bool,
    pub padding: Option<Insets>,
    pub gap: Option<f32>,
    pub background: Option<Color>,
    pub text_color: Option<Color>,
    pub font_size: Option<f32>,
    pub font_weight: Option<f32>,
    pub radius: Option<CornerRadii>,
    pub width: Option<Length>,
    pub height: Option<Length>,
    pub shadow: Option<BoxShadow>,
    pub border_width: Option<f32>,
    pub border_color: Option<Color>,
}

impl TailwindTokens {
    /// Parse a space-separated Tailwind / utility class string (e.g. `"flex flex-col gap-4 p-4 bg-slate-900 rounded-xl"`).
    pub fn parse(classes: &str) -> Self {
        let mut tokens = Self::default();
        for class in classes.split_whitespace() {
            // 1. Arbitrary Hex Colors e.g. bg-[#0E2938], text-[#00BCD4], border-[#1E293B]
            if let Some(hex) = class.strip_prefix("bg-[").and_then(|s| s.strip_suffix(']')) {
                if let Some(c) = Color::from_hex(hex) {
                    tokens.background = Some(c);
                    continue;
                }
            }
            if let Some(hex) = class
                .strip_prefix("border-[")
                .and_then(|s| s.strip_suffix(']'))
            {
                if let Some(c) = Color::from_hex(hex) {
                    tokens.border_color = Some(c);
                    if tokens.border_width.is_none() {
                        tokens.border_width = Some(1.0);
                    }
                    continue;
                }
            }
            if let Some(hex) = class
                .strip_prefix("text-[")
                .and_then(|s| s.strip_suffix(']'))
            {
                if let Some(c) = Color::from_hex(hex) {
                    tokens.text_color = Some(c);
                    continue;
                }
            }

            match class {
                "flex" => {}
                "flex-col" => tokens.is_column = true,
                "flex-row" => tokens.is_row = true,
                "items-center" => tokens.align_center = true,
                "items-start" => tokens.align_start = true,
                "items-end" => tokens.align_end = true,
                "justify-between" => tokens.justify_between = true,
                "justify-center" => tokens.justify_center = true,
                "justify-around" => tokens.justify_around = true,
                "justify-evenly" => tokens.justify_evenly = true,
                "justify-start" => tokens.justify_start = true,
                "justify-end" => tokens.justify_end = true,
                "w-full" => tokens.width = Some(Length::Pct(100.0)),
                "h-full" => tokens.height = Some(Length::Pct(100.0)),
                "w-2" => tokens.width = Some(Length::Dp(8.0)),
                "w-3" => tokens.width = Some(Length::Dp(12.0)),
                "w-4" => tokens.width = Some(Length::Dp(16.0)),
                "w-5" => tokens.width = Some(Length::Dp(20.0)),
                "w-6" => tokens.width = Some(Length::Dp(24.0)),
                "w-7" => tokens.width = Some(Length::Dp(28.0)),
                "w-8" => tokens.width = Some(Length::Dp(32.0)),
                "w-10" => tokens.width = Some(Length::Dp(40.0)),
                "w-11" => tokens.width = Some(Length::Dp(44.0)),
                "w-12" => tokens.width = Some(Length::Dp(48.0)),
                "w-14" => tokens.width = Some(Length::Dp(56.0)),
                "w-16" => tokens.width = Some(Length::Dp(66.0)),
                "w-20" => tokens.width = Some(Length::Dp(80.0)),
                "w-24" => tokens.width = Some(Length::Dp(96.0)),
                "h-1" => tokens.height = Some(Length::Dp(4.0)),
                "h-1.5" => tokens.height = Some(Length::Dp(6.0)),
                "h-2" => tokens.height = Some(Length::Dp(8.0)),
                "h-3" => tokens.height = Some(Length::Dp(12.0)),
                "h-4" => tokens.height = Some(Length::Dp(16.0)),
                "h-5" => tokens.height = Some(Length::Dp(20.0)),
                "h-6" => tokens.height = Some(Length::Dp(24.0)),
                "h-8" => tokens.height = Some(Length::Dp(32.0)),
                "h-10" => tokens.height = Some(Length::Dp(40.0)),
                "h-12" => tokens.height = Some(Length::Dp(48.0)),
                "h-14" => tokens.height = Some(Length::Dp(56.0)),
                "h-16" => tokens.height = Some(Length::Dp(64.0)),
                "h-20" => tokens.height = Some(Length::Dp(80.0)),
                "h-21" => tokens.height = Some(Length::Dp(86.0)),
                "h-22" => tokens.height = Some(Length::Dp(88.0)),
                "h-24" => tokens.height = Some(Length::Dp(96.0)),
                "rounded-none" => tokens.radius = Some(CornerRadii::ZERO),
                "rounded-sm" => tokens.radius = Some(CornerRadii::all(4.0)),
                "rounded" | "rounded-md" => tokens.radius = Some(CornerRadii::all(8.0)),
                "rounded-lg" => tokens.radius = Some(CornerRadii::all(12.0)),
                "rounded-xl" => tokens.radius = Some(CornerRadii::all(16.0)),
                "rounded-2xl" => tokens.radius = Some(CornerRadii::all(24.0)),
                "rounded-full" => tokens.radius = Some(CornerRadii::all(9999.0)),
                "border" => tokens.border_width = Some(1.0),
                "border-2" => tokens.border_width = Some(2.0),
                "border-slate-800" => tokens.border_color = Some(Color::rgb(30, 41, 59)),
                "border-slate-700" => tokens.border_color = Some(Color::rgb(51, 65, 85)),
                "border-slate-200" => tokens.border_color = Some(Color::rgb(226, 232, 240)),
                "font-normal" => tokens.font_weight = Some(400.0),
                "font-medium" => tokens.font_weight = Some(500.0),
                "font-semibold" => tokens.font_weight = Some(600.0),
                "font-bold" => tokens.font_weight = Some(700.0),
                "text-xs" => tokens.font_size = Some(12.0),
                "text-sm" => tokens.font_size = Some(14.0),
                "text-base" => tokens.font_size = Some(16.0),
                "text-lg" => tokens.font_size = Some(18.0),
                "text-xl" => tokens.font_size = Some(20.0),
                "text-2xl" => tokens.font_size = Some(24.0),
                "text-3xl" => tokens.font_size = Some(30.0),
                "shadow-sm" => {
                    tokens.shadow = Some(BoxShadow::drop(
                        0.0,
                        1.0,
                        3.0,
                        0.0,
                        Color::rgba(0, 0, 0, 30),
                    ))
                }
                "shadow" | "shadow-md" => {
                    tokens.shadow = Some(BoxShadow::drop(
                        0.0,
                        2.0,
                        6.0,
                        0.0,
                        Color::rgba(0, 0, 0, 50),
                    ))
                }
                "shadow-lg" => {
                    tokens.shadow = Some(BoxShadow::drop(
                        0.0,
                        4.0,
                        12.0,
                        0.0,
                        Color::rgba(0, 0, 0, 70),
                    ))
                }
                "shadow-xl" => {
                    tokens.shadow = Some(BoxShadow::drop(
                        0.0,
                        8.0,
                        20.0,
                        0.0,
                        Color::rgba(0, 0, 0, 90),
                    ))
                }
                "teal-glow" => {
                    tokens.shadow = Some(BoxShadow::drop(
                        0.0,
                        4.0,
                        20.0,
                        0.0,
                        Color::rgba(0, 188, 212, 90),
                    ))
                }
                // Padding
                "p-1" => tokens.padding = Some(Insets::all(4.0)),
                "p-2" => tokens.padding = Some(Insets::all(8.0)),
                "p-3" => tokens.padding = Some(Insets::all(12.0)),
                "p-4" => tokens.padding = Some(Insets::all(16.0)),
                "p-5" => tokens.padding = Some(Insets::all(20.0)),
                "p-6" => tokens.padding = Some(Insets::all(24.0)),
                "p-8" => tokens.padding = Some(Insets::all(32.0)),
                "px-2" => tokens.padding = Some(Insets::new(0.0, 8.0, 0.0, 8.0)),
                "px-3" => tokens.padding = Some(Insets::new(0.0, 12.0, 0.0, 12.0)),
                "px-4" => tokens.padding = Some(Insets::new(0.0, 16.0, 0.0, 16.0)),
                "px-8" => tokens.padding = Some(Insets::new(0.0, 32.0, 0.0, 32.0)),
                "py-1" => tokens.padding = Some(Insets::new(4.0, 0.0, 4.0, 0.0)),
                "py-2" => tokens.padding = Some(Insets::new(8.0, 0.0, 8.0, 0.0)),
                "py-3" => tokens.padding = Some(Insets::new(12.0, 0.0, 12.0, 0.0)),
                "py-4" => tokens.padding = Some(Insets::new(16.0, 0.0, 16.0, 0.0)),
                "pt-1" => tokens.padding = Some(Insets::new(4.0, 0.0, 0.0, 0.0)),
                "pt-2" => tokens.padding = Some(Insets::new(8.0, 0.0, 0.0, 0.0)),
                "pt-3" => tokens.padding = Some(Insets::new(12.0, 0.0, 0.0, 0.0)),
                "pb-1" => tokens.padding = Some(Insets::new(0.0, 0.0, 4.0, 0.0)),
                "pb-2" => tokens.padding = Some(Insets::new(0.0, 0.0, 8.0, 0.0)),
                "pb-3" => tokens.padding = Some(Insets::new(0.0, 0.0, 12.0, 0.0)),
                "pb-24" => tokens.padding = Some(Insets::new(0.0, 0.0, 96.0, 0.0)),
                // Gap
                "gap-0.5" => tokens.gap = Some(2.0),
                "gap-1" => tokens.gap = Some(4.0),
                "gap-1.5" => tokens.gap = Some(6.0),
                "gap-2" => tokens.gap = Some(8.0),
                "gap-3" => tokens.gap = Some(12.0),
                "gap-4" => tokens.gap = Some(16.0),
                "gap-5" => tokens.gap = Some(20.0),
                "gap-6" => tokens.gap = Some(24.0),
                "gap-8" => tokens.gap = Some(32.0),
                // Colors: Slate & Gray palettes
                "bg-slate-900" => tokens.background = Some(Color::rgb(15, 23, 42)),
                "bg-slate-800" => tokens.background = Some(Color::rgb(30, 41, 59)),
                "bg-slate-700" => tokens.background = Some(Color::rgb(51, 65, 85)),
                "bg-zinc-900" => tokens.background = Some(Color::rgb(24, 24, 27)),
                "bg-zinc-800" => tokens.background = Some(Color::rgb(39, 39, 42)),
                "bg-blue-600" => tokens.background = Some(Color::rgb(37, 99, 235)),
                "bg-blue-500" => tokens.background = Some(Color::rgb(59, 130, 246)),
                "bg-teal-500" => tokens.background = Some(Color::rgb(20, 184, 166)),
                "bg-teal-600" => tokens.background = Some(Color::rgb(13, 148, 136)),
                "bg-emerald-600" => tokens.background = Some(Color::rgb(5, 150, 105)),
                "bg-red-600" => tokens.background = Some(Color::rgb(220, 38, 38)),
                "bg-white" => tokens.background = Some(Color::WHITE),
                "bg-black" => tokens.background = Some(Color::BLACK),
                "text-white" => tokens.text_color = Some(Color::WHITE),
                "text-black" => tokens.text_color = Some(Color::BLACK),
                "text-slate-400" => tokens.text_color = Some(Color::rgb(148, 163, 184)),
                "text-slate-300" => tokens.text_color = Some(Color::rgb(203, 213, 225)),
                "text-slate-200" => tokens.text_color = Some(Color::rgb(226, 232, 240)),
                "text-blue-400" => tokens.text_color = Some(Color::rgb(96, 165, 250)),
                "text-teal-400" => tokens.text_color = Some(Color::rgb(45, 212, 191)),
                "text-cyan-400" => tokens.text_color = Some(Color::rgb(34, 211, 238)),
                "text-amber-400" => tokens.text_color = Some(Color::rgb(251, 191, 36)),
                "text-red-400" => tokens.text_color = Some(Color::rgb(248, 113, 113)),
                _ => {}
            }
        }
        tokens
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ipc_router_register_and_invoke() {
        let mut router = IpcRouter::new();
        router.register("calculate_sum", |args| {
            let a = args.get("a").and_then(|v| v.as_i64()).unwrap_or(0);
            let b = args.get("b").and_then(|v| v.as_i64()).unwrap_or(0);
            Ok(serde_json::json!({ "sum": a + b }))
        });

        let raw_req = serde_json::json!({
            "id": "req-42",
            "cmd": "calculate_sum",
            "args": { "a": 15, "b": 27 }
        })
        .to_string();

        let raw_resp = router.handle_invoke_json(&raw_req);
        let resp: IpcResponse = serde_json::from_str(&raw_resp).expect("valid response JSON");

        assert_eq!(resp.id, "req-42");
        assert!(resp.error.is_none());
        assert_eq!(resp.result.unwrap()["sum"], 42);
    }

    #[test]
    fn test_ipc_router_unknown_command() {
        let router = IpcRouter::new();
        let raw_req = serde_json::json!({
            "id": "req-99",
            "cmd": "non_existent_command",
            "args": {}
        })
        .to_string();

        let raw_resp = router.handle_invoke_json(&raw_req);
        let resp: IpcResponse = serde_json::from_str(&raw_resp).expect("valid response JSON");

        assert_eq!(resp.id, "req-99");
        assert!(resp.result.is_none());
        assert!(resp.error.unwrap().contains("Unknown CraftUI IPC command"));
    }

    #[test]
    fn test_ipc_router_event_system() {
        use std::sync::Arc;
        use std::sync::atomic::{AtomicU32, Ordering};

        let mut router = IpcRouter::new();
        let progress = Arc::new(AtomicU32::new(0));
        let progress_clone = Arc::clone(&progress);

        router.listen("download_progress", move |payload| {
            if let Some(pct) = payload.get("pct").and_then(|v| v.as_u64()) {
                progress_clone.store(pct as u32, Ordering::SeqCst);
            }
        });

        router.emit(
            "download_progress",
            serde_json::json!({ "file": "bundle.zip", "pct": 75 }),
        );

        assert_eq!(progress.load(Ordering::SeqCst), 75);
    }

    #[test]
    fn test_tauri_script_injection_has_required_bindings() {
        assert!(CRAFTUI_TAURI_IPC_SCRIPT.contains("window.__TAURI__"));
        assert!(CRAFTUI_TAURI_IPC_SCRIPT.contains("window.craftui"));
        assert!(CRAFTUI_TAURI_IPC_SCRIPT.contains("invoke:"));
        assert!(CRAFTUI_TAURI_IPC_SCRIPT.contains("listen:"));
        assert!(CRAFTUI_TAURI_IPC_SCRIPT.contains("emit:"));
    }

    #[test]
    fn test_tailwind_tokens_parse() {
        let classes =
            "flex flex-col gap-4 p-4 bg-slate-900 rounded-xl shadow-md text-white font-bold w-full";
        let tokens = TailwindTokens::parse(classes);

        assert!(tokens.is_column);
        assert!(!tokens.is_row);
        assert_eq!(tokens.padding, Some(Insets::all(16.0)));
        assert_eq!(tokens.gap, Some(16.0));
        assert_eq!(tokens.background, Some(Color::rgb(15, 23, 42)));
        assert_eq!(tokens.radius, Some(CornerRadii::all(16.0)));
        assert!(tokens.shadow.is_some());
        assert_eq!(tokens.text_color, Some(Color::WHITE));
        assert_eq!(tokens.font_weight, Some(700.0));
        assert_eq!(tokens.width, Some(Length::Pct(100.0)));
    }
}
