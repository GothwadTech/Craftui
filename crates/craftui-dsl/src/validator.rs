use crate::ast::*;
use craftui_core::Color;
use std::collections::{HashMap, HashSet};

pub const BUILTIN_WIDGETS: &[&str] = &[
    "View",
    "Container",
    "Column",
    "Row",
    "Grid",
    "Stack",
    "Spacer",
    "SafeArea",
    "Divider",
    "Text",
    "Button",
    "ImageView",
    "Image",
    "Icon",
    "Canvas",
    "ScrollView",
    "VirtualList",
    "VirtualGrid",
    "TextInput",
    "TextArea",
    "Checkbox",
    "Switch",
    "Slider",
    "Tabs",
    "TabBar",
    "Select",
    "Dropdown",
    "RadioGroup",
    "PopupMenu",
    "Tooltip",
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
    "flex_shrink",
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
    "clip",
    "shadow",
    "text",
    "content",
    "font_size",
    "font_weight",
    "source",
    "svg",
    "icon",
    "name",
    "size",
    "fit",
    "value",
    "min",
    "max",
    "checked",
    "placeholder",
    "multiline",
    "direction",
    "columns",
    "thickness",
    "title",
    "active",
    "selected",
    "tabs",
    "options",
    "items",
    "model",
    "item_count",
    "count",
    "item_extent",
    "item_height",
    "label",
    "open",
    "progress",
    "safe_area",
    "visible",
    "disabled",
    "id",
];

pub const BUILTIN_EVENTS: &[&str] = &[
    "on_tap",
    "on_click",
    "on_change",
    "on_submit",
    "on_close",
    "on_select",
    "on_hover",
    "on_double_tap",
    "on_long_press",
];

pub fn validate(file: &CuiFile) -> Vec<Diagnostic> {
    let mut imported_symbols = HashSet::new();
    for imp in &file.imports {
        for sym in &imp.symbols {
            imported_symbols.insert(sym.clone());
        }
    }
    validate_file_with_symbols(file, file, &imported_symbols)
}

pub fn validate_file_with_symbols(
    file: &CuiFile,
    merged: &CuiFile,
    imported_symbols: &HashSet<String>,
) -> Vec<Diagnostic> {
    let mut diags = Vec::new();
    let builtin_widget_set: HashSet<&str> = BUILTIN_WIDGETS.iter().copied().collect();
    let builtin_prop_set: HashSet<&str> = BUILTIN_PROPS.iter().copied().collect();
    let builtin_event_set: HashSet<&str> = BUILTIN_EVENTS.iter().copied().collect();

    let mut known_components: HashSet<String> =
        BUILTIN_WIDGETS.iter().map(|s| (*s).to_string()).collect();
    known_components.extend(imported_symbols.iter().cloned());

    let mut comp_prop_map: HashMap<&str, HashSet<&str>> = HashMap::new();
    let mut comp_cb_map: HashMap<&str, HashSet<&str>> = HashMap::new();
    for comp in &merged.components {
        known_components.insert(comp.name.clone());
        comp_prop_map.insert(
            comp.name.as_str(),
            comp.properties.iter().map(|p| p.name.as_str()).collect(),
        );
        comp_cb_map.insert(
            comp.name.as_str(),
            comp.callbacks.iter().map(|c| c.name.as_str()).collect(),
        );
    }

    // Validate structs
    let mut seen_structs = HashSet::new();
    for st in &file.structs {
        if !seen_structs.insert(st.name.clone()) {
            diags.push(Diagnostic {
                span: st.span,
                severity: Severity::Error,
                message: format!("duplicate struct '{}'", st.name),
            });
        }
        let mut field_names = HashSet::new();
        for f in &st.fields {
            if !field_names.insert(f.name.clone()) {
                diags.push(Diagnostic {
                    span: f.span,
                    severity: Severity::Error,
                    message: format!("duplicate field '{}' in struct '{}'", f.name, st.name),
                });
            }
            if let Some(ref def) = f.default_value {
                validate_expr(def, f.span, &mut diags);
            }
        }
    }

    // Validate enums
    let mut seen_enums = HashSet::new();
    for en in &file.enums {
        if !seen_enums.insert(en.name.clone()) {
            diags.push(Diagnostic {
                span: en.span,
                severity: Severity::Error,
                message: format!("duplicate enum '{}'", en.name),
            });
        }
        let mut var_names = HashSet::new();
        for v in &en.variants {
            if !var_names.insert(v.clone()) {
                diags.push(Diagnostic {
                    span: en.span,
                    severity: Severity::Error,
                    message: format!("duplicate variant '{}' in enum '{}'", v, en.name),
                });
            }
        }
    }

    for comp in &file.components {
        if builtin_widget_set.contains(comp.name.as_str()) {
            diags.push(Diagnostic {
                span: comp.span,
                severity: Severity::Warning,
                message: format!("component '{}' shadows built-in widget", comp.name),
            });
        }

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
            validate_element(
                root,
                &known_components,
                &builtin_widget_set,
                &builtin_prop_set,
                &builtin_event_set,
                &comp_prop_map,
                &comp_cb_map,
                &mut diags,
            );
        }
    }
    diags
}

#[allow(clippy::too_many_arguments)]
fn validate_element(
    el: &ElementNode,
    known_components: &HashSet<String>,
    builtin_widgets: &HashSet<&str>,
    builtin_props: &HashSet<&str>,
    builtin_events: &HashSet<&str>,
    comp_prop_map: &HashMap<&str, HashSet<&str>>,
    comp_cb_map: &HashMap<&str, HashSet<&str>>,
    diags: &mut Vec<Diagnostic>,
) {
    if !known_components.contains(&el.kind) {
        diags.push(Diagnostic {
            span: el.span,
            severity: Severity::Warning,
            message: format!("unknown component or widget '{}'", el.kind),
        });
    }

    let is_builtin = builtin_widgets.contains(el.kind.as_str());
    for prop in &el.props {
        if is_builtin && !builtin_props.contains(prop.name.as_str()) {
            diags.push(Diagnostic {
                span: prop.span,
                severity: Severity::Warning,
                message: format!("unknown property '{}' on '{}'", prop.name, el.kind),
            });
        } else if let Some(valid_props) = comp_prop_map.get(el.kind.as_str())
            && !valid_props.contains(prop.name.as_str())
            && !builtin_props.contains(prop.name.as_str())
        {
            diags.push(Diagnostic {
                span: prop.span,
                severity: Severity::Warning,
                message: format!(
                    "unknown property '{}' on component '{}'",
                    prop.name, el.kind
                ),
            });
        }
        validate_expr(&prop.value, prop.span, diags);
    }

    for handler in &el.handlers {
        let valid_event = builtin_events.contains(handler.event.as_str())
            || comp_cb_map
                .get(el.kind.as_str())
                .is_some_and(|cbs| cbs.contains(handler.event.as_str()));
        if !valid_event && known_components.contains(&el.kind) {
            diags.push(Diagnostic {
                span: handler.span,
                severity: Severity::Warning,
                message: format!("unknown event handler '{}' on '{}'", handler.event, el.kind),
            });
        }
        for stmt in &handler.body {
            match stmt {
                Stmt::Assign { value, .. } => validate_expr(value, handler.span, diags),
                Stmt::Call { args, .. } => {
                    for a in args {
                        validate_expr(a, handler.span, diags);
                    }
                }
                Stmt::Expr(e) => validate_expr(e, handler.span, diags),
            }
        }
    }

    for child in &el.children {
        validate_child(
            child,
            known_components,
            builtin_widgets,
            builtin_props,
            builtin_events,
            comp_prop_map,
            comp_cb_map,
            diags,
        );
    }
}

#[allow(clippy::too_many_arguments)]
fn validate_child(
    child: &ChildItem,
    known_components: &HashSet<String>,
    builtin_widgets: &HashSet<&str>,
    builtin_props: &HashSet<&str>,
    builtin_events: &HashSet<&str>,
    comp_prop_map: &HashMap<&str, HashSet<&str>>,
    comp_cb_map: &HashMap<&str, HashSet<&str>>,
    diags: &mut Vec<Diagnostic>,
) {
    match child {
        ChildItem::Element(c) => validate_element(
            c,
            known_components,
            builtin_widgets,
            builtin_props,
            builtin_events,
            comp_prop_map,
            comp_cb_map,
            diags,
        ),
        ChildItem::For {
            iterable,
            body,
            span,
            ..
        } => {
            validate_expr(iterable, *span, diags);
            validate_element(
                body,
                known_components,
                builtin_widgets,
                builtin_props,
                builtin_events,
                comp_prop_map,
                comp_cb_map,
                diags,
            );
        }
        ChildItem::If {
            condition,
            then_branch,
            else_branch,
            span,
        } => {
            validate_expr(condition, *span, diags);
            validate_element(
                then_branch,
                known_components,
                builtin_widgets,
                builtin_props,
                builtin_events,
                comp_prop_map,
                comp_cb_map,
                diags,
            );
            if let Some(eb) = else_branch {
                validate_element(
                    eb,
                    known_components,
                    builtin_widgets,
                    builtin_props,
                    builtin_events,
                    comp_prop_map,
                    comp_cb_map,
                    diags,
                );
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
        Expr::Binary(l, _, r) | Expr::Index(l, r) => {
            validate_expr(l, span, diags);
            validate_expr(r, span, diags);
        }
        Expr::Range { start, end, .. } => {
            validate_expr(start, span, diags);
            validate_expr(end, span, diags);
        }
        Expr::StructLit { fields, .. } => {
            for (_, v) in fields {
                validate_expr(v, span, diags);
            }
        }
        Expr::UnaryNot(inner) | Expr::UnaryNeg(inner) | Expr::Member(inner, _) => {
            validate_expr(inner, span, diags);
        }
        Expr::Call(_, args) => {
            for a in args {
                validate_expr(a, span, diags);
            }
        }
        Expr::MethodCall(recv, _, args) => {
            validate_expr(recv, span, diags);
            for a in args {
                validate_expr(a, span, diags);
            }
        }
        _ => {}
    }
}
