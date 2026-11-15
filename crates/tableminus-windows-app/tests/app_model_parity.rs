//! Pure-Rust parity tests mirroring `TablePro.App.Model.Tests`:
//! - Crash-safe `WorkspaceSessionSnapshot` save/load/corrupt recovery + `AppController` restore
//! - Environment tagging (`Local`, `Development`, `Staging`, `Production`) + legacy settings migration
//! - Tab close / `Ctrl+W` close guard, dirty grid edit protection, SafeMode write gating
//! - Connection URL parsing (`postgres://`, `sqlite://`, `mysql://`), query history & saved queries
//! - `UpdateState` / `UpdaterManager` lifecycle transitions & 6-locale i18n parity

use std::collections::BTreeSet;

use tableminus_state::connection_window::WorkspaceId;
use tableminus_state::tabs::{CloseActionResult, TabItem, TabKind};
use tableminus_windows_app::accessibility::KeyCommand;
use tableminus_windows_app::app::AppController;
use tableminus_windows_app::i18n::{Locale, Strings, diagnostics_locale_table};
use tableminus_windows_app::updater::{
    InstallMode, UpdateEvent, UpdateState, UpdaterManager, VELOPACK_PACK_ID,
};
use tableminus_windows_app::views::library::ConnectionFormDraft;
use tableminus_windows_platform::{
    AppSettings, EnvironmentKind, SavedTabSnapshot, WORKSPACE_SESSION_SCHEMA_VERSION, WindowsTheme,
    WorkspaceSessionSnapshot, load_workspace_session, save_workspace_session,
};
use tablepro_core::SafeModeLevel;
use tablepro_core::meta::TableRef;
use tablepro_core::value::Value;
use tempfile::TempDir;
use velopack::{UpdateInfo, VelopackAsset};

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn sample_multi_tab_snapshot() -> WorkspaceSessionSnapshot {
    WorkspaceSessionSnapshot {
        version: WORKSPACE_SESSION_SCHEMA_VERSION,
        active_connection_id: Some("conn-prod-1".to_owned()),
        active_database: Some("analytics".to_owned()),
        selected_table: Some((Some("public".to_owned()), "orders".to_owned())),
        active_tab_index: 1,
        tabs: vec![
            SavedTabSnapshot {
                tab_id: "10".to_owned(),
                title: "Query 1".to_owned(),
                kind: "query".to_owned(),
                connection_id: Some("conn-prod-1".to_owned()),
                database: Some("analytics".to_owned()),
                schema: None,
                table: None,
                sql_draft: "SELECT count(*) FROM public.orders;".to_owned(),
                filter_clause: None,
                sort_clause: None,
                is_pinned: false,
            },
            SavedTabSnapshot {
                tab_id: "11".to_owned(),
                title: "orders".to_owned(),
                kind: "table".to_owned(),
                connection_id: Some("conn-prod-1".to_owned()),
                database: Some("analytics".to_owned()),
                schema: Some("public".to_owned()),
                table: Some("orders".to_owned()),
                sql_draft: String::new(),
                filter_clause: Some("status = 'paid'".to_owned()),
                sort_clause: Some("id DESC".to_owned()),
                is_pinned: true,
            },
            SavedTabSnapshot {
                tab_id: "12".to_owned(),
                title: "orders (Structure)".to_owned(),
                kind: "structure".to_owned(),
                connection_id: Some("conn-prod-1".to_owned()),
                database: Some("analytics".to_owned()),
                schema: Some("public".to_owned()),
                table: Some("orders".to_owned()),
                sql_draft: String::new(),
                filter_clause: None,
                sort_clause: None,
                is_pinned: false,
            },
        ],
        sidebar_open: false,
        sidebar_width: Some(312.0),
        inspector_open: true,
        bottom_drawer_open: true,
        saved_at_unix: 1_763_200_000,
    }
}

#[test]
fn workspace_session_save_load_and_corrupt_recovery() -> TestResult {
    let tmp = TempDir::new()?;
    let session_path = tmp.path().join("state").join("workspace_session.json");

    assert_eq!(load_workspace_session(Some(&session_path))?, None);

    let snapshot = sample_multi_tab_snapshot();
    save_workspace_session(Some(&session_path), &snapshot)?;
    assert_eq!(
        load_workspace_session(Some(&session_path))?,
        Some(snapshot.clone())
    );

    // Truncated / malformed JSON recovers cleanly to None.
    std::fs::write(
        &session_path,
        br#"{"envelope_version": 1, "checksum": 999, "payload": {"#,
    )?;
    assert_eq!(load_workspace_session(Some(&session_path))?, None);

    // Valid JSON with tampered payload fails FNV-1a checksum and recovers to None.
    save_workspace_session(Some(&session_path), &snapshot)?;
    let raw = std::fs::read_to_string(&session_path)?;
    let tampered = raw.replacen("\"orders\"", "\"invoices\"", 1);
    assert_ne!(raw, tampered);
    std::fs::write(&session_path, tampered)?;
    assert_eq!(load_workspace_session(Some(&session_path))?, None);
    Ok(())
}

#[test]
fn controller_restores_multi_tab_session_and_respects_restore_flag() -> TestResult {
    let tmp = TempDir::new()?;
    let session_path = tmp.path().join("workspace_session.json");
    let snapshot = sample_multi_tab_snapshot();

    let writer = AppController::new(AppSettings::default(), WindowsTheme::Dark, Locale::EnUs);
    writer.save_workspace_session_from_path(&session_path, &snapshot);

    // Restoring directly from path populates QueryEditor/TableData/Structure tabs & layout.
    let mut enabled_settings = AppSettings {
        restore_last_session: true,
        ..AppSettings::default()
    };
    enabled_settings
        .environment_tags
        .insert("conn-prod-1".to_owned(), EnvironmentKind::Production);
    let mut restored_app = AppController::new(enabled_settings, WindowsTheme::Dark, Locale::EnUs);
    let restored = restored_app
        .restore_workspace_session_from_path(&session_path, false)
        .ok_or("expected restored snapshot")?;
    assert_eq!(restored.tabs.len(), 3);
    assert_eq!(restored_app.tabs.tabs().len(), 3);
    assert_eq!(
        restored_app.tabs.active_tab(),
        Some(restored_app.tabs.tabs()[1].id)
    );
    assert_eq!(restored_app.tabs.tabs()[1].title, "orders");
    assert_eq!(
        restored_app.active_table,
        Some(TableRef::new(
            Some("public".to_owned()),
            "orders".to_owned()
        ))
    );
    assert!(!restored_app.layout_config.sidebar_visible);
    assert!((restored_app.layout_config.sidebar_width - 312.0).abs() < f32::EPSILON);
    assert!(restored_app.layout_config.inspector_visible);
    assert!(matches!(
        restored_app.tabs.tabs()[0].kind,
        TabKind::QueryEditor { .. }
    ));
    assert!(matches!(
        restored_app.tabs.tabs()[1].kind,
        TabKind::TableData { .. }
    ));
    assert!(matches!(
        restored_app.tabs.tabs()[2].kind,
        TabKind::Structure { .. }
    ));

    // When active tab is a QueryEditor, editor_lines hydrates its SQL draft.
    let mut query_active_snap = snapshot;
    query_active_snap.active_tab_index = 0;
    writer.save_workspace_session_from_path(&session_path, &query_active_snap);
    let mut query_app =
        AppController::new(AppSettings::default(), WindowsTheme::Dark, Locale::EnUs);
    let _ = query_app
        .restore_workspace_session_from_path(&session_path, false)
        .ok_or("expected query-active snapshot")?;
    assert_eq!(
        query_app.editor_sql_text(),
        "SELECT count(*) FROM public.orders;"
    );

    // Startup restore returns None when restore_last_session is disabled.
    let disabled_settings = AppSettings {
        restore_last_session: false,
        ..AppSettings::default()
    };
    let mut disabled_app = AppController::new(disabled_settings, WindowsTheme::Dark, Locale::EnUs);
    assert!(
        disabled_app
            .restore_workspace_session_on_startup()
            .is_none()
    );
    Ok(())
}

#[test]
fn environment_tags_roundtrip_and_legacy_settings_deserialize() -> TestResult {
    let tmp = TempDir::new()?;
    let settings_path = tmp.path().join("settings.json");

    let mut app = AppController::new(AppSettings::default(), WindowsTheme::Dark, Locale::EnUs);
    assert_eq!(
        app.connection_environment("missing"),
        EnvironmentKind::Local
    );

    app.set_connection_environment("conn-local", EnvironmentKind::Local);
    app.set_connection_environment("conn-dev", EnvironmentKind::Development);
    app.set_connection_environment("conn-stg", EnvironmentKind::Staging);
    app.set_connection_environment("conn-prod", EnvironmentKind::Production);

    app.settings.save_atomic(&settings_path)?;
    let reloaded = AppSettings::load(&settings_path)?;
    let restored = AppController::new(reloaded, WindowsTheme::Dark, Locale::EnUs);
    assert_eq!(
        restored.connection_environment("conn-local"),
        EnvironmentKind::Local
    );
    assert_eq!(
        restored.connection_environment("conn-dev"),
        EnvironmentKind::Development
    );
    assert_eq!(
        restored.connection_environment("conn-stg"),
        EnvironmentKind::Staging
    );
    assert_eq!(
        restored.connection_environment("conn-prod"),
        EnvironmentKind::Production
    );

    // Legacy settings JSON without `environment_tags` or `restore_last_session`.
    let legacy_path = tmp.path().join("legacy_settings.json");
    std::fs::write(
        &legacy_path,
        r#"{
            "theme": "dark",
            "restore_session": false
        }"#,
    )?;
    let legacy = AppSettings::load(&legacy_path)?;
    assert!(legacy.environment_tags.is_empty());
    assert!(legacy.restore_last_session);
    assert!(!legacy.restore_session);
    Ok(())
}

#[test]
fn tab_close_guard_dirty_grid_and_safe_mode_write_gating() -> TestResult {
    let mut app = AppController::new(AppSettings::default(), WindowsTheme::Light, Locale::EnUs);
    assert!(!app.needs_close_guard());
    assert_eq!(app.active_safe_mode(), SafeModeLevel::Off);

    // SafeMode toggles SafeModeLevel::ConfirmWrites.
    app.safe_mode_enabled = true;
    assert_eq!(app.active_safe_mode(), SafeModeLevel::ConfirmWrites);

    // Unsaved grid row triggers close guard while safe_mode_enabled is true.
    app.insert_grid_row(vec![Value::Int(42)]);
    assert!(app.grid_model.edits.has_pending_edits());
    assert!(app.needs_close_guard());
    app.rollback_grid_edits();
    assert!(!app.needs_close_guard());

    // Open transaction triggers close guard regardless of tab dirty state.
    app.open_transactions.insert(WorkspaceId(1), true);
    assert!(app.needs_close_guard());
    app.open_transactions.insert(WorkspaceId(1), false);
    assert!(!app.needs_close_guard());

    // Unsaved tab flag triggers close guard in safe mode.
    let active_id = app.tabs.active_tab().ok_or("expected initial tab")?;
    let mut item: TabItem = app
        .tabs
        .detach_tab(active_id)
        .ok_or("expected detached tab")?;
    item.has_unsaved_edits = true;
    let conn = item.connection_id.clone();
    let _ = app.tabs.attach_tab(item, conn);
    assert!(app.needs_close_guard());

    // Ctrl+W closes frontmost tab before requesting window close.
    let _ = app.handle_key_command(KeyCommand::NewQueryTab);
    assert_eq!(app.tabs.tabs().len(), 2);
    assert!(matches!(
        app.close_frontmost_tab_or_window(),
        CloseActionResult::ClosedTab(_)
    ));
    assert_eq!(app.tabs.tabs().len(), 1);
    assert!(matches!(
        app.close_frontmost_tab_or_window(),
        CloseActionResult::ClosedTab(_)
    ));
    assert_eq!(
        app.close_frontmost_tab_or_window(),
        CloseActionResult::CloseWindow
    );
    Ok(())
}

#[test]
fn library_url_parsing_query_history_and_saved_queries() -> TestResult {
    let mut app = AppController::new(AppSettings::default(), WindowsTheme::Dark, Locale::EnUs);

    let pg = app.parse_and_apply_connection_url("postgres://analyst@db.internal:5433/warehouse")?;
    assert_eq!(pg.driver_id, "postgres");
    assert_eq!(app.connection_form.host, "db.internal");
    assert_eq!(app.connection_form.port, 5433);
    assert_eq!(app.connection_form.database, "warehouse");
    assert_eq!(app.connection_form.username, "analyst");

    let mysql = app.parse_and_apply_connection_url("mysql://root@127.0.0.1:3307/orders")?;
    assert_eq!(mysql.driver_id, "mysql");
    assert_eq!(app.connection_form.port, 3307);
    assert_eq!(app.connection_form.database, "orders");

    let mut form = ConnectionFormDraft::default();
    form.apply_url("sqlite:///C:/data/app.sqlite")?;
    assert!(form.database.contains("app.sqlite"));

    // Query history & saved queries.
    app.toolbar_status.database = "warehouse".to_owned();
    app.set_editor_sql("SELECT id, total FROM invoices;");
    app.record_query_history("SELECT id, total FROM invoices;", 14, Some(3), true);
    assert_eq!(app.query_history.len(), 1);
    assert_eq!(app.query_history[0].database, "warehouse");
    assert!(app.query_history[0].succeeded);

    app.save_current_query("invoice_totals", Some("finance".to_owned()));
    app.set_editor_sql("SELECT 1;");
    assert!(app.load_saved_query("invoice_totals"));
    assert_eq!(app.editor_sql_text(), "SELECT id, total FROM invoices;");
    assert!(!app.load_saved_query("missing_query"));
    Ok(())
}

#[test]
fn update_state_modal_lifecycle_transitions_without_panicking() {
    let mut mgr = UpdaterManager::new();
    assert_eq!(mgr.install_mode(), InstallMode::Standalone);
    assert_eq!(mgr.state(), &UpdateState::StandaloneMode);
    assert!(!mgr.is_busy());

    mgr.apply_event(UpdateEvent::StateChanged(UpdateState::Idle));
    assert_eq!(mgr.state(), &UpdateState::Idle);

    mgr.apply_event(UpdateEvent::StateChanged(UpdateState::Checking));
    assert!(mgr.is_busy());

    let asset = VelopackAsset {
        PackageId: VELOPACK_PACK_ID.to_owned(),
        Version: "0.2.0".to_owned(),
        Type: "Full".to_owned(),
        FileName: "TableMinus-0.2.0-full.nupkg".to_owned(),
        NotesMarkdown: "- Added pure Rust tests".to_owned(),
        ..VelopackAsset::default()
    };
    let info = UpdateInfo {
        TargetFullRelease: asset.clone(),
        BaseRelease: None,
        DeltasToTarget: Vec::new(),
        IsDowngrade: false,
    };
    mgr.apply_event(UpdateEvent::Available {
        info: Box::new(info),
    });
    assert!(!mgr.is_busy());
    assert!(matches!(mgr.state(), UpdateState::Available { version, .. } if version == "0.2.0"));

    mgr.apply_event(UpdateEvent::StateChanged(UpdateState::Downloading {
        percent: 55,
    }));
    assert!(mgr.is_busy());
    assert_eq!(mgr.state(), &UpdateState::Downloading { percent: 55 });

    mgr.apply_event(UpdateEvent::Downloaded {
        asset: Box::new(asset),
    });
    assert!(!mgr.is_busy());
    assert_eq!(
        mgr.state(),
        &UpdateState::ReadyToRestart {
            version: "0.2.0".to_owned()
        }
    );

    mgr.apply_event(UpdateEvent::StateChanged(UpdateState::Error(
        "network timeout".to_owned(),
    )));
    assert!(!mgr.is_busy());
    assert_eq!(
        mgr.state(),
        &UpdateState::Error("network timeout".to_owned())
    );
}

#[test]
fn six_locale_i18n_structural_and_translation_parity() {
    let expected_codes = ["en-US", "vi-VN", "zh-CN", "ja-JP", "es-ES", "fr-FR"];
    let en = Strings::for_locale(Locale::EnUs);
    let actual_codes: Vec<&str> = Locale::ALL.iter().map(|l| l.code()).collect();
    assert_eq!(actual_codes, expected_codes);

    for locale in Locale::ALL {
        let s = Strings::for_locale(locale);
        let parity_fields = [
            s.settings_restore_session,
            s.settings_restore_session_desc,
            s.session_restoring_connection,
            s.schema_designer,
            s.schema_create_table,
            s.schema_alter_table,
            s.schema_execute_ddl,
            s.import_wizard,
            s.import_run_import,
            s.import_sql_script,
            s.grid_agg_count,
            s.grid_agg_sum,
            s.grid_agg_avg,
            s.grid_agg_min,
            s.grid_agg_max,
            s.grid_copy_rows,
            s.grid_paste_rows,
            s.grid_pin_column,
            s.editor_find,
            s.editor_replace,
            s.editor_replace_all,
            s.editor_format_sql,
            s.metadata_search,
            s.env_local,
            s.env_development,
            s.env_staging,
            s.env_production,
            s.er_diagram,
            s.explain_plan,
            s.charts_tab,
            s.query_builder,
            s.compare_title,
            s.admin_title,
            s.admin_scheduled_tasks,
            s.update_modal_title,
        ];
        for val in parity_fields {
            assert!(!val.trim().is_empty(), "empty i18n field in {locale:?}");
        }

        if locale != Locale::EnUs {
            let distinct_pairs = [
                (s.schema_designer, en.schema_designer),
                (s.import_wizard, en.import_wizard),
                (s.query_builder, en.query_builder),
                (s.compare_title, en.compare_title),
                (s.admin_title, en.admin_title),
                (s.env_development, en.env_development),
                (s.settings_restore_session, en.settings_restore_session),
                (s.update_modal_title, en.update_modal_title),
            ];
            for (loc_val, en_val) in distinct_pairs {
                assert_ne!(
                    loc_val, en_val,
                    "expected localized translation for {locale:?}"
                );
            }
        }

        let diag = diagnostics_locale_table(locale);
        assert_eq!(diag.locale(), locale.code());
        assert!(!diag.is_empty());
    }

    // Verify helper set covers all 4 EnvironmentKind variants via label_key().
    let env_keys: BTreeSet<&str> = EnvironmentKind::ALL.iter().map(|e| e.label_key()).collect();
    assert_eq!(env_keys.len(), 4);
}
