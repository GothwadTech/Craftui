# CraftUI

A 100% pure-Rust, MIT-licensed cross-platform declarative UI framework targeting Desktop (Linux, macOS, Windows), Android, iOS, and Web (WebAssembly).

## Architecture Overview

CraftUI implements a 7-layer retained UI architecture built on permissive ecosystem crates (`winit`, `wgpu`, `vello`, `vello_cpu`, `tiny-skia`, `taffy`, `parley`, `accesskit`):

- **`craftui-core`**: Geometry (`Point`, `Size`, `Rect`, `Insets`, `CornerRadii`, `Breakpoint`, `WindowMetrics`), colors/brushes, dirty flags, pointer/key/IME events, theme design tokens (`Light`, `Dark`, `Amoled`, `HighContrast`, `ThemeOverride`), platform abstractions (`ClipboardBackend`, `FileDialogBackend`, `PlatformServices`), and `UiDispatcher`.
- **`craftui-reactive`**: Fine-grained generational `Copy` signals (`Scope`, `Signal<T>`, `ReadSignal<T>`, `Computed<T>`, `Effect`), `batch`, `untrack`, context lookup, scope-bound timer/task hooks (`after`, `interval`, `spawn`), and automatic node dirty binding.
- **`craftui-layout`**: Flexbox & Grid layout engine wrapping `taffy`, supporting `SafeArea` hardware/IME insets, RTL mirroring, and text leaf measurement.
- **`craftui-text`**: Multi-script shaping, hit-testing, selection rects, caret placement, and Unicode/IME `TextEditorState` backed by `parley` and `fontique`.
- **`craftui-render`**: Zero-copy `PixelBufferHandle` with partial damage commits (`commit_damage`), SVG icon/path parsing & painting, GPU `vello` + `wgpu` surface pipeline (sync & async `new_async` for WebGPU/WebGL2), `BlendMode::Clear` cutout compositing, and automatic `vello_cpu` / `tiny-skia` software fallbacks.
- **`craftui-a11y`**: Screen-reader semantic bridge backed by `accesskit`.
- **`craftui-widgets`**: Retained `WidgetTree`, standard widgets (`Container`, `Row`, `Column`, `Grid`, `Stack`, `Spacer`, `SafeArea`, `Divider`, `Icon`, `Text`, `Button`, `ImageView`, `Canvas`, `ScrollView`, `VirtualList`, `VirtualGrid`, `TextInput`, `TextArea`, `Checkbox`, `Switch`, `Slider`, `Tabs`, `Select`, `RadioGroup`, `PopupMenu`, `Tooltip`, `ProgressIndicator`, `Modal`, `BottomSheet`, `PlatformView`), downstream extensible `CustomWidget` trait, `GestureArena` (tap, double-tap, long-press, drag, pinch/rotate), `Spring`/`Tween` animations, `Router<R>`, keyboard focus traversal (`Tab`/`Shift+Tab`/`Escape`), dynamic slots & reactive keyed `for`, and `Ctrl+Shift+I` Inspector overlay.
- **`craftui-dsl` & `craftui-runtime`**: Lossless `rowan` + `logos` `.cui` parser supporting multi-file `import`, custom `struct` / `enum` / generic & list types, reactive `if / else` and `for item, idx in model`, two-way `<=>` bindings, and reactive `Theme.*` tokens — compiled AOT via `build.rs` or hot-reloaded in `< 100ms` dev sessions preserving live signal state.
- **`craftui-macros`**: Pure-Rust `cui!` declarative UI macro and `#[component]` attribute macro.
- **`craftui-platform-{desktop,android,ios,web}` & `craftui`**: Multi-window desktop runner with native clipboard & file picker integration, Android `GameActivity` runner with `Back`/`Enter`/D-pad + IME + external texture bridge, iOS Metal/C-ABI/`CVPixelBuffer` host, Web async `wgpu`/2D Canvas + hash routing + mobile DOM keyboard support, and unified `craftui::prelude::*` facade.
- **`craftui-cli` & `craftui-lsp`**: Developer CLI (`new`, `dev`, `preview`, `check`, `render`, `build`) and `.cui` Language Server (`tower-lsp`).

## Quick Start

```bash
# Run workspace unit & integration tests
cargo test --workspace

# Validate all .cui templates
cargo run -p craftui-cli -- check examples

# Render a headless snapshot of a .cui file
cargo run -p craftui-cli -- render examples/counter/ui/counter.cui -o preview.png
cargo run -p craftui-cli -- render examples/showcase/ui/showcase.cui -o showcase.png

# Launch flagship general-purpose examples
cargo run -p craftui-example-counter
cargo run -p craftui-example-showcase
cargo run -p craftui-example-todo
cargo run -p craftui-example-settings
cargo run -p craftui-example-canvas
```

## License

Licensed under the [MIT License](LICENSE). Zero GPL/LGPL/AGPL dependencies are permitted (enforced via `deny.toml`).
