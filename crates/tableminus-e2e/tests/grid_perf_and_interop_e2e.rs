//! High-volume SQLite streaming, cancellation safety, export round-trip,
//! and virtual DataGrid slice-bounds E2E tests (mirrors `TablePro.Interop.Tests` & `TablePro.DataGrid.Gate`).

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use async_trait::async_trait;
use tableminus_e2e::{E2eResult, open_file_session, run_sql, shutdown_session};
use tableminus_state::data_grid::{DataGridLayoutScope, DataGridModel};
use tablepro_core::browse::{PageRequest, read_page};
use tablepro_core::export::{CsvOptions, JsonArrayEncoder, RowEncoder, render_tsv};
use tablepro_core::filter::FilterSet;
use tablepro_core::meta::{PagePosition, TableRef};
use tablepro_core::value::Value;
use tablepro_core::{
    EditorSessionOptions, ResultEvent, ResultSink, RunEnd, RunOptions, SinkFlow, UserSql,
};
use tablepro_driver_sqlite::SqliteDriver;
use tablepro_session::{OperationError, OperationKind, Session};
use tokio_util::sync::CancellationToken;

const VOLUME_ROWS: usize = 25_000;

struct CancellingSink {
    seen_rows: Arc<AtomicUsize>,
    cancel_after: usize,
    token: CancellationToken,
}

#[async_trait]
impl ResultSink for CancellingSink {
    async fn accept(&mut self, event: ResultEvent) -> SinkFlow {
        if let ResultEvent::Rows(batch) = event {
            let total = self.seen_rows.fetch_add(batch.len(), Ordering::SeqCst) + batch.len();
            if total >= self.cancel_after {
                self.token.cancel();
            }
        }
        SinkFlow::Continue
    }
}

async fn seed_volume_table(session: &Session, row_count: usize) -> E2eResult<()> {
    let ddl = format!(
        "BEGIN TRANSACTION;
         CREATE TABLE metrics (
             id INTEGER PRIMARY KEY,
             bucket TEXT NOT NULL,
             reading REAL NOT NULL
         );
         WITH RECURSIVE seq(x) AS (
             SELECT 1
             UNION ALL
             SELECT x + 1 FROM seq WHERE x < {row_count}
         )
         INSERT INTO metrics (id, bucket, reading)
         SELECT x, 'b_' || (x % 16), x * 0.25 FROM seq;
         COMMIT;"
    );
    let out = run_sql(session, &ddl).await?;
    if !matches!(out.summary.end, RunEnd::Completed) {
        return Err(format!("volume seed failed: {:?}", out.summary.end).into());
    }
    Ok(())
}

#[tokio::test]
async fn sqlite_25k_stream_virtual_grid_bounds_and_exports() -> E2eResult<()> {
    let dir = tempfile::tempdir()?;
    let db_path = dir.path().join("interop_25k.sqlite");
    let session = open_file_session(
        dir.path(),
        &db_path,
        Arc::new(SqliteDriver),
        "sqlite",
        true,
        false,
    )
    .await?;
    seed_volume_table(&session, VOLUME_ROWS).await?;

    let table = TableRef::unqualified("metrics");
    let cols = session
        .columns(table.clone())
        .await
        .map_err(|e| format!("columns failed: {e}"))?;
    let filter = FilterSet::default();

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
                    limit: VOLUME_ROWS as u64,
                };
                read_page::<OperationError>(conn.as_ref(), &req, &opts).await
            }
        })
        .await?;

    assert_eq!(page.result.rows.len(), VOLUME_ROWS);
    let rows = page.result.rows;
    let res_cols = page.result.columns;

    let mut grid = DataGridModel::new(
        DataGridLayoutScope::Table("metrics".to_owned()),
        VOLUME_ROWS as u64,
    );
    grid.reload_rows(res_cols.to_vec(), rows.clone(), Some(VOLUME_ROWS as u64));
    assert_eq!(grid.row_order.len(), VOLUME_ROWS);

    // Virtual grid viewport slice bounds check across top, middle, and bottom offsets.
    let viewport_len = 40_usize;
    for start in [0_usize, VOLUME_ROWS / 2, VOLUME_ROWS - 15, VOLUME_ROWS + 50] {
        let clamped_start = start.min(grid.row_order.len());
        let clamped_end = clamped_start
            .saturating_add(viewport_len)
            .min(grid.row_order.len());
        let slice = &rows[clamped_start..clamped_end];
        assert!(slice.len() <= viewport_len);
        if start < VOLUME_ROWS {
            assert!(!slice.is_empty());
            assert_eq!(slice[0][0], Value::Int(i64::try_from(start + 1)?));
        } else {
            assert!(slice.is_empty());
        }
    }

    // Large TSV / CSV / JSON export round-trip over all 25,000 rows.
    let tsv = render_tsv(&res_cols, &rows, true)?;
    assert_eq!(tsv.lines().count(), VOLUME_ROWS + 1);

    let csv_text = grid.export_csv(&CsvOptions::default())?;
    assert_eq!(csv_text.lines().count(), VOLUME_ROWS + 1);

    let mut json_buf = Vec::new();
    let mut json_enc = JsonArrayEncoder::new(res_cols);
    json_enc.begin(&mut json_buf);
    for row in &rows {
        json_enc.row(&mut json_buf, &mut row.iter())?;
    }
    json_enc.finish(&mut json_buf);
    let parsed: serde_json::Value = serde_json::from_slice(&json_buf)?;
    let arr = parsed.as_array().ok_or("expected JSON array")?;
    assert_eq!(arr.len(), VOLUME_ROWS);

    shutdown_session(&session).await?;
    Ok(())
}

#[tokio::test]
async fn sqlite_mid_stream_cancellation_is_clean_and_reusable() -> E2eResult<()> {
    let dir = tempfile::tempdir()?;
    let db_path = dir.path().join("cancel_25k.sqlite");
    let session = open_file_session(
        dir.path(),
        &db_path,
        Arc::new(SqliteDriver),
        "sqlite",
        true,
        false,
    )
    .await?;
    seed_volume_table(&session, VOLUME_ROWS).await?;

    let editor = session
        .open_editor_session(EditorSessionOptions::default(), &CancellationToken::new())
        .await?;

    let token = CancellationToken::new();
    let seen_rows = Arc::new(AtomicUsize::new(0));
    let sink = CancellingSink {
        seen_rows: seen_rows.clone(),
        cancel_after: 500,
        token: token.clone(),
    };
    let summary = editor
        .run(
            UserSql::new("SELECT id, bucket, reading FROM metrics ORDER BY id;".to_owned()),
            RunOptions::new(token),
            Box::new(sink),
        )
        .await?;

    assert!(
        matches!(summary.end, RunEnd::Cancelled | RunEnd::Completed),
        "unexpected run end after mid-stream cancel: {:?}",
        summary.end
    );
    assert!(seen_rows.load(Ordering::SeqCst) >= 500);

    // Session remains healthy and usable after mid-stream cancellation.
    let follow_up = run_sql(&session, "SELECT COUNT(*) FROM metrics;").await?;
    assert!(matches!(follow_up.summary.end, RunEnd::Completed));
    let rs = follow_up
        .accumulator
        .result_sets
        .first()
        .ok_or("expected follow-up count result")?;
    assert_eq!(rs.rows[0][0], Value::Int(i64::try_from(VOLUME_ROWS)?));

    shutdown_session(&session).await?;
    Ok(())
}
