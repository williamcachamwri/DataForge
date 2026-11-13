//! Startup, i18n, crash-report persistence, logging, and binary performance E2E tests.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use dataforge_e2e::E2eResult;
use dataforge_gfx::context::{D2dContext, DEFAULT_MEMORY_BUDGET_BYTES, MemoryBudget};
use dataforge_state::tabs::{
    CrashSafeTabPersistence, RestoredConnectionEntry, SessionRestorePlanner, TabConnectionId,
    TabKind, TabSessionEnvelope, TabStripModel,
};
use dataforge_windows_platform::{
    AppSettings, is_forwardable_target, storage_paths_for,
};

fn workspace_root() -> E2eResult<PathBuf> {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest_dir
        .parent()
        .and_then(Path::parent)
        .map(Path::to_path_buf)
        .ok_or_else(|| "failed to resolve workspace root from CARGO_MANIFEST_DIR".into())
}

#[test]
fn i18n_locale_catalogs_are_complete_and_non_empty() -> E2eResult<()> {
    let root = workspace_root()?;
    let app_i18n = fs::read_to_string(root.join("crates/dataforge-windows-app/src/i18n.rs"))?;
    let svc_diag = fs::read_to_string(root.join("crates/dataforge-services/src/diagnostics.rs"))?;

    let required_app_fields = [
        "app_title",
        "connections_library",
        "settings",
        "tables",
        "views",
        "functions",
        "data_tab",
        "structure_tab",
        "new_query_tab",
        "retry_connection",
        "safe_mode_title",
        "open_tx_title",
        "ai_assistant_title",
        "row_inspector_title",
    ];
    for field in required_app_fields {
        assert!(
            app_i18n.contains(field),
            "missing i18n Strings field: {field}"
        );
    }
    assert!(app_i18n.contains("Locale::EnUs"));

    let required_diag_keys = [
        "app.name",
        "connection.connect",
        "connection.disconnect",
        "connection.test",
        "connection.unavailable",
        "editor.run",
        "editor.stop",
        "grid.commit",
        "grid.rollback",
        "export.title",
        "import.title",
        "settings.title",
    ];
    for key in required_diag_keys {
        assert!(
            svc_diag.contains(key),
            "missing diagnostics LocaleTable key: {key}"
        );
    }
    Ok(())
}
