//! Unified error types for `dataforge-services`.

use thiserror::Error;

/// Top-level error returned by DataForge service operations.
#[derive(Debug, Error)]
pub enum ServiceError {
    #[error("runtime error: {0}")]
    Runtime(String),

    #[error("session error: {0}")]
    Session(String),

    #[error("driver not registered for engine: {0}")]
    UnsupportedEngine(String),

    #[error("MCP host error: {0}")]
    Mcp(String),

    #[error("AI assistant error: {0}")]
    Ai(String),

    #[error("export error: {0}")]
    Export(String),

    #[error("import error: {0}")]
    Import(String),

    #[error("diagnostics error: {0}")]
    Diagnostics(String),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}
