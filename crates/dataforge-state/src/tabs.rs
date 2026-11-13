//! Tab strip model, detach/re-attach, Reopen Closed Tab, Ctrl+W precedence,
//! RestoredTabMerge, hasObservedTabs guard, and crash-safe session restore.

use serde::{Deserialize, Serialize};

use crate::connection_window::EditorTabLabelResolver;

/// Unique identifier for a tab.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct TabId(pub u64);

/// Unique identifier for a connection's tab strip.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct TabConnectionId(pub String);

/// Kind of content hosted by a tab.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TabKind {
    QueryEditor { sql: String },
    TableData { table_name: String },
    Structure { table_name: String },
}

/// A single tab in a connection's tab strip.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TabItem {
    pub id: TabId,
    pub connection_id: TabConnectionId,
    pub title: String,
    pub kind: TabKind,
    pub pinned: bool,
    pub has_unsaved_edits: bool,
    pub has_active_filters: bool,
    pub has_active_sort: bool,
}

impl TabItem {
    /// Whether this tab has state that `open_table_tab` must never overwrite.
    #[must_use]
    pub const fn protects_against_replacement(&self) -> bool {
        self.has_unsaved_edits || self.has_active_filters || self.has_active_sort
    }
}

/// Entry pushed onto the closed-tab stack for `Reopen Closed Tab`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClosedTabEntry {
    pub tab: TabItem,
    pub original_index: usize,
}

/// Result of `Ctrl+W` (`perform_close`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloseActionResult {
    ClosedTab(TabId),
    CloseWindow,
}

/// Per-connection tab strip state machine.
#[derive(Debug, Default, Clone)]
pub struct TabStripModel {
    tabs: Vec<TabItem>,
    active_tab: Option<TabId>,
    closed_stack: Vec<ClosedTabEntry>,
    next_id: u64,
    /// Guard: true only after saved tabs for this connection have been observed/loaded.
    pub has_observed_tabs: bool,
}

impl TabStripModel {
    /// Creates an empty tab strip.
    #[must_use]
    pub fn new() -> Self {
        Self {
            tabs: Vec::new(),
            active_tab: None,
            closed_stack: Vec::new(),
            next_id: 1,
            has_observed_tabs: false,
        }
    }

    fn alloc_id(&mut self) -> TabId {
        let id = TabId(self.next_id);
        self.next_id = self.next_id.saturating_add(1);
        id
    }

    /// Returns all tabs in pill order.
    #[must_use]
    pub fn tabs(&self) -> &[TabItem] {
        &self.tabs
    }

    /// Returns the currently selected tab id.
    #[must_use]
    pub const fn active_tab(&self) -> Option<TabId> {
        self.active_tab
    }

    /// Selects `id` if present in this strip.
    pub fn select(&mut self, id: TabId) -> bool {
        if self.tabs.iter().any(|t| t.id == id) {
            self.active_tab = Some(id);
            true
        } else {
            false
        }
    }

    /// Opens a new query editor tab with a guaranteed non-blank label.
    pub fn open_query_tab(&mut self, connection_id: TabConnectionId, explicit_title: Option<&str>) -> TabId {
        let id = self.alloc_id();
        let title = EditorTabLabelResolver::resolve(explicit_title, self.tabs.len(), false);
        self.tabs.push(TabItem {
            id,
            connection_id,
            title,
            kind: TabKind::QueryEditor { sql: String::new() },
            pinned: false,
            has_unsaved_edits: false,
            has_active_filters: false,
            has_active_sort: false,
        });
        self.active_tab = Some(id);
        id
    }

    /// Opens a table data tab.
    ///
    /// Per `connection-window.md`: never replaces a tab with unsaved edits,
    /// active filters, or active sorting; opens a new tab instead.
    pub fn open_table_tab(&mut self, connection_id: TabConnectionId, table_name: impl Into<String>) -> TabId {
        let table_name = table_name.into();
        if let Some(active_id) = self.active_tab
            && let Some(active) = self.tabs.iter_mut().find(|t| t.id == active_id)
            && matches!(active.kind, TabKind::TableData { .. })
            && !active.protects_against_replacement()
            && !active.pinned
        {
            active.title.clone_from(&table_name);
            active.kind = TabKind::TableData { table_name };
            return active_id;
        }

        let id = self.alloc_id();
        let title = EditorTabLabelResolver::resolve(Some(&table_name), self.tabs.len(), false);
        self.tabs.push(TabItem {
            id,
            connection_id,
            title,
            kind: TabKind::TableData { table_name },
            pinned: false,
            has_unsaved_edits: false,
            has_active_filters: false,
            has_active_sort: false,
        });
        self.active_tab = Some(id);
        id
    }

    /// Reorders a tab from `from_idx` to `to_idx` (pill drag-reorder).
    pub fn drag_reorder(&mut self, from_idx: usize, to_idx: usize) -> bool {
        if from_idx >= self.tabs.len() || to_idx >= self.tabs.len() {
            return false;
        }
        if from_idx == to_idx {
            return true;
        }
        let item = self.tabs.remove(from_idx);
        self.tabs.insert(to_idx, item);
        true
    }

    /// Closes a tab by user action and pushes it to the `Reopen Closed Tab` stack.
    pub fn close_tab_by_user(&mut self, id: TabId) -> bool {
        let Some(pos) = self.tabs.iter().position(|t| t.id == id) else {
            return false;
        };
        let removed = self.tabs.remove(pos);
        self.closed_stack.push(ClosedTabEntry {
            tab: removed,
            original_index: pos,
        });
        if self.active_tab == Some(id) {
            self.active_tab = if self.tabs.is_empty() {
                None
            } else {
                let next_idx = pos.min(self.tabs.len().saturating_sub(1));
                Some(self.tabs[next_idx].id)
            };
        }
        true
    }

    /// Reopens the most recently closed tab at its original index (clamped).
    pub fn reopen_closed_tab(&mut self) -> Option<TabId> {
        let entry = self.closed_stack.pop()?;
        let id = entry.tab.id;
        let idx = entry.original_index.min(self.tabs.len());
        self.tabs.insert(idx, entry.tab);
        self.active_tab = Some(id);
        Some(id)
    }

    /// `Ctrl+W` handler: closes the frontmost (active) tab before closing the window.
    pub fn perform_close(&mut self) -> CloseActionResult {
        if let Some(active) = self.active_tab
            && self.close_tab_by_user(active)
        {
            return CloseActionResult::ClosedTab(active);
        }
        if let Some(first_id) = self.tabs.first().map(|t| t.id)
            && self.close_tab_by_user(first_id)
        {
            return CloseActionResult::ClosedTab(first_id);
        }
        CloseActionResult::CloseWindow
    }

    /// Detaches a tab for moving to another window or connection.
    pub fn detach_tab(&mut self, id: TabId) -> Option<TabItem> {
        let pos = self.tabs.iter().position(|t| t.id == id)?;
        let tab = self.tabs.remove(pos);
        if self.active_tab == Some(id) {
            self.active_tab = self.tabs.first().map(|t| t.id);
        }
        Some(tab)
    }

    /// Attaches a detached tab onto this strip.
    pub fn attach_tab(&mut self, mut tab: TabItem, target_connection: TabConnectionId) -> TabId {
        let id = self.alloc_id();
        tab.id = id;
        tab.connection_id = target_connection;
        self.tabs.push(tab);
        self.active_tab = Some(id);
        id
    }
}

/// Merges restored tabs into a strip without clobbering tabs that arrived before restore.
#[derive(Debug, Clone, Copy, Default)]
pub struct RestoredTabMerge;

impl RestoredTabMerge {
    /// Prepends `restored` tabs ahead of any pre-existing tabs that arrived before
    /// the async restore completed (e.g. reopened or linked tabs), preserving both.
    pub fn merge(strip: &mut TabStripModel, restored: Vec<TabItem>) {
        let pre_existing = std::mem::take(&mut strip.tabs);
        let mut merged = Vec::with_capacity(restored.len() + pre_existing.len());
        for mut tab in restored {
            tab.id = strip.alloc_id();
            merged.push(tab);
        }
        let had_active = strip.active_tab;
        merged.extend(pre_existing);
        strip.tabs = merged;
        if had_active.is_none() {
            strip.active_tab = strip.tabs.first().map(|t| t.id);
        }
        strip.has_observed_tabs = true;
    }
}

/// Reason a tab-save operation was accepted or refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SaveTabsOutcome {
    Saved,
    SkippedUnobserved,
    SkippedEmptyOnSessionLoss,
}

/// Guards persisted tab lists against premature clearing on teardown or session loss.
#[derive(Debug, Default, Clone)]
pub struct SavedTabsStore {
    persisted: std::collections::BTreeMap<TabConnectionId, Vec<TabItem>>,
}

impl SavedTabsStore {
    /// Creates an empty saved-tabs store.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Reads the saved tabs for `conn`.
    #[must_use]
    pub fn get(&self, conn: &TabConnectionId) -> Option<&[TabItem]> {
        self.persisted.get(conn).map(Vec::as_slice)
    }

    /// Saves tabs during teardown (`save_aggregated_sync`):
    /// - Refuses to clear if `!strip.has_observed_tabs`.
    /// - Skips an empty list if `session_lost` is true (only user close/move may clear).
    pub fn save_aggregated_sync(
        &mut self,
        conn: TabConnectionId,
        strip: &TabStripModel,
        session_lost: bool,
    ) -> SaveTabsOutcome {
        if !strip.has_observed_tabs {
            return SaveTabsOutcome::SkippedUnobserved;
        }
        if strip.tabs.is_empty() && session_lost {
            return SaveTabsOutcome::SkippedEmptyOnSessionLoss;
        }
        self.persisted.insert(conn, strip.tabs.clone());
        SaveTabsOutcome::Saved
    }

    /// Explicitly clears saved tabs when the user closes or moves out the last tab.
    pub fn clear_by_user(&mut self, conn: &TabConnectionId, strip: &TabStripModel) -> SaveTabsOutcome {
        if !strip.has_observed_tabs {
            return SaveTabsOutcome::SkippedUnobserved;
        }
        if strip.tabs.is_empty() {
            self.persisted.remove(conn);
        } else {
            self.persisted.insert(conn.clone(), strip.tabs.clone());
        }
        SaveTabsOutcome::Saved
    }
}

/// Queued cross-connection tab move.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingTabMove {
    pub tab: TabItem,
    pub target_connection: TabConnectionId,
}

/// Coordinates moving a query tab between connections.
///
/// Per `connection-window.md`: a tab stays in its source until the target
/// workspace is `Connected` and its strip has `has_observed_tabs == true`.
/// A queued move is dropped if the target turns `Unavailable`.
#[derive(Debug, Default, Clone)]
pub struct MoveQueryTabQueue {
    pending: Vec<PendingTabMove>,
}

impl MoveQueryTabQueue {
    /// Queues a tab move to `target_connection`.
    pub fn enqueue(&mut self, tab: TabItem, target_connection: TabConnectionId) {
        self.pending.push(PendingTabMove {
            tab,
            target_connection,
        });
    }

    /// Returns the number of queued moves.
    #[must_use]
    pub fn len(&self) -> usize {
        self.pending.len()
    }

    /// Returns whether the queue is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.pending.is_empty()
    }

    /// Processes queued moves for `target_connection` given its current phase and strip.
    pub fn process_for_target(
        &mut self,
        target_connection: &TabConnectionId,
        target_phase: &crate::connection_window::ConnectionWindowPhase,
        target_strip: &mut TabStripModel,
    ) -> usize {
        if matches!(target_phase, crate::connection_window::ConnectionWindowPhase::Unavailable(_)) {
            self.pending.retain(|m| &m.target_connection != target_connection);
            return 0;
        }
        if !matches!(target_phase, crate::connection_window::ConnectionWindowPhase::Connected)
            || !target_strip.has_observed_tabs
        {
            return 0;
        }
        let mut moved = 0;
        let mut remaining = Vec::new();
        for item in self.pending.drain(..) {
            if &item.target_connection == target_connection {
                target_strip.attach_tab(item.tab, target_connection.clone());
                moved += 1;
            } else {
                remaining.push(item);
            }
        }
        self.pending = remaining;
        moved
    }
}

/// Entry in a session restore plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RestoredConnectionEntry {
    pub connection_id: TabConnectionId,
    pub should_dial: bool,
    pub connect_immediately_on_launch: bool,
}

/// Computes which connections are restored and which one connects on launch.
///
/// Per `connection-window.md`: every connection on the strip comes back, only
/// those with `retains_restore_intent` dial again, and only the frontmost
/// connection connects immediately on launch.
#[derive(Debug, Clone, Copy, Default)]
pub struct SessionRestorePlanner;

impl SessionRestorePlanner {
    /// Builds the restore plan for `connections` where `frontmost_index` is the active strip item.
    #[must_use]
    pub fn plan(
        connections: &[(TabConnectionId, crate::connection_window::ConnectionWindowPhase)],
        frontmost_index: usize,
    ) -> Vec<RestoredConnectionEntry> {
        connections
            .iter()
            .enumerate()
            .map(|(idx, (conn, phase))| {
                let should_dial = phase.retains_restore_intent();
                RestoredConnectionEntry {
                    connection_id: conn.clone(),
                    should_dial,
                    connect_immediately_on_launch: should_dial && idx == frontmost_index,
                }
            })
            .collect()
    }
}

/// Checksum-protected snapshot envelope for crash-during-save recovery.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TabSessionEnvelope {
    pub checksum: u64,
    pub strips: std::collections::BTreeMap<TabConnectionId, Vec<TabItem>>,
}

fn fnv1a_64(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in bytes {
        hash ^= u64::from(b);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    hash
}

/// Crash-safe session persistence using primary + backup files with checksum verification.
#[derive(Debug, Clone, Copy, Default)]
pub struct CrashSafeTabPersistence;

impl CrashSafeTabPersistence {
    /// Saves `strips` to `primary_path`, keeping the previous valid file at `backup_path`.
    ///
    /// # Errors
    /// Returns an IO or serialization error if writing fails.
    pub fn save_atomic(
        primary_path: &std::path::Path,
        backup_path: &std::path::Path,
        strips: &std::collections::BTreeMap<TabConnectionId, Vec<TabItem>>,
    ) -> Result<(), String> {
        let payload_bytes = serde_json::to_vec(strips).map_err(|e| e.to_string())?;
        let envelope = TabSessionEnvelope {
            checksum: fnv1a_64(&payload_bytes),
            strips: strips.clone(),
        };
        let encoded = serde_json::to_vec_pretty(&envelope).map_err(|e| e.to_string())?;

        if primary_path.exists() {
            let _ = std::fs::copy(primary_path, backup_path);
        }
        let tmp_path = primary_path.with_extension("tmp");
        std::fs::write(&tmp_path, &encoded).map_err(|e| e.to_string())?;
        std::fs::rename(&tmp_path, primary_path).map_err(|e| e.to_string())?;
        Ok(())
    }

    /// Loads the primary snapshot if valid, otherwise recovers from `backup_path`.
    ///
    /// # Errors
    /// Returns an error only if both primary and backup are missing or corrupt.
    pub fn load_with_recovery(
        primary_path: &std::path::Path,
        backup_path: &std::path::Path,
    ) -> Result<std::collections::BTreeMap<TabConnectionId, Vec<TabItem>>, String> {
        if let Ok(strips) = Self::try_load_verified(primary_path) {
            return Ok(strips);
        }
        Self::try_load_verified(backup_path)
    }

    fn try_load_verified(
        path: &std::path::Path,
    ) -> Result<std::collections::BTreeMap<TabConnectionId, Vec<TabItem>>, String> {
        let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
        let envelope: TabSessionEnvelope = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
        let payload_bytes = serde_json::to_vec(&envelope.strips).map_err(|e| e.to_string())?;
        if fnv1a_64(&payload_bytes) != envelope.checksum {
            return Err("tab session checksum mismatch".to_owned());
        }
        Ok(envelope.strips)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use proptest::prelude::*;

    use super::*;
    use crate::connection_window::{ConnectionUnavailableView, ConnectionWindowPhase};

    #[test]
    fn open_table_tab_never_replaces_unsaved_or_filtered_tab() {
        let mut strip = TabStripModel::new();
        let conn = TabConnectionId("pg-1".to_owned());
        let first = strip.open_table_tab(conn.clone(), "users");
        strip.tabs[0].has_unsaved_edits = true;

        let second = strip.open_table_tab(conn.clone(), "orders");
        assert_ne!(first, second);
        assert_eq!(strip.tabs().len(), 2);

        // Unmodified active table tab can be replaced in place.
        let third = strip.open_table_tab(conn, "products");
        assert_eq!(third, second);
        assert_eq!(strip.tabs().len(), 2);
    }

    #[test]
    fn ctrl_w_closes_frontmost_tab_before_window_and_reopen_restores() {
        let mut strip = TabStripModel::new();
        let conn = TabConnectionId("pg-1".to_owned());
        let t1 = strip.open_query_tab(conn.clone(), Some("Q1"));
        let t2 = strip.open_query_tab(conn, Some("Q2"));

        assert_eq!(strip.perform_close(), CloseActionResult::ClosedTab(t2));
        assert_eq!(strip.reopen_closed_tab(), Some(t2));
        assert_eq!(strip.tabs().len(), 2);

        assert_eq!(strip.perform_close(), CloseActionResult::ClosedTab(t2));
        assert_eq!(strip.perform_close(), CloseActionResult::ClosedTab(t1));
        assert_eq!(strip.perform_close(), CloseActionResult::CloseWindow);
    }

    #[test]
    fn restored_tab_merge_keeps_pre_restore_tabs_and_respects_observed_guard() {
        let mut strip = TabStripModel::new();
        let conn = TabConnectionId("pg-1".to_owned());
        let pre = strip.open_query_tab(conn.clone(), Some("Reopened"));

        let mut store = SavedTabsStore::new();
        // Before has_observed_tabs, teardown save is refused.
        assert_eq!(
            store.save_aggregated_sync(conn.clone(), &strip, false),
            SaveTabsOutcome::SkippedUnobserved
        );

        let restored = vec![TabItem {
            id: TabId(99),
            connection_id: conn.clone(),
            title: "Saved Q1".to_owned(),
            kind: TabKind::QueryEditor {
                sql: "SELECT 1".to_owned(),
            },
            pinned: false,
            has_unsaved_edits: false,
            has_active_filters: false,
            has_active_sort: false,
        }];
        RestoredTabMerge::merge(&mut strip, restored);
        assert!(strip.has_observed_tabs);
        assert_eq!(strip.tabs().len(), 2);
        assert_eq!(strip.tabs()[0].title, "Saved Q1");
        assert_eq!(strip.tabs()[1].id, pre);

        assert_eq!(
            store.save_aggregated_sync(conn.clone(), &strip, false),
            SaveTabsOutcome::Saved
        );

        // Simulate lost session clearing the strip: must NOT wipe saved tabs.
        strip.tabs.clear();
        assert_eq!(
            store.save_aggregated_sync(conn.clone(), &strip, true),
            SaveTabsOutcome::SkippedEmptyOnSessionLoss
        );
        assert_eq!(store.get(&conn).map(<[TabItem]>::len), Some(2));
    }

    #[test]
    fn session_restore_connects_only_frontmost_connection_on_launch() {
        let c1 = TabConnectionId("c1".to_owned());
        let c2 = TabConnectionId("c2".to_owned());
        let c3 = TabConnectionId("c3".to_owned());
        let plan = SessionRestorePlanner::plan(
            &[
                (c1, ConnectionWindowPhase::Connected),
                (c2, ConnectionWindowPhase::Disconnected),
                (
                    c3,
                    ConnectionWindowPhase::Unavailable(ConnectionUnavailableView {
                        summary: "timeout".to_owned(),
                        detail: None,
                        can_retry: true,
                        is_auth_failure: false,
                    }),
                ),
            ],
            2,
        );
        assert!(plan[0].should_dial);
        assert!(!plan[0].connect_immediately_on_launch);
        assert!(!plan[1].should_dial);
        assert!(!plan[1].connect_immediately_on_launch);
        assert!(plan[2].should_dial);
        assert!(plan[2].connect_immediately_on_launch);
    }

    #[test]
    fn crash_during_save_recovers_from_backup() -> Result<(), Box<dyn std::error::Error>> {
        let dir = tempfile::tempdir()?;
        let primary = dir.path().join("tabs.json");
        let backup = dir.path().join("tabs.bak.json");
        let conn = TabConnectionId("pg-1".to_owned());

        let mut strips = BTreeMap::new();
        strips.insert(
            conn.clone(),
            vec![TabItem {
                id: TabId(1),
                connection_id: conn.clone(),
                title: "Q1".to_owned(),
                kind: TabKind::QueryEditor {
                    sql: "SELECT 1".to_owned(),
                },
                pinned: false,
                has_unsaved_edits: false,
                has_active_filters: false,
                has_active_sort: false,
            }],
        );
        CrashSafeTabPersistence::save_atomic(&primary, &backup, &strips)?;
        // Second save creates backup of first save.
        CrashSafeTabPersistence::save_atomic(&primary, &backup, &strips)?;
        // Simulate crash corrupting primary file.
        std::fs::write(&primary, b"{truncated")?;

        let recovered = CrashSafeTabPersistence::load_with_recovery(&primary, &backup)?;
        assert_eq!(recovered.get(&conn).map(Vec::len), Some(1));
        Ok(())
    }

    proptest! {
        #[test]
        fn prop_drag_reorder_preserves_tab_multiset(
            count in 1usize..12,
            moves in prop::collection::vec((0usize..12, 0usize..12), 0..20),
        ) {
            let mut strip = TabStripModel::new();
            let conn = TabConnectionId("prop".to_owned());
            for i in 0..count {
                strip.open_query_tab(conn.clone(), Some(&format!("Tab {i}")));
            }
            let mut expected_ids: Vec<TabId> = strip.tabs().iter().map(|t| t.id).collect();
            expected_ids.sort();

            for (from, to) in moves {
                let from = from % count;
                let to = to % count;
                prop_assert!(strip.drag_reorder(from, to));
            }
            let mut actual_ids: Vec<TabId> = strip.tabs().iter().map(|t| t.id).collect();
            actual_ids.sort();
            prop_assert_eq!(actual_ids, expected_ids);
        }
    }
}
