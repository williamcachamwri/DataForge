use dataforge_windows_platform::{
    AppSettings, EditorFont, FontSpec, McpSettings, ThemePreference, WindowGeometry, WindowsTheme,
    apps_use_light_theme, detect_reduced_motion, detect_theme, detect_theme_or_default,
    is_high_contrast, theme_from_flags,
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
        window_backdrop: dataforge_windows_platform::WindowBackdropPreference::GaussianBlur,
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
    assert_eq!(loaded.editor_font.family, "Consolas");
    assert_eq!(loaded.value_font, FontSpec::default());
    assert!(!loaded.restore_session);
    assert!(loaded.auto_check_updates);
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
