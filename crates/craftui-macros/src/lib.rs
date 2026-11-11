//! Procedural macros (`cui!` and `#[component]`) for pure-Rust declarative UI authoring.

use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use quote::{format_ident, quote};
use syn::parse::{Parse, ParseStream};
use syn::punctuated::Punctuated;
use syn::{Expr, Ident, ItemFn, Lit, Member, Token, braced, parenthesized};

struct PropArg {
    name: Ident,
    value: Expr,
}

impl Parse for PropArg {
    fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
        let name: Ident = input.parse()?;
        input.parse::<Token![:]>()?;
        let value: Expr = input.parse()?;
        Ok(Self { name, value })
    }
}

struct CuiElement {
    kind: Ident,
    args: Vec<PropArg>,
    children: Vec<CuiElement>,
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
        let mut children = Vec::new();
        if input.peek(syn::token::Brace) {
            let content;
            braced!(content in input);
            while !content.is_empty() {
                children.push(content.parse()?);
            }
        }
        Ok(Self {
            kind,
            args,
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

fn emit_cui_node(el: &CuiElement, counter: &mut usize, stmts: &mut TokenStream2) -> Ident {
    let var = format_ident!("__cui_node_{}", *counter);
    *counter += 1;

    let kind_str = el.kind.to_string();
    let find_arg = |names: &[&str]| -> Option<&Expr> {
        el.args
            .iter()
            .find(|a| names.iter().any(|n| a.name == *n))
            .map(|a| &a.value)
    };

    let init_stmt = match kind_str.as_str() {
        "Row" => quote! { let #var = __tree.row(__cx); },
        "Column" | "View" | "Container" => quote! { let #var = __tree.column(__cx); },
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
        "Text" => {
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
        "Button" => {
            let label = find_arg(&["label", "text", "title"])
                .map(|e| quote!(#e))
                .unwrap_or_else(|| quote!("Button"));
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
            let val = find_arg(&["value"]).expect("Slider requires `value` signal");
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
                let #var = __tree.slider(__cx, #val, #min, #max, #lbl);
                #chg
            }
        }
        "Checkbox" => {
            let chk = find_arg(&["checked"]).expect("Checkbox requires `checked` Signal<bool>");
            let lbl = find_arg(&["label", "text"])
                .map(|e| quote!(#e))
                .unwrap_or_else(|| quote!(""));
            let chg =
                find_arg(&["on_change"]).map(|cb| quote! { __tree.set_on_bool_change(#var, #cb); });
            quote! {
                let #var = __tree.checkbox(__cx, #chk, #lbl);
                #chg
            }
        }
        "Switch" => {
            let chk = find_arg(&["checked"]).expect("Switch requires `checked` Signal<bool>");
            let lbl = find_arg(&["label", "text"])
                .map(|e| quote!(#e))
                .unwrap_or_else(|| quote!(""));
            let chg =
                find_arg(&["on_change"]).map(|cb| quote! { __tree.set_on_bool_change(#var, #cb); });
            quote! {
                let #var = __tree.switch(__cx, #chk, #lbl);
                #chg
            }
        }
        "TextInput" | "TextArea" => {
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
            let sig = find_arg(&["value"])
                .map(|e| quote!(Some(#e)))
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
            let active = find_arg(&["active", "selected", "value"])
                .expect("Tabs requires `active` Signal<usize>");
            let chg = find_arg(&["on_change", "on_select"])
                .map(|cb| quote! { __tree.set_on_index_change(#var, #cb); });
            quote! {
                let #var = __tree.tabs(__cx, #tabs, #active);
                #chg
            }
        }
        "Select" | "Dropdown" => {
            let opts = find_arg(&["options", "items"])
                .map(|e| quote!(#e))
                .unwrap_or_else(|| quote!(Vec::<String>::new()));
            let sel =
                find_arg(&["selected", "value"]).expect("Select requires `selected` Signal<usize>");
            let ph = find_arg(&["placeholder", "label"])
                .map(|e| quote!(#e))
                .unwrap_or_else(|| quote!("Select..."));
            let chg = find_arg(&["on_change", "on_select"])
                .map(|cb| quote! { __tree.set_on_index_change(#var, #cb); });
            quote! {
                let #var = __tree.select(__cx, #opts, #sel, #ph);
                #chg
            }
        }
        "RadioGroup" => {
            let opts = find_arg(&["options", "items"])
                .map(|e| quote!(#e))
                .unwrap_or_else(|| quote!(Vec::<String>::new()));
            let sel = find_arg(&["selected", "value"])
                .expect("RadioGroup requires `selected` Signal<usize>");
            let axis = find_arg(&["axis", "direction"])
                .map(|e| quote!(#e))
                .unwrap_or_else(|| quote!(::craftui::core::Axis::Vertical));
            let chg = find_arg(&["on_change", "on_select"])
                .map(|cb| quote! { __tree.set_on_index_change(#var, #cb); });
            quote! {
                let #var = __tree.radio_group(__cx, #opts, #sel, #axis);
                #chg
            }
        }
        "PopupMenu" => {
            let open = find_arg(&["open", "visible"])
                .map(|e| quote!(#e))
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
        "Modal" | "BottomSheet" => {
            let is_sheet = kind_str == "BottomSheet";
            let open = find_arg(&["open", "visible"])
                .map(|e| quote!(#e))
                .unwrap_or_else(|| quote!(false));
            let on_cls = find_arg(&["on_close"])
                .map(|cb| {
                    quote!(Some(::std::rc::Rc::new(::std::cell::RefCell::new(#cb)) as ::craftui::widgets::TapHandler))
                })
                .unwrap_or_else(|| quote!(None));
            quote! { let #var = __tree.overlay(__cx, #open, #is_sheet, #on_cls); }
        }
        "PlatformView" => {
            let view_id = find_arg(&["view_id", "id"])
                .map(|e| quote!(#e))
                .unwrap_or_else(|| quote!(::craftui::core::PlatformViewId::default()));
            quote! {
                let #var = __tree.platform_view(__cx, #view_id, ::craftui::layout::LayoutStyle::column());
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

    // Apply layout and visual style properties
    let mut layout_updates = TokenStream2::new();
    let mut box_updates = TokenStream2::new();
    let mut has_border_color = false;
    let mut has_border_width = false;

    for arg in &el.args {
        let name = arg.name.to_string();
        let val = &arg.value;
        match name.as_str() {
            "width" => layout_updates.extend(quote! { __ls.width = #val; }),
            "height" => layout_updates.extend(quote! { __ls.height = #val; }),
            "min_width" => layout_updates.extend(quote! { __ls.min_width = #val; }),
            "max_width" => layout_updates.extend(quote! { __ls.max_width = #val; }),
            "min_height" => layout_updates.extend(quote! { __ls.min_height = #val; }),
            "max_height" => layout_updates.extend(quote! { __ls.max_height = #val; }),
            "flex_grow" => layout_updates.extend(quote! { __ls.flex_grow = #val as f32; }),
            "flex_shrink" => layout_updates.extend(quote! { __ls.flex_shrink = #val as f32; }),
            "gap" => layout_updates
                .extend(quote! { __ls.gap_x = #val as f32; __ls.gap_y = #val as f32; }),
            "padding" => layout_updates
                .extend(quote! { __ls.padding = ::craftui::core::Insets::all(#val as f32); }),
            "padding_x" => layout_updates.extend(
                quote! { __ls.padding.left = #val as f32; __ls.padding.right = #val as f32; },
            ),
            "padding_y" => layout_updates.extend(
                quote! { __ls.padding.top = #val as f32; __ls.padding.bottom = #val as f32; },
            ),
            "margin" => layout_updates
                .extend(quote! { __ls.margin = ::craftui::core::Insets::all(#val as f32); }),
            "align" => layout_updates.extend(quote! { __ls.align_items = #val; }),
            "justify" => layout_updates.extend(quote! { __ls.justify_content = #val; }),
            "safe_area" => layout_updates.extend(quote! { __ls.safe_area = #val; }),
            "bg" | "background" => {
                let rv = reactive_brush_tokens(val);
                box_updates.extend(quote! { __bs.background = Some(#rv); });
            }
            "radius" | "border_radius" => box_updates.extend(quote! {
                __bs.radius = ::craftui::core::CornerRadii::all(#val as f32);
                if let Some(ref mut __br) = __bs.border { __br.radius = __bs.radius; }
            }),
            "border_width" => {
                has_border_width = true;
                box_updates.extend(quote! {
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
            "border_color" => {
                has_border_color = true;
                let rv = reactive_color_tokens(val);
                box_updates.extend(quote! {
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
            "opacity" => box_updates.extend(quote! {
                __bs.opacity = #val as f32;
            }),
            "clip" => {
                box_updates.extend(quote! { __bs.clip = #val; });
                layout_updates.extend(quote! { __ls.clip_overflow = #val; });
            }
            "shadow" => {
                if let Expr::Lit(syn::ExprLit {
                    lit: Lit::Bool(b), ..
                }) = val
                {
                    if b.value {
                        box_updates.extend(quote! {
                            __bs.shadow = Some(::craftui::core::BoxShadow::new(
                                0.0,
                                4.0,
                                12.0,
                                ::craftui::core::Color::rgba(0, 0, 0, 80),
                            ));
                        });
                    } else {
                        box_updates.extend(quote! { __bs.shadow = None; });
                    }
                } else {
                    box_updates.extend(quote! {
                        __bs.shadow = Some(#val);
                    });
                }
            }
            _ => {}
        }
    }

    if has_border_width && !has_border_color {
        box_updates.extend(quote! {
            if __bs.border_color.is_none() {
                __bs.border_color = Some(::craftui::widgets::ReactiveValue::Dynamic(::std::rc::Rc::new(move || __cui_theme_sig.get().border)));
            }
        });
    }

    if !layout_updates.is_empty() {
        stmts.extend(quote! {
            {
                let mut __ls = __tree.nodes.get(#var).map(|n| n.layout.clone()).unwrap_or_default();
                #layout_updates
                __tree.set_layout(#var, __ls);
            }
        });
    }
    if !box_updates.is_empty() {
        stmts.extend(quote! {
            {
                let mut __bs = __tree.nodes.get(#var).map(|n| n.box_style.clone()).unwrap_or_default();
                #box_updates
                __tree.set_box_style(#var, __bs);
            }
        });
    }

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
        stmts.extend(quote! { __tree.set_disabled(#var, #dis); });
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

    for child in &el.children {
        let cvar = emit_cui_node(child, counter, stmts);
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
