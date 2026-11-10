//! Dynamic `.cui` AST/IR interpreter enabling `< 100ms` visual hot-reload without Rust recompilation.

use craftui_core::{
    Alignment, Axis, Border, BoxShadow, Brush, Color, CornerRadii, CraftError, Fit, Insets,
    Justify, Length, NodeId, PlatformViewId, Result, TextureId, ThemeOverride, ThemeTokens,
};
use craftui_dsl::{
    BinOp, ChildItem, ComponentDecl, CuiFile, Diagnostic, ElementNode, Expr, PropBinding, Severity,
    Stmt, Unit, resolve_file, resolve_source,
};
use craftui_layout::LayoutStyle;
use craftui_reactive::{Scope, Signal};
use craftui_text::TextStyle;
use craftui_widgets::{IndexCallback, ReactiveValue, TapHandler, VirtualItemFactory, WidgetTree};
use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap};
use std::fmt::Write as _;
use std::path::Path;
use std::rc::Rc;
use std::time::{Duration, Instant};

/// Dynamically typed value inside interpreted `.cui` components.
#[derive(Debug, Clone, PartialEq)]
pub enum DynValue {
    Int(i64),
    Float(f64),
    Bool(bool),
    String(String),
    Color(Color),
    Length(Length),
    Texture(Option<TextureId>),
    List(Vec<DynValue>),
    Struct {
        name: String,
        fields: BTreeMap<String, DynValue>,
    },
    Enum(String, String),
    Null,
}

impl DynValue {
    pub fn as_f32(&self) -> f32 {
        match self {
            Self::Float(v) => *v as f32,
            Self::Int(v) => *v as f32,
            Self::Length(Length::Dp(v) | Length::Px(v) | Length::Pct(v)) => *v,
            _ => 0.0,
        }
    }

    pub fn as_i64(&self) -> i64 {
        match self {
            Self::Int(v) => *v,
            Self::Float(v) => *v as i64,
            Self::Bool(b) => i64::from(*b),
            _ => 0,
        }
    }

    pub fn as_bool(&self) -> bool {
        match self {
            Self::Bool(b) => *b,
            Self::Int(i) => *i != 0,
            Self::Float(f) => *f != 0.0,
            Self::String(s) => !s.is_empty(),
            Self::List(v) => !v.is_empty(),
            _ => false,
        }
    }

    pub fn as_str(&self) -> String {
        match self {
            Self::String(s) => s.clone(),
            Self::Int(i) => i.to_string(),
            Self::Float(f) => {
                if (f.fract()).abs() < 1e-6 {
                    format!("{f:.1}")
                } else {
                    format!("{f}")
                }
            }
            Self::Bool(b) => b.to_string(),
            Self::Color(c) => c.to_hex(),
            Self::Enum(_, v) => v.clone(),
            Self::Struct { name, fields } => {
                let mut out = format!("{name} {{ ");
                for (i, (k, v)) in fields.iter().enumerate() {
                    if i > 0 {
                        out.push_str(", ");
                    }
                    let _ = write!(out, "{k}: {}", v.as_str());
                }
                out.push_str(" }");
                out
            }
            Self::List(items) => {
                let joined: Vec<String> = items.iter().map(DynValue::as_str).collect();
                format!("[{}]", joined.join(", "))
            }
            Self::Length(Length::Dp(v)) => format!("{v}dp"),
            Self::Length(Length::Px(v)) => format!("{v}px"),
            Self::Length(Length::Pct(v)) => format!("{v}%"),
            Self::Length(Length::Auto) => "auto".to_string(),
            Self::Texture(Some(id)) => format!("{id:?}"),
            Self::Texture(None) | Self::Null => String::new(),
        }
    }

    pub fn as_length(&self) -> Length {
        match self {
            Self::Length(l) => *l,
            Self::Float(v) => Length::Dp(*v as f32),
            Self::Int(v) => Length::Dp(*v as f32),
            _ => Length::Auto,
        }
    }

    pub fn as_color(&self) -> Option<Color> {
        match self {
            Self::Color(c) => Some(*c),
            Self::String(s) => Color::from_hex(s),
            _ => None,
        }
    }

    pub fn as_string_list(&self) -> Vec<String> {
        match self {
            Self::List(items) => items.iter().map(DynValue::as_str).collect(),
            _ => Vec::new(),
        }
    }

    pub fn key_string(&self) -> String {
        match self {
            Self::Int(i) => format!("i:{i}"),
            Self::Float(f) => format!("f:{f:.4}"),
            Self::Bool(b) => format!("b:{b}"),
            Self::String(s) => format!("s:{s}"),
            Self::Color(c) => format!("c:{}", c.to_hex()),
            Self::Enum(n, v) => format!("e:{n}.{v}"),
            Self::Struct { name, fields } => {
                let mut s = format!("st:{name}{{");
                for (k, v) in fields {
                    let _ = write!(s, "{k}:{},", v.key_string());
                }
                s.push('}');
                s
            }
            Self::List(items) => {
                let parts: Vec<String> = items.iter().map(DynValue::key_string).collect();
                format!("l:[{}]", parts.join(","))
            }
            Self::Texture(id) => format!("t:{id:?}"),
            Self::Length(l) => format!("len:{l:?}"),
            Self::Null => "null".to_string(),
        }
    }
}

pub type DynCallback = Rc<RefCell<dyn FnMut(Vec<DynValue>)>>;

/// Live state store preserved across `.cui` hot-reloads.
pub struct DynamicStore {
    pub props: HashMap<String, Signal<DynValue>>,
    pub callbacks: HashMap<String, DynCallback>,
}

impl Default for DynamicStore {
    fn default() -> Self {
        Self::new()
    }
}

impl DynamicStore {
    pub fn new() -> Self {
        Self {
            props: HashMap::new(),
            callbacks: HashMap::new(),
        }
    }

    pub fn get_or_create_prop(
        &mut self,
        scope: Scope,
        name: &str,
        initial: DynValue,
    ) -> Signal<DynValue> {
        if let Some(&sig) = self.props.get(name)
            && sig.is_alive()
        {
            return sig;
        }
        let sig = scope.signal(initial);
        self.props.insert(name.to_string(), sig);
        sig
    }

    pub fn set_prop(&mut self, scope: Scope, name: &str, value: DynValue) {
        let sig = self.get_or_create_prop(scope, name, value.clone());
        sig.set(value);
    }

    pub fn register_callback(
        &mut self,
        name: impl Into<String>,
        cb: impl FnMut(Vec<DynValue>) + 'static,
    ) {
        self.callbacks
            .insert(name.into(), Rc::new(RefCell::new(cb)));
    }
}

/// Manages hot-reloading a `.cui` template into a live [`WidgetTree`].
pub struct HotReloadSession {
    pub state_scope: Scope,
    pub mount_scope: Option<Scope>,
    pub store: DynamicStore,
    pub last_source: String,
    pub last_reload_duration: Duration,
    pub last_diagnostics: Vec<Diagnostic>,
}

impl HotReloadSession {
    pub fn new(root_scope: Scope) -> Self {
        Self {
            state_scope: root_scope.child_scope(),
            mount_scope: None,
            store: DynamicStore::new(),
            last_source: String::new(),
            last_reload_duration: Duration::ZERO,
            last_diagnostics: Vec::new(),
        }
    }

    /// Parse, validate, and rebuild an inline `.cui` UI subtree while preserving live signal state.
    pub fn reload(
        &mut self,
        source: &str,
        component_name: Option<&str>,
        tree: &mut WidgetTree,
    ) -> Result<NodeId> {
        self.reload_with_base(source, None, component_name, tree)
    }

    /// Parse, resolve relative `import` modules on disk, validate, and hot-reload a `.cui` file.
    pub fn reload_file(
        &mut self,
        path: impl AsRef<Path>,
        component_name: Option<&str>,
        tree: &mut WidgetTree,
    ) -> Result<NodeId> {
        let path_ref = path.as_ref();
        let src = std::fs::read_to_string(path_ref).map_err(|e| {
            CraftError::Runtime(format!("failed reading {}: {e}", path_ref.display()))
        })?;
        let t0 = Instant::now();
        let resolved = resolve_file(path_ref);
        let root =
            self.mount_resolved(resolved.merged, resolved.diagnostics, component_name, tree)?;
        self.last_source = src;
        self.last_reload_duration = t0.elapsed();
        Ok(root)
    }

    /// Parse, resolve imports relative to `base_dir` (if provided), validate, and rebuild the subtree.
    pub fn reload_with_base(
        &mut self,
        source: &str,
        base_dir: Option<&Path>,
        component_name: Option<&str>,
        tree: &mut WidgetTree,
    ) -> Result<NodeId> {
        let t0 = Instant::now();
        let resolved = resolve_source(source, base_dir);
        let root =
            self.mount_resolved(resolved.merged, resolved.diagnostics, component_name, tree)?;
        self.last_source = source.to_string();
        self.last_reload_duration = t0.elapsed();
        Ok(root)
    }

    fn mount_resolved(
        &mut self,
        merged: CuiFile,
        diags: Vec<Diagnostic>,
        component_name: Option<&str>,
        tree: &mut WidgetTree,
    ) -> Result<NodeId> {
        self.last_diagnostics = diags.clone();
        if diags.iter().any(|d| d.severity == Severity::Error) {
            let msg = diags
                .iter()
                .filter(|d| d.severity == Severity::Error)
                .map(|d| d.message.as_str())
                .collect::<Vec<_>>()
                .join("; ");
            return Err(CraftError::Runtime(msg));
        }

        let comp = if let Some(name) = component_name {
            merged
                .components
                .iter()
                .find(|c| c.name == name)
                .or_else(|| merged.components.last())
        } else {
            merged.components.last()
        }
        .ok_or_else(|| CraftError::Runtime("no component found in .cui file".into()))?
        .clone();

        // Tear down old mounted tree nodes if present
        if let Some(old_root) = tree.root.take() {
            tree.remove_subtree(old_root);
        }
        if let Some(old_scope) = self.mount_scope.take() {
            old_scope.dispose();
        }

        let mount_scope = self.state_scope.child_scope();
        self.mount_scope = Some(mount_scope);

        let file_rc = Rc::new(merged);
        let base_env = DynEnv {
            file: file_rc,
            props: self.store.props.clone(),
            callbacks: self.store.callbacks.clone(),
            locals: HashMap::new(),
            theme_sig: tree.theme_signal,
            theme_ov_sig: tree.theme_override_signal,
        };

        // Ensure component properties exist in preserved DynamicStore
        for prop in &comp.properties {
            let env_cur = DynEnv {
                props: self.store.props.clone(),
                ..base_env.clone()
            };
            let init = prop
                .default_value
                .as_ref()
                .map(|e| eval_expr(e, &env_cur))
                .unwrap_or(DynValue::Null);
            let _ = self
                .store
                .get_or_create_prop(self.state_scope, &prop.name, init);
        }

        let root_env = DynEnv {
            props: self.store.props.clone(),
            ..base_env
        };

        let root_node = if let Some(ref root_el) = comp.root {
            build_element(mount_scope, root_el, &root_env, tree)
        } else {
            tree.column(mount_scope)
        };

        tree.set_root(root_node);
        Ok(root_node)
    }
}

#[derive(Clone)]
struct DynEnv {
    file: Rc<CuiFile>,
    props: HashMap<String, Signal<DynValue>>,
    callbacks: HashMap<String, DynCallback>,
    locals: HashMap<String, DynValue>,
    theme_sig: Signal<ThemeTokens>,
    theme_ov_sig: Signal<ThemeOverride>,
}

impl DynEnv {
    fn find_prop_signal(&self, expr: &Expr) -> Option<Signal<DynValue>> {
        if let Expr::Ident(name) = expr {
            self.props.get(name).copied()
        } else {
            None
        }
    }
}

fn find_prop<'a>(el: &'a ElementNode, names: &[&str]) -> Option<&'a PropBinding> {
    el.props.iter().find(|p| names.iter().any(|n| p.name == *n))
}

fn build_element(scope: Scope, el: &ElementNode, env: &DynEnv, tree: &mut WidgetTree) -> NodeId {
    if let Some(sub_comp) = env
        .file
        .components
        .iter()
        .find(|c| c.name == el.kind)
        .cloned()
    {
        return instantiate_subcomponent(scope, &sub_comp, el, env, tree);
    }

    let eval_prop = |name: &str| -> Option<DynValue> {
        el.props
            .iter()
            .find(|p| p.name == name)
            .map(|p| eval_expr(&p.value, env))
    };

    let node = match el.kind.as_str() {
        "Spacer" => tree.spacer(scope),
        "SafeArea" => tree.safe_area(scope),
        "Row" => tree.row(scope),
        "Grid" => {
            let cols = eval_prop("columns")
                .map(|v| v.as_i64().max(1) as u16)
                .unwrap_or(2);
            let gap = eval_prop("gap").map(|v| v.as_f32()).unwrap_or(0.0);
            tree.grid(scope, cols, gap)
        }
        "Stack" => tree.stack(scope),
        "Divider" => {
            let axis = match find_prop(el, &["direction"]).map(|p| &p.value) {
                Some(Expr::Ident(s)) if s == "Vertical" => Axis::Vertical,
                _ => Axis::Horizontal,
            };
            let thickness = eval_prop("thickness").map(|v| v.as_f32()).unwrap_or(1.0);
            let n = tree.divider(scope, axis, thickness);
            if let Some(cp) = find_prop(el, &["color", "foreground"]) {
                let rv = expr_to_reactive_color(&cp.value, env, Color::TRANSPARENT);
                tree.set_divider_color(n, rv);
            }
            n
        }
        "Icon" => {
            let svg_rv = find_prop(el, &["svg", "icon", "name", "source"])
                .map(|p| expr_to_reactive_string(&p.value, env))
                .unwrap_or_else(|| ReactiveValue::Static("info".to_string()));
            let size = find_prop(el, &["size", "font_size"])
                .map(|p| eval_expr(&p.value, env).as_f32())
                .unwrap_or(20.0);
            let col_opt = find_prop(el, &["color", "foreground"]).map(|p| {
                expr_to_reactive_color(&p.value, env, env.theme_sig.get_untracked().text_primary)
            });
            tree.icon(scope, svg_rv, size, col_opt)
        }
        "Text" => {
            let text_rv = find_prop(el, &["text", "title", "content"])
                .map(|p| expr_to_reactive_string(&p.value, env))
                .unwrap_or_else(|| ReactiveValue::Static(String::new()));
            let size = eval_prop("font_size").map(|v| v.as_f32()).unwrap_or(16.0);
            let weight = find_prop(el, &["font_weight"])
                .map(|p| eval_font_weight(&p.value, env))
                .unwrap_or(400.0);
            let color_expr = find_prop(el, &["color", "foreground"])
                .map(|p| p.value.clone())
                .unwrap_or_else(|| {
                    Expr::Member(Box::new(Expr::Ident("Theme".into())), "text_primary".into())
                });
            let style_rv = emit_reactive_text_style(size, weight, &color_expr, env);
            tree.text(scope, text_rv, style_rv)
        }
        "Button" => {
            let text_rv = find_prop(el, &["text", "title", "label"])
                .map(|p| expr_to_reactive_string(&p.value, env))
                .unwrap_or_else(|| ReactiveValue::Static("Button".to_string()));
            let tap_handler = make_handler_opt(el, &["on_tap", "on_click"], env, None);
            let n = tree.button(scope, text_rv, move || {
                if let Some(ref h) = tap_handler
                    && let Ok(mut cb) = h.try_borrow_mut()
                {
                    cb();
                }
            });
            if let Some(fg) = find_prop(el, &["foreground", "color"]) {
                let rv = expr_to_reactive_color(
                    &fg.value,
                    env,
                    env.theme_sig.get_untracked().on_primary,
                );
                tree.set_button_foreground(n, rv);
            }
            n
        }
        "ImageView" | "Image" => {
            let tex_rv = match find_prop(el, &["source"]) {
                Some(p) => {
                    let expr = p.value.clone();
                    if expr_has_reactivity(&expr, env) {
                        let env_c = env.clone();
                        ReactiveValue::Dynamic(Rc::new(move || {
                            match eval_expr_tracked(&expr, &env_c, true) {
                                DynValue::Texture(t) => t,
                                _ => None,
                            }
                        }))
                    } else {
                        let t = match eval_expr(&expr, env) {
                            DynValue::Texture(t) => t,
                            _ => None,
                        };
                        ReactiveValue::Static(t)
                    }
                }
                None => ReactiveValue::Static(None),
            };
            let fit = match find_prop(el, &["fit"]).map(|p| &p.value) {
                Some(Expr::Ident(s)) if s == "Cover" => Fit::Cover,
                Some(Expr::Ident(s)) if s == "Fill" => Fit::Fill,
                Some(Expr::Ident(s)) if s == "None" => Fit::None,
                _ => Fit::Contain,
            };
            let rad = eval_prop("border_radius")
                .map(|v| CornerRadii::all(v.as_f32()))
                .unwrap_or(CornerRadii::ZERO);
            tree.image(scope, tex_rv, fit, rad)
        }
        "Canvas" => tree.canvas(scope, LayoutStyle::column(), |_, _| {}, None),
        "ScrollView" => {
            let axis = match find_prop(el, &["direction"]).map(|p| &p.value) {
                Some(Expr::Ident(s)) if s == "Horizontal" => Axis::Horizontal,
                _ => Axis::Vertical,
            };
            let base_layout = match axis {
                Axis::Horizontal => LayoutStyle::row(),
                Axis::Vertical => LayoutStyle::column(),
            };
            tree.scroll_view(scope, axis, base_layout)
        }
        "VirtualList" | "VirtualGrid" => {
            let axis = match find_prop(el, &["direction"]).map(|p| &p.value) {
                Some(Expr::Ident(s)) if s == "Horizontal" => Axis::Horizontal,
                _ => Axis::Vertical,
            };
            let cols = eval_prop("columns")
                .map(|v| v.as_i64().max(1) as usize)
                .unwrap_or(if el.kind == "VirtualGrid" { 2 } else { 1 });
            let extent = eval_prop("item_extent")
                .or_else(|| eval_prop("item_height"))
                .map(|v| v.as_f32())
                .unwrap_or(48.0);

            if let Some(ChildItem::For {
                item_var,
                index_var,
                iterable,
                body,
                ..
            }) = el.children.first()
            {
                let iter_expr = iterable.clone();
                let env_cnt = env.clone();
                let count_rv = ReactiveValue::Dynamic(Rc::new(move || {
                    eval_iterable(&iter_expr, &env_cnt, true).len()
                }));
                let iter_fac = iterable.clone();
                let env_fac = env.clone();
                let item_name = item_var.clone();
                let idx_name = index_var.clone();
                let body_el = (**body).clone();
                let factory: VirtualItemFactory = Rc::new(move |v_cx, v_idx, v_tree| {
                    let items = eval_iterable(&iter_fac, &env_fac, false);
                    let Some(item_val) = items.get(v_idx).cloned() else {
                        return v_tree.spacer(v_cx);
                    };
                    let mut child_env = env_fac.clone();
                    child_env.locals.insert(item_name.clone(), item_val);
                    if let Some(ref iv) = idx_name {
                        child_env
                            .locals
                            .insert(iv.clone(), DynValue::Int(v_idx as i64));
                    }
                    build_element(v_cx, &body_el, &child_env, v_tree)
                });
                let n = tree.virtual_list(
                    scope,
                    axis,
                    cols,
                    extent,
                    count_rv,
                    LayoutStyle::column(),
                    factory,
                );
                apply_common_props_and_events(scope, n, el, env, tree, false);
                return n;
            } else {
                let count_rv = match find_prop(el, &["item_count", "count", "model", "items"]) {
                    Some(p) => {
                        let expr = p.value.clone();
                        let env_c = env.clone();
                        ReactiveValue::Dynamic(Rc::new(move || {
                            match eval_expr_tracked(&expr, &env_c, true) {
                                DynValue::List(v) => v.len(),
                                other => other.as_i64().max(0) as usize,
                            }
                        }))
                    }
                    None => ReactiveValue::Static(0usize),
                };
                tree.virtual_list(
                    scope,
                    axis,
                    cols,
                    extent,
                    count_rv,
                    LayoutStyle::column(),
                    Rc::new(|cx, idx, t| t.text(cx, format!("Item {idx}"), TextStyle::default())),
                )
            }
        }
        "Slider" => {
            let min = eval_prop("min").map(|v| v.as_f32()).unwrap_or(0.0);
            let max = eval_prop("max").map(|v| v.as_f32()).unwrap_or(100.0);
            let val_prop = find_prop(el, &["value"]);
            let sig = bind_f32_signal(scope, val_prop, env, 0.0);
            let lbl = eval_prop("label").map(|v| v.as_str()).unwrap_or_default();
            let n = tree.slider(scope, sig, min, max, lbl);
            if let Some(h) = el.handlers.iter().find(|h| h.event == "on_change") {
                let stmts = h.body.clone();
                let env_c = env.clone();
                tree.set_on_float_change(n, move |value| {
                    let mut h_env = env_c.clone();
                    h_env
                        .locals
                        .insert("value".into(), DynValue::Float(value as f64));
                    run_stmts(&stmts, &h_env);
                });
            }
            n
        }
        "TextInput" | "TextArea" => {
            let is_multiline = el.kind == "TextArea"
                || matches!(
                    find_prop(el, &["multiline"]).map(|p| &p.value),
                    Some(Expr::Bool(true))
                );
            let placeholder = eval_prop("placeholder")
                .map(|v| v.as_str())
                .unwrap_or_default();
            let val_prop = find_prop(el, &["value", "text"]);
            let (init_s, sig_opt) = bind_string_signal(scope, val_prop, env);
            let n = if is_multiline {
                tree.text_area(scope, init_s, placeholder, sig_opt)
            } else {
                tree.text_input(scope, init_s, placeholder, sig_opt)
            };
            if find_prop(el, &["font_size", "font_weight"]).is_some() {
                let sz = eval_prop("font_size").map(|v| v.as_f32()).unwrap_or(14.0);
                let wt = find_prop(el, &["font_weight"])
                    .map(|p| eval_font_weight(&p.value, env))
                    .unwrap_or(400.0);
                tree.set_text_input_style(n, TextStyle::default().with_size(sz).with_weight(wt));
            }
            if let Some(h) = el.handlers.iter().find(|h| h.event == "on_change") {
                let stmts = h.body.clone();
                let env_c = env.clone();
                tree.set_on_change(n, move |value| {
                    let mut h_env = env_c.clone();
                    h_env
                        .locals
                        .insert("value".into(), DynValue::String(value.clone()));
                    h_env.locals.insert("text".into(), DynValue::String(value));
                    run_stmts(&stmts, &h_env);
                });
            }
            if let Some(h) = el.handlers.iter().find(|h| h.event == "on_submit") {
                let stmts = h.body.clone();
                let env_c = env.clone();
                tree.set_on_submit(n, move |value| {
                    let mut h_env = env_c.clone();
                    h_env
                        .locals
                        .insert("value".into(), DynValue::String(value.clone()));
                    h_env.locals.insert("text".into(), DynValue::String(value));
                    run_stmts(&stmts, &h_env);
                });
            }
            n
        }
        "Checkbox" | "Switch" => {
            let checked_prop = find_prop(el, &["checked", "value"]);
            let sig = bind_bool_signal(scope, checked_prop, env, false);
            let lbl = eval_prop("label")
                .or_else(|| eval_prop("text"))
                .map(|v| v.as_str())
                .unwrap_or_default();
            let n = if el.kind == "Checkbox" {
                tree.checkbox(scope, sig, lbl)
            } else {
                tree.switch(scope, sig, lbl)
            };
            if let Some(h) = el
                .handlers
                .iter()
                .find(|h| h.event == "on_change" || h.event == "on_tap")
            {
                let stmts = h.body.clone();
                let env_c = env.clone();
                tree.set_on_bool_change(n, move |checked| {
                    let mut h_env = env_c.clone();
                    h_env
                        .locals
                        .insert("checked".into(), DynValue::Bool(checked));
                    h_env.locals.insert("value".into(), DynValue::Bool(checked));
                    run_stmts(&stmts, &h_env);
                });
            }
            n
        }
        "Tabs" | "TabBar" => {
            let tabs_rv = find_prop(el, &["tabs", "items", "options"])
                .map(|p| expr_to_reactive_string_list(&p.value, env))
                .unwrap_or_else(|| ReactiveValue::Static(vec!["Tab 1".to_string()]));
            let idx_prop = find_prop(el, &["active", "selected", "value"]);
            let idx_sig = bind_usize_signal(scope, idx_prop, env, 0);
            let n = tree.tabs(scope, tabs_rv, idx_sig);
            wire_index_change(n, el, env, tree);
            n
        }
        "Select" | "Dropdown" => {
            let opts_rv = find_prop(el, &["options", "items"])
                .map(|p| expr_to_reactive_string_list(&p.value, env))
                .unwrap_or_else(|| ReactiveValue::Static(Vec::new()));
            let placeholder = eval_prop("placeholder")
                .or_else(|| eval_prop("label"))
                .map(|v| v.as_str())
                .unwrap_or_else(|| "Select...".to_string());
            let idx_prop = find_prop(el, &["selected", "value"]);
            let idx_sig = bind_usize_signal(scope, idx_prop, env, 0);
            let n = tree.select(scope, opts_rv, idx_sig, placeholder);
            wire_index_change(n, el, env, tree);
            n
        }
        "RadioGroup" => {
            let opts_rv = find_prop(el, &["options", "items"])
                .map(|p| expr_to_reactive_string_list(&p.value, env))
                .unwrap_or_else(|| ReactiveValue::Static(Vec::new()));
            let axis = match find_prop(el, &["direction"]).map(|p| &p.value) {
                Some(Expr::Ident(s)) if s == "Horizontal" => Axis::Horizontal,
                _ => Axis::Vertical,
            };
            let idx_prop = find_prop(el, &["selected", "value"]);
            let idx_sig = bind_usize_signal(scope, idx_prop, env, 0);
            let n = tree.radio_group(scope, opts_rv, idx_sig, axis);
            wire_index_change(n, el, env, tree);
            n
        }
        "PopupMenu" => {
            let open_prop = find_prop(el, &["open", "visible"]);
            let open_rv = open_prop
                .map(|p| expr_to_reactive_bool(&p.value, env))
                .unwrap_or_else(|| ReactiveValue::Static(false));
            let items_rv = find_prop(el, &["items", "options"])
                .map(|p| expr_to_reactive_string_list(&p.value, env))
                .unwrap_or_else(|| ReactiveValue::Static(Vec::new()));
            let on_sel: Option<IndexCallback> = el
                .handlers
                .iter()
                .find(|h| h.event == "on_select" || h.event == "on_change")
                .map(|h| {
                    let stmts = h.body.clone();
                    let env_c = env.clone();
                    Rc::new(RefCell::new(move |idx: usize| {
                        let mut h_env = env_c.clone();
                        h_env.locals.insert("idx".into(), DynValue::Int(idx as i64));
                        h_env
                            .locals
                            .insert("index".into(), DynValue::Int(idx as i64));
                        run_stmts(&stmts, &h_env);
                    })) as IndexCallback
                });
            let on_cls = build_overlay_close_handler(el, open_prop, env);
            tree.popup_menu(scope, open_rv, items_rv, None, on_sel, on_cls)
        }
        "Tooltip" => {
            let txt_rv = find_prop(el, &["text", "label", "content"])
                .map(|p| expr_to_reactive_string(&p.value, env))
                .unwrap_or_else(|| ReactiveValue::Static(String::new()));
            tree.tooltip(scope, txt_rv)
        }
        "ProgressIndicator" => {
            let prog_rv = match find_prop(el, &["progress", "value"]) {
                Some(p) => {
                    let expr = p.value.clone();
                    if expr_has_reactivity(&expr, env) {
                        let env_c = env.clone();
                        ReactiveValue::Dynamic(Rc::new(move || {
                            match eval_expr_tracked(&expr, &env_c, true) {
                                DynValue::Null => None,
                                v => Some(v.as_f32()),
                            }
                        }))
                    } else {
                        match eval_expr(&expr, env) {
                            DynValue::Null => ReactiveValue::Static(None),
                            v => ReactiveValue::Static(Some(v.as_f32())),
                        }
                    }
                }
                None => ReactiveValue::Static(None),
            };
            tree.progress_indicator(scope, prog_rv)
        }
        "Modal" | "BottomSheet" => {
            let is_sheet = el.kind == "BottomSheet";
            let open_prop = find_prop(el, &["open", "visible"]);
            let open_rv = open_prop
                .map(|p| expr_to_reactive_bool(&p.value, env))
                .unwrap_or_else(|| ReactiveValue::Static(false));
            let on_cls = build_overlay_close_handler(el, open_prop, env);
            tree.overlay(scope, open_rv, is_sheet, on_cls)
        }
        "PlatformView" => {
            tree.platform_view(scope, PlatformViewId::default(), LayoutStyle::column())
        }
        _ => tree.column(scope),
    };

    apply_common_props_and_events(scope, node, el, env, tree, false);
    build_children(scope, node, el, env, tree);
    node
}

fn apply_common_props_and_events(
    _scope: Scope,
    node: NodeId,
    el: &ElementNode,
    env: &DynEnv,
    tree: &mut WidgetTree,
    is_subcomponent: bool,
) {
    let eval_prop = |name: &str| -> Option<DynValue> {
        el.props
            .iter()
            .find(|p| p.name == name)
            .map(|p| eval_expr(&p.value, env))
    };

    let mut layout = tree
        .nodes
        .get(node)
        .map(|n| n.layout.clone())
        .unwrap_or_default();
    let mut layout_dirty = false;
    let mut box_style = tree
        .nodes
        .get(node)
        .map(|n| n.box_style.clone())
        .unwrap_or_default();
    let mut box_dirty = false;

    if let Some(v) = eval_prop("width") {
        layout.width = v.as_length();
        layout_dirty = true;
    }
    if let Some(v) = eval_prop("height") {
        layout.height = v.as_length();
        layout_dirty = true;
    }
    if let Some(v) = eval_prop("min_width") {
        layout.min_width = v.as_length();
        layout_dirty = true;
    }
    if let Some(v) = eval_prop("max_width") {
        layout.max_width = v.as_length();
        layout_dirty = true;
    }
    if let Some(v) = eval_prop("min_height") {
        layout.min_height = v.as_length();
        layout_dirty = true;
    }
    if let Some(v) = eval_prop("max_height") {
        layout.max_height = v.as_length();
        layout_dirty = true;
    }
    if let Some(v) = eval_prop("flex_grow") {
        layout.flex_grow = v.as_f32();
        layout_dirty = true;
    }
    if let Some(v) = eval_prop("flex_shrink") {
        layout.flex_shrink = v.as_f32();
        layout_dirty = true;
    }
    if let Some(v) = eval_prop("gap") {
        let g = v.as_f32();
        layout.gap_x = g;
        layout.gap_y = g;
        layout_dirty = true;
    }
    if let Some(v) = eval_prop("safe_area") {
        layout.safe_area = v.as_bool();
        layout_dirty = true;
    }
    if let Some(p_binding) = el.props.iter().find(|p| p.name == "padding") {
        if let Expr::Member(base, field) = &p_binding.value
            && matches!(base.as_ref(), Expr::Ident(s) if s == "SafeArea")
            && field == "insets"
        {
            layout.safe_area = true;
        } else {
            let v = eval_expr(&p_binding.value, env).as_f32();
            layout.padding = Insets::all(v);
        }
        layout_dirty = true;
    }
    if let Some(v) = eval_prop("padding_x") {
        let px = v.as_f32();
        layout.padding.left = px;
        layout.padding.right = px;
        layout_dirty = true;
    }
    if let Some(v) = eval_prop("padding_y") {
        let py = v.as_f32();
        layout.padding.top = py;
        layout.padding.bottom = py;
        layout_dirty = true;
    }
    if let Some(v) = eval_prop("margin") {
        layout.margin = Insets::all(v.as_f32());
        layout_dirty = true;
    }
    if let Some(p_binding) = el.props.iter().find(|p| p.name == "justify")
        && let Expr::Ident(s) = &p_binding.value
    {
        layout.justify_content = match s.as_str() {
            "Center" => Justify::Center,
            "End" => Justify::End,
            "SpaceBetween" => Justify::SpaceBetween,
            "SpaceAround" => Justify::SpaceAround,
            "SpaceEvenly" => Justify::SpaceEvenly,
            _ => Justify::Start,
        };
        layout_dirty = true;
    }
    if let Some(p_binding) = el.props.iter().find(|p| p.name == "align")
        && let Expr::Ident(s) = &p_binding.value
    {
        layout.align_items = match s.as_str() {
            "Center" => Alignment::Center,
            "End" => Alignment::End,
            "Stretch" => Alignment::Stretch,
            _ => Alignment::Start,
        };
        layout_dirty = true;
    }

    if let Some(bg_prop) = find_prop(el, &["background"]) {
        if expr_has_reactivity(&bg_prop.value, env) {
            let expr = bg_prop.value.clone();
            let env_c = env.clone();
            box_style.background = Some(ReactiveValue::Dynamic(Rc::new(move || {
                let c = eval_expr_tracked(&expr, &env_c, true)
                    .as_color()
                    .unwrap_or(Color::TRANSPARENT);
                Brush::Solid(c)
            })));
            box_dirty = true;
        } else if let Some(c) = eval_expr(&bg_prop.value, env).as_color() {
            box_style.background = Some(ReactiveValue::Static(Brush::Solid(c)));
            box_dirty = true;
        }
    }
    if let Some(v) = eval_prop("border_radius") {
        let rad = CornerRadii::all(v.as_f32());
        box_style.radius = rad;
        if let Some(ref mut br) = box_style.border {
            br.radius = rad;
        }
        tree.set_image_radius(node, rad);
        box_dirty = true;
    }
    if let Some(v) = eval_prop("border_width") {
        let bw = v.as_f32();
        let def_c = env.theme_sig.get_untracked().border;
        let cur = box_style.border.unwrap_or(Border {
            width: bw,
            color: def_c,
            radius: box_style.radius,
        });
        box_style.border = Some(Border {
            width: bw,
            color: cur.color,
            radius: box_style.radius,
        });
        if find_prop(el, &["border_color"]).is_none() && box_style.border_color.is_none() {
            let tsig = env.theme_sig;
            box_style.border_color =
                Some(ReactiveValue::Dynamic(Rc::new(move || tsig.get().border)));
        }
        box_dirty = true;
    }
    if let Some(bc_prop) = find_prop(el, &["border_color"]) {
        let col_rv =
            expr_to_reactive_color(&bc_prop.value, env, env.theme_sig.get_untracked().border);
        box_style.border_color = Some(col_rv);
        if box_style.border.is_none() {
            box_style.border = Some(Border {
                width: 1.0,
                color: Color::TRANSPARENT,
                radius: box_style.radius,
            });
        }
        box_dirty = true;
    }
    if let Some(v) = eval_prop("opacity") {
        box_style.opacity = v.as_f32();
        box_dirty = true;
    }
    if let Some(v) = eval_prop("clip") {
        let c = v.as_bool();
        box_style.clip = c;
        if c {
            layout.clip_overflow = true;
            layout_dirty = true;
        }
        box_dirty = true;
    }
    if let Some(v) = eval_prop("shadow") {
        match v {
            DynValue::Bool(true) => {
                box_style.shadow = Some(BoxShadow {
                    offset_x: 0.0,
                    offset_y: 4.0,
                    blur_radius: 12.0,
                    color: Color::rgba(0, 0, 0, 80),
                });
                box_dirty = true;
            }
            DynValue::Bool(false) => {
                box_style.shadow = None;
                box_dirty = true;
            }
            other => {
                let blur = other.as_f32();
                if blur > 0.0 {
                    box_style.shadow = Some(BoxShadow {
                        offset_x: 0.0,
                        offset_y: 4.0,
                        blur_radius: blur,
                        color: Color::rgba(0, 0, 0, 80),
                    });
                    box_dirty = true;
                }
            }
        }
    }

    if layout_dirty {
        tree.set_layout(node, layout);
    }
    if box_dirty {
        tree.set_box_style(node, box_style);
    }

    if let Some(vp) = find_prop(el, &["visible"])
        && !matches!(el.kind.as_str(), "Modal" | "BottomSheet" | "PopupMenu")
    {
        let expr = vp.value.clone();
        let env_c = env.clone();
        tree.set_visible_when(node, move || {
            eval_expr_tracked(&expr, &env_c, true).as_bool()
        });
    }

    if let Some(dp) = find_prop(el, &["disabled"]) {
        let rv = expr_to_reactive_bool(&dp.value, env);
        tree.set_disabled(node, rv);
    }

    let is_sub_cb = is_subcomponent
        && env
            .file
            .components
            .iter()
            .find(|c| c.name == el.kind)
            .is_some_and(|c| c.callbacks.iter().any(|cb| cb.name == "on_tap"));

    if (is_subcomponent || el.kind != "Button")
        && !is_sub_cb
        && let Some(h) = make_handler_opt(el, &["on_tap", "on_click"], env, None)
    {
        tree.set_on_tap(node, move || {
            if let Ok(mut cb) = h.try_borrow_mut() {
                cb();
            }
        });
    }
    if let Some(h) = make_handler_opt(el, &["on_double_tap"], env, None) {
        tree.set_on_double_tap(node, move || {
            if let Ok(mut cb) = h.try_borrow_mut() {
                cb();
            }
        });
    }
    if let Some(h) = make_handler_opt(el, &["on_long_press"], env, None) {
        tree.set_on_long_press(node, move || {
            if let Ok(mut cb) = h.try_borrow_mut() {
                cb();
            }
        });
    }
    if let Some(h) = el.handlers.iter().find(|h| h.event == "on_hover") {
        let stmts = h.body.clone();
        let env_c = env.clone();
        tree.set_on_hover(node, move |hovered| {
            let mut h_env = env_c.clone();
            h_env
                .locals
                .insert("hovered".into(), DynValue::Bool(hovered));
            h_env.locals.insert("value".into(), DynValue::Bool(hovered));
            run_stmts(&stmts, &h_env);
        });
    }
}

fn build_children(
    scope: Scope,
    parent_node: NodeId,
    el: &ElementNode,
    env: &DynEnv,
    tree: &mut WidgetTree,
) {
    let slot_layout = {
        let mut base = match el.kind.as_str() {
            "Row" => LayoutStyle::row(),
            "Grid" => {
                let cols = find_prop(el, &["columns"])
                    .map(|p| eval_expr(&p.value, env).as_i64().max(1) as u16)
                    .unwrap_or(2);
                LayoutStyle::grid(cols)
            }
            "Stack" => LayoutStyle::stack(),
            _ => LayoutStyle::column(),
        };
        if let Some(g) = find_prop(el, &["gap"]) {
            base = base.with_gap(eval_expr(&g.value, env).as_f32());
        }
        base
    };

    for child in &el.children {
        match child {
            ChildItem::Element(c) => {
                let cnode = build_element(scope.child_scope(), c, env, tree);
                tree.append_child(parent_node, cnode);
            }
            ChildItem::For {
                item_var,
                index_var,
                iterable,
                body,
                ..
            } => {
                let iter_model = iterable.clone();
                let env_model = env.clone();
                let item_name = item_var.clone();
                let idx_name = index_var.clone();
                let body_el = (**body).clone();
                let env_fac = env.clone();

                let for_host = tree.reactive_for(
                    scope,
                    slot_layout.clone(),
                    move || eval_iterable(&iter_model, &env_model, true),
                    |idx, val| format!("{idx}:{}", val.key_string()),
                    move |for_cx, item_val, idx_usize, for_tree| {
                        let mut child_env = env_fac.clone();
                        child_env.locals.insert(item_name.clone(), item_val);
                        if let Some(ref iv) = idx_name {
                            child_env
                                .locals
                                .insert(iv.clone(), DynValue::Int(idx_usize as i64));
                        }
                        build_element(for_cx, &body_el, &child_env, for_tree)
                    },
                );
                tree.append_child(parent_node, for_host);
            }
            ChildItem::If {
                condition,
                then_branch,
                else_branch,
                ..
            } => {
                if let Some(eb) = else_branch {
                    let cond_expr = condition.clone();
                    let env_cond = env.clone();
                    let then_el = (**then_branch).clone();
                    let else_el = (**eb).clone();
                    let env_body = env.clone();

                    let if_host = tree.dynamic_slot(
                        scope,
                        slot_layout.clone(),
                        move || eval_expr_tracked(&cond_expr, &env_cond, true).as_bool(),
                        move |if_cx, &is_then, if_tree| {
                            if is_then {
                                build_element(if_cx, &then_el, &env_body, if_tree)
                            } else {
                                build_element(if_cx, &else_el, &env_body, if_tree)
                            }
                        },
                    );
                    tree.append_child(parent_node, if_host);
                } else {
                    let cnode = build_element(scope.child_scope(), then_branch, env, tree);
                    let cond_expr = condition.clone();
                    let env_cond = env.clone();
                    tree.set_visible_when(cnode, move || {
                        eval_expr_tracked(&cond_expr, &env_cond, true).as_bool()
                    });
                    tree.append_child(parent_node, cnode);
                }
            }
        }
    }
}

fn instantiate_subcomponent(
    scope: Scope,
    comp: &ComponentDecl,
    call_el: &ElementNode,
    outer_env: &DynEnv,
    tree: &mut WidgetTree,
) -> NodeId {
    let mut sub_props = HashMap::new();

    for p in &comp.properties {
        if let Some(bound) = call_el.props.iter().find(|bp| bp.name == p.name) {
            if bound.two_way
                && let Some(outer_sig) = outer_env.find_prop_signal(&bound.value)
            {
                // True bidirectional aliasing across parent and child components
                sub_props.insert(p.name.clone(), outer_sig);
                continue;
            }
            let init = eval_expr(&bound.value, outer_env);
            let child_sig = scope.signal(init);
            if expr_has_reactivity(&bound.value, outer_env) {
                let expr_c = bound.value.clone();
                let outer_c = outer_env.clone();
                scope.effect(move || {
                    let nv = eval_expr_tracked(&expr_c, &outer_c, true);
                    if child_sig.get_untracked() != nv {
                        child_sig.set(nv);
                    }
                });
            }
            sub_props.insert(p.name.clone(), child_sig);
        } else {
            let v = p
                .default_value
                .as_ref()
                .map(|e| eval_expr(e, outer_env))
                .unwrap_or(DynValue::Null);
            sub_props.insert(p.name.clone(), scope.signal(v));
        }
    }

    let mut sub_callbacks = outer_env.callbacks.clone();
    for cb_decl in &comp.callbacks {
        if let Some(handler) = call_el.handlers.iter().find(|h| h.event == cb_decl.name) {
            let stmts = handler.body.clone();
            let env_c = outer_env.clone();
            let dyn_cb: DynCallback = Rc::new(RefCell::new(move |args: Vec<DynValue>| {
                let mut h_env = env_c.clone();
                for (i, a) in args.into_iter().enumerate() {
                    if i == 0 {
                        h_env.locals.insert("value".into(), a.clone());
                    }
                    h_env.locals.insert(format!("arg{i}"), a);
                }
                run_stmts(&stmts, &h_env);
            }));
            sub_callbacks.insert(cb_decl.name.clone(), dyn_cb);
        }
    }

    let sub_env = DynEnv {
        file: outer_env.file.clone(),
        props: sub_props,
        callbacks: sub_callbacks,
        locals: HashMap::new(),
        theme_sig: outer_env.theme_sig,
        theme_ov_sig: outer_env.theme_ov_sig,
    };

    let node = if let Some(ref root) = comp.root {
        build_element(scope, root, &sub_env, tree)
    } else {
        tree.column(scope)
    };
    apply_common_props_and_events(scope, node, call_el, outer_env, tree, true);
    node
}

fn bind_f32_signal(
    scope: Scope,
    prop: Option<&PropBinding>,
    env: &DynEnv,
    default: f32,
) -> Signal<f32> {
    let init = prop
        .map(|p| eval_expr(&p.value, env).as_f32())
        .unwrap_or(default);
    let sig = scope.signal(init);
    if let Some(p) = prop {
        if p.two_way
            && let Some(dyn_sig) = env.find_prop_signal(&p.value)
        {
            sig.set(dyn_sig.get_untracked().as_f32());
            scope.effect(move || {
                let nv = dyn_sig.get().as_f32();
                if (sig.get_untracked() - nv).abs() > 1e-4 {
                    sig.set(nv);
                }
            });
            scope.effect(move || {
                let nv = sig.get();
                if (dyn_sig.get_untracked().as_f32() - nv).abs() > 1e-4 {
                    dyn_sig.set(DynValue::Float(nv as f64));
                }
            });
        } else if expr_has_reactivity(&p.value, env) {
            let expr = p.value.clone();
            let env_c = env.clone();
            scope.effect(move || {
                let nv = eval_expr_tracked(&expr, &env_c, true).as_f32();
                if (sig.get_untracked() - nv).abs() > 1e-4 {
                    sig.set(nv);
                }
            });
        }
    }
    sig
}

fn bind_bool_signal(
    scope: Scope,
    prop: Option<&PropBinding>,
    env: &DynEnv,
    default: bool,
) -> Signal<bool> {
    let init = prop
        .map(|p| eval_expr(&p.value, env).as_bool())
        .unwrap_or(default);
    let sig = scope.signal(init);
    if let Some(p) = prop {
        if p.two_way
            && let Some(dyn_sig) = env.find_prop_signal(&p.value)
        {
            sig.set(dyn_sig.get_untracked().as_bool());
            scope.effect(move || {
                let nv = dyn_sig.get().as_bool();
                if sig.get_untracked() != nv {
                    sig.set(nv);
                }
            });
            scope.effect(move || {
                let nv = sig.get();
                if dyn_sig.get_untracked().as_bool() != nv {
                    dyn_sig.set(DynValue::Bool(nv));
                }
            });
        } else if expr_has_reactivity(&p.value, env) {
            let expr = p.value.clone();
            let env_c = env.clone();
            scope.effect(move || {
                let nv = eval_expr_tracked(&expr, &env_c, true).as_bool();
                if sig.get_untracked() != nv {
                    sig.set(nv);
                }
            });
        }
    }
    sig
}

fn bind_usize_signal(
    scope: Scope,
    prop: Option<&PropBinding>,
    env: &DynEnv,
    default: usize,
) -> Signal<usize> {
    let init = prop
        .map(|p| eval_expr(&p.value, env).as_i64().max(0) as usize)
        .unwrap_or(default);
    let sig = scope.signal(init);
    if let Some(p) = prop {
        if p.two_way
            && let Some(dyn_sig) = env.find_prop_signal(&p.value)
        {
            sig.set(dyn_sig.get_untracked().as_i64().max(0) as usize);
            scope.effect(move || {
                let nv = dyn_sig.get().as_i64().max(0) as usize;
                if sig.get_untracked() != nv {
                    sig.set(nv);
                }
            });
            scope.effect(move || {
                let nv = sig.get();
                if (dyn_sig.get_untracked().as_i64().max(0) as usize) != nv {
                    dyn_sig.set(DynValue::Int(nv as i64));
                }
            });
        } else if expr_has_reactivity(&p.value, env) {
            let expr = p.value.clone();
            let env_c = env.clone();
            scope.effect(move || {
                let nv = eval_expr_tracked(&expr, &env_c, true).as_i64().max(0) as usize;
                if sig.get_untracked() != nv {
                    sig.set(nv);
                }
            });
        }
    }
    sig
}

fn bind_string_signal(
    scope: Scope,
    prop: Option<&PropBinding>,
    env: &DynEnv,
) -> (String, Option<Signal<String>>) {
    let Some(p) = prop else {
        return (String::new(), None);
    };
    let init = eval_expr(&p.value, env).as_str();
    if let Some(dyn_sig) = env.find_prop_signal(&p.value) {
        let str_sig = scope.signal(dyn_sig.get_untracked().as_str());
        scope.effect(move || {
            let nv = dyn_sig.get().as_str();
            if str_sig.get_untracked() != nv {
                str_sig.set(nv);
            }
        });
        if p.two_way {
            scope.effect(move || {
                let nv = str_sig.get();
                if dyn_sig.get_untracked().as_str() != nv {
                    dyn_sig.set(DynValue::String(nv));
                }
            });
        }
        return (String::new(), Some(str_sig));
    }
    if expr_has_reactivity(&p.value, env) {
        let str_sig = scope.signal(init.clone());
        let expr = p.value.clone();
        let env_c = env.clone();
        scope.effect(move || {
            let nv = eval_expr_tracked(&expr, &env_c, true).as_str();
            if str_sig.get_untracked() != nv {
                str_sig.set(nv);
            }
        });
        return (init, Some(str_sig));
    }
    (init, None)
}

fn wire_index_change(node: NodeId, el: &ElementNode, env: &DynEnv, tree: &mut WidgetTree) {
    if let Some(h) = el
        .handlers
        .iter()
        .find(|h| h.event == "on_change" || h.event == "on_select")
    {
        let stmts = h.body.clone();
        let env_c = env.clone();
        tree.set_on_index_change(node, move |idx| {
            let mut h_env = env_c.clone();
            let v = DynValue::Int(idx as i64);
            h_env.locals.insert("index".into(), v.clone());
            h_env.locals.insert("selected".into(), v.clone());
            h_env.locals.insert("active".into(), v.clone());
            h_env.locals.insert("value".into(), v);
            run_stmts(&stmts, &h_env);
        });
    }
}

fn build_overlay_close_handler(
    el: &ElementNode,
    open_prop: Option<&PropBinding>,
    env: &DynEnv,
) -> Option<TapHandler> {
    if let Some(h) = make_handler_opt(el, &["on_close"], env, None) {
        return Some(h);
    }
    if let Some(op) = open_prop
        && op.two_way
        && let Some(dyn_sig) = env.find_prop_signal(&op.value)
    {
        return Some(Rc::new(RefCell::new(move || {
            dyn_sig.set(DynValue::Bool(false));
        })));
    }
    None
}

fn make_handler_opt(
    el: &ElementNode,
    event_names: &[&str],
    env: &DynEnv,
    extra_cb: Option<DynCallback>,
) -> Option<TapHandler> {
    let handler = el
        .handlers
        .iter()
        .find(|h| event_names.iter().any(|e| h.event == *e))?;
    let stmts = handler.body.clone();
    let env_c = env.clone();
    Some(Rc::new(RefCell::new(move || {
        run_stmts(&stmts, &env_c);
        if let Some(ref c) = extra_cb
            && let Ok(mut f) = c.try_borrow_mut()
        {
            f(Vec::new());
        }
    })))
}

fn run_stmts(stmts: &[Stmt], env: &DynEnv) {
    for stmt in stmts {
        match stmt {
            Stmt::Assign { target, value } => {
                let v = eval_expr(value, env);
                if let Some(&sig) = env.props.get(target) {
                    sig.set(v);
                }
            }
            Stmt::Call { callee, args } => {
                let eval_args: Vec<DynValue> = args.iter().map(|a| eval_expr(a, env)).collect();
                if let Some(cb) = env.callbacks.get(callee)
                    && let Ok(mut f) = cb.try_borrow_mut()
                {
                    f(eval_args);
                }
            }
            Stmt::Expr(_) => {}
        }
    }
}

fn eval_iterable(expr: &Expr, env: &DynEnv, track: bool) -> Vec<DynValue> {
    match eval_expr_tracked(expr, env, track) {
        DynValue::List(items) => items,
        DynValue::Int(n) if n > 0 => (0..n.min(10_000)).map(DynValue::Int).collect(),
        _ => Vec::new(),
    }
}

fn expr_has_reactivity(expr: &Expr, env: &DynEnv) -> bool {
    match expr {
        Expr::Ident(name) => env.props.contains_key(name),
        Expr::Member(base, _) => {
            matches!(base.as_ref(), Expr::Ident(r) if r == "Theme")
                || expr_has_reactivity(base, env)
        }
        Expr::Index(a, b) | Expr::Binary(a, _, b) => {
            expr_has_reactivity(a, env) || expr_has_reactivity(b, env)
        }
        Expr::Range { start, end, .. } => {
            expr_has_reactivity(start, env) || expr_has_reactivity(end, env)
        }
        Expr::UnaryNot(i) | Expr::UnaryNeg(i) => expr_has_reactivity(i, env),
        Expr::Array(items) | Expr::Call(_, items) => {
            items.iter().any(|x| expr_has_reactivity(x, env))
        }
        Expr::MethodCall(recv, _, args) => {
            expr_has_reactivity(recv, env) || args.iter().any(|x| expr_has_reactivity(x, env))
        }
        Expr::StructLit { fields, .. } => fields.iter().any(|(_, v)| expr_has_reactivity(v, env)),
        _ => false,
    }
}

fn expr_to_reactive_string(expr: &Expr, env: &DynEnv) -> ReactiveValue<String> {
    if expr_has_reactivity(expr, env) {
        let e = expr.clone();
        let env_c = env.clone();
        ReactiveValue::Dynamic(Rc::new(move || {
            eval_expr_tracked(&e, &env_c, true).as_str()
        }))
    } else {
        ReactiveValue::Static(eval_expr(expr, env).as_str())
    }
}

fn expr_to_reactive_bool(expr: &Expr, env: &DynEnv) -> ReactiveValue<bool> {
    if expr_has_reactivity(expr, env) {
        let e = expr.clone();
        let env_c = env.clone();
        ReactiveValue::Dynamic(Rc::new(move || {
            eval_expr_tracked(&e, &env_c, true).as_bool()
        }))
    } else {
        ReactiveValue::Static(eval_expr(expr, env).as_bool())
    }
}

fn expr_to_reactive_string_list(expr: &Expr, env: &DynEnv) -> ReactiveValue<Vec<String>> {
    if expr_has_reactivity(expr, env) {
        let e = expr.clone();
        let env_c = env.clone();
        ReactiveValue::Dynamic(Rc::new(move || {
            eval_expr_tracked(&e, &env_c, true).as_string_list()
        }))
    } else {
        ReactiveValue::Static(eval_expr(expr, env).as_string_list())
    }
}

fn expr_to_reactive_color(expr: &Expr, env: &DynEnv, fallback: Color) -> ReactiveValue<Color> {
    if expr_has_reactivity(expr, env) {
        let e = expr.clone();
        let env_c = env.clone();
        ReactiveValue::Dynamic(Rc::new(move || {
            eval_expr_tracked(&e, &env_c, true)
                .as_color()
                .unwrap_or(fallback)
        }))
    } else {
        ReactiveValue::Static(eval_expr(expr, env).as_color().unwrap_or(fallback))
    }
}

fn emit_reactive_text_style(
    size: f32,
    weight: f32,
    color_expr: &Expr,
    env: &DynEnv,
) -> ReactiveValue<TextStyle> {
    if expr_has_reactivity(color_expr, env) {
        let ce = color_expr.clone();
        let env_c = env.clone();
        ReactiveValue::Dynamic(Rc::new(move || {
            let col = eval_expr_tracked(&ce, &env_c, true)
                .as_color()
                .unwrap_or_else(|| env_c.theme_sig.get().text_primary);
            TextStyle::default()
                .with_size(size)
                .with_weight(weight)
                .with_color(col)
        }))
    } else {
        let col = eval_expr(color_expr, env)
            .as_color()
            .unwrap_or_else(|| env.theme_sig.get_untracked().text_primary);
        ReactiveValue::Static(
            TextStyle::default()
                .with_size(size)
                .with_weight(weight)
                .with_color(col),
        )
    }
}

fn eval_font_weight(expr: &Expr, env: &DynEnv) -> f32 {
    match expr {
        Expr::Ident(s) => match s.as_str() {
            "Thin" => 100.0,
            "ExtraLight" => 200.0,
            "Light" => 300.0,
            "Normal" | "Regular" => 400.0,
            "Medium" => 500.0,
            "SemiBold" | "Semibold" => 600.0,
            "Bold" => 700.0,
            "ExtraBold" => 800.0,
            "Black" => 900.0,
            _ => 400.0,
        },
        other => eval_expr(other, env).as_f32().max(100.0),
    }
}

fn eval_expr(expr: &Expr, env: &DynEnv) -> DynValue {
    eval_expr_tracked(expr, env, false)
}

fn eval_expr_tracked(expr: &Expr, env: &DynEnv, track: bool) -> DynValue {
    match expr {
        Expr::Int(i) => DynValue::Int(*i),
        Expr::Float(f) => DynValue::Float(*f),
        Expr::Bool(b) => DynValue::Bool(*b),
        Expr::String(s) => DynValue::String(s.clone()),
        Expr::ColorHex(h) => DynValue::Color(Color::from_hex(h).unwrap_or(Color::rgb(255, 0, 255))),
        Expr::Length(v, unit) => DynValue::Length(match unit {
            Unit::Dp => Length::Dp(*v),
            Unit::Px => Length::Px(*v),
            Unit::Pct => Length::Pct(*v),
        }),
        Expr::Array(items) => DynValue::List(
            items
                .iter()
                .map(|i| eval_expr_tracked(i, env, track))
                .collect(),
        ),
        Expr::Range {
            start,
            end,
            inclusive,
        } => {
            let s = eval_expr_tracked(start, env, track).as_i64();
            let e = eval_expr_tracked(end, env, track).as_i64();
            let items: Vec<DynValue> = if *inclusive {
                (s..=e).take(10_000).map(DynValue::Int).collect()
            } else {
                (s..e).take(10_000).map(DynValue::Int).collect()
            };
            DynValue::List(items)
        }
        Expr::StructLit { name, fields } => {
            let mut map = BTreeMap::new();
            if let Some(st) = env.file.structs.iter().find(|s| s.name == *name) {
                for f in &st.fields {
                    let def = f
                        .default_value
                        .as_ref()
                        .map(|e| eval_expr_tracked(e, env, track))
                        .unwrap_or(DynValue::Null);
                    map.insert(f.name.clone(), def);
                }
            }
            for (k, v) in fields {
                map.insert(k.clone(), eval_expr_tracked(v, env, track));
            }
            DynValue::Struct {
                name: name.clone(),
                fields: map,
            }
        }
        Expr::Ident(name) => {
            if let Some(sig) = env.props.get(name) {
                if track {
                    sig.get()
                } else {
                    sig.get_untracked()
                }
            } else if let Some(val) = env.locals.get(name) {
                val.clone()
            } else if name == "None" || name == "null" {
                DynValue::Null
            } else {
                DynValue::String(name.clone())
            }
        }
        Expr::Member(base, field) => {
            if let Expr::Ident(root) = base.as_ref() {
                if root == "Theme" {
                    let theme = if track {
                        env.theme_sig.get()
                    } else {
                        env.theme_sig.get_untracked()
                    };
                    if let Some(c) = theme.token_color(field) {
                        return DynValue::Color(c);
                    }
                    let ov = if track {
                        env.theme_ov_sig.get()
                    } else {
                        env.theme_ov_sig.get_untracked()
                    };
                    return DynValue::Color(ov.color(field).unwrap_or(theme.primary));
                }
                if root == "ThemeMode" || env.file.enums.iter().any(|e| e.name == *root) {
                    return DynValue::Enum(root.clone(), field.clone());
                }
            }
            match eval_expr_tracked(base, env, track) {
                DynValue::Struct { fields, .. } => {
                    fields.get(field).cloned().unwrap_or(DynValue::Null)
                }
                DynValue::List(v) if field == "len" => DynValue::Int(v.len() as i64),
                _ => DynValue::Null,
            }
        }
        Expr::Index(base, idx) => {
            let b = eval_expr_tracked(base, env, track);
            let i = eval_expr_tracked(idx, env, track).as_i64();
            if i < 0 {
                return DynValue::Null;
            }
            let iu = i as usize;
            match b {
                DynValue::List(items) => items.get(iu).cloned().unwrap_or(DynValue::Null),
                DynValue::String(s) => s
                    .chars()
                    .nth(iu)
                    .map(|c| DynValue::String(c.to_string()))
                    .unwrap_or(DynValue::Null),
                _ => DynValue::Null,
            }
        }
        Expr::MethodCall(recv, method, args) => {
            let r = eval_expr_tracked(recv, env, track);
            let eval_args: Vec<DynValue> = args
                .iter()
                .map(|a| eval_expr_tracked(a, env, track))
                .collect();
            match (r, method.as_str()) {
                (DynValue::List(v), "len") => DynValue::Int(v.len() as i64),
                (DynValue::List(v), "is_empty") => DynValue::Bool(v.is_empty()),
                (DynValue::List(v), "contains") => DynValue::Bool(
                    eval_args
                        .first()
                        .is_some_and(|needle| v.iter().any(|x| x == needle)),
                ),
                (DynValue::String(s), "len") => DynValue::Int(s.chars().count() as i64),
                (DynValue::String(s), "is_empty") => DynValue::Bool(s.is_empty()),
                (DynValue::String(s), "to_uppercase") => DynValue::String(s.to_uppercase()),
                (DynValue::String(s), "to_lowercase") => DynValue::String(s.to_lowercase()),
                (DynValue::String(s), "contains") => DynValue::Bool(
                    eval_args
                        .first()
                        .is_some_and(|needle| s.contains(&needle.as_str())),
                ),
                _ => DynValue::Null,
            }
        }
        Expr::Call(name, args) => {
            let eval_args: Vec<DynValue> = args
                .iter()
                .map(|a| eval_expr_tracked(a, env, track))
                .collect();
            match name.as_str() {
                "Some" => eval_args.into_iter().next().unwrap_or(DynValue::Null),
                "min" if eval_args.len() == 2 => {
                    if let (DynValue::Int(a), DynValue::Int(b)) = (&eval_args[0], &eval_args[1]) {
                        DynValue::Int((*a).min(*b))
                    } else {
                        DynValue::Float((eval_args[0].as_f32().min(eval_args[1].as_f32())) as f64)
                    }
                }
                "max" if eval_args.len() == 2 => {
                    if let (DynValue::Int(a), DynValue::Int(b)) = (&eval_args[0], &eval_args[1]) {
                        DynValue::Int((*a).max(*b))
                    } else {
                        DynValue::Float((eval_args[0].as_f32().max(eval_args[1].as_f32())) as f64)
                    }
                }
                "rgb" if eval_args.len() == 3 => DynValue::Color(Color::rgb(
                    eval_args[0].as_i64().clamp(0, 255) as u8,
                    eval_args[1].as_i64().clamp(0, 255) as u8,
                    eval_args[2].as_i64().clamp(0, 255) as u8,
                )),
                "rgba" if eval_args.len() == 4 => DynValue::Color(Color::rgba(
                    eval_args[0].as_i64().clamp(0, 255) as u8,
                    eval_args[1].as_i64().clamp(0, 255) as u8,
                    eval_args[2].as_i64().clamp(0, 255) as u8,
                    eval_args[3].as_i64().clamp(0, 255) as u8,
                )),
                _ => DynValue::Null,
            }
        }
        Expr::UnaryNot(inner) => DynValue::Bool(!eval_expr_tracked(inner, env, track).as_bool()),
        Expr::UnaryNeg(inner) => match eval_expr_tracked(inner, env, track) {
            DynValue::Int(v) => DynValue::Int(-v),
            DynValue::Float(v) => DynValue::Float(-v),
            DynValue::Length(Length::Dp(v)) => DynValue::Length(Length::Dp(-v)),
            DynValue::Length(Length::Px(v)) => DynValue::Length(Length::Px(-v)),
            DynValue::Length(Length::Pct(v)) => DynValue::Length(Length::Pct(-v)),
            other => DynValue::Float(-(other.as_f32() as f64)),
        },
        Expr::Binary(l, op, r) => {
            let lv = eval_expr_tracked(l, env, track);
            let rv = eval_expr_tracked(r, env, track);
            match op {
                BinOp::Eq => DynValue::Bool(lv == rv),
                BinOp::Ne => DynValue::Bool(lv != rv),
                BinOp::Lt => DynValue::Bool(lv.as_f32() < rv.as_f32()),
                BinOp::Gt => DynValue::Bool(lv.as_f32() > rv.as_f32()),
                BinOp::Le => DynValue::Bool(lv.as_f32() <= rv.as_f32()),
                BinOp::Ge => DynValue::Bool(lv.as_f32() >= rv.as_f32()),
                BinOp::And => DynValue::Bool(lv.as_bool() && rv.as_bool()),
                BinOp::Or => DynValue::Bool(lv.as_bool() || rv.as_bool()),
                BinOp::Add => match (&lv, &rv) {
                    (DynValue::String(a), _) => DynValue::String(format!("{a}{}", rv.as_str())),
                    (_, DynValue::String(b)) => DynValue::String(format!("{}{b}", lv.as_str())),
                    (DynValue::Int(a), DynValue::Int(b)) => DynValue::Int(a + b),
                    _ => DynValue::Float((lv.as_f32() + rv.as_f32()) as f64),
                },
                BinOp::Sub => match (&lv, &rv) {
                    (DynValue::Int(a), DynValue::Int(b)) => DynValue::Int(a - b),
                    _ => DynValue::Float((lv.as_f32() - rv.as_f32()) as f64),
                },
                BinOp::Mul => match (&lv, &rv) {
                    (DynValue::Int(a), DynValue::Int(b)) => DynValue::Int(a * b),
                    _ => DynValue::Float((lv.as_f32() * rv.as_f32()) as f64),
                },
                BinOp::Div => {
                    let d = rv.as_f32();
                    DynValue::Float(if d.abs() > 1e-6 {
                        (lv.as_f32() / d) as f64
                    } else {
                        0.0
                    })
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use craftui_core::{
        Modifiers, Point, PointerButton, PointerEvent, PointerId, PointerPhase, Size, ThemeMode,
        UiEvent, WindowMetrics,
    };
    use craftui_reactive::reset_runtime;
    use craftui_widgets::WidgetKind;
    use std::collections::BTreeMap;

    #[test]
    fn hot_reload_interprets_cui_under_100ms_and_preserves_state() {
        reset_runtime();
        let cx = Scope::root();
        let mut tree = WidgetTree::new(cx);
        let mut session = HotReloadSession::new(cx);

        let src_v1 = r#"
component Chip {
    in property <string> title: "Chip";
    Button { text: title; }
}
export component Demo {
    in-out property <int> count: 0;
    Column {
        width: 100%;
        height: 100%;
        padding: 16dp;
        for name, i in ["A", "B"] : Chip { title: name; }
        Button { text: "Save"; }
    }
}
"#;
        let root1 = session.reload(src_v1, Some("Demo"), &mut tree).unwrap();
        assert!(session.last_reload_duration.as_millis() < 100);
        session
            .store
            .set_prop(session.state_scope, "count", DynValue::Int(42));
        tree.layout(WindowMetrics {
            logical_size: Size::new(400.0, 300.0),
            ..Default::default()
        });
        assert_eq!(tree.rect(root1).width, 400.0);

        // Hot-reload modified template -> count=42 must persist!
        let src_v2 = src_v1.replace("16dp", "24dp");
        session.reload(&src_v2, Some("Demo"), &mut tree).unwrap();
        assert_eq!(
            session.store.props.get("count").unwrap().get(),
            DynValue::Int(42)
        );
    }

    #[test]
    fn reactive_if_else_for_two_way_subcomponents_theme_and_imports_work() {
        reset_runtime();
        let cx = Scope::root();
        let mut tree = WidgetTree::new(cx);
        tree.set_theme_mode(ThemeMode::Dark);
        let mut session = HotReloadSession::new(cx);

        let tmp = std::env::temp_dir().join(format!(
            "craftui_runtime_test_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&tmp).unwrap();
        std::fs::write(
            tmp.join("shared.cui"),
            r#"
export struct TaskEntry {
    title: string = "",
}
export component ToggleChip {
    in property <string> label: "Pin";
    in-out property <bool> active: false;
    callback toggled();
    Button {
        text: label;
        on_tap => {
            active = !active;
            toggled();
        }
    }
}
"#,
        )
        .unwrap();
        std::fs::write(
            tmp.join("app.cui"),
            r#"
import { TaskEntry, ToggleChip } from "./shared.cui"

export component MainApp {
    in-out property <bool> pinned: false;
    in-out property <bool> show_list: false;
    in-out property <[string]> tags: ["Rust", "UI"];
    in-out property <int> selected_tab: 0;
    in-out property <string> query: "hello";

    Column {
        width: 100%;
        height: 100%;
        background: Theme.background;
        border_width: 1dp;
        border_color: Theme.border;

        ToggleChip {
            label: "Toggle";
            active <=> pinned;
        }
        Tabs {
            tabs: ["One", "Two", "Three"];
            active <=> selected_tab;
        }
        TextInput {
            placeholder: "Search";
            value <=> query;
        }
        Divider { thickness: 1dp; }
        Icon { svg: "check"; size: 18dp; }

        if show_list : Column {
            for item, idx in tags : Text {
                text: item;
                color: Theme.text_primary;
            }
        } else : Text {
            text: "Empty";
            color: Theme.text_secondary;
        }
    }
}
"#,
        )
        .unwrap();

        let root = session
            .reload_file(tmp.join("app.cui"), Some("MainApp"), &mut tree)
            .unwrap();
        tree.layout(WindowMetrics {
            logical_size: Size::new(500.0, 400.0),
            ..Default::default()
        });

        // Initially else-branch "Empty" is mounted and theme background is Dark
        let root_bg = tree
            .nodes
            .get(root)
            .unwrap()
            .box_style
            .background
            .as_ref()
            .unwrap()
            .get();
        assert_eq!(
            root_bg,
            craftui_core::Brush::Solid(ThemeTokens::dark().background)
        );

        // Toggle Theme live -> background updates smoothly without hot-reload
        tree.set_theme_mode(ThemeMode::Light);
        for _ in 0..240 {
            if !tree.tick_animations(0.016) {
                break;
            }
        }
        let root_bg_light = tree
            .nodes
            .get(root)
            .unwrap()
            .box_style
            .background
            .as_ref()
            .unwrap()
            .get();
        assert_eq!(
            root_bg_light,
            craftui_core::Brush::Solid(ThemeTokens::light().background)
        );

        // Tap ToggleChip -> toggles `pinned` via `<=>`
        let mut btn_rect = None;
        for id in tree.nodes.keys() {
            if let Some(n) = tree.nodes.get(id)
                && matches!(n.kind, WidgetKind::Button { .. })
            {
                btn_rect = Some(tree.rect(id));
                break;
            }
        }
        let br = btn_rect.unwrap();
        let pt = Point::new(br.x + br.width * 0.5, br.y + br.height * 0.5);
        tree.handle_event(&UiEvent::Pointer(PointerEvent {
            id: PointerId(0),
            phase: PointerPhase::Down,
            position: pt,
            button: Some(PointerButton::Primary),
            modifiers: Modifiers::default(),
            is_touch: false,
            timestamp_ms: 0,
        }));
        tree.handle_event(&UiEvent::Pointer(PointerEvent {
            id: PointerId(0),
            phase: PointerPhase::Up,
            position: pt,
            button: Some(PointerButton::Primary),
            modifiers: Modifiers::default(),
            is_touch: false,
            timestamp_ms: 20,
        }));
        assert_eq!(
            session.store.props.get("pinned").unwrap().get(),
            DynValue::Bool(true)
        );

        // Switch show_list -> dynamic_slot swaps from "Empty" to reactive_for tags
        session
            .store
            .set_prop(session.state_scope, "show_list", DynValue::Bool(true));
        tree.layout(WindowMetrics {
            logical_size: Size::new(500.0, 400.0),
            ..Default::default()
        });
        let has_rust = tree.nodes.values().any(|n| match &n.kind {
            WidgetKind::Text { content, .. } => content.get() == "Rust",
            _ => false,
        });
        assert!(has_rust);

        // Mutate tags signal live -> reactive_for adds 3rd item
        session.store.set_prop(
            session.state_scope,
            "tags",
            DynValue::List(vec![
                DynValue::String("Rust".into()),
                DynValue::String("UI".into()),
                DynValue::String("Framework".into()),
            ]),
        );
        tree.layout(WindowMetrics {
            logical_size: Size::new(500.0, 400.0),
            ..Default::default()
        });
        let has_fw = tree.nodes.values().any(|n| match &n.kind {
            WidgetKind::Text { content, .. } => content.get() == "Framework",
            _ => false,
        });
        assert!(has_fw);

        let _ = std::fs::remove_dir_all(&tmp);
        let _ = BTreeMap::<String, DynValue>::new();
    }
}
