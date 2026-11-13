//! Real DuckDB end-to-end tests: session connect, schema browsing,
//! paged table reads, analytical SQL queries, and CSV/JSON/SQL export.

use std::sync::Arc;

use dataforge_e2e::{E2eResult, open_file_session, run_sql, shutdown_session};
use dataforge_state::data_grid::{DataGridLayoutScope, DataGridModel};
use tablepro_core::browse::{PageRequest, build_count, read_page};
use tablepro_core::export::CsvOptions;
use tablepro_core::filter::FilterSet;
use tablepro_core::meta::{ObjectScope, PagePosition, TableRef};
use tablepro_core::value::Value;
use tablepro_core::{ResultEvent, RunEnd};
use tablepro_driver_duckdb::DuckDbDriver;
use tablepro_session::{OperationError, OperationKind, Session};

async fn create_empty_duckdb(path: &std::path::Path) -> E2eResult<()> {
    let conn = duckdb::Connection::open(path)?;
    conn.execute_batch("CHECKPOINT")?;
    Ok(())
}

async fn seed_duckdb(session: &Session) -> E2eResult<()> {
    let ddl = "
        CREATE TABLE events (
            id INTEGER PRIMARY KEY,
            region VARCHAR NOT NULL,
            category VARCHAR NOT NULL,
            revenue DOUBLE NOT NULL
        );
        INSERT INTO events VALUES
            (1, 'us-east', 'compute', 120.5),
            (2, 'us-east', 'storage', 45.0),
            (3, 'eu-west', 'compute', 200.0),
            (4, 'eu-west', 'storage', 80.5),
            (5, 'ap-south', 'compute', 150.0);
    ";
    let out = run_sql(session, ddl).await?;
    if !matches!(out.summary.end, RunEnd::Completed) {
        return Err(format!("duckdb seed failed: {:?}", out.summary.end).into());
    }
    Ok(())
}

#[tokio::test]
async fn duckdb_connect_and_browse_schema() -> E2eResult<()> {
    let dir = tempfile::tempdir()?;
    let db_path = dir.path().join("catalog.duckdb");
    create_empty_duckdb(&db_path).await?;
    let session = open_file_session(
        dir.path(),
        &db_path,
        Arc::new(DuckDbDriver),
        "duckdb",
        false,
        false,
    )
    .await?;
    seed_duckdb(&session).await?;

    let schemas = session
        .schemas()
        .await
        .map_err(|e| format!("schemas failed: {e}"))?;
    assert!(!schemas.is_empty(), "DuckDB should report main schema");

    let objects = session
        .refresh_objects(ObjectScope::DefaultSchema)
        .await
        .map_err(|e| format!("objects failed: {e}"))?;
    let names: Vec<&str> = objects.iter().map(|o| o.name.as_str()).collect();
    assert!(names.contains(&"events"), "expected events table in {names:?}");

    let cols = session
        .columns(TableRef::unqualified("events"))
        .await
        .map_err(|e| format!("columns failed: {e}"))?;
    let col_names: Vec<&str> = cols.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(col_names, vec!["id", "region", "category", "revenue"]);

    shutdown_session(&session).await?;
    Ok(())
}

#[tokio::test]
async fn duckdb_page_rows_and_analytical_queries() -> E2eResult<()> {
    let dir = tempfile::tempdir()?;
    let db_path = dir.path().join("analytics.duckdb");
    create_empty_duckdb(&db_path).await?;
    let session = open_file_session(
        dir.path(),
        &db_path,
        Arc::new(DuckDbDriver),
        "duckdb",
        false,
        false,
    )
    .await?;
    seed_duckdb(&session).await?;

    let table = TableRef::unqualified("events");
    let cols = session
        .columns(table.clone())
        .await
        .map_err(|e| format!("columns failed: {e}"))?;
    let filter = FilterSet::default();

    let (total_count, page1) = session
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
                let req = PageRequest {
                    table: &table,
                    columns: &cols,
                    filter: &filter,
                    sort: &[],
                    identity: &identity,
                    position: &PagePosition::First,
                    limit: 3,
                };
                let page = read_page::<OperationError>(conn.as_ref(), &req, &opts).await?;
                Ok((total, page))
            }
        })
        .await?;

    assert_eq!(total_count, 5);
    assert_eq!(page1.result.rows.len(), 3);

    // Analytical SQL: CTE + aggregation + window function.
    let analytics_sql = "
        WITH regional AS (
            SELECT region, SUM(revenue) AS total_rev
            FROM events
            GROUP BY region
        )
        SELECT region, total_rev, RANK() OVER (ORDER BY total_rev DESC) AS rnk
        FROM regional
        ORDER BY rnk
    ";
    let out = run_sql(&session, analytics_sql).await?;
    assert!(matches!(out.summary.end, RunEnd::Completed));
    assert!(
        out.events
            .iter()
            .any(|e| matches!(e, ResultEvent::ResultSetStarted { .. }))
    );
    let rs = out
        .accumulator
        .result_sets
        .first()
        .ok_or("expected result set")?;
    assert_eq!(rs.rows.len(), 3);
    assert_eq!(
        rs.rows[0].first(),
        Some(&Value::Text("eu-west".to_owned()))
    );

    shutdown_session(&session).await?;
    Ok(())
}

#[tokio::test]
async fn duckdb_export_results_csv_json_sql() -> E2eResult<()> {
    let dir = tempfile::tempdir()?;
    let db_path = dir.path().join("export.duckdb");
    create_empty_duckdb(&db_path).await?;
    let session = open_file_session(
        dir.path(),
        &db_path,
        Arc::new(DuckDbDriver),
        "duckdb",
        false,
        false,
    )
    .await?;
    seed_duckdb(&session).await?;

    let table = TableRef::unqualified("events");
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

    let mut grid = DataGridModel::new(DataGridLayoutScope::Table("events".to_owned()), 10);
    grid.reload_rows(page.result.columns.to_vec(), page.result.rows, Some(5));

    let csv_text = grid.export_csv(&CsvOptions::default())?;
    assert!(csv_text.contains("us-east"));
    assert!(csv_text.contains("eu-west"));

    let json_text = grid.export_json()?;
    let parsed: serde_json::Value = serde_json::from_str(&json_text)?;
    let arr = parsed.as_array().ok_or("expected JSON array")?;
    assert_eq!(arr.len(), 5);

    let sql_text = grid.export_sql_inserts(dialect.as_ref(), &table, &cols)?;
    assert!(sql_text.contains("INSERT INTO"));
    assert!(sql_text.contains("'ap-south'"));

    shutdown_session(&session).await?;
    Ok(())
}
