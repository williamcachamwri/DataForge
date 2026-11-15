//! Live server monitor, lock inspection, and safe session termination service.

use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tablepro_core::column::ResultColumn;
use tablepro_core::value::{Temporal, Value};
use tablepro_core::{
    EditorSessionOptions, QueryResult, ResultEvent, ResultSink, RunEnd, RunOptions, SinkFlow,
    UserSql,
};
use tablepro_session::Session;
use tokio_util::sync::CancellationToken;

use crate::ddl_builder::DatabaseEngine;
use crate::error::ServiceError;

/// Maximum accepted length for non-numeric server query/session identifiers (e.g. ClickHouse UUIDs).
const MAX_QUERY_ID_LEN: usize = 128;

/// Normalized active or idle session row reported by a database server's process catalog.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LiveSessionRow {
    pub id: String,
    pub user: String,
    pub database: String,
    pub state: String,
    pub duration_ms: Option<u64>,
    pub query: String,
    pub client_addr: Option<String>,
    pub wait_event: Option<String>,
}

/// Normalized lock / blocking entry reported by a database server's lock catalog.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServerLockRow {
    pub pid: String,
    pub lock_type: String,
    pub relation: String,
    pub mode: String,
    pub granted: bool,
    pub blocked_by: Option<String>,
}

/// Point-in-time snapshot of live server sessions, locks, and summary diagnostics.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServerMonitorSnapshot {
    pub engine: DatabaseEngine,
    pub sessions: Vec<LiveSessionRow>,
    pub locks: Vec<ServerLockRow>,
    pub summary: String,
}

/// Returns a dialect-specific read-only SQL query to list live server sessions,
/// or `None` for embedded/file engines without server process catalogs.
#[must_use]
pub const fn sessions_query(engine: DatabaseEngine) -> Option<&'static str> {
    match engine {
        DatabaseEngine::Postgres => Some(
            "SELECT pid::text AS id, \
             COALESCE(usename, '') AS username, \
             COALESCE(datname, '') AS database_name, \
             COALESCE(state, 'unknown') AS state, \
             CAST(GREATEST(0, EXTRACT(EPOCH FROM (clock_timestamp() - COALESCE(query_start, state_change, backend_start))) * 1000) AS BIGINT) AS duration_ms, \
             COALESCE(left(query, 500), '') AS query, \
             host(client_addr) AS client_addr, \
             CASE WHEN wait_event_type IS NOT NULL OR wait_event IS NOT NULL \
                  THEN concat_ws(':', wait_event_type, wait_event) \
                  ELSE NULL END AS wait_event \
             FROM pg_stat_activity \
             WHERE pid <> pg_backend_pid() \
             ORDER BY duration_ms DESC NULLS LAST;",
        ),
        DatabaseEngine::Mysql => Some(
            "SELECT CAST(Id AS CHAR) AS id, \
             COALESCE(User, '') AS username, \
             COALESCE(db, '') AS database_name, \
             COALESCE(State, Command, '') AS state, \
             CAST(COALESCE(Time, 0) * 1000 AS UNSIGNED) AS duration_ms, \
             COALESCE(LEFT(Info, 500), '') AS query, \
             Host AS client_addr, \
             Command AS wait_event \
             FROM information_schema.PROCESSLIST \
             ORDER BY Time DESC;",
        ),
        DatabaseEngine::Mssql => Some(
            "SELECT CAST(s.session_id AS VARCHAR(32)) AS id, \
             COALESCE(s.login_name, '') AS username, \
             COALESCE(DB_NAME(s.database_id), '') AS database_name, \
             COALESCE(r.status, s.status, '') AS state, \
             CAST(COALESCE(r.total_elapsed_time, DATEDIFF(MILLISECOND, s.last_request_start_time, GETDATE()), 0) AS BIGINT) AS duration_ms, \
             COALESCE(SUBSTRING(t.text, 1, 500), '') AS query, \
             c.client_net_address AS client_addr, \
             r.wait_type AS wait_event \
             FROM sys.dm_exec_sessions s \
             LEFT JOIN sys.dm_exec_requests r ON s.session_id = r.session_id \
             LEFT JOIN sys.dm_exec_connections c ON s.session_id = c.session_id \
             OUTER APPLY sys.dm_exec_sql_text(r.sql_handle) t \
             WHERE s.is_user_process = 1 \
             ORDER BY duration_ms DESC;",
        ),
        DatabaseEngine::Clickhouse => Some(
            "SELECT query_id AS id, \
             user AS username, \
             current_database AS database_name, \
             if(is_cancelled, 'cancelled', 'running') AS state, \
             toUInt64(elapsed * 1000) AS duration_ms, \
             substring(query, 1, 500) AS query, \
             toString(address) AS client_addr, \
             NULL AS wait_event \
             FROM system.processes \
             ORDER BY elapsed DESC;",
        ),
        DatabaseEngine::Sqlite | DatabaseEngine::Duckdb | DatabaseEngine::Generic => None,
    }
}

/// Returns a dialect-specific read-only SQL query to inspect active server locks and blockers,
/// or `None` for engines without a lock catalog.
#[must_use]
pub const fn locks_query(engine: DatabaseEngine) -> Option<&'static str> {
    match engine {
        DatabaseEngine::Postgres => Some(
            "SELECT l.pid::text AS pid, \
             COALESCE(l.locktype, '') AS lock_type, \
             COALESCE(l.relation::regclass::text, l.locktype, '') AS relation, \
             COALESCE(l.mode, '') AS mode, \
             l.granted AS granted, \
             NULLIF(array_to_string(pg_blocking_pids(l.pid), ','), '') AS blocked_by \
             FROM pg_locks l \
             WHERE l.pid <> pg_backend_pid() \
             ORDER BY l.granted ASC, l.pid ASC;",
        ),
        DatabaseEngine::Mysql => Some(
            "SELECT CAST(r.trx_mysql_thread_id AS CHAR) AS pid, \
             'transaction' AS lock_type, \
             COALESCE(w.blocking_engine_lock_id, 'innodb') AS relation, \
             COALESCE(r.trx_state, 'LOCK WAIT') AS mode, \
             (w.requesting_engine_transaction_id IS NULL) AS granted, \
             CAST(b.trx_mysql_thread_id AS CHAR) AS blocked_by \
             FROM information_schema.innodb_trx r \
             LEFT JOIN performance_schema.data_lock_waits w \
               ON w.requesting_engine_transaction_id = r.trx_id \
             LEFT JOIN information_schema.innodb_trx b \
               ON b.trx_id = w.blocking_engine_transaction_id \
             ORDER BY granted ASC, pid ASC;",
        ),
        DatabaseEngine::Mssql => Some(
            "SELECT CAST(l.request_session_id AS VARCHAR(32)) AS pid, \
             COALESCE(l.resource_type, '') AS lock_type, \
             COALESCE(CAST(l.resource_associated_entity_id AS VARCHAR(64)), '') AS relation, \
             COALESCE(l.request_mode, '') AS mode, \
             CASE WHEN l.request_status = 'GRANT' THEN CAST(1 AS BIT) ELSE CAST(0 AS BIT) END AS granted, \
             CASE WHEN r.blocking_session_id IS NOT NULL AND r.blocking_session_id <> 0 \
                  THEN CAST(r.blocking_session_id AS VARCHAR(32)) \
                  ELSE NULL END AS blocked_by \
             FROM sys.dm_tran_locks l \
             LEFT JOIN sys.dm_exec_requests r ON l.request_session_id = r.session_id \
             WHERE l.request_session_id > 50 \
             ORDER BY granted ASC, l.request_session_id ASC;",
        ),
        DatabaseEngine::Clickhouse
        | DatabaseEngine::Sqlite
        | DatabaseEngine::Duckdb
        | DatabaseEngine::Generic => None,
    }
}

/// Validates a session or query identifier before interpolating it into a termination statement.
///
/// Numeric PIDs (PostgreSQL, MySQL, MSSQL) must consist solely of ASCII digits and be greater
/// than zero, rejecting signs (`+`/`-`), whitespace, and SQL metacharacters. ClickHouse query
/// IDs must be safe identifier/UUID tokens (`[A-Za-z0-9._:-]`).
///
/// # Errors
///
/// Returns [`ServiceError::UnsupportedEngine`] for embedded engines or [`ServiceError::Session`]
/// if `session_id` fails strict validation.
pub fn validate_session_id(
    engine: DatabaseEngine,
    session_id: &str,
) -> Result<String, ServiceError> {
    let trimmed = session_id.trim();
    if trimmed.is_empty() {
        return Err(ServiceError::Session(
            "session id cannot be empty".to_owned(),
        ));
    }
    match engine {
        DatabaseEngine::Postgres | DatabaseEngine::Mysql | DatabaseEngine::Mssql => {
            if !trimmed.bytes().all(|b| b.is_ascii_digit()) {
                return Err(ServiceError::Session(format!(
                    "invalid numeric session pid: '{trimmed}'"
                )));
            }
            let pid = trimmed.parse::<u64>().map_err(|_| {
                ServiceError::Session(format!("session pid out of range: '{trimmed}'"))
            })?;
            if pid == 0 {
                return Err(ServiceError::Session(
                    "session pid must be greater than zero".to_owned(),
                ));
            }
            Ok(pid.to_string())
        }
        DatabaseEngine::Clickhouse => {
            if trimmed.len() > MAX_QUERY_ID_LEN {
                return Err(ServiceError::Session(
                    "ClickHouse query_id exceeds maximum length".to_owned(),
                ));
            }
            let is_safe = trimmed
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b':'));
            if !is_safe {
                return Err(ServiceError::Session(format!(
                    "invalid ClickHouse query_id: '{trimmed}'"
                )));
            }
            Ok(trimmed.to_owned())
        }
        DatabaseEngine::Sqlite | DatabaseEngine::Duckdb | DatabaseEngine::Generic => {
            Err(ServiceError::UnsupportedEngine(engine.as_str().to_owned()))
        }
    }
}

/// Generates a dialect-specific SQL statement to terminate a live session or query.
///
/// Strictly validates `session_id` via [`validate_session_id`] to prevent SQL injection.
///
/// # Errors
///
/// Returns [`ServiceError::UnsupportedEngine`] for embedded engines or [`ServiceError::Session`]
/// if `session_id` is invalid.
pub fn kill_session_sql(engine: DatabaseEngine, session_id: &str) -> Result<String, ServiceError> {
    let validated = validate_session_id(engine, session_id)?;
    match engine {
        DatabaseEngine::Postgres => Ok(format!("SELECT pg_terminate_backend({validated});")),
        DatabaseEngine::Mysql | DatabaseEngine::Mssql => Ok(format!("KILL {validated};")),
        DatabaseEngine::Clickhouse => Ok(format!(
            "KILL QUERY WHERE query_id = {};",
            engine.quote_literal(&validated)
        )),
        DatabaseEngine::Sqlite | DatabaseEngine::Duckdb | DatabaseEngine::Generic => {
            Err(ServiceError::UnsupportedEngine(engine.as_str().to_owned()))
        }
    }
}

/// Parses a [`QueryResult`] returned by [`sessions_query`] (or native engine process views)
/// into normalized [`LiveSessionRow`] entries.
#[must_use]
pub fn parse_sessions_result(engine: DatabaseEngine, set: &QueryResult) -> Vec<LiveSessionRow> {
    if set.rows.is_empty() || set.columns.is_empty() {
        return Vec::new();
    }
    let cols = &set.columns;
    let id_col = find_col(cols, &["id", "pid", "session_id", "query_id"]).or(Some(0));
    let user_col = find_col(cols, &["username", "user", "usename", "login_name"]);
    let db_col = find_col(
        cols,
        &[
            "database_name",
            "database",
            "datname",
            "db",
            "current_database",
        ],
    );
    let state_col = find_col(cols, &["state", "status", "command"]);
    let (dur_col, dur_is_seconds) = find_duration_col(cols);
    let query_col = find_col(cols, &["query", "info", "sql_text", "text"]);
    let addr_col = find_col(
        cols,
        &["client_addr", "host", "address", "client_net_address"],
    );
    let wait_col = find_col(cols, &["wait_event", "wait_type", "wait_state"]);
    let wait_type_col = find_col(cols, &["wait_event_type"]);

    let mut out = Vec::with_capacity(set.rows.len());
    for row in &set.rows {
        let id = cell_text(row, id_col);
        if id.is_empty() {
            continue;
        }
        let mut state = cell_text(row, state_col);
        if state.is_empty() && engine == DatabaseEngine::Clickhouse {
            state = "running".to_owned();
        }
        let wait_event = combine_wait_event(row, wait_type_col, wait_col);
        let duration_ms = dur_col
            .and_then(|idx| row.get(idx))
            .and_then(|v| value_to_duration_ms(v, dur_is_seconds));

        out.push(LiveSessionRow {
            id,
            user: cell_text(row, user_col),
            database: cell_text(row, db_col),
            state,
            duration_ms,
            query: cell_text(row, query_col),
            client_addr: cell_opt_text(row, addr_col),
            wait_event,
        });
    }
    out
}

/// Parses a [`QueryResult`] returned by [`locks_query`] (or native engine lock views)
/// into normalized [`ServerLockRow`] entries.
#[must_use]
pub fn parse_locks_result(_engine: DatabaseEngine, set: &QueryResult) -> Vec<ServerLockRow> {
    if set.rows.is_empty() || set.columns.is_empty() {
        return Vec::new();
    }
    let cols = &set.columns;
    let pid_col = find_col(
        cols,
        &[
            "pid",
            "session_id",
            "request_session_id",
            "trx_mysql_thread_id",
            "id",
        ],
    )
    .or(Some(0));
    let type_col = find_col(cols, &["lock_type", "locktype", "resource_type", "type"]);
    let rel_col = find_col(
        cols,
        &[
            "relation",
            "table",
            "resource_associated_entity_id",
            "object",
        ],
    );
    let mode_col = find_col(cols, &["mode", "request_mode", "lock_mode", "trx_state"]);
    let granted_col = find_col(cols, &["granted", "is_granted", "request_status"]);
    let blocked_col = find_col(
        cols,
        &[
            "blocked_by",
            "blocking_pid",
            "blocking_pids",
            "blocking_session_id",
        ],
    );

    let mut out = Vec::with_capacity(set.rows.len());
    for row in &set.rows {
        let pid = cell_text(row, pid_col);
        if pid.is_empty() {
            continue;
        }
        let granted = granted_col
            .and_then(|idx| row.get(idx))
            .is_some_and(value_to_granted_bool);
        let blocked_by = cell_opt_text(row, blocked_col).filter(|s| s != "0");

        out.push(ServerLockRow {
            pid,
            lock_type: cell_text(row, type_col),
            relation: cell_text(row, rel_col),
            mode: cell_text(row, mode_col),
            granted,
            blocked_by,
        });
    }
    out
}

/// Builds a concise human-readable summary line for a [`ServerMonitorSnapshot`].
#[must_use]
pub fn format_snapshot_summary(
    engine: DatabaseEngine,
    sessions: &[LiveSessionRow],
    locks: &[ServerLockRow],
) -> String {
    if sessions_query(engine).is_none() {
        return format!(
            "Embedded engine '{}' does not expose server sessions or locks.",
            engine.as_str()
        );
    }
    let active_count = sessions
        .iter()
        .filter(|s| !s.state.is_empty() && !s.state.eq_ignore_ascii_case("idle"))
        .count();
    let waiting_locks = locks
        .iter()
        .filter(|l| !l.granted || l.blocked_by.is_some())
        .count();
    format!(
        "{}: {} session(s) ({} active), {} lock(s) ({} waiting)",
        engine.as_str(),
        sessions.len(),
        active_count,
        locks.len(),
        waiting_locks
    )
}

/// Fetches a live [`ServerMonitorSnapshot`] from an active [`Session`].
///
/// Embedded engines (`Sqlite`, `Duckdb`, `Generic`) return an empty snapshot with an
/// informational summary.
///
/// # Errors
///
/// Returns [`ServiceError::Session`] if executing the session or lock inspection query fails.
pub async fn fetch_server_monitor_snapshot(
    session: &Session,
    engine: DatabaseEngine,
) -> Result<ServerMonitorSnapshot, ServiceError> {
    let Some(sessions_sql) = sessions_query(engine) else {
        return Ok(ServerMonitorSnapshot {
            engine,
            sessions: Vec::new(),
            locks: Vec::new(),
            summary: format_snapshot_summary(engine, &[], &[]),
        });
    };

    let sessions_res = run_capture_query_result(session, sessions_sql).await?;
    let sessions = parse_sessions_result(engine, &sessions_res);

    let locks = match locks_query(engine) {
        Some(locks_sql) => {
            let locks_res = run_capture_query_result(session, locks_sql).await?;
            parse_locks_result(engine, &locks_res)
        }
        None => Vec::new(),
    };

    let summary = format_snapshot_summary(engine, &sessions, &locks);
    Ok(ServerMonitorSnapshot {
        engine,
        sessions,
        locks,
        summary,
    })
}

/// Validates `session_id` and terminates the corresponding session/query on `session`.
///
/// # Errors
///
/// Returns [`ServiceError::UnsupportedEngine`] for embedded engines, or
/// [`ServiceError::Session`] if `session_id` is invalid or execution fails.
pub async fn terminate_server_session(
    session: &Session,
    engine: DatabaseEngine,
    session_id: &str,
) -> Result<(), ServiceError> {
    let sql = kill_session_sql(engine, session_id)?;
    let _ = run_capture_query_result(session, &sql).await?;
    Ok(())
}

/// Executes `sql` in a short-lived editor session and collects the first result set.
async fn run_capture_query_result(
    session: &Session,
    sql: &str,
) -> Result<QueryResult, ServiceError> {
    let cancel = CancellationToken::new();
    let editor = session
        .open_editor_session(EditorSessionOptions::default(), &cancel)
        .await
        .map_err(|err| ServiceError::Session(err.to_string()))?;

    let state = Arc::new(Mutex::new(CapturedResult::default()));
    let sink = Box::new(CapturingSink {
        state: state.clone(),
    });
    let run_res = editor
        .run(UserSql::new(sql.to_owned()), RunOptions::new(cancel), sink)
        .await;
    editor.close().await;

    let summary = run_res.map_err(|err| ServiceError::Session(err.to_string()))?;
    match summary.end {
        RunEnd::Completed => {
            let guard = state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            Ok(QueryResult::new(guard.columns.clone(), guard.rows.clone()))
        }
        RunEnd::Failed(err) => Err(ServiceError::Session(err.to_string())),
        RunEnd::Cancelled => Err(ServiceError::Session("query cancelled".to_owned())),
        RunEnd::TimedOut { .. } => Err(ServiceError::Session("query timed out".to_owned())),
    }
}

#[derive(Default)]
struct CapturedResult {
    columns: Vec<ResultColumn>,
    rows: Vec<Vec<Value>>,
}

struct CapturingSink {
    state: Arc<Mutex<CapturedResult>>,
}

#[async_trait]
impl ResultSink for CapturingSink {
    async fn accept(&mut self, event: ResultEvent) -> SinkFlow {
        let mut guard = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        match event {
            ResultEvent::ResultSetStarted { columns }
            | ResultEvent::ColumnsResolved { columns, .. } => {
                guard.columns = columns.to_vec();
            }
            ResultEvent::Rows(batch) => {
                guard.rows.extend(batch);
            }
            _ => {}
        }
        SinkFlow::Continue
    }
}

fn find_col(columns: &[ResultColumn], candidates: &[&str]) -> Option<usize> {
    for candidate in candidates {
        if let Some(pos) = columns
            .iter()
            .position(|c| c.name.eq_ignore_ascii_case(candidate))
        {
            return Some(pos);
        }
    }
    None
}

fn find_duration_col(columns: &[ResultColumn]) -> (Option<usize>, bool) {
    if let Some(idx) = find_col(
        columns,
        &["duration_ms", "elapsed_ms", "total_elapsed_time"],
    ) {
        return (Some(idx), false);
    }
    if let Some(idx) = find_col(columns, &["duration_sec", "elapsed_sec", "elapsed", "time"]) {
        return (Some(idx), true);
    }
    (None, false)
}

fn cell_opt_text(row: &[Value], col: Option<usize>) -> Option<String> {
    let val = col.and_then(|idx| row.get(idx))?;
    if val.is_null() {
        return None;
    }
    let text = value_to_string(val);
    let trimmed = text.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_owned())
    }
}

fn cell_text(row: &[Value], col: Option<usize>) -> String {
    cell_opt_text(row, col).unwrap_or_default()
}

fn combine_wait_event(
    row: &[Value],
    wait_type_col: Option<usize>,
    wait_col: Option<usize>,
) -> Option<String> {
    let wait_event = cell_opt_text(row, wait_col);
    let wait_type = cell_opt_text(row, wait_type_col);
    match (wait_type, wait_event) {
        (Some(wt), Some(we)) if !we.contains(':') => Some(format!("{wt}:{we}")),
        (_, Some(we)) => Some(we),
        (Some(wt), None) => Some(wt),
        (None, None) => None,
    }
}

fn value_to_duration_ms(val: &Value, is_seconds: bool) -> Option<u64> {
    let scale = if is_seconds { 1000.0 } else { 1.0 };
    match val {
        Value::Int32(v) => u64::try_from(*v).ok().map(|n| {
            if is_seconds {
                n.saturating_mul(1000)
            } else {
                n
            }
        }),
        Value::Int(v) => u64::try_from(*v).ok().map(|n| {
            if is_seconds {
                n.saturating_mul(1000)
            } else {
                n
            }
        }),
        Value::UInt(v) => Some(if is_seconds {
            v.saturating_mul(1000)
        } else {
            *v
        }),
        Value::Float32(f) if f.is_finite() && *f >= 0.0 =>
        {
            #[expect(
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                reason = "checked finite and non-negative"
            )]
            Some((f64::from(*f) * scale).round() as u64)
        }
        Value::Float64(f) if f.is_finite() && *f >= 0.0 =>
        {
            #[expect(
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                reason = "checked finite and non-negative"
            )]
            Some((*f * scale).round() as u64)
        }
        Value::Decimal(d) => parse_duration_str(&d.to_string(), is_seconds),
        Value::Text(s) => parse_duration_str(s, is_seconds),
        _ => None,
    }
}

fn parse_duration_str(raw: &str, is_seconds: bool) -> Option<u64> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    if !is_seconds && let Ok(ms) = trimmed.parse::<u64>() {
        return Some(ms);
    }
    let parsed = trimmed.parse::<f64>().ok()?;
    if !parsed.is_finite() || parsed < 0.0 {
        return None;
    }
    let scale = if is_seconds { 1000.0 } else { 1.0 };
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "checked finite and non-negative"
    )]
    Some((parsed * scale).round() as u64)
}

fn value_to_granted_bool(val: &Value) -> bool {
    match val {
        Value::Bool(b) => *b,
        Value::Int32(n) => *n != 0,
        Value::Int(n) => *n != 0,
        Value::UInt(n) => *n != 0,
        Value::Text(s) => matches!(
            s.trim().to_ascii_lowercase().as_str(),
            "true" | "t" | "1" | "yes" | "y" | "grant" | "granted"
        ),
        _ => false,
    }
}

fn value_to_string(val: &Value) -> String {
    match val {
        Value::Null => String::new(),
        Value::Bool(b) => b.to_string(),
        Value::Int32(n) => n.to_string(),
        Value::Int(n) => n.to_string(),
        Value::UInt(n) => n.to_string(),
        Value::WideInt(w) => w.to_string(),
        Value::Float32(f) => f.to_string(),
        Value::Float64(f) => f.to_string(),
        Value::Decimal(d) => d.to_string(),
        Value::Text(s) => s.clone(),
        Value::Bytes(b) => format!("<{} bytes>", b.len()),
        Value::Uuid(u) => u.to_string(),
        Value::Json(j) => j.as_str().to_owned(),
        Value::Date(Temporal::Finite(d)) => d.format("%Y-%m-%d").to_string(),
        Value::Time(t) => t.format(None),
        Value::TimeTz(t) => format!("{}{}", t.time.format(None), t.offset),
        Value::Timestamp(Temporal::Finite(ts)) => ts.format("%Y-%m-%d %H:%M:%S").to_string(),
        Value::TimestampTz(Temporal::Finite(ts)) => ts.to_string(),
        Value::Date(Temporal::Infinity)
        | Value::Timestamp(Temporal::Infinity)
        | Value::TimestampTz(Temporal::Infinity) => "infinity".to_owned(),
        Value::Date(Temporal::NegInfinity)
        | Value::Timestamp(Temporal::NegInfinity)
        | Value::TimestampTz(Temporal::NegInfinity) => "-infinity".to_owned(),
        Value::Interval(i) => i.to_string(),
        Value::Bits(b) => b.to_string(),
        Value::Array(arr) => format!(
            "[{}]",
            arr.iter()
                .map(value_to_string)
                .collect::<Vec<_>>()
                .join(", ")
        ),
        Value::Other(o) => o.text.clone(),
        Value::Undecodable(u) => format!("<undecodable:{}>", u.type_name),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tablepro_core::column::ColumnType;
    use tablepro_core::credentials::CredentialInteraction;
    use tablepro_core::{
        ConnectEnvironment, FileEndpoint, LivenessPolicy, SafeModeLevel, WorkspaceSettings,
    };
    use tablepro_driver_sqlite::SqliteDriver;
    use tablepro_session::connect::{
        ConnectRequest, ConnectServices, ConnectionMetadata, Connector, DriverConnector,
        NoStoredCredentials, RequestTarget,
    };
    use tablepro_session::operation::DefaultDeadlines;
    use tablepro_session::runtime::Tasks;
    use tablepro_session::{Session, SessionDeps};
    use tablepro_ssh::InProcessSsh;
    use uuid::Uuid;

    fn text_col(name: &str) -> ResultColumn {
        ResultColumn::new(name, ColumnType::untyped_text("TEXT"))
    }

    #[test]
    fn query_templates_cover_server_engines_and_omit_embedded() {
        assert!(
            sessions_query(DatabaseEngine::Postgres)
                .is_some_and(|q| q.contains("pg_stat_activity") && q.contains("duration_ms"))
        );
        assert!(
            sessions_query(DatabaseEngine::Mysql)
                .is_some_and(|q| q.contains("information_schema.PROCESSLIST"))
        );
        assert!(
            sessions_query(DatabaseEngine::Mssql).is_some_and(|q| q
                .contains("sys.dm_exec_sessions")
                && q.contains("dm_exec_requests"))
        );
        assert!(
            sessions_query(DatabaseEngine::Clickhouse)
                .is_some_and(|q| q.contains("system.processes"))
        );
        assert!(sessions_query(DatabaseEngine::Sqlite).is_none());
        assert!(sessions_query(DatabaseEngine::Duckdb).is_none());
        assert!(sessions_query(DatabaseEngine::Generic).is_none());

        assert!(locks_query(DatabaseEngine::Postgres).is_some_and(|q| q.contains("pg_locks")));
        assert!(locks_query(DatabaseEngine::Mysql).is_some_and(|q| q.contains("innodb_trx")));
        assert!(locks_query(DatabaseEngine::Mssql).is_some_and(|q| q.contains("dm_tran_locks")));
        assert!(locks_query(DatabaseEngine::Clickhouse).is_none());
        assert!(locks_query(DatabaseEngine::Sqlite).is_none());
    }

    #[test]
    fn kill_session_sql_validates_and_sanitizes_session_ids() {
        assert_eq!(
            kill_session_sql(DatabaseEngine::Postgres, " 4210 ")
                .ok()
                .as_deref(),
            Some("SELECT pg_terminate_backend(4210);")
        );
        assert_eq!(
            kill_session_sql(DatabaseEngine::Mysql, "98")
                .ok()
                .as_deref(),
            Some("KILL 98;")
        );
        assert_eq!(
            kill_session_sql(DatabaseEngine::Mssql, "77")
                .ok()
                .as_deref(),
            Some("KILL 77;")
        );
        assert_eq!(
            kill_session_sql(
                DatabaseEngine::Clickhouse,
                "550e8400-e29b-41d4-a716-446655440000"
            )
            .ok()
            .as_deref(),
            Some("KILL QUERY WHERE query_id = '550e8400-e29b-41d4-a716-446655440000';")
        );

        let rejected = [
            "",
            "   ",
            "0",
            "-1",
            "+42",
            "123; DROP TABLE users;--",
            "123 OR 1=1",
            "12 34",
            "abc",
        ];
        for bad in rejected {
            assert!(
                kill_session_sql(DatabaseEngine::Postgres, bad).is_err(),
                "Postgres should reject '{bad}'"
            );
            assert!(
                kill_session_sql(DatabaseEngine::Mysql, bad).is_err(),
                "MySQL should reject '{bad}'"
            );
            assert!(
                kill_session_sql(DatabaseEngine::Mssql, bad).is_err(),
                "MSSQL should reject '{bad}'"
            );
        }

        let bad_clickhouse = [
            "",
            "   ",
            "q'1",
            "q\"1",
            "q; DROP TABLE t",
            "id with spaces",
            "id\\slash",
            "id=1",
        ];
        for bad in bad_clickhouse {
            assert!(
                kill_session_sql(DatabaseEngine::Clickhouse, bad).is_err(),
                "ClickHouse should reject '{bad}'"
            );
        }

        assert!(matches!(
            kill_session_sql(DatabaseEngine::Sqlite, "1"),
            Err(ServiceError::UnsupportedEngine(_))
        ));
        assert!(matches!(
            kill_session_sql(DatabaseEngine::Duckdb, "1"),
            Err(ServiceError::UnsupportedEngine(_))
        ));
    }

    #[test]
    fn parse_sessions_result_postgres() {
        let pg_set = QueryResult::new(
            vec![
                text_col("pid"),
                text_col("usename"),
                text_col("datname"),
                text_col("state"),
                text_col("duration_ms"),
                text_col("query"),
                text_col("client_addr"),
                text_col("wait_event_type"),
                text_col("wait_event"),
            ],
            vec![
                vec![
                    Value::Int32(101),
                    Value::Text("postgres".to_owned()),
                    Value::Text("app_db".to_owned()),
                    Value::Text("active".to_owned()),
                    Value::Int(1450),
                    Value::Text("SELECT * FROM orders".to_owned()),
                    Value::Text("10.0.0.5".to_owned()),
                    Value::Text("Lock".to_owned()),
                    Value::Text("relation".to_owned()),
                ],
                vec![
                    Value::Int32(102),
                    Value::Text("worker".to_owned()),
                    Value::Text("app_db".to_owned()),
                    Value::Text("idle".to_owned()),
                    Value::Null,
                    Value::Text("COMMIT".to_owned()),
                    Value::Null,
                    Value::Null,
                    Value::Null,
                ],
            ],
        );
        let pg_rows = parse_sessions_result(DatabaseEngine::Postgres, &pg_set);
        assert_eq!(pg_rows.len(), 2);
        assert_eq!(pg_rows[0].id, "101");
        assert_eq!(pg_rows[0].user, "postgres");
        assert_eq!(pg_rows[0].database, "app_db");
        assert_eq!(pg_rows[0].state, "active");
        assert_eq!(pg_rows[0].duration_ms, Some(1450));
        assert_eq!(pg_rows[0].client_addr.as_deref(), Some("10.0.0.5"));
        assert_eq!(pg_rows[0].wait_event.as_deref(), Some("Lock:relation"));
        assert_eq!(pg_rows[1].duration_ms, None);
        assert_eq!(pg_rows[1].client_addr, None);
        assert_eq!(pg_rows[1].wait_event, None);
    }

    #[test]
    fn parse_sessions_result_mysql_mssql_and_clickhouse() {
        let mysql_set = QueryResult::new(
            vec![
                text_col("Id"),
                text_col("User"),
                text_col("db"),
                text_col("State"),
                text_col("Time"),
                text_col("Info"),
                text_col("Host"),
            ],
            vec![vec![
                Value::UInt(88),
                Value::Text("root".to_owned()),
                Value::Text("billing".to_owned()),
                Value::Text("Sending data".to_owned()),
                Value::Int(3),
                Value::Text("SELECT COUNT(*) FROM invoices".to_owned()),
                Value::Text("127.0.0.1:51234".to_owned()),
            ]],
        );
        let mysql_rows = parse_sessions_result(DatabaseEngine::Mysql, &mysql_set);
        assert_eq!(mysql_rows.len(), 1);
        assert_eq!(mysql_rows[0].id, "88");
        assert_eq!(mysql_rows[0].duration_ms, Some(3000));

        let mssql_set = QueryResult::new(
            vec![
                text_col("session_id"),
                text_col("login_name"),
                text_col("database_name"),
                text_col("status"),
                text_col("total_elapsed_time"),
                text_col("text"),
                text_col("client_net_address"),
                text_col("wait_type"),
            ],
            vec![vec![
                Value::Int32(54),
                Value::Text("sa".to_owned()),
                Value::Text("master".to_owned()),
                Value::Text("suspended".to_owned()),
                Value::Int(812),
                Value::Text("UPDATE ledger SET balance = 0".to_owned()),
                Value::Text("192.168.1.20".to_owned()),
                Value::Text("LCK_M_X".to_owned()),
            ]],
        );
        let mssql_rows = parse_sessions_result(DatabaseEngine::Mssql, &mssql_set);
        assert_eq!(mssql_rows.len(), 1);
        assert_eq!(mssql_rows[0].id, "54");
        assert_eq!(mssql_rows[0].user, "sa");
        assert_eq!(mssql_rows[0].duration_ms, Some(812));
        assert_eq!(mssql_rows[0].wait_event.as_deref(), Some("LCK_M_X"));

        let ch_set = QueryResult::new(
            vec![
                text_col("query_id"),
                text_col("user"),
                text_col("current_database"),
                text_col("elapsed"),
                text_col("query"),
                text_col("address"),
            ],
            vec![vec![
                Value::Text("q-uuid-1".to_owned()),
                Value::Text("default".to_owned()),
                Value::Text("analytics".to_owned()),
                Value::Float64(1.25),
                Value::Text("SELECT count() FROM events".to_owned()),
                Value::Text("::1".to_owned()),
            ]],
        );
        let ch_rows = parse_sessions_result(DatabaseEngine::Clickhouse, &ch_set);
        assert_eq!(ch_rows.len(), 1);
        assert_eq!(ch_rows[0].state, "running");
        assert_eq!(ch_rows[0].duration_ms, Some(1250));
    }

    #[test]
    fn parse_locks_result_handles_boolean_numeric_and_text_granted_flags() {
        let pg_locks = QueryResult::new(
            vec![
                text_col("pid"),
                text_col("lock_type"),
                text_col("relation"),
                text_col("mode"),
                text_col("granted"),
                text_col("blocked_by"),
            ],
            vec![
                vec![
                    Value::Text("201".to_owned()),
                    Value::Text("relation".to_owned()),
                    Value::Text("public.orders".to_owned()),
                    Value::Text("AccessExclusiveLock".to_owned()),
                    Value::Bool(false),
                    Value::Text("199".to_owned()),
                ],
                vec![
                    Value::Text("199".to_owned()),
                    Value::Text("relation".to_owned()),
                    Value::Text("public.orders".to_owned()),
                    Value::Text("RowExclusiveLock".to_owned()),
                    Value::Bool(true),
                    Value::Null,
                ],
            ],
        );
        let parsed_pg = parse_locks_result(DatabaseEngine::Postgres, &pg_locks);
        assert_eq!(parsed_pg.len(), 2);
        assert!(!parsed_pg[0].granted);
        assert_eq!(parsed_pg[0].blocked_by.as_deref(), Some("199"));
        assert!(parsed_pg[1].granted);
        assert_eq!(parsed_pg[1].blocked_by, None);

        let mssql_locks = QueryResult::new(
            vec![
                text_col("request_session_id"),
                text_col("resource_type"),
                text_col("resource_associated_entity_id"),
                text_col("request_mode"),
                text_col("request_status"),
                text_col("blocking_session_id"),
            ],
            vec![vec![
                Value::Int(55),
                Value::Text("OBJECT".to_owned()),
                Value::Text("942626401".to_owned()),
                Value::Text("IX".to_owned()),
                Value::Text("GRANT".to_owned()),
                Value::Int(0),
            ]],
        );
        let parsed_mssql = parse_locks_result(DatabaseEngine::Mssql, &mssql_locks);
        assert_eq!(parsed_mssql.len(), 1);
        assert!(parsed_mssql[0].granted);
        assert_eq!(parsed_mssql[0].blocked_by, None);
    }

    #[test]
    fn format_snapshot_summary_counts_active_and_waiting() {
        let sessions = vec![
            LiveSessionRow {
                id: "1".to_owned(),
                user: "u".to_owned(),
                database: "d".to_owned(),
                state: "active".to_owned(),
                duration_ms: Some(200),
                query: "SELECT 1".to_owned(),
                client_addr: None,
                wait_event: None,
            },
            LiveSessionRow {
                id: "2".to_owned(),
                user: "u".to_owned(),
                database: "d".to_owned(),
                state: "idle".to_owned(),
                duration_ms: Some(10),
                query: String::new(),
                client_addr: None,
                wait_event: None,
            },
        ];
        let locks = vec![ServerLockRow {
            pid: "1".to_owned(),
            lock_type: "relation".to_owned(),
            relation: "users".to_owned(),
            mode: "ExclusiveLock".to_owned(),
            granted: false,
            blocked_by: Some("2".to_owned()),
        }];
        let summary = format_snapshot_summary(DatabaseEngine::Postgres, &sessions, &locks);
        assert_eq!(
            summary,
            "postgres: 2 session(s) (1 active), 1 lock(s) (1 waiting)"
        );
    }

    async fn open_temp_sqlite_session(dir: &std::path::Path) -> Result<Arc<Session>, ServiceError> {
        let db_path = dir.join("server_monitor_test.sqlite");
        let connector = Arc::new(DriverConnector::new(
            ConnectServices {
                environment: ConnectEnvironment::new("dataforge-services-test"),
                ssh: Arc::new(InProcessSsh::system()),
                liveness: LivenessPolicy::DESKTOP,
            },
            Arc::new(DefaultDeadlines::new(LivenessPolicy::DESKTOP)),
        ));
        let endpoint =
            FileEndpoint::create_new(db_path).map_err(|e| ServiceError::Session(e.to_string()))?;
        let request = ConnectRequest {
            metadata: ConnectionMetadata {
                id: Uuid::new_v4(),
                name: "sqlite-monitor-test".to_owned(),
                driver_id: "sqlite".to_owned(),
            },
            driver: Arc::new(SqliteDriver),
            target: RequestTarget::File(endpoint),
            workspace_settings: WorkspaceSettings::workspace(false),
            safe_mode: SafeModeLevel::Off,
        };
        let pending = connector
            .establish(
                request,
                CredentialInteraction::Unattended,
                CancellationToken::new(),
            )
            .await
            .map_err(|e| ServiceError::Session(e.to_string()))?;
        Ok(Session::start(
            pending,
            SessionDeps {
                connector,
                credentials: Arc::new(NoStoredCredentials),
                deadlines: Arc::new(DefaultDeadlines::new(LivenessPolicy::DESKTOP)),
                tasks: Tasks::new(tokio::runtime::Handle::current()),
                prompter: None,
            },
        ))
    }

    #[tokio::test]
    async fn live_sqlite_snapshot_and_query_capture_round_trip() -> Result<(), ServiceError> {
        let dir = tempfile::tempdir()?;
        let session = open_temp_sqlite_session(dir.path()).await?;

        let snapshot = fetch_server_monitor_snapshot(&session, DatabaseEngine::Sqlite).await?;
        assert_eq!(snapshot.engine, DatabaseEngine::Sqlite);
        assert!(snapshot.sessions.is_empty());
        assert!(snapshot.locks.is_empty());
        assert!(snapshot.summary.contains("sqlite"));

        let kill_err = terminate_server_session(&session, DatabaseEngine::Sqlite, "1").await;
        assert!(matches!(kill_err, Err(ServiceError::UnsupportedEngine(_))));

        // Exercise live SQLite execution + ResultSet -> LiveSessionRow / ServerLockRow parsing.
        let live_sessions_set = run_capture_query_result(
            &session,
            "SELECT 42 AS id, 'alice' AS username, 'main' AS database_name, 'active' AS state, \
             250 AS duration_ms, 'SELECT 1' AS query, '127.0.0.1' AS client_addr, NULL AS wait_event;",
        )
        .await?;
        let parsed_sessions = parse_sessions_result(DatabaseEngine::Postgres, &live_sessions_set);
        assert_eq!(parsed_sessions.len(), 1);
        assert_eq!(parsed_sessions[0].id, "42");
        assert_eq!(parsed_sessions[0].user, "alice");
        assert_eq!(parsed_sessions[0].duration_ms, Some(250));
        assert_eq!(parsed_sessions[0].client_addr.as_deref(), Some("127.0.0.1"));
        assert_eq!(parsed_sessions[0].wait_event, None);

        let live_locks_set = run_capture_query_result(
            &session,
            "SELECT 42 AS pid, 'table' AS lock_type, 'orders' AS relation, \
             'Exclusive' AS mode, 0 AS granted, '17' AS blocked_by;",
        )
        .await?;
        let parsed_locks = parse_locks_result(DatabaseEngine::Postgres, &live_locks_set);
        assert_eq!(parsed_locks.len(), 1);
        assert_eq!(parsed_locks[0].pid, "42");
        assert!(!parsed_locks[0].granted);
        assert_eq!(parsed_locks[0].blocked_by.as_deref(), Some("17"));

        let _ = session.begin_shutdown().await;
        Ok(())
    }
}
