//! `.cui` declarative UI DSL parser (`logos` + lossless `rowan` CST), semantic validator,
//! and static Rust AOT code generator (`build.rs` helper).

pub mod ast;
pub mod codegen;
pub mod cst;
pub mod lexer;
pub mod parser;
pub mod validator;

pub use ast::{
    BinOp, CallbackDecl, ChildItem, ComponentDecl, CuiFile, Diagnostic, ElementNode, Expr,
    HandlerBinding, ImportDecl, PropBinding, PropertyDecl, PropertyDir, Severity, Span, Stmt, Unit,
};
pub use codegen::{compile_cui_dir, compile_cui_file, generate_rust};
pub use cst::{CuiLanguage, SyntaxKind, SyntaxNode, SyntaxToken};
pub use lexer::{Token, TokenKind, tokenize};
pub use parser::{ParseResult, parse};
pub use validator::{BUILTIN_PROPS, BUILTIN_WIDGETS, validate};

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_CUI: &str = r#"
import { Column, Row, Button, Slider, ImageView, ScrollView } from "std/widgets.cui"
import { Theme } from "theme/tokens.cui"

export component FilterStudio {
    in property <TextureHandle> preview_texture;
    in-out property <int> selected_filter: 0;
    in-out property <float> brightness: 0.0;
    callback on_filter_changed(int);
    callback on_apply_clicked();

    Column {
        width: 100%;
        height: 100%;
        background: Theme.background;
        padding: SafeArea.insets;

        ImageView {
            flex_grow: 1.0;
            source: preview_texture;
            fit: Contain;
            border_radius: 12dp;
            margin: 16dp;
        }

        ScrollView {
            direction: Horizontal;
            height: 88dp;
            Row {
                gap: 10dp;
                for item, idx in ["Original", "Magic Color", "B&W"] : Button {
                    text: item;
                    on_tap => {
                        selected_filter = idx;
                        on_filter_changed(idx);
                    }
                }
            }
        }

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
        assert_eq!(parsed.file.components.len(), 1);
        assert_eq!(parsed.file.components[0].name, "FilterStudio");
        assert_eq!(parsed.file.components[0].properties.len(), 3);
        assert_eq!(parsed.file.components[0].callbacks.len(), 2);

        let sem_diags = validate(&parsed.file);
        assert!(sem_diags.is_empty(), "semantic diags: {sem_diags:?}");

        let rust_code = generate_rust(&parsed.file);
        assert!(rust_code.contains("pub struct FilterStudio"));
        assert!(rust_code.contains("pub brightness: Signal<f32>"));
    }
}
