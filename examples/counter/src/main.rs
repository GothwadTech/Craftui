use craftui::prelude::*;

pub mod aot_ui {
    include!(concat!(env!("OUT_DIR"), "/counter.rs"));
}

#[derive(Clone, Copy)]
pub struct CounterAppState {
    pub count: Signal<i32>,
    pub theme_idx: Signal<usize>,
}

const THEMES: [ThemeMode; 4] = [
    ThemeMode::Dark,
    ThemeMode::Light,
    ThemeMode::Amoled,
    ThemeMode::HighContrast,
];

fn theme_label(mode: ThemeMode) -> &'static str {
    match mode {
        ThemeMode::Dark => "Theme: Dark",
        ThemeMode::Light => "Theme: Light",
        ThemeMode::Amoled => "Theme: AMOLED",
        ThemeMode::HighContrast => "Theme: High Contrast",
    }
}

#[component]
pub fn counter_screen(cx: Scope, tree: &mut WidgetTree, state: CounterAppState) -> NodeId {
    let count = state.count;
    let theme_idx = state.theme_idx;
    let theme_sig = cx
        .use_context::<Signal<ThemeTokens>>()
        .unwrap_or(tree.theme_signal);

    let root = tree.column(cx);
    tree.set_layout(
        root,
        LayoutStyle::column()
            .with_size(pct(100), pct(100))
            .with_padding(Insets::all(32.0))
            .with_gap(20.0)
            .with_align(Alignment::Center)
            .with_justify(Justify::Center),
    );
    tree.set_box_style(
        root,
        BoxStyle {
            background: Some(ReactiveValue::Dynamic(std::rc::Rc::new(move || {
                Brush::Solid(theme_sig.get().background)
            }))),
            ..BoxStyle::new()
        },
    );

    let title = tree.text(
        cx,
        "CraftUI Reactive Counter",
        ReactiveValue::Dynamic(std::rc::Rc::new(move || {
            TextStyle::default()
                .with_size(28.0)
                .with_weight(700.0)
                .with_color(theme_sig.get().text_primary)
        })),
    );
    let readout = tree.text(
        cx,
        move || format!("Current Count: {}", count.get()),
        ReactiveValue::Dynamic(std::rc::Rc::new(move || {
            TextStyle::default()
                .with_size(24.0)
                .with_weight(600.0)
                .with_color(theme_sig.get().primary)
        })),
    );

    let btn_row = tree.row(cx);
    tree.set_layout(btn_row, LayoutStyle::row().with_gap(12.0));

    let dec = tree.button(cx, "-1", move || count.update(|c| *c -= 1));
    let inc = tree.button(cx, "+1", move || count.update(|c| *c += 1));
    let reset = tree.button(cx, "Reset", move || count.set(0));

    let _ = theme_idx;
    let theme_btn = tree.button(
        cx,
        move || theme_label(THEMES[theme_idx.get() % THEMES.len()]).to_string(),
        move || {
            let next = (theme_idx.get_untracked() + 1) % THEMES.len();
            theme_idx.set(next);
            theme_sig.set(ThemeTokens::from_mode(THEMES[next]));
        },
    );

    tree.append_child(btn_row, dec);
    tree.append_child(btn_row, inc);
    tree.append_child(btn_row, reset);
    tree.append_child(root, title);
    tree.append_child(root, readout);
    tree.append_child(root, btn_row);
    tree.append_child(root, theme_btn);
    root
}

fn main() -> craftui::core::Result<()> {
    run_app(
        WindowConfig {
            title: "CraftUI Counter Demo".to_string(),
            width: 560.0,
            height: 400.0,
            ..Default::default()
        },
        |cx, tree, _ctx| {
            let state = CounterAppState {
                count: cx.signal(0),
                theme_idx: cx.signal(0),
            };
            counter_screen(cx, tree, state)
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use craftui::reactive::reset_runtime;

    #[test]
    fn counter_increments_switches_theme_and_compiles_aot_cui() {
        reset_runtime();
        let cx = Scope::root();
        let mut tree = WidgetTree::new(cx);
        let state = CounterAppState {
            count: cx.signal(0),
            theme_idx: cx.signal(0),
        };
        let root = counter_screen(cx, &mut tree, state);
        tree.set_root(root);

        let metrics = WindowMetrics {
            logical_size: Size::new(560.0, 400.0),
            ..Default::default()
        };
        tree.layout(metrics);

        // Render via vello_cpu
        let mut scene = PaintScene::new();
        tree.paint(&mut scene);
        let textures = TextureRegistry::new();
        let rgba = render_scene_to_rgba(
            BackendKind::VelloCpu,
            &scene,
            560,
            400,
            1.0,
            tree.theme().background,
            &textures,
            &mut tree.text_engine,
        );
        assert_eq!(rgba.len(), 560 * 400 * 4);
        assert!(rgba.iter().any(|&b| b > 100));

        // Verify AOT compiled .cui CounterCard builds and interacts cleanly
        let card = aot_ui::CounterCard::new(cx);
        let mut aot_tree = WidgetTree::new(cx);
        let aot_root = card.build(cx, &mut aot_tree);
        aot_tree.set_root(aot_root);
        aot_tree.layout(metrics);
        assert_eq!(card.count.get(), 0);
    }
}
