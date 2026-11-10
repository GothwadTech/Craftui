# CraftUI

A pure-Rust, MIT-licensed cross-platform declarative UI framework targeting **Desktop** (Windows, macOS, Linux), **Mobile** (Android, iOS), and **Web** (WebAssembly).

> See [`ROADMAP.md`](ROADMAP.md) for the full audited issue register and phased production roadmap, [`craftui_prd.md`](craftui_prd.md) for product requirements, [`craftui_architecture.md`](craftui_architecture.md) for system architecture, and [`craftui_dsl_and_design_spec.md`](craftui_dsl_and_design_spec.md) for `.cui` & design specs.

## Core Architecture Highlights

- **`craftui-core`**: Geometry (`Point`, `Size`, `Rect`, `Transform2D`, `CornerRadii`, `WindowMetrics`), colors/brushes/gradients, layered/inset `BoxShadow`, blend modes, cursor icons, theme design tokens (`Light`, `Dark`, `Amoled`, `HighContrast`, `ThemeOverride`), platform service traits, and `UiDispatcher`.
- **`craftui-reactive`**: Fine-grained generational `Copy` signals (`Scope`, `Signal<T>`, `ReadSignal<T>`, `Computed<T>`, `Effect`), `batch`, `untrack`, context lookup, and scope-bound timers/tasks.
- **`craftui-layout`**: Dirty-aware Flexbox, CSS Grid, and Z-`Stack` layout engine powered by `taffy` with `SafeArea` insets and RTL mirroring.
- **`craftui-text`**: Multi-script text shaping (`parley` + `harfrust` + `fontique`) with **LRU shaped-layout cache**, bundled permissive **65 KB `Vera.ttf` fallback font** (always renders text out of the box on WASM/headless Linux), and full `TextEditorState` (undo/redo, word motion, selection, IME).
- **`craftui-render`**: Feature-gated hybrid renderer supporting a lightweight **Instanced SDF Quad + Glyph Atlas + Kawase BackdropBlur GPU backend (`gpu-sdf`)** alongside optional **`vello` GPU (`gpu-vello`)** and **`vello_cpu` / `tiny-skia`** software backends, plus SVG icons and damage-tracked pixel buffers.
- **`craftui-a11y`**: Incremental screen-reader semantic tree synchronization powered by `accesskit`.
- **`craftui-widgets`**: Retained `WidgetTree`, property animations (`Spring` / `Tween` / transitions), standard & composable widgets (`Row`, `Column`, `Grid`, `Stack`, `Spacer`, `SafeArea`, `Divider`, `Icon`, `Text`, `Button`, `ImageView`, `Canvas`, `ScrollView`, `VirtualList`, `VirtualGrid`, `TextInput`, `TextArea`, `Checkbox`, `Switch`, `Slider`, `Tabs`, `Select`, `RadioGroup`, `PopupMenu`, `Tooltip`, `ProgressIndicator`, `Modal`, `BottomSheet`, `Toast`, `SplitPane`, `PlatformView`), `CustomWidget`, `GestureArena`, `Router<R>`, and `Ctrl+Shift+I` Inspector.
- **`craftui-dsl` & `craftui-runtime`**: Lossless `rowan` + `logos` `.cui` compiler with strict typed validation, declarative `animate` blocks, reactive transforms & styles, keyed `for`, AOT Rust codegen, and optional feature-gated hot-reload runtime.
- **`craftui-macros`**: Pure-Rust `cui!` macro and `#[component]` attribute macro.
- **`craftui-platform-{desktop,android,ios,web}` & `craftui`**: Demand-driven platform runners (`0%` idle spin) and unified `craftui::prelude::*` facade.
- **`craftui-cli` & `craftui-lsp`**: Project scaffolding/bundling CLI (`new`, `dev`, `preview`, `check`, `render`, `doctor`, `build`) and context-aware `.cui` Language Server (`tower-lsp`).

## Quick Start

```bash
# Run workspace unit & integration tests
cargo test --workspace

# Check strict lints
cargo clippy --workspace --all-targets -- -D warnings

# Validate all .cui templates
cargo run -p craftui-cli -- check examples

# Render a headless snapshot of a .cui file
cargo run -p craftui-cli -- render examples/showcase/ui/showcase.cui -o showcase.png

# Launch flagship examples
cargo run -p craftui-example-counter
cargo run -p craftui-example-showcase
cargo run -p craftui-example-todo
cargo run -p craftui-example-settings
cargo run -p craftui-example-canvas
```

## License

Licensed under the [MIT License](LICENSE). Zero GPL/LGPL/AGPL dependencies are permitted (enforced via `deny.toml`).
