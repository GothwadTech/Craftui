use std::sync::Arc;

use craftui::prelude::*;
use craftui_example_whiteboard::mount_whiteboard_app;

fn main() -> craftui::core::Result<()> {
    run_app(
        WindowConfig {
            title: "CraftUI Whiteboard (Excalidraw Multiplatform)".to_string(),
            width: 1200.0,
            height: 800.0,
            ..Default::default()
        },
        |cx, tree, app_context| {
            let services = Arc::new(app_context.services.clone());
            mount_whiteboard_app(cx, tree, Some(services))
        },
    )
}
