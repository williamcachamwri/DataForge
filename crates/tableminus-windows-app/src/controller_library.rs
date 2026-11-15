//! Live Connections Library (folders, tags, favorites, drag-and-drop reorder,
//! URL parser, and `tablepro-foreign-import` integration) for [`AppController`].

use std::path::PathBuf;
use std::sync::Arc;

use secrecy::SecretString;
use tableminus_services::import_bridge::{
    SUPPORTED_IMPORT_APPS, foreign_inventory, import_from_app, import_from_app_with_dirs,
    parse_connection_url,
};
use tableminus_state::connection_window::AttemptToken;
use tableminus_state::library::{
    ForeignImportReport, LibraryConnection, LibraryConnectionId, LibraryGroup, LibraryGroupId,
    connection_from_url,
};
use tableminus_windows_platform::ForeignAppDirs;
use tablepro_core::credentials::{SecretKind, SecretLookup, SecretVault};
use tablepro_core::{AuthMode as CoreAuthMode, TlsMode as CoreTlsMode};
use tablepro_foreign_import::{ForeignApp, ImportRequest, Inventory};
use tablepro_session::connect::{CredentialSource, NoStoredCredentials};
use tablepro_storage::{KeyringCredentials, SavedConnection, SavedSshAuth};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::app::AppController;
use crate::error::{AppError, Result};
use crate::services::{build_connect_request, default_port_for_engine};
use crate::views::library::{
    AuthMode, ConnectionFormDraft, TlsMode, engine_from_driver_id, import_foreign_connections,
};

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

    /// Parses a connection URL via `tableminus_services::import_bridge::parse_connection_url`
    /// and populates `connection_form`.
    pub fn parse_and_apply_connection_url(&mut self, raw_url: &str) -> Result<LibraryConnection> {
        let parsed = parse_connection_url(raw_url)?;
        let conn = connection_from_url(&parsed, None);
        let engine = engine_from_driver_id(&conn.driver_id);
        self.connection_form.url_input = raw_url.to_owned();
        self.connection_form.name = conn.name.clone();
        self.connection_form.host = conn.host.clone();
        self.connection_form.port = conn.port.unwrap_or_else(|| default_port_for_engine(engine));
        self.connection_form.database = conn.database.clone();
        self.connection_form.username = conn.username.clone();
        self.connection_form.engine = engine;
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

    /// Hydrates `connection_form` from the saved [`LibraryConnection`] identified by `id`
    /// (plus matching [`SavedConnection`] TLS, auth, and SSH fields when present on disk).
    pub fn hydrate_connection_form_from_library(&mut self, id: &str) -> Result<()> {
        let _ = self.hydrate_library_form_with_saved(id)?;
        Ok(())
    }

    /// Connects to a saved [`LibraryConnection`] by ID in the active workspace.
    pub fn connect_library_item_blocking(&mut self, id: &str) -> Result<()> {
        let saved = self.hydrate_library_form_with_saved(id)?;
        let services = self.require_services()?.clone();
        let ws = self.active_workspace;

        let db_password = load_saved_db_password(&services, id, saved.as_ref());
        let request = build_connect_request(
            &services.runtime,
            &self.connection_form,
            db_password,
            self.active_safe_mode(),
        )?;
        let engine = self.connection_form.engine;
        let label = request.metadata.name.clone();
        let creds: Arc<dyn CredentialSource> = match saved {
            Some(ref saved_conn) => Arc::new(KeyringCredentials::for_saved(
                services.credentials.clone(),
                saved_conn,
            )),
            None => Arc::new(NoStoredCredentials),
        };
        self.restoring_connection_id = Some(id.to_owned());
        let _ =
            self.connect_blocking_inner_with_creds(services, ws, request, label, engine, creds)?;
        Ok(())
    }

    /// Reconnects asynchronously to a saved [`LibraryConnection`] by ID using stored credentials.
    pub fn reconnect_saved_connection_async(&mut self, conn_id: &str) -> Result<AttemptToken> {
        let saved = self.hydrate_library_form_with_saved(conn_id)?;
        let services = self.require_services()?.clone();
        let password = load_saved_db_password(&services, conn_id, saved.as_ref());
        self.restoring_connection_id = Some(conn_id.to_owned());
        self.connect_from_draft_async(password)
    }

    fn hydrate_library_form_with_saved(&mut self, id: &str) -> Result<Option<SavedConnection>> {
        let key = LibraryConnectionId(id.to_owned());
        let conn = self
            .library
            .storage()
            .connections
            .get(&key)
            .cloned()
            .ok_or_else(|| AppError::InvalidState(format!("unknown connection id '{id}'")))?;

        let engine = engine_from_driver_id(&conn.driver_id);
        let port = conn.port.unwrap_or_else(|| default_port_for_engine(engine));
        let mut draft = ConnectionFormDraft {
            name: conn.name,
            engine,
            host: conn.host,
            port,
            database: conn.database,
            username: conn.username,
            auth_mode: AuthMode::Password,
            tls_mode: TlsMode::Prefer,
            ssh_enabled: false,
            ssh_host: String::new(),
            ssh_port: 22,
            ssh_user: String::new(),
            ssh_key_path: String::new(),
            url_input: String::new(),
            last_error: None,
        };

        let matched_saved = lookup_saved_connection(self.services.as_deref(), id);
        if let Some(ref saved) = matched_saved {
            apply_saved_security_and_ssh(&mut draft, saved);
        }
        self.connection_form = draft;
        Ok(matched_saved)
    }
}

fn lookup_saved_connection(
    services: Option<&crate::services::ServiceHub>,
    id: &str,
) -> Option<SavedConnection> {
    let services = services?;
    let target_uuid = Uuid::parse_str(id).ok()?;
    let list = services.connection_store.load_blocking().ok()?;
    list.iter().find(|c| c.id == target_uuid).cloned()
}

fn apply_saved_security_and_ssh(draft: &mut ConnectionFormDraft, saved: &SavedConnection) {
    draft.auth_mode = match saved.auth_mode {
        CoreAuthMode::Password => AuthMode::Password,
        CoreAuthMode::Kerberos => AuthMode::SspiIntegrated,
    };
    draft.tls_mode = match saved.tls.mode {
        CoreTlsMode::Disable => TlsMode::Disable,
        CoreTlsMode::Prefer => TlsMode::Prefer,
        CoreTlsMode::Require => TlsMode::Require,
        CoreTlsMode::VerifyFull | CoreTlsMode::Strict => TlsMode::VerifyFull,
    };
    if let Some(ref ssh) = saved.ssh {
        draft.ssh_enabled = true;
        draft.ssh_host.clone_from(&ssh.host);
        draft.ssh_port = ssh.port.unwrap_or(22);
        draft.ssh_user = ssh.username.clone().unwrap_or_default();
        draft.ssh_key_path = match &ssh.auth {
            SavedSshAuth::PrivateKey { path: Some(p), .. } => p.to_string_lossy().into_owned(),
            _ => String::new(),
        };
    }
}

fn load_saved_db_password(
    services: &crate::services::ServiceHub,
    id: &str,
    saved: Option<&SavedConnection>,
) -> Option<SecretString> {
    let uuid = saved.map(|s| s.id).or_else(|| Uuid::parse_str(id).ok())?;
    let lookup = services
        .runtime
        .block_on(
            services
                .credentials
                .load(uuid, SecretKind::DatabasePassword),
        )
        .ok()?;
    match lookup {
        SecretLookup::Found(secret) => Some(secret),
        SecretLookup::NotStored => None,
    }
}
