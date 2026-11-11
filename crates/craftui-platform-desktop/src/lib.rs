//! Desktop platform runner (`winit` + `wgpu`/`vello` + `accesskit_winit` + `notify` hot-reload)
//! supporting Linux (Wayland/X11), macOS, and Windows with multi-window support, OS clipboard/file
//! pickers without GPL C bindings, and timer scheduling.

use accesskit_winit::Adapter as AccessKitAdapter;
use craftui_core::{
    ClipboardBackend, Color, CraftError, FileDialogBackend, FileDialogRequest, ImeEvent, KeyCode,
    KeyEvent, Modifiers, NodeId, PlatformServices, Point, PointerButton, PointerEvent, PointerId,
    PointerPhase, Result, ScrollDelta, Size, UiDispatcher, UiEvent, WindowMetrics, monotonic_ms,
};
use craftui_reactive::Scope;
#[cfg(any(feature = "gpu-sdf", feature = "gpu-vello"))]
use craftui_render::GpuSurfacePipeline;
use craftui_render::{PaintScene, SoftwareRenderer, TextureRegistry};
#[cfg(feature = "hot-reload")]
use craftui_runtime::HotReloadSession;
use craftui_text::TextEngine;
use craftui_widgets::WidgetTree;
#[cfg(feature = "hot-reload")]
use notify::{Event as NotifyEvent, RecursiveMode, Watcher};
use std::cell::RefCell;
use std::collections::HashMap;
use std::io::Write;
use std::num::NonZeroU32;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::{
    ElementState, Ime as WinitIme, MouseButton, MouseScrollDelta, TouchPhase, WindowEvent,
};
use winit::event_loop::{
    ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy, OwnedDisplayHandle,
};
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

/// Pure permissive OS clipboard bridge (`wl-copy`/`xclip`/`xsel` on Linux, `pbcopy`/`pbpaste` on macOS,
/// PowerShell on Windows) with an in-memory fallback cache for headless CI.
#[derive(Clone, Default)]
pub struct DesktopClipboard {
    fallback: Arc<Mutex<String>>,
}

impl ClipboardBackend for DesktopClipboard {
    fn get_text(&mut self) -> Option<String> {
        if let Some(sys) = read_os_clipboard() {
            if let Ok(mut g) = self.fallback.lock() {
                *g = sys.clone();
            }
            return Some(sys);
        }
        self.fallback.lock().ok().map(|g| g.clone())
    }

    fn set_text(&mut self, text: &str) -> bool {
        if let Ok(mut g) = self.fallback.lock() {
            *g = text.to_string();
        }
        write_os_clipboard(text) || true
    }
}

fn read_os_clipboard() -> Option<String> {
    #[cfg(target_os = "macos")]
    {
        if let Ok(out) = Command::new("pbpaste").output()
            && out.status.success()
        {
            return String::from_utf8(out.stdout).ok();
        }
        return None;
    }
    #[cfg(target_os = "windows")]
    {
        if let Ok(out) = Command::new("powershell")
            .args(["-NoProfile", "-Command", "Get-Clipboard"])
            .output()
            && out.status.success()
        {
            let s = String::from_utf8_lossy(&out.stdout)
                .trim_end_matches(&['\r', '\n'][..])
                .to_string();
            return Some(s);
        }
        return None;
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        if std::env::var_os("WAYLAND_DISPLAY").is_some()
            && let Ok(out) = Command::new("wl-paste").arg("--no-newline").output()
            && out.status.success()
        {
            return String::from_utf8(out.stdout).ok();
        }
        if std::env::var_os("DISPLAY").is_some() {
            if let Ok(out) = Command::new("xclip")
                .args(["-o", "-selection", "clipboard"])
                .output()
                && out.status.success()
            {
                return String::from_utf8(out.stdout).ok();
            }
            if let Ok(out) = Command::new("xsel").args(["-ob"]).output()
                && out.status.success()
            {
                return String::from_utf8(out.stdout).ok();
            }
        }
        None
    }
}

fn write_os_clipboard(text: &str) -> bool {
    let run_stdin = |prog: &str, args: &[&str]| -> bool {
        let Ok(mut child) = Command::new(prog)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
        else {
            return false;
        };
        if let Some(mut stdin) = child.stdin.take() {
            let _ = stdin.write_all(text.as_bytes());
        }
        child.wait().is_ok_and(|s| s.success())
    };

    #[cfg(target_os = "macos")]
    {
        return run_stdin("pbcopy", &[]);
    }
    #[cfg(target_os = "windows")]
    {
        return run_stdin(
            "powershell",
            &[
                "-NoProfile",
                "-Command",
                "$input | Out-String -Stream | Set-Clipboard",
            ],
        );
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        if std::env::var_os("WAYLAND_DISPLAY").is_some() && run_stdin("wl-copy", &[]) {
            return true;
        }
        if std::env::var_os("DISPLAY").is_some() {
            if run_stdin("xclip", &["-i", "-selection", "clipboard"]) {
                return true;
            }
            if run_stdin("xsel", &["-ib"]) {
                return true;
            }
        }
        false
    }
}

/// Pure permissive native OS file open/save picker (`zenity`/`kdialog` on Linux, `osascript` on macOS,
/// PowerShell `Windows.Forms` on Windows) without GPL C dependencies.
#[derive(Debug, Default, Clone, Copy)]
pub struct DesktopFileDialog;

impl FileDialogBackend for DesktopFileDialog {
    fn open_files(&self, request: &FileDialogRequest) -> Vec<PathBuf> {
        #[cfg(target_os = "macos")]
        {
            return macos_pick_files(request, false);
        }
        #[cfg(target_os = "windows")]
        {
            return windows_pick_files(request);
        }
        #[cfg(all(unix, not(target_os = "macos")))]
        {
            linux_pick_files(request)
        }
    }

    fn save_file(&self, request: &FileDialogRequest) -> Option<PathBuf> {
        #[cfg(target_os = "macos")]
        {
            return macos_pick_files(request, true).into_iter().next();
        }
        #[cfg(target_os = "windows")]
        {
            return windows_save_file(request);
        }
        #[cfg(all(unix, not(target_os = "macos")))]
        {
            linux_save_file(request)
        }
    }
}

pub fn parse_zenity_output(raw: &str, separator: char) -> Vec<PathBuf> {
    raw.trim_end_matches(['\r', '\n'])
        .split(separator)
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .collect()
}

#[cfg(all(unix, not(target_os = "macos")))]
fn linux_pick_files(request: &FileDialogRequest) -> Vec<PathBuf> {
    let mut zargs = vec![
        "--file-selection".to_string(),
        format!("--title={}", request.title),
    ];
    if request.directory {
        zargs.push("--directory".into());
    }
    if request.multiple {
        zargs.push("--multiple".into());
        zargs.push("--separator=|".into());
    }
    for f in &request.filters {
        let globs: Vec<String> = f.extensions.iter().map(|e| format!("*.{e}")).collect();
        if !globs.is_empty() {
            zargs.push(format!("--file-filter={} | {}", f.name, globs.join(" ")));
        }
    }
    if let Ok(out) = Command::new("zenity").args(&zargs).output()
        && out.status.success()
    {
        let stdout = String::from_utf8_lossy(&out.stdout);
        return parse_zenity_output(&stdout, if request.multiple { '|' } else { '\n' });
    }

    let mut kargs = vec![if request.directory {
        "--getexistingdirectory".to_string()
    } else {
        "--getopenfilename".to_string()
    }];
    if let Some(dir) = &request.default_directory {
        kargs.push(dir.to_string_lossy().into_owned());
    }
    if request.multiple && !request.directory {
        kargs.push("--multiple".into());
    }
    if let Ok(out) = Command::new("kdialog").args(&kargs).output()
        && out.status.success()
    {
        let stdout = String::from_utf8_lossy(&out.stdout);
        return parse_zenity_output(&stdout, '\n');
    }
    Vec::new()
}

#[cfg(all(unix, not(target_os = "macos")))]
fn linux_save_file(request: &FileDialogRequest) -> Option<PathBuf> {
    let mut zargs = vec![
        "--file-selection".to_string(),
        "--save".to_string(),
        "--confirm-overwrite".to_string(),
        format!("--title={}", request.title),
    ];
    if let Some(name) = &request.default_name {
        let initial = request
            .default_directory
            .as_ref()
            .map(|d| d.join(name))
            .unwrap_or_else(|| PathBuf::from(name));
        zargs.push(format!("--filename={}", initial.display()));
    }
    if let Ok(out) = Command::new("zenity").args(&zargs).output()
        && out.status.success()
    {
        return parse_zenity_output(&String::from_utf8_lossy(&out.stdout), '\n')
            .into_iter()
            .next();
    }
    if let Ok(out) = Command::new("kdialog").arg("--getsavefilename").output()
        && out.status.success()
    {
        return parse_zenity_output(&String::from_utf8_lossy(&out.stdout), '\n')
            .into_iter()
            .next();
    }
    None
}

#[cfg(target_os = "macos")]
fn macos_pick_files(request: &FileDialogRequest, is_save: bool) -> Vec<PathBuf> {
    let verb = if is_save {
        "POSIX path of (choose file name)"
    } else if request.directory {
        "POSIX path of (choose folder)"
    } else {
        "POSIX path of (choose file)"
    };
    if let Ok(out) = Command::new("osascript").args(["-e", verb]).output()
        && out.status.success()
    {
        return parse_zenity_output(&String::from_utf8_lossy(&out.stdout), '\n');
    }
    Vec::new()
}

#[cfg(target_os = "windows")]
fn windows_pick_files(request: &FileDialogRequest) -> Vec<PathBuf> {
    let ps = if request.directory {
        "Add-Type -AssemblyName System.Windows.Forms; $d = New-Object System.Windows.Forms.FolderBrowserDialog; if($d.ShowDialog() -eq 'OK'){ Write-Output $d.SelectedPath }"
    } else {
        "Add-Type -AssemblyName System.Windows.Forms; $d = New-Object System.Windows.Forms.OpenFileDialog; if($d.ShowDialog() -eq 'OK'){ Write-Output $d.FileName }"
    };
    if let Ok(out) = Command::new("powershell")
        .args(["-NoProfile", "-STA", "-Command", ps])
        .output()
        && out.status.success()
    {
        return parse_zenity_output(&String::from_utf8_lossy(&out.stdout), '\n');
    }
    Vec::new()
}

#[cfg(target_os = "windows")]
fn windows_save_file(_request: &FileDialogRequest) -> Option<PathBuf> {
    let ps = "Add-Type -AssemblyName System.Windows.Forms; $d = New-Object System.Windows.Forms.SaveFileDialog; if($d.ShowDialog() -eq 'OK'){ Write-Output $d.FileName }";
    if let Ok(out) = Command::new("powershell")
        .args(["-NoProfile", "-STA", "-Command", ps])
        .output()
        && out.status.success()
    {
        return parse_zenity_output(&String::from_utf8_lossy(&out.stdout), '\n')
            .into_iter()
            .next();
    }
    None
}

pub fn default_desktop_services() -> PlatformServices {
    PlatformServices::new(DesktopClipboard::default(), DesktopFileDialog)
}

/// Custom event loop messages delivered from background worker threads, AccessKit, `.cui` watcher, or window opening.
#[derive(Debug)]
pub enum DesktopUserEvent {
    WakeUi,
    AccessKit(accesskit_winit::Event),
    #[cfg(feature = "hot-reload")]
    FileChanged,
    OpenWindow(u64),
}

impl From<accesskit_winit::Event> for DesktopUserEvent {
    fn from(ev: accesskit_winit::Event) -> Self {
        Self::AccessKit(ev)
    }
}

type BuildUiFn = Box<dyn FnOnce(Scope, &mut WidgetTree, &mut AppContext) -> NodeId>;

struct PendingWindowRequest {
    config: WindowConfig,
    builder: BuildUiFn,
}

thread_local! {
    static PENDING_WINDOW_QUEUE: RefCell<(u64, HashMap<u64, PendingWindowRequest>)> =
        RefCell::new((1, HashMap::new()));
}

/// Cloneable handle enabling button click handlers and effects to spawn additional desktop windows.
#[derive(Clone)]
pub struct WindowLauncher {
    proxy: EventLoopProxy<DesktopUserEvent>,
}

impl WindowLauncher {
    /// Request opening a new secondary desktop window with its own independent [`WidgetTree`] & GPU surface.
    pub fn open_window<F>(&self, config: WindowConfig, build_ui: F) -> Result<()>
    where
        F: FnOnce(Scope, &mut WidgetTree, &mut AppContext) -> NodeId + 'static,
    {
        let id = PENDING_WINDOW_QUEUE.with(|cell| {
            let mut guard = cell.borrow_mut();
            let id = guard.0;
            guard.0 = guard.0.wrapping_add(1).max(1);
            guard.1.insert(
                id,
                PendingWindowRequest {
                    config,
                    builder: Box::new(build_ui),
                },
            );
            id
        });
        self.proxy
            .send_event(DesktopUserEvent::OpenWindow(id))
            .map_err(|e| CraftError::Platform(format!("failed queuing open_window: {e}")))
    }
}

/// Platform services provided during app setup.
pub struct AppContext {
    pub dispatcher: UiDispatcher,
    pub textures: TextureRegistry,
    pub services: PlatformServices,
    launcher: WindowLauncher,
}

impl AppContext {
    pub fn window_launcher(&self) -> WindowLauncher {
        self.launcher.clone()
    }

    pub fn open_window<F>(&self, config: WindowConfig, build_ui: F) -> Result<()>
    where
        F: FnOnce(Scope, &mut WidgetTree, &mut AppContext) -> NodeId + 'static,
    {
        self.launcher.open_window(config, build_ui)
    }
}

/// Run a CraftUI application in a desktop `winit` window (supporting multi-window spawning via `AppContext::open_window`).
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

    let mut runner = DesktopRunner::new(config, proxy, dispatcher, Some(Box::new(build_ui)));
    event_loop
        .run_app(&mut runner)
        .map_err(|e| CraftError::Platform(format!("event loop exited with error: {e}")))
}

#[cfg(feature = "hot-reload")]
struct HotReloadSpec {
    path: PathBuf,
    component: Option<String>,
    session: HotReloadSession,
    _watcher: Option<notify::RecommendedWatcher>,
}

/// Determine whether a `notify` filesystem event should trigger `.cui` hot-reload.
///
/// Ignores read/open `Access` events emitted when CraftUI reads `.cui` files on Linux `inotify`.
#[cfg(feature = "hot-reload")]
pub fn should_trigger_hot_reload(event: &NotifyEvent) -> bool {
    let is_mutation = matches!(
        event.kind,
        notify::EventKind::Modify(_) | notify::EventKind::Create(_) | notify::EventKind::Remove(_)
    );
    if !is_mutation {
        return false;
    }
    event.paths.is_empty()
        || event
            .paths
            .iter()
            .any(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("cui")))
}

/// Run a live hot-reloading `.cui` preview window using `notify` + `craftui-runtime` (with full multi-file import support).
#[cfg(feature = "hot-reload")]
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
        if let Ok(ev) = res
            && should_trigger_hot_reload(&ev)
        {
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
    tree.set_platform_services(default_desktop_services());
    let mut session = HotReloadSession::new(root_scope);
    let _ = session.reload_file(&cui_path, component.as_deref(), &mut tree)?;

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
    );
    runner.set_primary_hot_reload(spec);
    event_loop
        .run_app(&mut runner)
        .map_err(|e| CraftError::Platform(format!("event loop exited with error: {e}")))
}

struct InitialWindowSetup {
    config: WindowConfig,
    root_scope: Scope,
    tree: WidgetTree,
    builder: Option<BuildUiFn>,
    #[cfg(feature = "hot-reload")]
    hot_reload: Option<HotReloadSpec>,
}

struct SoftbufferPresenter {
    surface: softbuffer::Surface<OwnedDisplayHandle, Arc<Window>>,
    cpu: SoftwareRenderer,
    width: u32,
    height: u32,
}

impl SoftbufferPresenter {
    fn new(
        context: &softbuffer::Context<OwnedDisplayHandle>,
        window: Arc<Window>,
        width: u32,
        height: u32,
    ) -> Result<Self> {
        let mut surface = softbuffer::Surface::new(context, window)
            .map_err(|e| CraftError::Render(format!("softbuffer surface creation failed: {e}")))?;
        let w = width.max(1);
        let h = height.max(1);
        if let (Some(nw), Some(nh)) = (NonZeroU32::new(w), NonZeroU32::new(h)) {
            surface
                .resize(nw, nh)
                .map_err(|e| CraftError::Render(format!("softbuffer resize failed: {e}")))?;
        }
        Ok(Self {
            surface,
            cpu: SoftwareRenderer::new(w, h),
            width: w,
            height: h,
        })
    }

    fn resize(&mut self, width: u32, height: u32) {
        let w = width.max(1);
        let h = height.max(1);
        if self.width == w && self.height == h {
            return;
        }
        if let (Some(nw), Some(nh)) = (NonZeroU32::new(w), NonZeroU32::new(h))
            && self.surface.resize(nw, nh).is_ok()
        {
            self.width = w;
            self.height = h;
        }
    }

    fn render_frame(
        &mut self,
        paint_scene: &PaintScene,
        scale_factor: f32,
        background: Color,
        textures: &TextureRegistry,
        text_engine: &mut TextEngine,
    ) -> Result<()> {
        let w = self.width.max(1);
        let h = self.height.max(1);
        let rgba = self.cpu.render_straight_rgba(
            paint_scene,
            w,
            h,
            scale_factor,
            background,
            textures,
            text_engine,
        );
        let mut buffer = self
            .surface
            .buffer_mut()
            .map_err(|e| CraftError::Render(format!("softbuffer buffer_mut failed: {e}")))?;
        pack_rgba8_to_softbuffer_0rgb(&rgba, &mut buffer);
        buffer
            .present()
            .map_err(|e| CraftError::Render(format!("softbuffer present failed: {e}")))?;
        Ok(())
    }
}

/// Convert straight RGBA8 pixels (`[r, g, b, a]`) into `softbuffer`'s `0x00RRGGBB` layout.
pub fn pack_rgba8_to_softbuffer_0rgb(rgba: &[u8], dst: &mut [u32]) {
    let (chunks, _) = rgba.as_chunks::<4>();
    for (pixel_out, chunk) in dst.iter_mut().zip(chunks.iter()) {
        *pixel_out = ((chunk[0] as u32) << 16) | ((chunk[1] as u32) << 8) | (chunk[2] as u32);
    }
}

enum DesktopPresenter {
    #[cfg(any(feature = "gpu-sdf", feature = "gpu-vello"))]
    Wgpu(Box<GpuSurfacePipeline>),
    Softbuffer(Box<SoftbufferPresenter>),
}

impl DesktopPresenter {
    fn resize(&mut self, width: u32, height: u32) {
        match self {
            #[cfg(any(feature = "gpu-sdf", feature = "gpu-vello"))]
            Self::Wgpu(gpu) => gpu.resize(width, height),
            Self::Softbuffer(sb) => sb.resize(width, height),
        }
    }

    fn render_frame(
        &mut self,
        paint_scene: &PaintScene,
        scale_factor: f32,
        background: Color,
        textures: &TextureRegistry,
        text_engine: &mut TextEngine,
    ) -> Result<()> {
        match self {
            #[cfg(any(feature = "gpu-sdf", feature = "gpu-vello"))]
            Self::Wgpu(gpu) => {
                gpu.render_frame(paint_scene, scale_factor, background, textures, text_engine)
            }
            Self::Softbuffer(sb) => {
                sb.render_frame(paint_scene, scale_factor, background, textures, text_engine)
            }
        }
    }

    fn label(&self) -> &'static str {
        match self {
            #[cfg(any(feature = "gpu-sdf", feature = "gpu-vello"))]
            Self::Wgpu(gpu) => gpu.active_backend_name(),
            Self::Softbuffer(_) => "softbuffer (CPU)",
        }
    }
}

struct DesktopWindowState {
    window: Arc<Window>,
    root_scope: Scope,
    is_primary: bool,
    tree: WidgetTree,
    textures: TextureRegistry,
    scene: PaintScene,
    a11y: Option<AccessKitAdapter>,
    a11y_active: bool,
    presenter: Option<DesktopPresenter>,
    #[cfg(feature = "hot-reload")]
    hot_reload: Option<HotReloadSpec>,
    cursor_pos: Point,
    modifiers: Modifiers,
    mouse_down: bool,
    last_focused_was_input: bool,
}

struct DesktopRunner {
    proxy: EventLoopProxy<DesktopUserEvent>,
    dispatcher: UiDispatcher,
    services: PlatformServices,
    softbuffer_ctx: Option<softbuffer::Context<OwnedDisplayHandle>>,
    pending_primary: Option<InitialWindowSetup>,
    windows: HashMap<WindowId, DesktopWindowState>,
    start_instant: Instant,
    last_frame_instant: Instant,
}

impl DesktopRunner {
    fn new(
        config: WindowConfig,
        proxy: EventLoopProxy<DesktopUserEvent>,
        dispatcher: UiDispatcher,
        builder: Option<BuildUiFn>,
    ) -> Self {
        let root_scope = Scope::root();
        let mut tree = WidgetTree::new(root_scope);
        let services = default_desktop_services();
        tree.set_platform_services(services.clone());
        Self::new_with_tree(config, proxy, dispatcher, root_scope, tree, builder)
    }

    fn new_with_tree(
        config: WindowConfig,
        proxy: EventLoopProxy<DesktopUserEvent>,
        dispatcher: UiDispatcher,
        root_scope: Scope,
        mut tree: WidgetTree,
        builder: Option<BuildUiFn>,
    ) -> Self {
        let services = default_desktop_services();
        tree.set_platform_services(services.clone());
        tree.metrics = WindowMetrics {
            logical_size: Size::new(config.width, config.height),
            scale_factor: 1.0,
            ..Default::default()
        };
        Self {
            proxy,
            dispatcher,
            services,
            softbuffer_ctx: None,
            pending_primary: Some(InitialWindowSetup {
                config,
                root_scope,
                tree,
                builder,
                #[cfg(feature = "hot-reload")]
                hot_reload: None,
            }),
            windows: HashMap::new(),
            start_instant: Instant::now(),
            last_frame_instant: Instant::now(),
        }
    }

    #[cfg(feature = "hot-reload")]
    fn set_primary_hot_reload(&mut self, spec: HotReloadSpec) {
        if let Some(init) = self.pending_primary.as_mut() {
            init.hot_reload = Some(spec);
        }
    }

    fn create_softbuffer_presenter(
        &mut self,
        display_handle: OwnedDisplayHandle,
        window: Arc<Window>,
        width: u32,
        height: u32,
    ) -> Result<SoftbufferPresenter> {
        if self.softbuffer_ctx.is_none() {
            let ctx = softbuffer::Context::new(display_handle).map_err(|e| {
                CraftError::Render(format!("softbuffer context creation failed: {e}"))
            })?;
            self.softbuffer_ctx = Some(ctx);
        }
        let ctx = self
            .softbuffer_ctx
            .as_ref()
            .ok_or_else(|| CraftError::Render("softbuffer context missing".into()))?;
        SoftbufferPresenter::new(ctx, window, width, height)
    }

    fn timestamp_ms(&self) -> u64 {
        self.start_instant.elapsed().as_millis() as u64
    }

    fn spawn_window(
        &mut self,
        event_loop: &ActiveEventLoop,
        setup: InitialWindowSetup,
        is_primary: bool,
    ) {
        let InitialWindowSetup {
            config,
            root_scope,
            mut tree,
            builder,
            #[cfg(feature = "hot-reload")]
            hot_reload,
        } = setup;
        let attrs = WindowAttributes::default()
            .with_title(config.title.clone())
            .with_inner_size(LogicalSize::new(config.width, config.height))
            .with_resizable(config.resizable)
            .with_visible(false);

        let window = match event_loop.create_window(attrs) {
            Ok(w) => Arc::new(w),
            Err(e) => {
                tracing::error!("create_window failed: {e}");
                eprintln!("[craftui] create_window failed: {e}");
                if self.windows.is_empty() {
                    event_loop.exit();
                }
                return;
            }
        };

        let a11y = AccessKitAdapter::with_event_loop_proxy(event_loop, &window, self.proxy.clone());
        window.set_visible(true);

        let scale = window.scale_factor().max(1.0) as f32;
        let phys = window.inner_size();
        let init_w = if phys.width > 0 {
            phys.width
        } else {
            (config.width.max(1.0) * scale).round().max(1.0) as u32
        };
        let init_h = if phys.height > 0 {
            phys.height
        } else {
            (config.height.max(1.0) * scale).round().max(1.0) as u32
        };

        tree.set_platform_services(self.services.clone());
        tree.metrics = WindowMetrics {
            logical_size: Size::new(init_w as f32 / scale, init_h as f32 / scale),
            scale_factor: scale,
            ..Default::default()
        };

        let force_cpu = config.force_cpu
            || std::env::var("CRAFTUI_FORCE_CPU")
                .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
                .unwrap_or(false)
            || !cfg!(any(feature = "gpu-sdf", feature = "gpu-vello"));

        let display_handle = event_loop.owned_display_handle();
        let presenter = if force_cpu {
            match self.create_softbuffer_presenter(
                display_handle.clone(),
                Arc::clone(&window),
                init_w,
                init_h,
            ) {
                Ok(sb) => Some(DesktopPresenter::Softbuffer(Box::new(sb))),
                Err(sb_err) => {
                    #[cfg(any(feature = "gpu-sdf", feature = "gpu-vello"))]
                    {
                        tracing::warn!("Softbuffer init failed ({sb_err}); trying wgpu CPU fallback");
                        match GpuSurfacePipeline::new_with_display(
                            || Arc::clone(&window),
                            || Box::new(display_handle.clone()),
                            init_w,
                            init_h,
                            true,
                        ) {
                            Ok(gpu) => Some(DesktopPresenter::Wgpu(Box::new(gpu))),
                            Err(gpu_err) => {
                                eprintln!(
                                    "[craftui] window '{}' failed to initialize any CPU presenter (softbuffer: {sb_err}; wgpu: {gpu_err})",
                                    config.title
                                );
                                None
                            }
                        }
                    }
                    #[cfg(not(any(feature = "gpu-sdf", feature = "gpu-vello")))]
                    {
                        eprintln!(
                            "[craftui] window '{}' failed to initialize softbuffer CPU presenter: {sb_err}",
                            config.title
                        );
                        None
                    }
                }
            }
        } else {
            #[cfg(any(feature = "gpu-sdf", feature = "gpu-vello"))]
            {
                match GpuSurfacePipeline::new_with_display(
                    || Arc::clone(&window),
                    || Box::new(display_handle.clone()),
                    init_w,
                    init_h,
                    false,
                ) {
                    Ok(gpu) => Some(DesktopPresenter::Wgpu(Box::new(gpu))),
                    Err(gpu_err) => {
                        tracing::warn!(
                            "GpuSurfacePipeline initialization failed ({gpu_err}); falling back to softbuffer CPU presenter"
                        );
                        eprintln!(
                            "[craftui] GPU presenter unavailable ({gpu_err}); falling back to CPU softbuffer"
                        );
                        match self.create_softbuffer_presenter(
                            display_handle,
                            Arc::clone(&window),
                            init_w,
                            init_h,
                        ) {
                            Ok(sb) => Some(DesktopPresenter::Softbuffer(Box::new(sb))),
                            Err(sb_err) => {
                                eprintln!(
                                    "[craftui] window '{}' failed to initialize softbuffer fallback: {sb_err}",
                                    config.title
                                );
                                None
                            }
                        }
                    }
                }
            }
            #[cfg(not(any(feature = "gpu-sdf", feature = "gpu-vello")))]
            {
                unreachable!("force_cpu is always true when GPU backends are disabled")
            }
        };

        if let Some(p) = presenter.as_ref() {
            tracing::info!(
                "CraftUI Desktop Window '{}' Renderer initialized ({})",
                config.title,
                p.label()
            );
        }

        let mut app_ctx = AppContext {
            dispatcher: self.dispatcher.clone(),
            textures: TextureRegistry::new(),
            services: self.services.clone(),
            launcher: WindowLauncher {
                proxy: self.proxy.clone(),
            },
        };

        if let Some(build_fn) = builder {
            let root = build_fn(root_scope, &mut tree, &mut app_ctx);
            tree.set_root(root);
        }

        let win_id = window.id();
        window.request_redraw();
        self.windows.insert(
            win_id,
            DesktopWindowState {
                window,
                root_scope,
                is_primary,
                tree,
                textures: app_ctx.textures,
                scene: PaintScene::new(),
                a11y: Some(a11y),
                a11y_active: false,
                presenter,
                #[cfg(feature = "hot-reload")]
                hot_reload,
                cursor_pos: Point::ZERO,
                modifiers: Modifiers::default(),
                mouse_down: false,
                last_focused_was_input: false,
            },
        );
    }

    fn render_and_present_window(&mut self, id: WindowId) {
        let now = Instant::now();
        let dt = (now - self.last_frame_instant)
            .as_secs_f32()
            .clamp(0.0001, 0.1);
        self.last_frame_instant = now;

        let _ = craftui_reactive::tick_timers();
        let _ = self.dispatcher.drain();

        let Some(state) = self.windows.get_mut(&id) else {
            return;
        };

        let frame_start = Instant::now();
        let _ = state.tree.tick_blink_timers(monotonic_ms());
        let anim_tick = state.tree.tick_animations(dt);

        let layout_start = Instant::now();
        let dirty_count = state.tree.layout(state.tree.metrics);
        let layout_ms = layout_start.elapsed().as_secs_f32() * 1000.0;
        let animating = anim_tick || state.tree.has_active_animations();

        state.tree.paint(&mut state.scene);

        let bg = state.tree.theme().background;
        let scale = state.tree.metrics.scale_factor;
        let tex_reg = &state.textures;
        if let Some(presenter) = state.presenter.as_mut()
            && let Err(e) = presenter.render_frame(
                &state.scene,
                scale,
                bg,
                tex_reg,
                &mut state.tree.text_engine,
            )
        {
            tracing::warn!("desktop frame render error: {e}");
            eprintln!("[craftui] frame render error: {e}");
        }

        if state.a11y_active
            && let Some(a11y) = state.a11y.as_mut()
            && let Some(update) = state.tree.build_a11y_update()
        {
            a11y.update_if_active(|| update);
        }

        let focused_is_input = state.tree.focused_is_text_input();
        if focused_is_input != state.last_focused_was_input {
            state.last_focused_was_input = focused_is_input;
            state.window.set_ime_allowed(focused_is_input);
        }

        let frame_ms = frame_start.elapsed().as_secs_f32() * 1000.0;
        let node_count = state.tree.nodes.len();
        state
            .tree
            .inspector
            .record_frame(frame_ms, layout_ms, node_count, dirty_count);

        if animating {
            state.window.request_redraw();
        }
    }

    #[cfg(feature = "hot-reload")]
    fn reload_cui_from_disk(&mut self) {
        for state in self.windows.values_mut() {
            let Some(spec) = state.hot_reload.as_mut() else {
                continue;
            };
            match spec
                .session
                .reload_file(&spec.path, spec.component.as_deref(), &mut state.tree)
            {
                Ok(_) => {
                    tracing::info!(
                        "Hot-reloaded {} in {:.2}ms",
                        spec.path.display(),
                        spec.session.last_reload_duration.as_secs_f64() * 1000.0
                    );
                    state.window.request_redraw();
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
        if let Some(init) = self.pending_primary.take() {
            event_loop.set_control_flow(ControlFlow::Wait);
            self.spawn_window(event_loop, init, true);
        }
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: DesktopUserEvent) {
        match event {
            DesktopUserEvent::WakeUi => {
                let _ = self.dispatcher.drain();
                for state in self.windows.values() {
                    state.window.request_redraw();
                }
            }
            DesktopUserEvent::AccessKit(ev) => {
                if let Some(state) = self.windows.get_mut(&ev.window_id) {
                    match &ev.window_event {
                        accesskit_winit::WindowEvent::ActionRequested(req) => {
                            if state.tree.handle_a11y_action(req) {
                                state.window.request_redraw();
                            }
                        }
                        accesskit_winit::WindowEvent::InitialTreeRequested => {
                            state.a11y_active = true;
                            state.tree.invalidate_a11y();
                            if let Some(a11y) = state.a11y.as_mut()
                                && let Some(update) = state.tree.build_a11y_update()
                            {
                                a11y.update_if_active(|| update);
                            }
                        }
                        accesskit_winit::WindowEvent::AccessibilityDeactivated => {
                            state.a11y_active = false;
                            state.tree.invalidate_a11y();
                        }
                    }
                }
            }
            #[cfg(feature = "hot-reload")]
            DesktopUserEvent::FileChanged => {
                self.reload_cui_from_disk();
            }
            DesktopUserEvent::OpenWindow(req_id) => {
                let req = PENDING_WINDOW_QUEUE.with(|cell| cell.borrow_mut().1.remove(&req_id));
                if let Some(req) = req {
                    let child_scope = Scope::root().child_scope();
                    let child_tree = WidgetTree::new(child_scope);
                    self.spawn_window(
                        event_loop,
                        InitialWindowSetup {
                            config: req.config,
                            root_scope: child_scope,
                            tree: child_tree,
                            builder: Some(req.builder),
                            #[cfg(feature = "hot-reload")]
                            hot_reload: None,
                        },
                        false,
                    );
                }
            }
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        let now_ms = monotonic_ms();
        let timers_ran = craftui_reactive::tick_timers();
        let jobs_ran = self.dispatcher.drain();
        let mut min_delay = craftui_reactive::next_timer_delay_ms(now_ms);
        for state in self.windows.values_mut() {
            let blink_due = state.tree.tick_blink_timers(now_ms);
            if timers_ran > 0 || jobs_ran > 0 || blink_due {
                state.window.request_redraw();
            }
            if let Some(win_delay) = state.tree.next_internal_delay_ms(now_ms) {
                min_delay = Some(min_delay.map_or(win_delay, |cur| cur.min(win_delay)));
            }
        }
        if let Some(delay_ms) = min_delay {
            event_loop.set_control_flow(ControlFlow::WaitUntil(
                Instant::now() + Duration::from_millis(delay_ms.max(1)),
            ));
        } else {
            event_loop.set_control_flow(ControlFlow::Wait);
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        let Some(state) = self.windows.get_mut(&id) else {
            return;
        };

        if let Some(a11y) = state.a11y.as_mut() {
            a11y.process_event(&state.window, &event);
        }

        match event {
            WindowEvent::CloseRequested => {
                if let Some(removed) = self.windows.remove(&id)
                    && !removed.is_primary
                {
                    removed.root_scope.dispose();
                }
                if self.windows.is_empty() {
                    event_loop.exit();
                }
            }
            WindowEvent::Resized(size) => {
                if size.width > 0 && size.height > 0 {
                    if let Some(presenter) = state.presenter.as_mut() {
                        presenter.resize(size.width, size.height);
                    }
                    let sf = state.tree.metrics.scale_factor.max(1.0);
                    state.tree.handle_event(&UiEvent::Resize {
                        width: size.width as f32 / sf,
                        height: size.height as f32 / sf,
                        scale_factor: sf,
                    });
                    state.window.request_redraw();
                }
            }
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                let sf = (scale_factor as f32).max(1.0);
                let phys = state.window.inner_size();
                if phys.width > 0 && phys.height > 0 {
                    if let Some(presenter) = state.presenter.as_mut() {
                        presenter.resize(phys.width, phys.height);
                    }
                    state.tree.handle_event(&UiEvent::Resize {
                        width: phys.width as f32 / sf,
                        height: phys.height as f32 / sf,
                        scale_factor: sf,
                    });
                    state.window.request_redraw();
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                let sf = state.tree.metrics.scale_factor.max(1.0);
                state.cursor_pos = Point::new(position.x as f32 / sf, position.y as f32 / sf);
                let ts = self.start_instant.elapsed().as_millis() as u64;
                if state.tree.handle_event(&UiEvent::Pointer(PointerEvent {
                    id: PointerId(1),
                    phase: PointerPhase::Move,
                    position: state.cursor_pos,
                    button: if state.mouse_down {
                        Some(PointerButton::Primary)
                    } else {
                        None
                    },
                    modifiers: state.modifiers,
                    is_touch: false,
                    timestamp_ms: ts,
                })) {
                    state.window.request_redraw();
                }
            }
            WindowEvent::MouseInput {
                state: btn_state,
                button,
                ..
            } => {
                let phase = match btn_state {
                    ElementState::Pressed => {
                        state.mouse_down = true;
                        PointerPhase::Down
                    }
                    ElementState::Released => {
                        state.mouse_down = false;
                        PointerPhase::Up
                    }
                };
                let ts = self.start_instant.elapsed().as_millis() as u64;
                if state.tree.handle_event(&UiEvent::Pointer(PointerEvent {
                    id: PointerId(1),
                    phase,
                    position: state.cursor_pos,
                    button: Some(translate_mouse_button(button)),
                    modifiers: state.modifiers,
                    is_touch: false,
                    timestamp_ms: ts,
                })) {
                    state.window.request_redraw();
                }
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let sf = state.tree.metrics.scale_factor.max(1.0);
                let (dx, dy) = translate_scroll_delta(delta, sf);
                if state.tree.handle_event(&UiEvent::Scroll(ScrollDelta {
                    x: dx,
                    y: dy,
                    position: state.cursor_pos,
                })) {
                    state.window.request_redraw();
                }
            }
            WindowEvent::Touch(touch) => {
                let sf = state.tree.metrics.scale_factor.max(1.0);
                let pos = Point::new(touch.location.x as f32 / sf, touch.location.y as f32 / sf);
                let phase = match touch.phase {
                    TouchPhase::Started => PointerPhase::Down,
                    TouchPhase::Moved => PointerPhase::Move,
                    TouchPhase::Ended => PointerPhase::Up,
                    TouchPhase::Cancelled => PointerPhase::Cancel,
                };
                let ts = self.timestamp_ms();
                if let Some(st) = self.windows.get_mut(&id)
                    && st.tree.handle_event(&UiEvent::Pointer(PointerEvent {
                        id: PointerId(touch.id + 100),
                        phase,
                        position: pos,
                        button: Some(PointerButton::Primary),
                        modifiers: st.modifiers,
                        is_touch: true,
                        timestamp_ms: ts,
                    }))
                {
                    st.window.request_redraw();
                }
            }
            WindowEvent::ModifiersChanged(mods) => {
                state.modifiers = translate_modifiers(mods.state());
            }
            WindowEvent::KeyboardInput { event: key_ev, .. } => {
                let ke = translate_winit_key(&key_ev, state.modifiers);
                if state.tree.handle_event(&UiEvent::Key(ke)) {
                    state.window.request_redraw();
                }
            }
            WindowEvent::Ime(ime) => {
                let ev = match ime {
                    WinitIme::Enabled => ImeEvent::Enabled,
                    WinitIme::Preedit(s, cur) => ImeEvent::Preedit(s, cur),
                    WinitIme::Commit(s) => ImeEvent::Commit(s),
                    WinitIme::Disabled => ImeEvent::Disabled,
                };
                if state.tree.handle_event(&UiEvent::Ime(ev)) {
                    state.window.request_redraw();
                }
            }
            WindowEvent::PinchGesture { delta, .. } => {
                if state.tree.handle_event(&UiEvent::Pinch {
                    factor: 1.0 + delta as f32,
                    center: state.cursor_pos,
                }) {
                    state.window.request_redraw();
                }
            }
            WindowEvent::Focused(focused) => {
                if state.tree.handle_event(&UiEvent::FocusChanged(focused)) {
                    state.window.request_redraw();
                }
            }
            WindowEvent::RedrawRequested => {
                self.render_and_present_window(id);
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
        PhysicalKey::Code(WinitKeyCode::KeyY) => KeyCode::KeyY,
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
    fn translates_mouse_buttons_scroll_deltas_and_dialog_outputs() {
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

        let paths = parse_zenity_output("/tmp/a.png|/tmp/b.png\n", '|');
        assert_eq!(
            paths,
            vec![PathBuf::from("/tmp/a.png"), PathBuf::from("/tmp/b.png")]
        );

        let rgba = [0x12u8, 0x34, 0x56, 0xFF, 0xAB, 0xCD, 0xEF, 0x80];
        let mut packed = [0u32; 2];
        pack_rgba8_to_softbuffer_0rgb(&rgba, &mut packed);
        assert_eq!(packed, [0x0012_3456, 0x00AB_CDEF]);
    }

    #[cfg(feature = "hot-reload")]
    #[test]
    fn filters_hot_reload_notify_events_to_cui_mutations() {
        let access_ev = NotifyEvent::new(notify::EventKind::Access(
            notify::event::AccessKind::Open(notify::event::AccessMode::Read),
        ))
        .add_path(PathBuf::from("ui/main.cui"));
        assert!(!should_trigger_hot_reload(&access_ev));

        let modify_cui = NotifyEvent::new(notify::EventKind::Modify(
            notify::event::ModifyKind::Data(notify::event::DataChange::Content),
        ))
        .add_path(PathBuf::from("ui/main.cui"));
        assert!(should_trigger_hot_reload(&modify_cui));

        let modify_other = NotifyEvent::new(notify::EventKind::Modify(
            notify::event::ModifyKind::Data(notify::event::DataChange::Content),
        ))
        .add_path(PathBuf::from("assets/icon.png"));
        assert!(!should_trigger_hot_reload(&modify_other));
    }
}
