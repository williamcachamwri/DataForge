//! Real SQLite end-to-end tests: session connect, schema catalog, paging,
//! `TableEdits` commit/rollback, `RunEvent` SQL execution, and CSV/JSON/SQL export.

use std::sync::Arc;

use tableminus_e2e::{E2eResult, open_file_session, run_sql, shutdown_session};
use tableminus_state::data_grid::{
    DataGridCellEdit, DataGridColumnID, DataGridLayoutScope, DataGridModel, RowID, TableEdits,
};
use tablepro_core::browse::{PageRequest, build_count, read_page};
use tablepro_core::dml::{build_delete, build_insert, build_update};
use tablepro_core::edit::CellInput;
use tablepro_core::export::CsvOptions;
use tablepro_core::filter::FilterSet;
use tablepro_core::meta::{ObjectScope, PagePosition, TableRef};
use tablepro_core::statement::GuardedStatement;
use tablepro_core::value::Value;
use tablepro_core::{ForeignKeyMode, ResultEvent, RunEnd, WriteBatch, WriteKind};
use tablepro_driver_sqlite::SqliteDriver;
use tablepro_session::{OperationError, OperationKind, Session};

async fn seed_sqlite_db(session: &Session) -> E2eResult<()> {
    let ddl = "
        CREATE TABLE users (
            id INTEGER PRIMARY KEY,
            name TEXT NOT NULL,
            role TEXT NOT NULL
        );
        CREATE TABLE orders (
            id INTEGER PRIMARY KEY,
            user_id INTEGER NOT NULL REFERENCES users(id),
            amount INTEGER NOT NULL
        );
        INSERT INTO users (id, name, role) VALUES
            (1, 'Alice', 'admin'),
            (2, 'Bob', 'editor'),
            (3, 'Charlie', 'viewer'),
            (4, 'Diana', 'editor'),
            (5, 'Eve', 'viewer');
        INSERT INTO orders (id, user_id, amount) VALUES
            (10, 1, 250),
            (11, 2, 100);
    ";
    let out = run_sql(session, ddl).await?;
    if !matches!(out.summary.end, RunEnd::Completed) {
        return Err(format!("seed failed: {:?}", out.summary.end).into());
    }
    Ok(())
}

#[tokio::test]
async fn sqlite_connect_and_browse_schema_catalog() -> E2eResult<()> {
    let dir = tempfile::tempdir()?;
    let db_path = dir.path().join("catalog.sqlite");
    let session = open_file_session(
        dir.path(),
        &db_path,
        Arc::new(SqliteDriver),
        "sqlite",
        true,
        false,
    )
    .await?;

    seed_sqlite_db(&session).await?;

    let schemas = session
        .schemas()
        .await
        .map_err(|e| format!("schemas failed: {e}"))?;
    assert!(
        !schemas.is_empty(),
        "SQLite should report at least main schema"
    );

    let objects = session
        .refresh_objects(ObjectScope::DefaultSchema)
        .await
        .map_err(|e| format!("objects failed: {e}"))?;
    let names: Vec<&str> = objects.iter().map(|o| o.name.as_str()).collect();
    assert!(
        names.contains(&"users"),
        "expected users table in {names:?}"
    );
    assert!(
        names.contains(&"orders"),
        "expected orders table in {names:?}"
    );

    let cols = session
        .columns(TableRef::unqualified("users"))
        .await
        .map_err(|e| format!("columns failed: {e}"))?;
    let col_names: Vec<&str> = cols.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(col_names, vec!["id", "name", "role"]);

    shutdown_session(&session).await?;
    Ok(())
}

#[tokio::test]
async fn sqlite_page_rows_and_row_count() -> E2eResult<()> {
    let dir = tempfile::tempdir()?;
    let db_path = dir.path().join("paging.sqlite");
    let session = open_file_session(
        dir.path(),
        &db_path,
        Arc::new(SqliteDriver),
        "sqlite",
        true,
        false,
    )
    .await?;
    seed_sqlite_db(&session).await?;

    let table = TableRef::unqualified("users");
    let cols = session
        .columns(table.clone())
        .await
        .map_err(|e| format!("columns failed: {e}"))?;
    let filter = FilterSet::default();

    let (total_count, page1, page2) = session
        .run(OperationKind::Page, |lease, opts| {
            let table = table.clone();
            let cols = cols.clone();
            let filter = filter.clone();
            async move {
                let conn = lease.connection();
                let dialect = conn.dialect();
                let count_stmt = build_count(dialect.as_ref(), &table, &cols, &filter)?;
                let total = conn.count_rows(&count_stmt, opts.clone()).await?;
                let identity = conn.row_identity(&table, opts.clone()).await?;

                let req1 = PageRequest {
                    table: &table,
                    columns: &cols,
                    filter: &filter,
                    sort: &[],
                    identity: &identity,
                    position: &PagePosition::First,
                    limit: 2,
                };
                let p1 = read_page::<OperationError>(conn.as_ref(), &req1, &opts).await?;

                let pos2 = p1.next.clone().unwrap_or(PagePosition::Offset(2));
                let req2 = PageRequest {
                    table: &table,
                    columns: &cols,
                    filter: &filter,
                    sort: &[],
                    identity: &identity,
                    position: &pos2,
                    limit: 2,
                };
                let p2 = read_page::<OperationError>(conn.as_ref(), &req2, &opts).await?;
                Ok((total, p1, p2))
            }
        })
        .await?;

    assert_eq!(total_count, 5);
    assert_eq!(page1.result.rows.len(), 2);
    assert_eq!(page2.result.rows.len(), 2);

    let mut grid = DataGridModel::new(DataGridLayoutScope::Table("users".to_owned()), 2);
    grid.reload_rows(
        page1.result.columns.to_vec(),
        page1.result.rows,
        Some(total_count),
    );
    assert_eq!(grid.page.page_info(), (1, Some(3)));
    assert!(grid.page.next_page());
    grid.reload_rows(
        page2.result.columns.to_vec(),
        page2.result.rows,
        Some(total_count),
    );
    assert_eq!(grid.page.page_info(), (2, Some(3)));
    assert!(grid.page.prev_page());

    shutdown_session(&session).await?;
    Ok(())
}

async fn apply_table_edits(
    session: &Session,
    table: &TableRef,
    cols: &[tablepro_core::ColumnInfo],
    committed: TableEdits,
) -> E2eResult<Option<u64>> {
    let report = session
        .run(OperationKind::Save(WriteKind::RowChanges), |lease, opts| {
            let table = table.clone();
            let cols = cols.to_vec();
            let committed = committed.clone();
            async move {
                let conn = lease.connection();
                let dialect = conn.dialect();
                let identity = conn.row_identity(&table, opts.clone()).await?;
                let mut steps = Vec::new();

                for ((row_id, col_id), new_val) in &committed.modified_cells {
                    let stmt = build_update(
                        dialect.as_ref(),
                        &table,
                        &cols,
                        &identity,
                        &[Value::Int(i64::try_from(row_id.0).unwrap_or(0))],
                        &[(
                            usize::try_from(col_id.0).unwrap_or(1),
                            CellInput::Value(new_val.clone()),
                        )],
                        &[],
                    )?;
                    steps.push(GuardedStatement::affecting_exactly(stmt, 1));
                }

                for values in committed.inserted_rows.values() {
                    let cells: Vec<CellInput> =
                        values.iter().cloned().map(CellInput::Value).collect();
                    let stmt = build_insert(dialect.as_ref(), &table, &cols, &cells)?;
                    steps.push(GuardedStatement::affecting_exactly(stmt, 1));
                }

                for row_id in &committed.deleted_rows {
                    let stmt = build_delete(
                        dialect.as_ref(),
                        &table,
                        &cols,
                        &identity,
                        &[Value::Int(i64::try_from(row_id.0).unwrap_or(0))],
                    )?;
                    steps.push(GuardedStatement::affecting_exactly(stmt, 1));
                }

                let batch = WriteBatch::new(
                    WriteKind::RowChanges,
                    table,
                    steps,
                    ForeignKeyMode::Enforced,
                );
                Ok(conn.write_batch(&batch, opts).await?)
            }
        })
        .await?;
    Ok(report.total_rows())
}

#[tokio::test]
async fn sqlite_edit_insert_delete_commit_and_rollback() -> E2eResult<()> {
    let dir = tempfile::tempdir()?;
    let db_path = dir.path().join("edits.sqlite");
    let session = open_file_session(
        dir.path(),
        &db_path,
        Arc::new(SqliteDriver),
        "sqlite",
        true,
        false,
    )
    .await?;
    seed_sqlite_db(&session).await?;

    // Verify TableEdits rollback discards pending modifications without touching SQLite.
    let mut edits = TableEdits::new();
    edits.commit_cells(
        vec![DataGridCellEdit {
            row_id: RowID(1),
            column_id: DataGridColumnID(1),
            old_value: Value::Text("Alice".to_owned()),
            new_value: Value::Text("Mallory".to_owned()),
        }],
        "rename user",
    );
    let temp_id = edits.insert_row(vec![
        Value::Int(99),
        Value::Text("Temp".to_owned()),
        Value::Text("guest".to_owned()),
    ]);
    assert!(edits.has_pending_edits());
    edits.delete_row(temp_id);
    edits.rollback();
    assert!(!edits.has_pending_edits());

    // Now stage real edits: update id=1 name, insert id=6, delete id=5, and commit via WriteBatch.
    edits.commit_cells(
        vec![DataGridCellEdit {
            row_id: RowID(1),
            column_id: DataGridColumnID(1),
            old_value: Value::Text("Alice".to_owned()),
            new_value: Value::Text("Alice Updated".to_owned()),
        }],
        "update Alice",
    );
    edits.insert_row(vec![
        Value::Int(6),
        Value::Text("Frank".to_owned()),
        Value::Text("admin".to_owned()),
    ]);
    edits.delete_row(RowID(5));
    let committed = edits.take_committed();
    assert!(!edits.has_pending_edits());
    assert_eq!(committed.modified_cells.len(), 1);
    assert_eq!(committed.inserted_rows.len(), 1);
    assert_eq!(committed.deleted_rows.len(), 1);

    let table = TableRef::unqualified("users");
    let cols = session
        .columns(table.clone())
        .await
        .map_err(|e| format!("columns failed: {e}"))?;
    let total_rows = apply_table_edits(&session, &table, &cols, committed).await?;
    assert_eq!(total_rows, Some(3));

    let verify = run_sql(&session, "SELECT id, name FROM users ORDER BY id").await?;
    let rs = verify
        .accumulator
        .result_sets
        .first()
        .ok_or("expected result set")?;
    assert_eq!(rs.rows.len(), 5);
    assert_eq!(
        rs.rows[0],
        vec![Value::Int(1), Value::Text("Alice Updated".to_owned())]
    );
    assert_eq!(
        rs.rows[4],
        vec![Value::Int(6), Value::Text("Frank".to_owned())]
    );

    shutdown_session(&session).await?;
    Ok(())
}

#[tokio::test]
async fn sqlite_execute_sql_and_verify_run_events() -> E2eResult<()> {
    let dir = tempfile::tempdir()?;
    let db_path = dir.path().join("sql_run.sqlite");
    let session = open_file_session(
        dir.path(),
        &db_path,
        Arc::new(SqliteDriver),
        "sqlite",
        true,
        false,
    )
    .await?;
    seed_sqlite_db(&session).await?;

    let out = run_sql(
        &session,
        "SELECT u.name, o.amount FROM users u JOIN orders o ON o.user_id = u.id ORDER BY o.id",
    )
    .await?;
    assert!(matches!(out.summary.end, RunEnd::Completed));
    assert!(
        out.events
            .iter()
            .any(|e| matches!(e, ResultEvent::ResultSetStarted { .. })),
        "expected ResultSetStarted event"
    );
    assert!(
        out.events
            .iter()
            .any(|e| matches!(e, ResultEvent::ResultSetFinished { .. })),
        "expected ResultSetFinished event"
    );
    let rs = out
        .accumulator
        .result_sets
        .first()
        .ok_or("expected result set")?;
    assert_eq!(rs.columns.len(), 2);
    assert_eq!(rs.rows.len(), 2);
    assert_eq!(
        rs.rows[0],
        vec![Value::Text("Alice".to_owned()), Value::Int(250)]
    );

    // Invalid SQL should produce RunEnd::Failed in summary.
    let err_out = run_sql(&session, "SELECT * FROM nonexistent_table").await?;
    assert!(matches!(err_out.summary.end, RunEnd::Failed { .. }));
    assert!(matches!(
        err_out.accumulator.summary.as_ref().map(|s| &s.end),
        Some(RunEnd::Failed { .. })
    ));

    shutdown_session(&session).await?;
    Ok(())
}

#[tokio::test]
async fn sqlite_export_csv_json_sql() -> E2eResult<()> {
    let dir = tempfile::tempdir()?;
    let db_path = dir.path().join("export.sqlite");
    let session = open_file_session(
        dir.path(),
        &db_path,
        Arc::new(SqliteDriver),
        "sqlite",
        true,
        false,
    )
    .await?;
    seed_sqlite_db(&session).await?;

    let table = TableRef::unqualified("users");
    let cols = session
        .columns(table.clone())
        .await
        .map_err(|e| format!("columns failed: {e}"))?;
    let filter = FilterSet::default();

    let (page, dialect) = session
        .run(OperationKind::Page, |lease, opts| {
            let table = table.clone();
            let cols = cols.clone();
            let filter = filter.clone();
            async move {
                let conn = lease.connection();
                let identity = conn.row_identity(&table, opts.clone()).await?;
                let req = PageRequest {
                    table: &table,
                    columns: &cols,
                    filter: &filter,
                    sort: &[],
                    identity: &identity,
                    position: &PagePosition::First,
                    limit: 10,
                };
                let page = read_page::<OperationError>(conn.as_ref(), &req, &opts).await?;
                Ok((page, conn.dialect()))
            }
        })
        .await?;

    let mut grid = DataGridModel::new(DataGridLayoutScope::Table("users".to_owned()), 10);
    grid.reload_rows(page.result.columns.to_vec(), page.result.rows, Some(5));

    let csv_text = grid.export_csv(&CsvOptions::default())?;
    assert!(csv_text.contains("Alice"));
    assert!(csv_text.contains("Eve"));

    let json_text = grid.export_json()?;
    let parsed: serde_json::Value = serde_json::from_str(&json_text)?;
    let arr = parsed.as_array().ok_or("expected JSON array")?;
    assert_eq!(arr.len(), 5);

    let sql_text = grid.export_sql_inserts(dialect.as_ref(), &table, &cols)?;
    assert!(sql_text.contains("INSERT INTO"));
    assert!(sql_text.contains("'Alice'"));

    shutdown_session(&session).await?;
    Ok(())
}

#[tokio::test]
async fn sqlite_100k_rows_load_and_scroll_perf() -> E2eResult<()> {
    let dir = tempfile::tempdir()?;
    let db_path = dir.path().join("bench_100k.sqlite");
    let session = open_file_session(
        dir.path(),
        &db_path,
        Arc::new(SqliteDriver),
        "sqlite",
        true,
        false,
    )
    .await?;

    let seed_start = std::time::Instant::now();
    let seed_sql = "
        CREATE TABLE bench_rows (
            id INTEGER PRIMARY KEY,
            label TEXT NOT NULL,
            score INTEGER NOT NULL
        );
        WITH RECURSIVE cnt(x) AS (
            SELECT 1
            UNION ALL
            SELECT x + 1 FROM cnt WHERE x < 100000
        )
        INSERT INTO bench_rows (id, label, score)
        SELECT x, 'row_' || x, x * 7 FROM cnt;
    ";
    let seed_out = run_sql(&session, seed_sql).await?;
    if !matches!(seed_out.summary.end, RunEnd::Completed) {
        return Err(format!("seed failed: {:?}", seed_out.summary.end).into());
    }
    let seed_ms = seed_start.elapsed().as_secs_f64() * 1000.0;

    let table = TableRef::unqualified("bench_rows");
    let cols = session
        .columns(table.clone())
        .await
        .map_err(|e| format!("columns failed: {e}"))?;
    let filter = FilterSet::default();

    let query_start = std::time::Instant::now();
    let page = session
        .run(OperationKind::Page, |lease, opts| {
            let table = table.clone();
            let cols = cols.clone();
            let filter = filter.clone();
            async move {
                let conn = lease.connection();
                let identity = conn.row_identity(&table, opts.clone()).await?;
                let req = PageRequest {
                    table: &table,
                    columns: &cols,
                    filter: &filter,
                    sort: &[],
                    identity: &identity,
                    position: &PagePosition::First,
                    limit: 100_000,
                };
                read_page::<OperationError>(conn.as_ref(), &req, &opts).await
            }
        })
        .await?;
    let query_100k_ms = query_start.elapsed().as_secs_f64() * 1000.0;
    assert_eq!(page.result.rows.len(), 100_000);

    let rows = page.result.rows;
    let grid_start = std::time::Instant::now();
    let mut grid = DataGridModel::new(DataGridLayoutScope::Table("bench_rows".to_owned()), 100_000);
    grid.reload_rows(page.result.columns.to_vec(), rows.clone(), Some(100_000));
    let grid_load_100k_ms = grid_start.elapsed().as_secs_f64() * 1000.0;
    assert_eq!(grid.row_order.len(), 100_000);

    let row_height_px: f32 = 28.0;
    let view_h: f32 = 720.0;
    let overscan: usize = 4;
    let total_rows = grid.row_order.len();
    let max_scroll_y = (total_rows as f32 * row_height_px - view_h).max(0.0);
    let vis_count = (view_h / row_height_px).ceil() as usize + overscan * 2;
    let frames: usize = 10_000;

    let scroll_start = std::time::Instant::now();
    let mut checksum: usize = 0;
    for frame in 0..frames {
        let scroll_y = max_scroll_y * (frame as f32 / (frames - 1) as f32);
        let first_row = ((scroll_y / row_height_px).floor() as usize).saturating_sub(overscan);
        let last_row = (first_row + vis_count).min(total_rows);
        for row_idx in first_row..last_row {
            if let Some(row_id) = grid.row_order.row_id(row_idx)
                && let Some(row) = rows.get(row_idx)
            {
                checksum = checksum.wrapping_add(row_id.0 as usize + row.len());
            }
        }
    }
    std::hint::black_box(checksum);
    let scroll_10k_frames_ms = scroll_start.elapsed().as_secs_f64() * 1000.0;
    let us_per_frame = (scroll_10k_frames_ms * 1000.0) / frames as f64;

    println!(
        "BENCH_100K: seed_ms={seed_ms:.2} query_100k_ms={query_100k_ms:.2} grid_load_100k_ms={grid_load_100k_ms:.2} scroll_10k_frames_ms={scroll_10k_frames_ms:.3} us_per_frame={us_per_frame:.3}"
    );

    shutdown_session(&session).await?;
    Ok(())
}
