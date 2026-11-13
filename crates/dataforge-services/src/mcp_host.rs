//! MCP host implementation, local MCP server lifecycle, and CLI pre-dispatch helper.

use std::ffi::OsString;
use std::net::SocketAddr;
use std::sync::Arc;

use async_trait::async_trait;
use futures::future::BoxFuture;
use tablepro_cli::{Dispatch, classify};
use tablepro_core::dialect::SqlDialect;
use tablepro_mcp::{
    AccessToken, DEFAULT_PORT, McpHost, McpServer, OpenOutcome, SeenClient, ToolContext,
    WriteApproval,
};
use tablepro_session::Session;
use tablepro_storage::query_history::NewEntry;
use tablepro_storage::{ConnectionStore, SavedConnection};
use tokio::sync::{Mutex, RwLock, watch};
use uuid::Uuid;

use crate::error::ServiceError;
use crate::runtime::AppRuntime;
use crate::session_bridge::SessionManager;

/// Async callback invoked when an MCP client requests approval for a write statement.
pub type WriteApprovalHandler =
    Arc<dyn Fn(WriteApproval) -> BoxFuture<'static, bool> + Send + Sync>;

/// Async callback invoked when an MCP client asks to open a saved connection.
pub type OpenConnectionHandler =
    Arc<dyn Fn(SavedConnection) -> BoxFuture<'static, OpenOutcome> + Send + Sync>;

/// Callback invoked to record an MCP-executed query in query history.
pub type QueryHistoryRecorder = Arc<dyn Fn(NewEntry, Arc<dyn SqlDialect>) + Send + Sync>;

/// Pre-dispatch helper classifying process arguments before launching the GUI.
#[must_use]
pub fn classify_cli_args(args: &[OsString]) -> Dispatch {
    classify(args)
}

/// Host adapter bridging `tablepro_mcp::McpHost` to [`SessionManager`] and UI callbacks.
pub struct DataForgeMcpHost {
    sessions: Arc<RwLock<SessionManager>>,
    approval_handler: Option<WriteApprovalHandler>,
    open_handler: Option<OpenConnectionHandler>,
    history_recorder: Option<QueryHistoryRecorder>,
    recorded_history: Mutex<Vec<NewEntry>>,
}

impl DataForgeMcpHost {
    /// Creates a new [`DataForgeMcpHost`] backed by `sessions`.
    #[must_use]
    pub fn new(sessions: Arc<RwLock<SessionManager>>) -> Self {
        Self {
            sessions,
            approval_handler: None,
            open_handler: None,
            history_recorder: None,
            recorded_history: Mutex::new(Vec::new()),
        }
    }

    /// Sets the callback used to ask the user for write approval.
    #[must_use]
    pub fn with_approval_handler(mut self, handler: WriteApprovalHandler) -> Self {
        self.approval_handler = Some(handler);
        self
    }

    /// Sets the callback used to open a saved connection on behalf of an MCP client.
    #[must_use]
    pub fn with_open_handler(mut self, handler: OpenConnectionHandler) -> Self {
        self.open_handler = Some(handler);
        self
    }

    /// Sets the callback used to persist executed MCP queries to history.
    #[must_use]
    pub fn with_history_recorder(mut self, recorder: QueryHistoryRecorder) -> Self {
        self.history_recorder = Some(recorder);
        self
    }

    /// Returns a snapshot of queries recorded when no external history recorder is set.
    pub async fn recorded_history(&self) -> Vec<NewEntry> {
        self.recorded_history.lock().await.clone()
    }
}

#[async_trait]
impl McpHost for DataForgeMcpHost {
    async fn session(&self, id: Uuid) -> Option<Arc<Session>> {
        let guard = self.sessions.read().await;
        guard.session_by_id(id)
    }

    async fn open_connections(&self) -> Vec<Uuid> {
        let guard = self.sessions.read().await;
        guard.open_connection_ids()
    }

    async fn open(&self, connection: SavedConnection) -> OpenOutcome {
        if let Some(handler) = &self.open_handler {
            return handler(connection).await;
        }
        let guard = self.sessions.read().await;
        if let Some(session) = guard.session_by_id(connection.id) {
            OpenOutcome::Open(session)
        } else {
            OpenOutcome::Closed
        }
    }

    async fn approve(&self, approval: WriteApproval) -> bool {
        if let Some(handler) = &self.approval_handler {
            handler(approval).await
        } else {
            false
        }
    }

    async fn record(&self, entry: NewEntry, dialect: Arc<dyn SqlDialect>) {
        if let Some(recorder) = &self.history_recorder {
            recorder(entry.clone(), dialect);
        }
        self.recorded_history.lock().await.push(entry);
    }
}

/// Manages the lifecycle of the local loopback [`McpServer`].
pub struct McpServerController {
    server: Option<McpServer>,
    port: u16,
}

impl McpServerController {
    /// Creates a new stopped [`McpServerController`] configured for `port` (defaults to [`DEFAULT_PORT`]).
    #[must_use]
    pub fn new(port: Option<u16>) -> Self {
        Self {
            server: None,
            port: port.unwrap_or(DEFAULT_PORT),
        }
    }

    /// Starts the local MCP server on `127.0.0.1`.
    ///
    /// # Errors
    ///
    /// Returns [`ServiceError::Mcp`] if the server is already running or binding fails.
    pub async fn start(
        &mut self,
        runtime: &AppRuntime,
        host: Arc<dyn McpHost>,
        connections: ConnectionStore,
        token: AccessToken,
    ) -> Result<SocketAddr, ServiceError> {
        if self.server.is_some() {
            return Err(ServiceError::Mcp(
                "MCP server is already running".to_owned(),
            ));
        }
        let context = ToolContext { host, connections };
        let server = McpServer::start(
            self.port,
            token,
            context,
            env!("CARGO_PKG_VERSION"),
            runtime.tasks(),
        )
        .await
        .map_err(|err| ServiceError::Mcp(err.to_string()))?;
        let addr = server.address();
        self.server = Some(server);
        Ok(addr)
    }

    /// Stops the local MCP server if running.
    pub fn stop(&mut self) {
        self.server = None;
    }

    /// Returns whether the MCP server is currently running.
    #[must_use]
    pub fn is_running(&self) -> bool {
        self.server.is_some()
    }

    /// Returns the bound loopback address, if running.
    #[must_use]
    pub fn address(&self) -> Option<SocketAddr> {
        self.server.as_ref().map(McpServer::address)
    }

    /// Subscribes to seen MCP clients if the server is running.
    #[must_use]
    pub fn seen_clients(&self) -> Option<watch::Receiver<Vec<SeenClient>>> {
        self.server.as_ref().map(McpServer::seen_clients)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio_util::sync::CancellationToken;

    #[test]
    fn classify_cli_args_routes_subcommands_and_gui() {
        let gui_args = vec![OsString::from("dataforge")];
        assert!(matches!(classify_cli_args(&gui_args), Dispatch::Gui(_)));

        let help_args = vec![OsString::from("dataforge"), OsString::from("--help")];
        assert_eq!(classify_cli_args(&help_args), Dispatch::Command);

        let url_args = vec![
            OsString::from("dataforge"),
            OsString::from("postgres://localhost/db"),
        ];
        assert!(matches!(classify_cli_args(&url_args), Dispatch::Gui(_)));
    }

    #[test]
    fn mcp_host_defaults_to_closed_and_declines_writes() -> Result<(), ServiceError> {
        let runtime = Arc::new(AppRuntime::new(Some(1))?);
        let sessions = Arc::new(RwLock::new(SessionManager::new(runtime.clone())));
        let host = DataForgeMcpHost::new(sessions);

        runtime.block_on(async {
            assert!(host.open_connections().await.is_empty());
            assert!(host.session(Uuid::new_v4()).await.is_none());

            let approval = WriteApproval {
                connection_id: Uuid::new_v4(),
                connection_name: "test".to_owned(),
                database: None,
                sql: "DELETE FROM users".to_owned(),
                destructive: true,
                cancel: CancellationToken::new(),
            };
            assert!(!host.approve(approval).await);
        });
        Ok(())
    }

    #[test]
    fn mcp_host_uses_custom_approval_handler() -> Result<(), ServiceError> {
        let runtime = Arc::new(AppRuntime::new(Some(1))?);
        let sessions = Arc::new(RwLock::new(SessionManager::new(runtime.clone())));
        let host = DataForgeMcpHost::new(sessions).with_approval_handler(Arc::new(|approval| {
            Box::pin(async move { !approval.destructive })
        }));

        runtime.block_on(async {
            let safe_write = WriteApproval {
                connection_id: Uuid::new_v4(),
                connection_name: "test".to_owned(),
                database: None,
                sql: "INSERT INTO logs VALUES (1)".to_owned(),
                destructive: false,
                cancel: CancellationToken::new(),
            };
            assert!(host.approve(safe_write).await);
        });
        Ok(())
    }
}
