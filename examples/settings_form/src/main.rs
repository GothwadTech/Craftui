use craftui::prelude::*;

pub mod aot_ui {
    include!(concat!(env!("OUT_DIR"), "/settings.rs"));
}

use aot_ui::SettingsScreen;

const THEMES: [ThemeMode; 4] = [
    ThemeMode::Dark,
    ThemeMode::Light,
    ThemeMode::Amoled,
    ThemeMode::HighContrast,
];

pub fn format_profile_json(screen: &SettingsScreen) -> String {
    let density = match screen.density_idx.get_untracked() {
        0 => "compact",
        1 => "comfortable",
        _ => "spacious",
    };
    let locale = match screen.locale_idx.get_untracked() {
        0 => "en-US",
        1 => "ja-JP",
        2 => "es-ES",
        _ => "de-DE",
    };
    format!(
        "{{\n  \"displayName\": {:?},\n  \"bio\": {:?},\n  \"desktopAlerts\": {},\n  \"autoSave\": {},\n  \"density\": {:?},\n  \"locale\": {:?},\n  \"uiScale\": {:.2}\n}}\n",
        screen.display_name.get_untracked(),
        screen.role_bio.get_untracked(),
        screen.desktop_alerts.get_untracked(),
        screen.auto_save.get_untracked(),
        density,
        locale,
        screen.ui_scale.get_untracked(),
    )
}

pub fn mount_settings_app(cx: Scope, tree: &mut WidgetTree) -> (NodeId, SettingsScreen) {
    let screen = SettingsScreen::new(cx);
    let theme_idx = cx.signal(0usize);
    let theme_sig = tree.theme_signal;
    let services_copy = tree.services.clone();
    let services_import = tree.services.clone();
    let services_export = tree.services.clone();

    let s_theme = screen.clone();
    screen.set_on_cycle_theme(move || {
        let next = (theme_idx.get_untracked() + 1) % THEMES.len();
        theme_idx.set(next);
        theme_sig.set(ThemeTokens::from_mode(THEMES[next]));
        s_theme
            .status_line
            .set(format!("Theme switched to {:?}", THEMES[next]));
    });

    let s_copy = screen.clone();
    screen.set_on_copy_profile(move || {
        let json = format_profile_json(&s_copy);
        if services_copy.write_clipboard(&json) {
            s_copy
                .status_line
                .set("Copied settings profile JSON to clipboard".into());
        } else {
            s_copy.status_line.set("Clipboard unavailable".into());
        }
    });

    let s_import = screen.clone();
    screen.set_on_import_profile(move || {
        let req = FileDialogRequest::open("Import Settings Profile")
            .with_filter(FileFilter::new("JSON", ["json"]));
        if let Some(path) = services_import.pick_open_file(&req) {
            s_import
                .status_line
                .set(format!("Selected import path: {}", path.display()));
        } else {
            s_import.status_line.set("Import cancelled".into());
        }
    });

    let s_export = screen.clone();
    screen.set_on_export_profile(move || {
        let req = FileDialogRequest::save("Export Settings Profile", "craftui-settings.json")
            .with_filter(FileFilter::new("JSON", ["json"]));
        if let Some(path) = services_export.pick_save_file(&req) {
            s_export
                .status_line
                .set(format!("Saved profile target: {}", path.display()));
        } else {
            s_export.status_line.set("Export cancelled".into());
        }
    });

    let root = screen.build(cx, tree);
    (root, screen)
}

fn main() -> craftui::core::Result<()> {
    run_app(
        WindowConfig {
            title: "CraftUI Settings Form".to_string(),
            width: 820.0,
            height: 680.0,
            ..Default::default()
        },
        |cx, tree, _| {
            let (root, _) = mount_settings_app(cx, tree);
            root
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use craftui::core::{MemoryClipboard, MockFileDialog};
    use craftui::reactive::reset_runtime;
    use std::path::PathBuf;

    #[test]
    fn settings_form_validation_clipboard_file_picker_tab_and_theme_work() {
        reset_runtime();
        let cx = Scope::root();
        let mut tree = WidgetTree::new(cx);
        let services = PlatformServices::new(
            MemoryClipboard::default(),
            MockFileDialog::new(
                vec![PathBuf::from("/tmp/imported-settings.json")],
                Some(PathBuf::from("/tmp/exported-settings.json")),
            ),
        );
        tree.set_platform_services(services.clone());

        let (root, screen) = mount_settings_app(cx, &mut tree);
        tree.set_root(root);
        let metrics = WindowMetrics {
            logical_size: Size::new(820.0, 680.0),
            ..Default::default()
        };
        tree.layout(metrics);

        // Verify Tab focus traversal moves focus between inputs cleanly
        tree.handle_event(&UiEvent::Key(KeyEvent {
            code: KeyCode::Tab,
            text: None,
            pressed: true,
            repeat: false,
            modifiers: Modifiers::default(),
        }));
        assert!(tree.focused.is_some());

        // Copy JSON profile to clipboard
        (screen.on_copy_profile.borrow_mut())();
        let clip = services.read_clipboard().expect("clipboard text missing");
        assert!(clip.contains("\"displayName\": \"Ada Lovelace\""));

        // Import & Export via MockFileDialog
        (screen.on_import_profile.borrow_mut())();
        assert!(
            screen
                .status_line
                .get()
                .contains("/tmp/imported-settings.json")
        );
        (screen.on_export_profile.borrow_mut())();
        assert!(
            screen
                .status_line
                .get()
                .contains("/tmp/exported-settings.json")
        );

        // Clearing display_name shows conditional validation banner & renders visually distinct frame
        let mut s_ok = PaintScene::new();
        tree.paint(&mut s_ok);
        let textures = TextureRegistry::new();
        let rgba_ok = render_scene_to_rgba(
            BackendKind::VelloCpu,
            &s_ok,
            820,
            680,
            1.0,
            tree.theme().background,
            &textures,
            &mut tree.text_engine,
        );

        screen.display_name.set(String::new());
        tree.layout(metrics);
        let mut s_err = PaintScene::new();
        tree.paint(&mut s_err);
        let rgba_err = render_scene_to_rgba(
            BackendKind::VelloCpu,
            &s_err,
            820,
            680,
            1.0,
            tree.theme().background,
            &textures,
            &mut tree.text_engine,
        );
        assert_ne!(rgba_ok, rgba_err);

        // Theme cycling updates live pixels
        (screen.on_cycle_theme.borrow_mut())();
        assert_eq!(tree.theme().mode, ThemeMode::Light);
    }
}
