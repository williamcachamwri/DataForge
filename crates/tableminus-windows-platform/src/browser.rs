//! Windows default-browser launcher via `ShellExecuteW`.

use async_trait::async_trait;
use tablepro_account::BrowserLauncher;
use url::Url;
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
use windows::core::PCWSTR;

use crate::error::{PlatformError, to_wide_null};

/// Validates that `url` uses the `http` or `https` scheme and has a host.
pub fn validate_http_url(url: &Url) -> Result<(), PlatformError> {
    if !matches!(url.scheme(), "http" | "https") {
        return Err(PlatformError::InvalidUrl(format!(
            "unsupported URL scheme {:?}; only http and https are allowed",
            url.scheme()
        )));
    }
    if !url.has_host() {
        return Err(PlatformError::InvalidUrl(
            "URL must include a host".to_owned(),
        ));
    }
    if url.as_str().contains('\0') {
        return Err(PlatformError::InvalidUrl(
            "URL must not contain NUL bytes".to_owned(),
        ));
    }
    Ok(())
}

/// Launches URLs in the user's default Windows browser using `ShellExecuteW`.
#[derive(Debug, Default, Clone, Copy)]
pub struct WindowsBrowserLauncher;

impl WindowsBrowserLauncher {
    /// Creates a new [`WindowsBrowserLauncher`].
    pub const fn new() -> Self {
        Self
    }

    /// Validates `url` and opens it synchronously via `ShellExecuteW`.
    pub fn open_sync(&self, url: &Url) -> Result<(), PlatformError> {
        validate_http_url(url)?;
        let verb = to_wide_null("open");
        let target = to_wide_null(url.as_str());

        // SAFETY: `verb` and `target` are valid NUL-terminated UTF-16 strings that outlive the call.
        let result = unsafe {
            ShellExecuteW(
                None,
                PCWSTR(verb.as_ptr()),
                PCWSTR(target.as_ptr()),
                PCWSTR::null(),
                PCWSTR::null(),
                SW_SHOWNORMAL,
            )
        };

        // ShellExecuteW returns an HINSTANCE > 32 on success.
        let code = result.0 as isize;
        if code <= 32 {
            return Err(PlatformError::Win32 {
                operation: "ShellExecuteW",
                source: std::io::Error::from_raw_os_error(code as i32),
            });
        }
        Ok(())
    }
}

#[async_trait]
impl BrowserLauncher for WindowsBrowserLauncher {
    async fn open(&self, url: &Url) -> Result<(), String> {
        let url = url.clone();
        let launcher = *self;
        tokio::task::spawn_blocking(move || launcher.open_sync(&url))
            .await
            .map_err(|err| format!("browser launch task failed: {err}"))?
            .map_err(|err| err.to_string())
    }
}
