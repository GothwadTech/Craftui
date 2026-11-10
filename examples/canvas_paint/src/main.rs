use craftui::prelude::*;
use kurbo::BezPath;
use std::cell::{Cell, RefCell};
use std::rc::Rc;

pub mod aot_ui {
    include!(concat!(env!("OUT_DIR"), "/toolbar.rs"));
}

#[derive(Debug, Clone, PartialEq)]
pub struct Stroke {
    pub points: Vec<Point>,
    pub color: Color,
    pub width: f32,
}

const PALETTE: [(&str, Color); 5] = [
    ("Blue", Color::rgb(59, 130, 246)),
    ("Coral", Color::rgb(239, 68, 68)),
    ("Gold", Color::rgb(245, 158, 11)),
    ("Teal", Color::rgb(20, 184, 166)),
    ("Slate", Color::rgb(148, 163, 184)),
];

pub struct SwatchButtonWidget {
    label: &'static str,
    color: Color,
    active_color: Signal<Color>,
    on_pick: Rc<dyn Fn(Color)>,
}

impl SwatchButtonWidget {
    pub fn new(
        label: &'static str,
        color: Color,
        active_color: Signal<Color>,
        on_pick: impl Fn(Color) + 'static,
    ) -> Self {
        Self {
            label,
            color,
            active_color,
            on_pick: Rc::new(on_pick),
        }
    }
}

impl CustomWidget for SwatchButtonWidget {
    fn debug_name(&self) -> &'static str {
        "SwatchButtonWidget"
    }

    fn measure(
        &mut self,
        _constraints: Constraints,
        _text_engine: &mut craftui::text::TextEngine,
        _theme: &ThemeTokens,
    ) -> Size {
        Size::new(112.0, 34.0)
    }

    fn paint(
        &mut self,
        rect: Rect,
        scene: &mut PaintScene,
        _text_engine: &mut craftui::text::TextEngine,
        theme: &ThemeTokens,
        focused: bool,
    ) {
        let selected = self.active_color.get_untracked() == self.color;
        scene.fill_rect(
            rect,
            if selected {
                theme.surface_variant
            } else {
                theme.surface
            },
            CornerRadii::all(8.0),
        );
        scene.stroke_rect(
            rect,
            if selected || focused {
                theme.primary
            } else {
                theme.border
            },
            if selected { 2.0 } else { 1.0 },
            CornerRadii::all(8.0),
        );
        scene.fill_rect(
            Rect::new(rect.x + 8.0, rect.y + 9.0, 16.0, 16.0),
            self.color,
            CornerRadii::all(8.0),
        );
        scene.draw_text(
            Point::new(rect.x + 30.0, rect.y + 9.0),
            self.label,
            TextStyle::default()
                .with_size(13.0)
                .with_weight(if selected { 700.0 } else { 500.0 })
                .with_color(theme.text_primary),
            Some(rect.width - 36.0),
        );
    }

    fn handle_event(&mut self, rect: Rect, event: &UiEvent) -> bool {
        if let UiEvent::Pointer(pe) = event
            && matches!(pe.phase, PointerPhase::Up)
            && rect.contains(pe.position)
        {
            self.active_color.set(self.color);
            (self.on_pick)(self.color);
            return true;
        }
        false
    }

    fn a11y(&self) -> A11yMetadata {
        A11yMetadata {
            role: A11yRole::Button,
            label: Some(format!("Brush color {}", self.label)),
            focusable: true,
            ..Default::default()
        }
    }
}

fn fill_preview_buffer(buf: &PixelBufferHandle, color: Color) {
    buf.with_pixels_mut(|w, h, px| {
        for y in 0..h {
            for x in 0..w {
                let idx = ((y * w + x) * 4) as usize;
                let checker = ((x / 8) + (y / 8)) % 2 == 0;
                let dist_center = ((x as f32 - w as f32 * 0.5).powi(2)
                    + (y as f32 - h as f32 * 0.5).powi(2))
                .sqrt();
                if dist_center < 22.0 {
                    px[idx] = color.r;
                    px[idx + 1] = color.g;
                    px[idx + 2] = color.b;
                    px[idx + 3] = 255;
                } else if checker {
                    px[idx] = 44;
                    px[idx + 1] = 52;
                    px[idx + 2] = 66;
                    px[idx + 3] = 255;
                } else {
                    px[idx] = 30;
                    px[idx + 1] = 36;
                    px[idx + 2] = 48;
                    px[idx + 3] = 255;
                }
            }
        }
    });
    buf.commit_damage(Rect::new(0.0, 0.0, 64.0, 64.0));
}

#[derive(Clone)]
pub struct CanvasAppState {
    pub toolbar: aot_ui::CanvasToolbar,
    pub active_color: Signal<Color>,
    pub strokes: Signal<Vec<Stroke>>,
    pub preview_tex: Signal<TextureHandle>,
    pub canvas_node: NodeId,
}

pub fn mount_canvas_app(
    cx: Scope,
    tree: &mut WidgetTree,
    textures: &mut TextureRegistry,
) -> (NodeId, CanvasAppState) {
    let active_color = cx.signal(PALETTE[0].1);
    let strokes = cx.signal(vec![Stroke {
        points: vec![
            Point::new(0.18, 0.25),
            Point::new(0.45, 0.60),
            Point::new(0.78, 0.30),
        ],
        color: PALETTE[0].1,
        width: 4.0,
    }]);

    let initial_buf = vec![0u8; 64 * 64 * 4];
    let tex_handle = textures.create_from_rgba(64, 64, initial_buf);
    fill_preview_buffer(&tex_handle.buffer, PALETTE[0].1);
    let preview_tex = cx.signal(tex_handle.clone());

    let toolbar = aot_ui::CanvasToolbar::new(cx);
    toolbar.set_on_clear(move || {
        strokes.update(|list| list.clear());
        toolbar.status_text.set("Canvas cleared".into());
    });

    let drawing = Rc::new(Cell::new(false));
    let theme_sig = tree.theme_signal;
    let status_sig = toolbar.status_text;
    let eraser_sig = toolbar.eraser_mode;
    let width_sig = toolbar.brush_width;

    let canvas_id = tree.canvas(
        cx,
        LayoutStyle::column()
            .with_size(pct(100), pct(100))
            .with_flex_grow(1.0),
        {
            move |rect, scene| {
                let th = theme_sig.get_untracked();
                scene.fill_rect(rect, th.surface, CornerRadii::all(12.0));
                scene.stroke_rect(rect, th.border, 1.0, CornerRadii::all(12.0));

                // Subtle reference grid
                let step = 32.0;
                let mut gx = rect.x + step;
                while gx < rect.max_x() {
                    let mut p = BezPath::new();
                    p.move_to((gx as f64, rect.y as f64));
                    p.line_to((gx as f64, rect.max_y() as f64));
                    scene.stroke_path(
                        p,
                        Color::rgba(th.border.r, th.border.g, th.border.b, 70),
                        1.0,
                    );
                    gx += step;
                }
                let mut gy = rect.y + step;
                while gy < rect.max_y() {
                    let mut p = BezPath::new();
                    p.move_to((rect.x as f64, gy as f64));
                    p.line_to((rect.max_x() as f64, gy as f64));
                    scene.stroke_path(
                        p,
                        Color::rgba(th.border.r, th.border.g, th.border.b, 70),
                        1.0,
                    );
                    gy += step;
                }

                // Render normalized strokes within canvas rect
                for st in strokes.get() {
                    if let Some(first) = st.points.first() {
                        let mut path = BezPath::new();
                        path.move_to((
                            (rect.x + first.x * rect.width) as f64,
                            (rect.y + first.y * rect.height) as f64,
                        ));
                        for pt in &st.points[1..] {
                            path.line_to((
                                (rect.x + pt.x * rect.width) as f64,
                                (rect.y + pt.y * rect.height) as f64,
                            ));
                        }
                        scene.stroke_path(path, st.color, st.width);
                    }
                }
            }
        },
        Some(Rc::new(RefCell::new(
            move |rect: Rect, pe: &PointerEvent| {
                let nx = ((pe.position.x - rect.x) / rect.width.max(1.0)).clamp(0.0, 1.0);
                let ny = ((pe.position.y - rect.y) / rect.height.max(1.0)).clamp(0.0, 1.0);
                let pt = Point::new(nx, ny);
                match pe.phase {
                    PointerPhase::Down => {
                        drawing.set(true);
                        let col = if eraser_sig.get_untracked() {
                            theme_sig.get_untracked().surface
                        } else {
                            active_color.get_untracked()
                        };
                        let w = width_sig.get_untracked();
                        strokes.update(|list| {
                            list.push(Stroke {
                                points: vec![pt],
                                color: col,
                                width: w,
                            });
                        });
                    }
                    PointerPhase::Move if drawing.get() => {
                        strokes.update(|list| {
                            if let Some(last) = list.last_mut() {
                                last.points.push(pt);
                            }
                        });
                    }
                    PointerPhase::Up | PointerPhase::Cancel if drawing.replace(false) => {
                        status_sig.set(format!("{} stroke(s)", strokes.get_untracked().len()));
                    }
                    _ => {}
                }
            },
        ))),
    );

    // Wire double-tap (clear canvas) & long-press (cycle brush thickness presets)
    tree.set_on_double_tap(canvas_id, move || {
        strokes.update(|list| list.clear());
        status_sig.set("Double-tap: canvas cleared".into());
    });
    tree.set_on_long_press(canvas_id, move || {
        let cur = width_sig.get_untracked();
        let next = if cur < 5.0 {
            8.0
        } else if cur < 10.0 {
            13.0
        } else {
            3.0
        };
        width_sig.set(next);
        status_sig.set(format!("Long-press: brush width {:.0}px", next));
    });

    let toolbar_node = toolbar.build(cx, tree);
    let root = tree.column(cx);
    tree.set_layout(
        root,
        LayoutStyle::column()
            .with_size(pct(100), pct(100))
            .with_padding(Insets::all(16.0))
            .with_gap(12.0),
    );
    tree.set_box_style(
        root,
        BoxStyle {
            background: Some(ReactiveValue::Dynamic(Rc::new(move || {
                Brush::Solid(theme_sig.get().background)
            }))),
            ..BoxStyle::new()
        },
    );
    tree.append_child(root, toolbar_node);

    let workspace_row = tree.row(cx);
    tree.set_layout(
        workspace_row,
        LayoutStyle::row()
            .with_size(pct(100), pct(100))
            .with_flex_grow(1.0)
            .with_gap(14.0),
    );

    let tex_for_pick = tex_handle;
    let status_for_pick = toolbar.status_text;
    let sidebar = cui! {
        Column(
            width: dp(156),
            height: pct(100),
            bg: theme.surface,
            border_width: 1,
            border_color: theme.border,
            radius: 12,
            padding: 12,
            gap: 10
        ) {
            Text(text: "Palette", size: 15, weight: 700.0, color: theme.text_primary)
            Custom(widget: SwatchButtonWidget::new("Blue", PALETTE[0].1, active_color, {
                let t = tex_for_pick.clone();
                let st = status_for_pick;
                move |c| { fill_preview_buffer(&t.buffer, c); st.set("Color: Blue".into()); }
            }))
            Custom(widget: SwatchButtonWidget::new("Coral", PALETTE[1].1, active_color, {
                let t = tex_for_pick.clone();
                let st = status_for_pick;
                move |c| { fill_preview_buffer(&t.buffer, c); st.set("Color: Coral".into()); }
            }))
            Custom(widget: SwatchButtonWidget::new("Gold", PALETTE[2].1, active_color, {
                let t = tex_for_pick.clone();
                let st = status_for_pick;
                move |c| { fill_preview_buffer(&t.buffer, c); st.set("Color: Gold".into()); }
            }))
            Custom(widget: SwatchButtonWidget::new("Teal", PALETTE[3].1, active_color, {
                let t = tex_for_pick.clone();
                let st = status_for_pick;
                move |c| { fill_preview_buffer(&t.buffer, c); st.set("Color: Teal".into()); }
            }))
            Custom(widget: SwatchButtonWidget::new("Slate", PALETTE[4].1, active_color, {
                let t = tex_for_pick.clone();
                let st = status_for_pick;
                move |c| { fill_preview_buffer(&t.buffer, c); st.set("Color: Slate".into()); }
            }))
            Divider(axis: Axis::Horizontal, thickness: 1, color: theme.border)
            Text(text: "Brush Preview", size: 13, color: theme.text_secondary)
            ImageView(source: preview_tex, width: dp(64), height: dp(64), radius: 8)
            Text(text: "Tips:\n• Drag to sketch\n• Long-press width\n• Double-tap clear", size: 12, color: theme.text_secondary)
        }
    }
    .build(cx, tree);

    tree.append_child(workspace_row, sidebar);
    tree.append_child(workspace_row, canvas_id);
    tree.append_child(root, workspace_row);

    let state = CanvasAppState {
        toolbar,
        active_color,
        strokes,
        preview_tex,
        canvas_node: canvas_id,
    };
    (root, state)
}

fn main() -> craftui::core::Result<()> {
    run_app(
        WindowConfig {
            title: "CraftUI Vector Canvas Paint".to_string(),
            width: 920.0,
            height: 640.0,
            ..Default::default()
        },
        |cx, tree, ctx| {
            let (root, _) = mount_canvas_app(cx, tree, &mut ctx.textures);
            root
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use craftui::reactive::reset_runtime;

    #[test]
    fn canvas_strokes_gestures_custom_swatches_pixel_buffer_and_render_work() {
        reset_runtime();
        let cx = Scope::root();
        let mut tree = WidgetTree::new(cx);
        let mut textures = TextureRegistry::new();
        let (root, state) = mount_canvas_app(cx, &mut tree, &mut textures);
        tree.set_root(root);

        let metrics = WindowMetrics {
            logical_size: Size::new(920.0, 640.0),
            ..Default::default()
        };
        tree.layout(metrics);
        assert_eq!(state.strokes.get().len(), 1);

        // Pick second swatch ("Coral")
        let coral_swatch = tree
            .nodes
            .iter()
            .filter_map(|(id, n)| (n.debug_name == "SwatchButtonWidget").then_some(id))
            .nth(1)
            .expect("second swatch missing");
        let sc = tree.rect(coral_swatch).center();
        for (phase, ts) in [(PointerPhase::Down, 10), (PointerPhase::Up, 30)] {
            tree.handle_event(&UiEvent::Pointer(PointerEvent {
                id: PointerId(1),
                phase,
                position: sc,
                button: Some(PointerButton::Primary),
                modifiers: Modifiers::default(),
                is_touch: false,
                timestamp_ms: ts,
            }));
        }
        assert_eq!(state.active_color.get(), PALETTE[1].1);

        // Draw a stroke on the Canvas
        let cr = tree.rect(state.canvas_node);
        for (phase, pos, ts) in [
            (
                PointerPhase::Down,
                Point::new(cr.x + 40.0, cr.y + 40.0),
                100,
            ),
            (
                PointerPhase::Move,
                Point::new(cr.x + 140.0, cr.y + 120.0),
                116,
            ),
            (PointerPhase::Up, Point::new(cr.x + 220.0, cr.y + 90.0), 132),
        ] {
            tree.handle_event(&UiEvent::Pointer(PointerEvent {
                id: PointerId(1),
                phase,
                position: pos,
                button: Some(PointerButton::Primary),
                modifiers: Modifiers::default(),
                is_touch: false,
                timestamp_ms: ts,
            }));
        }
        assert_eq!(state.strokes.get().len(), 2);

        // Long-press cycles brush width preset
        tree.handle_event(&UiEvent::Pointer(PointerEvent {
            id: PointerId(1),
            phase: PointerPhase::Down,
            position: cr.center(),
            button: Some(PointerButton::Primary),
            modifiers: Modifiers::default(),
            is_touch: true,
            timestamp_ms: 500,
        }));
        tree.handle_event(&UiEvent::Pointer(PointerEvent {
            id: PointerId(1),
            phase: PointerPhase::Up,
            position: cr.center(),
            button: Some(PointerButton::Primary),
            modifiers: Modifiers::default(),
            is_touch: true,
            timestamp_ms: 1100,
        }));
        assert!(state.toolbar.brush_width.get() > 7.5);

        // Render via VelloCpu & confirm non-trivial canvas + swatch + pixel buffer output
        tree.layout(metrics);
        let mut scene = PaintScene::new();
        tree.paint(&mut scene);
        let rgba = render_scene_to_rgba(
            BackendKind::VelloCpu,
            &scene,
            920,
            640,
            1.0,
            tree.theme().background,
            &textures,
            &mut tree.text_engine,
        );
        assert_eq!(rgba.len(), 920 * 640 * 4);
        assert!(rgba.iter().any(|&b| b > 200));

        // Double-tap clears all strokes
        for (phase, ts) in [
            (PointerPhase::Down, 2000),
            (PointerPhase::Up, 2020),
            (PointerPhase::Down, 2120),
            (PointerPhase::Up, 2140),
        ] {
            tree.handle_event(&UiEvent::Pointer(PointerEvent {
                id: PointerId(1),
                phase,
                position: cr.center(),
                button: Some(PointerButton::Primary),
                modifiers: Modifiers::default(),
                is_touch: false,
                timestamp_ms: ts,
            }));
        }
        assert!(state.strokes.get().is_empty());
    }
}
