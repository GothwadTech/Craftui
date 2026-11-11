# CraftUI

A pure-Rust, MIT-licensed cross-platform declarative UI framework targeting **Desktop** (Windows, macOS, Linux), **Mobile** (Android, iOS), and **Web** (WebAssembly), engineered for exceptional visual beauty, lightweight footprint, sub-100ms developer iteration, and native OS capabilities.

> **Roadmap & Docs:**
> - [`ROADMAP.md`](ROADMAP.md) — Audited issue register and verified production phases (Phases 1–5 completed).
> - [`craftui_prd.md`](craftui_prd.md) — Product requirements and capabilities.
> - [`craftui_architecture.md`](craftui_architecture.md) — System architecture and rendering pipeline.
> - [`craftui_dsl_and_design_spec.md`](craftui_dsl_and_design_spec.md) — `.cui` declarative template and design system specifications.

---

## Key Features & Highlights

- **Lightweight & High-Performance Hybrid Rendering**:
  - Pure-WGSL **Instanced SDF Quad + Glyph Atlas + Kawase Backdrop-Blur GPU backend (`gpu-sdf`)** running on Desktop (Vulkan, Metal, DirectX 12), Mobile (Vulkan, GLES), WebGPU, **and WebGL2** without compute-shader requirements.
  - Optional `vello` GPU (`gpu-vello`) and pure-software fallback renderers (`vello_cpu`, `tiny-skia`).
  - Zero idle CPU/GPU consumption: demand-driven frame pacing (`0%` idle spin) across desktop, mobile, and web.
- **Modern Declarative UI & Design System**:
  - Declarative `.cui` markup and pure-Rust `cui!` macro with compile-time type validation, two-way bindings (`<=>`), and declarative `animate` blocks.
  - Complete widget suite (40+ built-ins): `SafeArea` (dynamic notch/Dynamic Island/keyboard insets), `SegmentedControl`, `Accordion`, `DataTable` (sortable, striped, selectable), `SplitPane`, `Spinner`, `Badge`, `ToastHost`, `Modal`, `BottomSheet`, `ScrollView`, `VirtualList`, `VirtualGrid`, `Tabs`, `Slider`, `Switch`, `Checkbox`, `TextInput`, `TextArea`, `Canvas`, and `PlatformView`.
  - Built-in design token modes (`Dark`, `Light`, `Amoled`, `HighContrast`, `ThemeOverride`) with live runtime interpolation.
- **Native Mobile Capabilities & Zero-Boilerplate Permissions**:
  - **35 Standard Cross-Platform Permissions** (`Camera`, `Microphone`, `LocationWhenInUse`, `LocationAlways`, `Photos`, `Notifications`, `Bluetooth`, `Biometrics`, `Contacts`, `Sensors`, `StorageRead`, `Nfc`, `Vibrate`, `ClipboardRead`, etc.).
  - Single zero-dependency `craftui.toml` manifest:
    - Automatically emits Android `<uses-permission>` (with exact `android:maxSdkVersion` bounds) and `<uses-feature android:required="false" />`.
    - Automatically merges Apple `Info.plist` `NS*UsageDescription` privacy strings and `UIBackgroundModes`.
    - Automatically checks W3C Web Permissions API descriptor compatibility.
  - Native haptics (`HapticStyle`), OS share sheet (`SharePayload`), secure key-value storage (Android Keystore / iOS Keychain), and live device/network/battery telemetry.
- **Flutter-Surpassing Developer Experience (DX)**:
  - `craftui-cli`:
    - `craftui new <name> --template counter|dashboard|mobile`
    - `craftui init` (initializes `craftui.toml` in existing projects without overwriting code)
    - `craftui setup` (installs missing Rust targets and audits NDK/ADB/tools)
    - `craftui run --platform desktop|web|android` (multi-threaded Web static server with `application/wasm` MIME; direct ADB install/launch)
    - `craftui permissions list|check` (audits `craftui.toml` against target platforms)
    - `craftui doctor [--json]` (inspects rustc, cargo, targets, NDK, clang, Xcode, and `.cui` files)
  - **Official VS Code Extension** (`editors/vscode/`): syntax highlighting, LSP diagnostics, hover docs, completions, hex color pickers, and live hot-reload preview shortcuts.

---

## Quick Start

### 1. Install & Setup Toolchain
```bash
# Verify environment and install missing targets
cargo run -p craftui-cli -- doctor
cargo run -p craftui-cli -- setup
```

### 2. Scaffold a New Project
```bash
# Create a mobile-ready starter
cargo run -p craftui-cli -- new my_app --template mobile
cd my_app

# Audit permissions manifest
cargo run -p craftui-cli -- permissions check craftui.toml

# Validate templates and run on desktop
cargo run -p craftui-cli -- check ui/main.cui
cargo run -p craftui-cli -- run --platform desktop
```

### 3. Build & Package for All Platforms
```bash
# WebAssembly web bundle (dist/web) and serve locally
cargo run -p craftui-cli -- run --platform web --port 8080

# Android Gradle / GameActivity project (dist/android)
cargo run -p craftui-cli -- build --platform android

# iOS UIKit / Metal / C-ABI bridge bundle (dist/ios)
cargo run -p craftui-cli -- build --platform ios --skip-cargo
```

---

## Workspace Verification & Testing

```bash
# Run all workspace unit and integration tests
cargo test --workspace

# Enforce strict compiler and clippy warnings
cargo clippy --workspace --all-targets -- -D warnings

# Check headless minimal footprint build
cargo check -p craftui --no-default-features --features "desktop,dsl,hot-reload,embedded-fallback-font,gpu-sdf"

# Check Web WASM target
cargo clippy -p craftui-platform-web --target wasm32-unknown-unknown --all-targets -- -D warnings

# Run headless performance benchmark suite
cargo bench -p craftui --bench perf_suite -- --quick --json

# Run package binary size report
bash scripts/size_report.sh
```

---

## Platform Verification & Honest Limitations

| Platform | Verification Status | Notes & Limitations |
|---|---|---|
| **Linux (Host)** | **Verified** | Full desktop suite, Wayland/X11, software + GPU offscreen rendering, CLI, and LSP verified. |
| **Web (`wasm32-unknown-unknown`)** | **Verified** | Cross-compilation, strict `-D warnings` clippy, `application/wasm` HTTP server, and DOM ARIA/canvas verified; browser GPU display requires live browser. |
| **Android (`aarch64-linux-android`)** | **Verified** | Cross-compilation via Android NDK 28.2 (`aarch64-linux-android24-clang`), `AndroidHost` lifecycle/IME/permissions tests, and Gradle project generation verified; live APK on-device execution requires physical Android device or emulator. |
| **iOS (`aarch64-apple-ios`)** | **Verified C-ABI / Swift** | C-ABI bridge exports, `IosAppHost` lifecycle/touch/permissions tests, and Xcode bridge templates (`CraftUI.h`, `CraftUIViewController.swift`, `CraftUIPermissions.swift`, `Info.plist`) verified; final static library linking (`xcodebuild`) and IPA signing require macOS host. |
| **Windows (`x86_64-pc-windows-msvc`)** | **Verified Core/DSL** | Platform-neutral code, layout, rendering, and CLI verified on Linux host; Win32 `IFileDialog` / `CF_UNICODETEXT` native runtime execution requires Windows CI runner. |

---

## License

Licensed under the [MIT License](LICENSE) or [Apache License 2.0](LICENSE-APACHE) at your option. Zero GPL/LGPL/AGPL dependencies are permitted (enforced by `deny.toml`).
