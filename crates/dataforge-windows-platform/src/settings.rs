//! Application settings persistence with atomic write-and-rename.

use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tablepro_storage::StoragePaths;
use uuid::Uuid;

use crate::error::PlatformError;

/// Default filename for DataForge application settings inside [`StoragePaths::config`].
pub const SETTINGS_FILENAME: &str = "settings.json";

/// User-selected theme preference.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ThemePreference {
    /// Follow the Windows system Light/Dark/High-Contrast setting.
    #[default]
    System,
    /// Force Light mode.
    Light,
    /// Force Dark mode.
    Dark,
    /// Force High-Contrast mode.
    HighContrast,
}

/// SQL/code editor font configuration.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct EditorFont {
    /// Font family name.
    pub family: String,
    /// Font size in logical points.
    pub size: f32,
    /// Line height multiplier.
    pub line_height: f32,
    /// Whether programming ligatures are enabled.
    pub ligatures: bool,
}

impl Default for EditorFont {
    fn default() -> Self {
        Self {
            family: "Cascadia Mono".to_owned(),
            size: 13.0,
            line_height: 1.4,
            ligatures: true,
        }
    }
}

/// Persisted main window position and dimensions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct WindowGeometry {
    /// Window left edge in virtual screen coordinates.
    pub x: i32,
    /// Window top edge in virtual screen coordinates.
    pub y: i32,
    /// Window width in physical/logical pixels.
    pub width: u32,
    /// Window height in physical/logical pixels.
    pub height: u32,
    /// Whether the window was maximized when last closed.
    pub maximized: bool,
}

impl Default for WindowGeometry {
    fn default() -> Self {
        Self {
            x: 100,
            y: 100,
            width: 1280,
            height: 800,
            maximized: false,
        }
    }
}

/// Embedded MCP server settings read by the GUI and CLI host.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct McpSettings {
    /// Whether the local MCP HTTP server is enabled.
    pub enabled: bool,
    /// Loopback TCP port for the MCP server.
    pub port: u16,
}

impl Default for McpSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            port: 45_871,
        }
    }
}

/// Top-level persisted application settings for DataForge on Windows.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AppSettings {
    /// Appearance theme preference.
    pub theme: ThemePreference,
    /// SQL editor font configuration.
    pub editor_font: EditorFont,
    /// Main window geometry.
    pub window: WindowGeometry,
    /// Local MCP server configuration.
    pub mcp: McpSettings,
    /// Whether to restore open tabs on startup.
    pub restore_session: bool,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            theme: ThemePreference::default(),
            editor_font: EditorFont::default(),
            window: WindowGeometry::default(),
            mcp: McpSettings::default(),
            restore_session: true,
        }
    }
}

impl AppSettings {
    /// Returns the canonical settings file path for `paths`.
    pub fn file_path(paths: &StoragePaths) -> PathBuf {
        paths.config.join(SETTINGS_FILENAME)
    }

    /// Loads settings from `path`, returning [`AppSettings::default()`] if the file does not exist.
    pub fn load(path: &Path) -> Result<Self, PlatformError> {
        let bytes = match std::fs::read(path) {
            Ok(bytes) => bytes,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(err) => {
                return Err(PlatformError::Io {
                    operation: "read settings file",
                    source: err,
                });
            }
        };
        serde_json::from_slice(&bytes).map_err(|err| PlatformError::Json {
            context: "AppSettings",
            source: err,
        })
    }

    /// Atomically writes settings to `path` via a sibling temporary file and rename.
    pub fn save_atomic(&self, path: &Path) -> Result<(), PlatformError> {
        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
        {
            std::fs::create_dir_all(parent).map_err(|err| PlatformError::Io {
                operation: "create settings directory",
                source: err,
            })?;
        }

        let payload = serde_json::to_vec_pretty(self).map_err(|err| PlatformError::Json {
            context: "AppSettings",
            source: err,
        })?;

        let temp_path = sibling_temp_path(path);
        let write_result = write_and_sync(&temp_path, &payload);
        if let Err(err) = write_result {
            let _ = std::fs::remove_file(&temp_path);
            return Err(err);
        }

        if let Err(err) = std::fs::rename(&temp_path, path) {
            let _ = std::fs::remove_file(&temp_path);
            return Err(PlatformError::Io {
                operation: "rename settings temp file",
                source: err,
            });
        }
        Ok(())
    }
}

fn sibling_temp_path(target: &Path) -> PathBuf {
    let file_name = target
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(SETTINGS_FILENAME);
    let temp_name = format!(".{file_name}.{}.tmp", Uuid::new_v4());
    target.with_file_name(temp_name)
}

fn write_and_sync(temp_path: &Path, payload: &[u8]) -> Result<(), PlatformError> {
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(temp_path)
        .map_err(|err| PlatformError::Io {
            operation: "create settings temp file",
            source: err,
        })?;
    file.write_all(payload).map_err(|err| PlatformError::Io {
        operation: "write settings temp file",
        source: err,
    })?;
    file.sync_all().map_err(|err| PlatformError::Io {
        operation: "sync settings temp file",
        source: err,
    })?;
    Ok(())
}
