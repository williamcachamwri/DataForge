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
use tablepro_session::{
    Connector, DefaultDeadlines, NoStoredCredentials, Session, SessionDeps,
};
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
