# CraftUI — DSL, Reactivity & Design System Specification (`DESIGN.md`)

## 1. Ergonomics Philosophy

Writing UI in Rust usually suffers from two pain points:
1. **Closure / Lifetime Hell** (`let state = state.clone();` repeated 10 times per screen).
2. **Slow Compile Cycles** when tweaking `padding: 12px` vs `padding: 16px`.

CraftUI solves both using:
* **Generational `Copy` Signal Handles** (no `.clone()` in event handlers).
* **Dual Syntax:** Clean `.cui` declarative files (hot-reloaded in <100ms in dev, AOT-compiled in release) **plus** an optional `cui!` Rust macro.

---

## 2. `.cui` Declarative DSL Specification

### 2.1 Example: ScanCraft Filter Studio Screen in `.cui`

```slint
// Note: Clean declarative syntax (.cui) compiled to static Rust in release builds
import { Column, Row, Button, Slider, ImageView, ScrollView } from "std/widgets.cui"
import { Theme } from "theme/tokens.cui"

export component FilterStudio {
    // Reactive inputs & outputs bound directly to Rust structs
    in property <TextureHandle> preview_texture;
    in-out property <int> selected_filter: 0;
    in-out property <float> brightness: 0.0;
    in-out property <float> contrast: 0.0;

    callback on_filter_changed(int);
    callback on_apply_clicked();

    Column {
        width: 100%;
        height: 100%;
        background: Theme.background;
        padding: SafeArea.insets;

        // Top Preview Canvas (Zero-copy GPU texture)
        ImageView {
            flex_grow: 1.0;
            source: preview_texture;
            fit: Contain;
            border_radius: 12dp;
            margin: 16dp;
        }

        // Manual Tone Sliders
        Column {
            gap: 8dp;
            padding_x: 16dp;

            SliderRow {
                label: "Brightness";
                value <=> brightness;
                min: -100.0;
                max: 100.0;
            }
            SliderRow {
                label: "Contrast";
                value <=> contrast;
                min: -100.0;
                max: 100.0;
            }
        }

        // Horizontal Filter Strip
        ScrollView {
            direction: Horizontal;
            height: 88dp;
            Row {
                gap: 10dp;
                padding_x: 16dp;
                for item, idx in ["Original", "Magic Color", "B&W", "No Shadow", "Warm"] : FilterChip {
                    title: item;
                    active: selected_filter == idx;
                    on_tap => {
                        selected_filter = idx;
                        on_filter_changed(idx);
                    }
                }
            }
        }

        // Action Bar
        Row {
            padding: 16dp;
            justify: End;
            Button {
                text: "Save Scan";
                background: Theme.primary;
                foreground: #FFFFFF;
                border_radius: 24dp;
                on_tap => { on_apply_clicked(); }
            }
        }
    }
}
```

### 2.2 Pure-Rust `cui!` Macro Equivalent (Zero Separate File Format)

For developers who prefer staying 100% inside `.rs` files with native `rust-analyzer` type checking:

```rust
use craftui::prelude::*;

#[component]
pub fn FilterStudio(cx: Scope, preview: ReadSignal<TextureHandle>) -> impl View {
    let brightness = cx.signal(0.0f32);
    let selected = cx.signal(0usize);

    cui! {
        Column(width: pct(100), height: pct(100), bg: theme.background) {
            ImageView(source: preview, flex_grow: 1.0, fit: Fit::Contain)
            Slider(value: brightness, range: -100.0..=100.0)
            Button(
                label: "Save Scan",
                bg: theme.primary,
                on_tap: move || {
                    // Notice: `brightness` & `selected` are Copy! No .clone() needed!
                    log::info!("Saving with filter={}, brightness={}", selected.get(), brightness.get());
                }
            )
        }
    }
}
```

---

## 3. Fine-Grained Signal API Design (`craftui-reactive`)

### 3.1 Core Primitives
* `Signal<T>`: Copy handle (`u32 index + u32 generation`) into the thread-local arena.
  - `.get()` → Clones `T` and registers active dependency edge.
  - `.with(|v| ...)` → Zero-copy borrow read + registers dependency edge.
  - `.set(val)` → Updates `T`, compares `PartialEq` (if `T: PartialEq` via optional `set_if_changed`), schedules dirty propagation.
  - `.update(|v| ...)` → Mutates `T` in place cleanly.
* `Computed<T>`: Lazy memoized derived value; recalculates *only* when an upstream dependency changed and someone reads it.
* `Effect`: Side-effect runner bound to `Scope`; automatically tracks any `.get()` / `.with()` calls inside its closure and re-runs when dependencies change.

### 3.2 Memory Lifecycle Rules
* Every component creates a child `ScopeId`.
* When a conditional branch (`if show_dialog`) flips to `false`, `dispose_scope(child_scope)` frees all child signals, textures, effects, and `taffy` layout nodes in `O(children)` time.
* Stale handles accessed after disposal fail safely (returning `Option` via generational check, or panicking clearly in debug builds with source location).

---

## 4. Design Token & Theme System

CraftUI models theming as a reactive **Scoped Token Context** (`ThemeScope`), enabling zero-flicker instant toggling between Light, Dark, AMOLED Black, and Custom Brand palettes:

```rust
pub struct ThemeTokens {
    pub background: Color,         // e.g. #000000 (AMOLED) or #F8F9FA (Paper Light)
    pub surface: Color,
    pub surface_variant: Color,
    pub border: Color,
    pub primary: Color,            // e.g. #00BA66
    pub on_primary: Color,
    pub text_primary: Color,
    pub text_secondary: Color,
    pub danger: Color,
    pub radius_md: f32,
    pub spacing_unit: f32,
}
```

* Changing `active_theme.set(ThemeMode::Dark)` updates only nodes subscribed to `ThemeTokens` paint properties (`DIRTY_PAINT`), **without triggering a full layout recalculation**!

---

## 5. Gesture Arena & Mobile Touch Physics

To compete with Jetpack Compose and Flutter on mobile, touch handling cannot be a basic mouse-click wrapper:

1. **Disambiguation Arena:**
   - Resolves competing gestures cleanly (e.g., vertical `ScrollView` vs horizontal swipe vs 2-finger `PinchZoom` vs draggable crop corner handle).
2. **Physics Curves:**
   - Spring-based overshoot / deceleration (`VelocityTracker` + exponential decay fling) calibrated to platform feel (Android/iOS scroll friction).
3. **Hit-Testing:**
   - Supports expanded invisible touch targets (`hit_slop: 16dp`) so small visual handles (like an 8-point crop circle) feel effortless on touchscreens.

---

## 6. Custom Vector `Canvas` & High-Speed `PixelBuffer` API

Essential for creative tools, scanners, charts, and signature pads:

1. **Declarative `Canvas` Node:**
   - Exposes `vello::kurbo` paths (`MoveTo`, `LineTo`, `CubicTo`), variable-width strokes, gradients, and clip layers directly in Rust with sub-pixel GPU anti-aliasing.
2. **Mutable `PixelBufferHandle`:**
   - Allocates a shared RGBA8 pixel surface mapped cleanly to `wgpu::Texture`.
   - Worker threads (`rayon`) compute pixel filters in parallel; once done, calling `handle.commit_damage(rect)` uploads *only* the modified region to GPU without re-allocating memory.
