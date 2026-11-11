# Quickstart: Your First CraftUI Application

Let's build a cross-platform application in less than 2 minutes using `craftui-cli`.

---

## 1. Scaffolding a New Project

Use the `craftui new` command:

```bash
craftui new my_app --template mobile
cd my_app
```

This creates a clean project layout:
```
my_app/
├── Cargo.toml          # Rust dependencies & build settings
├── craftui.toml        # Application manifest & mobile permissions
├── build.rs            # AOT .cui template compiler
├── src/
│   └── main.rs         # Entrypoint & reactive logic
└── ui/
    └── app.cui         # Declarative UI markup
```

---

## 2. Inspecting the Markup (`ui/app.cui`)

CraftUI uses the `.cui` declarative markup syntax:

```cui
component MyApp {
    state {
        let count: int = 0;
        let name: string = "Explorer";
    }

    SafeArea {
        Column {
            padding: 24.0;
            gap: 16.0;
            align_items: "center";

            Text {
                text: "Welcome, " + name + "!";
                font_size: 24.0;
                color: "#FFFFFF";
                font_weight: "bold";
            }

            Text {
                text: "Current Count: " + count;
                font_size: 18.0;
                color: "#60A5FA";
            }

            Row {
                gap: 12.0;

                Button {
                    text: "Increment";
                    background_color: "#2563EB";
                    on_click: {
                        count += 1;
                    }
                }

                Button {
                    text: "Reset";
                    background_color: "#4B5563";
                    on_click: {
                        count = 0;
                    }
                }
            }
        }
    }
}
```

---

## 3. Running the App

### On Linux Desktop
```bash
cargo run
# Or with hot-reload enabled:
craftui run --platform desktop
```

### On Connected Android Phone
```bash
craftui run --platform android
```
`craftui-cli` will cross-compile the Rust library with NDK clang, bundle the APK, install it via ADB, and launch the activity immediately!

### In the Web Browser
```bash
craftui run --platform web
```
`craftui-cli` launches a local zero-dependency HTTP server with correct `application/wasm` MIME headers and opens your browser to `http://localhost:8080`.
