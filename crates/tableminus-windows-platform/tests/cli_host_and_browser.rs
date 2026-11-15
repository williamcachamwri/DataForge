use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};

use tableminus_windows_platform::{
    AppSettings, McpSettings, SingleInstanceLock, WindowsCliHost, WindowsCredentialStore,
    validate_http_url,
};
use secrecy::SecretString;
use tablepro_cli::{Host, HostError};
use tablepro_core::credentials::{SecretItem, SecretItems};
use tablepro_storage::StoragePaths;
use tempfile::TempDir;
use url::Url;
use uuid::Uuid;

type TestResult = Result<(), Box<dyn std::error::Error>>;

#[tokio::test]
async fn cli_host_reads_settings_runtime_files_and_credential_token() -> TestResult {
    let dir = TempDir::new()?;
    let paths = StoragePaths::under(dir.path(), "tableminus", "app.tableminus.TableMinus");
    std::fs::create_dir_all(&paths.config)?;
    std::fs::create_dir_all(&paths.runtime)?;

    let prefix = format!("TableMinusTest-{}", Uuid::new_v4());
    let mutex_name = format!(r"Local\TableMinus-HostTest-{}", Uuid::new_v4());
    let host = WindowsCliHost::new(paths.clone())
        .with_app_version("1.2.3-test")
        .with_mutex_name(&mutex_name)
        .with_credential_prefix(&prefix);

    assert_eq!(host.app_version(), "1.2.3-test");
    assert!(!host.app_running());
    assert!(!host.server_enabled()?);

    let settings = AppSettings {
        mcp: McpSettings {
            enabled: true,
            port: 47_111,
        },
        ..AppSettings::default()
    };
    settings.save_atomic(&AppSettings::file_path(&paths))?;
    assert!(host.server_enabled()?);
    assert_eq!(host.server_port()?, 47_111);

    // Runtime port file overrides settings port when present.
    std::fs::write(host.port_file_path(), "48222\n")?;
    assert_eq!(host.server_port()?, 48_222);

    // Token falls back to WindowsCredentialStore when runtime token file is absent.
    let store = WindowsCredentialStore::new(&prefix);
    store
        .store_item(SecretItem::McpToken, &SecretString::from("cred-token-abc"))
        .await?;
    let loaded_from_cred = host.token()?;
    store.delete_item(SecretItem::McpToken).await?;
    assert_eq!(loaded_from_cred.as_deref(), Some("cred-token-abc"));

    // Runtime token file takes precedence when present.
    std::fs::write(host.token_file_path(), "runtime-token-xyz\n")?;
    assert_eq!(host.token()?.as_deref(), Some("runtime-token-xyz"));

    // app_running reflects active SingleInstanceLock.
    let lock = SingleInstanceLock::try_acquire(&mutex_name)?;
    assert!(lock.is_some());
    assert!(host.app_running());
    drop(lock);
    assert!(!host.app_running());

    let _terminal = host.terminal();
    Ok(())
}

#[test]
fn cli_host_verify_peer_accepts_own_loopback_connection_and_rejects_foreign() -> TestResult {
    let dir = TempDir::new()?;
    let paths = StoragePaths::under(dir.path(), "tableminus", "app.tableminus.TableMinus");
    let host = WindowsCliHost::new(paths);

    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))?;
    let local = listener.local_addr()?;
    let client = TcpStream::connect(local)?;
    let (_server, peer) = listener.accept()?;

    host.verify_peer(peer, local)?;
    drop(client);

    // A non-existent socket pair is not owned by this process.
    let bogus_peer = SocketAddr::from((Ipv4Addr::LOCALHOST, 65_001));
    let bogus_local = SocketAddr::from((Ipv4Addr::LOCALHOST, 65_002));
    let err = host.verify_peer(bogus_peer, bogus_local);
    assert!(matches!(
        err,
        Err(HostError::ForeignServer { port: 65_002 })
    ));
    Ok(())
}

#[test]
fn validate_http_url_allows_http_and_https_only() -> TestResult {
    validate_http_url(&Url::parse("https://tableminus.example/oauth/callback")?)?;
    validate_http_url(&Url::parse("http://127.0.0.1:45871/auth")?)?;

    assert!(validate_http_url(&Url::parse("file:///C:/Windows/System32/calc.exe")?).is_err());
    assert!(validate_http_url(&Url::parse("javascript:alert(1)")?).is_err());
    assert!(validate_http_url(&Url::parse("ftp://example.com/file")?).is_err());
    Ok(())
}
