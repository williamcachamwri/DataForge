//! Windows platform integration for DataForge.

pub mod browser;
pub mod cli_host;
pub(crate) mod credential_parts;
pub mod error;
pub mod foreign;
pub mod machine_id;
pub mod paths;
pub mod secrets;
pub mod settings;
pub mod single_instance;
pub mod theme_detect;

pub use browser::{WindowsBrowserLauncher, validate_http_url};
pub use cli_host::WindowsCliHost;
pub use error::PlatformError;
pub use foreign::{
    ForeignAppDirs, ForeignHost, ForeignPlatform, PlainHost, foreign_app_dirs, foreign_app_dirs_for,
};
pub use machine_id::{derive_machine_id, normalize_machine_guid, read_machine_id};
pub use paths::{APP_DIR_NAME, storage_paths, storage_paths_for};
pub use secrets::{DEFAULT_CREDENTIAL_PREFIX, LAYOUT_KEYWORD, PART_SIZE, WindowsCredentialStore};
pub use settings::{
    AppSettings, EditorFont, FontSpec, McpSettings, SETTINGS_FILENAME, ThemePreference,
    WindowBackdropPreference, WindowGeometry,
};
pub use single_instance::{
    DEFAULT_MUTEX_NAME, DEFAULT_PIPE_NAME, ForwardPayload, NamedPipeServer, SingleInstanceLock,
    is_forwardable_target, send_to_existing_instance,
};
pub use theme_detect::{
    WindowsTheme, apps_use_light_theme, detect_reduced_motion, detect_theme,
    detect_theme_or_default, is_high_contrast, theme_from_flags,
};
