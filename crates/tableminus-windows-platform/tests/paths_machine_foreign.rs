use std::path::Path;

use tableminus_windows_platform::{
    ForeignHost, ForeignPlatform, PlainHost, derive_machine_id, foreign_app_dirs,
    foreign_app_dirs_for, normalize_machine_guid, read_machine_id, storage_paths,
    storage_paths_for,
};

type TestResult = Result<(), Box<dyn std::error::Error>>;

#[test]
fn storage_paths_resolve_under_local_and_roaming_appdata() -> TestResult {
    let paths = storage_paths()?;

    assert!(
        paths
            .config
            .ends_with(Path::new("TableMinus").join("config"))
    );
    assert!(paths.state.ends_with(Path::new("TableMinus").join("state")));
    assert!(paths.data.ends_with(Path::new("TableMinus").join("data")));
    assert!(paths.cache.ends_with(Path::new("TableMinus").join("cache")));
    assert!(
        paths
            .runtime
            .ends_with(Path::new("TableMinus").join("runtime"))
    );

    // Config/state live under RoamingAppData; data/cache/runtime under LocalAppData.
    assert_eq!(paths.config.parent(), paths.state.parent());
    assert_eq!(paths.data.parent(), paths.cache.parent());
    assert_eq!(paths.data.parent(), paths.runtime.parent());
    Ok(())
}

#[test]
fn storage_paths_for_uses_custom_app_dir_name() -> TestResult {
    let paths = storage_paths_for("TableMinusDev")?;
    assert!(
        paths
            .config
            .ends_with(Path::new("TableMinusDev").join("config"))
    );
    assert!(paths.data.ends_with(Path::new("TableMinusDev").join("data")));
    Ok(())
}

#[test]
fn read_machine_id_and_derive_succeed_on_windows() -> TestResult {
    let raw = read_machine_id()?;
    assert_eq!(raw.len(), 32);
    assert!(
        raw.bytes()
            .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
    );
    assert!(!raw.bytes().all(|byte| byte == b'0'));

    let derived = derive_machine_id()?;
    assert_eq!(derived.as_str().len(), 64);
    assert!(!derived.as_str().contains(&raw));
    Ok(())
}

#[test]
fn normalize_machine_guid_strips_dashes_braces_and_lowercases() -> TestResult {
    let normalized = normalize_machine_guid("{4C4F5E2A-0B6D-4F3C-9E8A-7B6C5D4E3F20}\0")?;
    assert_eq!(normalized, "4c4f5e2a0b6d4f3c9e8a7b6c5d4e3f20");

    assert!(normalize_machine_guid("00000000-0000-0000-0000-000000000000").is_err());
    assert!(normalize_machine_guid("not-a-guid").is_err());
    assert!(normalize_machine_guid("4c4f5e2a").is_err());
    Ok(())
}

#[test]
fn foreign_app_dirs_and_plain_host_match_windows_conventions() -> TestResult {
    let home = Path::new(r"C:\Users\Alice");
    let dirs = foreign_app_dirs_for(home);
    assert_eq!(dirs.platform, ForeignPlatform::Windows);
    assert_eq!(dirs.config, home.join("AppData").join("Roaming"));
    assert_eq!(dirs.data, home.join("AppData").join("Local"));

    let current = foreign_app_dirs()?;
    assert_eq!(current.platform, ForeignPlatform::Windows);
    assert!(
        current
            .config
            .ends_with(Path::new("AppData").join("Roaming"))
    );
    assert!(current.data.ends_with(Path::new("AppData").join("Local")));

    let host: &dyn ForeignHost = &PlainHost;
    let _ = host;
    Ok(())
}
