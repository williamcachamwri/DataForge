//! Service layer wiring TablePro core crates, Windows platform adapters,
//! and headless application state for TableMinus.

pub mod admin_tasks;
pub mod ai_bridge;
pub mod data_import;
pub mod data_transfer;
pub mod ddl_builder;
pub mod diagnostics;
pub mod error;
pub mod explain_parser;
pub mod export;
pub mod import_bridge;
pub mod mcp_host;
pub mod mock_generator;
pub mod runtime;
pub mod schema_compare;
pub mod server_monitor;
pub mod session_bridge;
pub mod sql_format;

pub use admin_tasks::{
    ScheduledTaskSchedule, active_sessions_query, evaluate_due_task_indices, evaluate_due_tasks,
    kill_session_sql, should_task_run_now, users_roles_query,
};
pub use ai_bridge::{
    AiAssistantService, AiEndpointConfig, DEFAULT_ANTHROPIC_URL, DEFAULT_OLLAMA_URL,
    DEFAULT_OPENAI_URL,
};
pub use data_import::{
    ColumnMapping, ImportFormat, ParsedImportPreview, detect_import_format,
    generate_import_sql_batches, preview_import_file, read_all_import_rows,
    split_sql_script_reader, split_sql_script_statements,
};
pub use ddl_builder::{
    AlterTableOp, ColumnSpec, CreateTableSpec, DatabaseEngine, ForeignKeySpec, IndexSpec,
    build_alter_table_sql, build_create_table_sql,
};
pub use diagnostics::{CrashReport, LocaleTable, init_logging, write_crash_report};
pub use error::ServiceError;
pub use explain_parser::{
    ExplainNode, JoinClause, JoinKind, VisualQuerySpec, WhereClause, build_visual_query_sql,
    parse_explain_output,
};
pub use export::{
    ExportFormat, ExportOptions, export_rows_atomic, part_path_for, stream_export_atomic,
};
pub use import_bridge::{
    SUPPORTED_IMPORT_APPS, foreign_inventory, has_foreign_data, import_from_app,
    import_from_app_with_dirs, parse_connection_url, test_connection,
};
pub use mcp_host::{
    McpServerController, OpenConnectionHandler, QueryHistoryRecorder, TableMinusMcpHost,
    WriteApprovalHandler, classify_cli_args,
};
pub use runtime::{AppRuntime, REGISTERED_ENGINE_IDS, build_driver_registry};
pub use schema_compare::{
    ColumnChangeKind, ColumnDiff, DataDiffReport, RowDiffEntry, RowDiffKind, SchemaDiffReport,
    diff_table_rows, diff_table_schemas,
};
pub use session_bridge::{ChannelResultSink, SessionManager, TableQueryOutcome};
pub use sql_format::{FindReplaceOptions, find_matches, format_sql, replace_all_matches};
