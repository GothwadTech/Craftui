# `.cui` DSL Syntax & Markup Guide

CraftUI provides a first-class declarative language tailored for UI composition, animations, and reactivity.

---

## 1. Component Anatomy

A `.cui` file defines one or more components:

```cui
component ProfileCard {
    // 1. Reactive state variables
    state {
        let is_expanded: bool = false;
        let username: string = "CraftDev";
        let rating: float = 4.8;
    }

    // 2. Widget hierarchy
    View {
        background_color: "#1E293B";
        corner_radius: 12.0;
        padding: 16.0;

        Column {
            gap: 8.0;

            Text {
                text: username;
                font_size: 20.0;
                color: "#F8FAFC";
            }

            // 3. Conditional rendering
            if is_expanded {
                Text {
                    text: "Detailed user biography and achievements...";
                    color: "#94A3B8";
                }
            }

            // 4. Interactive widgets and handlers
            Button {
                text: is_expanded ? "Show Less" : "Read More";
                on_click: {
                    is_expanded = !is_expanded;
                }
            }
        }
    }
}
```

---

## 2. Binding Types: One-Way (`:`) vs. Two-Way (`<=>`)

### One-Way Expression Binding (`:`)
Evaluates an expression whenever the dependent signals change:
```cui
Text {
    text: "Status: " + (is_online ? "Active" : "Away");
    color: is_online ? "#10B981" : "#6B7280";
}
```

### Two-Way Reactive Binding (`<=>`)
Directly ties a UI control's input value to a mutable reactive signal:
```cui
TextInput {
    text <=> search_query;
    placeholder: "Type to search...";
}

Slider {
    value <=> audio_volume;
    min: 0.0;
    max: 100.0;
}
```

---

## 3. Keyed Reactive Loops (`for ... in ...`)

CraftUI uses fine-grained keyed item reconciliation:

```cui
Column {
    gap: 8.0;

    for item in items key: item.id {
        Row {
            Text { text: item.title; }
            Badge { text: item.status; }
        }
    }
}
```
When `items` updates, only newly added or removed nodes are touched; existing nodes retain focus and animations.

---

## 4. Declarative Animations (`animate`)

Smooth spring physics and tweens are declared directly inside widget properties:

```cui
View {
    width: is_expanded ? 320.0 : 160.0;
    opacity: is_visible ? 1.0 : 0.0;

    animate width {
        duration: 300ms;
        easing: "spring(stiffness: 180, damping: 14)";
    }

    animate opacity {
        duration: 200ms;
        easing: "ease-out";
    }
}
```

---

## 5. Visual Styling: Glassmorphism & Transforms

### Frosted Glass (`backdrop_blur`)
```cui
View {
    background_color: "#FFFFFF1A"; // 10% translucent white
    backdrop_blur: 24.0;           // Kawase dual-filter GPU blur
    border_color: "#FFFFFF33";
    border_width: 1.0;
    corner_radius: 16.0;
}
```

### 2D Transforms
```cui
View {
    transform_scale: is_pressed ? 0.96 : 1.0;
    transform_rotation: degrees(angle);
    transform_origin: (0.5, 0.5); // Center pivot

    animate transform_scale {
        duration: 150ms;
        easing: "spring(stiffness: 300, damping: 20)";
    }
}
```
