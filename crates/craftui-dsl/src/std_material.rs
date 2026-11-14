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

export component FilledCard {
    in property <color> background: Theme.surface_variant;
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

export component FilledButton {
    in property <string> text: "";
    in property <string> icon: "";
    in property <bool> disabled: false;
    in property <color> background: Theme.primary;
    in property <color> foreground: Theme.on_primary;
    callback on_tap();

    Button {
        text: root.text;
        background: root.background;
        foreground: root.foreground;
        border_radius: 20dp;
        padding_x: 24dp;
        padding_y: 10dp;
        on_tap => { if !root.disabled : root.on_tap(); }
    }
}

export component ElevatedButton {
    in property <string> text: "";
    in property <string> icon: "";
    in property <bool> disabled: false;
    in property <color> background: Theme.surface;
    in property <color> foreground: Theme.primary;
    callback on_tap();

    Button {
        text: root.text;
        background: root.background;
        foreground: root.foreground;
        border_radius: 20dp;
        padding_x: 24dp;
        padding_y: 10dp;
        shadow: BoxShadow { blur: 6.0, offset_y: 2.0, color: #20000000 };
        on_tap => { if !root.disabled : root.on_tap(); }
    }
}

export component TonalButton {
    in property <string> text: "";
    in property <string> icon: "";
    in property <bool> disabled: false;
    in property <color> background: Theme.secondary_container;
    in property <color> foreground: Theme.on_secondary_container;
    callback on_tap();

    Button {
        text: root.text;
        background: root.background;
        foreground: root.foreground;
        border_radius: 20dp;
        padding_x: 24dp;
        padding_y: 10dp;
        on_tap => { if !root.disabled : root.on_tap(); }
    }
}

export component OutlinedButton {
    in property <string> text: "";
    in property <string> icon: "";
    in property <bool> disabled: false;
    in property <color> background: #00000000;
    in property <color> foreground: Theme.primary;
    in property <color> border_color: Theme.outline;
    callback on_tap();

    Button {
        text: root.text;
        background: root.background;
        foreground: root.foreground;
        border_color: root.border_color;
        border_width: 1dp;
        border_radius: 20dp;
        padding_x: 24dp;
        padding_y: 10dp;
        on_tap => { if !root.disabled : root.on_tap(); }
    }
}

export component TextButton {
    in property <string> text: "";
    in property <string> icon: "";
    in property <bool> disabled: false;
    in property <color> background: #00000000;
    in property <color> foreground: Theme.primary;
    callback on_tap();

    Button {
        text: root.text;
        background: root.background;
        foreground: root.foreground;
        border_radius: 20dp;
        padding_x: 12dp;
        padding_y: 10dp;
        on_tap => { if !root.disabled : root.on_tap(); }
    }
}

export component IconButton {
    in property <string> icon: "";
    in property <string> svg: "";
    in property <color> color: Theme.on_surface_variant;
    in property <color> background: #00000000;
    in property <length> size: 40dp;
    callback on_tap();

    Button {
        text: root.icon != "" ? root.icon : root.svg;
        foreground: root.color;
        background: root.background;
        border_radius: 20dp;
        width: root.size;
        height: root.size;
        padding: 8dp;
        on_tap => { root.on_tap(); }
    }
}

export component FilledIconButton {
    in property <string> icon: "";
    in property <string> svg: "";
    in property <color> color: Theme.on_primary;
    in property <color> background: Theme.primary;
    in property <length> size: 40dp;
    callback on_tap();

    Button {
        text: root.icon != "" ? root.icon : root.svg;
        foreground: root.color;
        background: root.background;
        border_radius: 20dp;
        width: root.size;
        height: root.size;
        padding: 8dp;
        on_tap => { root.on_tap(); }
    }
}

export component TonalIconButton {
    in property <string> icon: "";
    in property <string> svg: "";
    in property <color> color: Theme.on_secondary_container;
    in property <color> background: Theme.secondary_container;
    in property <length> size: 40dp;
    callback on_tap();

    Button {
        text: root.icon != "" ? root.icon : root.svg;
        foreground: root.color;
        background: root.background;
        border_radius: 20dp;
        width: root.size;
        height: root.size;
        padding: 8dp;
        on_tap => { root.on_tap(); }
    }
}

export component OutlinedIconButton {
    in property <string> icon: "";
    in property <string> svg: "";
    in property <color> color: Theme.on_surface_variant;
    in property <color> border_color: Theme.outline;
    in property <length> size: 40dp;
    callback on_tap();

    Button {
        text: root.icon != "" ? root.icon : root.svg;
        foreground: root.color;
        background: #00000000;
        border_color: root.border_color;
        border_width: 1dp;
        border_radius: 20dp;
        width: root.size;
        height: root.size;
        padding: 8dp;
        on_tap => { root.on_tap(); }
    }
}

export component FloatingActionButton {
    in property <string> icon: "add";
    in property <string> label: "";
    in property <color> background: Theme.primary_container;
    in property <color> foreground: Theme.on_primary_container;
    callback on_tap();

    Container {
        on_tap => { root.on_tap(); }
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

            if root.label != "" : Text {
                text: root.label;
                color: root.foreground;
                weight: Medium;
            }
        }
    }
}

export component Chip {
    in property <string> label: "";
    in property <string> icon: "";
    in-out property <bool> selected: false;
    callback on_tap();

    Container {
        on_tap => {
            root.selected = !root.selected;
            root.on_tap();
        }
        height: 32dp;
        border_radius: 8dp;
        padding_x: 12dp;
        padding_y: 6dp;
        background: root.selected ? Theme.secondary_container : Theme.surface;
        border_width: root.selected ? 0dp : 1dp;
        border_color: Theme.border;

        Row {
            gap: 6dp;
            align: Center;

            if root.selected : Icon {
                svg: "check";
                size: 16dp;
                color: Theme.on_secondary_container;
            }

            if !root.selected && root.icon != "" : Icon {
                svg: root.icon;
                size: 16dp;
                color: Theme.text_secondary;
            }

            Text {
                text: root.label;
                size: 14dp;
                color: root.selected ? Theme.on_secondary_container : Theme.text_primary;
            }
        }
    }
}

export component FilterChip {
    in property <string> label: "";
    in property <string> icon: "";
    in-out property <bool> selected: false;
    callback on_tap();

    Container {
        on_tap => {
            root.selected = !root.selected;
            root.on_tap();
        }
        height: 32dp;
        border_radius: 8dp;
        padding_x: 12dp;
        padding_y: 6dp;
        background: root.selected ? Theme.secondary_container : Theme.surface;
        border_width: root.selected ? 0dp : 1dp;
        border_color: Theme.border;

        Row {
            gap: 6dp;
            align: Center;

            if root.selected : Icon {
                svg: "check";
                size: 16dp;
                color: Theme.on_secondary_container;
            }

            if !root.selected && root.icon != "" : Icon {
                svg: root.icon;
                size: 16dp;
                color: Theme.text_secondary;
            }

            Text {
                text: root.label;
                size: 14dp;
                color: root.selected ? Theme.on_secondary_container : Theme.text_primary;
            }
        }
    }
}

export component ActionChip {
    in property <string> label: "";
    in property <string> icon: "";
    callback on_tap();

    Container {
        on_tap => { root.on_tap(); }
        height: 32dp;
        border_radius: 8dp;
        padding_x: 12dp;
        padding_y: 6dp;
        background: Theme.surface;
        border_width: 1dp;
        border_color: Theme.border;

        Row {
            gap: 6dp;
            align: Center;

            if root.icon != "" : Icon {
                svg: root.icon;
                size: 16dp;
                color: Theme.primary;
            }

            Text {
                text: root.label;
                size: 14dp;
                color: Theme.text_primary;
            }
        }
    }
}

export component ChoiceChip {
    in property <string> label: "";
    in-out property <bool> selected: false;
    callback on_tap();

    Container {
        on_tap => {
            root.selected = !root.selected;
            root.on_tap();
        }
        height: 32dp;
        border_radius: 8dp;
        padding_x: 12dp;
        padding_y: 6dp;
        background: root.selected ? Theme.primary_container : Theme.surface;
        border_width: root.selected ? 0dp : 1dp;
        border_color: Theme.border;

        Row {
            gap: 6dp;
            align: Center;

            if root.selected : Icon {
                svg: "check";
                size: 16dp;
                color: Theme.on_primary_container;
            }

            Text {
                text: root.label;
                size: 14dp;
                color: root.selected ? Theme.on_primary_container : Theme.text_primary;
            }
        }
    }
}

export component InputChip {
    in property <string> label: "";
    in property <string> icon: "";
    callback on_tap();
    callback on_delete();

    Container {
        on_tap => { root.on_tap(); }
        height: 32dp;
        border_radius: 8dp;
        padding_x: 10dp;
        padding_y: 6dp;
        background: Theme.surface;
        border_width: 1dp;
        border_color: Theme.border;

        Row {
            gap: 6dp;
            align: Center;

            if root.icon != "" : Icon {
                svg: root.icon;
                size: 16dp;
                color: Theme.text_secondary;
            }

            Text {
                text: root.label;
                size: 14dp;
                color: Theme.text_primary;
            }

            Button {
                text: "close";
                font_size: 12;
                background: #00000000;
                foreground: Theme.text_secondary;
                padding_x: 4dp;
                padding_y: 2dp;
                on_tap => { root.on_delete(); }
            }
        }
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

            Container {
                width: 100%;
                height: 100%;
            }
        }
    }
}

export component Switch {
    in-out property <bool> checked: false;
    in property <bool> disabled: false;

    Container {
        on_tap => {
            if !root.disabled : root.checked = !root.checked;
        }
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

export component CheckBoxTile {
    in-out property <bool> checked: false;
    in property <string> title: "";
    in property <string> subtitle: "";

    Container {
        on_tap => { root.checked = !root.checked; }
        width: 100%;
        padding: 12dp;

        Row {
            gap: 12dp;
            align: Center;

            Checkbox {
                checked <=> root.checked;
            }

            Column {
                gap: 2dp;
                Text {
                    text: root.title;
                    font_size: 15;
                    color: Theme.text_primary;
                }
                if root.subtitle != "" : Text {
                    text: root.subtitle;
                    font_size: 12;
                    color: Theme.text_secondary;
                }
            }
        }
    }
}

export component RadioButtonTile {
    in property <string> value: "";
    in-out property <string> selected_value: "";
    in property <string> title: "";

    Container {
        on_tap => { root.selected_value = root.value; }
        width: 100%;
        padding: 12dp;

        Row {
            gap: 12dp;
            align: Center;

            Container {
                width: 20dp;
                height: 20dp;
                border_radius: 10dp;
                border_width: 2dp;
                border_color: root.selected_value == root.value ? Theme.primary : Theme.outline;
                align: Center;
                justify: Center;

                if root.selected_value == root.value : Container {
                    width: 10dp;
                    height: 10dp;
                    border_radius: 5dp;
                    background: Theme.primary;
                }
            }

            Text {
                text: root.title;
                font_size: 15;
                color: Theme.text_primary;
            }
        }
    }
}

export component SnackBar {
    in property <string> message: "";
    in property <string> action_label: "";
    in-out property <bool> open: false;
    callback on_action();

    Container {
        visible: root.open;
        width: 100%;
        background: Theme.inverse_surface;
        border_radius: 8dp;
        padding_x: 16dp;
        padding_y: 14dp;

        Row {
            justify: SpaceBetween;
            align: Center;
            width: 100%;

            Text {
                text: root.message;
                color: Theme.inverse_on_surface;
                font_size: 14;
            }

            if root.action_label != "" : Button {
                text: root.action_label;
                background: #00000000;
                foreground: Theme.inverse_primary;
                on_tap => { root.on_action(); }
            }
        }
    }
}

export component Dialog {
    in-out property <bool> open: false;
    in property <string> title: "";
    in property <string> message: "";
    in property <string> confirm_text: "OK";
    in property <string> cancel_text: "Cancel";
    callback on_confirm();
    callback on_cancel();

    Container {
        visible: root.open;
        width: 320dp;
        background: Theme.surface;
        border_radius: 28dp;
        padding: 24dp;
        shadow: BoxShadow { blur: 24.0, offset_y: 8.0, color: #40000000 };

        Column {
            gap: 16dp;

            if root.title != "" : Text {
                text: root.title;
                font_size: 20;
                font_weight: Bold;
                color: Theme.text_primary;
            }

            if root.message != "" : Text {
                text: root.message;
                font_size: 14;
                color: Theme.text_secondary;
            }

            Row {
                justify: End;
                gap: 8dp;
                width: 100%;

                if root.cancel_text != "" : TextButton {
                    text: root.cancel_text;
                    on_tap => {
                        root.open = false;
                        root.on_cancel();
                    }
                }

                FilledButton {
                    text: root.confirm_text;
                    on_tap => {
                        root.open = false;
                        root.on_confirm();
                    }
                }
            }
        }
    }
}

export component CircularProgressIndicator {
    in property <length> size: 36dp;
    in property <color> color: Theme.primary;

    Spinner {
        size: root.size;
        color: root.color;
    }
}

export component LinearProgressIndicator {
    in property <float> value: 0.5;
    in property <color> color: Theme.primary;

    ProgressIndicator {
        progress: root.value;
    }
}

export component HorizontalDivider {
    in property <length> thickness: 1dp;
    in property <color> color: Theme.border;

    Divider {
        direction: Horizontal;
        thickness: root.thickness;
        color: root.color;
    }
}

export component VerticalDivider {
    in property <length> thickness: 1dp;
    in property <color> color: Theme.border;

    Divider {
        direction: Vertical;
        thickness: root.thickness;
        color: root.color;
    }
}

export component BottomSheet {
    in-out property <bool> open: false;
    in property <length> max_height: 400dp;

    Container {
        visible: root.open;
        width: 100%;
        height: root.open ? root.max_height : 0dp;
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
    "FilledCard",
    "OutlinedCard",
    "FilledButton",
    "ElevatedButton",
    "TonalButton",
    "OutlinedButton",
    "TextButton",
    "IconButton",
    "FilledIconButton",
    "TonalIconButton",
    "OutlinedIconButton",
    "FloatingActionButton",
    "Chip",
    "FilterChip",
    "ActionChip",
    "ChoiceChip",
    "InputChip",
    "SearchBar",
    "NavigationBar",
    "Switch",
    "CheckBoxTile",
    "RadioButtonTile",
    "SnackBar",
    "Dialog",
    "CircularProgressIndicator",
    "LinearProgressIndicator",
    "HorizontalDivider",
    "VerticalDivider",
    "BottomSheet",
    "StateLayer",
    "Ripple",
];
