//! Error types for the `tableminus-gfx` rendering crate.

use thiserror::Error;

/// Result alias for `tableminus-gfx` operations.
pub type Result<T> = std::result::Result<T, GfxError>;

/// Errors returned by Direct2D, DirectWrite, SVG path parsing, and layout operations.
#[derive(Debug, Error)]
pub enum GfxError {
    /// A Win32 / COM / Direct2D / DirectWrite API call failed.
    #[error("Win32 graphics error: {0}")]
    Win32(#[from] windows::core::Error),

    /// The Direct2D render target was lost (`D2DERR_RECREATE_TARGET`) and must be recreated.
    #[error("Direct2D render target lost and requires recreation")]
    TargetLost,

    /// An SVG path string could not be parsed.
    #[error("invalid SVG path at byte {offset}: {reason}")]
    InvalidSvgPath {
        /// Byte offset in the path string where parsing failed.
        offset: usize,
        /// Human-readable description of the syntax error.
        reason: String,
    },

    /// A requested surface dimension or scale factor is invalid.
    #[error("invalid surface dimensions: {width}x{height} (dpi {dpi})")]
    InvalidSurfaceMetrics {
        /// Surface width in physical pixels.
        width: u32,
        /// Surface height in physical pixels.
        height: u32,
        /// DPI value.
        dpi: f32,
    },
}
