use craftui::prelude::*;
use std::rc::Rc;

pub mod aot_ui {
    include!(concat!(env!("OUT_DIR"), "/showcase.rs"));
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShowcaseRoute {
    Gallery,
    TokenInspector,
}

pub struct AccentSwatchWidget {
    label: &'static str,
    accent: Color,
    clicks: Signal<usize>,
    on_select: Rc<dyn Fn(Color)>,
}

impl AccentSwatchWidget {
    pub fn new(
        label: &'static str,
        accent: Color,
        clicks: Signal<usize>,
        on_select: impl Fn(Color) + 'static,
    ) -> Self {
        Self {
            label,
            accent,
            clicks,
            on_select: Rc::new(on_select),
        }
    }
}

impl CustomWidget for AccentSwatchWidget {
    fn debug_name(&self) -> &'static str {
        "AccentSwatchWidget"
    }

    fn measure(
        &mut self,
        _constraints: Constraints,
        _text_engine: &mut craftui::text::TextEngine,
        _theme: &ThemeTokens,
    ) -> Size {
        Size::new(112.0, 32.0)
    }

    fn paint(
        &mut self,
        rect: Rect,
        scene: &mut PaintScene,
        _text_engine: &mut craftui::text::TextEngine,
        theme: &ThemeTokens,
        focused: bool,
    ) {
        scene.fill_rect(rect, theme.surface_variant, CornerRadii::all(8.0));
        scene.stroke_rect(
            rect,
            if focused { theme.primary } else { theme.border },
            if focused { 2.0 } else { 1.0 },
            CornerRadii::all(8.0),
        );
        let dot = Rect::new(rect.x + 8.0, rect.y + 8.0, 16.0, 16.0);
        scene.fill_rect(dot, self.accent, CornerRadii::all(8.0));
        scene.draw_text(
            Point::new(rect.x + 30.0, rect.y + 8.0),
            self.label,
            TextStyle::default()
                .with_size(12.5)
                .with_weight(600.0)
                .with_color(theme.text_primary),
            Some(rect.width - 36.0),
        );
    }

    fn handle_event(&mut self, rect: Rect, event: &UiEvent) -> bool {
        if let UiEvent::Pointer(pe) = event
            && matches!(pe.phase, PointerPhase::Up)
            && rect.contains(pe.position)
        {
            self.clicks.update(|c| *c += 1);
            (self.on_select)(self.accent);
            return true;
        }
        false
    }

    fn a11y(&self) -> A11yMetadata {
        A11yMetadata {
            role: A11yRole::Button,
            label: Some(format!("Apply {} swatch", self.label)),
            focusable: true,
            ..Default::default()
        }
    }
}

const MODES: [ThemeMode; 4] = [
    ThemeMode::Dark,
    ThemeMode::Light,
    ThemeMode::Amoled,
    ThemeMode::HighContrast,
];

pub fn mount_showcase_app(
    cx: Scope,
    tree: &mut WidgetTree,
    launcher: Option<WindowLauncher>,
) -> (NodeId, Router<ShowcaseRoute>, Signal<usize>) {
    let theme_idx = cx.signal(0usize);
    let swatch_hits = cx.signal(0usize);
    let router = Router::new(cx, ShowcaseRoute::Gallery);
    let theme_sig = tree.theme_signal;
    let ov_sig = tree.theme_override_signal;

    let screen = aot_ui::ShowcaseScreen::new(cx);
    screen.set_on_theme_cycle(move || {
        let next = (theme_idx.get_untracked() + 1) % MODES.len();
        theme_idx.set(next);
        theme_sig.set(ThemeTokens::from_mode(MODES[next]).applied(&ov_sig.get_untracked()));
    });

    let top_bar = {
        let r_nav = router;
        let l_opt = launcher;
        let ov_ocean = ov_sig;
        let th_ocean = theme_sig;
        let ov_amber = ov_sig;
        let th_amber = theme_sig;
        let _ = theme_idx;

        cui! {
            Column(width: pct(100), height: pct(100), bg: theme.background) {
                Row(
                    width: pct(100),
                    bg: theme.surface,
                    padding_x: 20,
                    padding_y: 10,
                    gap: 10,
                    align: Alignment::Center,
                    justify: Justify::SpaceBetween,
                    border_width: 1,
                    border_color: theme.border
                ) {
                    Row(gap: 8, align: Alignment::Center) {
                        Button(
                            label: move || match r_nav.current() {
                                ShowcaseRoute::Gallery => "Inspect Theme Tokens".to_string(),
                                ShowcaseRoute::TokenInspector => "< Back to Gallery".to_string(),
                            },
                            bg: theme.surface_variant,
                            fg: theme.text_primary,
                            on_tap: move || match r_nav.current() {
                                ShowcaseRoute::Gallery => r_nav.push(ShowcaseRoute::TokenInspector),
                                ShowcaseRoute::TokenInspector => {
                                    r_nav.pop();
                                }
                            }
                        )
                    }
                    Row(gap: 8, align: Alignment::Center) {
                        Custom(widget: AccentSwatchWidget::new(
                            "Ocean",
                            Color::rgb(14, 165, 233),
                            swatch_hits,
                            move |accent| {
                                let ov = ThemeOverride::new().with_primary(accent, Color::WHITE);
                                ov_ocean.set(ov.clone());
                                let mode = th_ocean.get_untracked().mode;
                                th_ocean.set(ThemeTokens::from_mode(mode).applied(&ov));
                            },
                        ))
                        Custom(widget: AccentSwatchWidget::new(
                            "Amber",
                            Color::rgb(217, 119, 6),
                            swatch_hits,
                            move |accent| {
                                let ov = ThemeOverride::new().with_primary(accent, Color::WHITE);
                                ov_amber.set(ov.clone());
                                let mode = th_amber.get_untracked().mode;
                                th_amber.set(ThemeTokens::from_mode(mode).applied(&ov));
                            },
                        ))
                        Button(
                            label: "New Window",
                            bg: theme.surface_variant,
                            fg: theme.text_primary,
                            on_tap: move || {
                                if let Some(ref win) = l_opt {
                                    let _ = win.open_window(
                                        WindowConfig {
                                            title: "CraftUI Theme Tokens".to_string(),
                                            width: 440.0,
                                            height: 320.0,
                                            ..Default::default()
                                        },
                                        |w_cx, _w_tree, _ctx| {
                                            cui! {
                                                Column(width: pct(100), height: pct(100), bg: theme.background, padding: 24, gap: 12) {
                                                    Text(text: "Secondary CraftUI Window", size: 20, weight: 700.0, color: theme.text_primary)
                                                    Text(text: "Multi-window DesktopRunner with isolated child Scope.", size: 14, color: theme.text_secondary)
                                                }
                                            }.build(w_cx, _w_tree)
                                        },
                                    );
                                }
                            }
                        )
                    }
                }
            }
        }
        .build(cx, tree)
    };

    let content_slot = tree.dynamic_slot(
        cx,
        LayoutStyle::column()
            .with_size(pct(100), pct(100))
            .with_flex_grow(1.0),
        {
            let r_c = router;
            move || r_c.current()
        },
        move |slot_cx, route, slot_tree| match route {
            ShowcaseRoute::Gallery => screen.build(slot_cx, slot_tree),
            ShowcaseRoute::TokenInspector => cui! {
                Column(width: pct(100), height: pct(100), padding: 24, gap: 14, bg: theme.background) {
                    Text(text: "Live Theme Tokens Inspector", size: 22, weight: 700.0, color: theme.text_primary)
                    Text(
                        text: move || format!("Active Theme Mode: {:?} | Primary: {}", theme_sig.get().mode, theme_sig.get().primary.to_hex()),
                        size: 14,
                        color: theme.text_secondary
                    )
                    Row(gap: 12) {
                        Container(width: dp(120), height: dp(64), bg: theme.primary, radius: 10)
                        Container(width: dp(120), height: dp(64), bg: theme.surface, radius: 10, border_width: 1, border_color: theme.border)
                        Container(width: dp(120), height: dp(64), bg: theme.danger, radius: 10)
                    }
                }
            }
            .build(slot_cx, slot_tree),
        },
    );
    tree.append_child(top_bar, content_slot);

    (top_bar, router, swatch_hits)
}

fn main() -> craftui::core::Result<()> {
    run_app(
        WindowConfig {
            title: "CraftUI Flagship Showcase".to_string(),
            width: 960.0,
            height: 700.0,
            ..Default::default()
        },
        |cx, tree, ctx| {
            let (root, _, _) = mount_showcase_app(cx, tree, Some(ctx.window_launcher()));
            root
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use craftui::reactive::reset_runtime;

    fn click_at(tree: &mut WidgetTree, pos: Point) {
        tree.handle_event(&UiEvent::Pointer(PointerEvent {
            id: PointerId(1),
            phase: PointerPhase::Down,
            position: pos,
            button: Some(PointerButton::Primary),
            modifiers: Modifiers::default(),
            is_touch: false,
            timestamp_ms: 10,
        }));
        tree.handle_event(&UiEvent::Pointer(PointerEvent {
            id: PointerId(1),
            phase: PointerPhase::Up,
            position: pos,
            button: Some(PointerButton::Primary),
            modifiers: Modifiers::default(),
            is_touch: false,
            timestamp_ms: 40,
        }));
    }

    #[test]
    fn aot_cui_multifile_showcase_compiles_interacts_and_switches_theme() {
        reset_runtime();
        let cx = Scope::root();
        let mut tree = WidgetTree::new(cx);
        let screen = aot_ui::ShowcaseScreen::new(cx);
        let root = screen.build(cx, &mut tree);
        tree.set_root(root);

        let metrics = WindowMetrics {
            logical_size: Size::new(960.0, 700.0),
            ..Default::default()
        };
        tree.layout(metrics);

        let mut dark_scene = PaintScene::new();
        tree.paint(&mut dark_scene);
        let textures = TextureRegistry::new();
        let dark_rgba = render_scene_to_rgba(
            BackendKind::VelloCpu,
            &dark_scene,
            960,
            700,
            1.0,
            tree.theme().background,
            &textures,
            &mut tree.text_engine,
        );
        assert_eq!(dark_rgba.len(), 960 * 700 * 4);
        assert!(dark_rgba.iter().any(|&b| b > 40));

        // Switch to DataList tab -> renders VirtualList cleanly
        screen.active_tab.set(1);
        tree.layout(metrics);

        // Open Modal & verify Escape closes via reactive on_close / open<=>show_modal
        screen.show_modal.set(true);
        tree.layout(metrics);
        tree.handle_event(&UiEvent::Key(KeyEvent {
            code: KeyCode::Escape,
            text: None,
            pressed: true,
            repeat: false,
            modifiers: Modifiers::default(),
        }));
        assert!(!screen.show_modal.get());

        // Live theme switch updates reactive Theme.* bindings
        tree.set_theme_mode(ThemeMode::Light);
        for _ in 0..120 {
            if !tree.tick_animations(0.016) {
                break;
            }
        }
        tree.layout(metrics);
        let mut light_scene = PaintScene::new();
        tree.paint(&mut light_scene);
        let light_rgba = render_scene_to_rgba(
            BackendKind::VelloCpu,
            &light_scene,
            960,
            700,
            1.0,
            tree.theme().background,
            &textures,
            &mut tree.text_engine,
        );
        assert_ne!(dark_rgba, light_rgba);
    }

    #[test]
    fn custom_widget_theme_override_and_router_work() {
        reset_runtime();
        let cx = Scope::root();
        let mut tree = WidgetTree::new(cx);
        let (root, router, hits) = mount_showcase_app(cx, &mut tree, None);
        tree.set_root(root);

        let metrics = WindowMetrics {
            logical_size: Size::new(960.0, 700.0),
            ..Default::default()
        };
        tree.layout(metrics);

        // Find CustomWidget node ("AccentSwatchWidget") and click it
        let swatch_id = tree
            .nodes
            .iter()
            .find_map(|(id, node)| (node.debug_name == "AccentSwatchWidget").then_some(id))
            .expect("swatch widget missing");
        let rect = tree.rect(swatch_id);
        click_at(&mut tree, rect.center());
        assert_eq!(hits.get(), 1);
        assert_eq!(tree.theme().primary, Color::rgb(14, 165, 233));

        // Navigate with Router
        assert_eq!(router.current(), ShowcaseRoute::Gallery);
        router.push(ShowcaseRoute::TokenInspector);
        tree.layout(metrics);
        assert_eq!(router.current(), ShowcaseRoute::TokenInspector);
        assert!(router.pop());
        assert_eq!(router.current(), ShowcaseRoute::Gallery);
    }
}
