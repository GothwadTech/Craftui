//! `.cui` declarative UI DSL parser (`logos` + lossless `rowan` CST),
//! multi-file module resolver, semantic validator, and static Rust AOT codegen.

pub mod ast;
pub mod codegen;
pub mod cst;
pub mod lexer;
pub mod parser;
pub mod passes;
pub mod resolver;
pub mod validator;

pub use ast::{
    AnimateSpec, AssignOp, BinOp, CallbackDecl, ChildItem, ComponentDecl, CuiFile, Diagnostic,
    ElementNode, EnumDecl, Expr, HandlerBinding, ImportDecl, PropBinding, PropertyDecl,
    PropertyDir, Severity, Span, StateDecl, Stmt, StructDecl, StructField, TransitionSpec,
    TypeExpr, Unit,
};
pub use codegen::{compile_cui_dir, compile_cui_file, generate_rust};
pub use cst::{CuiLanguage, SyntaxKind, SyntaxNode, SyntaxToken};
pub use lexer::{Token, TokenKind, tokenize};
pub use parser::{ParseResult, parse};
pub use resolver::{
    FsModuleLoader, MemoryModuleLoader, ModuleLoader, ResolvedCuiUnit, is_virtual_module,
    resolve_file, resolve_source, resolve_source_with_loader,
};
pub use validator::{
    ANIMATABLE_PROPS, BUILTIN_EVENTS, BUILTIN_PROPS, BUILTIN_WIDGETS, events_for_widget,
    props_for_widget, validate, validate_file_with_symbols, widget_allows_children,
};

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    const SAMPLE_CUI: &str = r#"
import { Column, Row, Button, Slider, ImageView, ScrollView } from "std/widgets.cui"
import { Theme } from "theme/tokens.cui"

export struct TaskItem {
    id: int = 0,
    title: string = "",
    done: bool = false,
}

export enum TaskFilter {
    All,
    Active,
    Completed,
}

export component TaskBoard {
    in property <Option<TextureHandle>> banner;
    in property <[TaskItem]> items: [];
    in-out property <int> active_filter: 0;
    in-out property <float> zoom: 1.0;
    in-out property <bool> show_modal: false;
    callback on_filter_changed(int);
    callback on_submit_clicked();

    Column {
        width: 100%;
        height: 100%;
        background: Theme.background;
        padding: SafeArea.insets;

        ImageView {
            flex_grow: 1.0;
            source: banner;
            fit: Contain;
            border_radius: 12dp;
            margin: 16dp;
        }

        ScrollView {
            direction: Horizontal;
            height: 88dp;
            Row {
                gap: 10dp;
                for item, idx in ["All", "Active", "Done"] : Button {
                    text: item;
                    on_tap => {
                        active_filter = idx;
                        on_filter_changed(idx);
                    }
                }
            }
        }

        if show_modal : Modal {
            open <=> show_modal;
            Text { text: "Confirm action"; color: Theme.text_primary; }
        } else : Text {
            text: "Ready";
            color: Theme.text_secondary;
        }

        Row {
            padding: 16dp;
            justify: End;
            Button {
                text: "Save";
                background: Theme.primary;
                foreground: #FFFFFF;
                border_radius: 24dp;
                on_tap => { on_submit_clicked(); }
            }
        }
    }
}
"#;

    #[test]
    fn parses_lossless_cst_and_validates_and_generates_rust() {
        let parsed = parse(SAMPLE_CUI);
        assert!(
            parsed.diagnostics.is_empty(),
            "unexpected parse diagnostics: {:?}",
            parsed.diagnostics
        );
        // Lossless round-trip check
        assert_eq!(parsed.cst.text().to_string(), SAMPLE_CUI);
        assert_eq!(parsed.file.imports.len(), 2);
        assert_eq!(parsed.file.structs.len(), 1);
        assert_eq!(parsed.file.enums.len(), 1);
        assert_eq!(parsed.file.components.len(), 1);
        assert_eq!(parsed.file.components[0].name, "TaskBoard");
        assert_eq!(parsed.file.components[0].properties.len(), 5);
        assert_eq!(parsed.file.components[0].callbacks.len(), 2);

        let sem_diags = validate(&parsed.file);
        assert!(sem_diags.is_empty(), "semantic diags: {sem_diags:?}");

        let rust_code = generate_rust(&parsed.file);
        assert!(rust_code.contains("pub struct TaskItem"));
        assert!(rust_code.contains("pub enum TaskFilter"));
        assert!(rust_code.contains("pub struct TaskBoard"));
        assert!(rust_code.contains("pub zoom: Signal<f32>"));
        assert!(rust_code.contains("pub items: Signal<Vec<TaskItem>>"));
        assert!(rust_code.contains("dynamic_slot"));
        assert!(rust_code.contains("reactive_for"));
        assert!(rust_code.contains("__theme_sig.get().background"));
    }

    #[test]
    fn resolves_multifile_imports_merges_definitions_and_detects_cycles() {
        let mut loader = MemoryModuleLoader::new();
        loader.insert(
            Path::new("/app/ui/models.cui"),
            r#"
export struct BadgeStyle {
    title: string = "Tag",
}
export component TagChip {
    in property <string> label: "New";
    in-out property <bool> selected: false;
    callback on_badge_tap();
    Button {
        text: label;
        on_tap => {
            selected = !selected;
            on_badge_tap();
        }
    }
}
"#,
        );
        let main_src = r#"
import { BadgeStyle, TagChip } from "./models.cui"
export component App {
    in-out property <bool> pinned: false;
    Column {
        TagChip { label: "v1"; selected <=> pinned; }
        for n in 0..=3 : Text { text: n; }
    }
}
"#;
        let resolved = resolve_source_with_loader(main_src, Path::new("/app/ui/main.cui"), &loader);
        assert!(
            resolved.diagnostics.is_empty(),
            "resolve diags: {:?}",
            resolved.diagnostics
        );
        assert_eq!(resolved.merged.structs.len(), 1);
        assert_eq!(resolved.merged.components.len(), 2);
        let code = generate_rust(&resolved.merged);
        assert!(code.contains("pub struct BadgeStyle"));
        assert!(code.contains("pub struct TagChip"));
        assert!(code.contains("__sub.selected = self.pinned;"));
        assert!(code.contains("(0i32)..=(3i32)"));

        // Circular import check
        let mut cyc = MemoryModuleLoader::new();
        cyc.insert(
            Path::new("/a.cui"),
            "import { B } from \"b.cui\"\nexport component A {}",
        );
        cyc.insert(
            Path::new("/b.cui"),
            "import { A } from \"a.cui\"\nexport component B {}",
        );
        let cyc_res = resolve_source_with_loader(
            "import { B } from \"b.cui\"\nexport component A {}",
            Path::new("/a.cui"),
            &cyc,
        );
        assert!(
            cyc_res
                .diagnostics
                .iter()
                .any(|d| d.severity == Severity::Error && d.message.contains("circular import"))
        );
    }

    #[test]
    fn phase3_cst_roundtrip_animate_ternary_strict_diagnostics_and_codegen() {
        let src = r#"export struct TodoRow {
    id: int = 0,
    title: string = "",
    done: bool = false,
}

export component ChildBadge {
    in property <string> label: "Tag";
    in-out property <bool> active: false;
    callback toggled(bool);
    Row {
        id: "badge_row";
        Button {
            on_tap => { active = !active; toggled(active); }
            Text { text: label; }
        }
    }
}

export component Phase3Demo {
    in-out property <[TodoRow]> rows: [];
    in-out property <int> score: 10;
    in-out property <bool> highlighted: false;

    Column {
        opacity: highlighted ? 1.0 : 0.65;
        scale: highlighted ? 1.05 : 1.0;
        background: highlighted ? #2563EB : #1E293B;
        backdrop_blur: 16dp;
        cursor: Pointer;
        shadow: [0dp, 4dp, 12dp, #00000044, 0dp, 1dp, 2dp, #FFFFFF22];
        animate opacity { duration: 180ms; easing: EaseOut; }
        animate scale { curve: snappy(); }

        ChildBadge {
            label: highlighted ? "Active" : "Idle";
            active: highlighted;
        }

        for row, idx in rows key: row.id : Button {
            on_tap => {
                if highlighted {
                    rows[idx].done = !rows[idx].done;
                    score += 5;
                } else {
                    score /= 2;
                }
            }
            Text { text: row.title; font_size: highlighted ? 16 : 14; }
        }
    }
}
"#;
        let parsed = parse(src);
        assert!(
            parsed.diagnostics.is_empty(),
            "parse diags: {:?}",
            parsed.diagnostics
        );
        assert_eq!(
            parsed.cst.text().to_string(),
            src,
            "lossless CST round-trip failed"
        );

        let sem_diags = validate(&parsed.file);
        assert!(sem_diags.is_empty(), "semantic diags: {sem_diags:?}");

        let rust_code = generate_rust(&parsed.file);
        // Keyed reactive_for uses reactive_for_keyed without "{__k_idx}:" prefix
        assert!(rust_code.contains("reactive_for_keyed"));
        assert!(!rust_code.contains("{__k_idx}:"));
        // Strict `:` vs `<=>`: ChildBadge `active: highlighted;` must emit one-way effect, NOT `__sub.active = self.highlighted;`
        assert!(!rust_code.contains("__sub.active = self.highlighted;"));
        // Compound + nested lvalue update & zero-division guard
        assert!(rust_code.contains("__root.get_mut(__lidx_0)"));
        assert!(rust_code.contains(".done"));
        assert!(rust_code.contains("__rhs != 0"));
        // Button with children and no `text:` prop defaults label to empty string ""
        assert!(rust_code.contains("ReactiveValue::Static(String::new())"));
        assert!(!rust_code.contains("\"Button\".to_string()"));
        // animate transitions MUST be attached AFTER initial opacity & initial bind_dynamic_prop!
        let bind_op_pos = rust_code
            .find("set_opacity(")
            .expect("set_opacity generated");
        let trans_op_pos = rust_code
            .find("AnimatableProp::Opacity")
            .expect("opacity transition generated");
        assert!(
            bind_op_pos < trans_op_pos,
            "set_transition must come after initial set_opacity/bind_dynamic_prop"
        );

        // Strict compile-time Severity::Error checks
        let invalid = r#"export component Bad {
    in property <int> ro: 1;
    in-out property <bool> ok: false;
    Column {
        UnknownWidget { }
        Text {
            unknown_prop: 1;
            text: "hi";
            Text { text: "nested leaf"; }
        }
        Button {
            on_fake => { ro = 2; }
            opacity: ro;
            animate width { duration: 100ms; }
        }
        Text { text <=> ro; }
        Slider { min: 0; max: 10; value <=> ro; }
    }
}"#;
        let p_bad = parse(invalid);
        let d_bad = validate(&p_bad.file);
        let err_msgs: Vec<&str> = d_bad
            .iter()
            .filter(|d| d.severity == Severity::Error)
            .map(|d| d.message.as_str())
            .collect();
        assert!(
            err_msgs
                .iter()
                .any(|m| m.contains("unknown component or widget")),
            "{err_msgs:?}"
        );
        assert!(
            err_msgs
                .iter()
                .any(|m| m.contains("unknown property 'unknown_prop'")),
            "{err_msgs:?}"
        );
        assert!(
            err_msgs
                .iter()
                .any(|m| m.contains("leaf widget 'Text' cannot contain")),
            "{err_msgs:?}"
        );
        assert!(
            err_msgs
                .iter()
                .any(|m| m.contains("unknown event handler 'on_fake'")),
            "{err_msgs:?}"
        );
        assert!(
            err_msgs
                .iter()
                .any(|m| m.contains("cannot mutate read-only 'in property' 'ro'")),
            "{err_msgs:?}"
        );
        assert!(
            err_msgs.iter().any(|m| m.contains("cannot be animated")),
            "{err_msgs:?}"
        );
        assert!(
            err_msgs
                .iter()
                .any(|m| m.contains("does not support two-way binding")),
            "{err_msgs:?}"
        );
        assert!(
            err_msgs
                .iter()
                .any(|m| m.contains("read-only 'in property' 'ro'")),
            "{err_msgs:?}"
        );
    }

    #[test]
    fn phase4_widgets_validate_and_codegen_cleanly() {
        let src = r#"export component Phase4Suite {
    in-out property <float> split_ratio: 0.4;
    in-out property <int> dismissed_id: 0;
    in-out property <[ToastItem]> alerts: [
        ToastItem {
            id: 1,
            title: "Saved",
            message: "Workspace built",
            severity: ToastSeverity.Success,
            duration_ms: 2500,
        }
    ];

    Stack {
        width: 100%;
        height: 100%;

        SplitPane {
            direction: Horizontal;
            ratio <=> split_ratio;
            min_first: 120dp;
            min_second: 140dp;
            divider_size: 10dp;
            on_change => {
                split_ratio = value;
            }

            Column {
                padding: 12dp;
                gap: 8dp;
                Spinner {
                    size: 28dp;
                    thickness: 3.5dp;
                    color: Theme.primary;
                }
                Badge {
                    label: "Production";
                    icon: "check";
                    background: #10B981;
                    color: #FFFFFF;
                }
            }

            Column {
                Text { text: "Right pane"; }
            }
        }

        ToastHost {
            items: alerts;
            on_dismiss => {
                dismissed_id = id;
            }
        }
    }
}
"#;
        let parsed = parse(src);
        assert!(
            parsed.diagnostics.is_empty(),
            "parse diags: {:?}",
            parsed.diagnostics
        );
        let diags = validate(&parsed.file);
        assert!(diags.is_empty(), "semantic diags: {diags:?}");

        let code = generate_rust(&parsed.file);
        assert!(code.contains("tree.split_pane("));
        assert!(code.contains("set_split_pane_config("));
        assert!(code.contains("tree.spinner("));
        assert!(code.contains("set_spinner_style("));
        assert!(code.contains("tree.badge("));
        assert!(code.contains("set_badge_colors("));
        assert!(code.contains("set_badge_icon("));
        assert!(code.contains("tree.toast_host_custom("));
        assert!(code.contains("set_on_dismiss("));
        assert!(code.contains("ToastSeverity::Success"));
        assert!(code.contains("ToastItem {"));
    }

    #[test]
    fn cleanroom_slint_parity_states_and_optimizer_passes() {
        let src = r#"
import { Button, Column, Container } from "std/widgets.cui"
import { Theme } from "theme/tokens.cui"

export component ParityCard {
    in-out property <bool> is_active: false;
    in-out property <int> count: 1 + 2 * 3;

    Column {
        width: 100%;
        padding: Theme.spacing_md;
        background: Theme.surface_container;

        Container {
            Button {
                text: "Interactive State Button";
                background: Theme.primary;
                states [
                    active when is_active: {
                        background: Theme.primary_container;
                    }
                    inactive when !is_active: {
                        background: Theme.surface;
                    }
                ]
            }
        }
    }
}
"#;
        let parsed = parse(src);
        assert!(
            parsed.diagnostics.is_empty(),
            "parse diags: {:?}",
            parsed.diagnostics
        );
        let diags = validate(&parsed.file);
        assert!(diags.is_empty(), "semantic diags: {diags:?}");

        let rust = generate_rust(&parsed.file);
        // Verify states lowered into reactive ternaries
        assert!(rust.contains("primary_container"));
        // Verify constant propagation folded 1 + 2 * 3 = 7
        assert!(rust.contains("7"));
        // Verify dead Container wrapper was pruned
        assert!(!rust.contains("tree.container("));
    }
}
