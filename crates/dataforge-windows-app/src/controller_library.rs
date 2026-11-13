//! Live Connections Library (folders, tags, favorites, drag-and-drop reorder,
//! URL parser, and `tablepro-foreign-import` integration) for [`AppController`].

use std::path::PathBuf;

use dataforge_services::import_bridge::{
    SUPPORTED_IMPORT_APPS, foreign_inventory, import_from_app, import_from_app_with_dirs,
    parse_connection_url,
};
use dataforge_state::library::{
    ForeignImportReport, LibraryConnection, LibraryConnectionId, LibraryGroup, LibraryGroupId,
    connection_from_url,
};
use dataforge_windows_platform::ForeignAppDirs;
use tablepro_foreign_import::{ForeignApp, ImportRequest, Inventory};
use tokio_util::sync::CancellationToken;

use crate::app::AppController;
use crate::error::{AppError, Result};
use crate::views::library::import_foreign_connections;

impl AppController {
    /// Creates or updates a folder/group in the Connections Library.
    pub fn upsert_library_folder(&mut self, id: &str, name: &str, order: usize) {
        self.library.upsert_group(LibraryGroup {
            id: LibraryGroupId(id.to_owned()),
            name: name.to_owned(),
            order,
            is_expanded: true,
        });
    }

    /// Saves the current `connection_form` into the Connections Library with optional folder and tags.
    pub fn save_form_to_library(
        &mut self,
        id: &str,
        group_id: Option<&str>,
        tags: std::collections::BTreeSet<String>,
        is_favorite: bool,
    ) {
        let driver_id = crate::services::driver_id_from_engine(self.connection_form.engine);
        let name = if self.connection_form.name.trim().is_empty() {
            format!(
                "{} ({})",
                self.connection_form.engine.display_name(),
                self.connection_form.database
            )
        } else {
            self.connection_form.name.clone()
        };
        self.library.upsert_connection(LibraryConnection {
            id: LibraryConnectionId(id.to_owned()),
            name,
            driver_id: driver_id.to_owned(),
            host: self.connection_form.host.clone(),
            port: Some(self.connection_form.port),
            database: self.connection_form.database.clone(),
            username: self.connection_form.username.clone(),
            group_id: group_id.map(|g| LibraryGroupId(g.to_owned())),
            tags,
            is_favorite,
            order: 0,
        });
    }

    /// Toggles favorite status on a saved connection in the Connections Library.
    pub fn toggle_library_favorite(&mut self, id: &str) -> bool {
        self.library
            .toggle_favorite(&LibraryConnectionId(id.to_owned()))
    }

    /// Reorders a library connection via drag-and-drop into `target_group` at `target_index`.
    pub fn reorder_library_connection(
        &mut self,
        id: &str,
        target_group: Option<&str>,
        target_index: usize,
    ) -> bool {
        let group = target_group.map(|g| LibraryGroupId(g.to_owned()));
        self.library
            .drop_connection(&LibraryConnectionId(id.to_owned()), group, target_index)
    }

    /// Parses a connection URL via `dataforge_services::import_bridge::parse_connection_url`
    /// and populates `connection_form`.
    pub fn parse_and_apply_connection_url(&mut self, raw_url: &str) -> Result<LibraryConnection> {
        let parsed = parse_connection_url(raw_url)?;
        let conn = connection_from_url(&parsed, None);
        self.connection_form.url_input = raw_url.to_owned();
        self.connection_form.name = conn.name.clone();
        self.connection_form.host = conn.host.clone();
        self.connection_form.port = conn.port.unwrap_or(5432);
        self.connection_form.database = conn.database.clone();
        self.connection_form.username = conn.username.clone();
        self.connection_form.engine =
            crate::views::library::engine_from_driver_id(&conn.driver_id);
        self.connection_form.last_error = None;
        Ok(conn)
    }

    /// Returns available inventories across all 5 supported foreign apps for `dirs`.
    #[must_use]
    pub fn scan_foreign_inventories(&self, dirs: &ForeignAppDirs) -> Vec<(ForeignApp, Inventory)> {
        SUPPORTED_IMPORT_APPS
            .iter()
            .map(|&app| (app, foreign_inventory(app, dirs)))
            .collect()
    }

    /// Imports connections and saved queries from `app` using Windows default directories.
    pub fn import_foreign_app_default(
        &mut self,
        app: ForeignApp,
        picked_file: Option<PathBuf>,
    ) -> Result<ForeignImportReport> {
        let cancel = CancellationToken::new();
        let req = ImportRequest {
            include_passwords: false,
            include_saved_queries: true,
            saved_query_byte_limit: 1024 * 1024,
        };
        let collected = import_from_app(app, picked_file, &req, &cancel)?;
        Ok(import_foreign_connections(&mut self.library, &collected))
    }

    /// Imports connections and saved queries from `app` using explicit `dirs`.
    pub fn import_foreign_app_from_dirs(
        &mut self,
        app: ForeignApp,
        dirs: &ForeignAppDirs,
        picked_file: Option<PathBuf>,
    ) -> Result<ForeignImportReport> {
        let cancel = CancellationToken::new();
        let req = ImportRequest {
            include_passwords: false,
            include_saved_queries: true,
            saved_query_byte_limit: 1024 * 1024,
        };
        let collected = import_from_app_with_dirs(app, dirs, picked_file, &req, &cancel)?;
        Ok(import_foreign_connections(&mut self.library, &collected))
    }

    /// Connects to a saved [`LibraryConnection`] by ID in the active workspace.
    pub fn connect_library_item_blocking(&mut self, id: &str) -> Result<()> {
        let outline_label = self
            .library
            .outline()
            .iter()
            .find(|r| match &r.row_id {
                dataforge_state::library::LibraryRowID::Connection { id: cid, .. } => cid.0 == id,
                dataforge_state::library::LibraryRowID::GroupHeader(_) => false,
            })
            .map(|r| r.label.clone())
            .ok_or_else(|| AppError::InvalidState(format!("unknown connection id '{id}'")))?;
        self.connection_form.name = outline_label;
        let _ = self.connect_from_draft_blocking(None)?;
        Ok(())
    }
}
