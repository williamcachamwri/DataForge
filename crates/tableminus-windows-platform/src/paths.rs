//! Windows known-folder resolution for [`StoragePaths`].

use std::path::PathBuf;

use tablepro_storage::StoragePaths;
use windows::Win32::System::Com::CoTaskMemFree;
use windows::Win32::UI::Shell::{
    FOLDERID_LocalAppData, FOLDERID_RoamingAppData, KF_FLAG_DEFAULT, SHGetKnownFolderPath,
};
use windows::core::{GUID, PWSTR};

use crate::error::PlatformError;

/// Default application directory name under `%LOCALAPPDATA%` and `%APPDATA%`.
pub const APP_DIR_NAME: &str = "TableMinus";

/// RAII guard ensuring a `PWSTR` allocated by `SHGetKnownFolderPath` is freed via `CoTaskMemFree`.
struct CoTaskMemPwstr(PWSTR);

impl Drop for CoTaskMemPwstr {
    fn drop(&mut self) {
        if !self.0.is_null() {
            // SAFETY: `self.0` was allocated by `SHGetKnownFolderPath` and is freed exactly once.
            unsafe { CoTaskMemFree(Some(self.0.as_ptr().cast())) };
        }
    }
}

/// Resolves a Windows known folder GUID via `SHGetKnownFolderPath`.
pub(crate) fn known_folder(
    folder_id: &GUID,
    operation: &'static str,
) -> Result<PathBuf, PlatformError> {
    // SAFETY: `folder_id` is a valid static `KNOWNFOLDERID` reference; `SHGetKnownFolderPath`
    // allocates a NUL-terminated UTF-16 string freed by `CoTaskMemPwstr::drop`.
    let raw = unsafe { SHGetKnownFolderPath(folder_id, KF_FLAG_DEFAULT, None) }.map_err(|err| {
        PlatformError::Win32 {
            operation,
            source: std::io::Error::from_raw_os_error(err.code().0),
        }
    })?;
    let guard = CoTaskMemPwstr(raw);
    // SAFETY: `guard.0` is a valid NUL-terminated UTF-16 string returned by `SHGetKnownFolderPath`.
    let text = unsafe { guard.0.to_string() }.map_err(|err| PlatformError::Win32 {
        operation,
        source: std::io::Error::new(std::io::ErrorKind::InvalidData, err.to_string()),
    })?;
    Ok(PathBuf::from(text))
}

/// Returns [`StoragePaths`] resolved from Windows Known Folders for TableMinus.
///
/// - `FOLDERID_LocalAppData` (`%LOCALAPPDATA%\TableMinus`) -> `data`, `cache`, `runtime`
/// - `FOLDERID_RoamingAppData` (`%APPDATA%\TableMinus`) -> `config`, `state`
pub fn storage_paths() -> Result<StoragePaths, PlatformError> {
    storage_paths_for(APP_DIR_NAME)
}

/// Returns [`StoragePaths`] under a custom application directory name.
pub fn storage_paths_for(app_dir_name: &str) -> Result<StoragePaths, PlatformError> {
    let local_root = known_folder(&FOLDERID_LocalAppData, "SHGetKnownFolderPath(LocalAppData)")?
        .join(app_dir_name);
    let roaming_root = known_folder(
        &FOLDERID_RoamingAppData,
        "SHGetKnownFolderPath(RoamingAppData)",
    )?
    .join(app_dir_name);

    Ok(StoragePaths {
        config: roaming_root.join("config"),
        state: roaming_root.join("state"),
        data: local_root.join("data"),
        cache: local_root.join("cache"),
        runtime: local_root.join("runtime"),
    })
}
