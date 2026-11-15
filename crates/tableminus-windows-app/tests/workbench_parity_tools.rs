//! End-to-end parity tests for TableMinus workbench tools on a real temporary SQLite DB
//! via `AppController::connect_local_file_blocking`:
//! 1. Schema Designer (`build_create_table_sql`, `build_alter_table_sql`, `execute_ddl_statements_blocking`, `fetch_table_definition_blocking`)
//! 2. Data Import (CSV, JSON Array, NDJSON, XLSX preview + `generate_import_sql_batches`) & SQL Script Runner (`execute_sql_script_blocking`)
//! 3. Visual EXPLAIN (`run_visual_explain_blocking` -> `ExplainNode` hierarchy)
//! 4. Visual Query Builder (`build_visual_query_sql` with JOIN/WHERE/GROUP BY/ORDER BY/LIMIT)
//! 5. Schema & Data Compare (`diff_table_schemas`, `diff_table_rows` -> live sync SQL)
//! 6. Scheduled Tasks & Admin (`upsert_scheduled_task`, `run_scheduled_task_now`, save/load persistence)

use std::io::Write;
use std::path::Path;

use tableminus_services::{
    AlterTableOp, ColumnMapping, ColumnSpec, CreateTableSpec, DatabaseEngine, ForeignKeySpec,
    ImportFormat, IndexSpec, JoinClause, JoinKind, VisualQuerySpec, WhereClause,
    build_alter_table_sql, build_create_table_sql, build_visual_query_sql, diff_table_rows,
    diff_table_schemas, generate_import_sql_batches, preview_import_file, read_all_import_rows,
};
use tableminus_windows_app::app::AppController;
use tableminus_windows_app::i18n::Locale;
use tableminus_windows_platform::{AppSettings, ScheduledTaskRecord, WindowsTheme};
use tablepro_core::QueryResult;
use tablepro_core::value::Value;
use tempfile::TempDir;

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn open_temp_sqlite(
    tmp: &TempDir,
    name: &str,
) -> Result<AppController, Box<dyn std::error::Error>> {
    let db_path = tmp.path().join(name);
    std::fs::File::create(&db_path)?;
    let mut app = AppController::new(AppSettings::default(), WindowsTheme::Dark, Locale::EnUs);
    app.connect_local_file_blocking(&db_path)?;
    Ok(app)
}

fn crc32_ieee(data: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFF_u32;
    for &byte in data {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
        }
    }
    !crc
}

fn write_stored_zip(path: &Path, entries: &[(&str, &[u8])]) -> TestResult {
    let mut file = std::fs::File::create(path)?;
    let mut central_dir = Vec::new();
    let mut offset = 0_u32;

    for &(name, data) in entries {
        let name_bytes = name.as_bytes();
        let crc = crc32_ieee(data);
        let len = u32::try_from(data.len())?;
        let name_len = u16::try_from(name_bytes.len())?;

        // Local file header (30 bytes + filename).
        let mut lfh = Vec::with_capacity(30 + name_bytes.len());
        lfh.extend_from_slice(&0x0403_4B50_u32.to_le_bytes());
        lfh.extend_from_slice(&20_u16.to_le_bytes());
        lfh.extend_from_slice(&0_u16.to_le_bytes());
        lfh.extend_from_slice(&0_u16.to_le_bytes());
        lfh.extend_from_slice(&0_u16.to_le_bytes());
        lfh.extend_from_slice(&0_u16.to_le_bytes());
        lfh.extend_from_slice(&crc.to_le_bytes());
        lfh.extend_from_slice(&len.to_le_bytes());
        lfh.extend_from_slice(&len.to_le_bytes());
        lfh.extend_from_slice(&name_len.to_le_bytes());
        lfh.extend_from_slice(&0_u16.to_le_bytes());
        lfh.extend_from_slice(name_bytes);
        file.write_all(&lfh)?;
        file.write_all(data)?;

        // Central directory header (46 bytes + filename).
        central_dir.extend_from_slice(&0x0201_4B50_u32.to_le_bytes());
        central_dir.extend_from_slice(&20_u16.to_le_bytes());
        central_dir.extend_from_slice(&20_u16.to_le_bytes());
        central_dir.extend_from_slice(&0_u16.to_le_bytes());
        central_dir.extend_from_slice(&0_u16.to_le_bytes());
        central_dir.extend_from_slice(&0_u16.to_le_bytes());
        central_dir.extend_from_slice(&0_u16.to_le_bytes());
        central_dir.extend_from_slice(&crc.to_le_bytes());
        central_dir.extend_from_slice(&len.to_le_bytes());
        central_dir.extend_from_slice(&len.to_le_bytes());
        central_dir.extend_from_slice(&name_len.to_le_bytes());
        central_dir.extend_from_slice(&0_u16.to_le_bytes());
        central_dir.extend_from_slice(&0_u16.to_le_bytes());
        central_dir.extend_from_slice(&0_u16.to_le_bytes());
        central_dir.extend_from_slice(&0_u16.to_le_bytes());
        central_dir.extend_from_slice(&0_u32.to_le_bytes());
        central_dir.extend_from_slice(&offset.to_le_bytes());
        central_dir.extend_from_slice(name_bytes);

        offset = offset
            .checked_add(u32::try_from(lfh.len())?)
            .and_then(|v| v.checked_add(len))
            .ok_or("zip offset overflow")?;
    }

    let cd_size = u32::try_from(central_dir.len())?;
    let entry_count = u16::try_from(entries.len())?;
    file.write_all(&central_dir)?;

    // End of central directory record (22 bytes).
    let mut eocd = Vec::with_capacity(22);
    eocd.extend_from_slice(&0x0605_4B50_u32.to_le_bytes());
    eocd.extend_from_slice(&0_u16.to_le_bytes());
    eocd.extend_from_slice(&0_u16.to_le_bytes());
    eocd.extend_from_slice(&entry_count.to_le_bytes());
    eocd.extend_from_slice(&entry_count.to_le_bytes());
    eocd.extend_from_slice(&cd_size.to_le_bytes());
    eocd.extend_from_slice(&offset.to_le_bytes());
    eocd.extend_from_slice(&0_u16.to_le_bytes());
    file.write_all(&eocd)?;
    file.flush()?;
    Ok(())
}

fn sample_schema_designer_ddl() -> Vec<String> {
    let dept_sql = build_create_table_sql(
        DatabaseEngine::Sqlite,
        None,
        &CreateTableSpec {
            table_name: "departments".to_owned(),
            columns: vec![
                ColumnSpec {
                    name: "id".to_owned(),
                    data_type: "INTEGER".to_owned(),
                    is_primary_key: true,
                    auto_increment: true,
                    ..ColumnSpec::default()
                },
                ColumnSpec {
                    name: "name".to_owned(),
                    data_type: "TEXT".to_owned(),
                    ..ColumnSpec::default()
                },
            ],
            foreign_keys: Vec::new(),
            indexes: Vec::new(),
            if_not_exists: true,
        },
    );

    let emp_sql = build_create_table_sql(
        DatabaseEngine::Sqlite,
        None,
        &CreateTableSpec {
            table_name: "employees".to_owned(),
            columns: vec![
                ColumnSpec {
                    name: "id".to_owned(),
                    data_type: "INTEGER".to_owned(),
                    is_primary_key: true,
                    auto_increment: true,
                    ..ColumnSpec::default()
                },
                ColumnSpec {
                    name: "dept_id".to_owned(),
                    data_type: "INTEGER".to_owned(),
                    ..ColumnSpec::default()
                },
                ColumnSpec {
                    name: "full_name".to_owned(),
                    data_type: "TEXT".to_owned(),
                    ..ColumnSpec::default()
                },
            ],
            foreign_keys: vec![ForeignKeySpec {
                name: "fk_emp_dept".to_owned(),
                column: "dept_id".to_owned(),
                ref_table: "departments".to_owned(),
                ref_column: "id".to_owned(),
                on_delete: Some("CASCADE".to_owned()),
                on_update: None,
            }],
            indexes: Vec::new(),
            if_not_exists: true,
        },
    );

    let alter_stmts = build_alter_table_sql(
        DatabaseEngine::Sqlite,
        None,
        "employees",
        &[
            AlterTableOp::AddColumn(ColumnSpec {
                name: "email".to_owned(),
                data_type: "TEXT".to_owned(),
                nullable: true,
                ..ColumnSpec::default()
            }),
            AlterTableOp::AddIndex(IndexSpec {
                name: "idx_employees_email".to_owned(),
                columns: vec!["email".to_owned()],
                unique: true,
            }),
        ],
    );

    let mut ddl_batch = vec![dept_sql, emp_sql];
    ddl_batch.extend(alter_stmts);
    ddl_batch
}

#[test]
fn schema_designer_create_alter_and_introspect_live_sqlite() -> TestResult {
    let tmp = TempDir::new()?;
    let mut app = open_temp_sqlite(&tmp, "schema_designer.sqlite")?;

    let ddl_batch = sample_schema_designer_ddl();
    let executed = app.execute_ddl_statements_blocking(&ddl_batch)?;
    assert_eq!(executed, 4);

    let def = app.fetch_table_definition_blocking(None, "employees")?;
    let col_names: Vec<&str> = def.columns.iter().map(|c| c.name()).collect();
    assert_eq!(col_names, vec!["id", "dept_id", "full_name", "email"]);
    assert!(
        def.foreign_keys
            .iter()
            .any(|fk| fk.ref_table == "departments"),
        "expected FK to departments in {:?}",
        def.foreign_keys
    );
    assert!(
        def.indexes
            .iter()
            .any(|idx| idx.name == "idx_employees_email"),
        "expected idx_employees_email in {:?}",
        def.indexes
    );

    let all_defs = app.fetch_all_table_definitions_blocking()?;
    assert!(all_defs.len() >= 2);
    Ok(())
}

#[test]
fn data_import_formats_and_sql_script_runner_live_sqlite() -> TestResult {
    let tmp = TempDir::new()?;
    let mut app = open_temp_sqlite(&tmp, "import_runner.sqlite")?;

    // 1. CSV preview + batched SQL import into live SQLite.
    let csv_path = tmp.path().join("products.csv");
    std::fs::write(
        &csv_path,
        "id,sku,price\n1,ANVIL-1,49.5\n2,HAMMER-2,19.25\n",
    )?;
    let csv_preview = preview_import_file(&csv_path, Some(ImportFormat::Csv), 5)?;
    assert_eq!(csv_preview.columns, vec!["id", "sku", "price"]);
    assert_eq!(csv_preview.total_rows, 2);

    let (cols, rows) = read_all_import_rows(&csv_path, Some(ImportFormat::Csv))?;
    let mappings: Vec<ColumnMapping> = cols
        .iter()
        .zip(csv_preview.inferred_types.iter())
        .enumerate()
        .map(|(i, (c, ty))| ColumnMapping {
            source_index: i,
            target_column: c.clone(),
            target_type: ty.clone(),
            include: true,
        })
        .collect();
    let import_batches = generate_import_sql_batches(
        DatabaseEngine::Sqlite,
        None,
        "products",
        true,
        &mappings,
        &rows,
        1,
    );
    let script = import_batches.join("\n");
    let (stmt_count, _) = app.execute_sql_script_blocking(&script)?;
    assert_eq!(stmt_count, 3);

    let (res_cols, res_rows) =
        app.execute_query_capture_blocking("SELECT id, sku, price FROM products ORDER BY id;")?;
    assert_eq!(res_cols.len(), 3);
    assert_eq!(res_rows.len(), 2);
    assert_eq!(res_rows[0][1], Value::Text("ANVIL-1".to_owned()));

    // 2. JSON Array, NDJSON, and minimal XLSX previews.
    let json_path = tmp.path().join("events.json");
    std::fs::write(
        &json_path,
        r#"[{"id": 10, "kind": "open"}, {"id": 11, "kind": "save"}]"#,
    )?;
    let json_preview = preview_import_file(&json_path, None, 10)?;
    assert_eq!(json_preview.columns, vec!["id", "kind"]);
    assert_eq!(json_preview.total_rows, 2);

    let ndjson_path = tmp.path().join("logs.ndjson");
    std::fs::write(&ndjson_path, "{\"lvl\":\"info\"}\n{\"lvl\":\"warn\"}\n")?;
    let ndjson_preview = preview_import_file(&ndjson_path, None, 10)?;
    assert_eq!(ndjson_preview.columns, vec!["lvl"]);
    assert_eq!(ndjson_preview.total_rows, 2);

    let xlsx_path = tmp.path().join("sheet.xlsx");
    write_stored_zip(
        &xlsx_path,
        &[
            (
                "xl/sharedStrings.xml",
                br#"<sst><si><t>id</t></si><si><t>city</t></si><si><t>Tokyo</t></si></sst>"#,
            ),
            (
                "xl/worksheets/sheet1.xml",
                br#"<worksheet><sheetData><row r="1"><c r="A1" t="s"><v>0</v></c><c r="B1" t="s"><v>1</v></c></row><row r="2"><c r="A2"><v>7</v></c><c r="B2" t="s"><v>2</v></c></row></sheetData></worksheet>"#,
            ),
        ],
    )?;
    let xlsx_preview = preview_import_file(&xlsx_path, Some(ImportFormat::Xlsx), 10)?;
    assert_eq!(xlsx_preview.columns, vec!["id", "city"]);
    assert_eq!(xlsx_preview.total_rows, 1);
    assert_eq!(xlsx_preview.sample_rows[0], vec!["7", "Tokyo"]);
    Ok(())
}

#[test]
fn visual_explain_and_query_builder_on_live_sqlite() -> TestResult {
    let tmp = TempDir::new()?;
    let mut app = open_temp_sqlite(&tmp, "explain_qb.sqlite")?;

    let _ = app.execute_sql_script_blocking(
        "CREATE TABLE teams (id INTEGER PRIMARY KEY, name TEXT NOT NULL);
         CREATE TABLE members (id INTEGER PRIMARY KEY, team_id INTEGER NOT NULL, score INTEGER NOT NULL);
         CREATE INDEX idx_members_team ON members(team_id);
         INSERT INTO teams VALUES (1, 'Core'), (2, 'Infra');
         INSERT INTO members VALUES (1, 1, 90), (2, 1, 80), (3, 2, 70);",
    )?;

    // Visual EXPLAIN on indexed lookup parses ExplainNode hierarchy without clobbering grid.
    let nodes =
        app.run_visual_explain_blocking("SELECT id, score FROM members WHERE team_id = 1;", false)?;
    assert!(!nodes.is_empty());
    assert_eq!(app.last_explain_nodes, nodes);
    assert!(app.grid_raw_rows.is_empty());
    assert!(
        nodes
            .iter()
            .any(|n| n.detail.contains("idx_members_team") || n.label.contains("members")),
        "expected index/table reference in explain nodes: {nodes:?}"
    );

    // Visual Query Builder generates valid SQL with JOIN, WHERE, GROUP BY, ORDER BY, LIMIT.
    let qb_sql = build_visual_query_sql(
        DatabaseEngine::Sqlite,
        &VisualQuerySpec {
            table: "teams".to_owned(),
            schema: None,
            selected_columns: vec![
                "teams.name".to_owned(),
                "SUM(members.score) AS total_score".to_owned(),
            ],
            joins: vec![JoinClause {
                kind: JoinKind::Inner,
                table: "members".to_owned(),
                alias: None,
                left_col: "teams.id".to_owned(),
                right_col: "members.team_id".to_owned(),
            }],
            filters: vec![WhereClause {
                column: "members.score".to_owned(),
                operator: ">=".to_owned(),
                value: "70".to_owned(),
                logical_op: "AND".to_owned(),
            }],
            group_by: vec!["teams.name".to_owned()],
            order_by: vec![("total_score".to_owned(), false)],
            limit: Some(5),
        },
    );
    let (_, rows) = app.execute_query_capture_blocking(&qb_sql)?;
    assert_eq!(rows.len(), 2);
    assert_eq!(
        rows[0],
        vec![Value::Text("Core".to_owned()), Value::Int(170)]
    );
    assert_eq!(
        rows[1],
        vec![Value::Text("Infra".to_owned()), Value::Int(70)]
    );
    Ok(())
}

#[test]
fn schema_and_data_compare_sync_live_sqlite() -> TestResult {
    let tmp = TempDir::new()?;
    let mut app = open_temp_sqlite(&tmp, "compare_sync.sqlite")?;

    let _ = app.execute_sql_script_blocking(
        "CREATE TABLE src_items (id INTEGER PRIMARY KEY, name TEXT NOT NULL, notes TEXT);
         CREATE TABLE tgt_items (id INTEGER PRIMARY KEY, name TEXT NOT NULL);
         INSERT INTO src_items VALUES (1, 'Alpha', NULL), (2, 'Beta Updated', 'v2'), (3, 'Gamma', 'v3');
         INSERT INTO tgt_items VALUES (1, 'Alpha'), (2, 'Beta Old'), (9, 'Extra');",
    )?;

    // Schema compare detects missing `notes` column on target and syncs DDL.
    let src_def = app.fetch_table_definition_blocking(None, "src_items")?;
    let tgt_def = app.fetch_table_definition_blocking(None, "tgt_items")?;
    let schema_diff = diff_table_schemas(DatabaseEngine::Sqlite, &src_def, &tgt_def);
    assert!(!schema_diff.is_identical);
    assert_eq!(schema_diff.added_columns, vec!["notes"]);
    assert!(!schema_diff.sync_ddl.is_empty());

    let _ = app.execute_ddl_statements_blocking(&schema_diff.sync_ddl)?;
    let tgt_after_ddl = app.fetch_table_definition_blocking(None, "tgt_items")?;
    let schema_recheck = diff_table_schemas(DatabaseEngine::Sqlite, &src_def, &tgt_after_ddl);
    assert!(schema_recheck.is_identical);

    // Data compare produces INSERT/UPDATE/DELETE sync SQL making target match source.
    let (src_cols, src_rows) =
        app.execute_query_capture_blocking("SELECT id, name, notes FROM src_items ORDER BY id;")?;
    let (tgt_cols, tgt_rows) =
        app.execute_query_capture_blocking("SELECT id, name, notes FROM tgt_items ORDER BY id;")?;
    let src_qr = QueryResult::new(src_cols, src_rows);
    let tgt_qr = QueryResult::new(tgt_cols, tgt_rows);

    let data_diff = diff_table_rows(
        DatabaseEngine::Sqlite,
        None,
        "tgt_items",
        &["id".to_owned()],
        &src_qr,
        &tgt_qr,
    );
    assert_eq!(data_diff.only_in_source.len(), 1);
    assert_eq!(data_diff.only_in_target.len(), 1);
    assert_eq!(data_diff.modified.len(), 1);

    let sync_script = data_diff.sync_sql.join("\n");
    let _ = app.execute_sql_script_blocking(&sync_script)?;

    let (src_cols2, src_rows2) =
        app.execute_query_capture_blocking("SELECT id, name, notes FROM src_items ORDER BY id;")?;
    let (tgt_cols2, tgt_rows2) =
        app.execute_query_capture_blocking("SELECT id, name, notes FROM tgt_items ORDER BY id;")?;
    let final_diff = diff_table_rows(
        DatabaseEngine::Sqlite,
        None,
        "tgt_items",
        &["id".to_owned()],
        &QueryResult::new(src_cols2, src_rows2),
        &QueryResult::new(tgt_cols2, tgt_rows2),
    );
    assert!(final_diff.only_in_source.is_empty());
    assert!(final_diff.only_in_target.is_empty());
    assert!(final_diff.modified.is_empty());
    assert_eq!(final_diff.matching_count, 3);
    Ok(())
}

#[test]
fn scheduled_tasks_execute_on_sqlite_and_persist_history() -> TestResult {
    let tmp = TempDir::new()?;
    let mut app = open_temp_sqlite(&tmp, "scheduled_tasks.sqlite")?;
    let tasks_path = tmp.path().join("scheduled_tasks.json");

    let _ = app.execute_sql_script_blocking(
        "CREATE TABLE audit_log (id INTEGER PRIMARY KEY AUTOINCREMENT, note TEXT NOT NULL);",
    )?;

    app.upsert_scheduled_task(ScheduledTaskRecord {
        id: "task-audit".to_owned(),
        name: "Insert Audit Row".to_owned(),
        connection_id: String::new(),
        task_kind: "sql".to_owned(),
        sql_or_target: "INSERT INTO audit_log (note) VALUES ('cron-tick');".to_owned(),
        interval_minutes: 15,
        enabled: true,
        last_run_unix: None,
        last_status: None,
        last_duration_ms: None,
        run_history: Vec::new(),
    });

    let entry = app.run_scheduled_task_now("task-audit")?;
    assert!(entry.success);
    assert_eq!(app.scheduled_tasks[0].run_history.len(), 1);
    assert!(app.scheduled_tasks[0].last_run_unix.is_some());

    let (_, rows) = app.execute_query_capture_blocking("SELECT note FROM audit_log;")?;
    assert_eq!(rows, vec![vec![Value::Text("cron-tick".to_owned())]]);

    app.save_scheduled_tasks_to_path(&tasks_path)?;
    let mut reloaded = AppController::new(AppSettings::default(), WindowsTheme::Dark, Locale::EnUs);
    reloaded.load_scheduled_tasks_from_path(&tasks_path);
    assert_eq!(reloaded.scheduled_tasks.len(), 1);
    assert_eq!(reloaded.scheduled_tasks[0].id, "task-audit");
    assert_eq!(reloaded.scheduled_tasks[0].run_history.len(), 1);
    Ok(())
}
