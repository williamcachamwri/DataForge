//! Service layer wiring TablePro core crates, Windows platform adapters,
//! and headless application state for DataForge.

pub mod error;
pub mod runtime;

pub use error::ServiceError;
pub use runtime::{AppRuntime, REGISTERED_ENGINE_IDS, build_driver_registry};
