//! Tokio application runtime, all-engine driver registry, and in-process SSH wiring.

use std::future::Future;
use std::sync::Arc;

use tablepro_core::{ConnectEnvironment, DriverRegistry, LivenessPolicy};
use tablepro_driver_cassandra::CassandraDriver;
use tablepro_driver_clickhouse::ClickhouseDriver;
use tablepro_driver_duckdb::DuckDbDriver;
use tablepro_driver_mongodb::MongoDriver;
use tablepro_driver_mssql::MssqlDriver;
use tablepro_driver_mysql::MysqlDriver;
use tablepro_driver_postgres::PgDriver;
use tablepro_driver_redis::RedisDriver;
use tablepro_driver_sqlite::SqliteDriver;
use tablepro_session::connect::{ConnectServices, DriverConnector};
use tablepro_session::operation::{DeadlinePolicy, DefaultDeadlines};
use tablepro_session::runtime::{TaskResult, Tasks};
use tablepro_ssh::InProcessSsh;
use tokio::runtime::{Builder, Handle, Runtime};

use crate::error::ServiceError;

/// All nine database engine IDs registered by [`build_driver_registry`].
pub const REGISTERED_ENGINE_IDS: [&str; 9] = [
    "postgres",
    "mysql",
    "sqlite",
    "duckdb",
    "mssql",
    "clickhouse",
    "cassandra",
    "mongodb",
    "redis",
];

/// Builds a [`DriverRegistry`] containing all 9 supported database drivers.
#[must_use]
pub fn build_driver_registry() -> DriverRegistry {
    let mut registry = DriverRegistry::new();
    registry.register(Arc::new(PgDriver));
    registry.register(Arc::new(MysqlDriver));
    registry.register(Arc::new(SqliteDriver));
    registry.register(Arc::new(DuckDbDriver));
    registry.register(Arc::new(MssqlDriver));
    registry.register(Arc::new(ClickhouseDriver));
    registry.register(Arc::new(CassandraDriver));
    registry.register(Arc::new(MongoDriver));
    registry.register(Arc::new(RedisDriver));
    registry
}

/// Central Tokio runtime and shared driver/SSH services for DataForge.
pub struct AppRuntime {
    runtime: Runtime,
    tasks: Tasks,
    drivers: Arc<DriverRegistry>,
    ssh: Arc<InProcessSsh>,
    connect_services: ConnectServices,
    deadlines: Arc<dyn DeadlinePolicy>,
}

impl AppRuntime {
    /// Creates a multi-threaded Tokio runtime and registers all 9 database drivers
    /// along with [`InProcessSsh::system()`].
    ///
    /// # Errors
    ///
    /// Returns [`ServiceError::Runtime`] if the Tokio runtime cannot be built.
    pub fn new(worker_threads: Option<usize>) -> Result<Self, ServiceError> {
        let mut builder = Builder::new_multi_thread();
        builder.thread_name("dataforge-tokio").enable_all();
        if let Some(threads) = worker_threads {
            builder.worker_threads(threads.max(1));
        }
        let runtime = builder
            .build()
            .map_err(|err| ServiceError::Runtime(err.to_string()))?;
        let tasks = Tasks::new(runtime.handle().clone());
        let drivers = Arc::new(build_driver_registry());
        let ssh = Arc::new(InProcessSsh::system());
        let liveness = LivenessPolicy::DESKTOP;
        let connect_services = ConnectServices {
            environment: ConnectEnvironment::new(env!("CARGO_PKG_VERSION")),
            ssh: ssh.clone(),
            liveness,
        };
        let deadlines: Arc<dyn DeadlinePolicy> = Arc::new(DefaultDeadlines::new(liveness));

        Ok(Self {
            runtime,
            tasks,
            drivers,
            ssh,
            connect_services,
            deadlines,
        })
    }

    /// Returns the `Tasks` handle for spawning async and blocking work.
    #[must_use]
    pub fn tasks(&self) -> &Tasks {
        &self.tasks
    }

    /// Returns the underlying Tokio runtime handle.
    #[must_use]
    pub fn handle(&self) -> &Handle {
        self.runtime.handle()
    }

    /// Returns the registry containing all 9 database drivers.
    #[must_use]
    pub fn drivers(&self) -> &Arc<DriverRegistry> {
        &self.drivers
    }

    /// Returns the in-process SSH transport configuration.
    #[must_use]
    pub fn ssh(&self) -> &Arc<InProcessSsh> {
        &self.ssh
    }

    /// Returns the shared [`ConnectServices`] for establishing connections.
    #[must_use]
    pub fn connect_services(&self) -> &ConnectServices {
        &self.connect_services
    }

    /// Returns the default operation deadline policy.
    #[must_use]
    pub fn deadlines(&self) -> &Arc<dyn DeadlinePolicy> {
        &self.deadlines
    }

    /// Constructs a [`DriverConnector`] wired to this runtime's SSH and deadline services.
    #[must_use]
    pub fn connector(&self) -> Arc<DriverConnector> {
        Arc::new(DriverConnector::new(
            self.connect_services.clone(),
            self.deadlines.clone(),
        ))
    }

    /// Runs a future to completion on the application runtime.
    pub fn block_on<F: Future>(&self, future: F) -> F::Output {
        self.runtime.block_on(future)
    }

    /// Spawns an asynchronous task on the application runtime.
    pub fn spawn<F>(&self, future: F) -> TaskResult<F::Output>
    where
        F: Future + Send + 'static,
        F::Output: Send + 'static,
    {
        self.tasks.spawn_task(future)
    }
}
