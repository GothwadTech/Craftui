//! Procedural macros (`cui!` and `#[component]`) for pure-Rust declarative UI authoring.

use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use quote::{format_ident, quote};
use syn::parse::{Parse, ParseStream};
use syn::punctuated::Punctuated;
use syn::{Expr, Ident, ItemFn, Token, braced, parenthesized};

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

/// Inline declarative Rust UI builder macro matching `DESIGN.md` §2.2:
///
/// ```ignore
/// cui! {
///     Column(width: pct(100), height: pct(100), bg: theme.background) {
///         Text(text: "Hello")
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
        "Text" => {
            let text_expr = find_arg(&["text", "label", "content"])
                .map(|e| quote!(#e))
                .unwrap_or_else(|| quote!(""));
            let size_expr = find_arg(&["size", "font_size"]);
            let color_expr = find_arg(&["color", "fg", "foreground"]);
            let weight_expr = find_arg(&["weight", "font_weight"]);
            let mut style_chain = quote! {
                let mut __ts = ::craftui::text::TextStyle::default().with_color(__tree.theme().text_primary);
            };
            if let Some(s) = size_expr {
                style_chain.extend(quote! { __ts.font_size = #s as f32; });
            }
            if let Some(c) = color_expr {
                style_chain.extend(quote! { __ts.color = #c; });
            }
            if let Some(w) = weight_expr {
                style_chain.extend(quote! { __ts.font_weight = #w as f32; });
            }
            quote! {
                let #var = {
                    #style_chain
                    __tree.text(__cx, #text_expr, __ts)
                };
            }
        }
        "Button" => {
            let label = find_arg(&["label", "text", "title"])
                .map(|e| quote!(#e))
                .unwrap_or_else(|| quote!("Button"));
            let on_tap = find_arg(&["on_tap", "on_click"])
                .map(|e| quote!(#e))
                .unwrap_or_else(|| quote!(|| {}));
            quote! { let #var = __tree.button(__cx, #label, #on_tap); }
        }
        "ImageView" | "Image" => {
            let src = find_arg(&["source", "src", "texture"])
                .map(|e| quote!(::craftui::widgets::ReactiveValue::Dynamic(::std::rc::Rc::new(move || Some(#e.get().id)))))
                .unwrap_or_else(|| quote!(::craftui::widgets::ReactiveValue::Static(None)));
            let fit = find_arg(&["fit"])
                .map(|e| quote!(#e))
                .unwrap_or_else(|| quote!(::craftui::core::Fit::Contain));
            let rad = find_arg(&["radius", "border_radius"])
                .map(|e| quote!(::craftui::core::CornerRadii::all(#e as f32)))
                .unwrap_or_else(|| quote!(::craftui::core::CornerRadii::ZERO));
            quote! { let #var = __tree.image(__cx, #src, #fit, #rad); }
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
            quote! { let #var = __tree.slider(__cx, #val, #min, #max, #lbl); }
        }
        "Checkbox" => {
            let chk = find_arg(&["checked"]).expect("Checkbox requires `checked` Signal<bool>");
            let lbl = find_arg(&["label"])
                .map(|e| quote!(#e))
                .unwrap_or_else(|| quote!(""));
            quote! { let #var = __tree.checkbox(__cx, #chk, #lbl); }
        }
        "Switch" => {
            let chk = find_arg(&["checked"]).expect("Switch requires `checked` Signal<bool>");
            let lbl = find_arg(&["label"])
                .map(|e| quote!(#e))
                .unwrap_or_else(|| quote!(""));
            quote! { let #var = __tree.switch(__cx, #chk, #lbl); }
        }
        "TextInput" => {
            let ph = find_arg(&["placeholder"])
                .map(|e| quote!(#e))
                .unwrap_or_else(|| quote!(""));
            let sig = find_arg(&["value"])
                .map(|e| quote!(Some(#e)))
                .unwrap_or_else(|| quote!(None));
            quote! { let #var = __tree.text_input(__cx, "", #ph, #sig); }
        }
        "ScrollView" => {
            let axis = find_arg(&["axis", "direction"])
                .map(|e| quote!(#e))
                .unwrap_or_else(|| quote!(::craftui::core::Axis::Vertical));
            quote! { let #var = __tree.scroll_view(__cx, #axis, ::craftui::layout::LayoutStyle::column()); }
        }
        _ => quote! { let #var = __tree.column(__cx); },
    };

    stmts.extend(init_stmt);

    // Apply layout and visual style properties
    let mut layout_updates = TokenStream2::new();
    let mut box_updates = TokenStream2::new();

    for arg in &el.args {
        let name = arg.name.to_string();
        let val = &arg.value;
        match name.as_str() {
            "width" => layout_updates.extend(quote! { __ls.width = #val; }),
            "height" => layout_updates.extend(quote! { __ls.height = #val; }),
            "flex_grow" => layout_updates.extend(quote! { __ls.flex_grow = #val as f32; }),
            "gap" => layout_updates.extend(quote! { __ls.gap_x = #val as f32; __ls.gap_y = #val as f32; }),
            "padding" => layout_updates.extend(quote! { __ls.padding = ::craftui::core::Insets::all(#val as f32); }),
            "padding_x" => layout_updates.extend(quote! { __ls.padding.left = #val as f32; __ls.padding.right = #val as f32; }),
            "padding_y" => layout_updates.extend(quote! { __ls.padding.top = #val as f32; __ls.padding.bottom = #val as f32; }),
            "margin" => layout_updates.extend(quote! { __ls.margin = ::craftui::core::Insets::all(#val as f32); }),
            "align" => layout_updates.extend(quote! { __ls.align_items = #val; }),
            "justify" => layout_updates.extend(quote! { __ls.justify_content = #val; }),
            "safe_area" => layout_updates.extend(quote! { __ls.safe_area = #val; }),
            "bg" | "background" => box_updates.extend(quote! {
                __bs.background = Some(::craftui::widgets::ReactiveValue::Static(::craftui::core::Brush::Solid(#val)));
            }),
            "radius" | "border_radius" => box_updates.extend(quote! {
                __bs.radius = ::craftui::core::CornerRadii::all(#val as f32);
            }),
            "opacity" => box_updates.extend(quote! {
                __bs.opacity = #val as f32;
            }),
            _ => {}
        }
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

    for child in &el.children {
        let cvar = emit_cui_node(child, counter, stmts);
        stmts.extend(quote! {
            __tree.append_child(#var, #cvar);
        });
    }

    var
}

/// Attribute macro marking a pure-Rust CraftUI component function and injecting `theme` tokens.
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
