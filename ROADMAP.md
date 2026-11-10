# CraftUI — Issue Register & Production Roadmap

> Last Audited: 2026-11-10  
> Purpose: Honest baseline audit of every bug, bottleneck, bloat source, visual gap, and platform gap in CraftUI, plus the phased plan to turn it into a top-tier MIT-licensed UI framework comparable to Slint, GPUI, and Flutter.

---

## Part 1: Verified Issue Register (Current Codebase Audit)

### 1. Performance & Power Bottlenecks
- [ ] **PERF-01 (Critical): Per-frame text re-shaping & full scene rebuild** — `WidgetTree::paint` calls `scene.clear()` every frame, and `encode_vello_scene` / `VelloCpuBackend::render_premul` calls `TextEngine::build_layout` (full Parley shaping + line-breaking + `Vec<i16>` allocations) for every `DrawText` on every frame (`TextInput` shapes twice per frame).
- [ ] **PERF-02 (High): Unconditional full-tree Taffy layout sync** — `LayoutEngine::compute_layout` walks and calls `taffy.set_style()` on every node in the tree plus clones `Vec<NodeId>` children lists on every layout pass.
- [ ] **PERF-03 (High): Unconditional full AccessKit tree rebuild** — `DesktopRunner::render_and_present_window` calls `tree_ref.build_a11y_update()` for the entire tree on every painted frame whenever a11y is active.
- [ ] **PERF-04 (Critical): Always-on 60fps idle spin loops** — Android (`run_android_app` 16ms polling ignoring `_anim`), Web (`start_web_app` unconditional `requestAnimationFrame` ignoring `_anim`), and Desktop focused `TextInput` (`tick_animations` returning `active = true` continuously for caret blink) waste battery and CPU/GPU even when idle.

### 2. "Lightweight" & Footprint Contradictions
- [ ] **BLOAT-01 (Critical): Unconditional triple renderer bundling** — `craftui-render/Cargo.toml` pulls `vello` (+ `vello_shaders`), `vello_cpu`, `wgpu`, and `tiny-skia` unconditionally with zero feature flags.
- [ ] **BLOAT-02 (High): `TinySkiaBackend` incomplete** — Silently drops `DrawText`, `DropShadow`, `PushClip`, `PushLayer`, and multi-stop gradients (`_ => {}`).
- [ ] **BLOAT-03 (High): Compiler & AST interpreter shipped in every app** — `craftui/Cargo.toml` unconditionally depends on `craftui-dsl` (`logos` + `rowan`) and `craftui-runtime`, inflating release and WASM binaries.
- [ ] **BLOAT-04 (Critical): Zero bundled fallback font** — `TextEngine::new()` loads zero fallback font bytes; WASM and minimal Linux render blank text unless manually registered.

### 3. Visual Quality & "Beautiful UI" Gaps
- [ ] **VIS-01 (High): Missing Transform2D stack** — `PaintCommand` lacks `PushTransform`/`PopTransform` (`translate`, `scale`, `rotate`, `skew`).
- [ ] **VIS-02 (High): Missing Backdrop Blur (Glassmorphism) & Blend Modes** — No frosted-glass backdrop pass or blend modes.
- [ ] **VIS-03 (Medium): Limited `DropShadow` & huge opacity layer bounds** — Shadow uses scalar radius instead of `CornerRadii`, lacks `spread` & `inset`, and GPU `PushLayer` passes `-1e5..1e5` dummy rect bounds; swapchain lacks explicit sRGB surface discipline.
- [ ] **VIS-04 (Critical): No declarative widget/property animation system** — `.cui` has no `animate` syntax; hover/press/tabs/switch/modal/router snap instantaneously.
- [ ] **VIS-05 (Medium): Composable widget & control limits** — `Button` cannot contain child widgets (e.g. Icon + Text) and uses hardcoded `Color::WHITE` focus ring (invisible in light themes).

### 4. Concrete Widget, DSL & Runtime Bugs
- [ ] **BUG-01 (High): `ScrollView` drag viewport bug** — `WidgetTree::handle_drag` clamps scroll offset using dragged child's `rect` instead of the `ScrollView` (`nid`) viewport rect.
- [ ] **BUG-02 (High): `for`-loop keys include array index** — `codegen.rs` and `runtime/lib.rs` prefix keys with `"{idx}:"`, breaking keyed reconciliation on list insert/delete/reorder.
- [ ] **BUG-03 (Critical): Typos silently compile** — Unknown identifiers in `codegen.rs`/`runtime/lib.rs` silently emit string literals (`"typo_var"`) or `Default::default()`. `validator.rs` checks all widgets against one flat `BUILTIN_PROPS` set and only emits `Warning`.
- [ ] **BUG-04 (High): AOT vs Runtime divergence & static properties** — `codegen.rs` mutates one-way `:` signals on controls instead of enforcing `<=>`; layout/box/font/opacity properties evaluate statically once at build time; `.cui` `Canvas` is an empty stub; `PlatformView` hardcodes `default()`.
- [ ] **BUG-05 (Medium): Handler statement limits & `cui!` gaps** — `.cui` handlers cannot mutate struct fields (`item.done = true`), `+=`, or `if/else`; `cui!` lacks `if`/`for`/two-way bindings and relies on fragile `theme.<field>` AST heuristics.
- [ ] **BUG-06 (Medium): Editor & Overlay defects** — `TextInput` `bound_signal` sync jumps cursor to `len()`; `KeyCode` maps only 6 shortcut letters; macOS `Cmd` (`super_key`) shortcuts missing; undo/redo & word jump missing; `Select` dropdown isn't scrollable & hit-tests in `SlotMap` order; `set_box_style` re-invocations leak effects; `HotReloadSession` destroys entire tree on save; `resolver.rs` lets imports shadow entry declarations.

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
