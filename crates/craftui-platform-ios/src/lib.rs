//! iOS platform adapter (`UIKit` lifecycle C-ABI entrypoints, `CAMetalLayer` + `CADisplayLink`
//! frame pacing, Dynamic Island / Home Indicator SafeArea, `UITextInput` IME, and `CVPixelBuffer` external textures).

use craftui_core::{
    AppLifecycleEvent, BatteryState, DeviceCapabilitiesBackend, DeviceInfo, HapticStyle,
    HapticsBackend, ImeEvent, Insets, KeyCode, KeyEvent, Modifiers, NetworkStatus, NodeId,
    Permission, PermissionBackend, PermissionOutcome, PermissionStatus, PlatformServices,
    PlatformViewId, Point, PointerButton, PointerEvent, PointerId, PointerPhase,
    SecureStorageBackend, ShareBackend, SharePayload, Size, TextureId, UiDispatcher, UiEvent,
    WindowMetrics,
};
use craftui_reactive::Scope;
use craftui_render::{PaintScene, TextureHandle, TextureRegistry};
use craftui_widgets::{WidgetKind, WidgetTree};
use std::collections::{HashMap, VecDeque};
use std::ffi::{CString, c_char, c_void};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

pub fn permission_status_to_code(status: PermissionStatus) -> u8 {
    match status {
        PermissionStatus::NotDetermined => 0,
        PermissionStatus::Granted => 1,
        PermissionStatus::Denied => 2,
        PermissionStatus::PermanentlyDenied => 3,
        PermissionStatus::Restricted => 4,
        PermissionStatus::Limited => 5,
        PermissionStatus::Provisional => 6,
        PermissionStatus::Unsupported => 7,
    }
}

pub fn permission_status_from_code(code: u8) -> PermissionStatus {
    match code {
        0 => PermissionStatus::NotDetermined,
        1 => PermissionStatus::Granted,
        2 => PermissionStatus::Denied,
        3 => PermissionStatus::PermanentlyDenied,
        4 => PermissionStatus::Restricted,
        5 => PermissionStatus::Limited,
        6 => PermissionStatus::Provisional,
        _ => PermissionStatus::Unsupported,
    }
}

pub fn lifecycle_event_from_code(code: u8) -> Option<AppLifecycleEvent> {
    match code {
        0 => Some(AppLifecycleEvent::Resumed),
        1 => Some(AppLifecycleEvent::Paused),
        2 => Some(AppLifecycleEvent::Inactive),
        3 => Some(AppLifecycleEvent::Detached),
        4 => Some(AppLifecycleEvent::MemoryWarning),
        _ => None,
    }
}

pub fn network_status_from_code(code: u8) -> NetworkStatus {
    match code {
        1 => NetworkStatus::Offline,
        2 => NetworkStatus::Wifi,
        3 => NetworkStatus::Cellular,
        4 => NetworkStatus::Ethernet,
        _ => NetworkStatus::Unknown,
    }
}

fn json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                use std::fmt::Write as _;
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Platform command emitted toward Swift `CraftUIViewController` / `CraftUIPermissions.swift`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IosPlatformCommand {
    RequestPermission { request_id: u64, key: String },
    OpenAppSettings,
    TriggerHaptic(HapticStyle),
    Share(SharePayload),
    SyncSecureKey { key: String, value: String },
    DeleteSecureKey { key: String },
}

impl IosPlatformCommand {
    pub fn to_json(&self) -> String {
        match self {
            Self::RequestPermission { request_id, key } => format!(
                "{{\"type\":\"request_permission\",\"request_id\":{request_id},\"key\":{}}}",
                json_escape(key)
            ),
            Self::OpenAppSettings => "{\"type\":\"open_app_settings\"}".to_string(),
            Self::TriggerHaptic(style) => {
                let name = match style {
                    HapticStyle::Light => "light",
                    HapticStyle::Medium => "medium",
                    HapticStyle::Heavy => "heavy",
                    HapticStyle::Selection => "selection",
                    HapticStyle::Success => "success",
                    HapticStyle::Warning => "warning",
                    HapticStyle::Error => "error",
                };
                format!("{{\"type\":\"haptic\",\"style\":\"{name}\"}}")
            }
            Self::Share(payload) => {
                let title = payload
                    .title
                    .as_deref()
                    .map(json_escape)
                    .unwrap_or_else(|| "null".to_string());
                let text = payload
                    .text
                    .as_deref()
                    .map(json_escape)
                    .unwrap_or_else(|| "null".to_string());
                let url = payload
                    .url
                    .as_deref()
                    .map(json_escape)
                    .unwrap_or_else(|| "null".to_string());
                let files: Vec<String> = payload
                    .files
                    .iter()
                    .map(|p| json_escape(&p.to_string_lossy()))
                    .collect();
                format!(
                    "{{\"type\":\"share\",\"title\":{title},\"text\":{text},\"url\":{url},\"files\":[{}]}}",
                    files.join(",")
                )
            }
            Self::SyncSecureKey { key, value } => format!(
                "{{\"type\":\"sync_secret\",\"key\":{},\"value\":{}}}",
                json_escape(key),
                json_escape(value)
            ),
            Self::DeleteSecureKey { key } => {
                format!(
                    "{{\"type\":\"delete_secret\",\"key\":{}}}",
                    json_escape(key)
                )
            }
        }
    }
}

/// Shared iOS capability, permission, haptics, keychain, and telemetry bridge state.
#[derive(Debug)]
pub struct IosBridgeState {
    next_request_id: AtomicU64,
    statuses: Mutex<HashMap<Permission, PermissionStatus>>,
    rationales: Mutex<HashMap<Permission, bool>>,
    commands: Mutex<VecDeque<IosPlatformCommand>>,
    pending_perms: Mutex<HashMap<u64, Option<PermissionOutcome>>>,
    perm_cv: Condvar,
    request_timeout: Mutex<Duration>,
    secure_entries: Mutex<HashMap<String, String>>,
    device_info: Mutex<DeviceInfo>,
    network_status: Mutex<NetworkStatus>,
    battery_state: Mutex<BatteryState>,
    lifecycle_history: Mutex<Vec<AppLifecycleEvent>>,
}

impl Default for IosBridgeState {
    fn default() -> Self {
        Self::new()
    }
}

impl IosBridgeState {
    pub fn new() -> Self {
        Self {
            next_request_id: AtomicU64::new(1),
            statuses: Mutex::new(HashMap::new()),
            rationales: Mutex::new(HashMap::new()),
            commands: Mutex::new(VecDeque::new()),
            pending_perms: Mutex::new(HashMap::new()),
            perm_cv: Condvar::new(),
            request_timeout: Mutex::new(Duration::from_millis(if cfg!(target_os = "ios") {
                30_000
            } else {
                2_000
            })),
            secure_entries: Mutex::new(HashMap::new()),
            device_info: Mutex::new(DeviceInfo {
                os: "iOS".to_string(),
                model: "iPhone".to_string(),
                os_version: String::new(),
                sdk_int: None,
                app_version: env!("CARGO_PKG_VERSION").to_string(),
            }),
            network_status: Mutex::new(NetworkStatus::Unknown),
            battery_state: Mutex::new(BatteryState::default()),
            lifecycle_history: Mutex::new(Vec::new()),
        }
    }

    pub fn set_request_timeout(&self, timeout: Duration) {
        if let Ok(mut guard) = self.request_timeout.lock() {
            *guard = timeout;
        }
    }

    fn timeout(&self) -> Duration {
        self.request_timeout
            .lock()
            .map(|g| *g)
            .unwrap_or(Duration::from_millis(250))
    }

    fn push_command(&self, cmd: IosPlatformCommand) {
        if let Ok(mut guard) = self.commands.lock() {
            guard.push_back(cmd);
        }
    }

    pub fn set_permission_status(
        &self,
        perm: Permission,
        status: PermissionStatus,
        rationale: bool,
    ) {
        if let Ok(mut guard) = self.statuses.lock() {
            guard.insert(perm.clone(), status);
        }
        if let Ok(mut guard) = self.rationales.lock() {
            guard.insert(perm, rationale);
        }
    }

    pub fn deliver_permission_result(
        &self,
        request_id: u64,
        key: &str,
        status: PermissionStatus,
        rationale: bool,
    ) {
        let perm = Permission::from_key(key);
        self.set_permission_status(perm.clone(), status, rationale);
        if let Ok(mut pending) = self.pending_perms.lock()
            && let Some(slot) = pending.get_mut(&request_id)
        {
            *slot = Some(PermissionOutcome {
                permission: perm,
                status,
                should_show_rationale: rationale,
            });
        }
        self.perm_cv.notify_all();
    }

    pub fn lifecycle_history(&self) -> Vec<AppLifecycleEvent> {
        self.lifecycle_history
            .lock()
            .map(|g| g.clone())
            .unwrap_or_default()
    }
}

/// iOS runtime permission backend delegating to UIKit / AVFoundation / CoreLocation / UNUserNotificationCenter.
#[derive(Clone)]
pub struct IosPermissionBackend {
    state: Arc<IosBridgeState>,
}

impl IosPermissionBackend {
    pub fn new(state: Arc<IosBridgeState>) -> Self {
        Self { state }
    }
}

impl PermissionBackend for IosPermissionBackend {
    fn check(&self, permission: &Permission) -> PermissionStatus {
        self.state
            .statuses
            .lock()
            .ok()
            .and_then(|g| g.get(permission).copied())
            .unwrap_or(PermissionStatus::NotDetermined)
    }

    fn request(&self, permission: &Permission) -> PermissionStatus {
        let current = self.check(permission);
        if current.is_granted() {
            return current;
        }
        let request_id = self.state.next_request_id.fetch_add(1, Ordering::SeqCst);
        if let Ok(mut pending) = self.state.pending_perms.lock() {
            pending.insert(request_id, None);
        }
        self.state
            .push_command(IosPlatformCommand::RequestPermission {
                request_id,
                key: permission.key().into_owned(),
            });

        let timeout = self.state.timeout();
        if !timeout.is_zero()
            && let Ok(mut pending) = self.state.pending_perms.lock()
        {
            let deadline = Instant::now() + timeout;
            while pending.get(&request_id).is_some_and(|slot| slot.is_none()) {
                let remaining = deadline.saturating_duration_since(Instant::now());
                if remaining.is_zero() {
                    break;
                }
                let Ok((next_guard, wait_res)) =
                    self.state.perm_cv.wait_timeout(pending, remaining)
                else {
                    return current;
                };
                pending = next_guard;
                if wait_res.timed_out() {
                    break;
                }
            }
            if let Some(Some(outcome)) = pending.remove(&request_id) {
                return outcome.status;
            }
        }
        if let Ok(mut pending) = self.state.pending_perms.lock() {
            pending.remove(&request_id);
        }
        self.check(permission)
    }

    fn should_show_rationale(&self, permission: &Permission) -> bool {
        self.state
            .rationales
            .lock()
            .ok()
            .and_then(|g| g.get(permission).copied())
            .unwrap_or(false)
    }

    fn open_app_settings(&self) -> bool {
        self.state.push_command(IosPlatformCommand::OpenAppSettings);
        true
    }
}

/// iOS `UIImpactFeedbackGenerator` / `UINotificationFeedbackGenerator` bridge.
#[derive(Clone)]
pub struct IosHapticsBackend {
    state: Arc<IosBridgeState>,
}

impl IosHapticsBackend {
    pub fn new(state: Arc<IosBridgeState>) -> Self {
        Self { state }
    }
}

impl HapticsBackend for IosHapticsBackend {
    fn trigger(&self, style: HapticStyle) -> bool {
        self.state
            .push_command(IosPlatformCommand::TriggerHaptic(style));
        true
    }
}

/// iOS `UIActivityViewController` native share sheet bridge.
#[derive(Clone)]
pub struct IosShareBackend {
    state: Arc<IosBridgeState>,
}

impl IosShareBackend {
    pub fn new(state: Arc<IosBridgeState>) -> Self {
        Self { state }
    }
}

impl ShareBackend for IosShareBackend {
    fn share(&self, payload: &SharePayload) -> bool {
        self.state
            .push_command(IosPlatformCommand::Share(payload.clone()));
        true
    }
}

/// iOS Keychain synchronized secure storage backend.
#[derive(Clone)]
pub struct IosSecureStorage {
    state: Arc<IosBridgeState>,
}

impl IosSecureStorage {
    pub fn new(state: Arc<IosBridgeState>) -> Self {
        Self { state }
    }
}

impl SecureStorageBackend for IosSecureStorage {
    fn get_secret(&self, key: &str) -> Option<String> {
        self.state
            .secure_entries
            .lock()
            .ok()
            .and_then(|g| g.get(key).cloned())
    }

    fn set_secret(&self, key: &str, value: &str) -> bool {
        let Ok(mut guard) = self.state.secure_entries.lock() else {
            return false;
        };
        guard.insert(key.to_string(), value.to_string());
        self.state.push_command(IosPlatformCommand::SyncSecureKey {
            key: key.to_string(),
            value: value.to_string(),
        });
        true
    }

    fn delete_secret(&self, key: &str) -> bool {
        let Ok(mut guard) = self.state.secure_entries.lock() else {
            return false;
        };
        let existed = guard.remove(key).is_some();
        self.state
            .push_command(IosPlatformCommand::DeleteSecureKey {
                key: key.to_string(),
            });
        existed
    }
}

/// iOS `UIDevice` / `NWPathMonitor` telemetry backend.
#[derive(Clone)]
pub struct IosDeviceCapabilities {
    state: Arc<IosBridgeState>,
}

impl IosDeviceCapabilities {
    pub fn new(state: Arc<IosBridgeState>) -> Self {
        Self { state }
    }
}

impl DeviceCapabilitiesBackend for IosDeviceCapabilities {
    fn device_info(&self) -> DeviceInfo {
        self.state
            .device_info
            .lock()
            .map(|g| g.clone())
            .unwrap_or_default()
    }

    fn network_status(&self) -> NetworkStatus {
        self.state
            .network_status
            .lock()
            .map(|g| *g)
            .unwrap_or_default()
    }

    fn battery_state(&self) -> BatteryState {
        self.state
            .battery_state
            .lock()
            .map(|g| *g)
            .unwrap_or_default()
    }
}

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
    pub bridge: Arc<IosBridgeState>,
    last_first_responder: bool,
    last_synced_text: Option<String>,
    needs_redraw: bool,
    pub last_ime_commands: Vec<IosImeCommand>,
    last_error: Option<String>,
    last_error_cstr: Option<CString>,
    #[cfg(all(target_os = "ios", any(feature = "gpu-sdf", feature = "gpu-vello")))]
    gpu: Option<craftui_render::GpuSurfacePipeline>,
}

impl IosAppHost {
    pub fn new(dispatcher: UiDispatcher, width_pt: f32, height_pt: f32, scale_factor: f32) -> Self {
        let scope = Scope::root();
        let mut tree = WidgetTree::new(scope);
        let bridge = Arc::new(IosBridgeState::new());
        tree.set_platform_services(
            PlatformServices::in_memory()
                .with_permissions(IosPermissionBackend::new(bridge.clone()))
                .with_haptics(IosHapticsBackend::new(bridge.clone()))
                .with_share(IosShareBackend::new(bridge.clone()))
                .with_secure_storage(IosSecureStorage::new(bridge.clone()))
                .with_device_capabilities(IosDeviceCapabilities::new(bridge.clone())),
        );
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
            bridge,
            last_first_responder: false,
            last_synced_text: None,
            needs_redraw: true,
            last_ime_commands: Vec::new(),
            last_error: None,
            last_error_cstr: None,
            #[cfg(all(target_os = "ios", any(feature = "gpu-sdf", feature = "gpu-vello")))]
            gpu: None,
        }
    }

    pub fn bridge(&self) -> Arc<IosBridgeState> {
        self.bridge.clone()
    }

    pub fn set_permission_wait_timeout(&self, timeout: Duration) {
        self.bridge.set_request_timeout(timeout);
    }

    pub fn last_error(&self) -> Option<&str> {
        self.last_error.as_deref()
    }

    pub fn clear_error(&mut self) {
        self.last_error = None;
        self.last_error_cstr = None;
    }

    pub fn record_error(&mut self, msg: impl Into<String>) {
        let text = msg.into();
        let sanitized = text.replace('\0', "?");
        self.last_error_cstr = CString::new(sanitized).ok();
        self.last_error = Some(text);
    }

    /// Attach or replace the UIKit `CAMetalLayer` target, recording errors in [`Self::last_error`].
    ///
    /// # Safety
    /// On iOS, if `layer.ca_metal_layer` is non-null it must point to a valid retained `CAMetalLayer`
    /// that outlives the attached GPU pipeline.
    pub unsafe fn attach_metal(
        &mut self,
        layer: IosMetalLayerDescriptor,
    ) -> craftui_core::Result<()> {
        if layer.ca_metal_layer.is_null() {
            let err =
                craftui_core::CraftError::Platform("iOS CAMetalLayer pointer is null".to_string());
            self.record_error(err.to_string());
            return Err(err);
        }
        #[cfg(all(target_os = "ios", any(feature = "gpu-sdf", feature = "gpu-vello")))]
        {
            // SAFETY: forwarded from caller invariant that `layer.ca_metal_layer` outlives `self.gpu`.
            match unsafe { ios_metal::attach_metal_pipeline(self, layer) } {
                Ok(()) => {
                    self.clear_error();
                    Ok(())
                }
                Err(err) => {
                    let msg = format!("iOS Metal attach failed: {err}");
                    tracing::warn!("{msg}");
                    self.record_error(msg);
                    Err(err)
                }
            }
        }
        #[cfg(not(all(target_os = "ios", any(feature = "gpu-sdf", feature = "gpu-vello"))))]
        {
            let _ = layer;
            let err = craftui_core::CraftError::Platform(
                "iOS Metal pipeline requires target_os = \"ios\" with gpu-sdf or gpu-vello enabled"
                    .to_string(),
            );
            self.record_error(err.to_string());
            Err(err)
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
            if let Err(err) = gpu.render_frame(
                &self.scene,
                sf,
                bg,
                &self.textures,
                &mut self.tree.text_engine,
            ) {
                let msg = format!("iOS Metal render_frame failed: {err}");
                tracing::warn!("{msg}");
                self.record_error(msg);
            }
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
    if !layer.ca_metal_layer.is_null() {
        // SAFETY: `raw` was just allocated by `craftui_ios_app_create` and `layer.ca_metal_layer` invariant is upheld by caller.
        let host = unsafe { &mut *raw };
        let _ = unsafe { host.attach_metal(layer) };
    }
    raw
}

/// Attach or reattach a `CAMetalLayer` after creation. Returns `0` on success or `-1` on error
/// (inspect the message with [`craftui_ios_app_last_error`]).
///
/// # Safety
/// `app` must be a valid non-null [`IosAppHost`] pointer and `layer.ca_metal_layer` (on iOS) must be
/// a valid retained `CAMetalLayer` pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn craftui_ios_app_attach_metal(
    app: *mut IosAppHost,
    layer: IosMetalLayerDescriptor,
) -> i32 {
    let Some(host) = (unsafe { app.as_mut() }) else {
        return -1;
    };
    match unsafe { host.attach_metal(layer) } {
        Ok(()) => 0,
        Err(_) => -1,
    }
}

/// Returns a NUL-terminated UTF-8 pointer to the most recent error message, or NULL when clear.
///
/// # Safety
/// `app` must be null or a valid [`IosAppHost`] pointer. The returned pointer remains valid only
/// until the next call that mutates or destroys `app`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn craftui_ios_app_last_error(app: *const IosAppHost) -> *const c_char {
    let Some(host) = (unsafe { app.as_ref() }) else {
        return std::ptr::null();
    };
    host.last_error_cstr
        .as_ref()
        .map_or(std::ptr::null(), |s| s.as_ptr())
}

/// Clear the stored error message on `app`.
///
/// # Safety
/// `app` must be null or a valid [`IosAppHost`] pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn craftui_ios_app_clear_error(app: *mut IosAppHost) {
    if let Some(host) = unsafe { app.as_mut() } {
        host.clear_error();
    }
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

/// Set marked (IME preedit) UTF-8 text from UIKit `UITextInput::setMarkedText`.
///
/// # Safety
/// `app` must be a valid non-null [`IosAppHost`] pointer and if `utf8_len > 0`, `utf8_ptr` must
/// point to `utf8_len` valid UTF-8 bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn craftui_ios_app_set_marked_text(
    app: *mut IosAppHost,
    utf8_ptr: *const u8,
    utf8_len: usize,
    cursor_end: usize,
) -> bool {
    let Some(host) = (unsafe { app.as_mut() }) else {
        return false;
    };
    if utf8_ptr.is_null() || utf8_len == 0 {
        return host.push_marked_text("", 0);
    }
    // SAFETY: Caller guarantees `utf8_ptr..utf8_ptr+utf8_len` is valid for reading during this call.
    let slice = unsafe { std::slice::from_raw_parts(utf8_ptr, utf8_len) };
    let text = String::from_utf8_lossy(slice).into_owned();
    host.push_marked_text(&text, cursor_end)
}

/// Advance one `CADisplayLink` frame. Returns a bitmask:
/// - bit 0 (`0b001`): animating
/// - bit 1 (`0b010`): wants keyboard
/// - bit 2 (`0b100`): error recorded (inspect via [`craftui_ios_app_last_error`])
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
    let has_err = host.last_error().is_some();
    (u8::from(anim)) | (u8::from(wants_kb) << 1) | (u8::from(has_err) << 2)
}

unsafe fn read_utf8_key<'a>(ptr: *const u8, len: usize) -> Option<&'a str> {
    if ptr.is_null() || len == 0 {
        return None;
    }
    // SAFETY: Caller upholds `ptr..ptr+len` is valid for reading during this C-ABI call.
    let slice = unsafe { std::slice::from_raw_parts(ptr, len) };
    std::str::from_utf8(slice).ok()
}

/// Query cached permission status code (`0=NotDetermined..7=Unsupported`) by permission key (`"camera"`, `"microphone"`, etc.).
///
/// # Safety
/// `app` must be a valid non-null [`IosAppHost`] pointer and `key_ptr..key_ptr+key_len` must be valid UTF-8.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn craftui_ios_app_permission_status_from_key(
    app: *const IosAppHost,
    key_ptr: *const u8,
    key_len: usize,
) -> u8 {
    let Some(host) = (unsafe { app.as_ref() }) else {
        return permission_status_to_code(PermissionStatus::Unsupported);
    };
    let Some(key) = (unsafe { read_utf8_key(key_ptr, key_len) }) else {
        return permission_status_to_code(PermissionStatus::Unsupported);
    };
    let perm = Permission::from_key(key);
    permission_status_to_code(host.tree.services.check_permission(&perm))
}

/// Update cached OS permission authorization state pushed from UIKit / AVFoundation / CoreLocation.
///
/// # Safety
/// `app` must be a valid non-null [`IosAppHost`] pointer and `key_ptr..key_ptr+key_len` must be valid UTF-8.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn craftui_ios_app_set_permission_status(
    app: *mut IosAppHost,
    key_ptr: *const u8,
    key_len: usize,
    status_code: u8,
) {
    let Some(host) = (unsafe { app.as_mut() }) else {
        return;
    };
    let Some(key) = (unsafe { read_utf8_key(key_ptr, key_len) }) else {
        return;
    };
    host.bridge.set_permission_status(
        Permission::from_key(key),
        permission_status_from_code(status_code),
        false,
    );
    host.request_redraw();
}

/// Deliver an asynchronous permission request result from Swift back into Rust waiters and `UiDispatcher`.
///
/// # Safety
/// `app` must be a valid non-null [`IosAppHost`] pointer and `key_ptr..key_ptr+key_len` must be valid UTF-8.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn craftui_ios_app_deliver_permission_result(
    app: *mut IosAppHost,
    request_id: u64,
    key_ptr: *const u8,
    key_len: usize,
    status_code: u8,
    rationale: bool,
) {
    let Some(host) = (unsafe { app.as_mut() }) else {
        return;
    };
    let Some(key) = (unsafe { read_utf8_key(key_ptr, key_len) }) else {
        return;
    };
    host.bridge.deliver_permission_result(
        request_id,
        key,
        permission_status_from_code(status_code),
        rationale,
    );
    host.request_redraw();
}

/// Poll the next pending [`IosPlatformCommand`] serialized as UTF-8 JSON for Swift `CraftUIViewController`.
/// Returns:
/// - `0` when the command queue is empty
/// - `> 0` byte length written into `out_buf` (command is popped)
/// - `< 0` negated required byte length when `out_buf` is null or `out_cap` is insufficient (command stays queued)
///
/// # Safety
/// `app` must be a valid non-null [`IosAppHost`] pointer. If `out_buf` is non-null, `out_cap` bytes
/// starting at `out_buf` must be writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn craftui_ios_app_poll_command_json(
    app: *mut IosAppHost,
    out_buf: *mut u8,
    out_cap: usize,
) -> i32 {
    let Some(host) = (unsafe { app.as_mut() }) else {
        return 0;
    };
    let Ok(mut queue) = host.bridge.commands.lock() else {
        return 0;
    };
    let Some(front) = queue.front() else {
        return 0;
    };
    let json = front.to_json();
    let bytes = json.as_bytes();
    if out_buf.is_null() || out_cap < bytes.len() {
        return -(bytes.len() as i32);
    }
    // SAFETY: Caller guarantees `out_buf` points to at least `out_cap >= bytes.len()` writable bytes.
    unsafe {
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), out_buf, bytes.len());
    }
    queue.pop_front();
    bytes.len() as i32
}

/// Forward a UIKit `UIApplication` lifecycle transition (`0=Resumed, 1=Paused, 2=Inactive, 3=Detached, 4=MemoryWarning`).
///
/// # Safety
/// `app` must be a valid non-null [`IosAppHost`] pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn craftui_ios_app_push_lifecycle(app: *mut IosAppHost, event_code: u8) {
    let Some(host) = (unsafe { app.as_mut() }) else {
        return;
    };
    if let Some(ev) = lifecycle_event_from_code(event_code)
        && let Ok(mut hist) = host.bridge.lifecycle_history.lock()
    {
        hist.push(ev);
    }
}

/// Update iOS `NWPathMonitor` network classification (`0=Unknown, 1=Offline, 2=Wifi, 3=Cellular, 4=Ethernet`).
///
/// # Safety
/// `app` must be a valid non-null [`IosAppHost`] pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn craftui_ios_app_set_network_status(
    app: *mut IosAppHost,
    network_code: u8,
) {
    let Some(host) = (unsafe { app.as_mut() }) else {
        return;
    };
    if let Ok(mut guard) = host.bridge.network_status.lock() {
        *guard = network_status_from_code(network_code);
    }
}

/// Update `UIDevice` battery state (`level_pct < 0` indicates unknown level).
///
/// # Safety
/// `app` must be a valid non-null [`IosAppHost`] pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn craftui_ios_app_set_battery(
    app: *mut IosAppHost,
    level_pct: i16,
    charging: bool,
) {
    let Some(host) = (unsafe { app.as_mut() }) else {
        return;
    };
    let level_pct = if (0..=100).contains(&level_pct) {
        Some(level_pct as u8)
    } else {
        None
    };
    if let Ok(mut guard) = host.bridge.battery_state.lock() {
        *guard = BatteryState {
            level_pct,
            charging,
        };
    }
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
            assert!(craftui_ios_app_last_error(app).is_null());
            let marked = b"nihon";
            assert!(craftui_ios_app_set_marked_text(
                app,
                marked.as_ptr(),
                marked.len(),
                5
            ));
            let msg = b"hello";
            assert!(craftui_ios_app_insert_text(app, msg.as_ptr(), msg.len()));
            // Attaching a null Metal layer fails gracefully and exposes last_error + tick bit 2
            assert_eq!(craftui_ios_app_attach_metal(app, layer), -1);
            let err_ptr = craftui_ios_app_last_error(app);
            assert!(!err_ptr.is_null());
            let err_str = std::ffi::CStr::from_ptr(err_ptr).to_str().unwrap();
            assert!(err_str.contains("null"), "unexpected error: {err_str}");
            let err_flags = craftui_ios_app_tick(app, 0.016);
            assert_ne!(err_flags & 0b100, 0, "expected error bit in tick flags");
            craftui_ios_app_clear_error(app);
            assert!(craftui_ios_app_last_error(app).is_null());
            assert_eq!(craftui_ios_app_tick(app, 0.016) & 0b100, 0);
            // Tap Button below TextInput (y around 60)
            assert!(craftui_ios_app_touch(app, 2, 0, 40.0, 60.0, 50));
            assert!(craftui_ios_app_touch(app, 2, 2, 40.0, 60.0, 70));
            assert!(clicked.load(std::sync::atomic::Ordering::SeqCst));
            craftui_ios_app_destroy(app);
        }
    }

    #[test]
    fn ios_c_abi_permissions_commands_haptics_share_and_telemetry_work() {
        reset_runtime();
        let layer = IosMetalLayerDescriptor {
            ca_metal_layer: std::ptr::null_mut(),
            width_pt: 393.0,
            height_pt: 852.0,
            contents_scale: 3.0,
        };
        let app = unsafe { craftui_ios_app_launch(layer, None, std::ptr::null_mut()) };
        assert!(!app.is_null());

        unsafe {
            let cam_key = b"camera";
            assert_eq!(
                craftui_ios_app_permission_status_from_key(app, cam_key.as_ptr(), cam_key.len()),
                0
            );
            craftui_ios_app_set_permission_status(app, cam_key.as_ptr(), cam_key.len(), 1);
            assert_eq!(
                craftui_ios_app_permission_status_from_key(app, cam_key.as_ptr(), cam_key.len()),
                1
            );

            craftui_ios_app_push_lifecycle(app, 0);
            craftui_ios_app_push_lifecycle(app, 4);
            craftui_ios_app_set_network_status(app, 2);
            craftui_ios_app_set_battery(app, 91, true);

            let host = &mut *app;
            assert_eq!(host.tree.services.network_status(), NetworkStatus::Wifi);
            assert_eq!(
                host.tree.services.battery_state(),
                BatteryState {
                    level_pct: Some(91),
                    charging: true,
                }
            );
            assert_eq!(
                host.bridge.lifecycle_history(),
                vec![AppLifecycleEvent::Resumed, AppLifecycleEvent::MemoryWarning]
            );

            assert!(host.tree.services.trigger_haptic(HapticStyle::Success));
            assert!(
                host.tree
                    .services
                    .share(&SharePayload::url("https://craftui.dev").with_title("CraftUI"))
            );
            assert!(host.tree.services.open_app_settings());
            assert!(host.tree.services.set_secret("api_key", "ios-token"));
            assert_eq!(
                host.tree.services.get_secret("api_key").as_deref(),
                Some("ios-token")
            );
            assert!(host.tree.services.delete_secret("api_key"));

            // Poll first command with undersized buffer -> negative required size, not popped
            let needed = craftui_ios_app_poll_command_json(app, std::ptr::null_mut(), 0);
            assert!(needed < 0);
            let mut buf = vec![0u8; (-needed) as usize];
            let written = craftui_ios_app_poll_command_json(app, buf.as_mut_ptr(), buf.len());
            assert_eq!(written, -needed);
            let cmd0 = std::str::from_utf8(&buf).unwrap();
            assert!(cmd0.contains("\"haptic\"") && cmd0.contains("success"));

            // Drain remaining queued commands
            let mut cmds = Vec::new();
            let mut large_buf = vec![0u8; 512];
            loop {
                let n =
                    craftui_ios_app_poll_command_json(app, large_buf.as_mut_ptr(), large_buf.len());
                if n == 0 {
                    break;
                }
                assert!(n > 0);
                cmds.push(String::from_utf8_lossy(&large_buf[..n as usize]).into_owned());
            }
            assert!(cmds.iter().any(|s| s.contains("\"share\"")));
            assert!(cmds.iter().any(|s| s.contains("open_app_settings")));
            assert!(cmds.iter().any(|s| s.contains("sync_secret")));
            assert!(cmds.iter().any(|s| s.contains("delete_secret")));

            // Async permission request unblocked via C-ABI deliver call
            let disp = host.dispatcher.clone();
            let result_slot = Arc::new(Mutex::new(None));
            let rs_c = result_slot.clone();
            host.tree.services.request_permission_async(
                Permission::Biometrics,
                disp.clone(),
                move |st| {
                    *rs_c.lock().unwrap() = Some(st);
                },
            );

            let mut bio_req_id = None;
            for _ in 0..100 {
                let n =
                    craftui_ios_app_poll_command_json(app, large_buf.as_mut_ptr(), large_buf.len());
                if n > 0 {
                    let s = std::str::from_utf8(&large_buf[..n as usize]).unwrap();
                    if s.contains("biometrics")
                        && let Some(pos) = s.find("\"request_id\":")
                    {
                        let rest = &s[pos + "\"request_id\":".len()..];
                        let digits: String =
                            rest.chars().take_while(|c| c.is_ascii_digit()).collect();
                        bio_req_id = digits.parse::<u64>().ok();
                    }
                }
                if bio_req_id.is_some() {
                    break;
                }
                std::thread::sleep(Duration::from_millis(2));
            }
            let rid = bio_req_id.expect("request_permission biometrics emitted");
            let bio_key = b"biometrics";
            craftui_ios_app_deliver_permission_result(
                app,
                rid,
                bio_key.as_ptr(),
                bio_key.len(),
                1,
                false,
            );
            for _ in 0..100 {
                let _ = disp.drain();
                if result_slot.lock().unwrap().is_some() {
                    break;
                }
                std::thread::sleep(Duration::from_millis(2));
            }
            assert_eq!(
                *result_slot.lock().unwrap(),
                Some(PermissionStatus::Granted)
            );
            craftui_ios_app_destroy(app);
        }
    }
}
