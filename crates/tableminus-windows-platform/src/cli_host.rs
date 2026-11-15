//! Windows implementation of [`tablepro_cli::Host`].

use std::net::SocketAddr;
use std::path::PathBuf;

use secrecy::ExposeSecret;
use tablepro_cli::{Host, HostError, Terminal};
use tablepro_core::credentials::SecretItem;
use tablepro_net::loopback_peer_is_ours;
use tablepro_storage::StoragePaths;

use crate::secrets::{DEFAULT_CREDENTIAL_PREFIX, WindowsCredentialStore};
use crate::settings::AppSettings;
use crate::single_instance::{DEFAULT_MUTEX_NAME, SingleInstanceLock};

const MCP_PORT_FILE: &str = "mcp.port";
const MCP_TOKEN_FILE: &str = "mcp.token";

/// Windows [`Host`] implementation backed by [`StoragePaths`], [`AppSettings`],
/// [`WindowsCredentialStore`], and [`tablepro_net::loopback_peer_is_ours`].
#[derive(Debug, Clone)]
pub struct WindowsCliHost {
    paths: StoragePaths,
    app_version: String,
    mutex_name: String,
    credential_prefix: String,
}

impl WindowsCliHost {
    /// Creates a [`WindowsCliHost`] using the given [`StoragePaths`] and default TableMinus identifiers.
    pub fn new(paths: StoragePaths) -> Self {
        Self {
            paths,
            app_version: env!("CARGO_PKG_VERSION").to_owned(),
            mutex_name: DEFAULT_MUTEX_NAME.to_owned(),
            credential_prefix: DEFAULT_CREDENTIAL_PREFIX.to_owned(),
        }
    }

    /// Overrides the reported application version string.
    #[must_use]
    pub fn with_app_version(mut self, version: impl Into<String>) -> Self {
        self.app_version = version.into();
        self
    }

    /// Overrides the single-instance mutex name checked by [`Host::app_running`].
    #[must_use]
    pub fn with_mutex_name(mut self, mutex_name: impl Into<String>) -> Self {
        self.mutex_name = mutex_name.into();
        self
    }

    /// Overrides the Windows Credential Manager prefix used to look up the MCP token.
    #[must_use]
    pub fn with_credential_prefix(mut self, prefix: impl Into<String>) -> Self {
        self.credential_prefix = prefix.into();
        self
    }

    /// Returns the runtime file path where the running server records its bound MCP port.
    pub fn port_file_path(&self) -> PathBuf {
        self.paths.runtime.join(MCP_PORT_FILE)
    }

    /// Returns the runtime file path where an ephemeral MCP token may be stored.
    pub fn token_file_path(&self) -> PathBuf {
        self.paths.runtime.join(MCP_TOKEN_FILE)
    }

    fn load_settings(&self) -> Result<AppSettings, HostError> {
        AppSettings::load(&AppSettings::file_path(&self.paths)).map_err(|err| HostError::Settings {
            reason: err.to_string(),
        })
    }
}

impl Host for WindowsCliHost {
    fn app_version(&self) -> String {
        self.app_version.clone()
    }

    fn app_running(&self) -> bool {
        matches!(SingleInstanceLock::try_acquire(&self.mutex_name), Ok(None))
    }

    fn server_enabled(&self) -> Result<bool, HostError> {
        Ok(self.load_settings()?.mcp.enabled)
    }

    fn server_port(&self) -> Result<u16, HostError> {
        match std::fs::read_to_string(self.port_file_path()) {
            Ok(raw) => {
                let trimmed = raw.trim();
                if let Ok(port) = trimmed.parse::<u16>()
                    && port > 0
                {
                    return Ok(port);
                }
            }
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            Err(err) => {
                return Err(HostError::Settings {
                    reason: format!("could not read {}: {err}", self.port_file_path().display()),
                });
            }
        }
        Ok(self.load_settings()?.mcp.port)
    }

    fn verify_peer(&self, peer: SocketAddr, local: SocketAddr) -> Result<(), HostError> {
        match loopback_peer_is_ours(peer, local) {
            Ok(true) => Ok(()),
            Ok(false) => Err(HostError::ForeignServer { port: local.port() }),
            Err(err) => Err(HostError::Unverified {
                port: local.port(),
                reason: err.to_string(),
            }),
        }
    }

    fn token(&self) -> Result<Option<String>, HostError> {
        match std::fs::read_to_string(self.token_file_path()) {
            Ok(raw) => {
                let trimmed = raw.trim();
                if !trimmed.is_empty() {
                    return Ok(Some(trimmed.to_owned()));
                }
            }
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            Err(err) => {
                return Err(HostError::Keyring {
                    reason: format!("could not read {}: {err}", self.token_file_path().display()),
                });
            }
        }

        let store = WindowsCredentialStore::new(&self.credential_prefix);
        let secret =
            store
                .load_item_sync(SecretItem::McpToken)
                .map_err(|err| HostError::Keyring {
                    reason: err.to_string(),
                })?;
        Ok(secret.map(|value| value.expose_secret().to_owned()))
    }

    fn terminal(&self) -> Terminal {
        Terminal::detect()
    }
}
