//! End-to-end and engine integration test harness for DataForge.

use std::path::Path;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use dataforge_state::editor::RunEventAccumulator;
use tablepro_core::credentials::CredentialInteraction;
use tablepro_core::{
    ConnectEnvironment, DatabaseDriver, EditorSession, EditorSessionOptions, FileEndpoint,
    LivenessPolicy, ResultEvent, ResultSink, RunOptions, RunSummary, SafeModeLevel, SinkFlow,
    UserSql, WorkspaceSettings,
};
use tablepro_session::runtime::Tasks;
use tablepro_session::{
    ConnectRequest, ConnectServices, ConnectionMetadata, Connector, DefaultDeadlines,
    DriverConnector, NoStoredCredentials, RequestTarget, Session, SessionDeps,
};
use tablepro_ssh::InProcessSsh;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

/// Shared result type for E2E tests (avoids `unwrap`/`expect` per workspace lints).
pub type E2eResult<T> = Result<T, Box<dyn std::error::Error + Send + Sync>>;

/// Opens or creates a file-backed database session (SQLite or DuckDB) through `DriverConnector`.
pub async fn open_file_session(
    _runtime_dir: &Path,
    db_file: &Path,
    driver: Arc<dyn DatabaseDriver>,
    driver_id: &str,
    create: bool,
    read_only: bool,
) -> E2eResult<Arc<Session>> {
    let connector = Arc::new(DriverConnector::new(
        ConnectServices {
            environment: ConnectEnvironment::new("dataforge-e2e"),
            ssh: Arc::new(InProcessSsh::system()),
            liveness: LivenessPolicy::DESKTOP,
        },
        Arc::new(DefaultDeadlines::new(LivenessPolicy::DESKTOP)),
    ));

    let endpoint = if create {
        FileEndpoint::create_new(db_file.to_owned())?
    } else {
        FileEndpoint::open_existing(db_file.to_owned())?
    };

    let request = ConnectRequest {
        metadata: ConnectionMetadata {
            id: Uuid::new_v4(),
            name: format!("e2e-{driver_id}"),
            driver_id: driver_id.to_owned(),
        },
        driver,
        target: RequestTarget::File(endpoint),
        workspace_settings: WorkspaceSettings::workspace(read_only),
        safe_mode: if read_only {
            SafeModeLevel::ReadOnly
        } else {
            SafeModeLevel::Off
        },
    };

    let pending = connector
        .establish(
            request,
            CredentialInteraction::Unattended,
            CancellationToken::new(),
        )
        .await?;

    let session = Session::start(
        pending,
        SessionDeps {
            connector,
            credentials: Arc::new(NoStoredCredentials),
            deadlines: Arc::new(DefaultDeadlines::new(LivenessPolicy::DESKTOP)),
            tasks: Tasks::new(tokio::runtime::Handle::current()),
            prompter: None,
        },
    );

    Ok(session)
}

/// Sink that collects every [`ResultEvent`] for inspection in tests.
#[derive(Clone, Default)]
pub struct CollectingSink {
    events: Arc<Mutex<Vec<ResultEvent>>>,
}

impl CollectingSink {
    /// Creates an empty event sink.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns a snapshot of all collected events.
    pub fn events(&self) -> E2eResult<Vec<ResultEvent>> {
        let guard = self
            .events
            .lock()
            .map_err(|err| format!("collecting sink lock poisoned: {err}"))?;
        Ok(guard.clone())
    }
}

#[async_trait]
impl ResultSink for CollectingSink {
    async fn accept(&mut self, event: ResultEvent) -> SinkFlow {
        if let Ok(mut guard) = self.events.lock() {
            guard.push(event);
        }
        SinkFlow::Continue
    }
}

/// Output of running SQL through an [`EditorSession`].
pub struct SqlRunOutput {
    pub events: Vec<ResultEvent>,
    pub summary: RunSummary,
    pub accumulator: RunEventAccumulator,
}

/// Executes SQL on a newly opened editor session and feeds all events into [`RunEventAccumulator`].
pub async fn run_sql(session: &Session, sql: &str) -> E2eResult<SqlRunOutput> {
    let editor = session
        .open_editor_session(EditorSessionOptions::default(), &CancellationToken::new())
        .await?;
    run_sql_on_editor(editor.as_ref(), sql).await
}

/// Executes SQL on an existing [`EditorSession`] and feeds all events into [`RunEventAccumulator`].
pub async fn run_sql_on_editor(editor: &dyn EditorSession, sql: &str) -> E2eResult<SqlRunOutput> {
    let sink = CollectingSink::new();
    let summary = editor
        .run(
            UserSql::new(sql.to_owned()),
            RunOptions::new(CancellationToken::new()),
            Box::new(sink.clone()),
        )
        .await?;
    let events = sink.events()?;
    let mut accumulator = RunEventAccumulator::new();
    for event in &events {
        accumulator.push_event(event.clone());
    }
    accumulator.finish(summary.clone());
    Ok(SqlRunOutput {
        events,
        summary,
        accumulator,
    })
}

/// Gracefully shuts down a [`Session`].
pub async fn shutdown_session(session: &Session) -> E2eResult<()> {
    let _report = session.begin_shutdown().await?;
    Ok(())
}
