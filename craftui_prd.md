# CraftUI — Product Requirements Document (PRD)

> **Working Codename:** `CraftUI` (placeholder — rename freely before crates.io reservation)  
> **License:** MIT (Dual-compatible with Apache-2.0)  
> **Target Platforms:** Desktop (Windows, macOS, Linux), Mobile (Android, iOS), Web (WASM)  
> **Core Stack:** 100% Pure Rust (`winit`, `wgpu`, `vello`, `tiny-skia`, `taffy`, `parley`, `accesskit`)

---

## 1. Executive Summary & Product Vision

**CraftUI** is an open-source, **MIT-licensed**, compiled declarative UI framework built in Rust. Its mission is to deliver the ergonomics, tiny runtime footprint, and smooth 60–120 FPS native performance of Slint — **without GPLv3 restrictions or commercial licensing fees** — while providing first-class support for **Desktop, Mobile (Android & iOS), and Web (WASM)** from a single codebase.

Unlike Electron or Tauri, CraftUI does **not** embed or rely on a browser WebView for native targets. Instead, it compiles declarative UI templates into zero-overhead native Rust code (with a fast interpreted hot-reload mode during development) rendered via GPU compute shaders (`vello`/`wgpu`) with an automatic CPU software fallback (`vello_cpu`/`tiny-skia`).

---

## 2. Market Gap & Competitive Positioning

| Framework | License | Language / Syntax | Desktop | Android / iOS | Web (WASM) | Key Limitation for Our Use Case |
| :--- | :--- | :--- | :---: | :---: | :---: | :--- |
| **Slint** | GPLv3 / Paid | `.slint` DSL + Rust | Yes | Yes | Yes | Copyleft GPLv3 or paid commercial license for closed apps |
| **Makepad** | MIT | `live_design!` DSL | Yes | Yes | Yes | Experimental API ergonomics, non-standard widget kit |
| **Xilem / Masonry** | Apache-2.0 / MIT | Rust Builder API | Yes | WIP | WIP | No dedicated markup DSL; mobile & web still maturing |
| **Iced** | MIT | Rust Elm (MVU) | Yes | Limited | Yes | Mobile touch/IME/platform-view support is secondary |
| **Dioxus** | MIT / Apache-2.0 | RSX Macro | Yes | Beta | Yes | WebView overhead by default; Blitz native renderer still maturing |
| **Tauri 2.0** | MIT / Apache-2.0 | HTML/JS + Rust | Yes | Yes | N/A | WebView memory/latency bottleneck for camera & heavy canvas apps |
| **CraftUI (Target)** | **MIT** | **`.cui` DSL + `cui!` Macro** | **Yes** | **Yes** | **Yes** | **New framework — mitigates risk by composing proven MIT crates** |

---

## 3. Target Audience & Personas

1. **Privacy & Performance-First App Developers (e.g., ScanCraft, Media Editors, PDF Tools):**
   - Need direct zero-copy GPU/buffer rendering for camera feeds, high-resolution images, and vector annotations without WebView serialization overhead.
   - Require a permissive MIT license so apps can be distributed on Google Play, Apple App Store, Steam, or Enterprise channels with zero legal friction.
2. **Cross-Platform Rust Product Teams:**
   - Want one shared business logic + UI codebase across Windows, macOS, Linux, Android, iOS, and Web.
   - Want sub-20ms incremental dev hot-reload without waiting 30 seconds for full Rust recompilation on every padding tweak.
3. **Embedded & Lightweight Utility Builders:**
   - Need small stripped binaries (< 5 MB native core) and deterministic memory usage without GC pauses.

---

## 4. Core Differentiators

1. **100% Permissive MIT Stack:** Automated CI license enforcement (`cargo-deny`) guarantees zero GPL/AGPL/LGPL dependencies anywhere in the dependency tree.
2. **Dual-Mode Compilation (Interpreter + AOT):** Instant hot-reload preview in development; zero-overhead statically compiled Rust state-machine in release builds.
3. **Fine-Grained Signal Reactivity (No Virtual DOM):** `Copy`-able arena signal handles update only affected layout nodes and paint commands, eliminating Rust borrow-checker friction in UI callbacks.
4. **First-Class Native Surface Embedding (`PlatformView`):** Day-1 support for embedding Android `SurfaceTexture`/CameraX, iOS `CALayer`/Metal views, or native video surfaces directly inside the CraftUI scene graph.
5. **GPU-First with Graceful Software Fallback:** Uses `vello` (`wgpu`) on modern hardware and transparently falls back to `vello_cpu`/`tiny-skia` on headless runners, VMs, or older mobile GPUs with broken Vulkan/GLES compute drivers.

---

## 5. Functional Requirements (FR)

### FR-1: Declarative UI DSL & Rust Macro Interop
* **FR-1.1 `.cui` File Format:** Human-readable declarative syntax supporting components, properties, two-way bindings, event callbacks, loops (`for`), conditionals (`if`), and built-in animation transitions.
* **FR-1.2 Inline `cui!` Proc-Macro:** Pure-Rust macro alternative for teams preferring single-file Rust development with full `rust-analyzer` type checking.
* **FR-1.3 AOT Codegen:** `build.rs` / CLI compiler translates `.cui` files into strongly typed Rust structs and Signal bindings at compile time.
* **FR-1.4 Dev Hot-Reload Interpreter:** File watcher reloads `.cui` layout, styles, and tokens into a running desktop/mobile preview shell in < 100ms without relinking Rust.

### FR-2: Fine-Grained Reactive State Engine
* **FR-2.1 Arena Signal Handles:** `Signal<T>`, `ReadSignal<T>`, `Computed<T>`, and `Effect` backed by generational arena indices (`slotmap`), making handles `Copy` so closures require no `.clone()` or `Rc<RefCell<...>>`.
* **FR-2.2 Scoped Ownership:** Signals attach to component scopes and automatically dispose subscriptions when a component unmounts (preventing memory leaks).
* **FR-2.3 Async Runtime Agnostic:** Built-in task bridge allowing background worker threads (`rayon`, `tokio`, `std::thread`) to push updates safely onto the UI event loop.

### FR-3: Cross-Platform Layout Engine
* **FR-3.1 Flexbox & CSS Grid:** Powered by `taffy`, supporting `Row`, `Column`, `Grid`, `Stack` (Z-order), `Spacer`, `Padding`, `Margin`, `AspectRatio`, and `Constraints`.
* **FR-3.2 Safe-Area & Keyboard Insets:** First-class `SafeArea` component reacting automatically to mobile notches, navigation bars, and software keyboard (IME) height changes.
* **FR-3.3 RTL & Responsive Breakpoints:** Automatic layout mirroring for Right-to-Left locales and reactive viewport size breakpoints (`Compact`, `Medium`, `Expanded`).

### FR-4: Hybrid GPU / Software Rendering & External Textures
* **FR-4.1 Vector & Scene Graph Rendering:** Anti-aliased paths, rounded rects, shadows, linear/radial gradients, clipping, opacity layers, and SVG rendering via `vello` (`wgpu`).
* **FR-4.2 Automatic Software Fallback:** Seamless fallback to CPU rasterization (`vello_cpu` / `tiny-skia`) when `wgpu` adapter creation fails or device lacks compute shader support.
* **FR-4.3 Zero-Copy Image & Pixel Buffer API:** Mutable `TextureHandle` / `PixelBuffer` API allowing real-time CV/image-processing pipelines (like live photo filters or camera frames) to stream straight to GPU textures without PNG/JPEG re-encoding.
* **FR-4.4 Native `PlatformView` Slot:** Hole-punch / external texture composition API for embedding OS-native Camera previews (CameraX/AVFoundation), WebViews, or Maps seamlessly underneath/between CraftUI layers.

### FR-5: Text Shaping, Fonts & International Input (IME)
* **FR-5.1 Complex Script Shaping:** Powered by `parley` + `harfrust`, supporting Latin, Devanagari, Arabic (BiDi), CJK, and Color Emoji with automatic system font fallback (`fontique`).
* **FR-5.2 Rich `TextInput` & IME:** Multi-line editing, selection handles, clipboard copy/paste, cursor blinking, hardware keyboard shortcuts, and mobile IME pre-edit composition (autocomplete/swipe typing).

### FR-6: Standard Widget Library & Adaptive Theming
* **FR-6.1 Core Primitives:** `View`, `Text`, `Image`, `Canvas`, `ScrollView`, `VirtualList` / `VirtualGrid` (windowed recycling for 10,000+ items), `Button`, `TextInput`, `Checkbox`, `Switch`, `Slider`, `ProgressIndicator`.
* **FR-6.2 Overlays & Navigation:** Declarative `Modal`, `BottomSheet`, `PopupMenu`, `Tooltip`, and stack-based `Router` with mobile back-gesture support.
* **FR-6.3 Design Token & Theme System:** Reactive `ThemeScope` supporting Light, Dark, AMOLED Black, and High-Contrast variants with smooth color interpolation.
* **FR-6.4 Gesture Arena:** Unified pointer/touch recognizer supporting `Tap`, `DoubleTap`, `LongPress`, `Drag`, `PinchZoom`, `Rotate`, and inertial momentum fling physics.

### FR-7: Web Target Architecture (WASM)
* **FR-7.1 Unified WASM Canvas Mode:** Compiles directly to `wasm32-unknown-unknown` rendering via WebGPU (with WebGL2 fallback) for 100% visual parity across all platforms.
* **FR-7.2 Browser Integration:** URL routing, browser history sync, virtual hidden `<input>` bridge for mobile web keyboards/IME, and native clipboard/file-picker APIs.
* **FR-7.3 Optional DOM Projection Mode (Phase 5+):** Trait-abstracted backend allowing future lightweight HTML/DOM rendering for SEO-critical web pages without changing app state logic.

### FR-8: Accessibility (`accesskit`)
* **FR-8.1 Native Screen Reader Sync:** Synchronizes semantic UI tree (roles, labels, bounds, focus, actions) to Windows UIA, macOS VoiceOver, Linux AT-SPI, Android TalkBack, and Web ARIA via `accesskit`.

### FR-9: Developer Tooling (`craftui-cli` & LSP)
* **FR-9.1 CLI Scaffolding:** `craftui new`, `craftui run --platform desktop|android|ios|web`, `craftui build --release`.
* **FR-9.2 Live Inspector:** Dev-mode overlay (`Ctrl+Shift+I`) showing layout boxes, padding bounds, FPS/frame-time graph, and signal updates.
* **FR-9.3 Language Server (`craftui-lsp`):** Syntax highlighting, autocomplete, inline color previews, and live error diagnostics for `.cui` files.

---

## 6. Non-Functional Requirements (NFR)

* **NFR-1 Frame Latency & Smoothness:**
  * UI layout + paint budget <= **8.3ms** (120 FPS) for standard screens on modern desktop and mid-range ARM64 mobile devices.
  * Continuous `VirtualList` scrolling must achieve zero dropped frames after warm-up.
* **NFR-2 Binary Size & Startup Time:**
  * Stripped Desktop release binary (Hello World + core widgets): **<= 5 MB**.
  * Stripped Android `.so` library (per ABI `arm64-v8a`): **<= 4 MB**.
  * Web WASM (`wasm-opt -Oz` + brotli): **<= 2.5 MB** compressed.
  * Cold startup to first rendered frame: **<= 150ms** on Desktop, **<= 300ms** on mid-range Android.
* **NFR-3 Memory Safety & Footprint:**
  * Zero `unsafe` in reactive, layout, and widget user-facing layers; any `unsafe` strictly isolated and documented inside FFI/GPU/JNI platform boundaries.
  * Idle memory footprint for a 5-screen utility app: **<= 45 MB RAM**.
* **NFR-4 License Purity:**
  * 100% of direct and transitive crates must pass `cargo-deny` allow-list (`MIT`, `Apache-2.0`, `BSD-2-Clause`, `BSD-3-Clause`, `ISC`, `Zlib`, `Unicode-3.0`).
* **NFR-5 Platform Minimums:**
  * **Desktop:** Windows 10+, macOS 12+, Linux (Wayland & X11).
  * **Mobile:** Android 8.0+ (API 26+, `arm64-v8a` & `x86_64`), iOS 15+.
  * **Web:** Modern Chrome, Firefox, Safari, Edge (WebGPU / WebGL2 + WASM).

---

## 7. Success Metrics (v0.1 Alpha → v1.0)

1. **Dogfooding Proof:** Successfully render a real-world multi-screen application (**ScanCraft Desktop & Mobile Prototype**: document grid, interactive 8-point crop canvas with magnifier loupe, live filter sliders, and virtualized page viewer) entirely in CraftUI at 60+ FPS.
2. **Developer Onboarding:** A Rust developer can scaffold, hot-reload, and deploy a multi-platform counter/image app to Desktop + Web in under **10 minutes**.
3. **CI Cross-Compilation Green:** Automated GitHub Actions matrix passes builds & headless snapshot tests for Linux, Windows, macOS, Android, iOS, and WASM on every PR.
