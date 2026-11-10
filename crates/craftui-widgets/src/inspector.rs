use craftui_core::Color;
use craftui_render::PaintScene;
use craftui_text::TextStyle;

/// Frame performance & layout debugging overlay toggled via `Ctrl+Shift+I` (PRD FR-9.2).
#[derive(Debug, Clone, Default)]
pub struct InspectorState {
    pub enabled: bool,
    pub last_frame_ms: f32,
    pub last_layout_ms: f32,
    pub fps: f32,
    pub node_count: usize,
    pub dirty_count: usize,
    pub hovered_node: Option<(String, craftui_core::Rect)>,
}

impl InspectorState {
    pub fn record_frame(
        &mut self,
        frame_ms: f32,
        layout_ms: f32,
        node_count: usize,
        dirty_count: usize,
    ) {
        self.last_frame_ms = frame_ms;
        self.last_layout_ms = layout_ms;
        self.fps = if frame_ms > 0.001 {
            (1000.0 / frame_ms).min(240.0)
        } else {
            120.0
        };
        self.node_count = node_count;
        self.dirty_count = dirty_count;
    }

    pub fn paint_overlay(
        &self,
        scene: &mut PaintScene,
        bounds_list: &[(craftui_core::Rect, &str)],
        viewport_w: f32,
    ) {
        if !self.enabled {
            return;
        }
        for (rect, _) in bounds_list {
            scene.stroke_rect(
                *rect,
                Color::rgba(0, 200, 255, 110),
                1.0,
                craftui_core::CornerRadii::ZERO,
            );
        }
        if let Some((ref label, rect)) = self.hovered_node {
            scene.stroke_rect(
                rect,
                Color::rgb(255, 180, 0),
                2.0,
                craftui_core::CornerRadii::ZERO,
            );
            scene.fill_rect(
                craftui_core::Rect::new(rect.x, (rect.y - 22.0).max(0.0), 210.0, 20.0),
                Color::rgba(20, 24, 32, 225),
                craftui_core::CornerRadii::all(4.0),
            );
            scene.draw_text(
                craftui_core::Point::new(rect.x + 6.0, (rect.y - 20.0).max(2.0)),
                format!("{label} ({:.0}x{:.0})", rect.width, rect.height),
                TextStyle::default()
                    .with_size(11.0)
                    .with_color(Color::rgb(255, 210, 90)),
                Some(200.0),
            );
        }

        let hud = craftui_core::Rect::new((viewport_w - 250.0).max(8.0), 8.0, 242.0, 62.0);
        scene.fill_rect(
            hud,
            Color::rgba(14, 18, 26, 220),
            craftui_core::CornerRadii::all(8.0),
        );
        scene.stroke_rect(
            hud,
            Color::rgb(59, 130, 246),
            1.0,
            craftui_core::CornerRadii::all(8.0),
        );
        scene.draw_text(
            craftui_core::Point::new(hud.x + 10.0, hud.y + 8.0),
            format!(
                "CraftUI Inspector (Ctrl+Shift+I)\n{:.1} FPS ({:.2}ms | layout {:.2}ms)\nNodes: {} | Dirty: {}",
                self.fps, self.last_frame_ms, self.last_layout_ms, self.node_count, self.dirty_count
            ),
            TextStyle::default()
                .with_size(11.0)
                .with_color(Color::rgb(225, 235, 245)),
            Some(224.0),
        );
    }
}
