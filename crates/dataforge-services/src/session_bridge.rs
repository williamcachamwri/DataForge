//! Session manager bridging `dataforge-state` models to `tablepro_session::Session`.

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use async_trait::async_trait;
use dataforge_state::connection_window::{
    AttemptToken, ConnectionUnavailableView, ConnectionWindowPhaseMachine, WorkspaceId,
};
use dataforge_state::data_grid::{DataGridModel, RowID, SortDirection};
use dataforge_state::tabs::TabStripModel;
use tablepro_core::browse::{
    BrowsePage, PageRequest, PageSpec, RowKeyLayout, SortKey, build_count, read_page, split_page,
};
use tablepro_core::column::ColumnInfo;
use tablepro_core::credentials::{CredentialInteraction, CredentialPrompter};
use tablepro_core::dialect::SqlDialect;
use tablepro_core::dml::{build_delete, build_insert, build_update};
use tablepro_core::edit::CellInput;
use tablepro_core::filter::FilterSet;
use tablepro_core::meta::{
    ObjectInfo, ObjectScope, PagePosition, RowIdentity, SchemaInfo, TableRef,
};
use tablepro_core::statement::{GuardedStatement, RowGuard, StatementBuilder};
use tablepro_core::value::Value;
use tablepro_core::{
    DriverError, EditorSessionOptions, ForeignKeyMode, ResultEvent, ResultSink, RunOptions,
    RunSummary, SinkFlow, UserSql, WriteBatch, WriteKind, WriteReport,
};
use tablepro_session::connect::{ConnectRequest, Connector, CredentialSource};
use tablepro_session::operation::{OperationError, OperationKind};
use tablepro_session::{Session, SessionDeps};
use tablepro_storage::CredentialDemand;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::error::ServiceError;
use crate::runtime::AppRuntime;

/// Result of a paged [`SessionManager::table_query`] call.
#[derive(Debug, Clone)]
pub struct TableQueryOutcome {
    pub page: BrowsePage,
    pub previous: Option<PagePosition>,
    pub next: Option<PagePosition>,
    pub row_keys: BTreeMap<RowID, Vec<Value>>,
}

/// [`ResultSink`] implementation that forwards streaming [`ResultEvent`]s over an MPSC channel.
pub struct ChannelResultSink {
    tx: mpsc::Sender<ResultEvent>,
}

impl ChannelResultSink {
    /// Creates a new sink forwarding events to `tx`.
    #[must_use]
    pub fn new(tx: mpsc::Sender<ResultEvent>) -> Self {
        Self { tx }
    }
}

#[async_trait]
impl ResultSink for ChannelResultSink {
    async fn accept(&mut self, event: ResultEvent) -> SinkFlow {
        if self.tx.send(event).await.is_ok() {
            SinkFlow::Continue
        } else {
            SinkFlow::ConsumerGone
        }
    }
}

/// Coordinates active [`Session`]s, connection phase machines, and per-workspace tab strips.
pub struct SessionManager {
    runtime: Arc<AppRuntime>,
    sessions: HashMap<WorkspaceId, Arc<Session>>,
    phase_machine: ConnectionWindowPhaseMachine,
    tab_strips: HashMap<WorkspaceId, TabStripModel>,
}

impl SessionManager {
    /// Creates an empty [`SessionManager`] backed by `runtime`.
    #[must_use]
    pub fn new(runtime: Arc<AppRuntime>) -> Self {
        Self {
            runtime,
            sessions: HashMap::new(),
            phase_machine: ConnectionWindowPhaseMachine::new(),
            tab_strips: HashMap::new(),
        }
    }

    /// Adopts an already-established [`Session`] for `workspace_id`.
    pub fn adopt_session(&mut self, workspace_id: WorkspaceId, session: Arc<Session>) {
        if let Some(token) = self.phase_machine.begin_connect(workspace_id) {
            let _ = self.phase_machine.complete_connect(workspace_id, token);
        }
        self.sessions.insert(workspace_id, session);
        self.tab_strips.entry(workspace_id).or_default();
    }

    /// Establishes a new connection for `workspace_id` and transitions its phase machine.
    ///
    /// # Errors
    ///
    /// Returns [`ServiceError::Session`] if credential resolution or driver connection fails.
    pub async fn connect_workspace(
        &mut self,
        workspace_id: WorkspaceId,
        request: ConnectRequest,
        credentials: Arc<dyn CredentialSource>,
        prompter: Arc<dyn CredentialPrompter>,
        cancel: CancellationToken,
    ) -> Result<AttemptToken, ServiceError> {
        let token = self
            .phase_machine
            .begin_connect(workspace_id)
            .ok_or_else(|| {
                ServiceError::Session(format!("workspace {workspace_id:?} is already connecting"))
            })?;

        let connector = self.runtime.connector();
        let refreshed = credentials
            .refresh(
                request,
                CredentialInteraction::Attended(prompter.clone()),
                CredentialDemand::default(),
            )
            .await
            .map_err(|err| {
                self.mark_unavailable(workspace_id, token, err.to_string(), true);
                ServiceError::Session(err.to_string())
            })?;

        let pending = connector
            .establish(
                refreshed.request,
                CredentialInteraction::Attended(prompter),
                cancel,
            )
            .await
            .map_err(|err| {
                self.mark_unavailable(workspace_id, token, err.to_string(), false);
                ServiceError::Session(err.to_string())
            })?;

        let deps = SessionDeps {
            connector,
            credentials,
            prompter: None,
            deadlines: self.runtime.deadlines().clone(),
            tasks: self.runtime.tasks().clone(),
        };
        let session = Session::start(pending, deps);
        let _ = self.phase_machine.complete_connect(workspace_id, token);
        self.sessions.insert(workspace_id, session);
        self.tab_strips.entry(workspace_id).or_default();
        Ok(token)
    }

    fn mark_unavailable(
        &mut self,
        workspace_id: WorkspaceId,
        token: AttemptToken,
        summary: String,
        is_auth_failure: bool,
    ) {
        let _ = self.phase_machine.fail_connect(
            workspace_id,
            token,
            ConnectionUnavailableView {
                summary,
                detail: None,
                can_retry: true,
                is_auth_failure,
            },
        );
    }

    /// Shuts down and removes the session for `workspace_id`.
    ///
    /// # Errors
    ///
    /// Returns [`ServiceError::Session`] if no session is registered for `workspace_id`.
    pub fn disconnect_workspace(&mut self, workspace_id: WorkspaceId) -> Result<(), ServiceError> {
        let session = self
            .sessions
            .remove(&workspace_id)
            .ok_or_else(|| ServiceError::Session(format!("unknown workspace {workspace_id:?}")))?;
        drop(session.begin_shutdown());
        self.phase_machine.disconnect(workspace_id);
        Ok(())
    }

    /// Returns the active [`Session`] for `workspace_id`, if connected.
    #[must_use]
    pub fn session(&self, workspace_id: WorkspaceId) -> Option<Arc<Session>> {
        self.sessions.get(&workspace_id).cloned()
    }

    /// Finds an active [`Session`] by its connection [`Uuid`].
    #[must_use]
    pub fn session_by_id(&self, id: Uuid) -> Option<Arc<Session>> {
        self.sessions.values().find(|s| s.id() == id).cloned()
    }

    /// Lists all currently open connection [`Uuid`]s.
    #[must_use]
    pub fn open_connection_ids(&self) -> Vec<Uuid> {
        self.sessions.values().map(|s| s.id()).collect()
    }

    /// Returns a reference to the [`ConnectionWindowPhaseMachine`].
    #[must_use]
    pub fn phase_machine(&self) -> &ConnectionWindowPhaseMachine {
        &self.phase_machine
    }

    /// Returns a mutable reference to the [`ConnectionWindowPhaseMachine`].
    pub fn phase_machine_mut(&mut self) -> &mut ConnectionWindowPhaseMachine {
        &mut self.phase_machine
    }

    /// Returns the [`TabStripModel`] for `workspace_id`, if present.
    #[must_use]
    pub fn tab_strip(&self, workspace_id: WorkspaceId) -> Option<&TabStripModel> {
        self.tab_strips.get(&workspace_id)
    }

    /// Returns a mutable [`TabStripModel`] for `workspace_id`, creating one if absent.
    pub fn tab_strip_mut(&mut self, workspace_id: WorkspaceId) -> &mut TabStripModel {
        self.tab_strips.entry(workspace_id).or_default()
    }

    fn require_session(&self, workspace_id: WorkspaceId) -> Result<Arc<Session>, ServiceError> {
        self.sessions
            .get(&workspace_id)
            .cloned()
            .ok_or_else(|| ServiceError::Session(format!("no active session for {workspace_id:?}")))
    }

    /// Lists schemas from the session's [`SchemaCatalog`](tablepro_session::SchemaCatalog).
    ///
    /// # Errors
    ///
    /// Returns [`ServiceError::Session`] if the workspace is disconnected or the catalog read fails.
    pub async fn list_schemas(
        &self,
        workspace_id: WorkspaceId,
    ) -> Result<Arc<[SchemaInfo]>, ServiceError> {
        let session = self.require_session(workspace_id)?;
        session
            .schemas()
            .await
            .map_err(|err| ServiceError::Session(err.to_string()))
    }

    /// Lists database objects for `scope` from the session's [`SchemaCatalog`](tablepro_session::SchemaCatalog).
    ///
    /// # Errors
    ///
    /// Returns [`ServiceError::Session`] if the workspace is disconnected or the catalog read fails.
    pub async fn list_objects(
        &self,
        workspace_id: WorkspaceId,
        scope: ObjectScope,
    ) -> Result<Arc<[ObjectInfo]>, ServiceError> {
        let session = self.require_session(workspace_id)?;
        session
            .objects(scope)
            .await
            .map_err(|err| ServiceError::Session(err.to_string()))
    }

    /// Queries a page of `table` and loads it into `grid`.
    ///
    /// # Errors
    ///
    /// Returns [`ServiceError::Session`] if the query or metadata lookup fails.
    pub async fn table_query(
        &self,
        workspace_id: WorkspaceId,
        table: &TableRef,
        grid: &mut DataGridModel,
        position: &PagePosition,
        cancel: &CancellationToken,
    ) -> Result<TableQueryOutcome, ServiceError> {
        let session = self.require_session(workspace_id)?;
        let columns = session
            .columns(table.clone())
            .await
            .map_err(|err| ServiceError::Session(err.to_string()))?;

        let identity = session
            .run_cancellable(OperationKind::Metadata, cancel, |lease, call| {
                let table = table.clone();
                async move {
                    lease
                        .row_identity(&table, call)
                        .await
                        .map_err(OperationError::from)
                }
            })
            .await
            .map_err(|err| ServiceError::Session(err.to_string()))?;

        let sort_keys = build_sort_keys(grid, &columns);
        let filter = grid.filter_bar.filter_set.clone();
        let limit = grid.page.limit.max(1);
        let table_owned = table.clone();
        let pos_owned = position.clone();

        let page_read = session
            .run_cancellable(OperationKind::Page, cancel, |lease, call| {
                let table = table_owned.clone();
                let columns = columns.clone();
                let filter = filter.clone();
                let sort_keys = sort_keys.clone();
                let identity = identity.clone();
                let position = pos_owned.clone();
                async move {
                    let req = PageRequest {
                        table: &table,
                        columns: &columns,
                        filter: &filter,
                        sort: &sort_keys,
                        identity: &identity,
                        position: &position,
                        limit,
                    };
                    read_page::<BrowseOperationError>(&*lease, &req, &call)
                        .await
                        .map_err(BrowseOperationError::into_operation_error)
                }
            })
            .await
            .map_err(|err| ServiceError::Session(err.to_string()))?;

        let browse_page = split_page(
            page_read.result,
            &PageSpec {
                statement: StatementBuilder::new().finish(),
                layout: page_read.layout.clone(),
                row_key: build_row_key_layout(&identity, &columns),
                stability: tablepro_core::meta::PageStability::Stable,
                cost: tablepro_core::meta::PageCost::KeySeek,
            },
        )
        .map_err(|err| ServiceError::Session(err.to_string()))?;

        grid.reload_rows(
            browse_page.columns.to_vec(),
            browse_page.rows.clone(),
            grid.page.table_row_count,
        );
        let row_keys = map_row_keys(grid, &browse_page.keys);

        Ok(TableQueryOutcome {
            page: browse_page,
            previous: page_read.previous,
            next: page_read.next,
            row_keys,
        })
    }

    /// Counts rows in `table` matching `filter`.
    ///
    /// # Errors
    ///
    /// Returns [`ServiceError::Session`] if the count query fails.
    pub async fn table_row_count(
        &self,
        workspace_id: WorkspaceId,
        table: &TableRef,
        filter: &FilterSet,
        cancel: &CancellationToken,
    ) -> Result<u64, ServiceError> {
        let session = self.require_session(workspace_id)?;
        let columns = session
            .columns(table.clone())
            .await
            .map_err(|err| ServiceError::Session(err.to_string()))?;

        let dialect = session
            .current_dialect()
            .ok_or_else(|| ServiceError::Session("session is offline".to_owned()))?;
        let stmt = build_count(&*dialect, table, &columns, filter)
            .map_err(|err| ServiceError::Session(err.to_string()))?;

        session
            .run_cancellable(OperationKind::Count, cancel, |lease, call| {
                let stmt = stmt.clone();
                async move {
                    lease
                        .count_rows(&stmt, call)
                        .await
                        .map_err(OperationError::from)
                }
            })
            .await
            .map_err(|err| ServiceError::Session(err.to_string()))
    }

    /// Commits pending [`TableEdits`](dataforge_state::data_grid::TableEdits) in `grid` to the database.
    ///
    /// # Errors
    ///
    /// Returns [`ServiceError::Session`] if building DML statements or executing the write batch fails.
    pub async fn commit_table_edits(
        &self,
        workspace_id: WorkspaceId,
        table: &TableRef,
        grid: &mut DataGridModel,
        row_keys: &BTreeMap<RowID, Vec<Value>>,
        foreign_keys: ForeignKeyMode,
        cancel: &CancellationToken,
    ) -> Result<WriteReport, ServiceError> {
        if !grid.edits.has_pending_edits() {
            return Ok(WriteReport::default());
        }
        let session = self.require_session(workspace_id)?;
        let columns = session
            .columns(table.clone())
            .await
            .map_err(|err| ServiceError::Session(err.to_string()))?;

        let identity = session
            .run_cancellable(OperationKind::Metadata, cancel, |lease, call| {
                let table = table.clone();
                async move {
                    lease
                        .row_identity(&table, call)
                        .await
                        .map_err(OperationError::from)
                }
            })
            .await
            .map_err(|err| ServiceError::Session(err.to_string()))?;

        let dialect = session
            .current_dialect()
            .ok_or_else(|| ServiceError::Session("session is offline".to_owned()))?;

        let steps = build_write_steps(&*dialect, table, &columns, &identity, grid, row_keys)?;
        let batch = WriteBatch::new(WriteKind::RowChanges, table.clone(), steps, foreign_keys);
        let report = session
            .run_cancellable(
                OperationKind::Save(WriteKind::RowChanges),
                cancel,
                |lease, call| {
                    let batch = batch.clone();
                    async move {
                        lease
                            .write_batch(&batch, call)
                            .await
                            .map_err(OperationError::from)
                    }
                },
            )
            .await
            .map_err(|err| ServiceError::Session(err.to_string()))?;

        let _ = grid.edits.take_committed();
        Ok(report)
    }

    /// Discards all uncommitted [`TableEdits`](dataforge_state::data_grid::TableEdits) in `grid`.
    pub fn rollback_table_edits(grid: &mut DataGridModel) {
        grid.edits.rollback();
    }

    /// Executes `sql` in an editor session, streaming [`ResultEvent`]s to `tx`.
    ///
    /// # Errors
    ///
    /// Returns [`ServiceError::Session`] if opening the editor session or running the query fails.
    pub async fn run_query(
        &self,
        workspace_id: WorkspaceId,
        sql: &str,
        options: RunOptions,
        editor_options: EditorSessionOptions,
        tx: mpsc::Sender<ResultEvent>,
        cancel: &CancellationToken,
    ) -> Result<RunSummary, ServiceError> {
        let session = self.require_session(workspace_id)?;
        let editor = session
            .open_editor_session(editor_options, cancel)
            .await
            .map_err(|err| ServiceError::Session(err.to_string()))?;
        let sink = Box::new(ChannelResultSink::new(tx));
        let summary = editor
            .run(UserSql::new(sql), options, sink)
            .await
            .map_err(|err| ServiceError::Session(err.to_string()))?;
        let _ = editor.close().await;
        Ok(summary)
    }
}

fn build_sort_keys(grid: &DataGridModel, columns: &[ColumnInfo]) -> Vec<SortKey> {
    grid.sort
        .keys
        .iter()
        .filter_map(|k| {
            let idx = grid.columns.data_index(k.column_id)?;
            let _ = columns.get(idx)?;
            Some(SortKey {
                column: idx,
                ascending: matches!(k.direction, SortDirection::Ascending),
            })
        })
        .collect()
}

fn build_row_key_layout(identity: &RowIdentity, columns: &[ColumnInfo]) -> RowKeyLayout {
    match identity {
        RowIdentity::UniqueKey { columns: key_cols } => {
            let mut positions = Vec::with_capacity(key_cols.len());
            for name in key_cols {
                if let Some(pos) = columns.iter().position(|c| c.name == *name) {
                    positions.push(pos);
                }
            }
            RowKeyLayout::Columns(positions)
        }
        RowIdentity::EngineRowId(engine_id) => RowKeyLayout::EngineRowId {
            first_hidden: columns.len(),
            parts: engine_id.parts(),
        },
        RowIdentity::SortingKey { .. } | RowIdentity::Unordered => RowKeyLayout::ReadOnly,
    }
}

fn map_row_keys(grid: &DataGridModel, keys: &[Option<Vec<Value>>]) -> BTreeMap<RowID, Vec<Value>> {
    let mut map = BTreeMap::new();
    for (row_id, key_opt) in grid.row_order.as_slice().iter().zip(keys.iter()) {
        if let Some(key) = key_opt {
            map.insert(*row_id, key.clone());
        }
    }
    map
}

fn build_write_steps(
    dialect: &dyn SqlDialect,
    table: &TableRef,
    columns: &[ColumnInfo],
    identity: &RowIdentity,
    grid: &DataGridModel,
    row_keys: &BTreeMap<RowID, Vec<Value>>,
) -> Result<Vec<GuardedStatement>, ServiceError> {
    let mut steps = Vec::new();

    let mut updates_by_row: BTreeMap<RowID, Vec<(usize, CellInput)>> = BTreeMap::new();
    for ((row_id, col_id), value) in &grid.edits.modified_cells {
        if grid.edits.deleted_rows.contains(row_id) {
            continue;
        }
        let col_idx = grid.columns.data_index(*col_id).ok_or_else(|| {
            ServiceError::Session(format!("unknown column id {col_id:?} in pending edits"))
        })?;
        updates_by_row
            .entry(*row_id)
            .or_default()
            .push((col_idx, CellInput::Value(value.clone())));
    }

    for (row_id, assignments) in updates_by_row {
        let key = row_keys.get(&row_id).ok_or_else(|| {
            ServiceError::Session(format!("row {row_id:?} has no key for update"))
        })?;
        let stmt = build_update(dialect, table, columns, identity, key, &assignments, &[])
            .map_err(|err| ServiceError::Session(err.to_string()))?;
        steps.push(GuardedStatement {
            statement: stmt,
            guard: RowGuard::AffectedExactly(1),
        });
    }

    for values in grid.edits.inserted_rows.values() {
        let cells: Vec<CellInput> = values.iter().cloned().map(CellInput::Value).collect();
        let stmt = build_insert(dialect, table, columns, &cells)
            .map_err(|err| ServiceError::Session(err.to_string()))?;
        steps.push(GuardedStatement {
            statement: stmt,
            guard: RowGuard::AffectedExactly(1),
        });
    }

    for row_id in &grid.edits.deleted_rows {
        let key = row_keys.get(row_id).ok_or_else(|| {
            ServiceError::Session(format!("row {row_id:?} has no key for delete"))
        })?;
        let stmt = build_delete(dialect, table, columns, identity, key)
            .map_err(|err| ServiceError::Session(err.to_string()))?;
        steps.push(GuardedStatement {
            statement: stmt,
            guard: RowGuard::AffectedExactly(1),
        });
    }

    Ok(steps)
}

enum BrowseOperationError {
    Browse(tablepro_core::browse::BrowseSqlError),
    Driver(DriverError),
}

impl From<tablepro_core::browse::BrowseSqlError> for BrowseOperationError {
    fn from(err: tablepro_core::browse::BrowseSqlError) -> Self {
        Self::Browse(err)
    }
}

impl From<DriverError> for BrowseOperationError {
    fn from(err: DriverError) -> Self {
        Self::Driver(err)
    }
}

impl BrowseOperationError {
    fn into_operation_error(self) -> OperationError {
        match self {
            Self::Browse(err) => OperationError::Driver(DriverError::Protocol(err.to_string())),
            Self::Driver(err) => OperationError::Driver(err),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dataforge_state::connection_window::ConnectionWindowPhase;
    use dataforge_state::data_grid::{DataGridColumnID, DataGridLayoutScope};
    use dataforge_state::tabs::TabKind;
    use tablepro_core::column::{ColumnDefault, ColumnType, ResultColumn};

    #[tokio::test]
    async fn channel_result_sink_forwards_and_detects_closed_receiver() {
        let (tx, mut rx) = mpsc::channel(4);
        let mut sink = ChannelResultSink::new(tx);

        let flow = sink
            .accept(ResultEvent::CommandCompleted {
                rows_affected: Some(3),
            })
            .await;
        assert_eq!(flow, SinkFlow::Continue);
        assert!(matches!(
            rx.recv().await,
            Some(ResultEvent::CommandCompleted {
                rows_affected: Some(3)
            })
        ));

        drop(rx);
        let closed = sink
            .accept(ResultEvent::CommandCompleted {
                rows_affected: None,
            })
            .await;
        assert_eq!(closed, SinkFlow::ConsumerGone);
    }

    #[test]
    fn session_manager_manages_tabs_and_rollback() -> Result<(), ServiceError> {
        let runtime = Arc::new(AppRuntime::new(Some(1))?);
        let mut manager = SessionManager::new(runtime);
        let ws = WorkspaceId(1);

        assert_eq!(manager.phase_machine().phase(ws), None);
        assert!(manager.disconnect_workspace(ws).is_err());

        let strip = manager.tab_strip_mut(ws);
        let _tab = strip.open_query_tab(
            dataforge_state::tabs::TabConnectionId("local".to_owned()),
            Some("SELECT 1"),
        );
        assert_eq!(strip.tabs().len(), 1);
        assert!(matches!(strip.tabs()[0].kind, TabKind::QueryEditor { .. }));

        let mut grid = DataGridModel::new(DataGridLayoutScope::Ephemeral, 100);
        grid.reload_rows(
            vec![ResultColumn::new("id", ColumnType::untyped_text("INTEGER"))],
            vec![vec![Value::Int(1)]],
            Some(1),
        );
        grid.edits.insert_row(vec![Value::Int(2)]);
        assert!(grid.edits.has_pending_edits());
        SessionManager::rollback_table_edits(&mut grid);
        assert!(!grid.edits.has_pending_edits());
        let _ = DataGridColumnID(1);
        let _ = ConnectionWindowPhase::Disconnected;
        Ok(())
    }

    #[test]
    fn build_row_key_layout_maps_unique_key_and_unordered() {
        let cols = vec![
            ColumnInfo {
                name: "id".to_owned(),
                column_type: ColumnType::untyped_text("INTEGER"),
                nullable: false,
                primary_key: true,
                is_auto_increment: true,
                is_generated: false,
                partition_scoped: false,
                default: ColumnDefault::None,
                comment: None,
            },
            ColumnInfo {
                name: "title".to_owned(),
                column_type: ColumnType::untyped_text("TEXT"),
                nullable: false,
                primary_key: false,
                is_auto_increment: false,
                is_generated: false,
                partition_scoped: false,
                default: ColumnDefault::None,
                comment: None,
            },
        ];

        let pk = RowIdentity::UniqueKey {
            columns: vec!["id".to_owned()],
        };
        assert_eq!(
            build_row_key_layout(&pk, &cols),
            RowKeyLayout::Columns(vec![0])
        );
        assert_eq!(
            build_row_key_layout(&RowIdentity::Unordered, &cols),
            RowKeyLayout::ReadOnly
        );
    }
}
