//! Windows platform integration for DataForge.

pub mod error;
pub mod paths;

pub use error::PlatformError;
pub use paths::{APP_DIR_NAME, storage_paths, storage_paths_for};
