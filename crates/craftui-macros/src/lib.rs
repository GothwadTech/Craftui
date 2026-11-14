//! Procedural macros (`cui!` and `#[component]`) for pure-Rust declarative UI authoring.

use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use quote::{format_ident, quote};
use syn::parse::{Parse, ParseStream};
use syn::punctuated::Punctuated;
use syn::{Expr, Ident, ItemFn, Lit, Member, Token, braced, parenthesized};

struct PropArg {
    name: Ident,
    two_way: bool,
    value: Expr,
}

impl Parse for PropArg {
    fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
        let name: Ident = input.parse()?;
        let two_way = if input.peek(Token![:]) {
            input.parse::<Token![:]>()?;
            false
        } else if input.peek(Token![<=]) {
            input.parse::<Token![<=]>()?;
            input.parse::<Token![>]>()?;
            true
        } else {
            return Err(input.error("expected `:` or `<=>` after property name"));
        };
        let value: Expr = input.parse()?;
        Ok(Self {
            name,
            two_way,
            value,
        })
    }
}

struct CuiAnimateSpec {
    prop: Ident,
    curve: Option<Expr>,
    duration: Option<Expr>,
    easing: Option<Expr>,
    stiffness: Option<Expr>,
    damping: Option<Expr>,
}

impl CuiAnimateSpec {
    fn parse_after_keyword(input: ParseStream<'_>) -> syn::Result<Self> {
        let prop: Ident = input.parse()?;
        let content;
        braced!(content in input);
        let mut spec = Self {
            prop,
            curve: None,
            duration: None,
            easing: None,
            stiffness: None,
            damping: None,
        };
        while !content.is_empty() {
            let key: Ident = content.parse()?;
            content.parse::<Token![:]>()?;
            let val: Expr = content.parse()?;
            match key.to_string().as_str() {
                "curve" => spec.curve = Some(val),
                "duration" => spec.duration = Some(val),
                "easing" => spec.easing = Some(val),
                "stiffness" => spec.stiffness = Some(val),
                "damping" => spec.damping = Some(val),
                other => {
                    return Err(syn::Error::new_spanned(
                        key,
                        format!("unknown `animate` field `{other}`"),
                    ));
                }
            }
            if content.peek(Token![,]) {
                content.parse::<Token![,]>()?;
            } else if content.peek(Token![;]) {
                content.parse::<Token![;]>()?;
            }
        }
        Ok(spec)
    }
}

enum CuiChild {
    Element(CuiElement),
    If {
        cond: Expr,
        then_children: Vec<CuiChild>,
        else_children: Option<Vec<CuiChild>>,
    },
    For {
        item_ident: Ident,
        idx_ident: Option<Ident>,
        iterable: Expr,
        key_expr: Option<Expr>,
        body: Vec<CuiChild>,
    },
}

struct CuiElement {
    kind: Ident,
    args: Vec<PropArg>,
    animations: Vec<CuiAnimateSpec>,
    children: Vec<CuiChild>,
}

fn parse_child_block(input: ParseStream<'_>) -> syn::Result<Vec<CuiChild>> {
    let content;
    braced!(content in input);
    let mut children = Vec::new();
    while !content.is_empty() {
        children.push(content.parse()?);
    }
    Ok(children)
}

impl Parse for CuiChild {
    fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
        if input.peek(Token![if]) {
            input.parse::<Token![if]>()?;
            let cond = Expr::parse_without_eager_brace(input)?;
            let then_children = parse_child_block(input)?;
            let else_children = if input.peek(Token![else]) {
                input.parse::<Token![else]>()?;
                if input.peek(Token![if]) {
                    let nested = CuiChild::parse(input)?;
                    Some(vec![nested])
                } else {
                    Some(parse_child_block(input)?)
                }
            } else {
                None
            };
            return Ok(CuiChild::If {
                cond,
                then_children,
                else_children,
            });
        }

        if input.peek(Token![for]) {
            input.parse::<Token![for]>()?;
            let (item_ident, idx_ident) = if input.peek(syn::token::Paren) {
                let content;
                parenthesized!(content in input);
                let item: Ident = content.parse()?;
                let idx = if content.peek(Token![,]) {
                    content.parse::<Token![,]>()?;
                    if content.is_empty() {
                        None
                    } else {
                        Some(content.parse::<Ident>()?)
                    }
                } else {
                    None
                };
                (item, idx)
            } else {
                let item: Ident = input.parse()?;
                let idx = if input.peek(Token![,]) {
                    input.parse::<Token![,]>()?;
                    Some(input.parse::<Ident>()?)
                } else {
                    None
                };
                (item, idx)
            };
            input.parse::<Token![in]>()?;
            let iterable = Expr::parse_without_eager_brace(input)?;
            if input.peek(Token![,]) {
                input.parse::<Token![,]>()?;
            }
            let key_expr = {
                let fk = input.fork();
                if fk.parse::<Ident>().is_ok_and(|id| id == "key") && fk.peek(Token![:]) {
                    let _k: Ident = input.parse()?;
                    input.parse::<Token![:]>()?;
                    Some(Expr::parse_without_eager_brace(input)?)
                } else {
                    None
                }
            };
            let body = parse_child_block(input)?;
            return Ok(CuiChild::For {
                item_ident,
                idx_ident,
                iterable,
                key_expr,
                body,
            });
        }

        Ok(CuiChild::Element(input.parse()?))
    }
}

impl Parse for CuiElement {
    fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
        let kind: Ident = input.parse()?;
        let mut args = Vec::new();
        if input.peek(syn::token::Paren) {
            let content;
            parenthesized!(content in input);
            let parsed: Punctuated<PropArg, Token![,]> = Punctuated::parse_terminated(&content)?;
            args.extend(parsed);
        }
        let mut animations = Vec::new();
        let mut children = Vec::new();
        if input.peek(syn::token::Brace) {
            let content;
            braced!(content in input);
            while !content.is_empty() {
                let is_animate_block = {
                    let fk = content.fork();
                    fk.parse::<Ident>().is_ok_and(|id| id == "animate")
                        && fk.parse::<Ident>().is_ok()
                        && fk.peek(syn::token::Brace)
                };
                if is_animate_block {
                    let _kw: Ident = content.parse()?;
                    animations.push(CuiAnimateSpec::parse_after_keyword(&content)?);
                } else {
                    children.push(content.parse()?);
                }
            }
        }
        Ok(Self {
            kind,
            args,
            animations,
            children,
        })
    }
}

/// Inspect whether `expr` reads a standard `theme.<token>` or `Theme.<token>` field so `cui!` can emit a live reactive binding.
fn extract_theme_token_field(expr: &Expr) -> Option<Ident> {
    if let Expr::Field(ef) = expr
        && let Expr::Path(ep) = ef.base.as_ref()
        && (ep.path.is_ident("theme") || ep.path.is_ident("Theme"))
        && let Member::Named(member) = &ef.member
    {
        let m = member.to_string();
        if matches!(
            m.as_str(),
            "background"
                | "surface"
                | "surface_variant"
                | "border"
                | "primary"
                | "on_primary"
                | "text_primary"
                | "text_secondary"
                | "danger"
        ) {
            return Some(member.clone());
        }
    }
    None
}

fn reactive_color_tokens(expr: &Expr) -> TokenStream2 {
    if let Some(field) = extract_theme_token_field(expr) {
        quote! {
            ::craftui::widgets::ReactiveValue::Dynamic(::std::rc::Rc::new(move || __cui_theme_sig.get().#field))
        }
    } else if matches!(expr, Expr::Closure(_)) {
        quote! {
            ::craftui::widgets::ReactiveValue::Dynamic(::std::rc::Rc::new(#expr))
        }
    } else {
        quote! {
            ::craftui::widgets::ReactiveValue::from(#expr)
        }
    }
}

fn reactive_brush_tokens(expr: &Expr) -> TokenStream2 {
    if let Some(field) = extract_theme_token_field(expr) {
        quote! {
            ::craftui::widgets::ReactiveValue::Dynamic(::std::rc::Rc::new(move || {
                ::craftui::core::Brush::Solid(__cui_theme_sig.get().#field)
            }))
        }
    } else if matches!(expr, Expr::Closure(_)) {
        quote! {
            ::craftui::widgets::ReactiveValue::Dynamic(::std::rc::Rc::new(move || {
                ::craftui::core::Brush::Solid((#expr)())
            }))
        }
    } else {
        quote! {
            ::craftui::widgets::ReactiveValue::Static(::craftui::core::Brush::Solid(#expr))
        }
    }
}

fn eval_scalar_expr(expr: &Expr) -> TokenStream2 {
    if matches!(expr, Expr::Closure(_)) {
        quote!(((#expr)() as f32))
    } else {
        quote!(((#expr) as f32))
    }
}

fn eval_typed_expr(expr: &Expr) -> TokenStream2 {
    if matches!(expr, Expr::Closure(_)) {
        quote!(((#expr)()))
    } else {
        quote!((#expr))
    }
}

fn is_two_way_signal_expr(arg: &PropArg) -> bool {
    arg.two_way || !matches!(arg.value, Expr::Closure(_))
}

/// Inline declarative Rust UI builder macro:
///
/// ```ignore
/// cui! {
///     Column(width: pct(100), height: pct(100), bg: theme.background) {
///         Text(text: "Hello", color: theme.text_primary)
///         Button(label: "Save", bg: theme.primary, on_tap: move || { ... })
///     }
/// }
/// ```
#[proc_macro]
pub fn cui(input: TokenStream) -> TokenStream {
    let root = syn::parse_macro_input!(input as CuiElement);
    let mut counter = 0usize;
    let mut stmts = TokenStream2::new();
    let root_var = emit_cui_node(&root, &mut counter, &mut stmts);
    let expanded = quote! {
        ::craftui::widgets::ViewFn::new(move |__cx: ::craftui::reactive::Scope, __tree: &mut ::craftui::widgets::WidgetTree| -> ::craftui::core::NodeId {
            #[allow(unused_variables)]
            let __cui_theme_sig = __tree.theme_signal;
            #stmts
            #root_var
        })
    };
    expanded.into()
}

fn emit_child_list(
    children: &[CuiChild],
    slot_layout: &TokenStream2,
    counter: &mut usize,
    stmts: &mut TokenStream2,
) -> Ident {
    if children.is_empty() {
        let var = format_ident!("__cui_node_{}", *counter);
        *counter += 1;
        stmts.extend(quote! { let #var = __tree.spacer(__cx); });
        return var;
    }
    if children.len() == 1
        && let CuiChild::Element(el) = &children[0]
    {
        return emit_cui_node(el, counter, stmts);
    }
    let container_var = format_ident!("__cui_node_{}", *counter);
    *counter += 1;
    stmts.extend(quote! {
        let #container_var = __tree.container(__cx, #slot_layout, ::craftui::widgets::BoxStyle::new());
    });
    for child in children {
        let cvar = emit_cui_child(child, slot_layout, counter, stmts);
        stmts.extend(quote! {
            __tree.append_child(#container_var, #cvar);
        });
    }
    container_var
}

fn emit_cui_child(
    child: &CuiChild,
    parent_slot_layout: &TokenStream2,
    counter: &mut usize,
    stmts: &mut TokenStream2,
) -> Ident {
    match child {
        CuiChild::Element(el) => emit_cui_node(el, counter, stmts),
        CuiChild::If {
            cond,
            then_children,
            else_children,
        } => {
            if let Some(else_ch) = else_children {
                let host_var = format_ident!("__cui_if_host_{}", *counter);
                *counter += 1;
                let mut then_stmts = TokenStream2::new();
                let then_var =
                    emit_child_list(then_children, parent_slot_layout, counter, &mut then_stmts);
                let mut else_stmts = TokenStream2::new();
                let else_var =
                    emit_child_list(else_ch, parent_slot_layout, counter, &mut else_stmts);
                stmts.extend(quote! {
                    let #host_var = __tree.dynamic_slot(
                        __cx,
                        #parent_slot_layout,
                        move || -> bool { #cond },
                        move |__cx: ::craftui::reactive::Scope, &__cui_is_then: &bool, __tree: &mut ::craftui::widgets::WidgetTree| -> ::craftui::core::NodeId {
                            #[allow(unused_variables)]
                            let __cui_theme_sig = __tree.theme_signal;
                            if __cui_is_then {
                                #then_stmts
                                #then_var
                            } else {
                                #else_stmts
                                #else_var
                            }
                        },
                    );
                });
                host_var
            } else {
                let cvar = emit_child_list(then_children, parent_slot_layout, counter, stmts);
                if matches!(cond, Expr::Closure(_)) {
                    stmts.extend(quote! { __tree.set_visible_when(#cvar, #cond); });
                } else {
                    stmts.extend(
                        quote! { __tree.set_visible_when(#cvar, move || -> bool { #cond }); },
                    );
                }
                cvar
            }
        }
        CuiChild::For {
            item_ident,
            idx_ident,
            iterable,
            key_expr,
            body,
        } => {
            let host_var = format_ident!("__cui_for_host_{}", *counter);
            *counter += 1;
            let model_rv = if matches!(iterable, Expr::Closure(_)) {
                quote! { ::craftui::widgets::ReactiveValue::Dynamic(::std::rc::Rc::new(#iterable)) }
            } else {
                quote! { ::craftui::widgets::ReactiveValue::from(#iterable) }
            };

            let key_closure = if let Some(ke) = key_expr {
                let idx_bind = idx_ident
                    .as_ref()
                    .map(|iv| quote! { #[allow(unused_variables)] let #iv = __k_idx; })
                    .unwrap_or_default();
                quote! {
                    move |__k_idx: usize, __k_it: &_| {
                        #[allow(unused_variables)]
                        let #item_ident = __k_it;
                        #idx_bind
                        ::core::clone::Clone::clone(&(#ke))
                    }
                }
            } else {
                quote! {
                    move |__k_idx: usize, _k_it: &_| __k_idx
                }
            };

            let idx_param = idx_ident
                .clone()
                .unwrap_or_else(|| format_ident!("__cui_unused_idx"));
            let mut body_stmts = TokenStream2::new();
            let body_ret = emit_child_list(body, parent_slot_layout, counter, &mut body_stmts);

            stmts.extend(quote! {
                let #host_var = {
                    let __cui_model_rv = #model_rv;
                    __tree.reactive_for_keyed(
                        __cx,
                        #parent_slot_layout,
                        move || __cui_model_rv.get(),
                        #key_closure,
                        move |__cx, #item_ident, #idx_param, __tree| {
                            #[allow(unused_variables)]
                            let __cui_theme_sig = __tree.theme_signal;
                            #body_stmts
                            #body_ret
                        },
                    )
                };
            });
            host_var
        }
    }
}

fn animatable_prop_tokens(name: &Ident) -> Vec<TokenStream2> {
    match name.to_string().as_str() {
        "opacity" => vec![quote!(::craftui::widgets::AnimatableProp::Opacity)],
        "translate_x" => vec![quote!(::craftui::widgets::AnimatableProp::TranslateX)],
        "translate_y" => vec![quote!(::craftui::widgets::AnimatableProp::TranslateY)],
        "scale" => vec![
            quote!(::craftui::widgets::AnimatableProp::ScaleX),
            quote!(::craftui::widgets::AnimatableProp::ScaleY),
        ],
        "scale_x" => vec![quote!(::craftui::widgets::AnimatableProp::ScaleX)],
        "scale_y" => vec![quote!(::craftui::widgets::AnimatableProp::ScaleY)],
        "rotate" | "rotation" => vec![quote!(::craftui::widgets::AnimatableProp::Rotation)],
        "background" | "bg" => vec![quote!(::craftui::widgets::AnimatableProp::BackgroundColor)],
        "border_color" => vec![quote!(::craftui::widgets::AnimatableProp::BorderColor)],
        _ => Vec::new(),
    }
}

fn easing_expr_tokens(expr: &Expr) -> TokenStream2 {
    if let Expr::Path(ep) = expr
        && let Some(ident) = ep.path.get_ident()
    {
        return match ident.to_string().as_str() {
            "Linear" => quote!(::craftui::widgets::Easing::Linear),
            "EaseIn" => quote!(::craftui::widgets::Easing::EaseIn),
            "EaseOut" => quote!(::craftui::widgets::Easing::EaseOut),
            "EaseInOut" => quote!(::craftui::widgets::Easing::EaseInOut),
            _ => quote!(#expr),
        };
    }
    quote!(#expr)
}

fn animate_curve_tokens(spec: &CuiAnimateSpec) -> TokenStream2 {
    if let Some(ref c) = spec.curve {
        return quote!(#c);
    }
    if spec.stiffness.is_some() || spec.damping.is_some() {
        let s = spec
            .stiffness
            .as_ref()
            .map(|e| quote!(#e as f32))
            .unwrap_or_else(|| quote!(320.0f32));
        let d = spec
            .damping
            .as_ref()
            .map(|e| quote!(#e as f32))
            .unwrap_or_else(|| quote!(28.0f32));
        return quote!(::craftui::widgets::TransitionCurve::spring(#s, #d));
    }
    let dur = spec
        .duration
        .as_ref()
        .map(|e| quote!(#e as f32))
        .unwrap_or_else(|| quote!(0.2f32));
    let eas = spec
        .easing
        .as_ref()
        .map(easing_expr_tokens)
        .unwrap_or_else(|| quote!(::craftui::widgets::Easing::EaseOut));
    quote!(::craftui::widgets::TransitionCurve::tween(#dur, #eas))
}

fn emit_cui_node(el: &CuiElement, counter: &mut usize, stmts: &mut TokenStream2) -> Ident {
    let var = format_ident!("__cui_node_{}", *counter);
    *counter += 1;

    let kind_str = el.kind.to_string();
    let find_prop = |names: &[&str]| -> Option<&PropArg> {
        el.args.iter().find(|a| names.iter().any(|n| a.name == *n))
    };
    let find_arg = |names: &[&str]| -> Option<&Expr> { find_prop(names).map(|a| &a.value) };

    let init_stmt = match kind_str.as_str() {
        "Row" => quote! { let #var = __tree.row(__cx); },
        "Column" | "View" | "Container" | "div" | "section" | "nav" => {
            quote! { let #var = __tree.column(__cx); }
        }
        "Grid" => {
            let cols = find_arg(&["columns", "cols"])
                .map(|e| quote!(#e as u16))
                .unwrap_or_else(|| quote!(2u16));
            let gap = find_arg(&["gap"])
                .map(|e| quote!(#e as f32))
                .unwrap_or_else(|| quote!(8.0f32));
            quote! { let #var = __tree.grid(__cx, #cols, #gap); }
        }
        "Stack" => quote! { let #var = __tree.stack(__cx); },
        "Spacer" => quote! { let #var = __tree.spacer(__cx); },
        "SafeArea" => quote! { let #var = __tree.safe_area(__cx); },
        "Divider" => {
            let axis = find_arg(&["axis", "direction"])
                .map(|e| quote!(#e))
                .unwrap_or_else(|| quote!(::craftui::core::Axis::Horizontal));
            let thickness = find_arg(&["thickness", "size"])
                .map(|e| quote!(#e as f32))
                .unwrap_or_else(|| quote!(1.0f32));
            let color_stmt = find_arg(&["color", "fg", "foreground"]).map(|c| {
                let rv = reactive_color_tokens(c);
                quote! { __tree.set_divider_color(#var, #rv); }
            });
            quote! {
                let #var = __tree.divider(__cx, #axis, #thickness);
                #color_stmt
            }
        }
        "Icon" => {
            let svg_expr = find_arg(&["svg", "icon", "name", "source"])
                .map(|e| quote!(#e))
                .unwrap_or_else(|| quote!("info"));
            let size_expr = find_arg(&["size", "font_size"])
                .map(|e| quote!(#e as f32))
                .unwrap_or_else(|| quote!(20.0f32));
            let col_opt = if let Some(c) = find_arg(&["color", "fg", "foreground"]) {
                let rv = reactive_color_tokens(c);
                quote!(Some(#rv))
            } else {
                quote!(Some(::craftui::widgets::ReactiveValue::Dynamic(
                    ::std::rc::Rc::new(move || __cui_theme_sig.get().text_primary)
                )))
            };
            quote! { let #var = __tree.icon(__cx, #svg_expr, #size_expr, #col_opt); }
        }
        "Text" | "p" | "h1" | "h2" | "h3" | "span" => {
            let text_expr = find_arg(&["text", "label", "content"])
                .map(|e| quote!(#e))
                .unwrap_or_else(|| quote!(""));
            let size_expr = find_arg(&["size", "font_size"]);
            let color_expr = find_arg(&["color", "fg", "foreground"]);
            let weight_expr = find_arg(&["weight", "font_weight"]);

            let mut base_style = quote! { let mut __ts = ::craftui::text::TextStyle::default(); };
            if let Some(s) = size_expr {
                base_style.extend(quote! { __ts.font_size = #s as f32; });
            }
            if let Some(w) = weight_expr {
                base_style.extend(quote! { __ts.font_weight = #w as f32; });
            }

            let style_rv = if let Some(c) = color_expr {
                if let Some(field) = extract_theme_token_field(c) {
                    quote! {
                        {
                            #base_style
                            ::craftui::widgets::ReactiveValue::Dynamic(::std::rc::Rc::new(move || {
                                __ts.clone().with_color(__cui_theme_sig.get().#field)
                            }))
                        }
                    }
                } else if matches!(c, Expr::Closure(_)) {
                    quote! {
                        {
                            #base_style
                            ::craftui::widgets::ReactiveValue::Dynamic(::std::rc::Rc::new(move || {
                                __ts.clone().with_color((#c)())
                            }))
                        }
                    }
                } else {
                    quote! {
                        {
                            #base_style
                            __ts.color = #c;
                            __ts
                        }
                    }
                }
            } else {
                quote! {
                    {
                        #base_style
                        ::craftui::widgets::ReactiveValue::Dynamic(::std::rc::Rc::new(move || {
                            __ts.clone().with_color(__cui_theme_sig.get().text_primary)
                        }))
                    }
                }
            };
            quote! { let #var = __tree.text(__cx, #text_expr, #style_rv); }
        }
        "Button" | "button" => {
            let label = find_arg(&["label", "text", "title"])
                .map(|e| quote!(#e))
                .unwrap_or_else(|| {
                    if el.children.is_empty() {
                        quote!("Button")
                    } else {
                        quote!("")
                    }
                });
            let on_tap = find_arg(&["on_tap", "on_click"])
                .map(|e| quote!(#e))
                .unwrap_or_else(|| quote!(|| {}));
            let fg_stmt = find_arg(&["fg", "foreground", "color"]).map(|c| {
                let rv = reactive_color_tokens(c);
                quote! { __tree.set_button_foreground(#var, #rv); }
            });
            quote! {
                let #var = __tree.button(__cx, #label, #on_tap);
                #fg_stmt
            }
        }
        "ImageView" | "Image" => {
            let src = find_arg(&["source", "src", "texture"])
                .map(|e| {
                    quote!(::craftui::widgets::ReactiveValue::Dynamic(::std::rc::Rc::new(move || Some(#e.get().id))))
                })
                .unwrap_or_else(|| quote!(::craftui::widgets::ReactiveValue::Static(None)));
            let fit = find_arg(&["fit"])
                .map(|e| quote!(#e))
                .unwrap_or_else(|| quote!(::craftui::core::Fit::Contain));
            let rad = find_arg(&["radius", "border_radius"])
                .map(|e| quote!(::craftui::core::CornerRadii::all(#e as f32)))
                .unwrap_or_else(|| quote!(::craftui::core::CornerRadii::ZERO));
            quote! { let #var = __tree.image(__cx, #src, #fit, #rad); }
        }
        "Canvas" => {
            let on_paint = find_arg(&["on_paint"])
                .map(|e| quote!(#e))
                .unwrap_or_else(|| quote!(|_, _| {}));
            let on_pointer = find_arg(&["on_pointer"])
                .map(|e| quote!(Some(::std::rc::Rc::new(#e))))
                .unwrap_or_else(|| quote!(None));
            quote! {
                let #var = __tree.canvas(__cx, ::craftui::layout::LayoutStyle::column(), #on_paint, #on_pointer);
            }
        }
        "Slider" => {
            let val_prop = find_prop(&["value"]).expect("Slider requires `value` signal");
            let val = &val_prop.value;
            let val_sig = if is_two_way_signal_expr(val_prop) {
                quote!(#val)
            } else {
                quote! {{
                    let __s = __cx.signal((#val)() as f32);
                    __cx.effect(move || {
                        let __nv = (#val)() as f32;
                        if (__s.get_untracked() - __nv).abs() > 1e-4 {
                            __s.set(__nv);
                        }
                    });
                    __s
                }}
            };
            let (min, max) = if let Some(range) = find_arg(&["range"]) {
                (
                    quote!(*(#range).start() as f32),
                    quote!(*(#range).end() as f32),
                )
            } else {
                let mn = find_arg(&["min"])
                    .map(|e| quote!(#e as f32))
                    .unwrap_or(quote!(0.0f32));
                let mx = find_arg(&["max"])
                    .map(|e| quote!(#e as f32))
                    .unwrap_or(quote!(100.0f32));
                (mn, mx)
            };
            let lbl = find_arg(&["label"])
                .map(|e| quote!(#e))
                .unwrap_or_else(|| quote!(""));
            let chg = find_arg(&["on_change"])
                .map(|cb| quote! { __tree.set_on_float_change(#var, #cb); });
            quote! {
                let #var = __tree.slider(__cx, #val_sig, #min, #max, #lbl);
                #chg
            }
        }
        "Checkbox" | "Switch" => {
            let builder = if kind_str == "Checkbox" {
                quote!(checkbox)
            } else {
                quote!(switch)
            };
            let chk_prop = find_prop(&["checked", "value"])
                .unwrap_or_else(|| panic!("{kind_str} requires `checked` Signal<bool>"));
            let chk = &chk_prop.value;
            let chk_sig = if is_two_way_signal_expr(chk_prop) {
                quote!(#chk)
            } else {
                quote! {{
                    let __s = __cx.signal((#chk)());
                    __cx.effect(move || {
                        let __nv = (#chk)();
                        if __s.get_untracked() != __nv {
                            __s.set(__nv);
                        }
                    });
                    __s
                }}
            };
            let lbl = find_arg(&["label", "text"])
                .map(|e| quote!(#e))
                .unwrap_or_else(|| quote!(""));
            let chg =
                find_arg(&["on_change"]).map(|cb| quote! { __tree.set_on_bool_change(#var, #cb); });
            quote! {
                let #var = __tree.#builder(__cx, #chk_sig, #lbl);
                #chg
            }
        }
        "TextInput" | "TextArea" | "input" => {
            let is_multiline = kind_str == "TextArea"
                || matches!(
                    find_arg(&["multiline"]),
                    Some(Expr::Lit(syn::ExprLit { lit: Lit::Bool(b), .. })) if b.value
                );
            let ph = find_arg(&["placeholder"])
                .map(|e| quote!(#e))
                .unwrap_or_else(|| quote!(""));
            let init = find_arg(&["initial", "text"])
                .map(|e| quote!(#e))
                .unwrap_or_else(|| quote!(""));
            let sig = find_prop(&["value"])
                .map(|p| {
                    let v = &p.value;
                    if is_two_way_signal_expr(p) {
                        quote!(Some(#v))
                    } else {
                        quote!(Some({
                            let __s = __cx.signal((#v)());
                            __cx.effect(move || {
                                let __nv = (#v)();
                                if __s.get_untracked() != __nv {
                                    __s.set(__nv);
                                }
                            });
                            __s
                        }))
                    }
                })
                .unwrap_or_else(|| quote!(None));
            let builder = if is_multiline {
                quote!(text_area)
            } else {
                quote!(text_input)
            };
            let style_stmt = if find_arg(&["size", "font_size", "weight", "font_weight"]).is_some()
            {
                let sz = find_arg(&["size", "font_size"])
                    .map(|e| quote!(#e as f32))
                    .unwrap_or_else(|| quote!(14.0f32));
                let wt = find_arg(&["weight", "font_weight"])
                    .map(|e| quote!(#e as f32))
                    .unwrap_or_else(|| quote!(400.0f32));
                Some(quote! {
                    __tree.set_text_input_style(#var, ::craftui::text::TextStyle::default().with_size(#sz).with_weight(#wt));
                })
            } else {
                None
            };
            let on_chg =
                find_arg(&["on_change"]).map(|cb| quote! { __tree.set_on_change(#var, #cb); });
            let on_sub =
                find_arg(&["on_submit"]).map(|cb| quote! { __tree.set_on_submit(#var, #cb); });
            quote! {
                let #var = __tree.#builder(__cx, #init, #ph, #sig);
                #style_stmt
                #on_chg
                #on_sub
            }
        }
        "Tabs" | "TabBar" => {
            let tabs = find_arg(&["tabs", "items", "options"])
                .map(|e| quote!(#e))
                .unwrap_or_else(|| quote!(Vec::<String>::new()));
            let active_prop = find_prop(&["active", "selected", "value"])
                .expect("Tabs requires `active` Signal<usize>");
            let active = &active_prop.value;
            let active_sig = if is_two_way_signal_expr(active_prop) {
                quote!(#active)
            } else {
                quote! {{
                    let __s = __cx.signal((#active)() as usize);
                    __cx.effect(move || {
                        let __nv = (#active)() as usize;
                        if __s.get_untracked() != __nv { __s.set(__nv); }
                    });
                    __s
                }}
            };
            let chg = find_arg(&["on_change", "on_select"])
                .map(|cb| quote! { __tree.set_on_index_change(#var, #cb); });
            quote! {
                let #var = __tree.tabs(__cx, #tabs, #active_sig);
                #chg
            }
        }
        "SegmentedControl" => {
            let items = find_arg(&["items", "options", "tabs"])
                .map(|e| {
                    if matches!(e, Expr::Closure(_)) {
                        quote!(::craftui::widgets::ReactiveValue::Dynamic(::std::rc::Rc::new(#e)))
                    } else {
                        quote!(#e)
                    }
                })
                .unwrap_or_else(|| quote!(Vec::<String>::new()));
            let sel_sig = match find_prop(&["selected", "active", "value"]) {
                Some(sel_prop) => {
                    let sel = &sel_prop.value;
                    if is_two_way_signal_expr(sel_prop) {
                        quote!(#sel)
                    } else {
                        quote! {{
                            let __s = __cx.signal((#sel)() as usize);
                            __cx.effect(move || {
                                let __nv = (#sel)() as usize;
                                if __s.get_untracked() != __nv { __s.set(__nv); }
                            });
                            __s
                        }}
                    }
                }
                None => quote!(__cx.signal(0usize)),
            };
            let chg = find_arg(&["on_change", "on_select"])
                .map(|cb| quote! { __tree.set_on_index_change(#var, #cb); });
            quote! {
                let #var = __tree.segmented_control(__cx, #items, #sel_sig);
                #chg
            }
        }
        "BottomNavBar" => {
            let items = find_arg(&["items", "options", "tabs"])
                .map(|e| {
                    if matches!(e, Expr::Closure(_)) {
                        quote!(::craftui::widgets::ReactiveValue::Dynamic(::std::rc::Rc::new(#e)))
                    } else {
                        quote!(#e)
                    }
                })
                .unwrap_or_else(|| quote!(Vec::<String>::new()));
            let icons = find_arg(&["icons"])
                .map(|e| {
                    if matches!(e, Expr::Closure(_)) {
                        quote!(::craftui::widgets::ReactiveValue::Dynamic(::std::rc::Rc::new(#e)))
                    } else {
                        quote!(#e)
                    }
                })
                .unwrap_or_else(|| quote!(Vec::<String>::new()));
            let sel_sig = match find_prop(&["selected", "active", "value"]) {
                Some(sel_prop) => {
                    let sel = &sel_prop.value;
                    if is_two_way_signal_expr(sel_prop) {
                        quote!(#sel)
                    } else {
                        quote! {{
                            let __s = __cx.signal((#sel)() as usize);
                            __cx.effect(move || {
                                let __nv = (#sel)() as usize;
                                if __s.get_untracked() != __nv { __s.set(__nv); }
                            });
                            __s
                        }}
                    }
                }
                None => quote!(__cx.signal(0usize)),
            };
            let chg = find_arg(&["on_change", "on_select"])
                .map(|cb| quote! { __tree.set_on_index_change(#var, #cb); });
            quote! {
                let #var = __tree.bottom_nav_bar(__cx, #items, #icons, #sel_sig);
                #chg
            }
        }
        "FloatingActionButton" => {
            let icon = find_arg(&["icon"])
                .map(|e| {
                    if matches!(e, Expr::Closure(_)) {
                        quote!(::craftui::widgets::ReactiveValue::Dynamic(::std::rc::Rc::new(#e)))
                    } else {
                        quote!(#e)
                    }
                })
                .unwrap_or_else(|| quote!(""));
            let label_opt = find_arg(&["label", "text"]).map(|e| {
                if matches!(e, Expr::Closure(_)) {
                    quote!(Some(::craftui::widgets::ReactiveValue::Dynamic(::std::rc::Rc::new(#e))))
                } else {
                    quote!(Some(#e))
                }
            }).unwrap_or_else(|| quote!(None::<String>));
            let tap = find_arg(&["on_tap", "on_click"])
                .map(|cb| quote! { __tree.set_on_tap(#var, #cb); });
            quote! {
                let #var = __tree.floating_action_button(__cx, #icon, #label_opt);
                #tap
            }
        }
        "Accordion" => {
            let title = find_arg(&["title", "label", "text"])
                .map(|e| {
                    if matches!(e, Expr::Closure(_)) {
                        quote!(::craftui::widgets::ReactiveValue::Dynamic(::std::rc::Rc::new(#e)))
                    } else {
                        quote!(#e)
                    }
                })
                .unwrap_or_else(|| quote!(""));
            let exp_sig = match find_prop(&["expanded", "open"]) {
                Some(exp_prop) => {
                    let exp = &exp_prop.value;
                    if is_two_way_signal_expr(exp_prop) {
                        quote!(#exp)
                    } else {
                        quote! {{
                            let __s = __cx.signal((#exp)());
                            __cx.effect(move || {
                                let __nv = (#exp)();
                                if __s.get_untracked() != __nv { __s.set(__nv); }
                            });
                            __s
                        }}
                    }
                }
                None => quote!(__cx.signal(false)),
            };
            let chg = find_arg(&["on_change", "on_toggle"])
                .map(|cb| quote! { __tree.set_on_bool_change(#var, #cb); });
            quote! {
                let #var = __tree.accordion(__cx, #title, #exp_sig);
                #chg
            }
        }
        "DataTable" => {
            let cols = find_arg(&["columns"])
                .map(|e| {
                    if matches!(e, Expr::Closure(_)) {
                        quote!(::craftui::widgets::ReactiveValue::Dynamic(::std::rc::Rc::new(#e)))
                    } else {
                        quote!(#e)
                    }
                })
                .unwrap_or_else(|| quote!(Vec::<String>::new()));
            let rows = find_arg(&["rows", "items"])
                .map(|e| {
                    if matches!(e, Expr::Closure(_)) {
                        quote!(::craftui::widgets::ReactiveValue::Dynamic(::std::rc::Rc::new(#e)))
                    } else {
                        quote!(#e)
                    }
                })
                .unwrap_or_else(|| quote!(Vec::<Vec<String>>::new()));
            let sel_sig = match find_prop(&["selected"]) {
                Some(sel_prop) => {
                    let sel = &sel_prop.value;
                    if is_two_way_signal_expr(sel_prop) {
                        quote!(#sel)
                    } else {
                        quote! {{
                            let __s = __cx.signal((#sel)() as usize);
                            __cx.effect(move || {
                                let __nv = (#sel)() as usize;
                                if __s.get_untracked() != __nv { __s.set(__nv); }
                            });
                            __s
                        }}
                    }
                }
                None => quote!(__cx.signal(0usize)),
            };
            let striped_arg = find_arg(&["striped"]);
            let rh_arg = find_arg(&["row_height"]);
            let opts_stmt = if striped_arg.is_some() || rh_arg.is_some() {
                let st_opt = striped_arg
                    .map(|e| quote!(Some(#e)))
                    .unwrap_or_else(|| quote!(None));
                let rh_opt = rh_arg
                    .map(|e| quote!(Some(#e as f32)))
                    .unwrap_or_else(|| quote!(None));
                quote! { __tree.set_data_table_options(#var, #st_opt, #rh_opt); }
            } else {
                TokenStream2::new()
            };
            let on_sel = find_arg(&["on_select", "on_change"])
                .map(|cb| quote! { __tree.set_on_index_change(#var, #cb); });
            let on_sort =
                find_arg(&["on_sort"]).map(|cb| quote! { __tree.set_on_sort(#var, #cb); });
            quote! {
                let #var = __tree.data_table(__cx, #cols, #rows, #sel_sig);
                #opts_stmt
                #on_sel
                #on_sort
            }
        }
        "Select" | "Dropdown" => {
            let opts = find_arg(&["options", "items"])
                .map(|e| quote!(#e))
                .unwrap_or_else(|| quote!(Vec::<String>::new()));
            let sel_prop = find_prop(&["selected", "value"])
                .expect("Select requires `selected` Signal<usize>");
            let sel = &sel_prop.value;
            let sel_sig = if is_two_way_signal_expr(sel_prop) {
                quote!(#sel)
            } else {
                quote! {{
                    let __s = __cx.signal((#sel)() as usize);
                    __cx.effect(move || {
                        let __nv = (#sel)() as usize;
                        if __s.get_untracked() != __nv { __s.set(__nv); }
                    });
                    __s
                }}
            };
            let ph = find_arg(&["placeholder", "label"])
                .map(|e| quote!(#e))
                .unwrap_or_else(|| quote!("Select..."));
            let chg = find_arg(&["on_change", "on_select"])
                .map(|cb| quote! { __tree.set_on_index_change(#var, #cb); });
            quote! {
                let #var = __tree.select(__cx, #opts, #sel_sig, #ph);
                #chg
            }
        }
        "RadioGroup" => {
            let opts = find_arg(&["options", "items"])
                .map(|e| quote!(#e))
                .unwrap_or_else(|| quote!(Vec::<String>::new()));
            let sel_prop = find_prop(&["selected", "value"])
                .expect("RadioGroup requires `selected` Signal<usize>");
            let sel = &sel_prop.value;
            let sel_sig = if is_two_way_signal_expr(sel_prop) {
                quote!(#sel)
            } else {
                quote! {{
                    let __s = __cx.signal((#sel)() as usize);
                    __cx.effect(move || {
                        let __nv = (#sel)() as usize;
                        if __s.get_untracked() != __nv { __s.set(__nv); }
                    });
                    __s
                }}
            };
            let axis = find_arg(&["axis", "direction"])
                .map(|e| quote!(#e))
                .unwrap_or_else(|| quote!(::craftui::core::Axis::Vertical));
            let chg = find_arg(&["on_change", "on_select"])
                .map(|cb| quote! { __tree.set_on_index_change(#var, #cb); });
            quote! {
                let #var = __tree.radio_group(__cx, #opts, #sel_sig, #axis);
                #chg
            }
        }
        "PopupMenu" => {
            let open = find_arg(&["open", "visible"])
                .map(|e| {
                    if matches!(e, Expr::Closure(_)) {
                        quote!(::craftui::widgets::ReactiveValue::Dynamic(::std::rc::Rc::new(#e)))
                    } else {
                        quote!(#e)
                    }
                })
                .unwrap_or_else(|| quote!(false));
            let items = find_arg(&["items", "options"])
                .map(|e| quote!(#e))
                .unwrap_or_else(|| quote!(Vec::<String>::new()));
            let anchor = find_arg(&["anchor"])
                .map(|e| quote!(#e))
                .unwrap_or_else(|| quote!(None));
            let on_sel = find_arg(&["on_select", "on_change"])
                .map(|cb| {
                    quote!(Some(::std::rc::Rc::new(::std::cell::RefCell::new(#cb)) as ::craftui::widgets::IndexCallback))
                })
                .unwrap_or_else(|| quote!(None));
            let on_cls = find_arg(&["on_close"])
                .map(|cb| {
                    quote!(Some(::std::rc::Rc::new(::std::cell::RefCell::new(#cb)) as ::craftui::widgets::TapHandler))
                })
                .unwrap_or_else(|| quote!(None));
            quote! { let #var = __tree.popup_menu(__cx, #open, #items, #anchor, #on_sel, #on_cls); }
        }
        "Tooltip" => {
            let text = find_arg(&["text", "label", "content"])
                .map(|e| quote!(#e))
                .unwrap_or_else(|| quote!(""));
            quote! { let #var = __tree.tooltip(__cx, #text); }
        }
        "ProgressIndicator" => {
            let prog = find_arg(&["progress", "value"])
                .map(|e| quote!(#e))
                .unwrap_or_else(|| quote!(None::<f32>));
            quote! { let #var = __tree.progress_indicator(__cx, #prog); }
        }
        "Spinner" => {
            let sz_arg = find_arg(&["size"]);
            let th_arg = find_arg(&["thickness"]);
            let col_arg = find_arg(&["color", "fg", "foreground"]);
            let init_sz = match sz_arg {
                Some(e) if !matches!(e, Expr::Closure(_)) => quote!(#e as f32),
                _ => quote!(24.0f32),
            };
            let has_static_th = th_arg.is_some_and(|e| !matches!(e, Expr::Closure(_)));
            let th_opt = match th_arg {
                Some(e) if !matches!(e, Expr::Closure(_)) => quote!(Some(#e as f32)),
                _ => quote!(None),
            };
            let col_opt = match col_arg {
                Some(c) => {
                    let rv = reactive_color_tokens(c);
                    quote!(Some(#rv))
                }
                None => quote!(None),
            };
            let static_style = if has_static_th || col_arg.is_some() {
                Some(quote! { __tree.set_spinner_style(#var, None, #th_opt, #col_opt); })
            } else {
                None
            };
            let sz_closure = sz_arg.filter(|e| matches!(e, Expr::Closure(_)));
            let th_closure = th_arg.filter(|e| matches!(e, Expr::Closure(_)));
            let dyn_style = if sz_closure.is_some() || th_closure.is_some() {
                let mut rc_decls = TokenStream2::new();
                let mut tr_reads = TokenStream2::new();
                let sz_val = if let Some(se) = sz_closure {
                    rc_decls.extend(quote! {
                        let __sp_sz = ::std::rc::Rc::new(#se);
                        let __sp_sz_tr = __sp_sz.clone();
                    });
                    tr_reads.extend(quote! { let _ = __sp_sz_tr(); });
                    quote!(Some(__sp_sz() as f32))
                } else {
                    quote!(None)
                };
                let th_val = if let Some(te) = th_closure {
                    rc_decls.extend(quote! {
                        let __sp_th = ::std::rc::Rc::new(#te);
                        let __sp_th_tr = __sp_th.clone();
                    });
                    tr_reads.extend(quote! { let _ = __sp_th_tr(); });
                    quote!(Some(__sp_th() as f32))
                } else {
                    quote!(None)
                };
                Some(quote! {
                    {
                        #rc_decls
                        __tree.bind_dynamic_prop(
                            #var,
                            ::craftui::core::DirtyFlags::LAYOUT | ::craftui::core::DirtyFlags::PAINT,
                            move || { #tr_reads },
                            move |__id, __t| { __t.set_spinner_style(__id, #sz_val, #th_val, None); },
                        );
                    }
                })
            } else {
                None
            };
            quote! {
                let #var = __tree.spinner(__cx, #init_sz);
                #static_style
                #dyn_style
            }
        }
        "Badge" => {
            let lbl = find_arg(&["label", "text"])
                .map(|e| quote!(#e))
                .unwrap_or_else(|| quote!(""));
            let fg_opt = find_arg(&["color", "fg", "foreground"]).map(|c| {
                let rv = reactive_color_tokens(c);
                quote!(Some(#rv))
            });
            let bg_opt = find_arg(&["bg", "background"]).map(|c| {
                let rv = reactive_color_tokens(c);
                quote!(Some(#rv))
            });
            let color_stmt = if fg_opt.is_some() || bg_opt.is_some() {
                let f = fg_opt.unwrap_or_else(|| quote!(None));
                let b = bg_opt.unwrap_or_else(|| quote!(None));
                quote! { __tree.set_badge_colors(#var, #f, #b); }
            } else {
                TokenStream2::new()
            };
            let icon_stmt = find_arg(&["icon"]).map(|ic| {
                if matches!(ic, Expr::Closure(_)) {
                    quote! {
                        __tree.set_badge_icon(
                            #var,
                            Some(::craftui::widgets::ReactiveValue::Dynamic(::std::rc::Rc::new(
                                move || ::core::convert::Into::<String>::into((#ic)())
                            ))),
                        );
                    }
                } else {
                    quote! {
                        __tree.set_badge_icon(#var, Some(::craftui::widgets::ReactiveValue::from(#ic)));
                    }
                }
            });
            quote! {
                let #var = __tree.badge(__cx, #lbl);
                #color_stmt
                #icon_stmt
            }
        }
        "SplitPane" => {
            let axis = find_arg(&["axis", "direction"])
                .map(|e| quote!(#e))
                .unwrap_or_else(|| quote!(::craftui::core::Axis::Horizontal));
            let ratio_sig = match find_prop(&["ratio"]) {
                Some(p) => {
                    let v = &p.value;
                    if is_two_way_signal_expr(p) {
                        quote!(#v)
                    } else {
                        quote! {{
                            let __s = __cx.signal((#v)() as f32);
                            __cx.effect(move || {
                                let __nv = (#v)() as f32;
                                if (__s.get_untracked() - __nv).abs() > 1e-4 { __s.set(__nv); }
                            });
                            __s
                        }}
                    }
                }
                None => quote!(__cx.signal(0.5f32)),
            };
            let mf = find_arg(&["min_first"]);
            let ms = find_arg(&["min_second"]);
            let ds = find_arg(&["divider_size"]);
            let cfg_stmt = if mf.is_some() || ms.is_some() || ds.is_some() {
                let mf_c = mf
                    .map(|e| quote!(#e as f32))
                    .unwrap_or_else(|| quote!(80.0f32));
                let ms_c = ms
                    .map(|e| quote!(#e as f32))
                    .unwrap_or_else(|| quote!(80.0f32));
                let ds_c = ds
                    .map(|e| quote!(#e as f32))
                    .unwrap_or_else(|| quote!(8.0f32));
                quote! { __tree.set_split_pane_config(#var, #mf_c, #ms_c, #ds_c); }
            } else {
                TokenStream2::new()
            };
            let chg = find_arg(&["on_change"])
                .map(|cb| quote! { __tree.set_on_float_change(#var, #cb); });
            quote! {
                let #var = __tree.split_pane(__cx, #axis, #ratio_sig);
                #cfg_stmt
                #chg
            }
        }
        "ToastHost" => {
            if let Some(mgr) = find_arg(&["manager"]) {
                let dis = find_arg(&["on_dismiss"])
                    .map(|cb| quote! { __tree.set_on_dismiss(#var, #cb); });
                quote! {
                    let #var = __tree.toast_host(__cx, #mgr);
                    #dis
                }
            } else {
                let items = find_arg(&["items"])
                    .map(|e| {
                        if matches!(e, Expr::Closure(_)) {
                            quote!(::craftui::widgets::ReactiveValue::Dynamic(::std::rc::Rc::new(#e)))
                        } else {
                            quote!(#e)
                        }
                    })
                    .unwrap_or_else(|| quote!(Vec::<::craftui::widgets::ToastItem>::new()));
                let on_dis = find_arg(&["on_dismiss"])
                    .map(|cb| {
                        quote!(Some(::std::rc::Rc::new(::std::cell::RefCell::new(#cb)) as ::craftui::widgets::ToastDismissCallback))
                    })
                    .unwrap_or_else(|| quote!(None));
                quote! {
                    let #var = __tree.toast_host_custom(__cx, #items, #on_dis);
                }
            }
        }
        "Modal" | "BottomSheet" => {
            let is_sheet = kind_str == "BottomSheet";
            let open = find_arg(&["open", "visible"])
                .map(|e| {
                    if matches!(e, Expr::Closure(_)) {
                        quote!(::craftui::widgets::ReactiveValue::Dynamic(::std::rc::Rc::new(#e)))
                    } else {
                        quote!(#e)
                    }
                })
                .unwrap_or_else(|| quote!(false));
            let on_cls = find_arg(&["on_close"])
                .map(|cb| {
                    quote!(Some(::std::rc::Rc::new(::std::cell::RefCell::new(#cb)) as ::craftui::widgets::TapHandler))
                })
                .unwrap_or_else(|| quote!(None));
            quote! { let #var = __tree.overlay(__cx, #open, #is_sheet, #on_cls); }
        }
        "PlatformView" => {
            let view_id = find_arg(&["view_id"])
                .or_else(|| {
                    find_arg(&["id"]).filter(|e| {
                        !matches!(
                            e,
                            Expr::Lit(syn::ExprLit {
                                lit: Lit::Str(_),
                                ..
                            })
                        )
                    })
                })
                .map(|e| quote!(#e))
                .unwrap_or_else(|| quote!(::craftui::core::PlatformViewId::default()));
            quote! {
                let #var = __tree.platform_view(__cx, #view_id, ::craftui::layout::LayoutStyle::column());
            }
        }
        "HtmlView" => {
            let src = find_arg(&["src", "url"])
                .map(|e| quote!(#e))
                .unwrap_or_else(|| quote!(""));
            let html = find_arg(&["html"])
                .map(|e| quote!(#e))
                .unwrap_or_else(|| quote!(""));
            quote! {
                let #var = __tree.html_view(__cx, #src, #html, None);
            }
        }
        "Card" => {
            let variant = find_arg(&["variant"])
                .map(|e| quote!(#e))
                .unwrap_or_else(|| quote!(::craftui::widgets::CardVariant::Elevated));
            quote! {
                let #var = __tree.card(__cx, #variant);
            }
        }
        "ListTile" => {
            let title = find_arg(&["title"])
                .map(|e| quote!(#e))
                .unwrap_or_else(|| quote!(""));
            let subtitle = find_arg(&["subtitle"])
                .map(|e| quote!(Some(#e)))
                .unwrap_or_else(|| quote!(None::<&str>));
            let leading = find_arg(&["leading"])
                .map(|e| quote!(Some(#e)))
                .unwrap_or_else(|| quote!(None::<&str>));
            let trailing = find_arg(&["trailing"])
                .map(|e| quote!(Some(#e)))
                .unwrap_or_else(|| quote!(None::<&str>));
            let on_tap = find_arg(&["on_tap", "on_click"])
                .map(|e| quote!(Some(#e)))
                .unwrap_or_else(|| quote!(None::<fn()>));
            quote! {
                let #var = __tree.list_tile(__cx, #title, #subtitle, #leading, #trailing, #on_tap);
            }
        }
        "Chip" => {
            let label = find_arg(&["label"])
                .map(|e| quote!(#e))
                .unwrap_or_else(|| quote!(""));
            let selected = find_arg(&["selected"])
                .map(|e| quote!(#e))
                .unwrap_or_else(|| quote!(__cx.signal(false)));
            let icon = find_arg(&["icon"])
                .map(|e| quote!(Some(#e)))
                .unwrap_or_else(|| quote!(None::<&str>));
            let on_tap = find_arg(&["on_tap", "on_click"])
                .map(|e| quote!(Some(#e)))
                .unwrap_or_else(|| quote!(None::<fn()>));
            quote! {
                let #var = __tree.chip(__cx, #label, #selected, #icon, #on_tap);
            }
        }
        "AppBar" => {
            let title = find_arg(&["title"])
                .map(|e| quote!(#e))
                .unwrap_or_else(|| quote!(""));
            let leading = find_arg(&["leading", "leading_icon"])
                .map(|e| quote!(Some(#e)))
                .unwrap_or_else(|| quote!(None::<&str>));
            let elevation = find_arg(&["elevation"])
                .map(|e| quote!(#e))
                .unwrap_or_else(|| quote!(4.0f32));
            let on_leading_tap = find_arg(&["on_leading_tap", "on_menu_tap"])
                .map(|e| quote!(Some(#e)))
                .unwrap_or_else(|| quote!(None::<fn()>));
            quote! {
                let #var = __tree.app_bar(__cx, #title, #leading, #elevation, #on_leading_tap);
            }
        }
        "Drawer" => {
            let open = find_arg(&["open"])
                .map(|e| quote!(#e))
                .unwrap_or_else(|| quote!(__cx.signal(false)));
            let width = find_arg(&["width"])
                .map(|e| quote!(#e))
                .unwrap_or_else(|| quote!(304.0f32));
            let on_close = find_arg(&["on_close"])
                .map(|e| quote!(Some(#e)))
                .unwrap_or_else(|| quote!(None::<fn()>));
            quote! {
                let #var = __tree.drawer(__cx, #open, #width, #on_close);
            }
        }
        "Custom" => {
            let widget = find_arg(&["widget", "impl", "custom"])
                .expect("Custom requires `widget: MyCustomWidget::new(...)`");
            quote! {
                let #var = __tree.custom_widget(__cx, #widget, ::craftui::layout::LayoutStyle::column());
            }
        }
        "ScrollView" => {
            let axis = find_arg(&["axis", "direction"])
                .map(|e| quote!(#e))
                .unwrap_or_else(|| quote!(::craftui::core::Axis::Vertical));
            quote! {
                let #var = __tree.scroll_view(__cx, #axis, ::craftui::layout::LayoutStyle::column());
            }
        }
        "VirtualList" | "VirtualGrid" => {
            let axis = find_arg(&["axis", "direction"])
                .map(|e| quote!(#e))
                .unwrap_or_else(|| quote!(::craftui::core::Axis::Vertical));
            let default_cols: u16 = if kind_str == "VirtualGrid" { 2 } else { 1 };
            let cols = find_arg(&["columns", "cols"])
                .map(|e| quote!(#e as usize))
                .unwrap_or_else(|| quote!(#default_cols as usize));
            let extent = find_arg(&["item_extent", "item_height"])
                .map(|e| quote!(#e as f32))
                .unwrap_or_else(|| quote!(48.0f32));
            let count = find_arg(&["item_count", "count"])
                .map(|e| quote!(#e))
                .unwrap_or_else(|| quote!(0usize));
            let factory = find_arg(&["factory", "render_item"])
                .map(|e| quote!(::std::rc::Rc::new(#e)))
                .unwrap_or_else(|| {
                    quote!(::std::rc::Rc::new(|vcx, vidx, vtree| vtree.text(
                        vcx,
                        format!("Item {vidx}"),
                        ::craftui::text::TextStyle::default()
                    )))
                });
            quote! {
                let #var = __tree.virtual_list(__cx, #axis, #cols, #extent, #count, ::craftui::layout::LayoutStyle::column(), #factory);
            }
        }
        _ => quote! { let #var = __tree.column(__cx); },
    };

    stmts.extend(init_stmt);

    // Register node `id` if provided (skip numeric/PlatformViewId `id` on PlatformView)
    if let Some(id_expr) = find_arg(&["id"])
        && (kind_str != "PlatformView"
            || matches!(
                id_expr,
                Expr::Lit(syn::ExprLit {
                    lit: Lit::Str(_),
                    ..
                })
            ))
    {
        stmts.extend(quote! { __tree.register_node_id(#var, #id_expr); });
    }

    // Apply layout and visual style properties (both static & reactive closure-backed)
    let mut layout_static = TokenStream2::new();
    let mut layout_dyn_rc = TokenStream2::new();
    let mut layout_dyn_track = TokenStream2::new();
    let mut layout_dyn_apply = TokenStream2::new();

    let mut box_static = TokenStream2::new();
    let mut box_dyn_rc = TokenStream2::new();
    let mut box_dyn_track = TokenStream2::new();
    let mut box_dyn_apply = TokenStream2::new();

    let mut has_border_color = false;
    let mut has_border_width = false;

    for (idx, arg) in el.args.iter().enumerate() {
        let name = arg.name.to_string();
        let val = &arg.value;
        let is_closure = matches!(val, Expr::Closure(_));

        macro_rules! push_layout_prop {
            ($assign_static:expr, $assign_dyn:expr) => {
                if is_closure {
                    let rc_id = format_ident!("__cui_lfn_{idx}");
                    let rc_tr = format_ident!("__cui_lfn_tr_{idx}");
                    layout_dyn_rc.extend(quote! {
                        let #rc_id = ::std::rc::Rc::new(#val);
                        let #rc_tr = #rc_id.clone();
                    });
                    layout_dyn_track.extend(quote! { let _ = #rc_tr(); });
                    layout_dyn_apply.extend($assign_dyn(&rc_id));
                } else {
                    layout_static.extend($assign_static);
                }
            };
        }

        match name.as_str() {
            "class" => {
                layout_static.extend(quote! {
                    let __tw = ::craftui::core::TailwindTokens::parse(&#val.to_string());
                    if __tw.is_row { __ls.mode = ::craftui::layout::LayoutMode::Row; }
                    else if __tw.is_column { __ls.mode = ::craftui::layout::LayoutMode::Column; }
                    if let Some(__p) = __tw.padding { __ls.padding = __p; }
                    if let Some(__g) = __tw.gap { __ls.gap_x = __g; __ls.gap_y = __g; }
                    if let Some(__w) = __tw.width { __ls.width = __w; }
                    if let Some(__h) = __tw.height { __ls.height = __h; }
                });
                box_static.extend(quote! {
                    let __tw = ::craftui::core::TailwindTokens::parse(&#val.to_string());
                    if let Some(__bg) = __tw.background {
                        if __bs.background.is_none() {
                            __bs.background = Some(::craftui::widgets::ReactiveValue::Static(
                                ::craftui::core::Brush::Solid(__bg)
                            ));
                        }
                    }
                    if let Some(__r) = __tw.radius {
                        if __bs.radius == ::craftui::core::CornerRadii::ZERO {
                            __bs.radius = __r;
                        }
                    }
                    if let Some(__sh) = __tw.shadow {
                        if __bs.shadow.is_none() {
                            __bs.shadow = Some(__sh);
                        }
                    }
                });
            }
            "width" => {
                push_layout_prop!(
                    quote! { __ls.width = #val; },
                    |id: &Ident| quote! { __ls.width = #id(); }
                );
            }
            "height" => {
                push_layout_prop!(
                    quote! { __ls.height = #val; },
                    |id: &Ident| quote! { __ls.height = #id(); }
                );
            }
            "min_width" => {
                push_layout_prop!(
                    quote! { __ls.min_width = #val; },
                    |id: &Ident| quote! { __ls.min_width = #id(); }
                );
            }
            "max_width" => {
                push_layout_prop!(
                    quote! { __ls.max_width = #val; },
                    |id: &Ident| quote! { __ls.max_width = #id(); }
                );
            }
            "min_height" => {
                push_layout_prop!(
                    quote! { __ls.min_height = #val; },
                    |id: &Ident| quote! { __ls.min_height = #id(); }
                );
            }
            "max_height" => {
                push_layout_prop!(
                    quote! { __ls.max_height = #val; },
                    |id: &Ident| quote! { __ls.max_height = #id(); }
                );
            }
            "flex_grow" => {
                push_layout_prop!(
                    quote! { __ls.flex_grow = #val as f32; },
                    |id: &Ident| quote! { __ls.flex_grow = #id() as f32; }
                );
            }
            "flex_shrink" => {
                push_layout_prop!(
                    quote! { __ls.flex_shrink = #val as f32; },
                    |id: &Ident| quote! { __ls.flex_shrink = #id() as f32; }
                );
            }
            "gap" => {
                push_layout_prop!(
                    quote! { __ls.gap_x = #val as f32; __ls.gap_y = #val as f32; },
                    |id: &Ident| quote! { let __gv = #id() as f32; __ls.gap_x = __gv; __ls.gap_y = __gv; }
                );
            }
            "padding" => {
                push_layout_prop!(
                    quote! { __ls.padding = ::craftui::core::Insets::all(#val as f32); },
                    |id: &Ident| quote! { __ls.padding = ::craftui::core::Insets::all(#id() as f32); }
                );
            }
            "padding_x" => {
                push_layout_prop!(
                    quote! { __ls.padding.left = #val as f32; __ls.padding.right = #val as f32; },
                    |id: &Ident| quote! { let __pv = #id() as f32; __ls.padding.left = __pv; __ls.padding.right = __pv; }
                );
            }
            "padding_y" => {
                push_layout_prop!(
                    quote! { __ls.padding.top = #val as f32; __ls.padding.bottom = #val as f32; },
                    |id: &Ident| quote! { let __pv = #id() as f32; __ls.padding.top = __pv; __ls.padding.bottom = __pv; }
                );
            }
            "margin" => {
                push_layout_prop!(
                    quote! { __ls.margin = ::craftui::core::Insets::all(#val as f32); },
                    |id: &Ident| quote! { __ls.margin = ::craftui::core::Insets::all(#id() as f32); }
                );
            }
            "align" => {
                push_layout_prop!(
                    quote! { __ls.align_items = #val; },
                    |id: &Ident| quote! { __ls.align_items = #id(); }
                );
            }
            "justify" => {
                push_layout_prop!(
                    quote! { __ls.justify_content = #val; },
                    |id: &Ident| quote! { __ls.justify_content = #id(); }
                );
            }
            "safe_area" => {
                push_layout_prop!(
                    quote! { __ls.safe_area = #val; },
                    |id: &Ident| quote! { __ls.safe_area = #id(); }
                );
            }
            "bg" | "background" if kind_str != "Badge" => {
                let rv = reactive_brush_tokens(val);
                box_static.extend(quote! { __bs.background = Some(#rv); });
            }
            "radius" | "border_radius" => {
                if is_closure {
                    let rc_id = format_ident!("__cui_bfn_{idx}");
                    let rc_tr = format_ident!("__cui_bfn_tr_{idx}");
                    box_dyn_rc.extend(quote! {
                        let #rc_id = ::std::rc::Rc::new(#val);
                        let #rc_tr = #rc_id.clone();
                    });
                    box_dyn_track.extend(quote! { let _ = #rc_tr(); });
                    box_dyn_apply.extend(quote! {
                        __bs.radius = ::craftui::core::CornerRadii::all(#rc_id() as f32);
                        if let Some(ref mut __br) = __bs.border { __br.radius = __bs.radius; }
                    });
                } else {
                    box_static.extend(quote! {
                        __bs.radius = ::craftui::core::CornerRadii::all(#val as f32);
                        if let Some(ref mut __br) = __bs.border { __br.radius = __bs.radius; }
                    });
                }
            }
            "border_width" => {
                has_border_width = true;
                if is_closure {
                    let rc_id = format_ident!("__cui_bfn_{idx}");
                    let rc_tr = format_ident!("__cui_bfn_tr_{idx}");
                    box_dyn_rc.extend(quote! {
                        let #rc_id = ::std::rc::Rc::new(#val);
                        let #rc_tr = #rc_id.clone();
                    });
                    box_dyn_track.extend(quote! { let _ = #rc_tr(); });
                    box_dyn_apply.extend(quote! {
                        let __bw = #rc_id() as f32;
                        let __cur = __bs.border.unwrap_or_else(|| ::craftui::core::Border {
                            width: __bw,
                            color: __t.theme().border,
                            radius: __bs.radius,
                        });
                        __bs.border = Some(::craftui::core::Border {
                            width: __bw,
                            color: __cur.color,
                            radius: __bs.radius,
                        });
                    });
                } else {
                    box_static.extend(quote! {
                        let __bw = #val as f32;
                        let __cur = __bs.border.unwrap_or_else(|| ::craftui::core::Border {
                            width: __bw,
                            color: __tree.theme().border,
                            radius: __bs.radius,
                        });
                        __bs.border = Some(::craftui::core::Border {
                            width: __bw,
                            color: __cur.color,
                            radius: __bs.radius,
                        });
                    });
                }
            }
            "border_color" => {
                has_border_color = true;
                let rv = reactive_color_tokens(val);
                box_static.extend(quote! {
                    __bs.border_color = Some(#rv);
                    if __bs.border.is_none() {
                        __bs.border = Some(::craftui::core::Border {
                            width: 1.0,
                            color: ::craftui::core::Color::TRANSPARENT,
                            radius: __bs.radius,
                        });
                    }
                });
            }
            "clip" => {
                if is_closure {
                    let rc_id = format_ident!("__cui_bfn_{idx}");
                    let rc_tr = format_ident!("__cui_bfn_tr_{idx}");
                    box_dyn_rc.extend(quote! {
                        let #rc_id = ::std::rc::Rc::new(#val);
                        let #rc_tr = #rc_id.clone();
                    });
                    box_dyn_track.extend(quote! { let _ = #rc_tr(); });
                    box_dyn_apply.extend(quote! { __bs.clip = #rc_id(); });

                    let lrc_id = format_ident!("__cui_lclip_{idx}");
                    let lrc_tr = format_ident!("__cui_lclip_tr_{idx}");
                    layout_dyn_rc.extend(quote! {
                        let #lrc_id = #rc_id.clone();
                        let #lrc_tr = #lrc_id.clone();
                    });
                    layout_dyn_track.extend(quote! { let _ = #lrc_tr(); });
                    layout_dyn_apply.extend(quote! { __ls.clip_overflow = #lrc_id(); });
                } else {
                    box_static.extend(quote! { __bs.clip = #val; });
                    layout_static.extend(quote! { __ls.clip_overflow = #val; });
                }
            }
            "shadow" => {
                if let Expr::Lit(syn::ExprLit {
                    lit: Lit::Bool(b), ..
                }) = val
                {
                    if b.value {
                        box_static.extend(quote! {
                            __bs.shadow = Some(::craftui::core::BoxShadow::new(
                                0.0,
                                4.0,
                                12.0,
                                ::craftui::core::Color::rgba(0, 0, 0, 80),
                            ));
                        });
                    } else {
                        box_static.extend(quote! { __bs.shadow = None; });
                    }
                } else if is_closure {
                    let rc_id = format_ident!("__cui_bfn_{idx}");
                    let rc_tr = format_ident!("__cui_bfn_tr_{idx}");
                    box_dyn_rc.extend(quote! {
                        let #rc_id = ::std::rc::Rc::new(#val);
                        let #rc_tr = #rc_id.clone();
                    });
                    box_dyn_track.extend(quote! { let _ = #rc_tr(); });
                    box_dyn_apply.extend(quote! { __bs.shadow = Some(#rc_id()); });
                } else {
                    box_static.extend(quote! { __bs.shadow = Some(#val); });
                }
            }
            "shadows" => {
                if is_closure {
                    let rc_id = format_ident!("__cui_bfn_{idx}");
                    let rc_tr = format_ident!("__cui_bfn_tr_{idx}");
                    box_dyn_rc.extend(quote! {
                        let #rc_id = ::std::rc::Rc::new(#val);
                        let #rc_tr = #rc_id.clone();
                    });
                    box_dyn_track.extend(quote! { let _ = #rc_tr(); });
                    box_dyn_apply.extend(quote! { __bs.shadows = #rc_id(); });
                } else {
                    box_static.extend(quote! { __bs.shadows = #val; });
                }
            }
            _ => {}
        }
    }

    if has_border_width && !has_border_color {
        box_static.extend(quote! {
            if __bs.border_color.is_none() {
                __bs.border_color = Some(::craftui::widgets::ReactiveValue::Dynamic(::std::rc::Rc::new(move || __cui_theme_sig.get().border)));
            }
        });
    }

    if !layout_static.is_empty() {
        stmts.extend(quote! {
            {
                let mut __ls = __tree.nodes.get(#var).map(|n| n.layout.clone()).unwrap_or_default();
                #layout_static
                __tree.set_layout(#var, __ls);
            }
        });
    }
    if !layout_dyn_apply.is_empty() {
        stmts.extend(quote! {
            {
                #layout_dyn_rc
                __tree.bind_dynamic_prop(
                    #var,
                    ::craftui::core::DirtyFlags::LAYOUT | ::craftui::core::DirtyFlags::PAINT,
                    move || { #layout_dyn_track },
                    move |__id, __t| {
                        let mut __ls = __t.nodes.get(__id).map(|n| n.layout.clone()).unwrap_or_default();
                        #layout_dyn_apply
                        __t.set_layout(__id, __ls);
                    },
                );
            }
        });
    }

    if !box_static.is_empty() {
        stmts.extend(quote! {
            {
                let mut __bs = __tree.nodes.get(#var).map(|n| n.box_style.clone()).unwrap_or_default();
                #box_static
                __tree.set_box_style(#var, __bs);
            }
        });
    }
    if !box_dyn_apply.is_empty() {
        stmts.extend(quote! {
            {
                #box_dyn_rc
                __tree.bind_dynamic_prop(
                    #var,
                    ::craftui::core::DirtyFlags::PAINT,
                    move || { #box_dyn_track },
                    move |__id, __t| {
                        let mut __bs = __t.nodes.get(__id).map(|n| n.box_style.clone()).unwrap_or_default();
                        #box_dyn_apply
                        __t.set_box_style(__id, __bs);
                    },
                );
            }
        });
    }

    // Glassmorphism (`backdrop_blur`, `backdrop_tint`)
    let blur_arg = find_arg(&["backdrop_blur"]);
    let tint_arg = find_arg(&["backdrop_tint"]);
    if blur_arg.is_some() || tint_arg.is_some() {
        let reactive = blur_arg.is_some_and(|e| matches!(e, Expr::Closure(_)))
            || tint_arg.is_some_and(|e| {
                matches!(e, Expr::Closure(_)) || extract_theme_token_field(e).is_some()
            });
        if reactive {
            let blur_fn = blur_arg
                .map(|e| {
                    let ev = eval_scalar_expr(e);
                    quote!(move || -> f32 { #ev })
                })
                .unwrap_or_else(|| quote!(|| 0.0f32));
            let tint_fn = tint_arg
                .map(|e| {
                    if let Some(f) = extract_theme_token_field(e) {
                        quote!(move || __cui_theme_sig.get().#f)
                    } else {
                        let ev = eval_typed_expr(e);
                        quote!(move || -> ::craftui::core::Color { #ev })
                    }
                })
                .unwrap_or_else(|| quote!(|| ::craftui::core::Color::TRANSPARENT));
            stmts.extend(quote! {
                {
                    let __bb_fn = ::std::rc::Rc::new(#blur_fn);
                    let __bt_fn = ::std::rc::Rc::new(#tint_fn);
                    let __bb_tr = __bb_fn.clone();
                    let __bt_tr = __bt_fn.clone();
                    __tree.bind_dynamic_prop(
                        #var,
                        ::craftui::core::DirtyFlags::PAINT,
                        move || { let _ = (__bb_tr(), __bt_tr()); },
                        move |__id, __t| { __t.set_backdrop_blur(__id, __bb_fn(), __bt_fn()); },
                    );
                }
            });
        } else {
            let b_val = blur_arg
                .map(|e| quote!(#e as f32))
                .unwrap_or_else(|| quote!(0.0f32));
            let t_val = tint_arg
                .map(|e| quote!(#e))
                .unwrap_or_else(|| quote!(::craftui::core::Color::TRANSPARENT));
            stmts.extend(quote! { __tree.set_backdrop_blur(#var, #b_val, #t_val); });
        }
    }

    // BlendMode & Cursor
    if let Some(bm) = find_arg(&["blend_mode"]) {
        if matches!(bm, Expr::Closure(_)) {
            stmts.extend(quote! {
                {
                    let __bm_fn = ::std::rc::Rc::new(#bm);
                    let __bm_tr = __bm_fn.clone();
                    __tree.bind_dynamic_prop(
                        #var,
                        ::craftui::core::DirtyFlags::PAINT,
                        move || { let _ = __bm_tr(); },
                        move |__id, __t| { __t.set_blend_mode(__id, __bm_fn()); },
                    );
                }
            });
        } else {
            stmts.extend(quote! { __tree.set_blend_mode(#var, #bm); });
        }
    }

    if let Some(cur) = find_arg(&["cursor"]) {
        if matches!(cur, Expr::Closure(_)) {
            stmts.extend(quote! {
                {
                    let __cur_fn = ::std::rc::Rc::new(#cur);
                    let __cur_tr = __cur_fn.clone();
                    __tree.bind_dynamic_prop(
                        #var,
                        ::craftui::core::DirtyFlags::PAINT,
                        move || { let _ = __cur_tr(); },
                        move |__id, __t| {
                            let __c: ::core::option::Option<::craftui::core::CursorIcon> = ::core::convert::Into::into(__cur_fn());
                            __t.set_cursor(__id, __c);
                        },
                    );
                }
            });
        } else {
            stmts.extend(quote! {
                __tree.set_cursor(
                    #var,
                    ::core::convert::Into::<::core::option::Option<::craftui::core::CursorIcon>>::into(#cur),
                );
            });
        }
    }

    // Opacity + Transforms BEFORE set_transition so initial values snap cleanly!
    if let Some(op) = find_arg(&["opacity"]) {
        if matches!(op, Expr::Closure(_)) {
            stmts.extend(quote! {
                {
                    let __op_fn = ::std::rc::Rc::new(#op);
                    let __op_tr = __op_fn.clone();
                    __tree.bind_dynamic_prop(
                        #var,
                        ::craftui::core::DirtyFlags::PAINT,
                        move || { let _ = __op_tr(); },
                        move |__id, __t| { __t.set_opacity(__id, __op_fn() as f32); },
                    );
                }
            });
        } else {
            stmts.extend(quote! { __tree.set_opacity(#var, #op as f32); });
        }
    }

    let sc_arg = find_arg(&["scale"]);
    let scx_arg = find_arg(&["scale_x"]);
    let scy_arg = find_arg(&["scale_y"]);
    if sc_arg.is_some() || scx_arg.is_some() || scy_arg.is_some() {
        let reactive = [sc_arg, scx_arg, scy_arg]
            .into_iter()
            .flatten()
            .any(|e| matches!(e, Expr::Closure(_)));
        if reactive {
            let sx_closure = match (sc_arg, scx_arg) {
                (Some(b), Some(x)) => {
                    let bv = eval_scalar_expr(b);
                    let xv = eval_scalar_expr(x);
                    quote!(move || -> f32 { #bv * #xv })
                }
                (Some(b), None) => {
                    let bv = eval_scalar_expr(b);
                    quote!(move || -> f32 { #bv })
                }
                (None, Some(x)) => {
                    let xv = eval_scalar_expr(x);
                    quote!(move || -> f32 { #xv })
                }
                (None, None) => quote!(|| 1.0f32),
            };
            let sy_closure = match (sc_arg, scy_arg) {
                (Some(b), Some(y)) => {
                    let bv = eval_scalar_expr(b);
                    let yv = eval_scalar_expr(y);
                    quote!(move || -> f32 { #bv * #yv })
                }
                (Some(b), None) => {
                    let bv = eval_scalar_expr(b);
                    quote!(move || -> f32 { #bv })
                }
                (None, Some(y)) => {
                    let yv = eval_scalar_expr(y);
                    quote!(move || -> f32 { #yv })
                }
                (None, None) => quote!(|| 1.0f32),
            };
            stmts.extend(quote! {
                {
                    let __sx_fn = ::std::rc::Rc::new(#sx_closure);
                    let __sy_fn = ::std::rc::Rc::new(#sy_closure);
                    let __sx_tr = __sx_fn.clone();
                    let __sy_tr = __sy_fn.clone();
                    __tree.bind_dynamic_prop(
                        #var,
                        ::craftui::core::DirtyFlags::PAINT,
                        move || { let _ = (__sx_tr(), __sy_tr()); },
                        move |__id, __t| { __t.set_scale(__id, __sx_fn(), __sy_fn()); },
                    );
                }
            });
        } else {
            let sx = match (sc_arg, scx_arg) {
                (Some(b), Some(x)) => quote!((#b as f32) * (#x as f32)),
                (Some(b), None) => quote!(#b as f32),
                (None, Some(x)) => quote!(#x as f32),
                (None, None) => quote!(1.0f32),
            };
            let sy = match (sc_arg, scy_arg) {
                (Some(b), Some(y)) => quote!((#b as f32) * (#y as f32)),
                (Some(b), None) => quote!(#b as f32),
                (None, Some(y)) => quote!(#y as f32),
                (None, None) => quote!(1.0f32),
            };
            stmts.extend(quote! { __tree.set_scale(#var, #sx, #sy); });
        }
    }

    let tx_arg = find_arg(&["translate_x"]);
    let ty_arg = find_arg(&["translate_y"]);
    if tx_arg.is_some() || ty_arg.is_some() {
        let reactive = [tx_arg, ty_arg]
            .into_iter()
            .flatten()
            .any(|e| matches!(e, Expr::Closure(_)));
        if reactive {
            let tx_fn = tx_arg
                .map(|e| {
                    let v = eval_scalar_expr(e);
                    quote!(move || -> f32 { #v })
                })
                .unwrap_or_else(|| quote!(|| 0.0f32));
            let ty_fn = ty_arg
                .map(|e| {
                    let v = eval_scalar_expr(e);
                    quote!(move || -> f32 { #v })
                })
                .unwrap_or_else(|| quote!(|| 0.0f32));
            stmts.extend(quote! {
                {
                    let __tx_fn = ::std::rc::Rc::new(#tx_fn);
                    let __ty_fn = ::std::rc::Rc::new(#ty_fn);
                    let __tx_tr = __tx_fn.clone();
                    let __ty_tr = __ty_fn.clone();
                    __tree.bind_dynamic_prop(
                        #var,
                        ::craftui::core::DirtyFlags::PAINT,
                        move || { let _ = (__tx_tr(), __ty_tr()); },
                        move |__id, __t| { __t.set_translate(__id, __tx_fn(), __ty_fn()); },
                    );
                }
            });
        } else {
            let tx = tx_arg
                .map(|e| quote!(#e as f32))
                .unwrap_or_else(|| quote!(0.0f32));
            let ty = ty_arg
                .map(|e| quote!(#e as f32))
                .unwrap_or_else(|| quote!(0.0f32));
            stmts.extend(quote! { __tree.set_translate(#var, #tx, #ty); });
        }
    }

    if let Some(rot) = find_arg(&["rotate", "rotation"]) {
        if matches!(rot, Expr::Closure(_)) {
            stmts.extend(quote! {
                {
                    let __rot_fn = ::std::rc::Rc::new(#rot);
                    let __rot_tr = __rot_fn.clone();
                    __tree.bind_dynamic_prop(
                        #var,
                        ::craftui::core::DirtyFlags::PAINT,
                        move || { let _ = __rot_tr(); },
                        move |__id, __t| { __t.set_rotation(__id, __rot_fn() as f32); },
                    );
                }
            });
        } else {
            stmts.extend(quote! { __tree.set_rotation(#var, #rot as f32); });
        }
    }

    let kx_arg = find_arg(&["skew_x"]);
    let ky_arg = find_arg(&["skew_y"]);
    if kx_arg.is_some() || ky_arg.is_some() {
        let reactive = [kx_arg, ky_arg]
            .into_iter()
            .flatten()
            .any(|e| matches!(e, Expr::Closure(_)));
        if reactive {
            let kx_fn = kx_arg
                .map(|e| {
                    let v = eval_scalar_expr(e);
                    quote!(move || -> f32 { #v })
                })
                .unwrap_or_else(|| quote!(|| 0.0f32));
            let ky_fn = ky_arg
                .map(|e| {
                    let v = eval_scalar_expr(e);
                    quote!(move || -> f32 { #v })
                })
                .unwrap_or_else(|| quote!(|| 0.0f32));
            stmts.extend(quote! {
                {
                    let __kx_fn = ::std::rc::Rc::new(#kx_fn);
                    let __ky_fn = ::std::rc::Rc::new(#ky_fn);
                    let __kx_tr = __kx_fn.clone();
                    let __ky_tr = __ky_fn.clone();
                    __tree.bind_dynamic_prop(
                        #var,
                        ::craftui::core::DirtyFlags::PAINT,
                        move || { let _ = (__kx_tr(), __ky_tr()); },
                        move |__id, __t| { __t.set_transform(__id, ::craftui::core::Transform2D::skew(__kx_fn(), __ky_fn())); },
                    );
                }
            });
        } else {
            let kx = kx_arg
                .map(|e| quote!(#e as f32))
                .unwrap_or_else(|| quote!(0.0f32));
            let ky = ky_arg
                .map(|e| quote!(#e as f32))
                .unwrap_or_else(|| quote!(0.0f32));
            stmts.extend(quote! { __tree.set_transform(#var, ::craftui::core::Transform2D::skew(#kx, #ky)); });
        }
    }

    // Visibility, Disabled & Event handlers
    if let Some(vis) = find_arg(&["visible", "visible_when"])
        && !matches!(kind_str.as_str(), "Modal" | "BottomSheet" | "PopupMenu")
    {
        if matches!(vis, Expr::Closure(_)) {
            stmts.extend(quote! { __tree.set_visible_when(#var, #vis); });
        } else {
            stmts.extend(quote! { __tree.set_visible_when(#var, move || #vis); });
        }
    }
    if let Some(dis) = find_arg(&["disabled"]) {
        if matches!(dis, Expr::Closure(_)) {
            stmts.extend(quote! {
                __tree.set_disabled(#var, ::craftui::widgets::ReactiveValue::Dynamic(::std::rc::Rc::new(#dis)));
            });
        } else {
            stmts.extend(quote! { __tree.set_disabled(#var, #dis); });
        }
    }
    if kind_str != "Button"
        && let Some(cb) = find_arg(&["on_tap", "on_click"])
    {
        stmts.extend(quote! { __tree.set_on_tap(#var, #cb); });
    }
    if let Some(cb) = find_arg(&["on_double_tap"]) {
        stmts.extend(quote! { __tree.set_on_double_tap(#var, #cb); });
    }
    if let Some(cb) = find_arg(&["on_long_press"]) {
        stmts.extend(quote! { __tree.set_on_long_press(#var, #cb); });
    }
    if let Some(cb) = find_arg(&["on_hover"]) {
        stmts.extend(quote! { __tree.set_on_hover(#var, #cb); });
    }

    // Declarative `animate` transitions LAST (after initial setters + initial dynamic snapshots)
    if let Some(anim_arg) = find_arg(&["animate"]) {
        stmts.extend(quote! {
            {
                let (__ap, __cv) = #anim_arg;
                __tree.set_transition(#var, __ap, __cv);
            }
        });
    }
    for spec in &el.animations {
        let curve = animate_curve_tokens(spec);
        for ap in animatable_prop_tokens(&spec.prop) {
            stmts.extend(quote! { __tree.set_transition(#var, #ap, #curve); });
        }
    }

    // Children (elements, reactive `if/else`, reactive `for [key:]`)
    let slot_layout = match kind_str.as_str() {
        "Row" => quote!(::craftui::layout::LayoutStyle::row()),
        "Stack" => quote!(::craftui::layout::LayoutStyle::stack()),
        "Grid" => {
            let cols = find_arg(&["columns", "cols"])
                .map(|e| quote!(#e as u16))
                .unwrap_or_else(|| quote!(2u16));
            quote!(::craftui::layout::LayoutStyle::grid(#cols))
        }
        _ => quote!(::craftui::layout::LayoutStyle::column()),
    };

    for child in &el.children {
        let cvar = emit_cui_child(child, &slot_layout, counter, stmts);
        stmts.extend(quote! {
            __tree.append_child(#var, #cvar);
        });
    }

    var
}

/// Attribute macro marking a pure-Rust CraftUI component function and injecting reactive `theme` tokens.
#[proc_macro_attribute]
pub fn component(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let mut func = syn::parse_macro_input!(item as ItemFn);
    let block = &func.block;
    let first_arg_ident = func.sig.inputs.first().and_then(|arg| match arg {
        syn::FnArg::Typed(pt) => match pt.pat.as_ref() {
            syn::Pat::Ident(id) => Some(id.ident.clone()),
            _ => None,
        },
        _ => None,
    });

    if let Some(cx_ident) = first_arg_ident {
        let new_block: syn::Block = syn::parse_quote!({
            #[allow(unused_variables)]
            let theme = #cx_ident
                .use_context::<::craftui::reactive::Signal<::craftui::core::ThemeTokens>>()
                .map(|s| s.get())
                .unwrap_or_default();
            #block
        });
        *func.block = new_block;
    }

    quote!(#func).into()
}
