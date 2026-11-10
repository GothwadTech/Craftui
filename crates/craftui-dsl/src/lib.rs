//! `.cui` declarative UI DSL parser (`logos` + lossless `rowan` CST),
//! multi-file module resolver, semantic validator, and static Rust AOT codegen.

pub mod ast;
pub mod codegen;
pub mod cst;
pub mod lexer;
pub mod parser;
pub mod resolver;
pub mod validator;

pub use ast::{
    BinOp, CallbackDecl, ChildItem, ComponentDecl, CuiFile, Diagnostic, ElementNode, EnumDecl,
    Expr, HandlerBinding, ImportDecl, PropBinding, PropertyDecl, PropertyDir, Severity, Span, Stmt,
    StructDecl, StructField, TypeExpr, Unit,
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
    BUILTIN_EVENTS, BUILTIN_PROPS, BUILTIN_WIDGETS, validate, validate_file_with_symbols,
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
export component Badge {
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
import { BadgeStyle, Badge } from "./models.cui"
export component App {
    in-out property <bool> pinned: false;
    Column {
        Badge { label: "v1"; selected <=> pinned; }
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
        assert!(code.contains("pub struct Badge"));
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
}
