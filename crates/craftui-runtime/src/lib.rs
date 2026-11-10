//! Dynamic `.cui` AST/IR interpreter enabling `< 100ms` visual hot-reload without Rust recompilation.

use craftui_core::{
    Alignment, Axis, Brush, Color, CornerRadii, CraftError, Fit, Insets, Justify, Length, NodeId,
    Result, TextureId, ThemeTokens,
};
use craftui_dsl::{
    BinOp, ChildItem, ComponentDecl, CuiFile, Diagnostic, ElementNode, Expr, Severity, Stmt, Unit,
    parse, validate,
};
use craftui_layout::LayoutStyle;
use craftui_reactive::{Scope, Signal};
use craftui_text::TextStyle;
use craftui_widgets::{ReactiveValue, WidgetTree};
use std::cell::RefCell;
use std::collections::HashMap;
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
            _ => 0,
        }
    }

    pub fn as_bool(&self) -> bool {
        match self {
            Self::Bool(b) => *b,
            Self::Int(i) => *i != 0,
            _ => false,
        }
    }

    pub fn as_str(&self) -> String {
        match self {
            Self::String(s) => s.clone(),
            Self::Int(i) => i.to_string(),
            Self::Float(f) => format!("{f:.2}"),
            Self::Bool(b) => b.to_string(),
            Self::Color(c) => c.to_hex(),
            _ => String::new(),
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

    /// Parse, validate, and rebuild the `.cui` UI subtree while preserving live signal state.
    pub fn reload(
        &mut self,
        source: &str,
        component_name: Option<&str>,
        tree: &mut WidgetTree,
    ) -> Result<NodeId> {
        let t0 = Instant::now();
        let parsed = parse(source);
        let mut diags = parsed.diagnostics;
        diags.extend(validate(&parsed.file));
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
            parsed
                .file
                .components
                .iter()
                .find(|c| c.name == name)
                .or_else(|| parsed.file.components.last())
        } else {
            parsed.file.components.last()
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

        // Ensure component properties exist in preserved DynamicStore
        let mut locals = HashMap::new();
        for prop in &comp.properties {
            let init = prop
                .default_value
                .as_ref()
                .map(|e| eval_expr(e, &locals, &self.store.props, &tree.theme()))
                .unwrap_or(DynValue::Null);
            let sig = self
                .store
                .get_or_create_prop(self.state_scope, &prop.name, init);
            locals.insert(prop.name.clone(), sig.get_untracked());
        }

        let root_node = if let Some(ref root_el) = comp.root {
            build_element(
                mount_scope,
                root_el,
                &parsed.file,
                &locals,
                &self.store,
                tree,
            )
        } else {
            tree.column(mount_scope)
        };

        tree.set_root(root_node);
        self.last_source = source.to_string();
        self.last_reload_duration = t0.elapsed();
        Ok(root_node)
    }
}

fn build_element(
    scope: Scope,
    el: &ElementNode,
    file: &CuiFile,
    locals: &HashMap<String, DynValue>,
    store: &DynamicStore,
    tree: &mut WidgetTree,
) -> NodeId {
    // Check if element is a user-defined component in the same file
    if let Some(sub_comp) = file.components.iter().find(|c| c.name == el.kind) {
        return instantiate_subcomponent(scope, sub_comp, el, file, locals, store, tree);
    }

    let theme = tree.theme();
    let eval_prop = |name: &str| -> Option<DynValue> {
        el.props
            .iter()
            .find(|p| p.name == name)
            .map(|p| eval_expr(&p.value, locals, &store.props, &theme))
    };

    let node = match el.kind.as_str() {
        "Spacer" => tree.spacer(scope),
        "SafeArea" => tree.safe_area(scope),
        "Row" => tree.row(scope),
        "Grid" | "VirtualGrid" => {
            let cols = eval_prop("columns")
                .map(|v| v.as_i64().max(1) as u16)
                .unwrap_or(2);
            tree.grid(scope, cols, 0.0)
        }
        "Stack" => tree.stack(scope),
        "Text" => {
            let text_prop = el
                .props
                .iter()
                .find(|p| p.name == "text" || p.name == "title")
                .map(|p| p.value.clone());
            let size = eval_prop("font_size").map(|v| v.as_f32()).unwrap_or(16.0);
            let color = eval_prop("color")
                .or_else(|| eval_prop("foreground"))
                .and_then(|v| v.as_color())
                .unwrap_or(theme.text_primary);
            let rv = if let Some(expr) = text_prop {
                let locals_c = locals.clone();
                let props_c = store.props.clone();
                let theme_c = theme;
                ReactiveValue::Dynamic(Rc::new(move || {
                    eval_expr_tracked(&expr, &locals_c, &props_c, &theme_c, true).as_str()
                }))
            } else {
                ReactiveValue::Static(String::new())
            };
            tree.text(
                scope,
                rv,
                TextStyle::default().with_size(size).with_color(color),
            )
        }
        "Button" => {
            let text_prop = el
                .props
                .iter()
                .find(|p| p.name == "text" || p.name == "title")
                .map(|p| p.value.clone());
            let rv = if let Some(expr) = text_prop {
                let locals_c = locals.clone();
                let props_c = store.props.clone();
                let theme_c = theme;
                ReactiveValue::Dynamic(Rc::new(move || {
                    eval_expr_tracked(&expr, &locals_c, &props_c, &theme_c, true).as_str()
                }))
            } else {
                ReactiveValue::Static("Button".to_string())
            };
            let tap_handler = make_handler(el, "on_tap", locals, store, theme);
            let n = tree.button(scope, rv, move || {
                tap_handler();
            });
            if let Some(fg) = eval_prop("foreground").and_then(|v| v.as_color()) {
                tree.set_button_foreground(n, fg);
            }
            n
        }
        "ImageView" | "Image" => {
            let tex = eval_prop("source").and_then(|v| match v {
                DynValue::Texture(t) => t,
                _ => None,
            });
            let fit = match el.props.iter().find(|p| p.name == "fit").map(|p| &p.value) {
                Some(Expr::Ident(s)) if s == "Cover" => Fit::Cover,
                Some(Expr::Ident(s)) if s == "Fill" => Fit::Fill,
                Some(Expr::Ident(s)) if s == "None" => Fit::None,
                _ => Fit::Contain,
            };
            let rad = eval_prop("border_radius")
                .map(|v| CornerRadii::all(v.as_f32()))
                .unwrap_or(CornerRadii::ZERO);
            tree.image(scope, tex, fit, rad)
        }
        "ScrollView" => {
            let axis = match el
                .props
                .iter()
                .find(|p| p.name == "direction")
                .map(|p| &p.value)
            {
                Some(Expr::Ident(s)) if s == "Horizontal" => Axis::Horizontal,
                _ => Axis::Vertical,
            };
            let base_layout = match axis {
                Axis::Horizontal => LayoutStyle::row(),
                Axis::Vertical => LayoutStyle::column(),
            };
            tree.scroll_view(scope, axis, base_layout)
        }
        "Slider" => {
            let min = eval_prop("min").map(|v| v.as_f32()).unwrap_or(0.0);
            let max = eval_prop("max").map(|v| v.as_f32()).unwrap_or(100.0);
            let init = eval_prop("value").map(|v| v.as_f32()).unwrap_or(0.0);
            let sig = scope.signal(init);
            if let Some(val_prop) = el.props.iter().find(|p| p.name == "value" && p.two_way)
                && let Expr::Ident(target_name) = &val_prop.value
                && let Some(&dyn_sig) = store.props.get(target_name)
            {
                sig.set(dyn_sig.get_untracked().as_f32());
                scope.effect(move || {
                    dyn_sig.set(DynValue::Float(sig.get() as f64));
                });
            }
            let lbl = eval_prop("label").map(|v| v.as_str()).unwrap_or_default();
            tree.slider(scope, sig, min, max, lbl)
        }
        "TextInput" => {
            let placeholder = eval_prop("placeholder")
                .map(|v| v.as_str())
                .unwrap_or_default();
            tree.text_input(scope, "", placeholder, None)
        }
        "Checkbox" => {
            let init = eval_prop("checked").map(|v| v.as_bool()).unwrap_or(false);
            let sig = scope.signal(init);
            let lbl = eval_prop("label").map(|v| v.as_str()).unwrap_or_default();
            tree.checkbox(scope, sig, lbl)
        }
        "Switch" => {
            let init = eval_prop("checked").map(|v| v.as_bool()).unwrap_or(false);
            let sig = scope.signal(init);
            let lbl = eval_prop("label").map(|v| v.as_str()).unwrap_or_default();
            tree.switch(scope, sig, lbl)
        }
        "ProgressIndicator" => {
            let p = eval_prop("progress").map(|v| v.as_f32());
            tree.progress_indicator(scope, p)
        }
        _ => tree.column(scope),
    };

    // Patch ONLY explicitly specified layout & visual properties onto the widget's defaults.
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
    if let Some(p_binding) = el.props.iter().find(|p| p.name == "padding") {
        if let Expr::Member(base, field) = &p_binding.value
            && matches!(base.as_ref(), Expr::Ident(s) if s == "SafeArea")
            && field == "insets"
        {
            layout.safe_area = true;
        } else {
            let v = eval_expr(&p_binding.value, locals, &store.props, &theme).as_f32();
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

    if let Some(v) = eval_prop("background")
        && let Some(c) = v.as_color()
    {
        box_style.background = Some(ReactiveValue::Static(Brush::Solid(c)));
        box_dirty = true;
    }
    if let Some(v) = eval_prop("border_radius") {
        let rad = CornerRadii::all(v.as_f32());
        box_style.radius = rad;
        tree.set_image_radius(node, rad);
        box_dirty = true;
    }
    if let Some(v) = eval_prop("opacity") {
        box_style.opacity = v.as_f32();
        box_dirty = true;
    }

    if layout_dirty {
        tree.set_layout(node, layout);
    }
    if box_dirty {
        tree.set_box_style(node, box_style);
    }

    for child in &el.children {
        match child {
            ChildItem::Element(c) => {
                let cnode = build_element(scope.child_scope(), c, file, locals, store, tree);
                tree.append_child(node, cnode);
            }
            ChildItem::For {
                item_var,
                index_var,
                iterable,
                body,
                ..
            } => {
                if let DynValue::List(items) = eval_expr(iterable, locals, &store.props, &theme) {
                    for (idx, item) in items.into_iter().enumerate() {
                        let mut child_locals = locals.clone();
                        child_locals.insert(item_var.clone(), item);
                        if let Some(iv) = index_var {
                            child_locals.insert(iv.clone(), DynValue::Int(idx as i64));
                        }
                        let cnode = build_element(
                            scope.child_scope(),
                            body,
                            file,
                            &child_locals,
                            store,
                            tree,
                        );
                        tree.append_child(node, cnode);
                    }
                }
            }
            ChildItem::If {
                condition,
                then_branch,
                else_branch,
                ..
            } => {
                if eval_expr(condition, locals, &store.props, &theme).as_bool() {
                    let cnode =
                        build_element(scope.child_scope(), then_branch, file, locals, store, tree);
                    tree.append_child(node, cnode);
                } else if let Some(eb) = else_branch {
                    let cnode = build_element(scope.child_scope(), eb, file, locals, store, tree);
                    tree.append_child(node, cnode);
                }
            }
        }
    }

    node
}

fn instantiate_subcomponent(
    scope: Scope,
    comp: &ComponentDecl,
    call_el: &ElementNode,
    file: &CuiFile,
    outer_locals: &HashMap<String, DynValue>,
    store: &DynamicStore,
    tree: &mut WidgetTree,
) -> NodeId {
    let theme = tree.theme();
    let mut sub_locals = HashMap::new();
    for p in &comp.properties {
        let v = p
            .default_value
            .as_ref()
            .map(|e| eval_expr(e, outer_locals, &store.props, &theme))
            .unwrap_or(DynValue::Null);
        sub_locals.insert(p.name.clone(), v);
    }
    for prop in &call_el.props {
        let v = eval_expr(&prop.value, outer_locals, &store.props, &theme);
        sub_locals.insert(prop.name.clone(), v);
    }
    if let Some(ref root) = comp.root {
        let node = build_element(scope, root, file, &sub_locals, store, tree);
        if !call_el.handlers.is_empty() {
            let h = make_handler(call_el, "on_tap", outer_locals, store, theme);
            tree.set_on_tap(node, h);
        }
        node
    } else {
        tree.column(scope)
    }
}

fn make_handler(
    el: &ElementNode,
    event_name: &str,
    locals: &HashMap<String, DynValue>,
    store: &DynamicStore,
    theme: ThemeTokens,
) -> Box<dyn Fn()> {
    let Some(handler) = el.handlers.iter().find(|h| h.event == event_name) else {
        return Box::new(|| {});
    };
    let stmts = handler.body.clone();
    let locals_snap = locals.clone();
    let props_snap = store.props.clone();
    let cbs_snap = store.callbacks.clone();

    Box::new(move || {
        for stmt in &stmts {
            match stmt {
                Stmt::Assign { target, value } => {
                    let v = eval_expr(value, &locals_snap, &props_snap, &theme);
                    if let Some(&sig) = props_snap.get(target) {
                        sig.set(v);
                    }
                }
                Stmt::Call { callee, args } => {
                    let eval_args: Vec<DynValue> = args
                        .iter()
                        .map(|a| eval_expr(a, &locals_snap, &props_snap, &theme))
                        .collect();
                    if let Some(cb) = cbs_snap.get(callee)
                        && let Ok(mut f) = cb.try_borrow_mut()
                    {
                        f(eval_args);
                    }
                }
                Stmt::Expr(_) => {}
            }
        }
    })
}

fn eval_expr(
    expr: &Expr,
    locals: &HashMap<String, DynValue>,
    props: &HashMap<String, Signal<DynValue>>,
    theme: &ThemeTokens,
) -> DynValue {
    eval_expr_tracked(expr, locals, props, theme, false)
}

fn eval_expr_tracked(
    expr: &Expr,
    locals: &HashMap<String, DynValue>,
    props: &HashMap<String, Signal<DynValue>>,
    theme: &ThemeTokens,
    track: bool,
) -> DynValue {
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
                .map(|i| eval_expr_tracked(i, locals, props, theme, track))
                .collect(),
        ),
        Expr::Ident(name) => {
            if let Some(sig) = props.get(name) {
                if track {
                    sig.get()
                } else {
                    sig.get_untracked()
                }
            } else if let Some(val) = locals.get(name) {
                val.clone()
            } else {
                DynValue::String(name.clone())
            }
        }
        Expr::Member(base, field) => {
            if let Expr::Ident(root) = base.as_ref()
                && root == "Theme"
            {
                let c = match field.as_str() {
                    "background" => theme.background,
                    "surface" => theme.surface,
                    "surface_variant" => theme.surface_variant,
                    "border" => theme.border,
                    "primary" => theme.primary,
                    "on_primary" => theme.on_primary,
                    "text_primary" => theme.text_primary,
                    "text_secondary" => theme.text_secondary,
                    "danger" => theme.danger,
                    _ => theme.primary,
                };
                return DynValue::Color(c);
            }
            DynValue::Null
        }
        Expr::UnaryNot(inner) => {
            DynValue::Bool(!eval_expr_tracked(inner, locals, props, theme, track).as_bool())
        }
        Expr::Binary(l, op, r) => {
            let lv = eval_expr_tracked(l, locals, props, theme, track);
            let rv = eval_expr_tracked(r, locals, props, theme, track);
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
        Expr::Call(_, _) => DynValue::Null,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use craftui_core::{Size, WindowMetrics};
    use craftui_reactive::reset_runtime;

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
        assert!(tree.rect(root1).width == 400.0);

        // Hot-reload modified template -> count=42 must persist!
        let src_v2 = src_v1.replace("16dp", "24dp");
        session.reload(&src_v2, Some("Demo"), &mut tree).unwrap();
        assert_eq!(
            session.store.props.get("count").unwrap().get(),
            DynValue::Int(42)
        );
    }
}
