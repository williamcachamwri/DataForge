//! Windows platform integration for DataForge.

pub mod browser;
pub mod error;
pub mod machine_id;
pub mod paths;
pub mod theme_detect;

pub use browser::{WindowsBrowserLauncher, validate_http_url};
pub use error::PlatformError;
pub use machine_id::{derive_machine_id, normalize_machine_guid, read_machine_id};
pub use paths::{APP_DIR_NAME, storage_paths, storage_paths_for};
pub use theme_detect::{
    WindowsTheme, apps_use_light_theme, detect_theme, detect_theme_or_default, is_high_contrast,
    theme_from_flags,
};
