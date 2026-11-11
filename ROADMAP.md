# CraftUI — Issue Register & Production Roadmap

> Last Audited: 2026-11-10  
> Purpose: Honest baseline audit of every bug, bottleneck, bloat source, visual gap, and platform gap in CraftUI, plus the phased plan to turn it into a top-tier MIT-licensed UI framework comparable to Slint, GPUI, and Flutter.

---

## Part 1: Verified Issue Register (Current Codebase Audit)

### 1. Performance & Power Bottlenecks
- [x] **PERF-01 (Critical): Per-frame text re-shaping & full scene rebuild** — Bounded 1024-entry `ShapedLayoutCache` added in `craftui-text` and wired across `WidgetTree`, `VelloCpuBackend`, and `GpuSurfacePipeline`; idle frames skip scene rebuilds across Desktop, Web, Android, and iOS.
- [x] **PERF-02 (High): Unconditional full-tree Taffy layout sync** — `LayoutEngine::compute_layout` now tracks `dirty_styles` and environment changes per node and eliminates per-node child `Vec` clones.
- [x] **PERF-03 (High): Unconditional full AccessKit tree rebuild** — Added `A11yBridge::build_incremental_update` and `a11y_active` tracking so AccessKit diffs only when state or focus changes while AT is active.
- [x] **PERF-04 (Critical): Always-on 60fps idle spin loops** — Replaced with 530ms demand-driven caret blink timers, `ControlFlow::Wait`/`WaitUntil` on Desktop, dynamic `poll_events` timeouts on Android, on-demand `requestAnimationFrame` + `setTimeout` on Web, and `needs_redraw` + deduplicated `SyncText` on iOS.

### 2. "Lightweight" & Footprint Contradictions
- [x] **BLOAT-01 (Critical): Unconditional triple renderer bundling** — Added strict Cargo feature flags in `craftui-render` (`gpu-sdf`, `gpu-vello`, `cpu-vello`, `cpu-tiny-skia`) and built the pure-WGSL instanced SDF + glyph-atlas + Kawase backdrop-blur renderer (`gpu_sdf_backend.rs` + `sdf_quad.wgsl`).
- [x] **BLOAT-02 (High): `TinySkiaBackend` incomplete** — Completed `TinySkiaBackend` with glyph rasterization (`glyph_raster.rs`), `DrawText`, rounded/spread/inset `DropShadow`, `PushClip`, `PushLayer`, `BackdropBlur`, `Transform2D`, and multi-stop gradients.
- [x] **BLOAT-03 (High): Compiler & AST interpreter shipped in every app** — Gated `craftui-dsl`, `craftui-runtime`, and `notify` behind `dsl` and `hot-reload` features across `craftui` and `craftui-platform-desktop`; verified `--no-default-features --features desktop,gpu-sdf` excludes `vello`, `craftui-dsl`, `craftui-runtime`, and `notify`.
- [x] **BLOAT-04 (Critical): Zero bundled fallback font** — Embedded permissive 65 KB Bitstream `Vera.ttf` behind default `embedded-fallback-font` feature in `craftui-text` so WASM and minimal Linux always render text out of the box.

### 3. Visual Quality & "Beautiful UI" Gaps
- [x] **VIS-01 (High): Missing Transform2D stack** — Added `Transform2D` (`translate`, `scale`, `rotate`, `skew`, `around_pivot`) with `PushTransform`/`PopTransform` across `craftui-core`, `PaintScene`, `WidgetTree`, GPU (`gpu-sdf` / `vello`), and CPU backends.
- [x] **VIS-02 (High): Missing Backdrop Blur (Glassmorphism) & Blend Modes** — Implemented `BackdropBlur` (Kawase dual-filter GPU pass in `gpu-sdf` + separable box blur in software backends) and `BlendMode` compositing across `BoxStyle`, `PaintScene`, GPU, and CPU renderers.
- [x] **VIS-03 (Medium): Limited `DropShadow` & huge opacity layer bounds** — Upgraded `BoxShadow` with per-corner `CornerRadii`, `spread`, and `inset` shadows, computed tight subtree opacity layer bounds in `WidgetTree`, and configured explicit sRGB/non-sRGB WGSL blending discipline.
- [x] **VIS-04 (Critical): No declarative widget/property animation system** — Completed across `WidgetTree`, `.cui` (`animate <prop> { duration: ...; easing: ...; }`), `codegen.rs`, `craftui-runtime`, `cui!` macro (`animate <prop> { ... }`), and `craftui-lsp` completions/validation.
- [x] **VIS-05 (Medium): Composable widget & control limits** — `Button` renders child subtrees when provided and draws a high-contrast dual focus ring visible in both dark and light themes.

### 4. Concrete Widget, DSL & Runtime Bugs
- [x] **BUG-01 (High): `ScrollView` drag viewport bug** — `WidgetTree::handle_drag` now clamps scroll offset against the `ScrollView` (`nid`) viewport rect instead of the dragged child's rect.
- [x] **BUG-02 (High): `for`-loop keys include array index** — `reactive_for_keyed`, `codegen.rs`, `craftui-runtime`, and `cui!` now key items by explicit `key: <expr>` identity (reusing `NodeId`+`Scope` and updating `Signal<T>`/`Signal<usize>` in place), with duplicate-key occurrence fallback.
- [x] **BUG-03 (Critical): Typos silently compile** — Replaced flat `BUILTIN_PROPS` with strict per-widget `props_for_widget`/`events_for_widget` schemas, `Severity::Error` diagnostics, ternary `? :` validation, scope/identifier checks, and zero-division guards in `validator.rs` + `craftui-lsp`.
- [x] **BUG-04 (High): AOT vs Runtime divergence & static properties** — Unified `:` (one-way) vs `<=>` (two-way) enforcement, dynamic reactive layout/box/transform/glass/text bindings, `Button` empty child labels, `Canvas` callbacks, and `PlatformViewId::from_raw` across `codegen.rs`, `craftui-runtime`, and `cui!`.
- [x] **BUG-05 (Medium): Handler statement limits & `cui!` gaps** — Supported nested/indexed/compound (`+=`, `-=`, `*=`, `/=`) handler assignments and `if/else` in `.cui`; brought full parity (`if/else`, keyed `for`, `<=>`, transforms, glassmorphism, `animate` blocks) to `cui!`.
- [x] **BUG-06 (Medium): Editor & Overlay defects** — Fixed in Phase 1 + Phase 3 (`HotReloadSession` preserves widget state across live reloads and `resolver.rs` fixes local-before-import priority).

### 5. Platform Integration & Tooling Gaps
- [ ] **PLAT-01 (High): Blocking desktop OS dialogs & clipboard** — Spawning `zenity`/`kdialog`/`osascript`/`powershell`/`wl-paste` synchronously on the UI thread freezes rendering and fails in sandboxes.
- [ ] **PLAT-02 (High): Incomplete Web bridge** — `WebClipboard::get_text` ignores real browser clipboard; zero DOM ARIA accessibility nodes; `obtain_2d_context` leaks listeners; no CSS cursor sync.
- [ ] **PLAT-03 (High): Prototype Android & iOS bridges** — CPU RGBA copies instead of `AHardwareBuffer`/`CVMetalTextureCache`; Android missing punctuation keys & `compose_region`; iOS emits `SyncText` every frame and ignores Metal init errors; no Kotlin/Gradle or Swift/UIKit templates.
- [ ] **PLAT-04 (High): `craftui build` lacks real packaging** — Only calls `cargo build -p craftui-platform-*` instead of building user apps into Web `dist/`, Android Gradle/APK, or iOS XCFramework outputs.

---

## Part 2: Phased Production Roadmap

### Phase 1 — Performance Engine, Demand-Driven Power & Core Bug Fixes
- Add LRU **ShapedLayoutCache** inside `craftui-text` + embed permissive 65 KB **`Vera.ttf`** fallback font (feature-gated `embedded-fallback-font`).
- Make `LayoutEngine::compute_layout` dirty-aware and eliminate per-frame child `Vec` clones; add incremental dirty tracking to `A11yBridge`.
- Replace all idle 60fps spin loops (Desktop caret blink timer at 530ms, Android dynamic `poll_events` timeout/blocking wait, Web on-demand `requestAnimationFrame`, iOS dirty-checked `SyncText` + redraw flags).
- Fix `ScrollView` drag viewport bug, `Select` Z-order hit-test & scrolling, `Button` child composability & theme-aware focus ring, `TextEditorState` undo/redo + word motion + cursor preservation + cross-OS `Cmd`/`Ctrl` shortcuts, and property-effect replacement hygiene.

### Phase 2 — Lightweight Feature-Gating & Next-Gen Hybrid GPU Renderer
- Add strict Cargo feature flags in `craftui-render` (`gpu-sdf`, `gpu-vello`, `cpu-vello`, `cpu-tiny-skia`) and `craftui` (`dsl`, `hot-reload`).
- Upgrade `PaintScene` & `BoxStyle` with `Transform2D` push/pop stack, rich `BoxShadow` (per-corner `CornerRadii`, `spread`, `inset`, multi-shadow), `BackdropBlur` (frosted glass), `BlendMode`, and tight bounded opacity layers.
- Build **Instanced SDF Quad + Glyph Atlas + Kawase BackdropBlur GPU Renderer (`gpu-sdf`)** using pure WGSL render pipelines (works on Desktop Vulkan/Metal/DX12, Mobile Vulkan/GLES, WebGPU, **and WebGL2** without compute shaders!), plus complete `TinySkiaBackend` (text, shadows, gradients, clips, opacity).
- Wire first-class **Property Animation & Transition Engine (`AnimatedProperty` + `TransitionSpec`)** with spring/tween transitions on widgets, modals, sheets, tabs, switches, checkboxes, buttons, and `Router` transitions.

### Phase 3 — Strict `.cui` Compiler, Declarative Animations, LSP & State-Preserving Hot-Reload
- Replace flat `BUILTIN_PROPS` with typed per-widget schemas; check expressions, scopes, member accesses, types, and two-way `<=>` writability; emit `Severity::Error` on invalid code.
- Fix `resolver.rs` import priority; implement `for` stable identity keys (`key: expr` without index); support all reactive layout/visual/transform props, `animate` blocks, rich handler statements (`item.field = ...`, `+=`, `if/else`), `Button` children, `Canvas` callbacks, and `PlatformView` IDs in both AOT `codegen.rs` and `craftui-runtime`.
- Upgrade `cui!` macro (`if`, `for`, two-way bindings, transforms/animations) and state-preserving `HotReloadSession` + context-aware `craftui-lsp`.

### Phase 4 — Production Polish, Native Bridges & Multi-Platform CLI Bundling
- Expand polished UI components (`ToastManager`, `Spinner`, `SplitPane`, `Badge`/`Chip`, `CursorIcon` propagation).
- Upgrade Desktop (non-blocking native clipboard/file dialogs via `arboard` / `rfd` or async portals), Web (DOM ARIA mirror, async clipboard, CSS cursor, leak-free listeners, Web demo bundling), Android (full keymap, `compose_region`, Gradle/Kotlin templates), iOS (safe Metal attach errors, Swift/UIKit templates).
- Upgrade `craftui-cli` (`new`, `doctor`, `build --platform web|android|ios|desktop`) with real deployable artifacts and modernize `examples/showcase` to demonstrate glassmorphism, spring animations, transforms, and multi-platform readiness.
