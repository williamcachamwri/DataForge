//! Crash-safe persistence for workspace session snapshots and scheduled tasks.

use std::path::{Path, PathBuf};

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use tablepro_storage::StoragePaths;

use crate::error::PlatformError;
use crate::paths::storage_paths;
use crate::settings::write_bytes_atomic;

/// Convenience result alias for `tableminus-windows-platform` operations.
pub type PlatformResult<T> = Result<T, PlatformError>;

/// Default filename for persisted workspace session state inside [`StoragePaths::state`].
pub const WORKSPACE_SESSION_FILENAME: &str = "workspace_session.json";

/// Default filename for persisted scheduled tasks inside [`StoragePaths::state`].
pub const SCHEDULED_TASKS_FILENAME: &str = "scheduled_tasks.json";

/// Current schema version for [`WorkspaceSessionSnapshot`].
pub const WORKSPACE_SESSION_SCHEMA_VERSION: u32 = 1;

const ENVELOPE_VERSION: u32 = 1;

/// Persisted snapshot of an individual workspace tab.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct SavedTabSnapshot {
    /// Stable tab identifier.
    pub tab_id: String,
    /// Display title shown in the tab strip.
    pub title: String,
    /// Tab kind (`"table" | "query" | "structure" | "erd" | "compare" | "admin" | "query_builder"`).
    pub kind: String,
    /// Associated connection ID, if any.
    pub connection_id: Option<String>,
    /// Associated database / catalog name, if any.
    pub database: Option<String>,
    /// Associated schema / namespace name, if any.
    pub schema: Option<String>,
    /// Associated table / collection name, if any.
    pub table: Option<String>,
    /// Unsaved SQL editor draft contents.
    pub sql_draft: String,
    /// Active WHERE filter expression for table tabs.
    pub filter_clause: Option<String>,
    /// Active ORDER BY expression for table tabs.
    pub sort_clause: Option<String>,
    /// Whether the tab is pinned in the tab strip.
    pub is_pinned: bool,
}

/// Persisted workspace session snapshot restored across application restarts.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct WorkspaceSessionSnapshot {
    /// Snapshot schema version.
    pub version: u32,
    /// Active connection ID when the snapshot was captured.
    pub active_connection_id: Option<String>,
    /// Active database name when the snapshot was captured.
    pub active_database: Option<String>,
    /// Selected `(schema, table)` pair in the object tree.
    pub selected_table: Option<(Option<String>, String)>,
    /// Zero-based index of the active tab in [`Self::tabs`].
    pub active_tab_index: usize,
    /// Ordered open tabs to restore.
    pub tabs: Vec<SavedTabSnapshot>,
    /// Whether the left connection/object sidebar is open.
    pub sidebar_open: bool,
    /// Persisted sidebar width in logical pixels.
    pub sidebar_width: Option<f32>,
    /// Whether the right inspector panel is open.
    pub inspector_open: bool,
    /// Whether the bottom drawer is open.
    pub bottom_drawer_open: bool,
    /// Unix epoch timestamp in seconds when this snapshot was written.
    pub saved_at_unix: i64,
}

impl Default for WorkspaceSessionSnapshot {
    fn default() -> Self {
        Self {
            version: WORKSPACE_SESSION_SCHEMA_VERSION,
            active_connection_id: None,
            active_database: None,
            selected_table: None,
            active_tab_index: 0,
            tabs: Vec::new(),
            sidebar_open: true,
            sidebar_width: None,
            inspector_open: false,
            bottom_drawer_open: false,
            saved_at_unix: 0,
        }
    }
}

/// Execution history entry for a [`ScheduledTaskRecord`].
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ScheduledTaskRunEntry {
    /// Unix epoch timestamp in seconds when the run started/completed.
    pub ran_at_unix: i64,
    /// Whether the run completed without error.
    pub success: bool,
    /// Concise human-readable status or error summary.
    pub summary: String,
    /// Execution duration in milliseconds.
    pub duration_ms: u64,
}

/// Persisted scheduled maintenance/automation task definition and run history.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ScheduledTaskRecord {
    /// Unique task identifier.
    pub id: String,
    /// Human-readable task name.
    pub name: String,
    /// Target saved connection ID.
    pub connection_id: String,
    /// Task kind (`"sql" | "export" | "schema_compare"`).
    pub task_kind: String,
    /// SQL query or target configuration string for the task.
    pub sql_or_target: String,
    /// Recurrence interval in minutes.
    pub interval_minutes: u32,
    /// Whether automatic execution is enabled.
    pub enabled: bool,
    /// Unix epoch timestamp in seconds of the most recent run.
    pub last_run_unix: Option<i64>,
    /// Summary status of the most recent run.
    pub last_status: Option<String>,
    /// Duration in milliseconds of the most recent run.
    pub last_duration_ms: Option<u64>,
    /// Recent execution log entries.
    pub run_history: Vec<ScheduledTaskRunEntry>,
}

/// Returns the canonical workspace session file path for `paths`.
pub fn workspace_session_path(paths: &StoragePaths) -> PathBuf {
    paths.state.join(WORKSPACE_SESSION_FILENAME)
}

/// Returns the canonical scheduled tasks file path for `paths`.
pub fn scheduled_tasks_path(paths: &StoragePaths) -> PathBuf {
    paths.state.join(SCHEDULED_TASKS_FILENAME)
}

/// Atomically persists `snapshot` with a checksum envelope.
///
/// When `path_opt` is `None`, writes to [`workspace_session_path`] under [`storage_paths`].
pub fn save_workspace_session(
    path_opt: Option<&Path>,
    snapshot: &WorkspaceSessionSnapshot,
) -> PlatformResult<()> {
    let path = resolve_state_path(path_opt, WORKSPACE_SESSION_FILENAME)?;
    save_enveloped_atomic(&path, "WorkspaceSessionSnapshot", snapshot)
}

/// Loads a checksum-verified workspace session snapshot.
///
/// Returns `Ok(None)` when the file does not exist or when the persisted JSON/checksum
/// is corrupted (logging a warning instead of failing startup).
pub fn load_workspace_session(
    path_opt: Option<&Path>,
) -> PlatformResult<Option<WorkspaceSessionSnapshot>> {
    let path = resolve_state_path(path_opt, WORKSPACE_SESSION_FILENAME)?;
    load_enveloped(&path, "WorkspaceSessionSnapshot")
}

/// Atomically persists `tasks` with a checksum envelope.
///
/// When `path_opt` is `None`, writes to [`scheduled_tasks_path`] under [`storage_paths`].
pub fn save_scheduled_tasks(
    path_opt: Option<&Path>,
    tasks: &[ScheduledTaskRecord],
) -> PlatformResult<()> {
    let path = resolve_state_path(path_opt, SCHEDULED_TASKS_FILENAME)?;
    save_enveloped_atomic(&path, "ScheduledTaskRecord", tasks)
}

/// Loads checksum-verified scheduled tasks.
///
/// Returns an empty `Vec` when the file does not exist or when the persisted JSON/checksum
/// is corrupted (logging a warning instead of failing startup).
pub fn load_scheduled_tasks(path_opt: Option<&Path>) -> PlatformResult<Vec<ScheduledTaskRecord>> {
    let path = resolve_state_path(path_opt, SCHEDULED_TASKS_FILENAME)?;
    Ok(load_enveloped(&path, "ScheduledTaskRecord")?.unwrap_or_default())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ChecksumEnvelope<T> {
    envelope_version: u32,
    checksum: u64,
    payload: T,
}

fn resolve_state_path(path_opt: Option<&Path>, default_filename: &str) -> PlatformResult<PathBuf> {
    match path_opt {
        Some(path) => Ok(path.to_path_buf()),
        None => Ok(storage_paths()?.state.join(default_filename)),
    }
}

fn save_enveloped_atomic<T: Serialize + ?Sized>(
    path: &Path,
    context: &'static str,
    payload: &T,
) -> PlatformResult<()> {
    let canonical_bytes = serde_json::to_vec(payload).map_err(|err| PlatformError::Json {
        context,
        source: err,
    })?;
    let envelope = ChecksumEnvelope {
        envelope_version: ENVELOPE_VERSION,
        checksum: fnv1a_64(&canonical_bytes),
        payload,
    };
    let encoded = serde_json::to_vec_pretty(&envelope).map_err(|err| PlatformError::Json {
        context,
        source: err,
    })?;
    write_bytes_atomic(path, &encoded)
}

fn load_enveloped<T: Serialize + DeserializeOwned>(
    path: &Path,
    context: &'static str,
) -> PlatformResult<Option<T>> {
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => {
            return Err(PlatformError::Io {
                operation: "read persisted state file",
                source: err,
            });
        }
    };

    let envelope: ChecksumEnvelope<T> = match serde_json::from_slice(&bytes) {
        Ok(env) => env,
        Err(err) => {
            tracing::warn!(
                path = %path.display(),
                context,
                error = %err,
                "ignoring corrupt persisted state file"
            );
            return Ok(None);
        }
    };

    if envelope.envelope_version != ENVELOPE_VERSION {
        tracing::warn!(
            path = %path.display(),
            context,
            version = envelope.envelope_version,
            "ignoring unsupported persisted state envelope version"
        );
        return Ok(None);
    }

    let canonical_bytes = match serde_json::to_vec(&envelope.payload) {
        Ok(bytes) => bytes,
        Err(err) => {
            tracing::warn!(
                path = %path.display(),
                context,
                error = %err,
                "failed to canonicalize persisted payload for checksum check"
            );
            return Ok(None);
        }
    };

    let actual_checksum = fnv1a_64(&canonical_bytes);
    if actual_checksum != envelope.checksum {
        tracing::warn!(
            path = %path.display(),
            context,
            expected = envelope.checksum,
            actual = actual_checksum,
            "checksum mismatch in persisted state file; ignoring corrupt snapshot"
        );
        return Ok(None);
    }

    Ok(Some(envelope.payload))
}

fn fnv1a_64(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for &byte in bytes {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    hash
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    type TestResult = Result<(), Box<dyn std::error::Error>>;

    fn sample_snapshot() -> WorkspaceSessionSnapshot {
        WorkspaceSessionSnapshot {
            version: WORKSPACE_SESSION_SCHEMA_VERSION,
            active_connection_id: Some("pg-prod".to_owned()),
            active_database: Some("analytics".to_owned()),
            selected_table: Some((Some("public".to_owned()), "orders".to_owned())),
            active_tab_index: 1,
            tabs: vec![
                SavedTabSnapshot {
                    tab_id: "tab-1".to_owned(),
                    title: "orders".to_owned(),
                    kind: "table".to_owned(),
                    connection_id: Some("pg-prod".to_owned()),
                    database: Some("analytics".to_owned()),
                    schema: Some("public".to_owned()),
                    table: Some("orders".to_owned()),
                    sql_draft: String::new(),
                    filter_clause: Some("status = 'paid'".to_owned()),
                    sort_clause: Some("created_at DESC".to_owned()),
                    is_pinned: true,
                },
                SavedTabSnapshot {
                    tab_id: "tab-2".to_owned(),
                    title: "Query 1".to_owned(),
                    kind: "query".to_owned(),
                    connection_id: Some("pg-prod".to_owned()),
                    database: Some("analytics".to_owned()),
                    schema: None,
                    table: None,
                    sql_draft: "SELECT count(*) FROM public.orders;".to_owned(),
                    filter_clause: None,
                    sort_clause: None,
                    is_pinned: false,
                },
            ],
            sidebar_open: true,
            sidebar_width: Some(275.5),
            inspector_open: true,
            bottom_drawer_open: false,
            saved_at_unix: 1_763_180_000,
        }
    }

    #[test]
    fn workspace_session_roundtrip_and_missing_file() -> TestResult {
        let dir = TempDir::new()?;
        let path = dir.path().join("state").join(WORKSPACE_SESSION_FILENAME);

        assert_eq!(load_workspace_session(Some(&path))?, None);

        let snapshot = sample_snapshot();
        save_workspace_session(Some(&path), &snapshot)?;
        let loaded = load_workspace_session(Some(&path))?;
        assert_eq!(loaded, Some(snapshot));

        let updated = WorkspaceSessionSnapshot {
            active_tab_index: 0,
            bottom_drawer_open: true,
            ..sample_snapshot()
        };
        save_workspace_session(Some(&path), &updated)?;
        assert_eq!(load_workspace_session(Some(&path))?, Some(updated));

        let state_dir = path
            .parent()
            .ok_or_else(|| std::io::Error::other("missing state dir"))?;
        let entries: Vec<_> = std::fs::read_dir(state_dir)?
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(entries, vec![WORKSPACE_SESSION_FILENAME.to_owned()]);
        Ok(())
    }

    #[test]
    fn workspace_session_recovers_from_corrupt_json_and_checksum_mismatch() -> TestResult {
        let dir = TempDir::new()?;
        let path = dir.path().join(WORKSPACE_SESSION_FILENAME);

        // Corrupt JSON / truncated write.
        std::fs::write(
            &path,
            br#"{"envelope_version": 1, "checksum": 123, "payload": {"#,
        )?;
        assert_eq!(load_workspace_session(Some(&path))?, None);

        // Valid JSON, tampered payload with stale checksum.
        let snapshot = sample_snapshot();
        save_workspace_session(Some(&path), &snapshot)?;
        let raw = std::fs::read_to_string(&path)?;
        let tampered = raw.replacen("\"orders\"", "\"users\"", 1);
        assert_ne!(raw, tampered);
        std::fs::write(&path, tampered)?;
        assert_eq!(load_workspace_session(Some(&path))?, None);

        // Valid payload, unsupported envelope version.
        save_workspace_session(Some(&path), &snapshot)?;
        let raw = std::fs::read_to_string(&path)?;
        let bad_version = raw.replacen("\"envelope_version\": 1", "\"envelope_version\": 99", 1);
        std::fs::write(&path, bad_version)?;
        assert_eq!(load_workspace_session(Some(&path))?, None);
        Ok(())
    }

    #[test]
    fn scheduled_tasks_roundtrip_and_corrupt_recovery() -> TestResult {
        let dir = TempDir::new()?;
        let path = dir.path().join("state").join(SCHEDULED_TASKS_FILENAME);

        assert!(load_scheduled_tasks(Some(&path))?.is_empty());

        let tasks = vec![
            ScheduledTaskRecord {
                id: "task-1".to_owned(),
                name: "Vacuum SQLite".to_owned(),
                connection_id: "local-sqlite".to_owned(),
                task_kind: "sql".to_owned(),
                sql_or_target: "VACUUM;".to_owned(),
                interval_minutes: 60,
                enabled: true,
                last_run_unix: Some(1_763_180_100),
                last_status: Some("ok".to_owned()),
                last_duration_ms: Some(42),
                run_history: vec![ScheduledTaskRunEntry {
                    ran_at_unix: 1_763_180_100,
                    success: true,
                    summary: "0 rows affected".to_owned(),
                    duration_ms: 42,
                }],
            },
            ScheduledTaskRecord {
                id: "task-2".to_owned(),
                name: "Nightly Export".to_owned(),
                connection_id: "pg-prod".to_owned(),
                task_kind: "export".to_owned(),
                sql_or_target: "public.orders -> exports/orders.csv".to_owned(),
                interval_minutes: 1440,
                enabled: false,
                last_run_unix: None,
                last_status: None,
                last_duration_ms: None,
                run_history: Vec::new(),
            },
        ];

        save_scheduled_tasks(Some(&path), &tasks)?;
        assert_eq!(load_scheduled_tasks(Some(&path))?, tasks);

        // Tamper payload without updating checksum.
        let raw = std::fs::read_to_string(&path)?;
        let tampered = raw.replacen("VACUUM;", "DROP TABLE x;", 1);
        std::fs::write(&path, tampered)?;
        assert!(load_scheduled_tasks(Some(&path))?.is_empty());
        Ok(())
    }

    #[test]
    fn storage_path_helpers_use_state_dir() {
        let paths = StoragePaths {
            config: PathBuf::from(r"C:\tmp\TableMinus\config"),
            state: PathBuf::from(r"C:\tmp\TableMinus\state"),
            data: PathBuf::from(r"C:\tmp\TableMinus\data"),
            cache: PathBuf::from(r"C:\tmp\TableMinus\cache"),
            runtime: PathBuf::from(r"C:\tmp\TableMinus\runtime"),
        };
        assert_eq!(
            workspace_session_path(&paths),
            PathBuf::from(r"C:\tmp\TableMinus\state\workspace_session.json")
        );
        assert_eq!(
            scheduled_tasks_path(&paths),
            PathBuf::from(r"C:\tmp\TableMinus\state\scheduled_tasks.json")
        );
    }
}
