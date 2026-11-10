# CraftUI

A 100% pure-Rust, MIT-licensed cross-platform declarative UI framework targeting Desktop (Linux, macOS, Windows), Android, iOS, and Web (WebAssembly).

## Architecture Overview

CraftUI implements a 7-layer retained UI architecture built on permissive ecosystem crates (`winit`, `wgpu`, `vello`, `vello_cpu`, `tiny-skia`, `taffy`, `parley`, `accesskit`):

- **`craftui-core`**: Geometry (`Point`, `Size`, `Rect`, `Insets`, `CornerRadii`, `Breakpoint`, `WindowMetrics`), colors/brushes, dirty flags, pointer/key/IME events, theme design tokens (`Light`, `Dark`, `Amoled`, `HighContrast`), and `UiDispatcher`.
- **`craftui-reactive`**: Fine-grained generational `Copy` signals (`Scope`, `Signal<T>`, `ReadSignal<T>`, `Computed<T>`, `Effect`), `batch`, `untrack`, context lookup, and automatic node dirty binding.
- **`craftui-layout`**: Flexbox & Grid layout engine wrapping `taffy`, supporting `SafeArea` hardware/IME insets, RTL mirroring, and text leaf measurement.
- **`craftui-text`**: Multi-script shaping, hit-testing, selection rects, caret placement, and Unicode/IME `TextEditorState` backed by `parley` and `fontique`.
- **`craftui-render`**: Zero-copy `PixelBufferHandle` with partial damage commits (`commit_damage`), GPU `vello` + `wgpu` surface pipeline, and automatic `vello_cpu` / `tiny-skia` software fallbacks.
- **`craftui-a11y`**: Screen-reader semantic bridge backed by `accesskit`.
- **`craftui-widgets`**: Retained `WidgetTree`, standard widgets (`Container`, `Row`, `Column`, `Grid`, `Stack`, `Spacer`, `SafeArea`, `Text`, `Button`, `ImageView`, `Canvas`, `ScrollView`, `VirtualList`, `TextInput`, `Checkbox`, `Switch`, `Slider`, `ProgressIndicator`, `Overlay`, `PlatformView`), `GestureArena` (tap, double-tap, long-press, drag, pinch/rotate), `Spring`/`Tween` animations, `Router<R>`, dynamic slots, and `Ctrl+Shift+I` Inspector overlay.
- **`craftui-dsl` & `craftui-runtime`**: Lossless `rowan` + `logos` `.cui` parser, semantic validator, AOT Rust `build.rs` codegen, and `< 100ms` dev hot-reload session preserving live signal state.
- **`craftui-macros`**: Pure-Rust `cui!` declarative UI macro and `#[component]` attribute macro.
- **`craftui-platform-{desktop,android,ios,web}` & `craftui`**: Platform backends and unified `craftui::prelude::*` facade.
- **`craftui-cli` & `craftui-lsp`**: Developer CLI (`new`, `dev`, `preview`, `check`, `render`, `build`) and `.cui` Language Server (`tower-lsp`).

## Quick Start

```bash
# Run workspace unit & integration tests
cargo test --workspace

# Validate .cui templates
cargo run -p craftui-cli -- check examples

# Render a headless snapshot of a .cui file
cargo run -p craftui-cli -- render examples/counter/ui/counter.cui -o preview.png

# Launch examples
cargo run -p craftui-example-counter
cargo run -p scancraft-studio-demo
```

## License

Licensed under the [MIT License](LICENSE). Zero GPL/LGPL/AGPL dependencies are permitted (enforced via `deny.toml`).
