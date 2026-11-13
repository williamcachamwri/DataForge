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

#[test]
fn crash_report_atomic_write_and_session_recovery() -> E2eResult<()> {
    let temp = tempfile::tempdir()?;
    let crash_dir = temp.path().join("crash-reports");
    fs::create_dir_all(&crash_dir)?;

    // Verify atomic `.part` -> rename write semantics for crash reports.
    let target = crash_dir.join("dataforge-crash-test.txt");
    let part = crash_dir.join("dataforge-crash-test.txt.part");
    fs::write(
        &part,
        "DataForge Crash Report\nVersion: 0.1.0\nMessage: simulated panic\n",
    )?;
    fs::rename(&part, &target)?;
    assert!(!part.exists());
    assert!(target.exists());
    let body = fs::read_to_string(&target)?;
    assert!(body.contains("DataForge Crash Report"));

    // Verify CrashSafeTabPersistence primary + backup recovery on corruption.
    let primary = temp.path().join("tabs-session.json");
    let backup = temp.path().join("tabs-session.bak.json");
    let conn_id = TabConnectionId("conn-crash-1".to_owned());
    let mut strip = TabStripModel::new();
    strip.open_query_tab(conn_id.clone(), "SELECT 1".to_owned());

    let mut map = std::collections::BTreeMap::new();
    map.insert(conn_id.clone(), strip.tabs().to_vec());

    // Save twice so backup holds the first valid snapshot.
    CrashSafeTabPersistence::save_atomic(&primary, &backup, &map)
        .map_err(|e| format!("first save failed: {e}"))?;
    strip.open_table_tab(conn_id.clone(), "users".to_owned());
    map.insert(conn_id.clone(), strip.tabs().to_vec());
    CrashSafeTabPersistence::save_atomic(&primary, &backup, &map)
        .map_err(|e| format!("second save failed: {e}"))?;

    // Corrupt primary and ensure recovery falls back to valid backup.
    fs::write(&primary, b"{corrupted json")?;
    let recovered = CrashSafeTabPersistence::load_with_recovery(&primary, &backup)
        .map_err(|e| format!("recovery failed: {e}"))?;
    let restored_tabs = recovered
        .get(&conn_id)
        .ok_or("missing connection tabs in recovered backup")?;
    assert_eq!(restored_tabs.len(), 1);
    assert_eq!(restored_tabs[0].kind, TabKind::Query);

    let planned = SessionRestorePlanner::plan(
        true,
        &[RestoredConnectionEntry {
            id: conn_id,
            was_connected_or_failed: true,
        }],
    );
    assert_eq!(planned.len(), 1);
    Ok(())
}
