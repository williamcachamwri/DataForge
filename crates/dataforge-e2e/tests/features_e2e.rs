//! End-to-end & edge-case tests for Wave 3 features:
//! 1. Mock Data Generator (seeding, all value kinds, null ratios, escaping, dialects, live FK hierarchy)
//! 2. Cross-Engine Data Transfer (SQLite <-> DuckDB, SQLite -> SQLite, 2,500+ rows, modes, mapping, cancel)
//! 3. Live Server Monitor (dialect query builders, validation errors, live SQLite/DuckDB snapshots)
//! 4. `WindowControlsStyle` & `AppSettings` serialization round-trips.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use dataforge_e2e::{E2eResult, open_file_session, run_sql, shutdown_session};
use dataforge_services::ServiceError;
use dataforge_services::data_transfer::{
    ColumnTypeMapping, TransferConfig, TransferMode, build_target_create_table_ddl,
    format_sql_literal, map_sql_type, transfer_table, transfer_table_cancellable,
};
use dataforge_services::ddl_builder::DatabaseEngine;
use dataforge_services::mock_generator::{
    MockColumnRule, MockGenerationPlan, MockValueKind, SeededRng, build_mock_insert_batches,
    escape_sql_literal, execute_mock_generation, format_value_as_sql_literal,
    infer_plan_from_table, infer_rule_from_column,
};
use dataforge_services::server_monitor::{
    ServerLockRow, fetch_server_monitor_snapshot, format_snapshot_summary, kill_session_sql,
    locks_query, parse_locks_result, parse_sessions_result, sessions_query,
    terminate_server_session, validate_session_id,
};
use dataforge_windows_platform::{AppSettings, WindowControlsStyle};
use tablepro_core::column::{
    BoolStorage, CatalogType, ColumnDefault, ColumnInfo, ColumnKind, ColumnType, IntegerKind,
    IntegerWidth, ReadForm, ResultColumn, SqlTypeExpr, TemporalZone, TextLength,
};
use tablepro_core::ddl::{
    ColumnAttributes, ColumnDefinition, PrimaryKeyConstraint, ReferentialAction, TableDefinition,
};
use tablepro_core::foreign_key_info::ForeignKeyInfo;
use tablepro_core::meta::TableRef;
use tablepro_core::value::{Temporal, Value};
use tablepro_core::{QueryResult, RunEnd};
use tablepro_driver_duckdb::DuckDbDriver;
use tablepro_driver_sqlite::SqliteDriver;
use tablepro_session::Session;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

async fn open_sqlite_session(dir: &Path, file_name: &str) -> E2eResult<Arc<Session>> {
    let db_path = dir.join(file_name);
    open_file_session(dir, &db_path, Arc::new(SqliteDriver), "sqlite", true, false).await
}

async fn open_duckdb_session(dir: &Path, file_name: &str) -> E2eResult<Arc<Session>> {
    let db_path = dir.join(file_name);
    let conn = duckdb::Connection::open(&db_path)?;
    conn.execute_batch("CHECKPOINT")?;
    drop(conn);
    open_file_session(
        dir,
        &db_path,
        Arc::new(DuckDbDriver),
        "duckdb",
        false,
        false,
    )
    .await
}

async fn exec_sql_rows(session: &Session, sql: &str) -> E2eResult<Vec<Vec<Value>>> {
    let out = run_sql(session, sql).await?;
    if !matches!(out.summary.end, RunEnd::Completed) {
        return Err(format!("SQL execution failed: {:?}", out.summary.end).into());
    }
    Ok(out
        .accumulator
        .result_sets
        .into_iter()
        .next()
        .map(|rs| rs.rows)
        .unwrap_or_default())
}

fn rule(
    column_name: &str,
    sql_type: &str,
    nullable: bool,
    null_ratio: f32,
    kind: MockValueKind,
) -> MockColumnRule {
    MockColumnRule {
        column_name: column_name.to_owned(),
        sql_type: sql_type.to_owned(),
        nullable,
        null_ratio,
        kind,
    }
}

fn col_def(
    name: &str,
    sql_type: &str,
    kind: ColumnKind,
    nullable: bool,
    pk: bool,
    auto_inc: bool,
) -> ColumnDefinition {
    ColumnDefinition::new(
        ColumnInfo {
            name: name.to_owned(),
            column_type: ColumnType {
                name: sql_type.to_owned(),
                kind,
                catalog: Some(CatalogType {
                    declaration: SqlTypeExpr::from_catalog_text(sql_type),
                    cast_target: SqlTypeExpr::from_catalog_text(sql_type),
                }),
                dynamic_storage: false,
                read_form: ReadForm::Native,
            },
            nullable,
            primary_key: pk,
            is_auto_increment: auto_inc,
            is_generated: false,
            partition_scoped: false,
            default: ColumnDefault::None,
            comment: None,
        },
        ColumnAttributes::default(),
    )
}

fn all_kinds_plan(seed: u64, row_count: usize, batch_size: usize) -> MockGenerationPlan {
    MockGenerationPlan {
        schema: None,
        table: "all_kinds".to_owned(),
        row_count,
        batch_size,
        seed,
        rules: vec![
            rule("id", "INTEGER", false, 0.0, MockValueKind::AutoIncrement),
            rule("uid", "TEXT", false, 1.0, MockValueKind::Uuid),
            rule("email", "TEXT", false, 0.5, MockValueKind::Email),
            rule("full_name", "TEXT", false, 0.0, MockValueKind::FullName),
            rule("company", "TEXT", false, 0.0, MockValueKind::Company),
            rule("phone", "TEXT", false, 0.0, MockValueKind::Phone),
            rule(
                "age",
                "INTEGER",
                false,
                0.0,
                MockValueKind::IntRange { min: 18, max: 65 },
            ),
            rule(
                "ratio",
                "REAL",
                false,
                0.0,
                MockValueKind::FloatRange { min: 0.1, max: 0.9 },
            ),
            rule("active", "BOOLEAN", false, 0.0, MockValueKind::Bool),
            rule(
                "created_at",
                "TEXT",
                false,
                0.0,
                MockValueKind::TimestampIso,
            ),
            rule("meta", "TEXT", false, 0.0, MockValueKind::JsonObject),
            rule("notes", "TEXT", true, 1.0, MockValueKind::LoremText),
            rule(
                "tier",
                "TEXT",
                false,
                0.0,
                MockValueKind::EnumValues(vec![
                    "free".to_owned(),
                    "pro's".to_owned(),
                    "enterprise".to_owned(),
                ]),
            ),
            rule(
                "parent_id",
                "INTEGER",
                false,
                0.0,
                MockValueKind::ForeignKeySample {
                    ref_table: "parents".to_owned(),
                    ref_column: "id".to_owned(),
                },
            ),
        ],
    }
}

#[test]
fn mock_generator_seeding_all_kinds_null_rules_and_escaping() -> E2eResult<()> {
    let mut rng_a = SeededRng::new(9876);
    let mut rng_b = SeededRng::new(9876);
    let mut rng_c = SeededRng::new(9877);
    let seq_a: Vec<u64> = (0..12).map(|_| rng_a.next_u64()).collect();
    let seq_b: Vec<u64> = (0..12).map(|_| rng_b.next_u64()).collect();
    let seq_c: Vec<u64> = (0..12).map(|_| rng_c.next_u64()).collect();
    assert_eq!(seq_a, seq_b);
    assert_ne!(seq_a, seq_c);

    let mut fk_pools = HashMap::new();
    fk_pools.insert(
        ("parents".to_owned(), "id".to_owned()),
        vec![Value::Int(10), Value::Int(20), Value::Int(30)],
    );

    let plan_a = all_kinds_plan(4242, 16, 8);
    let plan_b = all_kinds_plan(4242, 16, 8);
    let plan_c = all_kinds_plan(4243, 16, 8);
    let batches_a = build_mock_insert_batches(DatabaseEngine::Postgres, &plan_a, &fk_pools)?;
    let batches_b = build_mock_insert_batches(DatabaseEngine::Postgres, &plan_b, &fk_pools)?;
    let batches_c = build_mock_insert_batches(DatabaseEngine::Postgres, &plan_c, &fk_pools)?;

    assert_eq!(batches_a, batches_b);
    assert_ne!(batches_a, batches_c);
    assert_eq!(batches_a.len(), 2);

    // AutoIncrement column "id" must be omitted from INSERT columns.
    assert!(!batches_a[0].contains("(\"id\""));
    assert!(batches_a[0].contains("(\"uid\", \"email\", \"full_name\""));
    // Non-nullable columns ignore null_ratio == 1.0, whereas nullable with 1.0 always emits NULL.
    for batch in &batches_a {
        for tuple_line in batch.lines().skip(1) {
            assert!(!tuple_line.starts_with("  (NULL,"), "{tuple_line}");
            assert!(tuple_line.contains(", NULL, "), "{tuple_line}");
        }
    }

    // Quote, backslash, and NUL escaping across dialects.
    assert_eq!(
        escape_sql_literal(DatabaseEngine::Postgres, "O'Brien\\path\0"),
        "'O''Brien\\path'"
    );
    assert_eq!(
        escape_sql_literal(DatabaseEngine::Sqlite, "a'b\\c"),
        "'a''b\\c'"
    );
    assert_eq!(
        escape_sql_literal(DatabaseEngine::Duckdb, "a'b\\c"),
        "'a''b\\c'"
    );
    assert_eq!(
        escape_sql_literal(DatabaseEngine::Mssql, "a'b\\c"),
        "'a''b\\c'"
    );
    assert_eq!(
        escape_sql_literal(DatabaseEngine::Mysql, "a'b\\c"),
        "'a''b\\\\c'"
    );
    assert_eq!(
        escape_sql_literal(DatabaseEngine::Clickhouse, "a'b\\c"),
        "'a''b\\\\c'"
    );
    assert_eq!(
        format_value_as_sql_literal(DatabaseEngine::Sqlite, &Value::Text("x'y".to_owned())),
        "'x''y'"
    );
    Ok(())
}

#[test]
fn mock_generator_dialects_and_mssql_1000_row_cap() -> E2eResult<()> {
    let plan = MockGenerationPlan {
        schema: Some("analytics".to_owned()),
        table: "events".to_owned(),
        row_count: 2505,
        batch_size: 2500,
        seed: 777,
        rules: vec![
            rule("id", "INTEGER", false, 0.0, MockValueKind::AutoIncrement),
            rule("flag", "BOOLEAN", false, 0.0, MockValueKind::Bool),
            rule("tag", "TEXT", false, 0.0, MockValueKind::Company),
        ],
    };

    let mssql = build_mock_insert_batches(DatabaseEngine::Mssql, &plan, &HashMap::new())?;
    assert_eq!(mssql.len(), 3, "MSSQL must cap VALUES at 1000 rows");
    assert!(mssql[0].starts_with("INSERT INTO [analytics].[events] ([flag], [tag]) VALUES"));
    assert_eq!(mssql[0].lines().count(), 1001);
    assert_eq!(mssql[1].lines().count(), 1001);
    assert_eq!(mssql[2].lines().count(), 506);

    let sqlite = build_mock_insert_batches(DatabaseEngine::Sqlite, &plan, &HashMap::new())?;
    assert_eq!(sqlite.len(), 2);
    assert!(sqlite[0].starts_with("INSERT INTO \"analytics\".\"events\" (\"flag\", \"tag\")"));

    let duckdb = build_mock_insert_batches(DatabaseEngine::Duckdb, &plan, &HashMap::new())?;
    assert_eq!(duckdb.len(), 2);
    assert!(duckdb[0].starts_with("INSERT INTO \"analytics\".\"events\" (\"flag\", \"tag\")"));

    let postgres = build_mock_insert_batches(DatabaseEngine::Postgres, &plan, &HashMap::new())?;
    assert_eq!(postgres.len(), 2);
    assert!(postgres[0].starts_with("INSERT INTO \"analytics\".\"events\" (\"flag\", \"tag\")"));

    let mysql = build_mock_insert_batches(DatabaseEngine::Mysql, &plan, &HashMap::new())?;
    assert_eq!(mysql.len(), 2);
    assert!(mysql[0].starts_with("INSERT INTO `analytics`.`events` (`flag`, `tag`)"));

    let clickhouse = build_mock_insert_batches(DatabaseEngine::Clickhouse, &plan, &HashMap::new())?;
    assert_eq!(clickhouse.len(), 2);
    assert!(clickhouse[0].starts_with("INSERT INTO \"analytics\".\"events\" (\"flag\", \"tag\")"));
    Ok(())
}

#[tokio::test]
async fn mock_generator_edge_cases_zero_rows_empty_rules_and_fk_errors() -> E2eResult<()> {
    let empty_rules_plan = MockGenerationPlan {
        schema: None,
        table: "items".to_owned(),
        row_count: 5,
        batch_size: 5,
        seed: 1,
        rules: Vec::new(),
    };
    let err = build_mock_insert_batches(DatabaseEngine::Sqlite, &empty_rules_plan, &HashMap::new());
    assert!(matches!(err, Err(ServiceError::Runtime(_))));

    let empty_table_plan = MockGenerationPlan {
        table: "   ".to_owned(),
        rules: vec![rule("a", "TEXT", false, 0.0, MockValueKind::LoremText)],
        ..empty_rules_plan.clone()
    };
    assert!(matches!(
        build_mock_insert_batches(DatabaseEngine::Sqlite, &empty_table_plan, &HashMap::new()),
        Err(ServiceError::Runtime(_))
    ));

    let auto_only_plan = MockGenerationPlan {
        row_count: 3,
        rules: vec![rule(
            "id",
            "INTEGER",
            false,
            0.0,
            MockValueKind::AutoIncrement,
        )],
        ..empty_rules_plan.clone()
    };
    let sqlite_default =
        build_mock_insert_batches(DatabaseEngine::Sqlite, &auto_only_plan, &HashMap::new())?;
    assert_eq!(sqlite_default.len(), 3);
    assert!(sqlite_default[0].contains("DEFAULT VALUES;"));
    let mysql_default =
        build_mock_insert_batches(DatabaseEngine::Mysql, &auto_only_plan, &HashMap::new())?;
    assert_eq!(mysql_default.len(), 3);
    assert!(mysql_default[0].contains("() VALUES ();"));

    // 0-row live execution returns inserted_rows = 0, batches_executed = 0.
    let dir = tempfile::tempdir()?;
    let session = open_sqlite_session(dir.path(), "mock_edges.sqlite").await?;
    exec_sql_rows(
        &session,
        "CREATE TABLE parents (id INTEGER PRIMARY KEY);
         CREATE TABLE items (id INTEGER PRIMARY KEY, parent_id INTEGER REFERENCES parents(id));",
    )
    .await?;

    let zero_plan = MockGenerationPlan {
        schema: None,
        table: "items".to_owned(),
        row_count: 0,
        batch_size: 10,
        seed: 1,
        rules: vec![rule(
            "parent_id",
            "INTEGER",
            false,
            0.0,
            MockValueKind::ForeignKeySample {
                ref_table: "parents".to_owned(),
                ref_column: "id".to_owned(),
            },
        )],
    };
    let zero_batches =
        build_mock_insert_batches(DatabaseEngine::Sqlite, &zero_plan, &HashMap::new())?;
    assert!(zero_batches.is_empty());
    let zero_report = execute_mock_generation(&session, &zero_plan).await?;
    assert_eq!(zero_report.inserted_rows, 0);
    assert_eq!(zero_report.batches_executed, 0);

    // Non-null FK with empty parent table errors; nullable 100% null FK succeeds.
    let non_null_fk = MockGenerationPlan {
        row_count: 4,
        ..zero_plan.clone()
    };
    assert!(
        execute_mock_generation(&session, &non_null_fk)
            .await
            .is_err()
    );

    let always_null_fk = MockGenerationPlan {
        row_count: 4,
        rules: vec![rule(
            "parent_id",
            "INTEGER",
            true,
            1.0,
            MockValueKind::ForeignKeySample {
                ref_table: "parents".to_owned(),
                ref_column: "id".to_owned(),
            },
        )],
        ..zero_plan
    };
    let null_report = execute_mock_generation(&session, &always_null_fk).await?;
    assert_eq!(null_report.inserted_rows, 4);

    shutdown_session(&session).await?;
    Ok(())
}

fn i64_kind() -> ColumnKind {
    ColumnKind::Integer(IntegerKind {
        width: IntegerWidth::W64,
        signed: true,
    })
}

fn build_tasks_table_definition() -> TableDefinition {
    let text_kind = ColumnKind::Text {
        length: TextLength::Unbounded,
    };
    TableDefinition {
        table: TableRef::unqualified("tasks"),
        columns: vec![
            col_def("id", "INTEGER", i64_kind(), false, true, true),
            col_def("employee_id", "INTEGER", i64_kind(), false, false, false),
            col_def("task_uuid", "TEXT", ColumnKind::Uuid, false, false, false),
            col_def("contact_email", "TEXT", text_kind, false, false, false),
            col_def(
                "created_at",
                "TEXT",
                ColumnKind::Timestamp {
                    fractional_digits: None,
                    zone: TemporalZone::Naive,
                },
                false,
                false,
                false,
            ),
            col_def(
                "payload_json",
                "TEXT",
                ColumnKind::Json { binary: false },
                false,
                false,
                false,
            ),
        ],
        primary_key: Some(PrimaryKeyConstraint {
            name: None,
            columns: vec!["id".to_owned()],
        }),
        indexes: Vec::new(),
        foreign_keys: vec![ForeignKeyInfo {
            name: "fk_tasks_employee".to_owned(),
            columns: vec!["employee_id".to_owned()],
            ref_schema: None,
            ref_table: "employees".to_owned(),
            ref_columns: vec!["id".to_owned()],
            on_delete: Some(ReferentialAction::Cascade),
            on_update: None,
        }],
        inbound_foreign_keys: Vec::new(),
        source: None,
        options: None,
    }
}

fn validate_generated_task_rows(rows: &[Vec<Value>]) -> E2eResult<()> {
    assert_eq!(rows.len(), 60);
    for row in rows {
        let uid = match row.get(1) {
            Some(Value::Text(s)) => s,
            other => return Err(format!("expected UUID text, got {other:?}").into()),
        };
        Uuid::parse_str(uid)?;

        let email = match row.get(2) {
            Some(Value::Text(s)) => s,
            other => return Err(format!("expected email text, got {other:?}").into()),
        };
        assert!(
            email.contains('@') && email.contains('.'),
            "bad email: {email}"
        );

        let ts = match row.get(3) {
            Some(Value::Text(s)) => s,
            other => return Err(format!("expected ISO timestamp text, got {other:?}").into()),
        };
        chrono::DateTime::parse_from_rfc3339(ts)?;

        let json_raw = match row.get(4) {
            Some(Value::Text(s)) => s,
            other => return Err(format!("expected JSON text, got {other:?}").into()),
        };
        let parsed: serde_json::Value = serde_json::from_str(json_raw)?;
        assert!(parsed.is_object(), "expected JSON object: {json_raw}");

        let priority = match row.get(5) {
            Some(Value::Int(n)) => *n,
            other => return Err(format!("expected int priority, got {other:?}").into()),
        };
        assert!(
            (1..=5).contains(&priority),
            "priority out of range: {priority}"
        );
    }
    Ok(())
}

#[tokio::test]
#[expect(clippy::too_many_lines, reason = "end-to-end scenario test")]
async fn mock_generator_live_sqlite_three_level_fk_hierarchy() -> E2eResult<()> {
    let dir = tempfile::tempdir()?;
    let session = open_sqlite_session(dir.path(), "mock_hierarchy.sqlite").await?;
    exec_sql_rows(
        &session,
        "PRAGMA foreign_keys = ON;
         CREATE TABLE departments (
             id INTEGER PRIMARY KEY AUTOINCREMENT,
             name TEXT NOT NULL,
             code TEXT NOT NULL
         );
         CREATE TABLE employees (
             id INTEGER PRIMARY KEY AUTOINCREMENT,
             department_id INTEGER NOT NULL REFERENCES departments(id),
             full_name TEXT NOT NULL,
             email TEXT NOT NULL,
             phone TEXT NOT NULL,
             salary REAL NOT NULL
         );
         CREATE TABLE tasks (
             id INTEGER PRIMARY KEY AUTOINCREMENT,
             employee_id INTEGER NOT NULL REFERENCES employees(id),
             task_uuid TEXT NOT NULL,
             contact_email TEXT NOT NULL,
             created_at TEXT NOT NULL,
             payload_json TEXT NOT NULL,
             priority INTEGER NOT NULL
         );",
    )
    .await?;

    let dept_plan = MockGenerationPlan {
        schema: None,
        table: "departments".to_owned(),
        row_count: 8,
        batch_size: 4,
        seed: 100,
        rules: vec![
            rule("id", "INTEGER", false, 0.0, MockValueKind::AutoIncrement),
            rule("name", "TEXT", false, 0.0, MockValueKind::Company),
            rule(
                "code",
                "TEXT",
                false,
                0.0,
                MockValueKind::EnumValues(vec![
                    "ENG".to_owned(),
                    "OPS".to_owned(),
                    "FIN".to_owned(),
                ]),
            ),
        ],
    };
    let emp_plan = MockGenerationPlan {
        schema: None,
        table: "employees".to_owned(),
        row_count: 24,
        batch_size: 10,
        seed: 200,
        rules: vec![
            rule("id", "INTEGER", false, 0.0, MockValueKind::AutoIncrement),
            rule(
                "department_id",
                "INTEGER",
                false,
                0.0,
                MockValueKind::ForeignKeySample {
                    ref_table: "departments".to_owned(),
                    ref_column: "id".to_owned(),
                },
            ),
            rule("full_name", "TEXT", false, 0.0, MockValueKind::FullName),
            rule("email", "TEXT", false, 0.0, MockValueKind::Email),
            rule("phone", "TEXT", false, 0.0, MockValueKind::Phone),
            rule(
                "salary",
                "REAL",
                false,
                0.0,
                MockValueKind::FloatRange {
                    min: 50_000.0,
                    max: 180_000.0,
                },
            ),
        ],
    };

    let mut task_plan = infer_plan_from_table(&build_tasks_table_definition(), 60, 25, 300);
    task_plan.rules.push(rule(
        "priority",
        "INTEGER",
        false,
        0.0,
        MockValueKind::IntRange { min: 1, max: 5 },
    ));
    assert_eq!(
        infer_rule_from_column(
            &build_tasks_table_definition().columns[1],
            &build_tasks_table_definition().foreign_keys
        )
        .kind,
        MockValueKind::ForeignKeySample {
            ref_table: "employees".to_owned(),
            ref_column: "id".to_owned(),
        }
    );

    let r_dept = execute_mock_generation(&session, &dept_plan).await?;
    let r_emp = execute_mock_generation(&session, &emp_plan).await?;
    let r_task = execute_mock_generation(&session, &task_plan).await?;
    assert_eq!((r_dept.inserted_rows, r_dept.batches_executed), (8, 2));
    assert_eq!((r_emp.inserted_rows, r_emp.batches_executed), (24, 3));
    assert_eq!((r_task.inserted_rows, r_task.batches_executed), (60, 3));

    let orphans = exec_sql_rows(
        &session,
        "SELECT
            (SELECT COUNT(*) FROM employees e LEFT JOIN departments d ON d.id = e.department_id WHERE d.id IS NULL),
            (SELECT COUNT(*) FROM tasks t LEFT JOIN employees e ON e.id = t.employee_id WHERE e.id IS NULL);",
    )
    .await?;
    assert_eq!(orphans[0], vec![Value::Int(0), Value::Int(0)]);

    let task_rows = exec_sql_rows(
        &session,
        "SELECT id, task_uuid, contact_email, created_at, payload_json, priority FROM tasks ORDER BY id;",
    )
    .await?;
    validate_generated_task_rows(&task_rows)?;

    shutdown_session(&session).await?;
    Ok(())
}

#[test]
#[expect(clippy::too_many_lines, reason = "end-to-end scenario test")]
fn data_transfer_type_mapping_matrix_and_ddl_generation() {
    let cases = [
        (
            DatabaseEngine::Sqlite,
            DatabaseEngine::Duckdb,
            "INTEGER",
            "BIGINT",
        ),
        (
            DatabaseEngine::Sqlite,
            DatabaseEngine::Duckdb,
            "REAL",
            "DOUBLE",
        ),
        (
            DatabaseEngine::Sqlite,
            DatabaseEngine::Postgres,
            "BLOB",
            "BYTEA",
        ),
        (
            DatabaseEngine::Duckdb,
            DatabaseEngine::Sqlite,
            "TIMESTAMP",
            "TEXT",
        ),
        (
            DatabaseEngine::Duckdb,
            DatabaseEngine::Mssql,
            "TIMESTAMPTZ",
            "DATETIMEOFFSET",
        ),
        (
            DatabaseEngine::Postgres,
            DatabaseEngine::Mysql,
            "VARCHAR(120)",
            "VARCHAR(120)",
        ),
        (
            DatabaseEngine::Postgres,
            DatabaseEngine::Mssql,
            "BOOLEAN",
            "BIT",
        ),
        (
            DatabaseEngine::Postgres,
            DatabaseEngine::Clickhouse,
            "DECIMAL(10, 2)",
            "Decimal(10, 2)",
        ),
        (
            DatabaseEngine::Mysql,
            DatabaseEngine::Postgres,
            "TINYINT(1)",
            "BOOLEAN",
        ),
        (
            DatabaseEngine::Mysql,
            DatabaseEngine::Duckdb,
            "DATETIME",
            "TIMESTAMP",
        ),
        (
            DatabaseEngine::Mssql,
            DatabaseEngine::Postgres,
            "UNIQUEIDENTIFIER",
            "UUID",
        ),
        (
            DatabaseEngine::Mssql,
            DatabaseEngine::Clickhouse,
            "BIT",
            "Bool",
        ),
        (
            DatabaseEngine::Clickhouse,
            DatabaseEngine::Postgres,
            "Nullable(Int8)",
            "SMALLINT",
        ),
        (
            DatabaseEngine::Clickhouse,
            DatabaseEngine::Mysql,
            "LowCardinality(String)",
            "LONGTEXT",
        ),
        (
            DatabaseEngine::Generic,
            DatabaseEngine::Clickhouse,
            "UUID",
            "UUID",
        ),
    ];
    for (src, dst, raw, expected) in cases {
        assert_eq!(
            map_sql_type(src, dst, raw),
            expected,
            "{src:?} -> {dst:?} ({raw})"
        );
    }

    let def = TableDefinition {
        table: TableRef::unqualified("metrics"),
        columns: vec![
            col_def("id", "INTEGER", i64_kind(), false, true, true),
            col_def(
                "title",
                "VARCHAR(80)",
                ColumnKind::Text {
                    length: TextLength::Varying(80),
                },
                false,
                false,
                false,
            ),
            col_def(
                "flag",
                "BOOLEAN",
                ColumnKind::Bool(BoolStorage::Native),
                true,
                false,
                false,
            ),
        ],
        primary_key: Some(PrimaryKeyConstraint {
            name: None,
            columns: vec!["id".to_owned()],
        }),
        indexes: Vec::new(),
        foreign_keys: Vec::new(),
        inbound_foreign_keys: Vec::new(),
        source: None,
        options: None,
    };

    for engine in [
        DatabaseEngine::Sqlite,
        DatabaseEngine::Duckdb,
        DatabaseEngine::Postgres,
        DatabaseEngine::Mysql,
        DatabaseEngine::Clickhouse,
    ] {
        let ddl = build_target_create_table_ddl(&def, engine, "metrics_copy");
        assert!(
            ddl.contains("CREATE TABLE IF NOT EXISTS"),
            "{engine:?}: {ddl}"
        );
    }
    let mssql_ddl = build_target_create_table_ddl(&def, DatabaseEngine::Mssql, "metrics_copy");
    assert!(mssql_ddl.contains("IF OBJECT_ID(N'metrics_copy', N'U') IS NULL"));
    assert!(mssql_ddl.contains("CREATE TABLE [metrics_copy]"));

    assert_eq!(
        format_sql_literal(DatabaseEngine::Sqlite, &Value::Bool(true)),
        "1"
    );
    assert_eq!(
        format_sql_literal(DatabaseEngine::Duckdb, &Value::Bool(false)),
        "FALSE"
    );
}

#[tokio::test]
async fn data_transfer_live_sqlite_to_duckdb_2550_rows_all_types() -> E2eResult<()> {
    const TOTAL_ROWS: i64 = 2_550;
    let dir = tempfile::tempdir()?;
    let sqlite = open_sqlite_session(dir.path(), "src_2550.sqlite").await?;
    let duckdb = open_duckdb_session(dir.path(), "dst_2550.duckdb").await?;

    exec_sql_rows(
        &sqlite,
        &format!(
            "BEGIN TRANSACTION;
             CREATE TABLE events (
                 id INTEGER PRIMARY KEY,
                 qty INTEGER NOT NULL,
                 score REAL,
                 active BOOLEAN NOT NULL,
                 happened_at TIMESTAMP,
                 label TEXT
             );
             WITH RECURSIVE seq(x) AS (
                 SELECT 1
                 UNION ALL
                 SELECT x + 1 FROM seq WHERE x < {TOTAL_ROWS}
             )
             INSERT INTO events (id, qty, score, active, happened_at, label)
             SELECT
                 x,
                 x * 2,
                 CASE WHEN x % 10 = 0 THEN NULL ELSE x * 1.25 END,
                 x % 2,
                 CASE WHEN x % 5 = 0 THEN NULL ELSE '2026-11-15 08:30:00' END,
                 CASE
                     WHEN x = 1 THEN 'O''Reilly — 日本語 🚀 \"SQL\"'
                     WHEN x % 7 = 0 THEN NULL
                     ELSE 'row_' || x
                 END
             FROM seq;
             COMMIT;"
        ),
    )
    .await?;

    let cfg = TransferConfig {
        source_schema: None,
        source_table: "events".to_owned(),
        target_schema: None,
        target_table: "events_duck".to_owned(),
        target_engine: DatabaseEngine::Duckdb,
        mode: TransferMode::CreateAndLoad,
        batch_size: 400,
        column_mappings: Vec::new(),
    };
    let report = transfer_table(
        &sqlite,
        &duckdb,
        DatabaseEngine::Sqlite,
        DatabaseEngine::Duckdb,
        &cfg,
    )
    .await?;

    assert!(report.created_table);
    assert_eq!(report.rows_read, 2550);
    assert_eq!(report.rows_written, 2550);
    assert_eq!(report.batches, 7);

    let summary_rows = exec_sql_rows(
        &duckdb,
        "SELECT COUNT(*), CAST(SUM(qty) AS BIGINT), COUNT(*) FILTER (WHERE score IS NULL), COUNT(*) FILTER (WHERE label IS NULL) FROM events_duck",
    )
    .await?;
    let expected_sum: i64 = (1..=TOTAL_ROWS).map(|x| x * 2).sum();
    assert_eq!(
        summary_rows[0],
        vec![
            Value::Int(2550),
            Value::Int(expected_sum),
            Value::Int(255),
            Value::Int(2550 / 7),
        ]
    );

    let sentinel = exec_sql_rows(
        &duckdb,
        "SELECT id, score, active, happened_at, label FROM events_duck WHERE id IN (1, 10) ORDER BY id",
    )
    .await?;
    assert_eq!(sentinel[0][1], Value::Float64(1.25));
    assert_eq!(sentinel[0][2], Value::Bool(true));
    assert!(matches!(
        sentinel[0][3],
        Value::Timestamp(Temporal::Finite(_))
    ));
    assert_eq!(
        sentinel[0][4],
        Value::Text("O'Reilly — 日本語 🚀 \"SQL\"".to_owned())
    );
    assert_eq!(sentinel[1][1], Value::Null);
    assert_eq!(sentinel[1][2], Value::Bool(false));
    assert_eq!(sentinel[1][3], Value::Null);

    shutdown_session(&sqlite).await?;
    shutdown_session(&duckdb).await?;
    Ok(())
}

#[tokio::test]
async fn data_transfer_live_duckdb_to_sqlite_2600_rows_bidirectional() -> E2eResult<()> {
    const TOTAL_ROWS: i64 = 2_600;
    let dir = tempfile::tempdir()?;
    let duckdb = open_duckdb_session(dir.path(), "src_2600.duckdb").await?;
    let sqlite = open_sqlite_session(dir.path(), "dst_2600.sqlite").await?;

    exec_sql_rows(
        &duckdb,
        &format!(
            "CREATE TABLE telemetry (
                 id INTEGER PRIMARY KEY,
                 reading DOUBLE,
                 ok BOOLEAN NOT NULL,
                 recorded_at TIMESTAMP,
                 note VARCHAR
             );
             INSERT INTO telemetry
             SELECT
                 i AS id,
                 CASE WHEN i % 20 = 0 THEN NULL ELSE i * 0.5 END AS reading,
                 (i % 2 = 1) AS ok,
                 CASE WHEN i % 10 = 0 THEN NULL ELSE TIMESTAMP '2026-11-15 14:45:00' END AS recorded_at,
                 CASE WHEN i = 1 THEN 'Duck''s café — 東京' WHEN i % 8 = 0 THEN NULL ELSE 'msg_' || i::VARCHAR END AS note
             FROM generate_series(1, {TOTAL_ROWS}) AS t(i);"
        ),
    )
    .await?;

    let cfg = TransferConfig {
        source_schema: None,
        source_table: "telemetry".to_owned(),
        target_schema: None,
        target_table: "telemetry_sqlite".to_owned(),
        target_engine: DatabaseEngine::Sqlite,
        mode: TransferMode::CreateAndLoad,
        batch_size: 500,
        column_mappings: Vec::new(),
    };
    let report = transfer_table(
        &duckdb,
        &sqlite,
        DatabaseEngine::Duckdb,
        DatabaseEngine::Sqlite,
        &cfg,
    )
    .await?;

    assert!(report.created_table);
    assert_eq!(report.rows_read, 2600);
    assert_eq!(report.rows_written, 2600);
    assert_eq!(report.batches, 6);

    let counts = exec_sql_rows(
        &sqlite,
        "SELECT COUNT(*), SUM(CASE WHEN reading IS NULL THEN 1 ELSE 0 END), SUM(CASE WHEN note IS NULL THEN 1 ELSE 0 END) FROM telemetry_sqlite",
    )
    .await?;
    assert_eq!(
        counts[0],
        vec![Value::Int(2600), Value::Int(130), Value::Int(325)]
    );

    let sentinel = exec_sql_rows(
        &sqlite,
        "SELECT id, reading, ok, recorded_at, note FROM telemetry_sqlite WHERE id IN (1, 20) ORDER BY id",
    )
    .await?;
    assert_eq!(sentinel[0][1], Value::Float64(0.5));
    assert_eq!(sentinel[0][2], Value::Int(1));
    assert_eq!(
        sentinel[0][3],
        Value::Text("2026-11-15 14:45:00".to_owned())
    );
    assert_eq!(sentinel[0][4], Value::Text("Duck's café — 東京".to_owned()));
    assert_eq!(sentinel[1][1], Value::Null);
    assert_eq!(sentinel[1][2], Value::Int(0));
    assert_eq!(sentinel[1][3], Value::Null);

    shutdown_session(&duckdb).await?;
    shutdown_session(&sqlite).await?;
    Ok(())
}

#[tokio::test]
#[expect(clippy::too_many_lines, reason = "end-to-end scenario test")]
async fn data_transfer_sqlite_to_sqlite_2550_rows_modes_subset_and_rename() -> E2eResult<()> {
    let dir = tempfile::tempdir()?;
    let src = open_sqlite_session(dir.path(), "s2s_src.sqlite").await?;
    let dst = open_sqlite_session(dir.path(), "s2s_dst.sqlite").await?;

    exec_sql_rows(
        &src,
        "BEGIN TRANSACTION;
         CREATE TABLE users_main (id INTEGER PRIMARY KEY, secret TEXT NOT NULL, full_name TEXT NOT NULL, score REAL);
         WITH RECURSIVE seq(x) AS (SELECT 1 UNION ALL SELECT x + 1 FROM seq WHERE x < 2520)
         INSERT INTO users_main SELECT x, 'sec_' || x, 'User ' || x, x * 0.5 FROM seq;
         CREATE TABLE users_extra (id INTEGER PRIMARY KEY, secret TEXT NOT NULL, full_name TEXT NOT NULL, score REAL);
         WITH RECURSIVE seq(x) AS (SELECT 2521 UNION ALL SELECT x + 1 FROM seq WHERE x < 2600)
         INSERT INTO users_extra SELECT x, 'sec_' || x, 'Extra ' || x, x * 0.5 FROM seq;
         CREATE TABLE users_replace (id INTEGER PRIMARY KEY, secret TEXT NOT NULL, full_name TEXT NOT NULL, score REAL);
         INSERT INTO users_replace VALUES (9001, 's', 'Replaced Only', 99.5);
         COMMIT;",
    )
    .await?;

    let mappings = vec![
        ColumnTypeMapping {
            source_column: "secret".to_owned(),
            target_column: String::new(),
            target_type: String::new(),
            include: false,
        },
        ColumnTypeMapping {
            source_column: "full_name".to_owned(),
            target_column: "display_name".to_owned(),
            target_type: "TEXT".to_owned(),
            include: true,
        },
    ];

    let create_cfg = TransferConfig {
        source_schema: None,
        source_table: "users_main".to_owned(),
        target_schema: None,
        target_table: "users_out".to_owned(),
        target_engine: DatabaseEngine::Sqlite,
        mode: TransferMode::CreateAndLoad,
        batch_size: 600,
        column_mappings: mappings.clone(),
    };
    let r1 = transfer_table(
        &src,
        &dst,
        DatabaseEngine::Sqlite,
        DatabaseEngine::Sqlite,
        &create_cfg,
    )
    .await?;
    assert!(r1.created_table);
    assert_eq!((r1.rows_read, r1.rows_written, r1.batches), (2520, 2520, 5));

    let dst_cols = dst
        .columns(TableRef::unqualified("users_out"))
        .await
        .map_err(|e| format!("dst columns failed: {e}"))?;
    let col_names: Vec<&str> = dst_cols.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(col_names, vec!["id", "display_name", "score"]);

    let r2 = transfer_table(
        &src,
        &dst,
        DatabaseEngine::Sqlite,
        DatabaseEngine::Sqlite,
        &TransferConfig {
            source_table: "users_extra".to_owned(),
            mode: TransferMode::Append,
            ..create_cfg.clone()
        },
    )
    .await?;
    assert!(!r2.created_table);
    assert_eq!(r2.rows_written, 80);
    assert_eq!(
        exec_sql_rows(&dst, "SELECT COUNT(*) FROM users_out").await?[0][0],
        Value::Int(2600)
    );

    let r3 = transfer_table(
        &src,
        &dst,
        DatabaseEngine::Sqlite,
        DatabaseEngine::Sqlite,
        &TransferConfig {
            source_table: "users_replace".to_owned(),
            mode: TransferMode::TruncateAndLoad,
            ..create_cfg
        },
    )
    .await?;
    assert!(!r3.created_table);
    assert_eq!(r3.rows_written, 1);
    let final_rows = exec_sql_rows(&dst, "SELECT id, display_name, score FROM users_out").await?;
    assert_eq!(final_rows.len(), 1);
    assert_eq!(
        final_rows[0],
        vec![
            Value::Int(9001),
            Value::Text("Replaced Only".to_owned()),
            Value::Float64(99.5),
        ]
    );

    shutdown_session(&src).await?;
    shutdown_session(&dst).await?;
    Ok(())
}

#[tokio::test]
async fn data_transfer_cancellation_before_and_during_transfer() -> E2eResult<()> {
    let dir = tempfile::tempdir()?;
    let src = open_sqlite_session(dir.path(), "cancel_src.sqlite").await?;
    let dst = open_sqlite_session(dir.path(), "cancel_dst.sqlite").await?;

    exec_sql_rows(
        &src,
        "BEGIN TRANSACTION;
         CREATE TABLE bulk_items (id INTEGER PRIMARY KEY, val TEXT NOT NULL);
         WITH RECURSIVE seq(x) AS (SELECT 1 UNION ALL SELECT x + 1 FROM seq WHERE x < 4000)
         INSERT INTO bulk_items SELECT x, 'item_' || x FROM seq;
         COMMIT;",
    )
    .await?;

    let cfg = TransferConfig {
        source_schema: None,
        source_table: "bulk_items".to_owned(),
        target_schema: None,
        target_table: "bulk_out".to_owned(),
        target_engine: DatabaseEngine::Sqlite,
        mode: TransferMode::CreateAndLoad,
        batch_size: 4,
        column_mappings: Vec::new(),
    };

    // 1. Cancelled before transfer starts.
    let pre_cancel = CancellationToken::new();
    pre_cancel.cancel();
    let pre_res = transfer_table_cancellable(
        &src,
        &dst,
        DatabaseEngine::Sqlite,
        DatabaseEngine::Sqlite,
        &cfg,
        &pre_cancel,
    )
    .await;
    assert!(pre_res.is_err(), "pre-cancelled transfer must fail");

    // 2. Cancelled mid-transfer across 1,000 tiny batches.
    let mid_cancel = CancellationToken::new();
    let trigger = mid_cancel.clone();
    let cancel_task = tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(8)).await;
        trigger.cancel();
    });
    let mid_res = transfer_table_cancellable(
        &src,
        &dst,
        DatabaseEngine::Sqlite,
        DatabaseEngine::Sqlite,
        &cfg,
        &mid_cancel,
    )
    .await;
    let _ = cancel_task.await;
    assert!(mid_res.is_err(), "mid-transfer cancellation must fail");

    let count_after = exec_sql_rows(&dst, "SELECT COUNT(*) FROM bulk_out").await?;
    let written = match count_after[0][0] {
        Value::Int(n) => n,
        ref other => return Err(format!("unexpected count value: {other:?}").into()),
    };
    assert!(
        written < 4000,
        "expected partial transfer (< 4000 rows), got {written}"
    );

    shutdown_session(&src).await?;
    shutdown_session(&dst).await?;
    Ok(())
}

#[test]
#[expect(clippy::too_many_lines, reason = "end-to-end scenario test")]
fn server_monitor_dialect_queries_and_validation_edges() {
    assert!(
        sessions_query(DatabaseEngine::Postgres).is_some_and(|q| q.contains("pg_stat_activity"))
    );
    assert!(sessions_query(DatabaseEngine::Mysql).is_some_and(|q| q.contains("PROCESSLIST")));
    assert!(sessions_query(DatabaseEngine::Mssql).is_some_and(|q| q.contains("dm_exec_sessions")));
    assert!(
        sessions_query(DatabaseEngine::Clickhouse).is_some_and(|q| q.contains("system.processes"))
    );
    assert!(sessions_query(DatabaseEngine::Sqlite).is_none());
    assert!(sessions_query(DatabaseEngine::Duckdb).is_none());
    assert!(sessions_query(DatabaseEngine::Generic).is_none());

    assert!(locks_query(DatabaseEngine::Postgres).is_some_and(|q| q.contains("pg_locks")));
    assert!(locks_query(DatabaseEngine::Mysql).is_some_and(|q| q.contains("innodb_trx")));
    assert!(locks_query(DatabaseEngine::Mssql).is_some_and(|q| q.contains("dm_tran_locks")));
    assert!(locks_query(DatabaseEngine::Clickhouse).is_none());
    assert!(locks_query(DatabaseEngine::Sqlite).is_none());
    assert!(locks_query(DatabaseEngine::Duckdb).is_none());
    assert!(locks_query(DatabaseEngine::Generic).is_none());

    assert_eq!(
        validate_session_id(DatabaseEngine::Postgres, " 42 ")
            .ok()
            .as_deref(),
        Some("42")
    );
    assert_eq!(
        validate_session_id(DatabaseEngine::Clickhouse, "q-1_a.b:2")
            .ok()
            .as_deref(),
        Some("q-1_a.b:2")
    );

    for bad in [
        "",
        "   ",
        "0",
        "-1",
        "+10",
        "1; DROP TABLE t",
        "1 OR 1=1",
        "1 2",
        "abc",
        "99999999999999999999999",
    ] {
        for engine in [
            DatabaseEngine::Postgres,
            DatabaseEngine::Mysql,
            DatabaseEngine::Mssql,
        ] {
            assert!(
                matches!(
                    validate_session_id(engine, bad),
                    Err(ServiceError::Session(_))
                ),
                "{engine:?} should reject '{bad}'"
            );
            assert!(kill_session_sql(engine, bad).is_err());
        }
    }

    let too_long = "a".repeat(129);
    for bad_ch in ["", " ", "q'1", "q;drop", "has space", "a\\b", &too_long] {
        assert!(matches!(
            validate_session_id(DatabaseEngine::Clickhouse, bad_ch),
            Err(ServiceError::Session(_))
        ));
    }

    for embedded in [
        DatabaseEngine::Sqlite,
        DatabaseEngine::Duckdb,
        DatabaseEngine::Generic,
    ] {
        assert!(matches!(
            validate_session_id(embedded, "1"),
            Err(ServiceError::UnsupportedEngine(_))
        ));
        assert!(matches!(
            kill_session_sql(embedded, "1"),
            Err(ServiceError::UnsupportedEngine(_))
        ));
    }

    let text_col = |n: &str| ResultColumn::new(n, ColumnType::untyped_text("TEXT"));
    let sess_res = QueryResult::new(
        vec![
            text_col("pid"),
            text_col("usename"),
            text_col("state"),
            text_col("duration_ms"),
        ],
        vec![vec![
            Value::Int(9),
            Value::Text("pg".to_owned()),
            Value::Text("active".to_owned()),
            Value::Int(50),
        ]],
    );
    let parsed_sessions = parse_sessions_result(DatabaseEngine::Postgres, &sess_res);
    assert_eq!(parsed_sessions.len(), 1);
    assert_eq!(parsed_sessions[0].id, "9");

    let lock_res = QueryResult::new(
        vec![text_col("pid"), text_col("granted"), text_col("blocked_by")],
        vec![vec![Value::Int(9), Value::Bool(false), Value::Int(3)]],
    );
    let parsed_locks: Vec<ServerLockRow> = parse_locks_result(DatabaseEngine::Postgres, &lock_res);
    assert_eq!(parsed_locks.len(), 1);
    assert!(!parsed_locks[0].granted);
    assert_eq!(parsed_locks[0].blocked_by.as_deref(), Some("3"));
    assert!(
        format_snapshot_summary(DatabaseEngine::Postgres, &parsed_sessions, &parsed_locks)
            .contains("1 active")
    );
}

#[tokio::test]
async fn server_monitor_live_sqlite_and_duckdb_snapshots() -> E2eResult<()> {
    let dir = tempfile::tempdir()?;
    let sqlite = open_sqlite_session(dir.path(), "monitor.sqlite").await?;
    let duckdb = open_duckdb_session(dir.path(), "monitor.duckdb").await?;

    let sqlite_snap = fetch_server_monitor_snapshot(&sqlite, DatabaseEngine::Sqlite).await?;
    assert_eq!(sqlite_snap.engine, DatabaseEngine::Sqlite);
    assert!(sqlite_snap.sessions.is_empty());
    assert!(sqlite_snap.locks.is_empty());
    assert!(sqlite_snap.summary.contains("sqlite"));

    let duck_snap = fetch_server_monitor_snapshot(&duckdb, DatabaseEngine::Duckdb).await?;
    assert_eq!(duck_snap.engine, DatabaseEngine::Duckdb);
    assert!(duck_snap.sessions.is_empty());
    assert!(duck_snap.locks.is_empty());
    assert!(duck_snap.summary.contains("duckdb"));

    assert!(matches!(
        terminate_server_session(&sqlite, DatabaseEngine::Sqlite, "1").await,
        Err(ServiceError::UnsupportedEngine(_))
    ));
    assert!(matches!(
        terminate_server_session(&duckdb, DatabaseEngine::Duckdb, "1").await,
        Err(ServiceError::UnsupportedEngine(_))
    ));

    shutdown_session(&sqlite).await?;
    shutdown_session(&duckdb).await?;
    Ok(())
}

#[test]
fn settings_window_controls_style_roundtrip_and_legacy_default() -> E2eResult<()> {
    assert_eq!(
        serde_json::to_string(&WindowControlsStyle::MacOs)?,
        "\"mac_os\""
    );
    assert_eq!(
        serde_json::to_string(&WindowControlsStyle::Windows)?,
        "\"windows\""
    );
    let mac_enum: WindowControlsStyle = serde_json::from_str("\"mac_os\"")?;
    let win_enum: WindowControlsStyle = serde_json::from_str("\"windows\"")?;
    assert_eq!(mac_enum, WindowControlsStyle::MacOs);
    assert_eq!(win_enum, WindowControlsStyle::Windows);

    let dir = tempfile::tempdir()?;
    let path = dir.path().join("settings.json");

    let mac_settings = AppSettings {
        window_controls: WindowControlsStyle::MacOs,
        ..AppSettings::default()
    };
    mac_settings.save_atomic(&path)?;
    assert_eq!(
        AppSettings::load(&path)?.window_controls,
        WindowControlsStyle::MacOs
    );

    let win_settings = AppSettings {
        window_controls: WindowControlsStyle::Windows,
        ..AppSettings::default()
    };
    win_settings.save_atomic(&path)?;
    let reloaded = AppSettings::load(&path)?;
    assert_eq!(reloaded.window_controls, WindowControlsStyle::Windows);
    assert_eq!(reloaded, win_settings);

    // Legacy JSON omitting `window_controls` must default to MacOs.
    std::fs::write(&path, r#"{"theme":"dark"}"#)?;
    assert_eq!(
        AppSettings::load(&path)?.window_controls,
        WindowControlsStyle::MacOs
    );
    Ok(())
}
