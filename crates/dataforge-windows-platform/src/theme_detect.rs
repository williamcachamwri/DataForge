//! Windows Light, Dark, and High-Contrast theme detection.

use windows::Win32::Foundation::{ERROR_SUCCESS, WIN32_ERROR};
use windows::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, KEY_READ, REG_DWORD, REG_VALUE_TYPE, RegOpenKeyExW, RegQueryValueExW,
};
use windows::Win32::UI::Accessibility::{HCF_HIGHCONTRASTON, HIGHCONTRASTW};
use windows::Win32::UI::WindowsAndMessaging::{
    SPI_GETHIGHCONTRAST, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS, SystemParametersInfoW,
};
use windows::core::PCWSTR;

use crate::error::{PlatformError, last_win32_error, to_wide_null};
use crate::machine_id::RegistryKey;

const PERSONALIZE_SUBKEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize";
const APPS_USE_LIGHT_THEME: &str = "AppsUseLightTheme";

/// The active Windows appearance mode for applications.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WindowsTheme {
    /// Standard light mode (`AppsUseLightTheme = 1`).
    Light,
    /// Standard dark mode (`AppsUseLightTheme = 0`).
    Dark,
    /// Windows High Contrast mode (`HCF_HIGHCONTRASTON` set).
    HighContrast,
}

impl WindowsTheme {
    /// Returns `true` when the theme is [`WindowsTheme::Dark`].
    pub const fn is_dark(self) -> bool {
        matches!(self, Self::Dark)
    }

    /// Returns `true` when the theme is [`WindowsTheme::HighContrast`].
    pub const fn is_high_contrast(self) -> bool {
        matches!(self, Self::HighContrast)
    }
}

/// Combines High-Contrast status and `AppsUseLightTheme` into a [`WindowsTheme`].
pub const fn theme_from_flags(
    high_contrast: bool,
    apps_use_light_theme: Option<bool>,
) -> WindowsTheme {
    if high_contrast {
        return WindowsTheme::HighContrast;
    }
    match apps_use_light_theme {
        Some(false) => WindowsTheme::Dark,
        Some(true) | None => WindowsTheme::Light,
    }
}

/// Queries `SPI_GETHIGHCONTRAST` via `SystemParametersInfoW`.
pub fn is_high_contrast() -> Result<bool, PlatformError> {
    let mut info = HIGHCONTRASTW {
        cbSize: std::mem::size_of::<HIGHCONTRASTW>() as u32,
        ..Default::default()
    };
    let size = info.cbSize;
    // SAFETY: `info` is properly initialized with `cbSize` set to `size_of::<HIGHCONTRASTW>()`.
    let result = unsafe {
        SystemParametersInfoW(
            SPI_GETHIGHCONTRAST,
            size,
            Some((&raw mut info).cast()),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        )
    };
    if let Err(err) = result {
        return Err(PlatformError::Win32 {
            operation: "SystemParametersInfoW(SPI_GETHIGHCONTRAST)",
            source: std::io::Error::from_raw_os_error(err.code().0),
        });
    }
    Ok(info.dwFlags.contains(HCF_HIGHCONTRASTON))
}

/// Reads `HKCU\Software\Microsoft\Windows\CurrentVersion\Themes\Personalize\AppsUseLightTheme`.
///
/// Returns `Ok(None)` if the key or value does not exist.
pub fn apps_use_light_theme() -> Result<Option<bool>, PlatformError> {
    let subkey = to_wide_null(PERSONALIZE_SUBKEY);
    let value_name = to_wide_null(APPS_USE_LIGHT_THEME);
    let mut hkey = HKEY::default();

    // SAFETY: `subkey` is NUL-terminated; `hkey` receives the opened handle.
    let open_status = unsafe {
        RegOpenKeyExW(
            HKEY_CURRENT_USER,
            PCWSTR(subkey.as_ptr()),
            Some(0),
            KEY_READ,
            &raw mut hkey,
        )
    };
    if open_status != ERROR_SUCCESS {
        if is_not_found(open_status) {
            return Ok(None);
        }
        return Err(reg_error("RegOpenKeyExW(Personalize)", open_status));
    }
    let _guard = RegistryKey(hkey);

    let mut value_type = REG_VALUE_TYPE::default();
    let mut dword: u32 = 0;
    let mut byte_len = std::mem::size_of::<u32>() as u32;

    // SAFETY: `dword` is writable for 4 bytes and `byte_len` is initialized to 4.
    let read_status = unsafe {
        RegQueryValueExW(
            hkey,
            PCWSTR(value_name.as_ptr()),
            None,
            Some(&raw mut value_type),
            Some((&raw mut dword).cast()),
            Some(&raw mut byte_len),
        )
    };
    if read_status != ERROR_SUCCESS {
        if is_not_found(read_status) {
            return Ok(None);
        }
        return Err(reg_error(
            "RegQueryValueExW(AppsUseLightTheme)",
            read_status,
        ));
    }
    if value_type != REG_DWORD || byte_len != 4 {
        return Err(PlatformError::InvalidRegistryValue {
            value_name: APPS_USE_LIGHT_THEME.to_owned(),
            detail: format!(
                "expected 4-byte REG_DWORD, got type {} ({byte_len} bytes)",
                value_type.0
            ),
        });
    }

    Ok(Some(dword != 0))
}

/// Detects the current Windows theme, prioritizing High Contrast over Light/Dark.
pub fn detect_theme() -> Result<WindowsTheme, PlatformError> {
    let high_contrast = is_high_contrast()?;
    if high_contrast {
        return Ok(WindowsTheme::HighContrast);
    }
    let light = apps_use_light_theme()?;
    Ok(theme_from_flags(false, light))
}

/// Detects the current Windows theme, falling back to [`WindowsTheme::Light`] on error.
pub fn detect_theme_or_default() -> WindowsTheme {
    detect_theme().unwrap_or(WindowsTheme::Light)
}

fn is_not_found(status: WIN32_ERROR) -> bool {
    status == windows::Win32::Foundation::ERROR_FILE_NOT_FOUND
        || status == windows::Win32::Foundation::ERROR_PATH_NOT_FOUND
}

fn reg_error(operation: &'static str, status: WIN32_ERROR) -> PlatformError {
    if status.0 == 0 {
        return last_win32_error(operation);
    }
    PlatformError::Win32 {
        operation,
        source: std::io::Error::from_raw_os_error(status.0 as i32),
    }
}
