# Comprehensive Widget Catalog

CraftUI ships with a comprehensive set of 25+ built-in widgets covering layout, navigation, inputs, data visualization, and native mobile insets.

---

## 1. Structural & Layout Containers

### `View`
The fundamental container for background styling, borders, shadows, and clipping.
```cui
View {
    background_color: "#1E293B";
    corner_radius: 8.0;
    padding: 16.0;
    border_width: 1.0;
    border_color: "#334155";
}
```

### `Row` and `Column`
Flexbox flex-direction wrappers with spacing control.
```cui
Row {
    gap: 12.0;
    align_items: "center";
    justify_content: "space-between";
}
```

### `SafeArea`
Automatically applies insets for mobile notches, status bars, and on-screen software keyboards.
```cui
SafeArea {
    top: true;
    bottom: true;
    left: true;
    right: true;
    // child container...
}
```

### `SplitPane`
A resizable dual-pane container with interactive drag handle and min/max clamping.
```cui
SplitPane {
    orientation: "horizontal";
    split_ratio: 0.35;
    min_size: 150.0;
}
```

---

## 2. Text & Typography

### `Text`
High-performance shaped text supporting Latin, CJK, and complex scripts with fallback font embedding.
```cui
Text {
    text: "CraftUI High-DPI Text";
    font_size: 16.0;
    font_weight: "medium";
    color: "#F8FAFC";
    line_height: 1.4;
}
```

---

## 3. Interactive Controls & Buttons

### `Button`
Clickable button with hover, pressed states, keyboard focus ring, and composable child subtrees.
```cui
Button {
    text: "Submit Order";
    variant: "primary";
    disabled: false;
    on_click: {
        submit_order();
    }
}
```

### `SegmentedControl`
Sliding pill multi-option selector with spring animations and arrow key navigation.
```cui
SegmentedControl {
    options: ["All", "Pending", "Completed"];
    selected_index <=> current_filter;
}
```

### `TextInput`
Single-line or multi-line text field with selection, cursor blinking, undo/redo, and native IME support.
```cui
TextInput {
    text <=> search_term;
    placeholder: "Search documents...";
}
```

### `Slider`
Continuous or stepped slider.
```cui
Slider {
    value <=> brightness;
    min: 0.0;
    max: 100.0;
    step: 1.0;
}
```

### `Switch` and `Checkbox`
Reactive boolean toggles.
```cui
Switch {
    checked <=> dark_mode_enabled;
}
```

---

## 4. Collections & Data Visualization

### `DataTable`
Sortable, striped, selectable tabular data container.
```cui
DataTable {
    columns: ["ID", "Name", "Date", "Size"];
    data <=> document_rows;
    selected_row <=> active_row_index;
    striped: true;
}
```

### `VirtualList`
High-performance windowed recycling for lists with 10,000+ items at 120 FPS.
```cui
VirtualList {
    item_count: 10000;
    item_height: 48.0;
}
```

### `Accordion`
Collapsible disclosure panel with smooth spring height transitions and accessibility semantics.
```cui
Accordion {
    title: "Advanced System Settings";
    is_expanded <=> settings_open;
    // Collapsible content...
}
```

### `Badge` and `Spinner`
Status pills and animated indeterminate loading spinners.
```cui
Badge {
    text: "PROD";
    color: "#10B981";
}

Spinner {
    size: 24.0;
    stroke_width: 3.0;
    color: "#3B82F6";
}
```

---

## 5. Custom Graphics & Canvas

### `Canvas`
Direct 2D path rendering, shapes, gradients, and custom image transformations.
```cui
Canvas {
    width: 400.0;
    height: 300.0;
    on_draw: "paint_canvas_handler";
}
```
In your Rust controller, the `on_draw` callback receives a `CanvasContext` for drawing lines, arcs, bezier curves, images, and text.
