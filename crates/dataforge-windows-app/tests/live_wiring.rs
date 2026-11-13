//! End-to-end integration tests for live `AppController` wiring:
//! - Live SQLite connect -> DDL/DML -> schema sidebar -> DataGrid paging/sort/filter/edit/commit/export
//! - Live SQL editor execution, query history, saved queries, autocomplete, Vim mode, split pane
//! - Library folders, tags, favorites, reordering
//! - CLI pre-GUI flags (`--version`, `--help`, `--smoke-startup`)
//! - Theme JSON import from file
//! - Single-instance IPC file-open into live SQLite workspace
//! - Safe Mode + open transaction close guard

use std::collections::BTreeSet;
use std::ffi::OsString;

use dataforge_services::export::ExportFormat;
use dataforge_state::connection_window::WorkspaceId;
use dataforge_windows_app::app::{AppController, ScreenMode};
use dataforge_windows_app::cli_flags::{AppFlagOutcome, handle_app_flags};
use dataforge_windows_app::controller_types::{SplitPaneMode, VimMode};
use dataforge_windows_app::i18n::Locale;
use dataforge_windows_platform::{AppSettings, WindowsTheme};
use tablepro_core::filter::FilterOp;
use tablepro_core::meta::TableRef;
use tablepro_core::value::Value;
use tempfile::TempDir;

#[test]
fn cli_flags_handle_version_help_and_smoke_startup() {
    let mut ver_out = Vec::new();
    let ver = handle_app_flags(
        &[OsString::from("dataforge"), OsString::from("--version")],
        &mut ver_out,
    );
    assert_eq!(ver, AppFlagOutcome::Exit(0));
    let ver_str = String::from_utf8(ver_out).expect("utf8");
    assert!(ver_str.starts_with("dataforge "));

    let mut help_out = Vec::new();
    let help = handle_app_flags(
        &[OsString::from("dataforge"), OsString::from("--help")],
        &mut help_out,
    );
    assert_eq!(help, AppFlagOutcome::Exit(0));
    let help_str = String::from_utf8(help_out).expect("utf8");
    assert!(help_str.contains("--smoke-startup"));

    let mut smoke_out = Vec::new();
    let smoke = handle_app_flags(
        &[
            OsString::from("dataforge"),
            OsString::from("--smoke-startup"),
        ],
        &mut smoke_out,
    );
    assert_eq!(smoke, AppFlagOutcome::Exit(0));
    let smoke_str = String::from_utf8(smoke_out).expect("utf8");
    assert!(smoke_str.contains("dataforge smoke-startup ok:"));

    let mut cont_out = Vec::new();
    let cont = handle_app_flags(
        &[OsString::from("dataforge"), OsString::from("query")],
        &mut cont_out,
    );
    assert_eq!(cont, AppFlagOutcome::Continue);
}

#[test]
fn live_sqlite_roundtrip_sql_grid_edits_export_and_editor() {
    let tmp = TempDir::new().expect("tempdir");
    let db_path = tmp.path().join("workbench.sqlite");
    std::fs::File::create(&db_path).expect("create sqlite file");

    let mut app = AppController::new(AppSettings::default(), WindowsTheme::Dark, Locale::EnUs);
    app.connect_local_file_blocking(&db_path)
        .expect("connect sqlite");
    assert_eq!(app.screen_mode, ScreenMode::Workspace);
    assert!(!app.rail_items.is_empty());

    // Execute DDL + seed rows via live SQL editor.
    app.set_editor_sql(
        "CREATE TABLE items (id INTEGER PRIMARY KEY, name TEXT NOT NULL, qty INTEGER NOT NULL)",
    );
    let _ddl = app.execute_active_sql_blocking().expect("execute ddl");
    app.set_editor_sql(
        "INSERT INTO items (id, name, qty) VALUES (1, 'anvil', 4), (2, 'bellows', 2), (3, 'hammer', 7)",
    );
    let _dml = app.execute_active_sql_blocking().expect("execute dml");
    assert!(!app.query_history.is_empty());

    // Save and reload a query.
    app.save_current_query("all_items", Some("inventory".to_owned()));
    app.set_editor_sql("SELECT 1");
    assert!(app.load_saved_query("all_items"));
    assert!(app.editor_sql_text().contains("INSERT INTO items"));

    // Refresh schema sidebar and verify `items` is listed.
    app.refresh_schema_sidebar_blocking()
        .expect("refresh schema");
    assert!(
        app.cached_objects.iter().any(|o| o.name == "items"),
        "expected `items` in cached_objects; schemas={:?}, objects={:?}, notices={:?}",
        app.cached_schemas,
        app.cached_objects,
        app.query_notices
    );
    assert!(!app.sidebar_rows.is_empty());

    // Autocomplete + Vim + split pane toggles.
    app.refresh_editor_autocomplete("it");
    let popup = app
        .editor_autocomplete
        .as_ref()
        .expect("autocomplete popup");
    assert!(popup.items.iter().any(|item| item.label == "items"));

    app.toggle_vim_mode();
    assert_eq!(app.vim_mode, Some(VimMode::Normal));
    app.set_vim_mode(VimMode::Insert);
    assert_eq!(app.vim_mode, Some(VimMode::Insert));

    app.toggle_split_pane();
    assert_eq!(app.split_pane, SplitPaneMode::Horizontal);

    // Open `items` in DataGrid, sort, filter, edit, commit, and export.
    app.open_table_tab_blocking(TableRef::new(None, "items"))
        .expect("open table tab");
    assert_eq!(app.grid_rows.len(), 3);
    assert_eq!(app.grid_raw_rows.len(), 3);

    app.toggle_grid_sort_blocking(2).expect("sort qty");
    app.add_grid_filter_blocking("name", FilterOp::Eq, Some("anvil".to_owned()))
        .expect("filter anvil");
    assert_eq!(app.grid_rows.len(), 1);
    app.clear_grid_filters_blocking().expect("clear filters");
    assert_eq!(app.grid_rows.len(), 3);

    // Inline cell edit + commit to SQLite.
    app.select_grid_cell(0, 1);
    app.edit_grid_cell(0, 1, Value::Text("forged_anvil".to_owned()));
    assert!(app.grid_model.edits.has_pending_edits());
    let committed = app.commit_grid_edits_blocking().expect("commit edits");
    assert!(committed >= 1);
    assert!(!app.grid_model.edits.has_pending_edits());

    // Export grid to CSV.
    let csv_path = tmp.path().join("items.csv");
    let exported = app
        .export_active_grid(&csv_path, ExportFormat::Csv)
        .expect("export csv");
    assert_eq!(exported, 3);
    let csv_text = std::fs::read_to_string(&csv_path).expect("read csv");
    assert!(csv_text.contains("forged_anvil"));

    app.disconnect_active_workspace()
        .expect("disconnect workspace");
}

#[test]
fn ipc_file_open_theme_json_import_library_and_safe_mode_guard() {
    let tmp = TempDir::new().expect("tempdir");
    let db_path = tmp.path().join("ipc_target.sqlite");
    std::fs::File::create(&db_path).expect("create sqlite file");

    let mut app = AppController::new(AppSettings::default(), WindowsTheme::Light, Locale::EnUs);

    // Single-instance IPC file-open into live workspace.
    app.open_forwarded_targets(&[db_path.to_string_lossy().into_owned()]);
    assert_eq!(app.screen_mode, ScreenMode::Workspace);
    assert_eq!(app.rail_items.len(), 1);
    assert!(app.rail_items[0].connected);

    // Theme JSON file import.
    let theme_path = tmp.path().join("forge_theme.json");
    std::fs::write(
        &theme_path,
        br##"{
            "name": "Forge Ember",
            "type": "dark",
            "colors": {
                "editor.background": "#1a1816",
                "editor.foreground": "#f0e6dc"
            }
        }"##,
    )
    .expect("write theme json");
    app.import_vscode_theme_file(&theme_path)
        .expect("import theme file");
    assert_eq!(app.content_theme.name, "Forge Ember");

    // Library folder/tag/favorite/reorder.
    app.upsert_library_folder("prod", "Production", 0);
    let tags = BTreeSet::from(["analytics".to_owned()]);
    app.save_form_to_library("conn-1", Some("prod"), tags, false);
    assert!(app.toggle_library_favorite("conn-1"));
    assert!(app.reorder_library_connection("conn-1", Some("prod"), 0));

    // Safe Mode + open transaction close guard.
    assert!(!app.needs_close_guard());
    app.open_transactions.insert(WorkspaceId(1), true);
    assert!(app.needs_close_guard());
    app.open_transactions.insert(WorkspaceId(1), false);
    assert!(!app.needs_close_guard());

    app.safe_mode_enabled = true;
    app.insert_grid_row(vec![Value::Int(99)]);
    assert!(app.needs_close_guard());
}

#[test]
fn startup_state_has_no_mock_rows_columns_or_fake_status() {
    let app = AppController::new(AppSettings::default(), WindowsTheme::Dark, Locale::EnUs);
    assert!(app.grid_columns.is_empty());
    assert!(app.grid_rows.is_empty());
    assert_eq!(app.editor_sql_text(), "");
    assert_eq!(app.toolbar_status.host, "");
    assert_eq!(app.toolbar_status.engine_version, "Not connected");
    assert_eq!(app.toolbar_status.database, "—");
    assert_eq!(app.toolbar_status.latency_label, "—");
    assert_eq!(
        app.toolbar_status.format_pill_text(),
        "Not connected | — | —"
    );
}

#[test]
fn connect_library_item_hydrates_actual_saved_connection_fields()
-> Result<(), Box<dyn std::error::Error>> {
    use dataforge_gfx::glyphs::EngineKind;
    use dataforge_state::library::{LibraryConnection, LibraryConnectionId};

    let tmp = TempDir::new()?;
    let db_path = tmp.path().join("library_target.sqlite");
    std::fs::File::create(&db_path)?;

    let mut app = AppController::new(AppSettings::default(), WindowsTheme::Dark, Locale::EnUs);
    app.library.upsert_connection(LibraryConnection {
        id: LibraryConnectionId("remote-pg".to_owned()),
        name: "Analytics PG".to_owned(),
        driver_id: "postgres".to_owned(),
        host: "db.internal".to_owned(),
        port: Some(5433),
        database: "analytics".to_owned(),
        username: "analyst".to_owned(),
        group_id: None,
        tags: BTreeSet::new(),
        is_favorite: false,
        order: 0,
    });
    app.library.upsert_connection(LibraryConnection {
        id: LibraryConnectionId("local-sqlite".to_owned()),
        name: "Local SQLite".to_owned(),
        driver_id: "sqlite".to_owned(),
        host: String::new(),
        port: None,
        database: db_path.to_string_lossy().into_owned(),
        username: String::new(),
        group_id: None,
        tags: BTreeSet::new(),
        is_favorite: true,
        order: 1,
    });

    app.hydrate_connection_form_from_library("remote-pg")?;
    assert_eq!(app.connection_form.name, "Analytics PG");
    assert_eq!(app.connection_form.engine, EngineKind::Postgresql);
    assert_eq!(app.connection_form.host, "db.internal");
    assert_eq!(app.connection_form.port, 5433);
    assert_eq!(app.connection_form.database, "analytics");
    assert_eq!(app.connection_form.username, "analyst");

    // Connecting the SQLite library entry must replace the remote-pg draft with SQLite
    // and open the actual SQLite file instead of dialing Postgres defaults.
    app.connect_library_item_blocking("local-sqlite")?;
    assert_eq!(app.screen_mode, ScreenMode::Workspace);
    assert_eq!(app.connection_form.engine, EngineKind::Sqlite);
    assert_eq!(app.toolbar_status.engine_version, "SQLite");
    assert_eq!(app.toolbar_status.host, "local");
    Ok(())
}
