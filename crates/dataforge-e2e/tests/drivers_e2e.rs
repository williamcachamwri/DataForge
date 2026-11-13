//! Driver & Session harness E2E tests: verifies all 9 `tablepro-driver-*`
//! crates instantiate, register in `DriverRegistry`, expose accurate
//! `DriverCapabilities` / `EngineCategory`, and integrate with `tablepro-session`.

use std::sync::Arc;

use dataforge_e2e::{E2eResult, open_file_session, shutdown_session};
use tablepro_core::{
    ConnectEnvironment, DataModel, DatabaseDriver, DriverRegistry, EndpointKind, EngineCategory, LivenessPolicy,
    NetworkEndpoint, SafeModeLevel, TlsConfig, WorkspaceSettings,
};
use tablepro_core::credentials::{CredentialInteraction, Credentials};
use tablepro_driver_cassandra::CassandraDriver;
use tablepro_driver_clickhouse::ClickhouseDriver;
use tablepro_driver_duckdb::DuckDbDriver;
use tablepro_driver_mongodb::MongoDriver;
use tablepro_driver_mssql::MssqlDriver;
use tablepro_driver_mysql::MysqlDriver;
use tablepro_driver_postgres::PgDriver;
use tablepro_driver_redis::RedisDriver;
use tablepro_driver_sqlite::SqliteDriver;
use tablepro_session::{
    ConnectRequest, ConnectServices, ConnectionMetadata, Connector, DefaultDeadlines, DriverConnector, RequestTarget,
    SessionKey, SessionRegistry,
};
use tablepro_ssh::InProcessSsh;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

fn all_drivers() -> Vec<Arc<dyn DatabaseDriver>> {
    vec![
        Arc::new(PgDriver),
        Arc::new(MysqlDriver),
        Arc::new(SqliteDriver),
        Arc::new(DuckDbDriver),
        Arc::new(MssqlDriver),
        Arc::new(ClickhouseDriver),
        Arc::new(CassandraDriver),
        Arc::new(MongoDriver),
        Arc::new(RedisDriver),
    ]
}

#[test]
fn all_nine_drivers_register_and_report_capabilities() {
    let mut registry = DriverRegistry::new();
    for driver in all_drivers() {
        registry.register(driver);
    }
    assert_eq!(registry.len(), 9);

    let expected = [
        ("postgres", EngineCategory::Relational, DataModel::Relational, false),
        ("mysql", EngineCategory::Relational, DataModel::Relational, false),
        ("sqlite", EngineCategory::Relational, DataModel::Relational, true),
        ("duckdb", EngineCategory::Analytical, DataModel::Relational, true),
        ("mssql", EngineCategory::Relational, DataModel::Relational, false),
        ("clickhouse", EngineCategory::Analytical, DataModel::Relational, false),
        ("cassandra", EngineCategory::WideColumn, DataModel::Relational, false),
        ("mongodb", EngineCategory::Document, DataModel::Document, false),
        ("redis", EngineCategory::KeyValue, DataModel::KeyValue, false),
    ];

    for (id, cat, model, is_file) in expected {
        let driver = registry.get(id);
        assert!(driver.is_some(), "missing driver {id}");
        if let Some(drv) = driver {
            assert_eq!(drv.id(), id);
            assert!(!drv.display_name().is_empty());
            assert_eq!(drv.category(), cat);
            let caps = drv.capabilities();
            assert_eq!(caps.data_model, model);
            assert_eq!(matches!(caps.endpoint, EndpointKind::File(_)), is_file);
        }
    }
}

#[tokio::test]
async fn drivers_integrate_with_connector_and_session_registry() -> E2eResult<()> {
    let connector = DriverConnector::new(
        ConnectServices {
            environment: ConnectEnvironment::new("dataforge-e2e"),
            ssh: Arc::new(InProcessSsh::system()),
            liveness: LivenessPolicy::DESKTOP,
        },
        Arc::new(DefaultDeadlines::new(LivenessPolicy::DESKTOP)),
    );

    // Every network driver should reject a cancelled connect cleanly without panicking.
    for driver in all_drivers() {
        let caps = driver.capabilities();
        if matches!(caps.endpoint, EndpointKind::Network { .. }) {
            let endpoint = NetworkEndpoint::new("127.0.0.1", 1)?;
            let request = ConnectRequest {
                metadata: ConnectionMetadata {
                    id: Uuid::new_v4(),
                    name: format!("probe-{}", driver.id()),
                    driver_id: driver.id().to_owned(),
                },
                driver: driver.clone(),
                target: RequestTarget::Network {
                    endpoint,
                    ssh: None,
                    credentials: Credentials::Integrated,
                    database: Some("test".to_owned()),
                    tls: TlsConfig::disabled(),
                    options: Default::default(),
                },
                workspace_settings: WorkspaceSettings::workspace(false),
                safe_mode: SafeModeLevel::Off,
            };
            let cancel = CancellationToken::new();
            cancel.cancel();
            let res = connector
                .establish(request, CredentialInteraction::Unattended, cancel)
                .await;
            assert!(
                res.is_err(),
                "cancelled connect for {} should return Err",
                driver.id()
            );
        }
    }

    // Real SQLite session integrates with SessionRegistry single-window claim.
    let dir = tempfile::tempdir()?;
    let db_path = dir.path().join("registry.sqlite");
    let session = open_file_session(
        dir.path(),
        &db_path,
        Arc::new(SqliteDriver),
        "sqlite",
        true,
        false,
    )
    .await?;

    let registry = SessionRegistry::new();
    let key = SessionKey::Saved(session.id());
    let claim = registry.claim(key.clone(), 1);
    assert!(claim.is_ok(), "first claim should succeed");
    assert!(
        registry.claim(key, 2).is_err(),
        "duplicate claim should be rejected"
    );
    drop(claim);
    shutdown_session(&session).await?;
    Ok(())
}
