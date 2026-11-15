//! Foreign connection import (`tablepro-foreign-import`), connection URL parsing,
//! and connection testing.

use std::path::PathBuf;
use std::sync::Arc;

use tableminus_windows_platform::{ForeignAppDirs, ForeignHost, PlainHost, foreign_app_dirs};
use tablepro_core::connection_url::{ConnectionUrl, parse as parse_url};
use tablepro_core::credentials::CredentialInteraction;
use tablepro_foreign_import::{
    CollectedImport, ForeignApp, ForeignImporter, ImportRequest, Inventory,
};
use tablepro_session::connect::{ConnectRequest, Connector};
use tokio_util::sync::CancellationToken;

use crate::error::ServiceError;
use crate::runtime::AppRuntime;

/// Supported third-party database clients for import.
pub const SUPPORTED_IMPORT_APPS: [ForeignApp; 5] = [
    ForeignApp::TablePlus,
    ForeignApp::DBeaver,
    ForeignApp::DataGrip,
    ForeignApp::Navicat,
    ForeignApp::SequelAce,
];

/// Parses a database connection URL (`postgres://...`, `mysql://...`, `sqlite:...`, etc.).
///
/// # Errors
///
/// Returns [`ServiceError::Import`] if the URL cannot be parsed.
pub fn parse_connection_url(url: &str) -> Result<ConnectionUrl, ServiceError> {
    parse_url(url).map_err(|err| ServiceError::Import(err.to_string()))
}

/// Checks whether `app` has detectable data in the given `dirs`.
#[must_use]
pub fn has_foreign_data(app: ForeignApp, dirs: &ForeignAppDirs) -> bool {
    let host: Arc<dyn ForeignHost> = Arc::new(PlainHost);
    ForeignImporter::new(app, dirs.clone(), host).is_available_blocking()
}

/// Returns the inventory (connection and saved query counts) for `app` in `dirs`.
#[must_use]
pub fn foreign_inventory(app: ForeignApp, dirs: &ForeignAppDirs) -> Inventory {
    let host: Arc<dyn ForeignHost> = Arc::new(PlainHost);
    ForeignImporter::new(app, dirs.clone(), host).inventory_blocking()
}

/// Imports connections and saved queries from `app` using Windows default directories
/// (or an optional user-picked export file such as Navicat `.ncx`).
///
/// # Errors
///
/// Returns [`ServiceError::Import`] if resolving app dirs or collecting the foreign store fails.
pub fn import_from_app(
    app: ForeignApp,
    picked_file: Option<PathBuf>,
    request: &ImportRequest,
    cancel: &CancellationToken,
) -> Result<CollectedImport, ServiceError> {
    let dirs = foreign_app_dirs().map_err(|err| ServiceError::Import(err.to_string()))?;
    import_from_app_with_dirs(app, &dirs, picked_file, request, cancel)
}

/// Imports connections and saved queries from `app` using explicit `dirs`.
///
/// # Errors
///
/// Returns [`ServiceError::Import`] if collecting or parsing the foreign store fails.
pub fn import_from_app_with_dirs(
    app: ForeignApp,
    dirs: &ForeignAppDirs,
    picked_file: Option<PathBuf>,
    request: &ImportRequest,
    cancel: &CancellationToken,
) -> Result<CollectedImport, ServiceError> {
    let host: Arc<dyn ForeignHost> = Arc::new(PlainHost);
    let mut importer = ForeignImporter::new(app, dirs.clone(), host);
    if let Some(path) = picked_file {
        importer = importer.with_file(path);
    }
    importer
        .collect_blocking(request, cancel)
        .map_err(|err| ServiceError::Import(err.to_string()))
}

/// Tests a [`ConnectRequest`] by establishing a connection and immediately discarding it.
///
/// # Errors
///
/// Returns [`ServiceError::Session`] if establishing the connection fails.
pub async fn test_connection(
    runtime: &AppRuntime,
    request: ConnectRequest,
    interaction: CredentialInteraction,
    cancel: CancellationToken,
) -> Result<(), ServiceError> {
    let connector = runtime.connector();
    let pending = connector
        .establish(request, interaction, cancel)
        .await
        .map_err(|err| ServiceError::Session(err.to_string()))?;
    let _ = pending.discard().await;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tableminus_windows_platform::ForeignPlatform;

    #[test]
    fn supported_import_apps_includes_all_five_clients() {
        assert_eq!(SUPPORTED_IMPORT_APPS.len(), 5);
        assert!(SUPPORTED_IMPORT_APPS.contains(&ForeignApp::TablePlus));
        assert!(SUPPORTED_IMPORT_APPS.contains(&ForeignApp::DBeaver));
        assert!(SUPPORTED_IMPORT_APPS.contains(&ForeignApp::DataGrip));
        assert!(SUPPORTED_IMPORT_APPS.contains(&ForeignApp::Navicat));
        assert!(SUPPORTED_IMPORT_APPS.contains(&ForeignApp::SequelAce));
    }

    #[test]
    fn parse_connection_url_parses_postgres_and_rejects_invalid() -> Result<(), ServiceError> {
        let parsed = parse_connection_url("postgres://alice:secret@db.example.com:5432/appdb")?;
        assert_eq!(parsed.driver_id, "postgres");
        assert_eq!(parsed.host, "db.example.com");
        assert_eq!(parsed.port, Some(5432));
        assert_eq!(parsed.username, "alice");
        assert_eq!(parsed.database, "appdb");

        let invalid = parse_connection_url("not-a-valid-url");
        assert!(invalid.is_err());
        Ok(())
    }

    #[test]
    fn has_foreign_data_returns_false_for_empty_dirs() -> Result<(), Box<dyn std::error::Error>> {
        let dir = tempfile::tempdir()?;
        let dirs = ForeignAppDirs {
            platform: ForeignPlatform::Windows,
            config: dir.path().to_path_buf(),
            data: dir.path().to_path_buf(),
            home: dir.path().to_path_buf(),
        };
        assert!(!has_foreign_data(ForeignApp::DBeaver, &dirs));
        let inv = foreign_inventory(ForeignApp::DBeaver, &dirs);
        assert_eq!(inv.connections, 0);
        Ok(())
    }
}
