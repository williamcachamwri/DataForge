//! Foreign-app import directory resolution and host re-exports for Windows.

use std::path::Path;

pub use tablepro_foreign_import::{ForeignAppDirs, ForeignHost, ForeignPlatform, PlainHost};
use windows::Win32::UI::Shell::FOLDERID_Profile;

use crate::error::PlatformError;
use crate::paths::known_folder;

/// Returns [`ForeignAppDirs`] for the current Windows user's profile directory.
pub fn foreign_app_dirs() -> Result<ForeignAppDirs, PlatformError> {
    let home = known_folder(&FOLDERID_Profile, "SHGetKnownFolderPath(Profile)")?;
    Ok(foreign_app_dirs_for(&home))
}

/// Returns [`ForeignAppDirs`] for Windows under the supplied `home` directory.
pub fn foreign_app_dirs_for(home: &Path) -> ForeignAppDirs {
    ForeignAppDirs::conventional(ForeignPlatform::Windows, home)
}
