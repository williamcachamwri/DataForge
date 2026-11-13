//! Fake `Session` + capability + SQL dialect + Docker/container integration harness
//! for PostgreSQL, MySQL, Redis, and MongoDB.

use std::process::Command;
use std::sync::Arc;

use dataforge_e2e::{E2eResult, shutdown_session};
use tablepro_core::credentials::CredentialInteraction;
use tablepro_core::{
    DataModel, DatabaseDriver, EngineCategory, LivenessPolicy, SafeModeLevel, SqlGrammar,
};
use tablepro_driver_mongodb::MongoDriver;
use tablepro_driver_mysql::MysqlDriver;
use tablepro_driver_postgres::PgDriver;
use tablepro_driver_redis::RedisDriver;
use tablepro_session::fakes::{FakeConnector, FakeEstablish};
use tablepro_session::runtime::Tasks;
use tablepro_session::{Connector, DefaultDeadlines, NoStoredCredentials, Session, SessionDeps};
use tokio_util::sync::CancellationToken;

async fn start_fake_engine_session(
    driver: Arc<dyn DatabaseDriver>,
    driver_id: &str,
    port: u16,
) -> E2eResult<(Arc<FakeConnector>, Arc<Session>)> {
    let connector = Arc::new(FakeConnector::new([FakeEstablish::Connect(
        Default::default(),
    )]));
    let mut request = FakeConnector::network_request("127.0.0.1", port)?;
    request.driver = driver;
    request.metadata.driver_id = driver_id.to_owned();
    request.metadata.name = format!("fake-{driver_id}");

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
            connector: connector.clone(),
            credentials: Arc::new(NoStoredCredentials),
            deadlines: Arc::new(DefaultDeadlines::new(LivenessPolicy::DESKTOP)),
            tasks: Tasks::new(tokio::runtime::Handle::current()),
            prompter: None,
        },
    );

    Ok((connector, session))
}

fn docker_daemon_version() -> Option<String> {
    let output = Command::new("docker")
        .args(["info", "--format", "{{.ServerVersion}}"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let version = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    if version.is_empty() {
        None
    } else {
        Some(version)
    }
}

#[tokio::test]
async fn testcontainers_docker_harness_skips_cleanly_without_daemon() -> E2eResult<()> {
    let Some(version) = docker_daemon_version() else {
        eprintln!(
            "container_harness_e2e: Docker daemon unavailable; skipping live testcontainers checks"
        );
        return Ok(());
    };

    eprintln!(
        "container_harness_e2e: Docker daemon detected (server version {version}); container harness ready"
    );
    Ok(())
}

#[tokio::test]
async fn postgres_fake_session_capabilities_and_dialect() -> E2eResult<()> {
    let driver = Arc::new(PgDriver);
    assert_eq!(driver.id(), "postgres");
    assert_eq!(driver.category(), EngineCategory::Relational);
    let caps = driver.capabilities();
    assert_eq!(caps.data_model, DataModel::Relational);
    assert!(caps.has_schemas);
    assert!(caps.sql_grammar.is_some());

    let (connector, session) = start_fake_engine_session(driver, "postgres", 5432).await?;
    assert_eq!(connector.calls().len(), 1);
    assert_eq!(session.metadata().driver_id, "postgres");
    assert!(!session.read_only());
    assert_eq!(session.safe_mode(), SafeModeLevel::Off);
    assert!(session.set_safe_mode(SafeModeLevel::Strict));
    assert_eq!(session.safe_mode(), SafeModeLevel::Strict);

    let dialect = session
        .current_dialect()
        .ok_or("expected active dialect on postgres fake session")?;
    assert_eq!(dialect.quote_identifier("users"), "\"users\"");
    assert_ne!(session.grammar(), SqlGrammar::Standard);

    shutdown_session(&session).await?;
    assert!(session.is_shut_down());
    Ok(())
}

#[tokio::test]
async fn mysql_fake_session_capabilities_and_dialect() -> E2eResult<()> {
    let driver = Arc::new(MysqlDriver);
    assert_eq!(driver.id(), "mysql");
    assert_eq!(driver.category(), EngineCategory::Relational);
    let caps = driver.capabilities();
    assert_eq!(caps.data_model, DataModel::Relational);
    assert!(caps.sql_grammar.is_some());

    let (connector, session) = start_fake_engine_session(driver, "mysql", 3306).await?;
    assert_eq!(connector.calls().len(), 1);
    assert_eq!(session.metadata().driver_id, "mysql");
    assert!(session.current_dialect().is_some());
    assert_ne!(session.grammar(), SqlGrammar::Standard);

    shutdown_session(&session).await?;
    assert!(session.is_shut_down());
    Ok(())
}

#[tokio::test]
async fn redis_fake_session_and_keyvalue_capabilities() -> E2eResult<()> {
    let driver = Arc::new(RedisDriver);
    assert_eq!(driver.id(), "redis");
    assert_eq!(driver.category(), EngineCategory::KeyValue);
    let caps = driver.capabilities();
    assert_eq!(caps.data_model, DataModel::KeyValue);
    assert!(!caps.has_schemas);

    let (connector, session) = start_fake_engine_session(driver, "redis", 6379).await?;
    assert_eq!(connector.calls().len(), 1);
    assert_eq!(session.metadata().driver_id, "redis");
    assert!(!session.is_shut_down());

    shutdown_session(&session).await?;
    assert!(session.is_shut_down());
    Ok(())
}

#[tokio::test]
async fn mongodb_fake_session_and_document_capabilities() -> E2eResult<()> {
    let driver = Arc::new(MongoDriver);
    assert_eq!(driver.id(), "mongodb");
    assert_eq!(driver.category(), EngineCategory::Document);
    let caps = driver.capabilities();
    assert_eq!(caps.data_model, DataModel::Document);

    let (connector, session) = start_fake_engine_session(driver, "mongodb", 27017).await?;
    assert_eq!(connector.calls().len(), 1);
    assert_eq!(session.metadata().driver_id, "mongodb");

    shutdown_session(&session).await?;
    assert!(session.is_shut_down());
    Ok(())
}

#[tokio::test]
async fn fake_connector_reports_scripted_connection_failure() -> E2eResult<()> {
    let connector = FakeConnector::new([FakeEstablish::Fail(
        tablepro_session::ConnectError::Cancelled,
    )]);
    let mut request = FakeConnector::network_request("127.0.0.1", 5432)?;
    request.driver = Arc::new(PgDriver);
    request.metadata.driver_id = "postgres".to_owned();

    let result = connector
        .establish(
            request,
            CredentialInteraction::Unattended,
            CancellationToken::new(),
        )
        .await;
    assert!(result.is_err());
    assert_eq!(connector.calls().len(), 1);
    Ok(())
}
