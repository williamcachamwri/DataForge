//! Live connection lifecycle, `AttemptToken` cancel fencing, schema sidebar,
//! and structure view wiring for [`AppController`].

use std::path::Path;
use std::sync::Arc;

use dataforge_gfx::glyphs::EngineKind;
use dataforge_services::import_bridge::test_connection;
use dataforge_state::connection_window::{AttemptToken, ConnectionUnavailableView};
use secrecy::SecretString;
use tablepro_core::SafeModeLevel;
use tablepro_core::credentials::CredentialInteraction;
use tablepro_core::meta::{ObjectInfo, ObjectKind, ObjectScope, TableRef};
use tablepro_session::connect::{ConnectRequest, CredentialSource, NoStoredCredentials};

use crate::app::{AppController, ScreenMode};
use crate::error::{AppError, Result};
use crate::services::{DialogCredentialPrompter, ServiceEvent, build_connect_request};
use crate::views::content::StructureColumnRow;
use crate::views::library::ConnectionFormDraft;
use crate::views::sidebar::{SidebarRow, SidebarSectionKind};

impl AppController {
    /// Returns the active [`SafeModeLevel`] from controller state.
    #[must_use]
    pub const fn active_safe_mode(&self) -> SafeModeLevel {
        if self.safe_mode_enabled {
            SafeModeLevel::ConfirmWrites
        } else {
            SafeModeLevel::Off
        }
    }

    /// Starts an asynchronous connection for `active_workspace` from `connection_form`,
    /// fencing stale attempts with [`AttemptToken`].
    pub fn connect_from_draft_async(&mut self, password: Option<SecretString>) -> Result<AttemptToken> {
        let services = self.require_services()?.clone();
        let ws = self.active_workspace;
        let request = build_connect_request(
            &services.runtime,
            &self.connection_form,
            password,
            self.active_safe_mode(),
        )?;
        let engine = self.connection_form.engine;
        let label = request.metadata.name.clone();
        self.start_connect_task(services, ws, request, label, engine)
    }

    /// Connects synchronously (blocking on `AppRuntime`) from `connection_form` and refreshes schema.
    pub fn connect_from_draft_blocking(&mut self, password: Option<SecretString>) -> Result<AttemptToken> {
        let services = self.require_services()?.clone();
        let ws = self.active_workspace;
        let request = build_connect_request(
            &services.runtime,
            &self.connection_form,
            password,
            self.active_safe_mode(),
        )?;
        let engine = self.connection_form.engine;
        let label = request.metadata.name.clone();
        self.connect_blocking_inner(services, ws, request, label, engine)
    }

    /// Connects synchronously to a local SQLite or DuckDB database file.
    pub fn connect_local_file_blocking(&mut self, path: &Path) -> Result<AttemptToken> {
        let is_duckdb = path
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| e.eq_ignore_ascii_case("duckdb"));
        let engine = if is_duckdb {
            EngineKind::Duckdb
        } else {
            EngineKind::Sqlite
        };
        let label = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("database")
            .to_owned();
        self.connection_form = ConnectionFormDraft {
            name: label,
            engine,
            database: path.to_string_lossy().into_owned(),
            ..ConnectionFormDraft::default()
        };
        self.connect_from_draft_blocking(None)
    }

    fn start_connect_task(
        &mut self,
        services: Arc<crate::services::ServiceHub>,
        ws: dataforge_state::connection_window::WorkspaceId,
        request: ConnectRequest,
        label: String,
        engine: EngineKind,
    ) -> Result<AttemptToken> {
        self.connections.select_workspace(ws);
        let token = self
            .connections
            .begin_connect(ws)
            .ok_or_else(|| AppError::InvalidState("workspace is already connecting".to_owned()))?;
        self.active_attempt = Some(token);
        let cancel = services.register_attempt(ws, token);
        let headless = self.headless;

        let svc = services.clone();
        let _task = services.runtime.spawn(async move {
            let creds: Arc<dyn CredentialSource> = Arc::new(NoStoredCredentials);
            let prompter = Arc::new(DialogCredentialPrompter::new(headless));
            let mut guard = svc.sessions.write().await;
            let res = guard
                .connect_workspace(ws, request, creds, prompter, cancel)
                .await;
            drop(guard);
            match res {
                Ok(_) => svc.push_event(ServiceEvent::Connected {
                    workspace_id: ws,
                    token,
                    label,
                    engine,
                }),
                Err(err) => svc.push_event(ServiceEvent::ConnectFailed {
                    workspace_id: ws,
                    token,
                    summary: err.to_string(),
                    is_auth_failure: false,
                }),
            }
        });
        Ok(token)
    }

    fn connect_blocking_inner(
        &mut self,
        services: Arc<crate::services::ServiceHub>,
        ws: dataforge_state::connection_window::WorkspaceId,
        request: ConnectRequest,
        label: String,
        engine: EngineKind,
    ) -> Result<AttemptToken> {
        self.connections.select_workspace(ws);
        let token = self
            .connections
            .begin_connect(ws)
            .ok_or_else(|| AppError::InvalidState("workspace is already connecting".to_owned()))?;
        self.active_attempt = Some(token);
        let cancel = services.register_attempt(ws, token);
        let headless = self.headless;

        let res = services.runtime.block_on(async {
            let creds: Arc<dyn CredentialSource> = Arc::new(NoStoredCredentials);
            let prompter = Arc::new(DialogCredentialPrompter::new(headless));
            let mut guard = services.sessions.write().await;
            guard
                .connect_workspace(ws, request, creds, prompter, cancel)
                .await
        });

        match res {
            Ok(_) => {
                self.apply_connected(ws, token, &label, engine);
                let _ = self.refresh_schema_sidebar_blocking();
                Ok(token)
            }
            Err(err) => {
                self.apply_connect_failed(ws, token, err.to_string(), false);
                Err(AppError::Service(err))
            }
        }
    }

    /// Cancels an in-flight connection attempt and fences its [`AttemptToken`].
    pub fn cancel_active_connect(&mut self) {
        if let Some(services) = &self.services {
            services.cancel_attempt(self.active_workspace);
        }
        if let Some(_token) = self.active_attempt.take() {
            self.connections.cancel_connect(self.active_workspace);
        }
    }

    /// Disconnects the active workspace session.
    pub fn disconnect_active_workspace(&mut self) -> Result<()> {
        let services = self.require_services()?.clone();
        let ws = self.active_workspace;
        services.cancel_attempt(ws);
        services.runtime.block_on(async {
            let mut guard = services.sessions.write().await;
            guard.disconnect_workspace(ws)
        })?;
        self.connections.disconnect(ws);
        self.active_table = None;
        self.cached_schemas.clear();
        self.cached_objects.clear();
        self.sidebar_rows.clear();
        self.sync_layout_counts();
        Ok(())
    }

    /// Reconnects the active workspace using the current `connection_form`.
    pub fn reconnect_active_workspace(&mut self) -> Result<AttemptToken> {
        let _ = self.disconnect_active_workspace();
        self.connect_from_draft_async(None)
    }

    /// Retries a failed connection from `ConnectionUnavailableView`.
    pub fn retry_unavailable_connection(&mut self) -> Result<AttemptToken> {
        self.connections.disconnect(self.active_workspace);
        self.connect_from_draft_async(None)
    }

    /// Switches to `ConnectionsLibrary` to edit a failed connection.
    pub fn edit_unavailable_connection(&mut self) {
        self.screen_mode = ScreenMode::ConnectionsLibrary;
    }

    /// Tests `connection_form` without opening a persistent workspace session.
    pub fn test_connection_blocking(&mut self, password: Option<SecretString>) -> Result<()> {
        let services = self.require_services()?.clone();
        let request = build_connect_request(
            &services.runtime,
            &self.connection_form,
            password,
            self.active_safe_mode(),
        )?;
        let prompter = Arc::new(DialogCredentialPrompter::new(self.headless));
        let cancel = tokio_util::sync::CancellationToken::new();
        let res = services.runtime.block_on(test_connection(
            &services.runtime,
            request,
            CredentialInteraction::Attended(prompter),
            cancel,
        ));
        match res {
            Ok(()) => {
                self.last_test_status = Some("Connection succeeded".to_owned());
                Ok(())
            }
            Err(err) => {
                self.last_test_status = Some(format!("Connection failed: {err}"));
                Err(AppError::Service(err))
            }
        }
    }

    /// Loads schemas and objects for the active workspace and updates `sidebar_rows`.
    pub fn refresh_schema_sidebar_blocking(&mut self) -> Result<()> {
        let services = self.require_services()?.clone();
        let ws = self.active_workspace;
        let (schemas, objects) = services.runtime.block_on(async {
            let guard = services.sessions.read().await;
            let schemas = guard.list_schemas(ws).await?;
            let scope = schemas
                .first()
                .map_or(ObjectScope::DefaultSchema, |s| ObjectScope::Schema(s.name.clone()));
            let objects = guard.list_objects(ws, scope).await?;
            Ok::<_, dataforge_services::ServiceError>((schemas, objects))
        })?;

        self.cached_schemas = schemas.to_vec();
        self.cached_objects = objects.to_vec();
        self.rebuild_sidebar_rows();
        Ok(())
    }

    /// Sets `sidebar_filter` and recomputes visible `sidebar_rows`.
    pub fn set_sidebar_filter(&mut self, filter: &str) {
        self.sidebar_filter = filter.to_owned();
        self.rebuild_sidebar_rows();
    }

    /// Toggles favorite status for an object in the schema sidebar.
    pub fn toggle_object_favorite(&mut self, object_name: &str) {
        if !self.favorite_objects.remove(object_name) {
            self.favorite_objects.insert(object_name.to_owned());
        }
        self.rebuild_sidebar_rows();
    }

    /// Rebuilds `sidebar_rows` from `cached_objects`, `sidebar_filter`, and `sidebar_favorites_only`.
    pub fn rebuild_sidebar_rows(&mut self) {
        let filter_lower = self.sidebar_filter.trim().to_ascii_lowercase();
        let matches = |obj: &&ObjectInfo| -> bool {
            let name_ok = filter_lower.is_empty()
                || obj.name.to_ascii_lowercase().contains(&filter_lower);
            let fav_ok = !self.sidebar_favorites_only || self.favorite_objects.contains(&obj.name);
            name_ok && fav_ok
        };

        let tables: Vec<&ObjectInfo> = self
            .cached_objects
            .iter()
            .filter(|o| matches!(o.kind, ObjectKind::Table) && matches(o))
            .collect();
        let views: Vec<&ObjectInfo> = self
            .cached_objects
            .iter()
            .filter(|o| matches!(o.kind, ObjectKind::View) && matches(o))
            .collect();
        let functions: Vec<&ObjectInfo> = self
            .cached_objects
            .iter()
            .filter(|o| {
                !matches!(o.kind, ObjectKind::Table | ObjectKind::View) && matches(o)
            })
            .collect();

        let mut rows = Vec::new();
        push_sidebar_section(&mut rows, SidebarSectionKind::Tables, self.strings.tables, &tables, &self.favorite_objects);
        push_sidebar_section(&mut rows, SidebarSectionKind::Views, self.strings.views, &views, &self.favorite_objects);
        push_sidebar_section(&mut rows, SidebarSectionKind::Functions, self.strings.functions, &functions, &self.favorite_objects);
        self.sidebar_rows = rows;
        self.sync_layout_counts();
    }

    /// Loads column metadata for `table` into `structure_columns`.
    pub fn load_table_structure_blocking(&mut self, table: &TableRef) -> Result<()> {
        let services = self.require_services()?.clone();
        let ws = self.active_workspace;
        let columns = services.runtime.block_on(async {
            let guard = services.sessions.read().await;
            let session = guard
                .session(ws)
                .ok_or_else(|| dataforge_services::ServiceError::Session("no active session".to_owned()))?;
            session
                .columns(table.clone())
                .await
                .map_err(|err| dataforge_services::ServiceError::Session(err.to_string()))
        })?;

        self.structure_columns = columns
            .iter()
            .map(|c| StructureColumnRow {
                name: c.name.clone(),
                data_type: format!("{:?}", c.column_type),
                nullable: c.nullable,
                default_expr: Some(format!("{:?}", c.default)),
                is_primary_key: c.primary_key,
            })
            .collect();
        Ok(())
    }

    /// Drains and applies all queued [`ServiceEvent`]s from background tasks.
    pub fn drain_service_events(&mut self) {
        let Some(services) = self.services.clone() else {
            return;
        };
        for event in services.drain_events() {
            match event {
                ServiceEvent::Connected {
                    workspace_id,
                    token,
                    label,
                    engine,
                } => {
                    if services.is_attempt_current(workspace_id, token) {
                        self.apply_connected(workspace_id, token, &label, engine);
                    }
                }
                ServiceEvent::ConnectFailed {
                    workspace_id,
                    token,
                    summary,
                    is_auth_failure,
                } => {
                    if services.is_attempt_current(workspace_id, token) {
                        self.apply_connect_failed(workspace_id, token, summary, is_auth_failure);
                    }
                }
                ServiceEvent::ConnectionTested { message, .. } => {
                    self.last_test_status = Some(message);
                }
                ServiceEvent::SchemasLoaded { schemas, .. } => {
                    self.cached_schemas = schemas.to_vec();
                }
                ServiceEvent::ObjectsLoaded { objects, .. } => {
                    self.cached_objects = objects.to_vec();
                    self.rebuild_sidebar_rows();
                }
                ServiceEvent::TableStructureLoaded { columns, .. } => {
                    self.structure_columns = columns
                        .into_iter()
                        .map(|c| StructureColumnRow {
                            name: c.name,
                            data_type: format!("{:?}", c.column_type),
                            nullable: c.nullable,
                            default_expr: Some(format!("{:?}", c.default)),
                            is_primary_key: c.primary_key,
                        })
                        .collect();
                }
                ServiceEvent::TableQueried { outcome, .. } => {
                    self.apply_table_query_outcome(outcome);
                }
                ServiceEvent::TableRowCounted { count, .. } => {
                    self.apply_table_row_count(count);
                }
                ServiceEvent::TableEditsCommitted { .. } => {
                    self.clear_active_tab_unsaved();
                }
                ServiceEvent::QueryStreamEvent { event, .. } => {
                    self.apply_query_stream_event(event);
                }
                ServiceEvent::QueryFinished {
                    summary,
                    elapsed_ms,
                    ..
                } => {
                    self.apply_query_finished(summary, elapsed_ms);
                }
                ServiceEvent::AiReplyEvent { event } => {
                    self.apply_ai_reply_event(event);
                }
                ServiceEvent::AiFinished { error } => {
                    if let Some(err) = error {
                        self.ai_drawer.messages.push(crate::views::ai_drawer::AiMessage {
                            is_user: false,
                            text: format!("Error: {err}"),
                        });
                    }
                }
            }
        }
    }

    fn apply_connected(
        &mut self,
        ws: dataforge_state::connection_window::WorkspaceId,
        token: AttemptToken,
        label: &str,
        engine: EngineKind,
    ) {
        let _ = self.connections.complete_connect(ws, token);
        self.active_attempt = None;
        let conn_id = format!("ws-{}", ws.0);
        self.add_rail_connection(&conn_id, label, engine, true);
        self.screen_mode = ScreenMode::Workspace;
    }

    fn apply_connect_failed(
        &mut self,
        ws: dataforge_state::connection_window::WorkspaceId,
        token: AttemptToken,
        summary: String,
        is_auth_failure: bool,
    ) {
        let _ = self.connections.fail_connect(
            ws,
            token,
            ConnectionUnavailableView {
                summary,
                detail: None,
                can_retry: true,
                is_auth_failure,
            },
        );
        self.active_attempt = None;
    }
}

fn push_sidebar_section(
    rows: &mut Vec<SidebarRow>,
    kind: SidebarSectionKind,
    title: &str,
    items: &[&ObjectInfo],
    favorites: &std::collections::BTreeSet<String>,
) {
    rows.push(SidebarRow::SectionHeader {
        kind,
        title: title.to_owned(),
        expanded: true,
        count: items.len(),
    });
    for obj in items {
        rows.push(SidebarRow::ObjectItem {
            kind,
            name: obj.name.clone(),
            is_favorite: favorites.contains(&obj.name),
            selected: false,
        });
    }
}
