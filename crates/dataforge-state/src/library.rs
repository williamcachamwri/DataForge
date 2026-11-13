//! Connections library model: grouped tree with folders, tags, favorites,
//! search, LibraryRowID identity, drag-and-drop, deferred reloads, URL parser,
//! and tablepro-foreign-import integration.

use std::collections::{BTreeMap, BTreeSet};

use tablepro_core::connection_url::{self, ConnectionUrl, ConnectionUrlError};
use tablepro_foreign_import::{CollectedConnection, CollectedImport, ForeignApp};

/// Identifier for a saved connection in the library.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct LibraryConnectionId(pub String);

/// Identifier for a folder/group in the library.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct LibraryGroupId(pub String);

/// Section in which a library row appears.
///
/// Per `welcome-library.md`: a row's identity is its section plus its id
/// (`LibraryRowID`), so a favorite has one row under `Favorites` and another
/// inside its group.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum LibrarySection {
    Favorites,
    Group(LibraryGroupId),
    Ungrouped,
}

/// Unique row identity in the library outline.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum LibraryRowID {
    GroupHeader(LibraryGroupId),
    Connection {
        section: LibrarySection,
        id: LibraryConnectionId,
    },
}

/// A folder/group in the connections library.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LibraryGroup {
    pub id: LibraryGroupId,
    pub name: String,
    pub order: usize,
    pub is_expanded: bool,
}

/// A connection record stored in the library.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LibraryConnection {
    pub id: LibraryConnectionId,
    pub name: String,
    pub driver_id: String,
    pub host: String,
    pub port: Option<u16>,
    pub database: String,
    pub username: String,
    pub group_id: Option<LibraryGroupId>,
    pub tags: BTreeSet<String>,
    pub is_favorite: bool,
    pub order: usize,
}

/// Flattened outline row produced by `rebuild_outline()`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LibraryOutlineRow {
    pub row_id: LibraryRowID,
    pub label: String,
    pub depth: usize,
    pub is_group: bool,
}

/// Backing storage snapshot for the connections library.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct LibraryStorageSnapshot {
    pub groups: BTreeMap<LibraryGroupId, LibraryGroup>,
    pub connections: BTreeMap<LibraryConnectionId, LibraryConnection>,
}

/// Active inline rename session in the library.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenameSession {
    pub row_id: LibraryRowID,
    pub draft_name: String,
}

/// Connections library state machine.
///
/// Per `welcome-library.md`:
/// - The outline is built from storage, never a cache of it.
/// - Every edit mutates storage and calls `rebuild_outline()`.
/// - Reloads wait while `is_dragging` or `rename_session` is active and run in
///   `apply_pending_reload_if_needed()`.
#[derive(Debug, Default, Clone)]
pub struct ConnectionsLibraryModel {
    storage: LibraryStorageSnapshot,
    outline: Vec<LibraryOutlineRow>,
    pub search_query: String,
    pub active_tag_filter: Option<String>,
    pub is_dragging: bool,
    pub rename_session: Option<RenameSession>,
    pub pending_reload: bool,
    pub reload_count: u64,
}

impl ConnectionsLibraryModel {
    /// Creates an empty library model.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns the current outline rows.
    #[must_use]
    pub fn outline(&self) -> &[LibraryOutlineRow] {
        &self.outline
    }

    /// Returns a reference to the backing storage snapshot.
    #[must_use]
    pub const fn storage(&self) -> &LibraryStorageSnapshot {
        &self.storage
    }

    /// Requests a reload from storage; defers if a drag or inline rename is in flight.
    pub fn request_reload(&mut self, fresh: LibraryStorageSnapshot) {
        self.storage = fresh;
        if self.is_dragging || self.rename_session.is_some() {
            self.pending_reload = true;
        } else {
            self.rebuild_outline();
        }
    }

    /// Runs any deferred reload once neither `is_dragging` nor `rename_session` is active.
    pub fn apply_pending_reload_if_needed(&mut self) -> bool {
        if self.pending_reload && !self.is_dragging && self.rename_session.is_none() {
            self.pending_reload = false;
            self.rebuild_outline();
            return true;
        }
        false
    }

    /// Updates `search_query` and rebuilds the outline.
    pub fn set_search_query(&mut self, query: impl Into<String>) {
        self.search_query = query.into();
        self.rebuild_outline();
    }

    /// Updates `active_tag_filter` and rebuilds the outline.
    pub fn set_tag_filter(&mut self, tag: Option<String>) {
        self.active_tag_filter = tag;
        self.rebuild_outline();
    }

    /// Adds or updates a group in storage and rebuilds the outline.
    pub fn upsert_group(&mut self, group: LibraryGroup) {
        self.storage.groups.insert(group.id.clone(), group);
        self.rebuild_outline();
    }

    /// Adds or updates a connection in storage and rebuilds the outline.
    pub fn upsert_connection(&mut self, conn: LibraryConnection) {
        self.storage.connections.insert(conn.id.clone(), conn);
        self.rebuild_outline();
    }

    /// Toggles favorite status for a connection and rebuilds the outline.
    pub fn toggle_favorite(&mut self, id: &LibraryConnectionId) -> bool {
        let Some(conn) = self.storage.connections.get_mut(id) else {
            return false;
        };
        conn.is_favorite = !conn.is_favorite;
        self.rebuild_outline();
        true
    }

    /// Begins a drag operation (defers reloads until `end_drag`).
    pub fn begin_drag(&mut self) {
        self.is_dragging = true;
    }

    /// Ends a drag operation and applies any pending reload.
    pub fn end_drag(&mut self) {
        self.is_dragging = false;
        self.apply_pending_reload_if_needed();
    }

    /// Begins an inline rename session (defers reloads until committed/cancelled).
    pub fn begin_rename(&mut self, row_id: LibraryRowID, initial_name: impl Into<String>) {
        self.rename_session = Some(RenameSession {
            row_id,
            draft_name: initial_name.into(),
        });
    }

    /// Commits the active rename session to storage and flushes any pending reload.
    pub fn commit_rename(&mut self) -> bool {
        let Some(session) = self.rename_session.take() else {
            return false;
        };
        let trimmed = session.draft_name.trim().to_owned();
        if !trimmed.is_empty() {
            match session.row_id {
                LibraryRowID::GroupHeader(gid) => {
                    if let Some(group) = self.storage.groups.get_mut(&gid) {
                        group.name = trimmed;
                    }
                }
                LibraryRowID::Connection { id, .. } => {
                    if let Some(conn) = self.storage.connections.get_mut(&id) {
                        conn.name = trimmed;
                    }
                }
            }
        }
        self.pending_reload = false;
        self.rebuild_outline();
        true
    }

    /// Cancels the active rename session and flushes any pending reload.
    pub fn cancel_rename(&mut self) {
        self.rename_session = None;
        self.apply_pending_reload_if_needed();
    }

    /// Moves a connection into `target_group` at `target_order` via drag-and-drop.
    pub fn drop_connection(
        &mut self,
        id: &LibraryConnectionId,
        target_group: Option<LibraryGroupId>,
        target_order: usize,
    ) -> bool {
        if !self.storage.connections.contains_key(id) {
            return false;
        }
        if let Some(ref gid) = target_group
            && !self.storage.groups.contains_key(gid)
        {
            return false;
        }
        if let Some(conn) = self.storage.connections.get_mut(id) {
            conn.group_id = target_group;
            conn.order = target_order;
        }
        self.rebuild_outline();
        true
    }

    /// Rebuilds the outline from storage, applying search and tag filters.
    pub fn rebuild_outline(&mut self) {
        let query = self.search_query.trim().to_lowercase();
        let tag_filter = self.active_tag_filter.as_deref();
        let matches = |conn: &LibraryConnection| -> bool {
            let tag_ok = tag_filter.is_none_or(|t| conn.tags.contains(t));
            let query_ok = query.is_empty()
                || conn.name.to_lowercase().contains(&query)
                || conn.host.to_lowercase().contains(&query)
                || conn.database.to_lowercase().contains(&query)
                || conn.driver_id.to_lowercase().contains(&query);
            tag_ok && query_ok
        };

        let mut rows = Vec::new();
        let mut all_conns: Vec<&LibraryConnection> = self.storage.connections.values().filter(|c| matches(c)).collect();
        all_conns.sort_by(|a, b| a.order.cmp(&b.order).then_with(|| a.name.cmp(&b.name)));

        // 1. Favorites section (each favorite gets a Favorites rowId AND stays in its group).
        for conn in all_conns.iter().filter(|c| c.is_favorite) {
            rows.push(LibraryOutlineRow {
                row_id: LibraryRowID::Connection {
                    section: LibrarySection::Favorites,
                    id: conn.id.clone(),
                },
                label: conn.name.clone(),
                depth: 0,
                is_group: false,
            });
        }

        // 2. Grouped connections.
        let mut groups: Vec<&LibraryGroup> = self.storage.groups.values().collect();
        groups.sort_by(|a, b| a.order.cmp(&b.order).then_with(|| a.name.cmp(&b.name)));
        for group in groups {
            rows.push(LibraryOutlineRow {
                row_id: LibraryRowID::GroupHeader(group.id.clone()),
                label: group.name.clone(),
                depth: 0,
                is_group: true,
            });
            if group.is_expanded {
                for conn in all_conns.iter().filter(|c| c.group_id.as_ref() == Some(&group.id)) {
                    rows.push(LibraryOutlineRow {
                        row_id: LibraryRowID::Connection {
                            section: LibrarySection::Group(group.id.clone()),
                            id: conn.id.clone(),
                        },
                        label: conn.name.clone(),
                        depth: 1,
                        is_group: false,
                    });
                }
            }
        }

        // 3. Ungrouped connections.
        for conn in all_conns.iter().filter(|c| c.group_id.is_none()) {
            rows.push(LibraryOutlineRow {
                row_id: LibraryRowID::Connection {
                    section: LibrarySection::Ungrouped,
                    id: conn.id.clone(),
                },
                label: conn.name.clone(),
                depth: 0,
                is_group: false,
            });
        }

        self.outline = rows;
        self.reload_count = self.reload_count.saturating_add(1);
    }
}

/// Parses a connection URL using `tablepro_core::connection_url::ConnectionUrl`.
///
/// # Errors
/// Returns [`ConnectionUrlError`] if the URL scheme or syntax is invalid.
pub fn parse_connection_url(raw: &str) -> Result<ConnectionUrl, ConnectionUrlError> {
    connection_url::parse(raw)
}

/// Converts a parsed [`ConnectionUrl`] into a [`LibraryConnection`].
#[must_use]
pub fn connection_from_url(url: &ConnectionUrl, name: Option<&str>) -> LibraryConnection {
    let display_name = name
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map_or_else(
            || {
                if url.host.is_empty() {
                    format!("{} ({})", url.driver_id, url.database)
                } else {
                    format!("{}@{}", url.driver_id, url.host)
                }
            },
            str::to_owned,
        );
    LibraryConnection {
        id: LibraryConnectionId(uuid::Uuid::new_v4().to_string()),
        name: display_name,
        driver_id: url.driver_id.to_owned(),
        host: url.host.clone(),
        port: url.port,
        database: url.database.clone(),
        username: url.username.clone(),
        group_id: None,
        tags: BTreeSet::new(),
        is_favorite: false,
        order: 0,
    }
}

/// Summary of importing connections from `tablepro-foreign-import`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForeignImportReport {
    pub source_app: ForeignApp,
    pub imported_count: usize,
    pub skipped_duplicates: usize,
}

impl ConnectionsLibraryModel {
    /// Imports collected connections from `tablepro-foreign-import` into the library,
    /// skipping duplicates that match an existing `(driver_id, host, port, database, username)`.
    pub fn import_foreign(&mut self, import: &CollectedImport) -> ForeignImportReport {
        let mut imported_count = 0;
        let mut skipped_duplicates = 0;
        for entry in &import.connections {
            if self.has_duplicate_foreign(entry) {
                skipped_duplicates += 1;
                continue;
            }
            let fc = &entry.settings;
            let port = (fc.port != 0).then_some(fc.port);
            let conn = LibraryConnection {
                id: LibraryConnectionId(uuid::Uuid::new_v4().to_string()),
                name: fc.name.clone(),
                driver_id: fc.engine.clone(),
                host: fc.host.clone(),
                port,
                database: fc.database.clone(),
                username: fc.username.clone(),
                group_id: None,
                tags: BTreeSet::from([format!("imported:{}", import.app.display_name())]),
                is_favorite: false,
                order: self.storage.connections.len(),
            };
            self.storage.connections.insert(conn.id.clone(), conn);
            imported_count += 1;
        }
        self.rebuild_outline();
        ForeignImportReport {
            source_app: import.app,
            imported_count,
            skipped_duplicates,
        }
    }

    fn has_duplicate_foreign(&self, entry: &CollectedConnection) -> bool {
        let fc = &entry.settings;
        let port = (fc.port != 0).then_some(fc.port);
        self.storage.connections.values().any(|existing| {
            existing.driver_id == fc.engine
                && existing.host == fc.host
                && existing.port == port
                && existing.database == fc.database
                && existing.username == fc.username
        })
    }
}
