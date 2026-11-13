//! Integration tests for startup CLI pre-dispatch, IPC target forwarding,
//! tab lifecycle, Safe Mode / transaction close guards, and theme/URL import.

use std::ffi::OsString;

use dataforge_state::connection_window::WorkspaceId;
use dataforge_state::tabs::CloseActionResult;
use dataforge_windows_app::app::{AppController, ScreenMode};
use dataforge_windows_app::i18n::Locale;
use dataforge_windows_app::ipc::extract_forward_targets;
use dataforge_windows_app::views::library::ConnectionFormDraft;
use dataforge_windows_platform::{AppSettings, WindowsTheme};
use tablepro_cli::{Dispatch, classify};

#[test]
fn cli_pre_dispatch_routes_subcommands_and_gui_targets() {
    let help = classify(&[OsString::from("dataforge"), OsString::from("--help")]);
    assert_eq!(help, Dispatch::Command);

    let query = classify(&[
        OsString::from("dataforge"),
        OsString::from("query"),
        OsString::from("-c"),
        OsString::from("local"),
        OsString::from("SELECT 1"),
    ]);
    assert_eq!(query, Dispatch::Command);

    let url = classify(&[
        OsString::from("dataforge"),
        OsString::from("postgres://user@localhost:5432/app"),
    ]);
    assert!(matches!(url, Dispatch::Gui(_)));

    let bogus = classify(&[
        OsString::from("dataforge"),
        OsString::from("nonexistent_cmd"),
    ]);
    assert!(matches!(bogus, Dispatch::Invalid(_)));
}

#[test]
fn ipc_extracts_db_files_and_connection_urls() {
    let args = vec![
        OsString::from("dataforge.exe"),
        OsString::from("--new-window"),
        OsString::from("C:\\data\\prod.sqlite"),
        OsString::from("analytics.duckdb"),
        OsString::from("postgres://localhost/mydb"),
        OsString::from("not_a_target"),
    ];
    let targets = extract_forward_targets(&args);
    assert_eq!(
        targets,
        vec![
            "C:\\data\\prod.sqlite",
            "analytics.duckdb",
            "postgres://localhost/mydb"
        ]
    );
}

#[test]
fn controller_opens_forwarded_targets_and_updates_rail() {
    let mut app = AppController::new(AppSettings::default(), WindowsTheme::Dark, Locale::EnUs);
    assert!(app.uses_dark_titlebar());
    app.open_forwarded_targets(&[
        "postgres://alice@db.internal:5432/orders".to_owned(),
        "C:\\tmp\\local.duckdb".to_owned(),
    ]);
    assert_eq!(app.rail_items.len(), 2);
    assert_eq!(app.screen_mode, ScreenMode::Workspace);
}

#[test]
fn ctrl_w_closes_frontmost_tab_before_window_and_reopens_with_ctrl_shift_t() {
    let mut app = AppController::new(AppSettings::default(), WindowsTheme::Light, Locale::EnUs);
    let initial_id = app.tabs.active_tab();
    assert!(initial_id.is_some());

    // Open a second tab, so Ctrl+W closes only the frontmost tab.
    let second_res =
        app.handle_key_command(dataforge_windows_app::accessibility::KeyCommand::NewQueryTab);
    assert!(!second_res);
    assert_eq!(app.tabs.tabs().len(), 2);

    assert!(matches!(
        app.close_frontmost_tab_or_window(),
        CloseActionResult::ClosedTab(_)
    ));
    assert_eq!(app.tabs.tabs().len(), 1);

    // Reopen closed tab.
    assert!(app.reopen_closed_tab().is_some());
    assert_eq!(app.tabs.tabs().len(), 2);

    // Closing all tabs eventually requests window close.
    assert!(matches!(
        app.close_frontmost_tab_or_window(),
        CloseActionResult::ClosedTab(_)
    ));
    assert!(matches!(
        app.close_frontmost_tab_or_window(),
        CloseActionResult::ClosedTab(_)
    ));
    assert_eq!(
        app.close_frontmost_tab_or_window(),
        CloseActionResult::CloseWindow
    );
}

#[test]
fn close_guard_triggers_for_open_transactions_and_safe_mode() {
    let mut app = AppController::new(AppSettings::default(), WindowsTheme::Light, Locale::EnUs);
    assert!(!app.needs_close_guard());

    app.open_transactions.insert(WorkspaceId(1), true);
    assert!(app.needs_close_guard());
    app.open_transactions.insert(WorkspaceId(1), false);
    assert!(!app.needs_close_guard());

    app.safe_mode_enabled = true;
    if let Some(id) = app.tabs.active_tab()
        && let Some(mut item) = app.tabs.detach_tab(id)
    {
        item.has_unsaved_edits = true;
        let conn = item.connection_id.clone();
        app.tabs.attach_tab(item, conn);
    }
    assert!(app.needs_close_guard());
}

#[test]
fn connection_form_parses_urls_and_theme_importer_accepts_vscode_json() {
    let mut form = ConnectionFormDraft::default();
    let conn = form.apply_url("mysql://admin@127.0.0.1:3307/shop");
    assert!(conn.is_ok());
    assert_eq!(form.host, "127.0.0.1");
    assert_eq!(form.port, 3307);
    assert_eq!(form.database, "shop");

    let mut app = AppController::new(AppSettings::default(), WindowsTheme::Dark, Locale::EnUs);
    let vscode_json = br##"{
        "name": "One Dark Test",
        "type": "dark",
        "colors": {
            "editor.background": "#282c34",
            "editor.foreground": "#abb2bf"
        }
    }"##;
    let imported = app.import_vscode_theme(vscode_json);
    assert!(imported.is_ok());
    assert_eq!(app.content_theme.name, "One Dark Test");
}
