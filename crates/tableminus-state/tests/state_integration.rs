//! Integration tests exercising `tableminus-state` with `tablepro-session` and `tablepro-account` fakes.

use chrono::{TimeDelta, Utc};
use std::collections::BTreeMap;
use tableminus_state::connection_window::{
    ConnectionSession, ConnectionUnavailableView, ConnectionWindowPhase,
    ConnectionWindowPhaseMachine, DriverHandle, ReportedStatus, WorkspaceId,
};
use tableminus_state::data_grid::{
    DataGridLayoutScope, DataGridModel, GridSelectionAction, GridSelectionMode,
};
use tableminus_state::editor::{CursorPosition, Rope, UndoHistory};
use tableminus_state::tabs::{
    MoveQueryTabQueue, RestoredTabMerge, TabConnectionId, TabItem, TabKind, TabStripModel,
};
use tableminus_state::theme::{
    ThemeAppearance, ThemeDefinition, ThemeFileFormat, ThemeResolver, ThemeScope,
};
use tablepro_core::SafeModeLevel;
use tablepro_core::column::{ColumnType, ResultColumn};
use tablepro_core::value::Value;
use tablepro_session::fakes::{FakeConnector, FakeCredentialSource};

#[test]
fn account_fakes_entitlement_and_theme_roundtrip() -> Result<(), Box<dyn std::error::Error>> {
    let issued = Utc::now();
    let claims = tablepro_account::fakes::claims(issued, TimeDelta::days(30), &["sync", "themes"]);
    let entitlement = tablepro_account::fakes::entitlement(&claims)?;
    assert!(entitlement.claims().features.iter().any(|f| f == "themes"));

    let mut catalog = BTreeMap::new();
    let dark = ThemeDefinition::builtin_dark();
    let bytes = ThemeFileFormat::encode(&dark)?;
    let decoded = ThemeFileFormat::decode(&bytes)?;
    catalog.insert(decoded.id.clone(), decoded.clone());

    let resolved = ThemeResolver::resolve(
        Some(&decoded.id),
        ThemeAppearance::Dark,
        &ThemeScope::Live,
        &catalog,
    );
    assert!(!resolved.used_fallback);
    assert_eq!(resolved.effective_id, decoded.id);
    Ok(())
}

#[test]
fn session_fakes_connection_phase_tab_move_grid_and_editor_flow()
-> Result<(), Box<dyn std::error::Error>> {
    // Verify session fake constructors are wired and usable.
    let _connector = FakeConnector::new([]);
    let _creds = FakeCredentialSource::new();

    let mut machine = ConnectionWindowPhaseMachine::new();
    let w1 = WorkspaceId(1);
    let w2 = WorkspaceId(2);
    machine.add_workspace(w1);
    machine.add_workspace(w2);

    let t1 = machine.begin_connect(w1).ok_or("missing token w1")?;
    assert!(machine.complete_connect(w1, t1));

    let mut session = ConnectionSession::new(SafeModeLevel::ConfirmWrites);
    session.install_driver(DriverHandle {
        driver_id: "postgres".to_owned(),
        generation: 1,
    });
    assert_eq!(session.reported_status(), ReportedStatus::Connected);

    // Move a query tab from w1 to w2; waits until w2 is Connected and observed.
    let c1 = TabConnectionId("w1".to_owned());
    let c2 = TabConnectionId("w2".to_owned());
    let mut strip1 = TabStripModel::new();
    let mut strip2 = TabStripModel::new();
    let q_id = strip1.open_query_tab(c1, Some("Users Query"));
    let detached = strip1.detach_tab(q_id).ok_or("detach failed")?;

    let mut move_queue = MoveQueryTabQueue::default();
    move_queue.enqueue(detached, c2.clone());
    assert_eq!(
        move_queue.process_for_target(&c2, machine.phase(w2).ok_or("missing w2")?, &mut strip2),
        0
    );

    let t2 = machine.begin_connect(w2).ok_or("missing token w2")?;
    assert!(machine.complete_connect(w2, t2));
    RestoredTabMerge::merge(
        &mut strip2,
        vec![TabItem {
            id: tableminus_state::tabs::TabId(1),
            connection_id: c2.clone(),
            title: "Init".to_owned(),
            kind: TabKind::QueryEditor {
                sql: "SELECT 1".to_owned(),
            },
            pinned: false,
            has_unsaved_edits: false,
            has_active_filters: false,
            has_active_sort: false,
        }],
    );
    assert_eq!(
        move_queue.process_for_target(&c2, machine.phase(w2).ok_or("missing w2")?, &mut strip2),
        1
    );
    assert_eq!(strip2.tabs().len(), 2);

    // Verify queued move drops if target turns Unavailable.
    let mut drop_queue = MoveQueryTabQueue::default();
    let detached2 = strip2
        .detach_tab(strip2.tabs()[0].id)
        .ok_or("detach2 failed")?;
    let c3 = TabConnectionId("w3".to_owned());
    drop_queue.enqueue(detached2, c3.clone());
    let mut strip3 = TabStripModel::new();
    strip3.has_observed_tabs = true;
    let unavail = ConnectionWindowPhase::Unavailable(ConnectionUnavailableView {
        summary: "down".to_owned(),
        detail: None,
        can_retry: true,
        is_auth_failure: false,
    });
    assert_eq!(drop_queue.process_for_target(&c3, &unavail, &mut strip3), 0);
    assert!(drop_queue.is_empty());

    // DataGrid + Editor smoke check.
    let mut grid = DataGridModel::new(DataGridLayoutScope::Ephemeral, 50);
    grid.reload_rows(
        vec![ResultColumn::new("id", ColumnType::unknown())],
        vec![vec![Value::Int(42)]],
        Some(1),
    );
    grid.apply_selection(GridSelectionAction::SelectAll);
    assert_eq!(grid.selection.mode, GridSelectionMode::Rows);

    let mut rope = Rope::from_text("SELECT 1;");
    let mut cursors = vec![CursorPosition::caret(0)];
    let mut history = UndoHistory::new();
    history.apply_multi_caret_insert(&mut rope, &mut cursors, "-- ");
    assert_eq!(rope.text(), "-- SELECT 1;");
    assert!(history.undo(&mut rope, &mut cursors));
    assert_eq!(rope.text(), "SELECT 1;");
    Ok(())
}
