//! Standard Material 3 component definitions for CraftUI (`"std/material.cui"`).

pub const STD_MATERIAL_SOURCE: &str = r#"
export component Card {
    in property <color> background: Theme.surface;
    in property <length> border_radius: 12dp;
    in property <length> padding: 16dp;
    in property <length> width: Auto;
    in property <length> height: Auto;

    Container {
        width: root.width;
        height: root.height;
        background: root.background;
        border_radius: root.border_radius;
        padding: root.padding;
    }
}

export component ElevatedCard {
    in property <color> background: Theme.surface;
    in property <length> border_radius: 12dp;
    in property <length> padding: 16dp;
    in property <length> width: Auto;
    in property <length> height: Auto;

    Container {
        width: root.width;
        height: root.height;
        background: root.background;
        border_radius: root.border_radius;
        padding: root.padding;
        shadow: BoxShadow { blur: 8.0, offset_y: 3.0, color: #20000000 };
    }
}

export component OutlinedCard {
    in property <color> background: Theme.surface;
    in property <color> border_color: Theme.border;
    in property <length> border_radius: 12dp;
    in property <length> border_width: 1dp;
    in property <length> padding: 16dp;
    in property <length> width: Auto;
    in property <length> height: Auto;

    Container {
        width: root.width;
        height: root.height;
        background: root.background;
        border_color: root.border_color;
        border_width: root.border_width;
        border_radius: root.border_radius;
        padding: root.padding;
    }
}

export component SearchBar {
    in-out property <string> text: "";
    in property <string> placeholder: "Search...";
    in property <length> width: Auto;

    Container {
        width: root.width;
        height: 56dp;
        background: Theme.surface_variant;
        border_radius: 28dp;
        padding: 12dp;

        Row {
            gap: 12dp;
            align: Center;

            Icon {
                svg: "search";
                size: 24dp;
                color: Theme.text_secondary;
            }

            TextInput {
                text <=> root.text;
                placeholder: root.placeholder;
                width: 100%;
            }
        }
    }
}

export component FloatingActionButton {
    in property <string> icon: "add";
    in property <string> label: "";
    in property <color> background: Theme.primary_container;
    in property <color> foreground: Theme.on_primary_container;
    callback on_tap();

    Container {
        height: 56dp;
        background: root.background;
        border_radius: 16dp;
        padding: 16dp;
        shadow: BoxShadow { blur: 10.0, offset_y: 4.0, color: #30000000 };

        Row {
            gap: 8dp;
            align: Center;

            Icon {
                svg: root.icon;
                size: 24dp;
                color: root.foreground;
            }

            if root.label != "" {
                Text {
                    text: root.label;
                    color: root.foreground;
                    weight: Medium;
                }
            }
        }
    }
}

export component NavigationBar {
    in-out property <int> selected_index: 0;
    in property <[string]> items: ["Home", "Search", "Settings"];
    in property <[string]> icons: ["home", "search", "settings"];

    Container {
        width: 100%;
        height: 80dp;
        background: Theme.surface;
        border_color: Theme.border;
        border_width: 1dp;

        Row {
            gap: 16dp;
            align: Center;
            justify: SpaceAround;
            width: 100%;
            height: 100%;
        }
    }
}

export component Switch {
    in-out property <bool> checked: false;
    in property <bool> disabled: false;

    Container {
        width: 52dp;
        height: 32dp;
        border_radius: 16dp;
        background: root.checked ? Theme.primary : Theme.surface_variant;
        border_width: root.checked ? 0dp : 2dp;
        border_color: Theme.outline;

        Container {
            width: root.checked ? 24dp : 16dp;
            height: root.checked ? 24dp : 16dp;
            border_radius: 12dp;
            background: root.checked ? Theme.on_primary : Theme.outline;
        }
    }
}

export component Chip {
    in property <string> label: "";
    in property <string> icon: "";
    in-out property <bool> selected: false;
    callback on_tap();

    Container {
        height: 32dp;
        border_radius: 8dp;
        padding: 8dp;
        background: root.selected ? Theme.secondary_container : Theme.surface;
        border_width: root.selected ? 0dp : 1dp;
        border_color: Theme.border;

        Row {
            gap: 6dp;
            align: Center;

            if root.icon != "" {
                Icon {
                    svg: root.icon;
                    size: 18dp;
                    color: root.selected ? Theme.on_secondary_container : Theme.text_primary;
                }
            }

            Text {
                text: root.label;
                size: 14dp;
                color: root.selected ? Theme.on_secondary_container : Theme.text_primary;
            }
        }
    }
}

export component BottomSheet {
    in-out property <bool> open: false;
    in property <length> max_height: 400dp;

    if root.open {
        Container {
            width: 100%;
            height: root.max_height;
            background: Theme.surface;
            border_radius: 28dp;
            padding: 16dp;
            shadow: BoxShadow { blur: 24.0, offset_y: -4.0, color: #40000000 };

            Column {
                gap: 16dp;
                align: Center;

                // M3 Drag handle
                Container {
                    width: 32dp;
                    height: 4dp;
                    border_radius: 2dp;
                    background: Theme.border;
                }
            }
        }
    }
}

export component StateLayer {
    in property <color> color: Theme.primary;
    in-out property <bool> hovered: false;
    in-out property <bool> pressed: false;

    Container {
        background: root.pressed ? root.color : (root.hovered ? root.color : #00000000);
        opacity: root.pressed ? 0.12 : (root.hovered ? 0.08 : 0.0);
    }
}

export component Ripple {
    in property <color> color: Theme.primary;
    in-out property <bool> active: false;

    Container {
        background: root.active ? root.color : #00000000;
        opacity: root.active ? 0.12 : 0.0;
    }
}
"#;

pub const STD_MATERIAL_EXPORTS: &[&str] = &[
    "Card",
    "ElevatedCard",
    "OutlinedCard",
    "SearchBar",
    "FloatingActionButton",
    "NavigationBar",
    "Switch",
    "Chip",
    "BottomSheet",
    "StateLayer",
    "Ripple",
];
