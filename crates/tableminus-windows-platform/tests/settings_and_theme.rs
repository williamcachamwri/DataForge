use std::collections::BTreeMap;

use tableminus_windows_platform::{
    AppSettings, EditorFont, EnvironmentKind, FontSpec, McpSettings, ThemePreference,
    WindowControlsStyle, WindowGeometry, WindowsTheme, apps_use_light_theme, detect_reduced_motion,
    detect_theme, detect_theme_or_default, is_high_contrast, theme_from_flags,
};
use tempfile::TempDir;

type TestResult = Result<(), Box<dyn std::error::Error>>;

#[test]
fn settings_load_returns_default_when_missing() -> TestResult {
    let dir = TempDir::new()?;
    let path = dir.path().join("nested").join("settings.json");

    let loaded = AppSettings::load(&path)?;
    assert_eq!(loaded, AppSettings::default());
    Ok(())
}

#[test]
fn settings_save_atomic_roundtrips_and_leaves_no_temp_files() -> TestResult {
    let dir = TempDir::new()?;
    let path = dir.path().join("config").join("settings.json");

    let custom = AppSettings {
        theme: ThemePreference::Dark,
        window_backdrop: tableminus_windows_platform::WindowBackdropPreference::GaussianBlur,
        window_controls: WindowControlsStyle::Windows,
        content_theme_id: Some("builtin.tokyo_night".to_owned()),
        syntax_palette_id: Some("builtin.dracula".to_owned()),
        editor_font: EditorFont {
            family: "JetBrains Mono".to_owned(),
            size: 14.5,
            line_height: 1.5,
            ligatures: false,
        },
        value_font: FontSpec {
            family: "Segoe UI".to_owned(),
            size: 12.5,
            line_height: 1.4,
            ligatures: false,
        },
        window: WindowGeometry {
            x: -120,
            y: 40,
            width: 1920,
            height: 1080,
            maximized: true,
        },
        mcp: McpSettings {
            enabled: true,
            port: 49_123,
        },
        restore_session: false,
        auto_check_updates: false,
        locale: Some("vi-VN".to_owned()),
        environment_tags: BTreeMap::from([("prod-db".to_owned(), EnvironmentKind::Production)]),
        restore_last_session: false,
    };

    custom.save_atomic(&path)?;
    let loaded = AppSettings::load(&path)?;
    assert_eq!(loaded, custom);

    // Overwrite atomically and confirm no `.tmp` files remain.
    let updated = AppSettings {
        theme: ThemePreference::HighContrast,
        ..custom
    };
    updated.save_atomic(&path)?;
    assert_eq!(AppSettings::load(&path)?, updated);

    let config_dir = path
        .parent()
        .ok_or_else(|| std::io::Error::other("missing parent dir"))?;
    let entries: Vec<_> = std::fs::read_dir(config_dir)?
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(entries, vec!["settings.json".to_owned()]);
    Ok(())
}

#[test]
fn settings_load_backcompat_defaults_missing_value_font() -> TestResult {
    let dir = TempDir::new()?;
    let path = dir.path().join("legacy_settings.json");
    let legacy_json = r#"{
        "theme": "dark",
        "editor_font": { "family": "Consolas", "size": 12.0, "line_height": 1.3, "ligatures": false },
        "restore_session": false
    }"#;
    std::fs::write(&path, legacy_json)?;

    let loaded = AppSettings::load(&path)?;
    assert_eq!(loaded.theme, ThemePreference::Dark);
    assert_eq!(loaded.window_controls, WindowControlsStyle::MacOs);
    assert_eq!(loaded.editor_font.family, "Consolas");
    assert_eq!(loaded.value_font, FontSpec::default());
    assert!(!loaded.restore_session);
    assert!(loaded.auto_check_updates);
    assert!(loaded.restore_last_session);
    assert!(loaded.environment_tags.is_empty());
    Ok(())
}

#[test]
fn theme_detection_succeeds_and_flags_map_deterministically() -> TestResult {
    let _high_contrast = is_high_contrast()?;
    let _apps_light = apps_use_light_theme()?;
    let _reduced_motion = detect_reduced_motion();
    let detected = detect_theme()?;
    assert_eq!(detect_theme_or_default(), detected);

    assert_eq!(
        theme_from_flags(true, Some(true)),
        WindowsTheme::HighContrast
    );
    assert_eq!(
        theme_from_flags(true, Some(false)),
        WindowsTheme::HighContrast
    );
    assert_eq!(theme_from_flags(false, Some(true)), WindowsTheme::Light);
    assert_eq!(theme_from_flags(false, Some(false)), WindowsTheme::Dark);
    assert_eq!(theme_from_flags(false, None), WindowsTheme::Light);
    assert!(WindowsTheme::Dark.is_dark());
    assert!(WindowsTheme::HighContrast.is_high_contrast());
    Ok(())
}

#[test]
fn settings_window_controls_serialization_and_backcompat() -> TestResult {
    let dir = TempDir::new()?;
    let path = dir.path().join("settings.json");

    // Default is MacOs and serializes as "mac_os".
    let default_settings = AppSettings::default();
    assert_eq!(default_settings.window_controls, WindowControlsStyle::MacOs);
    default_settings.save_atomic(&path)?;
    let raw_default = std::fs::read_to_string(&path)?;
    assert!(raw_default.contains("\"window_controls\": \"mac_os\""));

    // Windows variant serializes as "windows" and round-trips.
    let windows_settings = AppSettings {
        window_controls: WindowControlsStyle::Windows,
        ..AppSettings::default()
    };
    windows_settings.save_atomic(&path)?;
    let raw_windows = std::fs::read_to_string(&path)?;
    assert!(raw_windows.contains("\"window_controls\": \"windows\""));
    assert_eq!(
        AppSettings::load(&path)?.window_controls,
        WindowControlsStyle::Windows
    );

    // Explicit "mac_os" deserializes cleanly.
    std::fs::write(&path, r#"{ "window_controls": "mac_os" }"#)?;
    assert_eq!(
        AppSettings::load(&path)?.window_controls,
        WindowControlsStyle::MacOs
    );

    // Legacy settings without `window_controls` default to MacOs.
    std::fs::write(&path, r#"{ "theme": "system", "restore_session": true }"#)?;
    assert_eq!(
        AppSettings::load(&path)?.window_controls,
        WindowControlsStyle::MacOs
    );
    Ok(())
}
