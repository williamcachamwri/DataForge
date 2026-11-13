//! Service layer wiring TablePro core crates, Windows platform adapters,
//! and headless application state for DataForge.

pub mod ai_bridge;
pub mod diagnostics;
pub mod error;
pub mod export;
pub mod import_bridge;
pub mod mcp_host;
pub mod runtime;
pub mod session_bridge;

pub use ai_bridge::{
    AiAssistantService, AiEndpointConfig, DEFAULT_ANTHROPIC_URL, DEFAULT_OLLAMA_URL,
    DEFAULT_OPENAI_URL,
};
pub use diagnostics::{CrashReport, LocaleTable, init_logging, write_crash_report};
pub use error::ServiceError;
pub use export::{
    ExportFormat, ExportOptions, export_rows_atomic, part_path_for, stream_export_atomic,
};
pub use import_bridge::{
    SUPPORTED_IMPORT_APPS, foreign_inventory, has_foreign_data, import_from_app,
    import_from_app_with_dirs, parse_connection_url, test_connection,
};
pub use mcp_host::{
    DataForgeMcpHost, McpServerController, OpenConnectionHandler, QueryHistoryRecorder,
    WriteApprovalHandler, classify_cli_args,
};
pub use runtime::{AppRuntime, REGISTERED_ENGINE_IDS, build_driver_registry};
pub use session_bridge::{ChannelResultSink, SessionManager, TableQueryOutcome};
