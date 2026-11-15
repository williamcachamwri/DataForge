use tableminus_windows_platform::{PART_SIZE, WindowsCredentialStore};
use secrecy::{ExposeSecret, SecretString};
use tablepro_account::RefreshTokenVault;
use tablepro_ai::ApiKeyVault;
use tablepro_core::AiProvider;
use tablepro_core::credentials::{SecretItem, SecretItems, SecretKind, SecretVault};
use uuid::Uuid;

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn test_store() -> WindowsCredentialStore {
    WindowsCredentialStore::new(format!("TableMinusTest-{}", Uuid::new_v4()))
}

#[tokio::test]
async fn secret_vault_roundtrip_relabel_and_cleanup() -> TestResult {
    let store = test_store();
    let id = Uuid::new_v4();
    let secret = SecretString::from("p\u{e4}ss-\u{1f511}");

    store
        .store(id, SecretKind::DatabasePassword, &secret, "initial-label")
        .await?;
    let loaded = store.load(id, SecretKind::DatabasePassword).await?;
    assert_eq!(
        loaded.found().map(|s| s.expose_secret().to_owned()),
        Some("p\u{e4}ss-\u{1f511}".to_owned())
    );

    store
        .relabel(id, SecretKind::DatabasePassword, "updated-label")
        .await?;
    let relabeled = store.load(id, SecretKind::DatabasePassword).await?;
    assert_eq!(
        relabeled.found().map(|s| s.expose_secret().to_owned()),
        Some("p\u{e4}ss-\u{1f511}".to_owned())
    );

    let targets = store.enumerate_targets().await?;
    assert_eq!(
        targets,
        vec![store.connection_target(id, SecretKind::DatabasePassword)]
    );

    store.delete_connection(id).await?;
    let gone = store.load(id, SecretKind::DatabasePassword).await?;
    assert!(!gone.is_stored());
    assert!(store.enumerate_targets().await?.is_empty());
    Ok(())
}

#[tokio::test]
async fn secret_vault_handles_empty_and_chunked_secrets_above_2560_bytes() -> TestResult {
    let store = test_store();
    let id = Uuid::new_v4();

    // Empty secret roundtrip.
    store
        .store(
            id,
            SecretKind::SshPassword,
            &SecretString::from(""),
            "empty",
        )
        .await?;
    let empty_loaded = store.load(id, SecretKind::SshPassword).await?;
    assert_eq!(
        empty_loaded.found().map(|s| s.expose_secret().to_owned()),
        Some(String::new())
    );

    // Multi-part secret exceeding PART_SIZE (2,560 bytes) with a surrogate pair straddling the boundary.
    let long_secret = format!(
        "{}\u{1f511}{}",
        "a".repeat(PART_SIZE / 2 - 1),
        "b".repeat(2500)
    );
    store
        .store(
            id,
            SecretKind::DatabasePassword,
            &SecretString::from(long_secret.clone()),
            &"l".repeat(1000),
        )
        .await?;

    let enumerated = store.enumerate_targets().await?;
    assert!(enumerated.len() >= 3);

    let loaded_long = store.load(id, SecretKind::DatabasePassword).await?;
    assert_eq!(
        loaded_long.found().map(|s| s.expose_secret().to_owned()),
        Some(long_secret)
    );

    // Overwriting with a short secret removes leftover continuation parts.
    store
        .store(
            id,
            SecretKind::DatabasePassword,
            &SecretString::from("short"),
            "short-label",
        )
        .await?;
    let loaded_short = store.load(id, SecretKind::DatabasePassword).await?;
    assert_eq!(
        loaded_short.found().map(|s| s.expose_secret().to_owned()),
        Some("short".to_owned())
    );

    store.delete_connection(id).await?;
    assert!(store.enumerate_targets().await?.is_empty());
    Ok(())
}

#[tokio::test]
async fn secret_items_api_key_vault_and_refresh_token_vault_roundtrip() -> TestResult {
    let store = test_store();

    // SecretItems + ApiKeyVault
    store
        .store_item(SecretItem::McpToken, &SecretString::from("mcp-secret-123"))
        .await?;
    store
        .store_key(AiProvider::Anthropic, &SecretString::from("sk-ant-test"))
        .await?;

    assert_eq!(
        store
            .load_item(SecretItem::McpToken)
            .await?
            .map(|s| s.expose_secret().to_owned()),
        Some("mcp-secret-123".to_owned())
    );
    assert_eq!(
        store
            .load_key(AiProvider::Anthropic)
            .await?
            .map(|s| s.expose_secret().to_owned()),
        Some("sk-ant-test".to_owned())
    );

    // RefreshTokenVault
    store
        .store_refresh_token(&SecretString::from("rt-refresh-xyz"))
        .await?;
    assert_eq!(
        store
            .load_refresh_token()
            .await?
            .map(|s| s.expose_secret().to_owned()),
        Some("rt-refresh-xyz".to_owned())
    );

    store.delete_item(SecretItem::McpToken).await?;
    store.delete_key(AiProvider::Anthropic).await?;
    store.delete_refresh_token().await?;

    assert!(store.load_item(SecretItem::McpToken).await?.is_none());
    assert!(store.load_key(AiProvider::Anthropic).await?.is_none());
    assert!(store.load_refresh_token().await?.is_none());
    assert!(store.enumerate_targets().await?.is_empty());
    Ok(())
}

#[tokio::test]
async fn concurrent_credential_writes_all_hold() -> TestResult {
    let base_prefix = format!("TableMinusTest-{}", Uuid::new_v4());
    let id = Uuid::new_v4();

    let results = futures::future::join_all((0..8).map(|index| {
        let store = WindowsCredentialStore::new(format!("{base_prefix}-{index}"));
        async move {
            let expected = format!("secret-{index}");
            store
                .store(
                    id,
                    SecretKind::DatabasePassword,
                    &SecretString::from(expected.clone()),
                    "concurrent",
                )
                .await?;
            let read = store.load(id, SecretKind::DatabasePassword).await?;
            store.delete_connection(id).await?;
            let gone = store.load(id, SecretKind::DatabasePassword).await?;
            Ok::<_, tablepro_core::credentials::SecretError>((
                read.found().map(|s| s.expose_secret() == expected),
                gone.is_stored(),
            ))
        }
    }))
    .await;

    for outcome in results {
        let (matched, still_stored) = outcome?;
        assert_eq!(matched, Some(true));
        assert!(!still_stored);
    }
    Ok(())
}
