//! Server administration SQL query templates and scheduled task evaluation helpers (Feature 12).

use serde::{Deserialize, Serialize};

use crate::ddl_builder::DatabaseEngine;

/// Returns a dialect-specific read-only SQL query to list server users and roles,
/// or `None` for embedded/file engines without server user catalogs.
#[must_use]
pub const fn users_roles_query(engine: DatabaseEngine) -> Option<&'static str> {
    match engine {
        DatabaseEngine::Postgres => Some(
            "SELECT rolname AS role_name, rolsuper AS is_superuser, rolcanlogin AS can_login, \
             rolcreatedb AS can_create_db, rolcreaterole AS can_create_role, \
             rolconnlimit AS connection_limit \
             FROM pg_roles ORDER BY rolname;",
        ),
        DatabaseEngine::Mysql => Some(
            "SELECT User AS username, Host AS host, account_locked, password_expired \
             FROM mysql.user ORDER BY User, Host;",
        ),
        DatabaseEngine::Mssql => Some(
            "SELECT name, type_desc, is_disabled, create_date \
             FROM sys.server_principals \
             WHERE type IN ('S', 'U', 'G', 'R') ORDER BY name;",
        ),
        DatabaseEngine::Clickhouse => Some(
            "SELECT name, storage, auth_type, default_roles_all \
             FROM system.users ORDER BY name;",
        ),
        DatabaseEngine::Sqlite | DatabaseEngine::Duckdb | DatabaseEngine::Generic => None,
    }
}

/// Returns a dialect-specific read-only SQL query to inspect active server sessions/queries,
/// or `None` for embedded/file engines.
#[must_use]
pub const fn active_sessions_query(engine: DatabaseEngine) -> Option<&'static str> {
    match engine {
        DatabaseEngine::Postgres => Some(
            "SELECT pid, usename AS username, datname AS database_name, client_addr, state, \
             wait_event_type, query_start, left(query, 200) AS query \
             FROM pg_stat_activity \
             WHERE state IS NOT NULL \
             ORDER BY query_start DESC NULLS LAST;",
        ),
        DatabaseEngine::Mysql => Some(
            "SELECT Id AS pid, User AS username, Host AS host, db AS database_name, \
             Command AS command, Time AS duration_sec, State AS state, Info AS query \
             FROM information_schema.PROCESSLIST ORDER BY Time DESC;",
        ),
        DatabaseEngine::Mssql => Some(
            "SELECT s.session_id AS pid, s.login_name AS username, \
             DB_NAME(s.database_id) AS database_name, s.status, r.command, \
             r.wait_type, r.total_elapsed_time AS elapsed_ms \
             FROM sys.dm_exec_sessions s \
             LEFT JOIN sys.dm_exec_requests r ON s.session_id = r.session_id \
             WHERE s.is_user_process = 1 ORDER BY s.session_id;",
        ),
        DatabaseEngine::Clickhouse => Some(
            "SELECT query_id AS pid, user AS username, address AS client_addr, \
             elapsed AS elapsed_sec, read_rows, memory_usage, query \
             FROM system.processes ORDER BY elapsed DESC;",
        ),
        DatabaseEngine::Sqlite | DatabaseEngine::Duckdb | DatabaseEngine::Generic => None,
    }
}

/// Generates a dialect-specific SQL statement to terminate/kill an active session or query.
///
/// Validates numeric session IDs for PostgreSQL, MySQL, and SQL Server to prevent injection,
/// and escapes string query IDs for ClickHouse. Returns `None` if unsupported or invalid.
#[must_use]
pub fn kill_session_sql(engine: DatabaseEngine, pid_or_id: &str) -> Option<String> {
    let trimmed = pid_or_id.trim();
    if trimmed.is_empty() {
        return None;
    }
    match engine {
        DatabaseEngine::Postgres => {
            let pid = trimmed.parse::<u64>().ok()?;
            Some(format!("SELECT pg_terminate_backend({pid});"))
        }
        DatabaseEngine::Mysql | DatabaseEngine::Mssql => {
            let pid = trimmed.parse::<u64>().ok()?;
            Some(format!("KILL {pid};"))
        }
        DatabaseEngine::Clickhouse => Some(format!(
            "KILL QUERY WHERE query_id = {};",
            engine.quote_literal(trimmed)
        )),
        DatabaseEngine::Sqlite | DatabaseEngine::Duckdb | DatabaseEngine::Generic => None,
    }
}

/// Minimal scheduling metadata required to evaluate whether a recurring task is due.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScheduledTaskSchedule {
    pub id: String,
    pub enabled: bool,
    pub last_run_unix: Option<i64>,
    pub interval_minutes: u32,
}

/// Returns `true` when a task with `last_run_unix` and `interval_minutes` should run at `now_unix`.
#[must_use]
pub const fn should_task_run_now(
    last_run_unix: Option<i64>,
    interval_minutes: u32,
    now_unix: i64,
) -> bool {
    if interval_minutes == 0 {
        return false;
    }
    let Some(last) = last_run_unix else {
        return true;
    };
    let interval_secs = (interval_minutes as i64).saturating_mul(60);
    now_unix.saturating_sub(last) >= interval_secs
}

/// Evaluates a slice of [`ScheduledTaskSchedule`] entries and returns the IDs of enabled tasks that are due.
#[must_use]
pub fn evaluate_due_tasks(tasks: &[ScheduledTaskSchedule], now_unix: i64) -> Vec<String> {
    tasks
        .iter()
        .filter(|t| t.enabled && should_task_run_now(t.last_run_unix, t.interval_minutes, now_unix))
        .map(|t| t.id.clone())
        .collect()
}

/// Generic index-based variant of [`evaluate_due_tasks`] for arbitrary caller task structs.
#[must_use]
pub fn evaluate_due_task_indices<T, F>(tasks: &[T], now_unix: i64, mut inspect: F) -> Vec<usize>
where
    F: FnMut(&T) -> (bool, Option<i64>, u32),
{
    tasks
        .iter()
        .enumerate()
        .filter_map(|(idx, item)| {
            let (enabled, last_run, interval_minutes) = inspect(item);
            (enabled && should_task_run_now(last_run, interval_minutes, now_unix)).then_some(idx)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn admin_queries_cover_server_engines_and_omit_embedded() {
        assert!(
            users_roles_query(DatabaseEngine::Postgres).is_some_and(|q| q.contains("pg_roles"))
        );
        assert!(users_roles_query(DatabaseEngine::Mysql).is_some_and(|q| q.contains("mysql.user")));
        assert!(
            users_roles_query(DatabaseEngine::Mssql)
                .is_some_and(|q| q.contains("server_principals"))
        );
        assert!(users_roles_query(DatabaseEngine::Sqlite).is_none());
        assert!(users_roles_query(DatabaseEngine::Duckdb).is_none());

        assert!(
            active_sessions_query(DatabaseEngine::Postgres)
                .is_some_and(|q| q.contains("pg_stat_activity"))
        );
        assert!(
            active_sessions_query(DatabaseEngine::Mysql).is_some_and(|q| q.contains("PROCESSLIST"))
        );
        assert!(
            active_sessions_query(DatabaseEngine::Mssql)
                .is_some_and(|q| q.contains("dm_exec_sessions"))
        );
        assert!(
            active_sessions_query(DatabaseEngine::Clickhouse)
                .is_some_and(|q| q.contains("system.processes"))
        );
        assert!(active_sessions_query(DatabaseEngine::Sqlite).is_none());
    }

    #[test]
    fn kill_session_validates_pid_and_quotes_query_id() {
        assert_eq!(
            kill_session_sql(DatabaseEngine::Postgres, "1234").as_deref(),
            Some("SELECT pg_terminate_backend(1234);")
        );
        assert_eq!(
            kill_session_sql(DatabaseEngine::Mysql, "42").as_deref(),
            Some("KILL 42;")
        );
        assert_eq!(
            kill_session_sql(DatabaseEngine::Mssql, "99").as_deref(),
            Some("KILL 99;")
        );
        assert_eq!(
            kill_session_sql(DatabaseEngine::Postgres, "1234; DROP TABLE users"),
            None
        );
        assert_eq!(
            kill_session_sql(DatabaseEngine::Clickhouse, "q'1").as_deref(),
            Some("KILL QUERY WHERE query_id = 'q''1';")
        );
        assert_eq!(kill_session_sql(DatabaseEngine::Sqlite, "1"), None);
    }

    #[test]
    fn evaluate_due_tasks_respects_enabled_and_intervals() {
        let now = 10_000;
        let tasks = vec![
            ScheduledTaskSchedule {
                id: "never-ran".to_owned(),
                enabled: true,
                last_run_unix: None,
                interval_minutes: 15,
            },
            ScheduledTaskSchedule {
                id: "due".to_owned(),
                enabled: true,
                last_run_unix: Some(now - 900),
                interval_minutes: 15,
            },
            ScheduledTaskSchedule {
                id: "not-yet".to_owned(),
                enabled: true,
                last_run_unix: Some(now - 300),
                interval_minutes: 15,
            },
            ScheduledTaskSchedule {
                id: "disabled".to_owned(),
                enabled: false,
                last_run_unix: None,
                interval_minutes: 5,
            },
            ScheduledTaskSchedule {
                id: "zero-interval".to_owned(),
                enabled: true,
                last_run_unix: None,
                interval_minutes: 0,
            },
        ];

        assert_eq!(evaluate_due_tasks(&tasks, now), vec!["never-ran", "due"]);
        assert_eq!(
            evaluate_due_task_indices(&tasks, now, |t| (
                t.enabled,
                t.last_run_unix,
                t.interval_minutes
            )),
            vec![0, 1]
        );
    }
}
