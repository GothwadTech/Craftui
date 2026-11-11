use craftui::prelude::*;
use craftui_example_scancraft::mount_scancraft_app_with_services;

fn main() -> craftui::core::Result<()> {
    run_app(
        WindowConfig {
            title: "ScanCraft Pro — CraftUI Dogfood Scanner".to_string(),
            width: 1024.0,
            height: 720.0,
            ..Default::default()
        },
        |cx, tree, _ctx| {
            let services = PlatformServices::new(DesktopClipboard::default(), DesktopFileDialog);
            let (root, _) = mount_scancraft_app_with_services(cx, tree, services);
            root
        },
    )
}
