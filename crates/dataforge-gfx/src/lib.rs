//! GPU-native Win32 + Direct2D + DirectWrite rendering library for DataForge.

pub mod context;
pub mod error;
pub mod glyphs;
pub mod grid_renderer;
pub mod palette;

pub use error::{GfxError, Result};
