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
