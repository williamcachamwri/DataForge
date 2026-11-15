//! Isolated state-transition tests for `tableminus-state` public APIs.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;
use std::time::Duration;

use tableminus_state::connection_window::{
    CloseDecision, ConnectionSession, ConnectionUnavailableView, ConnectionWindowPaneResolver,
    ConnectionWindowPhase, ConnectionWindowPhaseMachine, DriverHandle, EditorTabLabelResolver,
    ReportedStatus, SessionLiveness, WindowTitleResolver, WorkspaceId, WorkspacePaneKind,
};
use tableminus_state::data_grid::{
    DataGridCellEdit, DataGridChange, DataGridColumnSet, DataGridLayoutScope, DataGridModel,
    DataGridPane, DataGridRowOrder, FilterBarState, GridCoord, GridSelection, GridSelectionAction,
    GridSelectionMode, GridSelectionReducer, MAX_COLUMN_WIDTH, MAXIMUM_FROZEN_SHARE,
    MIN_COLUMN_WIDTH, PaneColumnSlot, RowID, SortDirection, SortKey, TableEdits, TableQueryPage,
};
use tableminus_state::editor::{
    AutocompleteContext, AutocompleteState, CursorPosition, EditorPaneId, EditorSplitNode,
    IncrementalLineHighlighter, QueryHistoryEntry, QueryHistoryModel, Rope, RunEventAccumulator,
    SchemaCompletionItem, SchemaCompletionKind, SplitAxis, SplitPaneLayoutPolicy, UndoHistory,
    VimAction, VimMode, VimMotion, VimOperator, VimState, normalize_line_endings,
};
use tableminus_state::library::{
    ConnectionsLibraryModel, LibraryConnection, LibraryConnectionId, LibraryGroup, LibraryGroupId,
    LibraryRowID, LibrarySection, LibraryStorageSnapshot,
};
use tableminus_state::tabs::{
    CloseActionResult, CrashSafeTabPersistence, MoveQueryTabQueue, RestoredTabMerge,
    SaveTabsOutcome, SavedTabsStore, SessionRestorePlanner, TabConnectionId, TabId, TabItem,
    TabKind, TabStripModel,
};
use tablepro_core::column::{ColumnType, ResultColumn};
use tablepro_core::export::CsvOptions;
use tablepro_core::filter::{FilterOp, FilterRule, FilterValue};
use tablepro_core::sql_syntax::{SqlGrammar, StatementAccess, SyntaxKind};
use tablepro_core::value::Value;
use tablepro_core::{
    ResultEvent, RunEnd, RunSummary, SafeModeLevel, ServerNotice, TransactionState, WriteGate,
};

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn make_tab(id: u64, conn: &str, title: &str, kind: TabKind, pinned: bool) -> TabItem {
    TabItem {
        id: TabId(id),
        connection_id: TabConnectionId(conn.to_owned()),
        title: title.to_owned(),
        kind,
        pinned,
        has_unsaved_edits: false,
        has_active_filters: false,
        has_active_sort: false,
    }
}

fn cycle_next(strip: &mut TabStripModel) -> Option<TabId> {
    let tabs = strip.tabs();
    if tabs.is_empty() {
        return None;
    }
    let next_idx = match strip
        .active_tab()
        .and_then(|active| tabs.iter().position(|t| t.id == active))
    {
        Some(idx) => (idx + 1) % tabs.len(),
        None => 0,
    };
    let next_id = tabs[next_idx].id;
    strip.select(next_id);
    Some(next_id)
}

fn cycle_prev(strip: &mut TabStripModel) -> Option<TabId> {
    let tabs = strip.tabs();
    if tabs.is_empty() {
        return None;
    }
    let prev_idx = match strip
        .active_tab()
        .and_then(|active| tabs.iter().position(|t| t.id == active))
    {
        Some(0) | None => tabs.len() - 1,
        Some(idx) => idx - 1,
    };
    let prev_id = tabs[prev_idx].id;
    strip.select(prev_id);
    Some(prev_id)
}

fn sample_columns() -> Vec<ResultColumn> {
    vec![
        ResultColumn::new("id", ColumnType::unknown()),
        ResultColumn::new("name", ColumnType::unknown()),
        ResultColumn::new("score", ColumnType::unknown()),
    ]
}

fn sample_library_connection(
    id: &str,
    name: &str,
    group: Option<&str>,
    is_favorite: bool,
    order: usize,
) -> LibraryConnection {
    LibraryConnection {
        id: LibraryConnectionId(id.to_owned()),
        name: name.to_owned(),
        driver_id: "postgres".to_owned(),
        host: "db.local".to_owned(),
        port: Some(5432),
        database: "analytics".to_owned(),
        username: "app".to_owned(),
        group_id: group.map(|g| LibraryGroupId(g.to_owned())),
        tags: BTreeSet::from(["prod".to_owned()]),
        is_favorite,
        order,
    }
}

#[test]
fn tab_rapid_open_close_and_lifo_reopen_with_index_clamping() {
    let mut strip = TabStripModel::new();
    let conn = TabConnectionId("pg-main".to_owned());

    let t1 = strip.open_query_tab(conn.clone(), Some("Q1"));
    let t2 = strip.open_query_tab(conn.clone(), Some("Q2"));
    let t3 = strip.open_query_tab(conn.clone(), Some("Q3"));
    let t4 = strip.open_table_tab(conn, "orders");
    assert_eq!(strip.tabs().len(), 4);
    assert_eq!(strip.active_tab(), Some(t4));

    // Close t4 (index 3), then t2 (index 1), then t3 (index 1 after t2 removal).
    assert_eq!(strip.perform_close(), CloseActionResult::ClosedTab(t4));
    assert!(strip.close_tab_by_user(t2));
    assert!(strip.close_tab_by_user(t3));
    assert_eq!(strip.tabs().len(), 1);
    assert_eq!(strip.active_tab(), Some(t1));

    // Reopen in strict LIFO order: t3 -> t2 -> t4, clamping original_index if needed.
    assert_eq!(strip.reopen_closed_tab(), Some(t3));
    assert_eq!(strip.active_tab(), Some(t3));
    assert_eq!(strip.reopen_closed_tab(), Some(t2));
    assert_eq!(strip.active_tab(), Some(t2));
    assert_eq!(strip.reopen_closed_tab(), Some(t4));
    assert_eq!(strip.active_tab(), Some(t4));
    assert_eq!(strip.reopen_closed_tab(), None);

    let ids: Vec<TabId> = strip.tabs().iter().map(|t| t.id).collect();
    assert_eq!(ids, vec![t1, t2, t3, t4]);
}

#[test]
fn tab_active_vs_inactive_close_and_perform_close_window_fallback() {
    let mut strip = TabStripModel::new();
    let conn = TabConnectionId("pg-main".to_owned());
    assert_eq!(strip.perform_close(), CloseActionResult::CloseWindow);

    let t1 = strip.open_query_tab(conn.clone(), Some("One"));
    let t2 = strip.open_query_tab(conn.clone(), Some("Two"));
    let t3 = strip.open_query_tab(conn, Some("Three"));
    assert!(strip.select(t2));

    // Closing an inactive tab keeps t2 active.
    assert!(strip.close_tab_by_user(t1));
    assert_eq!(strip.active_tab(), Some(t2));
    assert!(!strip.close_tab_by_user(TabId(999)));

    // Closing active first tab promotes new index 0 (t3).
    assert!(strip.close_tab_by_user(t2));
    assert_eq!(strip.active_tab(), Some(t3));

    // Reopen t2 at index 0, select last tab t3, close t3 -> clamps to t2.
    assert_eq!(strip.reopen_closed_tab(), Some(t2));
    assert!(strip.select(t3));
    assert_eq!(strip.perform_close(), CloseActionResult::ClosedTab(t3));
    assert_eq!(strip.active_tab(), Some(t2));

    assert_eq!(strip.perform_close(), CloseActionResult::ClosedTab(t2));
    assert_eq!(strip.active_tab(), None);
    assert_eq!(strip.perform_close(), CloseActionResult::CloseWindow);
}

#[test]
fn tab_boundary_cycling_and_rapid_keyboard_selection_sequences() {
    let mut strip = TabStripModel::new();
    assert_eq!(cycle_next(&mut strip), None);
    assert_eq!(cycle_prev(&mut strip), None);

    let conn = TabConnectionId("sqlite-1".to_owned());
    let t1 = strip.open_query_tab(conn.clone(), None);
    let t2 = strip.open_query_tab(conn.clone(), Some("  "));
    let t3 = strip.open_query_tab(conn, Some("Final"));
    assert_eq!(strip.tabs()[0].title, "Query 1");
    assert_eq!(strip.tabs()[1].title, "Query 2");
    assert_eq!(strip.active_tab(), Some(t3));

    // Wrap forward at end: t3 -> t1 -> t2 -> t3.
    assert_eq!(cycle_next(&mut strip), Some(t1));
    assert_eq!(cycle_next(&mut strip), Some(t2));
    assert_eq!(cycle_next(&mut strip), Some(t3));

    // Wrap backward at start: t3 -> t2 -> t1 -> t3.
    assert_eq!(cycle_prev(&mut strip), Some(t2));
    assert_eq!(cycle_prev(&mut strip), Some(t1));
    assert_eq!(cycle_prev(&mut strip), Some(t3));

    // Selecting unknown tab fails without altering active_tab.
    assert!(!strip.select(TabId(404)));
    assert_eq!(strip.active_tab(), Some(t3));

    // Drag reorder preserves active tab identity and validates bounds.
    assert!(!strip.drag_reorder(0, 5));
    assert!(strip.drag_reorder(1, 1));
    assert!(strip.drag_reorder(2, 0));
    assert_eq!(strip.tabs()[0].id, t3);
    assert_eq!(strip.active_tab(), Some(t3));
}

#[test]
fn tab_replacement_guards_for_dirty_filtered_sorted_and_pinned_tabs() {
    let conn = TabConnectionId("pg-1".to_owned());
    let mut clean = make_tab(
        1,
        "pg-1",
        "users",
        TabKind::TableData {
            table_name: "users".to_owned(),
        },
        false,
    );
    assert!(!clean.protects_against_replacement());

    clean.has_unsaved_edits = true;
    assert!(clean.protects_against_replacement());
    clean.has_unsaved_edits = false;
    clean.has_active_filters = true;
    assert!(clean.protects_against_replacement());
    clean.has_active_filters = false;
    clean.has_active_sort = true;
    assert!(clean.protects_against_replacement());

    let mut strip = TabStripModel::new();
    // QueryEditor active tab is never replaced by open_table_tab.
    let q1 = strip.open_query_tab(conn.clone(), Some("Scratch"));
    let tbl1 = strip.open_table_tab(conn.clone(), "users");
    assert_ne!(q1, tbl1);

    // Clean active TableData tab is replaced in place.
    let tbl1_replaced = strip.open_table_tab(conn.clone(), "invoices");
    assert_eq!(tbl1_replaced, tbl1);
    assert_eq!(strip.tabs().len(), 2);

    // Pinned TableData tab is never replaced in place.
    let pinned_table = make_tab(
        99,
        "pg-1",
        "audit_log",
        TabKind::TableData {
            table_name: "audit_log".to_owned(),
        },
        true,
    );
    let pinned_id = strip.attach_tab(pinned_table, conn.clone());
    let tbl2 = strip.open_table_tab(conn.clone(), "events");
    assert_ne!(tbl2, pinned_id);
    assert_eq!(strip.tabs().len(), 4);

    // Filtered TableData tab attached via RestoredTabMerge is also protected.
    let mut filtered_table = make_tab(
        100,
        "pg-1",
        "orders",
        TabKind::TableData {
            table_name: "orders".to_owned(),
        },
        false,
    );
    filtered_table.has_active_filters = true;
    let filtered_id = strip.attach_tab(filtered_table, conn.clone());
    let tbl3 = strip.open_table_tab(conn, "payments");
    assert_ne!(tbl3, filtered_id);
    assert_eq!(strip.tabs().len(), 6);
}

#[test]
fn tab_detach_attach_restore_merge_and_saved_tabs_guards() {
    let conn1 = TabConnectionId("c1".to_owned());
    let conn2 = TabConnectionId("c2".to_owned());
    let mut strip1 = TabStripModel::new();
    let t1 = strip1.open_query_tab(conn1.clone(), Some("A"));
    let t2 = strip1.open_query_tab(conn1.clone(), Some("B"));
    assert_eq!(strip1.active_tab(), Some(t2));

    // Detaching inactive tab keeps t2 active; detaching active picks first remaining.
    let detached1 = strip1.detach_tab(t1);
    assert!(detached1.is_some());
    assert_eq!(strip1.active_tab(), Some(t2));
    let detached2 = strip1.detach_tab(t2);
    assert!(detached2.is_some());
    assert_eq!(strip1.active_tab(), None);
    assert!(strip1.detach_tab(TabId(999)).is_none());

    let mut strip2 = TabStripModel::new();
    let mut store = SavedTabsStore::new();
    assert_eq!(
        store.clear_by_user(&conn2, &strip2),
        SaveTabsOutcome::SkippedUnobserved
    );

    if let Some(tab) = detached1 {
        strip2.attach_tab(tab, conn2.clone());
    }
    let pre_active = strip2.active_tab();
    RestoredTabMerge::merge(
        &mut strip2,
        vec![make_tab(
            50,
            "c2",
            "Restored",
            TabKind::Structure {
                table_name: "users".to_owned(),
            },
            false,
        )],
    );
    assert!(strip2.has_observed_tabs);
    // Existing active tab is preserved when already set before merge.
    assert_eq!(strip2.active_tab(), pre_active);
    assert_eq!(strip2.tabs()[0].title, "Restored");
    assert_eq!(
        store.save_aggregated_sync(conn2.clone(), &strip2, false),
        SaveTabsOutcome::Saved
    );

    // User closing all tabs allows clear_by_user to purge saved entry.
    while strip2.perform_close() != CloseActionResult::CloseWindow {}
    assert_eq!(store.clear_by_user(&conn2, &strip2), SaveTabsOutcome::Saved);
    assert!(store.get(&conn2).is_none());
}

#[test]
fn tab_move_queue_session_restore_and_crash_safe_recovery() -> TestResult {
    let c1 = TabConnectionId("c1".to_owned());
    let c2 = TabConnectionId("c2".to_owned());
    let mut queue = MoveQueryTabQueue::default();
    assert!(queue.is_empty());

    queue.enqueue(
        make_tab(
            1,
            "c1",
            "Q1",
            TabKind::QueryEditor {
                sql: "SELECT 1".to_owned(),
            },
            false,
        ),
        c1.clone(),
    );
    queue.enqueue(
        make_tab(
            2,
            "c1",
            "Q2",
            TabKind::QueryEditor {
                sql: "SELECT 2".to_owned(),
            },
            false,
        ),
        c2.clone(),
    );
    assert_eq!(queue.len(), 2);

    // Unobserved Connected target does not drain queue.
    let mut strip1 = TabStripModel::new();
    assert_eq!(
        queue.process_for_target(&c1, &ConnectionWindowPhase::Connected, &mut strip1),
        0
    );
    strip1.has_observed_tabs = true;
    assert_eq!(
        queue.process_for_target(&c1, &ConnectionWindowPhase::Connected, &mut strip1),
        1
    );
    assert_eq!(queue.len(), 1);
    assert_eq!(strip1.tabs().len(), 1);

    // Unavailable target drops queued moves for that connection.
    let mut strip2 = TabStripModel::new();
    strip2.has_observed_tabs = true;
    let unavail = ConnectionWindowPhase::Unavailable(ConnectionUnavailableView {
        summary: "refused".to_owned(),
        detail: None,
        can_retry: true,
        is_auth_failure: false,
    });
    assert_eq!(queue.process_for_target(&c2, &unavail, &mut strip2), 0);
    assert!(queue.is_empty());

    // Session restore planner dials only retentive phases and launches only frontmost.
    let plan = SessionRestorePlanner::plan(
        &[
            (c1.clone(), ConnectionWindowPhase::Connected),
            (c2, ConnectionWindowPhase::Disconnected),
        ],
        0,
    );
    assert!(plan[0].should_dial && plan[0].connect_immediately_on_launch);
    assert!(!plan[1].should_dial && !plan[1].connect_immediately_on_launch);

    // Crash-safe persistence recovers from backup when primary checksum fails.
    let dir = tempfile::tempdir()?;
    let primary = dir.path().join("session.json");
    let backup = dir.path().join("session.bak.json");
    let mut strips = BTreeMap::new();
    strips.insert(c1.clone(), strip1.tabs().to_vec());
    CrashSafeTabPersistence::save_atomic(&primary, &backup, &strips)?;
    CrashSafeTabPersistence::save_atomic(&primary, &backup, &strips)?;

    // Tamper checksum in valid JSON to exercise checksum verification error path.
    let raw = std::fs::read_to_string(&primary)?;
    let tampered = raw.replacen("\"checksum\": ", "\"checksum\": 1", 1);
    std::fs::write(&primary, tampered)?;
    let recovered = CrashSafeTabPersistence::load_with_recovery(&primary, &backup)?;
    assert_eq!(recovered.get(&c1).map(Vec::len), Some(1));

    std::fs::write(&primary, b"bad")?;
    std::fs::write(&backup, b"bad")?;
    assert!(CrashSafeTabPersistence::load_with_recovery(&primary, &backup).is_err());
    Ok(())
}

#[test]
fn grid_table_edits_stage_update_insert_delete_and_commit_vs_rollback() {
    let mut edits = TableEdits::new();
    assert!(!edits.has_pending_edits());

    let col1 = tableminus_state::data_grid::DataGridColumnID(1);
    let col2 = tableminus_state::data_grid::DataGridColumnID(2);

    // Empty batch is a no-op.
    edits.commit_cells(Vec::new(), "Empty");
    assert!(!edits.has_pending_edits());

    // Stage cell updates on existing row.
    edits.commit_cells(
        vec![
            DataGridCellEdit {
                row_id: RowID(1),
                column_id: col1,
                old_value: Value::Int(1),
                new_value: Value::Int(10),
            },
            DataGridCellEdit {
                row_id: RowID(1),
                column_id: col2,
                old_value: Value::Text("old".to_owned()),
                new_value: Value::Text("new".to_owned()),
            },
        ],
        "Update row 1",
    );
    assert!(edits.has_pending_edits());
    assert_eq!(edits.modified_cells.len(), 2);

    // Revert single cell edit directly from modified_cells.
    assert_eq!(
        edits.modified_cells.remove(&(RowID(1), col1)),
        Some(Value::Int(10))
    );
    assert_eq!(edits.modified_cells.len(), 1);

    // Insert temp row, then delete it -> removes from inserted_rows, NOT deleted_rows.
    let temp_id1 = edits.insert_row(vec![Value::Int(99), Value::Text("tmp".to_owned())]);
    let temp_id2 = edits.insert_row(vec![Value::Int(100), Value::Text("keep".to_owned())]);
    assert_ne!(temp_id1, temp_id2);
    edits.delete_row(temp_id1);
    assert!(!edits.inserted_rows.contains_key(&temp_id1));
    assert!(!edits.deleted_rows.contains(&temp_id1));
    assert!(edits.inserted_rows.contains_key(&temp_id2));

    // Delete persisted row -> recorded in deleted_rows.
    edits.delete_row(RowID(2));
    assert!(edits.deleted_rows.contains(&RowID(2)));

    // take_committed drains pending edits; rollback clears cloned copy.
    let mut cloned = edits.clone();
    cloned.rollback();
    assert!(!cloned.has_pending_edits());

    let committed = edits.take_committed();
    assert!(!edits.has_pending_edits());
    assert_eq!(committed.modified_cells.len(), 1);
    assert_eq!(committed.inserted_rows.len(), 1);
    assert_eq!(committed.deleted_rows.len(), 1);
}

#[test]
fn grid_model_commit_delta_and_reload_column_preservation() -> TestResult {
    let mut model = DataGridModel::new(DataGridLayoutScope::Table("users".to_owned()), 25);
    model.reload_rows(
        sample_columns(),
        vec![
            vec![
                Value::Int(1),
                Value::Text("Alice".to_owned()),
                Value::Int(10),
            ],
            vec![Value::Int(2), Value::Text("Bob".to_owned()), Value::Int(20)],
        ],
        Some(2),
    );
    assert_eq!(model.take_changes(), vec![DataGridChange::ReloadAll]);

    // Customize width; reloading same column names preserves the customized width.
    let id_col = model.columns.columns()[0].id;
    let name_col = model.columns.columns()[1].id;
    assert!(model.columns.resize_column(id_col, 240.0));
    model.reload_rows(
        sample_columns(),
        vec![vec![
            Value::Int(3),
            Value::Text("Cara".to_owned()),
            Value::Int(30),
        ]],
        Some(1),
    );
    assert!((model.columns.columns()[0].width - 240.0).abs() < f64::EPSILON);

    let r0 = model.row_order.row_id(0).ok_or("missing row 0")?;
    model.commit(
        vec![DataGridCellEdit {
            row_id: r0,
            column_id: name_col,
            old_value: Value::Text("Cara".to_owned()),
            new_value: Value::Text("Caroline".to_owned()),
        }],
        "Rename",
    );
    assert_eq!(
        model.take_changes(),
        vec![
            DataGridChange::ReloadAll,
            DataGridChange::RowsUpdated(vec![r0]),
        ]
    );
    let csv = model.export_csv(&CsvOptions::default())?;
    assert!(csv.contains("Caroline"));

    // Reloading with changed column schema resets column widths to default (140.0).
    model.reload_rows(
        vec![ResultColumn::new("different", ColumnType::unknown())],
        vec![vec![Value::Int(7)]],
        Some(1),
    );
    assert_eq!(model.columns.columns().len(), 1);
    assert!((model.columns.columns()[0].width - 140.0).abs() < f64::EPSILON);
    Ok(())
}

#[test]
fn grid_selection_reducer_rapid_cursor_range_row_and_clear_transitions() {
    let cols =
        DataGridColumnSet::from_names(&["a".to_owned(), "b".to_owned(), "c".to_owned()], 100.0);
    let rows = DataGridRowOrder::new(vec![RowID(10), RowID(20), RowID(30), RowID(40)]);
    let c0 = cols.columns()[0].id;
    let c1 = cols.columns()[1].id;
    let c2 = cols.columns()[2].id;

    // Click bottom-right then reverse DragTo top-left.
    let s1 = GridSelectionReducer::reduce(
        &GridSelection::default(),
        GridSelectionAction::Click(GridCoord {
            row: RowID(40),
            col: c2,
        }),
        &rows,
        &cols,
    );
    assert_eq!(s1.mode, GridSelectionMode::Cursor);

    let s2 = GridSelectionReducer::reduce(
        &s1,
        GridSelectionAction::DragTo(GridCoord {
            row: RowID(20),
            col: c0,
        }),
        &rows,
        &cols,
    );
    assert_eq!(s2.mode, GridSelectionMode::Cells);
    assert_eq!(
        s2.selected_rows,
        BTreeSet::from([RowID(20), RowID(30), RowID(40)])
    );
    assert_eq!(s2.selected_cols, BTreeSet::from([c0, c1, c2]));

    // ShiftClick with non-existent row resets to default.
    let bad = GridSelectionReducer::reduce(
        &s2,
        GridSelectionAction::ShiftClick(GridCoord {
            row: RowID(999),
            col: c1,
        }),
        &rows,
        &cols,
    );
    assert_eq!(bad, GridSelection::default());

    // SelectRow selects full row width in Rows mode.
    let row_sel = GridSelectionReducer::reduce(
        &bad,
        GridSelectionAction::SelectRow(RowID(20)),
        &rows,
        &cols,
    );
    assert_eq!(row_sel.mode, GridSelectionMode::Rows);
    assert_eq!(row_sel.selected_rows, BTreeSet::from([RowID(20)]));
    assert_eq!(row_sel.selected_cols.len(), 3);

    // RowsSelectedNatively switches Cursor vs Rows depending on count.
    let native_single = GridSelectionReducer::reduce(
        &row_sel,
        GridSelectionAction::RowsSelectedNatively(vec![RowID(10)]),
        &rows,
        &cols,
    );
    assert_eq!(native_single.mode, GridSelectionMode::Cursor);

    let native_multi = GridSelectionReducer::reduce(
        &native_single,
        GridSelectionAction::RowsSelectedNatively(vec![RowID(10), RowID(30)]),
        &rows,
        &cols,
    );
    assert_eq!(native_multi.mode, GridSelectionMode::Rows);

    let cleared =
        GridSelectionReducer::reduce(&native_multi, GridSelectionAction::Clear, &rows, &cols);
    assert_eq!(cleared, GridSelection::default());
}

#[test]
fn grid_paging_and_column_freezing_boundaries() {
    // Limit 0 clamps to 1; prev_page at 0 fails; next_page stops at total_row_count.
    let mut page = TableQueryPage::new(0, 0, Some(3));
    assert_eq!(page.limit, 1);
    assert!(!page.prev_page());
    assert_eq!(page.page_info(), (1, Some(3)));
    assert!(page.next_page());
    assert!(page.next_page());
    assert_eq!(page.page_info(), (3, Some(3)));
    assert!(!page.next_page());
    assert!(page.prev_page());
    assert_eq!(page.offset, 1);

    let mut unbounded = TableQueryPage::new(0, 10, None);
    assert_eq!(unbounded.page_info(), (1, None));
    assert!(unbounded.next_page());
    assert_eq!(unbounded.offset, 10);

    // Column widths clamp to [MIN_COLUMN_WIDTH, MAX_COLUMN_WIDTH].
    let mut cols =
        DataGridColumnSet::from_names(&["a".to_owned(), "b".to_owned(), "c".to_owned()], 10.0);
    assert!((cols.columns()[0].width - MIN_COLUMN_WIDTH).abs() < f64::EPSILON);
    let id0 = cols.columns()[0].id;
    let id1 = cols.columns()[1].id;
    assert!(cols.resize_column(id0, 5000.0));
    assert!((cols.columns()[0].width - MAX_COLUMN_WIDTH).abs() < f64::EPSILON);
    assert!(cols.resize_column(id0, 100.0));
    assert!(cols.resize_column(id1, 100.0));

    // Viewport 300 -> max frozen 200 -> freezing 2 cols succeeds, 3 cols fails.
    assert!(cols.freeze_up_to(1, 300.0));
    assert_eq!(cols.frozen_count(), 2);
    assert!(cols.frozen_width() <= 300.0 * MAXIMUM_FROZEN_SHARE + 1e-6);
    assert!(!cols.freeze_up_to(2, 300.0));
    assert_eq!(cols.table_index(DataGridPane::Frozen, 1), Some(2));
    assert_eq!(cols.table_index(DataGridPane::Frozen, 2), None);
    assert_eq!(
        cols.slot(DataGridPane::Main, 0),
        Some(PaneColumnSlot::RowNumbers)
    );
    assert!(!cols.move_within_side(1, 2));
    assert!(cols.move_within_side(0, 1));
    assert_eq!(cols.display_position(id0), Some(1));
    cols.unfreeze_all();
    assert_eq!(cols.frozen_count(), 0);
}

#[test]
fn grid_sorting_and_filter_bar_transitions() {
    let mut model = DataGridModel::new(DataGridLayoutScope::Ephemeral, 50);
    model.reload_rows(
        sample_columns(),
        vec![
            vec![Value::Int(1), Value::Text("B".to_owned()), Value::Int(10)],
            vec![Value::Int(2), Value::Text("A".to_owned()), Value::Null],
            vec![Value::Int(3), Value::Text("A".to_owned()), Value::Int(5)],
        ],
        Some(3),
    );
    let id_col = model.columns.columns()[0].id;
    let name_col = model.columns.columns()[1].id;
    let score_col = model.columns.columns()[2].id;

    // Multi-key sort: name ASC, then score ASC (Null sorts last per compare_opt_values).
    model.sort.click(name_col, false);
    model.sort.click(score_col, true);
    assert_eq!(
        model.sort.keys,
        vec![
            SortKey {
                column_id: name_col,
                direction: SortDirection::Ascending,
            },
            SortKey {
                column_id: score_col,
                direction: SortDirection::Ascending,
            },
        ]
    );
    model.sort_client_rows();
    // Expect RowID(3) [A, 5], RowID(2) [A, Null], RowID(1) [B, 10].
    assert_eq!(model.row_order.as_slice(), &[RowID(3), RowID(2), RowID(1)]);

    // Option-click existing key flips only that key; plain click replaces all keys.
    model.sort.click(score_col, true);
    assert_eq!(model.sort.keys[1].direction, SortDirection::Descending);
    model.sort.click(id_col, false);
    assert_eq!(model.sort.keys.len(), 1);
    model.sort.clear();
    assert!(model.sort.keys.is_empty());

    // Filter bar add/remove/clear transitions.
    let mut filter = FilterBarState::default();
    assert!(!filter.is_visible);
    assert!(!filter.has_active_filters());
    filter.add_rule(FilterRule {
        column: "name".to_owned(),
        op: FilterOp::Eq,
        value: Some(FilterValue::Single("A".to_owned())),
    });
    assert!(filter.is_visible);
    assert!(filter.has_active_filters());
    assert!(!filter.remove_rule(5));
    assert!(filter.remove_rule(0));
    assert!(!filter.has_active_filters());
    filter.add_rule(FilterRule {
        column: "score".to_owned(),
        op: FilterOp::IsNotNull,
        value: None,
    });
    filter.clear();
    assert!(!filter.has_active_filters());
}

#[test]
fn editor_rope_multi_leaf_utf16_and_slice_invariants() {
    assert_eq!(normalize_line_endings("a\r\nb\rc\n"), "a\nb\nc\n");

    // Exceed MAX_LEAF_CHARS (2048) to force multi-leaf balanced Rope.
    let body = "x".repeat(2500);
    let mut rope = Rope::from_text(&format!("head\r\n{body}\r🚀tail"));
    assert!(rope.height() >= 2);
    assert_eq!(rope.line_count(), 3);
    assert_eq!(rope.line(0).as_deref(), Some("head"));
    assert_eq!(rope.line(2).as_deref(), Some("🚀tail"));
    assert_eq!(rope.line(3), None);

    // UTF-16 conversions around surrogate pair '🚀' (1 char, 2 UTF-16 units).
    let rocket_pos = 5 + 2500 + 1;
    let utf16_before = rope.char_to_utf16(rocket_pos);
    let utf16_after = rope.char_to_utf16(rocket_pos + 1);
    assert_eq!(utf16_after - utf16_before, 2);
    assert_eq!(rope.utf16_to_char(utf16_after), rocket_pos + 1);

    // Clamped out-of-bounds insert/delete/slice never panic.
    rope.insert(usize::MAX, "!");
    assert_eq!(rope.slice(rope.len_chars() - 5, usize::MAX), "tail!");
    rope.delete(rope.len_chars() - 1, usize::MAX);
    assert_eq!(rope.slice(10, 5), "");
    rope.delete(10, 5);
    assert_eq!(rope.line(2).as_deref(), Some("🚀tail"));
}

#[test]
fn editor_undo_history_rapid_multi_caret_edits_and_redo_invariants() {
    let mut rope = Rope::from_text("alpha beta gamma");
    let mut history = UndoHistory::new();
    let mut empty_cursors = vec![CursorPosition::caret(0)];
    assert!(!history.undo(&mut rope, &mut empty_cursors));
    assert!(!history.redo(&mut rope, &mut empty_cursors));

    // Unsorted cursors with reversed selection (anchor > head) + forward selection.
    let mut cursors = vec![
        CursorPosition {
            anchor: 16,
            head: 11,
        },
        CursorPosition { anchor: 0, head: 5 },
    ];
    history.apply_multi_caret_insert(&mut rope, &mut cursors, "X");
    assert_eq!(rope.text(), "X beta X");
    assert_eq!(
        cursors,
        vec![CursorPosition::caret(8), CursorPosition::caret(1)]
    );
    assert!(history.can_undo());
    assert!(!history.can_redo());

    // Second edit at current carets.
    history.apply_multi_caret_insert(&mut rope, &mut cursors, "!");
    assert_eq!(rope.text(), "X! beta X!");

    // Undo one step, then apply new edit -> clears redo stack.
    assert!(history.undo(&mut rope, &mut cursors));
    assert_eq!(rope.text(), "X beta X");
    assert!(history.can_redo());

    history.apply_multi_caret_insert(&mut rope, &mut cursors, "?");
    assert!(!history.can_redo());
    assert_eq!(rope.text(), "X? beta X?");

    // Undo all the way back to original text & selections, then redo all.
    assert!(history.undo(&mut rope, &mut cursors));
    assert!(history.undo(&mut rope, &mut cursors));
    assert!(!history.can_undo());
    assert_eq!(rope.text(), "alpha beta gamma");
    assert_eq!(
        cursors,
        vec![
            CursorPosition {
                anchor: 16,
                head: 11,
            },
            CursorPosition { anchor: 0, head: 5 },
        ]
    );

    assert!(history.redo(&mut rope, &mut cursors));
    assert!(history.redo(&mut rope, &mut cursors));
    assert_eq!(rope.text(), "X? beta X?");
    assert!(!history.can_redo());
}

#[test]
fn editor_highlighter_and_autocomplete_state_transitions() {
    let rope = Rope::from_text("SELECT id FROM users\nWHERE id = 1");
    let mut hl = IncrementalLineHighlighter::new(SqlGrammar::PostgreSql);
    let line0 = hl.highlight_line(&rope, 0);
    assert!(line0.iter().any(|s| s.kind == SyntaxKind::Keyword));
    hl.invalidate_lines(0, 0);
    assert_eq!(hl.highlight_line(&rope, 0), line0);
    assert!(hl.highlight_line(&rope, 99).is_empty());

    // Autocomplete context prioritization & wraparound navigation.
    let mut ac = AutocompleteState::with_catalog(vec![
        SchemaCompletionItem {
            label: "users".to_owned(),
            kind: SchemaCompletionKind::Table,
            detail: None,
            insert_text: "users".to_owned(),
        },
        SchemaCompletionItem {
            label: "user_id".to_owned(),
            kind: SchemaCompletionKind::Column,
            detail: None,
            insert_text: "user_id".to_owned(),
        },
    ]);
    ac.update_prefix_with_context("", AutocompleteContext::ColumnExpecting);
    assert!(ac.is_open);
    assert_eq!(ac.items().len(), 1);
    assert_eq!(ac.items()[0].label, "user_id");

    ac.update_prefix("us");
    assert_eq!(ac.items().len(), 2);
    ac.move_prev();
    assert_eq!(ac.selected_index, 1);
    ac.move_next();
    assert_eq!(ac.selected_index, 0);
    assert_eq!(ac.accept().map(|i| i.label), Some("users".to_owned()));
    assert!(!ac.is_open);
    ac.update_prefix("zzz_none");
    assert!(!ac.is_open);
}

#[test]
fn editor_vim_splits_history_and_run_accumulator_transitions() {
    // Vim modal transitions & operators.
    let mut disabled = VimState::new(false);
    assert_eq!(disabled.handle_key("i"), VimAction::None);

    let mut vim = VimState::new(true);
    assert_eq!(vim.handle_key("v"), VimAction::None);
    assert_eq!(vim.mode, VimMode::Visual);
    assert_eq!(vim.handle_key("v"), VimAction::None);
    assert_eq!(vim.mode, VimMode::Normal);
    assert_eq!(vim.handle_key("3"), VimAction::None);
    assert_eq!(vim.handle_key("d"), VimAction::None);
    assert_eq!(
        vim.handle_key("w"),
        VimAction::ApplyOperator(VimOperator::Delete, VimMotion::WordForward, 3)
    );
    assert_eq!(vim.handle_key("c"), VimAction::None);
    assert_eq!(
        vim.handle_key("c"),
        VimAction::LineOperator(VimOperator::Change, 1)
    );
    assert_eq!(vim.mode, VimMode::Insert);
    vim.handle_key("Esc");
    assert_eq!(vim.mode, VimMode::Normal);
    vim.handle_key(":");
    vim.handle_key("q");
    vim.handle_key("Backspace");
    assert_eq!(vim.mode, VimMode::Normal);
    vim.handle_key("g");
    assert_eq!(vim.handle_key("g"), VimAction::Move(VimMotion::DocStart, 1));

    // Split tree & bounded QueryHistoryModel & RunEventAccumulator.
    let mut split = EditorSplitNode::single(EditorPaneId(1));
    assert!(!split.split_pane(EditorPaneId(99), EditorPaneId(2), SplitAxis::Vertical));
    assert!(split.split_pane(EditorPaneId(1), EditorPaneId(2), SplitAxis::Vertical));
    assert_eq!(split.leaf_ids(), vec![EditorPaneId(1), EditorPaneId(2)]);
    let policy = SplitPaneLayoutPolicy::default();
    assert!(policy.holding_priority < 490);

    let mut qh = QueryHistoryModel::new(2);
    for idx in 0..3 {
        qh.record(QueryHistoryEntry {
            sql: format!("SELECT {idx}"),
            connection_id: "c1".to_owned(),
            elapsed_ms: idx,
            succeeded: true,
        });
    }
    assert_eq!(qh.entries().len(), 2);
    assert_eq!(qh.entries()[0].sql, "SELECT 2");
    assert_eq!(qh.search("select 1").len(), 1);

    let mut acc = RunEventAccumulator::new();
    acc.push_event(ResultEvent::ResultSetStarted {
        columns: Arc::from([ResultColumn::new("id", ColumnType::unknown())]),
    });
    acc.push_event(ResultEvent::RowLimitReached { retained: 10 });
    acc.push_event(ResultEvent::ByteLimitReached { retained: 10 });
    acc.push_event(ResultEvent::CommandCompleted {
        rows_affected: Some(3),
    });
    acc.push_event(ResultEvent::Notice(ServerNotice {
        severity: "INFO".to_owned(),
        message: "ok".to_owned(),
        code: None,
    }));
    acc.push_event(ResultEvent::NoticeLimitReached { retained: 1 });
    acc.finish(RunSummary {
        elapsed: Duration::from_millis(5),
        end: RunEnd::Completed,
        transaction_state: TransactionState::Idle,
    });
    assert!(acc.result_sets[0].row_limit_reached);
    assert!(acc.result_sets[0].byte_limit_reached);
    assert_eq!(acc.commands_completed, vec![Some(3)]);
    assert_eq!(acc.notice_limit_reached, Some(1));
}

#[test]
fn connection_window_phase_lifecycle_and_token_fencing() -> TestResult {
    let mut machine = ConnectionWindowPhaseMachine::new();
    let ws = WorkspaceId(1);
    machine.add_workspace(ws);
    assert_eq!(
        machine.phase(ws),
        Some(&ConnectionWindowPhase::Disconnected)
    );
    assert_eq!(machine.pane_rebuild_count(ws), 1);

    // Redundant disconnect does not rebuild panes.
    machine.disconnect(ws);
    assert_eq!(machine.pane_rebuild_count(ws), 1);

    // Disconnected -> Connecting -> Connected.
    let tok1 = machine.begin_connect(ws).ok_or("missing tok1")?;
    assert_eq!(
        ConnectionWindowPaneResolver::resolve(machine.phase(ws).ok_or("missing phase")?),
        WorkspacePaneKind::ConnectingSpinner
    );
    assert!(machine.complete_connect(ws, tok1));
    assert_eq!(machine.phase(ws), Some(&ConnectionWindowPhase::Connected));
    assert_eq!(
        ConnectionWindowPaneResolver::resolve(&ConnectionWindowPhase::Connected),
        WorkspacePaneKind::ConnectedContent
    );
    assert!(ConnectionWindowPhase::Connected.retains_restore_intent());

    // Connected -> Reconnecting -> Unavailable (failed) -> Reconnecting -> Connected -> Disconnected.
    let tok2 = machine.begin_reconnect(ws).ok_or("missing tok2")?;
    let tok3 = machine.begin_reconnect(ws).ok_or("missing tok3")?;
    // tok2 is fenced out by newer tok3.
    assert!(!machine.complete_connect(ws, tok2));
    assert!(machine.fail_connect(
        ws,
        tok3,
        ConnectionUnavailableView {
            summary: "timeout".to_owned(),
            detail: Some("5s".to_owned()),
            can_retry: true,
            is_auth_failure: false,
        }
    ));
    let phase_after_fail = machine.phase(ws).ok_or("missing phase")?;
    assert!(matches!(
        phase_after_fail,
        ConnectionWindowPhase::Unavailable(_)
    ));
    assert!(phase_after_fail.retains_restore_intent());
    assert_eq!(
        ConnectionWindowPaneResolver::resolve(phase_after_fail),
        WorkspacePaneKind::UnavailableInline
    );

    // Retry from Unavailable -> Reconnecting -> Connected -> Disconnected.
    let tok4 = machine.begin_reconnect(ws).ok_or("missing tok4")?;
    assert!(machine.complete_connect(ws, tok4));
    machine.disconnect(ws);
    assert_eq!(
        machine.phase(ws),
        Some(&ConnectionWindowPhase::Disconnected)
    );
    assert!(!ConnectionWindowPhase::Disconnected.retains_restore_intent());

    // Title and tab label resolvers.
    assert_eq!(
        WindowTitleResolver::resolve("Prod", Some("app"), &ConnectionWindowPhase::Connected),
        "Prod — app"
    );
    assert_eq!(EditorTabLabelResolver::resolve(None, 2, true), "Query 3 •");
    Ok(())
}

#[test]
fn connection_session_liveness_safemode_and_close_guard_transitions() {
    let mut session = ConnectionSession::new(SafeModeLevel::ConfirmWrites);
    assert_eq!(session.reported_status(), ReportedStatus::Disconnected);
    assert!(session.driver().is_none());

    session.install_driver(DriverHandle {
        driver_id: "postgres".to_owned(),
        generation: 1,
    });
    assert_eq!(session.reported_status(), ReportedStatus::Connected);
    assert_eq!(*session.liveness(), SessionLiveness::Live);

    // Outage keeps driver handle non-null while reporting Unreachable.
    session.mark_session_unreachable(42, "socket closed");
    assert_eq!(session.reported_status(), ReportedStatus::Unreachable);
    assert!(session.driver().is_some());

    session.mark_session_live();
    assert_eq!(session.reported_status(), ReportedStatus::Connected);

    assert_eq!(
        session.gate_statement(StatementAccess::Read),
        WriteGate::Run
    );
    assert_eq!(
        session.gate_statement(StatementAccess::Write),
        WriteGate::Confirm
    );

    // Close guard across all TransactionState variants.
    assert_eq!(session.check_close_guard(), CloseDecision::Allow);
    assert!(!session.check_close_guard().requires_confirmation());

    session.transaction_state = TransactionState::InTransaction;
    assert_eq!(
        session.check_close_guard(),
        CloseDecision::WarnOpenTransaction
    );
    assert!(session.check_close_guard().requires_confirmation());

    session.transaction_state = TransactionState::Failed;
    assert_eq!(
        session.check_close_guard(),
        CloseDecision::WarnFailedTransaction
    );

    session.transaction_state = TransactionState::Unknown;
    assert_eq!(
        session.check_close_guard(),
        CloseDecision::WarnUnknownTransaction
    );

    // Explicit teardown clears driver and resets transaction state.
    session.clear_driver();
    assert!(session.driver().is_none());
    assert_eq!(session.reported_status(), ReportedStatus::Disconnected);
    assert_eq!(session.check_close_guard(), CloseDecision::Allow);
}

#[test]
fn library_deferred_reload_drag_rename_and_outline_transitions() {
    let mut model = ConnectionsLibraryModel::new();
    model.upsert_group(LibraryGroup {
        id: LibraryGroupId("g1".to_owned()),
        name: "Prod".to_owned(),
        order: 0,
        is_expanded: true,
    });
    let c1_id = LibraryConnectionId("c1".to_owned());
    model.upsert_connection(sample_library_connection("c1", "Main", None, false, 0));
    let initial_reloads = model.reload_count;

    // Defer reload across overlapping drag + rename, then flush on commit_rename.
    model.begin_drag();
    let mut fresh = LibraryStorageSnapshot::default();
    fresh.groups.insert(
        LibraryGroupId("g1".to_owned()),
        LibraryGroup {
            id: LibraryGroupId("g1".to_owned()),
            name: "Prod".to_owned(),
            order: 0,
            is_expanded: true,
        },
    );
    fresh.connections.insert(
        c1_id.clone(),
        sample_library_connection("c1", "Main", None, false, 0),
    );
    model.request_reload(fresh);
    assert!(model.pending_reload);
    assert_eq!(model.reload_count, initial_reloads);

    model.begin_rename(
        LibraryRowID::Connection {
            section: LibrarySection::Ungrouped,
            id: c1_id.clone(),
        },
        "  Primary  ",
    );
    model.end_drag();
    assert!(model.pending_reload);
    assert!(model.commit_rename());
    assert!(!model.pending_reload);
    assert_eq!(model.reload_count, initial_reloads + 1);

    // Drag-drop into group and toggle favorite.
    assert!(!model.drop_connection(&c1_id, Some(LibraryGroupId("missing".to_owned())), 0));
    assert!(model.drop_connection(&c1_id, Some(LibraryGroupId("g1".to_owned())), 1));
    assert!(model.toggle_favorite(&c1_id));
    assert!(model.outline().iter().any(|r| matches!(
        &r.row_id,
        LibraryRowID::Connection {
            section: LibrarySection::Favorites,
            id,
        } if id == &c1_id
    )));

    // Search + tag filter transitions.
    model.set_tag_filter(Some("nonexistent".to_owned()));
    assert_eq!(model.outline().len(), 1); // Only GroupHeader remains
    model.set_tag_filter(Some("prod".to_owned()));
    model.set_search_query("primary");
    assert_eq!(model.outline().len(), 3); // Favorites + GroupHeader + Group item
}
