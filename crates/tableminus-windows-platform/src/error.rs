//! Shared error and Win32 helper types for `tableminus-windows-platform`.

use thiserror::Error;

/// Errors returned by Windows platform integration helpers.
#[derive(Debug, Error)]
pub enum PlatformError {
    /// A Win32 API call failed with an OS error or HRESULT.
    #[error("{operation} failed: {source}")]
    Win32 {
        /// Human-readable description of the operation that failed.
        operation: &'static str,
        /// Underlying OS or Win32 error.
        #[source]
        source: std::io::Error,
    },

    /// A registry value had an unexpected type or invalid UTF-16/format.
    #[error("invalid registry value for {value_name}: {detail}")]
    InvalidRegistryValue {
        /// Name of the registry value.
        value_name: String,
        /// Reason the value could not be interpreted.
        detail: String,
    },

    /// A URL failed validation before being handed to `ShellExecuteW`.
    #[error("invalid URL for browser launch: {0}")]
    InvalidUrl(String),

    /// An I/O error occurred while reading or writing platform files or pipes.
    #[error("platform I/O error during {operation}: {source}")]
    Io {
        /// Human-readable description of the operation that failed.
        operation: &'static str,
        /// Underlying standard I/O error.
        #[source]
        source: std::io::Error,
    },

    /// JSON serialization or deserialization failed.
    #[error("failed to serialize or parse {context}: {source}")]
    Json {
        /// Context describing what was being serialized or parsed.
        context: &'static str,
        /// Underlying `serde_json` error.
        #[source]
        source: serde_json::Error,
    },
}

/// Encodes a Rust string slice as a NUL-terminated UTF-16 buffer for Win32 `*W` APIs.
pub(crate) fn to_wide_null(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Converts the current thread's `GetLastError` into a [`PlatformError::Win32`].
pub(crate) fn last_win32_error(operation: &'static str) -> PlatformError {
    PlatformError::Win32 {
        operation,
        source: std::io::Error::last_os_error(),
    }
}
