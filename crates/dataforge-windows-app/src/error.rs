//! Top-level error types for the DataForge Windows desktop application.

use std::fmt;

use dataforge_gfx::GfxError;
use dataforge_services::ServiceError;
use dataforge_windows_platform::PlatformError;

/// Unified result type for `dataforge-windows-app`.
pub type Result<T> = std::result::Result<T, AppError>;

/// Errors returned by DataForge application initialization, IPC, or UI rendering.
#[derive(Debug)]
pub enum AppError {
    /// Platform integration error (paths, credentials, settings, single-instance).
    Platform(PlatformError),
    /// Direct2D / DirectWrite graphics error.
    Gfx(GfxError),
    /// Win32 windowing or system API failure.
    Win32 {
        /// Win32 API operation name.
        operation: &'static str,
        /// Underlying OS error.
        source: std::io::Error,
    },
    /// I/O error during logging, crash reporting, or theme import.
    Io(std::io::Error),
    /// CLI argument or dispatch error.
    Cli(String),
    /// Service layer error (runtime, session, AI, MCP, export, import).
    Service(ServiceError),
    /// Invalid configuration or state transition.
    InvalidState(String),
}

impl fmt::Display for AppError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Platform(err) => write!(f, "platform error: {err}"),
            Self::Gfx(err) => write!(f, "graphics error: {err}"),
            Self::Win32 { operation, source } => write!(f, "Win32 {operation} failed: {source}"),
            Self::Io(err) => write!(f, "I/O error: {err}"),
            Self::Cli(msg) => write!(f, "CLI error: {msg}"),
            Self::Service(err) => write!(f, "service error: {err}"),
            Self::InvalidState(msg) => write!(f, "invalid state: {msg}"),
        }
    }
}

impl std::error::Error for AppError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Platform(err) => Some(err),
            Self::Gfx(err) => Some(err),
            Self::Win32 { source, .. } => Some(source),
            Self::Io(err) => Some(err),
            Self::Service(err) => Some(err),
            Self::Cli(_) | Self::InvalidState(_) => None,
        }
    }
}

impl From<PlatformError> for AppError {
    fn from(err: PlatformError) -> Self {
        Self::Platform(err)
    }
}

impl From<GfxError> for AppError {
    fn from(err: GfxError) -> Self {
        Self::Gfx(err)
    }
}

impl From<std::io::Error> for AppError {
    fn from(err: std::io::Error) -> Self {
        Self::Io(err)
    }
}

impl From<ServiceError> for AppError {
    fn from(err: ServiceError) -> Self {
        Self::Service(err)
    }
}
