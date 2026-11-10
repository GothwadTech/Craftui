use crate::ast::*;
use craftui_core::Color;
use std::collections::HashSet;

pub const BUILTIN_WIDGETS: &[&str] = &[
    "View",
    "Container",
    "Column",
    "Row",
    "Grid",
    "Stack",
    "Spacer",
    "SafeArea",
    "Text",
    "Button",
    "ImageView",
    "Image",
    "Canvas",
    "ScrollView",
    "VirtualList",
    "VirtualGrid",
    "TextInput",
    "Checkbox",
    "Switch",
    "Slider",
    "ProgressIndicator",
    "Modal",
    "BottomSheet",
    "PlatformView",
];

pub const BUILTIN_PROPS: &[&str] = &[
    "width",
    "height",
    "min_width",
    "max_width",
    "min_height",
    "max_height",
    "flex_grow",
    "padding",
    "padding_x",
    "padding_y",
    "margin",
    "gap",
    "align",
    "justify",
    "background",
    "foreground",
    "color",
    "border_radius",
    "border_width",
    "border_color",
    "opacity",
    "text",
    "font_size",
    "font_weight",
    "source",
    "fit",
    "value",
    "min",
    "max",
    "checked",
    "placeholder",
    "direction",
    "columns",
    "title",
    "active",
    "label",
    "open",
    "progress",
    "safe_area",
    "visible",
];

pub fn validate(file: &CuiFile) -> Vec<Diagnostic> {
    let mut diags = Vec::new();
    let mut known_components: HashSet<String> =
        BUILTIN_WIDGETS.iter().map(|s| (*s).to_string()).collect();

    for imp in &file.imports {
        for sym in &imp.symbols {
            known_components.insert(sym.clone());
        }
    }
    for comp in &file.components {
        if !known_components.insert(comp.name.clone())
            && BUILTIN_WIDGETS.contains(&comp.name.as_str())
        {
            diags.push(Diagnostic {
                span: comp.span,
                severity: Severity::Warning,
                message: format!("component '{}' shadows built-in widget", comp.name),
            });
        }
    }

    for comp in &file.components {
        let mut prop_names = HashSet::new();
        for prop in &comp.properties {
            if !prop_names.insert(prop.name.clone()) {
                diags.push(Diagnostic {
                    span: prop.span,
                    severity: Severity::Error,
                    message: format!(
                        "duplicate property '{}' in component '{}'",
                        prop.name, comp.name
                    ),
                });
            }
            if let Some(ref expr) = prop.default_value {
                validate_expr(expr, prop.span, &mut diags);
            }
        }
        let mut cb_names = HashSet::new();
        for cb in &comp.callbacks {
            if !cb_names.insert(cb.name.clone()) {
                diags.push(Diagnostic {
                    span: cb.span,
                    severity: Severity::Error,
                    message: format!(
                        "duplicate callback '{}' in component '{}'",
                        cb.name, comp.name
                    ),
                });
            }
        }
        if let Some(ref root) = comp.root {
            validate_element(root, &known_components, &mut diags);
        }
    }
    diags
}

fn validate_element(
    el: &ElementNode,
    known_components: &HashSet<String>,
    diags: &mut Vec<Diagnostic>,
) {
    if !known_components.contains(&el.kind) {
        diags.push(Diagnostic {
            span: el.span,
            severity: Severity::Warning,
            message: format!("unknown component or widget '{}'", el.kind),
        });
    }
    for prop in &el.props {
        validate_expr(&prop.value, prop.span, diags);
    }
    for child in &el.children {
        match child {
            ChildItem::Element(c) => validate_element(c, known_components, diags),
            ChildItem::For { body, .. } => validate_element(body, known_components, diags),
            ChildItem::If {
                then_branch,
                else_branch,
                ..
            } => {
                validate_element(then_branch, known_components, diags);
                if let Some(eb) = else_branch {
                    validate_element(eb, known_components, diags);
                }
            }
        }
    }
}

fn validate_expr(expr: &Expr, span: Span, diags: &mut Vec<Diagnostic>) {
    match expr {
        Expr::ColorHex(hex) => {
            if Color::from_hex(hex).is_none() {
                diags.push(Diagnostic {
                    span,
                    severity: Severity::Error,
                    message: format!("invalid hex color literal '{hex}'"),
                });
            }
        }
        Expr::Array(items) => {
            for item in items {
                validate_expr(item, span, diags);
            }
        }
        Expr::Binary(l, _, r) => {
            validate_expr(l, span, diags);
            validate_expr(r, span, diags);
        }
        Expr::UnaryNot(inner) => validate_expr(inner, span, diags),
        _ => {}
    }
}
