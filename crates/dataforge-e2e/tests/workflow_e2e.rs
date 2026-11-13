//! Cross-crate State + Platform + Gfx E2E tests:
//! 1. Theme JSON import -> `ThemeResolver` -> `DataGridRenderer` / `SqlEditorRenderer` layout
//! 2. `ConnectionWindowPhaseMachine` + `TabStripModel` session restore & crash recovery
//! 3. `WindowsCredentialStore` + `StoragePaths` integration

use std::collections::BTreeMap;

use dataforge_e2e::E2eResult;
use dataforge_gfx::editor_renderer::{
    AutocompleteItem, AutocompletePopup, CompletionKind, EditorLayout, EditorLine,
    SqlEditorRenderer, SqlTokenKind, tokenize_sql_line,
};
use dataforge_gfx::grid_renderer::{DataGridRenderer, GridColumn, GridHitTest, GridLayout};
use dataforge_gfx::palette::{Color, ThemePalette};
use dataforge_state::connection_window::{
    ConnectionUnavailableView, ConnectionWindowPhase, ConnectionWindowPhaseMachine, WorkspaceId,
};
use dataforge_state::tabs::{
    CrashSafeTabPersistence, RestoredTabMerge, SessionRestorePlanner, TabConnectionId,
    TabStripModel,
};
use dataforge_state::theme::{
    Rgba, ThemeAppearance, ThemeFileFormat, ThemeResolver, ThemeScope, ThemeSlot,
    VsCodeThemeImporter,
};
use dataforge_windows_platform::{WindowsCredentialStore, storage_paths_for};
use secrecy::{ExposeSecret, SecretString};
use tablepro_core::credentials::{SecretKind, SecretLookup, SecretVault};
use uuid::Uuid;

#[test]
fn theme_import_resolve_and_gfx_renderer_layout() -> E2eResult<()> {
    let vscode_json = br##"{
        "name": "DataForge One Dark",
        "type": "dark",
        "colors": {
            "editor.background": "#1E222A",
            "editor.foreground": "#ABB2BF",
            "sideBar.background": "#181B22",
            "focusBorder": "#61AFEF"
        },
        "tokenColors": [
            {
                "scope": ["keyword"],
                "settings": { "foreground": "#C678DD" }
            },
            {
                "scope": ["string"],
                "settings": { "foreground": "#98C379" }
            }
        ]
    }"##;

    let imported = VsCodeThemeImporter::import_bytes(vscode_json)?;
    assert_eq!(imported.appearance, ThemeAppearance::Dark);

    // Round-trip through ThemeFileFormat encode/decode.
    let encoded = ThemeFileFormat::encode(&imported)?;
    let decoded = ThemeFileFormat::decode(&encoded)?;
    assert_eq!(decoded.name, imported.name);

    let mut catalog = BTreeMap::new();
    catalog.insert(decoded.id.clone(), decoded.clone());

    let resolved = ThemeResolver::resolve(
        Some(&decoded.id),
        ThemeAppearance::Dark,
        &ThemeScope::Live,
        &catalog,
    );
    assert!(!resolved.used_fallback);

    let bg = resolved.color_or_fallback(ThemeSlot::EditorBackground, Rgba::rgb(0, 0, 0));
    let packed = (u32::from(bg.r) << 24)
        | (u32::from(bg.g) << 16)
        | (u32::from(bg.b) << 8)
        | u32::from(bg.a);
    let gfx_bg = Color::from_rgba_hex(packed);
    let mut palette = ThemePalette::dark();
    palette.content.bg = gfx_bg;
    assert_eq!(palette.content.bg.to_packed_rgba(), gfx_bg.to_packed_rgba());

    // Verify DataGridRenderer layout & hit testing.
    let grid_renderer = DataGridRenderer::new("Consolas");
    assert_eq!(grid_renderer.font_family, "Consolas");
    let columns = vec![
        GridColumn::new("id", 80.0),
        GridColumn::new("name", 160.0),
        GridColumn::new("email", 220.0),
    ];
    let grid_layout = GridLayout {
        frozen_columns: 1,
        ..GridLayout::default()
    };
    assert!((grid_layout.frozen_pane_width(&columns) - 80.0).abs() < f32::EPSILON);
    assert!(grid_layout.scrollable_origin_x(&columns) >= 80.0);
    let visible_rows = grid_layout.visible_rows(100);
    assert!(!visible_rows.is_empty());
    let hit = DataGridRenderer::hit_test(120.0, 48.0, &grid_layout, &columns, 10);
    assert!(!matches!(hit, GridHitTest::None));

    // Verify SqlEditorRenderer tokenization & layout.
    let editor_renderer = SqlEditorRenderer::new("Consolas");
    assert_eq!(editor_renderer.font_family, "Consolas");
    let tokens = tokenize_sql_line("SELECT id, name FROM users WHERE name = 'Alice'");
    assert!(tokens.iter().any(|t| t.kind == SqlTokenKind::Keyword));
    assert!(tokens.iter().any(|t| t.kind == SqlTokenKind::StringLiteral));

    let lines = vec![
        EditorLine::new("SELECT id, name", 1),
        EditorLine::new("FROM users", 1),
    ];
    let editor_layout = EditorLayout::default();
    assert_eq!(editor_layout.visible_lines(lines.len()), 0..2);
    assert!(editor_layout.text_origin_x() > 0.0);
    let popup = AutocompletePopup {
        visible: true,
        anchor: (0, 7),
        selected: 0,
        items: vec![AutocompleteItem {
            label: "users".to_owned(),
            detail: "table".to_owned(),
            kind: CompletionKind::Table,
        }],
    };
    let (_x, _y, w, h) = editor_layout.popup_rect(&popup);
    assert!(w > 0.0 && h > 0.0);
    let _editor_hit =
        SqlEditorRenderer::hit_test(100.0, 12.0, &editor_layout, &lines, Some(&popup));

    Ok(())
}

#[test]
fn connection_window_phase_and_tab_strip_crash_recovery() -> E2eResult<()> {
    let dir = tempfile::tempdir()?;
    let mut machine = ConnectionWindowPhaseMachine::new();
    let ws = WorkspaceId(1);
    machine.add_workspace(ws);
    assert!(matches!(
        machine.phase(ws),
        Some(ConnectionWindowPhase::Disconnected)
    ));

    let token = machine
        .begin_connect(ws)
        .ok_or("expected attempt token from begin_connect")?;
    assert!(matches!(
        machine.phase(ws),
        Some(ConnectionWindowPhase::Connecting { .. })
    ));
    assert!(machine.complete_connect(ws, token));
    assert!(matches!(
        machine.phase(ws),
        Some(ConnectionWindowPhase::Connected)
    ));

    machine.disconnect(ws);
    assert!(matches!(
        machine.phase(ws),
        Some(ConnectionWindowPhase::Disconnected)
    ));
    let retry = machine
        .begin_reconnect(ws)
        .ok_or("expected attempt token from begin_reconnect")?;
    assert!(machine.fail_connect(
        ws,
        retry,
        ConnectionUnavailableView {
            summary: "connection refused".to_owned(),
            detail: None,
            can_retry: true,
            is_auth_failure: false,
        },
    ));
    assert!(matches!(
        machine.phase(ws),
        Some(ConnectionWindowPhase::Unavailable(_))
    ));
    assert!(machine.pane_rebuild_count(ws) >= 2);

    // Drive TabStripModel + CrashSafeTabPersistence + SessionRestorePlanner.
    let conn_id = TabConnectionId("conn-sqlite-1".to_owned());
    let mut strip = TabStripModel::new();
    let q1 = strip.open_query_tab(conn_id.clone(), Some("Query 1"));
    let t1 = strip.open_table_tab(conn_id.clone(), "users");
    assert_eq!(strip.tabs().len(), 2);
    assert!(strip.drag_reorder(0, 1));
    assert!(strip.close_tab_by_user(q1));
    assert_eq!(strip.tabs().len(), 1);
    let reopened = strip.reopen_closed_tab();
    assert!(reopened.is_some());
    assert_eq!(strip.tabs().len(), 2);
    assert!(strip.select(t1));

    let strips = BTreeMap::from([(conn_id.clone(), strip.tabs().to_vec())]);
    let primary_file = dir.path().join("tabs-session.json");
    let backup_file = dir.path().join("tabs-session.bak");
    CrashSafeTabPersistence::save_atomic(&primary_file, &backup_file, &strips)?;
    // Write again so backup is populated, then corrupt primary to exercise backup recovery.
    CrashSafeTabPersistence::save_atomic(&primary_file, &backup_file, &strips)?;
    std::fs::write(&primary_file, b"{corrupted-json")?;

    let recovered = CrashSafeTabPersistence::load_with_recovery(&primary_file, &backup_file)?;
    let restored_tabs = recovered
        .get(&conn_id)
        .ok_or("expected recovered tabs for connection")?;
    assert_eq!(restored_tabs.len(), 2);

    let plan = SessionRestorePlanner::plan(&[(conn_id, ConnectionWindowPhase::Connected)], 0);
    assert_eq!(plan.len(), 1);
    assert!(plan[0].connect_immediately_on_launch);

    let mut fresh_strip = TabStripModel::new();
    RestoredTabMerge::merge(&mut fresh_strip, restored_tabs.clone());
    assert_eq!(fresh_strip.tabs().len(), 2);

    Ok(())
}

#[tokio::test]
async fn windows_credential_store_and_storage_paths_integration() -> E2eResult<()> {
    let app_dir = format!("DataForge-E2E-{}", Uuid::new_v4());
    let paths = storage_paths_for(&app_dir)?;
    assert_ne!(paths.connections_file(), paths.filter_settings_file());

    let vault = WindowsCredentialStore::new(app_dir);
    let conn_id = Uuid::new_v4();

    let secret = SecretString::from("s3cr3t-e2e-password");
    vault
        .store(conn_id, SecretKind::DatabasePassword, &secret, "e2e-db")
        .await?;

    let loaded = vault.load(conn_id, SecretKind::DatabasePassword).await?;
    let SecretLookup::Found(loaded_secret) = loaded else {
        return Err(format!("expected SecretLookup::Found, got {loaded:?}").into());
    };
    assert_eq!(loaded_secret.expose_secret(), "s3cr3t-e2e-password");

    vault
        .delete_kind(conn_id, SecretKind::DatabasePassword)
        .await?;
    let after_remove = vault.load(conn_id, SecretKind::DatabasePassword).await?;
    assert!(
        matches!(after_remove, SecretLookup::NotStored),
        "secret should be removed after delete_kind"
    );

    Ok(())
}
