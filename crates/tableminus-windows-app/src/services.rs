//! Service layer hub bridging `tableminus-services` (`AppRuntime`, `SessionManager`,
//! `AiAssistantService`, `TableMinusMcpHost`, `McpServerController`, export/import)
//! with `AppController` and `MainWindow`.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicIsize, Ordering};
use std::sync::{Arc, Mutex};

use futures::future::BoxFuture;
use secrecy::SecretString;
use tableminus_gfx::glyphs::EngineKind;
use tableminus_services::session_bridge::TableQueryOutcome;
use tableminus_services::{
    AiAssistantService, AiEndpointConfig, AppRuntime, McpServerController, ServiceError,
    SessionManager, TableMinusMcpHost,
};
use tableminus_state::connection_window::{AttemptToken, WorkspaceId};
use tableminus_state::library::LibraryConnection;
use tableminus_windows_platform::{
    AppSettings, DEFAULT_CREDENTIAL_PREFIX, WindowsCredentialStore, storage_paths,
};
use tablepro_ai::ReplyEvent;
use tablepro_core::column::ColumnInfo;
use tablepro_core::credentials::{CredentialPrompt, CredentialPrompter, PromptReply};
use tablepro_core::meta::{ObjectInfo, SchemaInfo, TableRef};
use tablepro_core::{
    Credentials, FileEndpoint, NetworkEndpoint, NetworkOptions, ResultEvent, RunSummary,
    SafeModeLevel, TlsConfig, TlsMode as CoreTlsMode, WorkspaceSettings,
};
use tablepro_mcp::AccessToken;
use tablepro_session::connect::{ConnectRequest, ConnectionMetadata, RequestTarget};
use tablepro_ssh::{SshAuth, SshConfig, SshDestination};
use tablepro_storage::{ConnectionStore, StoragePaths};
use tokio::sync::RwLock;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{PostMessageW, WM_APP};

use crate::dialogs::{DialogKind, DialogOutcome, show_native_dialog};
use crate::i18n::{Locale, Strings, init_locale};
use crate::ipc::WakeupSignal;
use crate::views::library::{AuthMode, ConnectionFormDraft, TlsMode};

/// Custom Win32 message posted when background service tasks push [`ServiceEvent`]s.
pub const WM_APP_SERVICE_EVENT: u32 = WM_APP + 2;

/// Events delivered from async Tokio tasks back to [`AppController`](crate::app::AppController).
#[derive(Debug)]
pub enum ServiceEvent {
    Connected {
        workspace_id: WorkspaceId,
        token: AttemptToken,
        label: String,
        engine: EngineKind,
    },
    ConnectFailed {
        workspace_id: WorkspaceId,
        token: AttemptToken,
        summary: String,
        is_auth_failure: bool,
    },
    ConnectionTested {
        success: bool,
        message: String,
    },
    SchemasLoaded {
        workspace_id: WorkspaceId,
        schemas: Arc<[SchemaInfo]>,
    },
    ObjectsLoaded {
        workspace_id: WorkspaceId,
        schema: Option<String>,
        objects: Arc<[ObjectInfo]>,
    },
    TableStructureLoaded {
        workspace_id: WorkspaceId,
        table: TableRef,
        columns: Vec<ColumnInfo>,
    },
    TableQueried {
        workspace_id: WorkspaceId,
        table: TableRef,
        outcome: TableQueryOutcome,
    },
    TableRowCounted {
        workspace_id: WorkspaceId,
        table: TableRef,
        count: u64,
    },
    TableEditsCommitted {
        workspace_id: WorkspaceId,
        affected: u64,
    },
    QueryStreamEvent {
        workspace_id: WorkspaceId,
        event: ResultEvent,
    },
    QueryFinished {
        workspace_id: WorkspaceId,
        summary: Result<RunSummary, String>,
        elapsed_ms: u64,
    },
    AiReplyEvent {
        event: ReplyEvent,
    },
    AiFinished {
        error: Option<String>,
    },
}

/// Resolves the active localized [`Strings`] catalog from persisted [`AppSettings`]
/// or OS/environment locale variables via [`init_locale`].
#[must_use]
pub fn resolve_active_strings() -> Strings {
    let preferred = storage_paths()
        .ok()
        .and_then(|paths| AppSettings::load(&AppSettings::file_path(&paths)).ok())
        .and_then(|settings| settings.locale);
    Strings::for_locale(init_locale(preferred.as_deref()))
}

/// [`CredentialPrompter`] backed by Win32 native confirmation dialogs (or headless auto-decline).
pub struct DialogCredentialPrompter {
    headless: bool,
    strings: Strings,
    custom_locale: bool,
}

impl DialogCredentialPrompter {
    /// Creates a prompter using the active persisted/environment locale;
    /// when `headless` is `true`, prompts decline safely without blocking UI.
    #[must_use]
    pub fn new(headless: bool) -> Self {
        Self {
            headless,
            strings: resolve_active_strings(),
            custom_locale: false,
        }
    }

    /// Creates a prompter pinned to an explicit [`Locale`].
    #[must_use]
    pub fn with_locale(headless: bool, locale: Locale) -> Self {
        Self {
            headless,
            strings: Strings::for_locale(locale),
            custom_locale: true,
        }
    }
}

fn format_credential_prompt_text(request: &CredentialPrompt, strings: &Strings) -> String {
    request
        .fields
        .first()
        .map(|f| f.label.clone())
        .unwrap_or_else(|| {
            strings
                .auth_for_target_template
                .replace("{target}", &request.target)
        })
}

impl CredentialPrompter for DialogCredentialPrompter {
    fn prompt(&self, request: CredentialPrompt) -> BoxFuture<'static, PromptReply> {
        let headless = self.headless;
        let pinned_strings = self.custom_locale.then(|| self.strings.clone());
        Box::pin(async move {
            if headless {
                return PromptReply::Cancelled;
            }
            let strings = pinned_strings.unwrap_or_else(resolve_active_strings);
            let prompt_text = format_credential_prompt_text(&request, &strings);
            let masked = request.fields.first().is_some_and(|f| f.secret);
            let kind = DialogKind::InteractiveAuth {
                prompt: prompt_text,
                masked,
            };
            match show_native_dialog(None, &kind, &strings) {
                DialogOutcome::Accepted(secret) => PromptReply::Submitted {
                    values: vec![SecretString::from(secret.unwrap_or_default())],
                    remember: request.offer_remember,
                },
                DialogOutcome::Rejected | DialogOutcome::Cancelled => PromptReply::Cancelled,
            }
        })
    }
}

/// Central hub owning all `tableminus-services` instances and async event delivery.
pub struct ServiceHub {
    pub runtime: Arc<AppRuntime>,
    pub sessions: Arc<RwLock<SessionManager>>,
    pub credentials: Arc<WindowsCredentialStore>,
    pub ai: Option<Arc<AiAssistantService>>,
    pub mcp_host: Arc<TableMinusMcpHost>,
    pub mcp_server: Mutex<McpServerController>,
    pub connection_store: ConnectionStore,
    pub ai_config: AiEndpointConfig,
    events: Arc<Mutex<Vec<ServiceEvent>>>,
    notify_hwnd: Arc<AtomicIsize>,
    wakeup: Arc<WakeupSignal>,
    active_cancels: Mutex<HashMap<WorkspaceId, (AttemptToken, CancellationToken)>>,
}

impl ServiceHub {
    /// Initializes the service hub with `AppRuntime` (all 9 drivers), `SessionManager`,
    /// `WindowsCredentialStore`, `AiAssistantService`, `TableMinusMcpHost`, and `ConnectionStore`.
    pub fn new(worker_threads: Option<usize>) -> Result<Self, ServiceError> {
        let runtime = Arc::new(AppRuntime::new(worker_threads)?);
        let sessions = Arc::new(RwLock::new(SessionManager::new(runtime.clone())));
        let credentials = Arc::new(WindowsCredentialStore::new(DEFAULT_CREDENTIAL_PREFIX));
        let ai = AiAssistantService::new(credentials.clone())
            .ok()
            .map(Arc::new);
        let mcp_host = Arc::new(TableMinusMcpHost::new(sessions.clone()));
        let mcp_server = Mutex::new(McpServerController::new(None));
        let paths = storage_paths().unwrap_or_else(|_| {
            StoragePaths::under(&std::env::temp_dir(), "TableMinus", "dev.tableminus.app")
        });
        let connection_store = ConnectionStore::new(&paths);

        Ok(Self {
            runtime,
            sessions,
            credentials,
            ai,
            mcp_host,
            mcp_server,
            connection_store,
            ai_config: AiEndpointConfig::ollama("llama3.2"),
            events: Arc::new(Mutex::new(Vec::new())),
            notify_hwnd: Arc::new(AtomicIsize::new(0)),
            wakeup: Arc::new(WakeupSignal::default()),
            active_cancels: Mutex::new(HashMap::new()),
        })
    }

    /// Returns the shared async wakeup sender for testing or manual nudges.
    #[must_use]
    pub fn notify_sender(&self) -> async_channel::Sender<()> {
        self.wakeup.sender()
    }

    /// Returns a clone of the shared async wakeup receiver.
    #[must_use]
    pub fn notify_receiver(&self) -> async_channel::Receiver<()> {
        self.wakeup.receiver()
    }

    /// Creates an independent coalescing receiver notified whenever [`ServiceEvent`]s arrive.
    #[must_use]
    pub fn subscribe(&self) -> async_channel::Receiver<()> {
        self.wakeup.subscribe()
    }

    /// Attaches the Win32 `HWND` so background tasks wake the message loop via `WM_APP_SERVICE_EVENT`.
    pub fn attach_hwnd(&self, hwnd: HWND) {
        self.notify_hwnd.store(hwnd.0 as isize, Ordering::Release);
    }

    /// Detaches the Win32 `HWND` on window destruction.
    pub fn detach_hwnd(&self) {
        self.notify_hwnd.store(0, Ordering::Release);
    }

    /// Pushes a [`ServiceEvent`] and wakes the attached window if any.
    pub fn push_event(&self, event: ServiceEvent) {
        if let Ok(mut guard) = self.events.lock() {
            guard.push(event);
        }
        self.wakeup.notify();
        let raw = self.notify_hwnd.load(Ordering::Acquire);
        if raw != 0 {
            let hwnd = HWND(raw as *mut std::ffi::c_void);
            // SAFETY: `hwnd` was registered by `MainWindow::create` and cleared on `WM_DESTROY`.
            unsafe {
                let _ = PostMessageW(Some(hwnd), WM_APP_SERVICE_EVENT, WPARAM(0), LPARAM(0));
            }
        }
    }

    /// Drains all pending [`ServiceEvent`]s queued by background tasks.
    #[must_use]
    pub fn drain_events(&self) -> Vec<ServiceEvent> {
        match self.events.lock() {
            Ok(mut guard) => std::mem::take(&mut *guard),
            Err(poisoned) => std::mem::take(&mut *poisoned.into_inner()),
        }
    }

    /// Registers a new [`CancellationToken`] for `(workspace_id, token)`, cancelling any prior attempt.
    pub fn register_attempt(
        &self,
        workspace_id: WorkspaceId,
        token: AttemptToken,
    ) -> CancellationToken {
        let cancel = CancellationToken::new();
        if let Ok(mut guard) = self.active_cancels.lock()
            && let Some((_, prev)) = guard.insert(workspace_id, (token, cancel.clone()))
        {
            prev.cancel();
        }
        cancel
    }

    /// Cancels any in-flight attempt for `workspace_id`.
    pub fn cancel_attempt(&self, workspace_id: WorkspaceId) {
        if let Ok(mut guard) = self.active_cancels.lock()
            && let Some((_, cancel)) = guard.remove(&workspace_id)
        {
            cancel.cancel();
        }
    }

    /// Returns `true` if `token` is still the active attempt for `workspace_id`.
    #[must_use]
    pub fn is_attempt_current(&self, workspace_id: WorkspaceId, token: AttemptToken) -> bool {
        self.active_cancels
            .lock()
            .ok()
            .and_then(|g| g.get(&workspace_id).map(|(t, _)| *t == token))
            .unwrap_or(false)
    }

    /// Starts or stops the local loopback MCP server.
    pub fn toggle_mcp_server(&self, enable: bool) -> Result<Option<String>, ServiceError> {
        let mut guard = self
            .mcp_server
            .lock()
            .map_err(|_| ServiceError::Mcp("MCP server lock poisoned".to_owned()))?;
        if enable {
            if guard.is_running() {
                return Ok(guard.address().map(|a| a.to_string()));
            }
            let token =
                AccessToken::generate().map_err(|err| ServiceError::Mcp(err.to_string()))?;
            let addr = self.runtime.block_on(guard.start(
                &self.runtime,
                self.mcp_host.clone(),
                self.connection_store.clone(),
                token,
            ))?;
            Ok(Some(addr.to_string()))
        } else {
            guard.stop();
            Ok(None)
        }
    }

    /// Returns whether the local MCP server is currently running.
    #[must_use]
    pub fn is_mcp_running(&self) -> bool {
        self.mcp_server
            .lock()
            .map(|g| g.is_running())
            .unwrap_or(false)
    }
}

/// Maps a [`EngineKind`] to its `tablepro-core` driver ID string.
#[must_use]
pub const fn driver_id_from_engine(engine: EngineKind) -> &'static str {
    match engine {
        EngineKind::Postgresql => "postgres",
        EngineKind::Mysql | EngineKind::Mariadb => "mysql",
        EngineKind::Sqlite => "sqlite",
        EngineKind::Duckdb => "duckdb",
        EngineKind::Mssql => "mssql",
        EngineKind::Clickhouse => "clickhouse",
        EngineKind::Cassandra | EngineKind::Scylladb => "cassandra",
        EngineKind::Mongodb => "mongodb",
        EngineKind::Redis => "redis",
    }
}

/// Builds a [`ConnectRequest`] from a [`ConnectionFormDraft`], supporting all 9 drivers,
/// SSH tunnels, TLS VerifyFull/VerifyCa, and MSSQL SSPI/NTLM integrated auth.
pub fn build_connect_request(
    runtime: &AppRuntime,
    draft: &ConnectionFormDraft,
    password: Option<SecretString>,
    safe_mode: SafeModeLevel,
) -> Result<ConnectRequest, ServiceError> {
    let driver_id = driver_id_from_engine(draft.engine);
    let driver = runtime
        .drivers()
        .get(driver_id)
        .ok_or_else(|| ServiceError::Session(format!("unregistered driver '{driver_id}'")))?
        .clone();

    let name = if draft.name.trim().is_empty() {
        format!("{} ({})", draft.engine.display_name(), draft.database)
    } else {
        draft.name.clone()
    };

    let target = if matches!(draft.engine, EngineKind::Sqlite | EngineKind::Duckdb) {
        let path = PathBuf::from(draft.database.trim());
        let abs = if path.is_absolute() {
            path
        } else {
            std::env::current_dir()
                .map_err(ServiceError::Io)?
                .join(path)
        };
        let endpoint = FileEndpoint::open_existing(abs)
            .map_err(|err| ServiceError::Session(err.to_string()))?;
        RequestTarget::File(endpoint)
    } else {
        let endpoint = NetworkEndpoint::new(&draft.host, draft.port.max(1))
            .map_err(|err| ServiceError::Session(err.to_string()))?;
        let credentials = match draft.auth_mode {
            AuthMode::SspiIntegrated => Credentials::Integrated,
            AuthMode::Password | AuthMode::Certificate => Credentials::Password {
                username: draft.username.clone(),
                password: password.unwrap_or_else(|| SecretString::from(String::new())),
            },
        };
        let tls = match draft.tls_mode {
            TlsMode::Disable => TlsConfig::disabled(),
            TlsMode::VerifyFull => TlsConfig::verify_full(),
            TlsMode::VerifyCa => {
                let mut cfg = TlsConfig::verify_full();
                cfg.mode = CoreTlsMode::VerifyFull;
                cfg
            }
            TlsMode::Require => {
                let mut cfg = TlsConfig::disabled();
                cfg.mode = CoreTlsMode::Require;
                cfg
            }
            TlsMode::Prefer => {
                let mut cfg = TlsConfig::disabled();
                cfg.mode = CoreTlsMode::Prefer;
                cfg
            }
        };
        let ssh = if draft.ssh_enabled && !draft.ssh_host.trim().is_empty() {
            let user = (!draft.ssh_user.trim().is_empty()).then(|| draft.ssh_user.clone());
            let dest = SshDestination::new(&draft.ssh_host, Some(draft.ssh_port.max(1)), user)
                .map_err(|err| ServiceError::Session(format!("{err:?}")))?;
            let auth = if draft.ssh_key_path.trim().is_empty() {
                SshAuth::Agent
            } else {
                SshAuth::PrivateKey {
                    path: Some(PathBuf::from(draft.ssh_key_path.trim())),
                    passphrase: None,
                }
            };
            Some(SshConfig {
                destination: dest,
                jump_hosts: Vec::new(),
                auth,
            })
        } else {
            None
        };
        let db = (!draft.database.trim().is_empty()).then(|| draft.database.clone());
        RequestTarget::Network {
            endpoint,
            ssh,
            credentials,
            database: db,
            tls,
            options: NetworkOptions::default(),
        }
    };

    Ok(ConnectRequest {
        metadata: ConnectionMetadata {
            id: Uuid::new_v4(),
            name,
            driver_id: driver_id.to_owned(),
        },
        driver,
        target,
        workspace_settings: WorkspaceSettings::workspace(false),
        safe_mode,
    })
}

/// Returns the standard TCP port for `engine` (`0` for embedded file engines).
#[must_use]
pub const fn default_port_for_engine(engine: EngineKind) -> u16 {
    match engine {
        EngineKind::Postgresql => 5432,
        EngineKind::Mysql | EngineKind::Mariadb => 3306,
        EngineKind::Mssql => 1433,
        EngineKind::Clickhouse => 8123,
        EngineKind::Cassandra | EngineKind::Scylladb => 9042,
        EngineKind::Mongodb => 27017,
        EngineKind::Redis => 6379,
        EngineKind::Sqlite | EngineKind::Duckdb => 0,
    }
}

/// Builds a [`ConnectRequest`] from a [`LibraryConnection`].
pub fn build_connect_request_from_library(
    runtime: &AppRuntime,
    conn: &LibraryConnection,
    safe_mode: SafeModeLevel,
) -> Result<ConnectRequest, ServiceError> {
    let engine = crate::views::library::engine_from_driver_id(&conn.driver_id);
    let draft = ConnectionFormDraft {
        name: conn.name.clone(),
        engine,
        host: conn.host.clone(),
        port: conn.port.unwrap_or_else(|| default_port_for_engine(engine)),
        database: conn.database.clone(),
        username: conn.username.clone(),
        ..ConnectionFormDraft::default()
    };
    build_connect_request(runtime, &draft, None, safe_mode)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn service_hub_wakes_async_subscribers_on_push_event() -> Result<(), ServiceError> {
        let hub = ServiceHub::new(Some(1))?;
        let rx = hub.subscribe();
        let shared = hub.notify_receiver();

        hub.push_event(ServiceEvent::AiFinished { error: None });
        assert_eq!(rx.try_recv(), Ok(()));
        assert_eq!(shared.try_recv(), Ok(()));
        assert_eq!(hub.drain_events().len(), 1);
        Ok(())
    }

    #[test]
    fn dialog_credential_prompter_uses_locale_and_formats_fallback_prompt() {
        use tablepro_core::credentials::PromptPurpose;

        for locale in Locale::ALL {
            let prompter = DialogCredentialPrompter::with_locale(true, locale);
            let expected = Strings::for_locale(locale);
            assert_eq!(
                prompter.strings.interactive_auth_title,
                expected.interactive_auth_title
            );

            let req = CredentialPrompt {
                purpose: PromptPurpose::SshConfirmation {
                    text: "Verify".to_owned(),
                },
                target: "prod-pg.internal".to_owned(),
                fields: Vec::new(),
                offer_remember: false,
            };
            let text = format_credential_prompt_text(&req, &prompter.strings);
            assert!(text.contains("prod-pg.internal"));
            assert!(!text.contains("{target}"));
        }
    }
}
