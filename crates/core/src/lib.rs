//! `homeai-core` library. The binary is a thin `main` over this crate.
//!
//! Agents live under [`agents`] as modules. They are spawned as supervised
//! tokio tasks (UC-101). They never call each other; they publish/subscribe
//! on the in-process bus.

pub mod agents;
pub mod api;
pub mod bus;
pub mod db;
pub mod health;
pub mod house;
pub mod logging;
pub mod model;
pub mod supervisor;
pub mod tls;

use std::time::Duration;

use homeai_common::{Config, Paths, TokenStore};
use tokio_util::sync::CancellationToken;
use tracing::{error, info};

use crate::bus::Bus;
use crate::db::Db;
use crate::health::HealthState;
use crate::supervisor::Supervisor;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Config(#[from] homeai_common::ConfigError),
    #[error("invalid listen address: {0}")]
    Addr(#[from] std::net::AddrParseError),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Tls(#[from] tls::TlsError),
    #[error(transparent)]
    Db(#[from] db::DbError),
    #[error(transparent)]
    Token(#[from] homeai_common::TokenError),
    #[error(transparent)]
    House(#[from] house::HouseError),
    #[error("{0}")]
    Other(String),
}

/// Boot the Core: config, logs, SQLite, bus, supervisor, API :8443, gRPC :50051.
pub async fn run() -> Result<(), Error> {
    run_with(Paths::from_env(), None).await
}

/// Same as [`run`], with an optional external shutdown signal (tests).
pub async fn run_with(paths: Paths, shutdown: Option<CancellationToken>) -> Result<(), Error> {
    install_crypto_provider();
    paths.ensure_runtime_dirs()?;

    let _log_guard = logging::init(&paths.log_dir)?;
    info!(
        prefix = ?paths.prefix().map(|p| p.display().to_string()),
        config = %paths.config.display(),
        "homeai-core starting"
    );

    let config = Config::load(&paths.config)?;
    let tokens = TokenStore::load(paths.tokens_dir())?;
    info!(
        llm = %config.llm.url,
        stt = %config.stt.url,
        tts = %config.tts.url,
        wake = %config.wake.keyword,
        tokens = tokens.list().len(),
        "config loaded"
    );
    if paths.is_prefixed() {
        tls::ensure_dev_certs(&paths)?;
    }
    tls::require_server_certs(&paths)?;

    let db = Db::open(&paths.db)?;
    info!(db = %paths.db.display(), "sqlite opened (WAL)");
    match crate::house::seed_if_empty(&db, &paths.house) {
        Ok(true) => info!(file = %paths.house.display(), "house model seeded"),
        Ok(false) => {}
        Err(err) => return Err(err.into()),
    }

    let bus = Bus::new(db.clone());
    let health = HealthState::new();
    let shutdown = shutdown.unwrap_or_else(CancellationToken::new);
    let supervisor = Supervisor::new(bus.clone(), health.clone(), shutdown.clone());

    spawn_api(
        &supervisor,
        config.clone(),
        paths.clone(),
        health.clone(),
        db.clone(),
    );
    spawn_grpc(&supervisor, config.clone(), paths.clone());
    spawn_retention(&supervisor, bus.clone());

    info!(
        api = %config.api_addr()?,
        grpc = %config.grpc_addr()?,
        "listeners requested"
    );

    tokio::select! {
        _ = shutdown.cancelled() => {
            info!("shutdown requested");
        }
        _ = tokio::signal::ctrl_c() => {
            info!("SIGINT");
            shutdown.cancel();
        }
    }

    supervisor.shutdown(Duration::from_secs(5)).await;
    db.close();
    info!("homeai-core stopped");
    Ok(())
}

fn spawn_api(supervisor: &Supervisor, config: Config, paths: Paths, health: HealthState, db: Db) {
    supervisor.spawn("api", move |_child| {
        let config = config.clone();
        let paths = paths.clone();
        let health = health.clone();
        let db = db.clone();
        async move {
            api::serve(config, paths, health, db).await.map_err(|err| {
                error!(error = %err, "api task failed");
                anyhow::anyhow!(err)
            })
        }
    });
}

fn spawn_retention(supervisor: &Supervisor, bus: Bus) {
    supervisor.spawn("retention", move |child| {
        let bus = bus.clone();
        async move {
            crate::bus::retention_loop(bus, Duration::from_secs(3600), child).await;
            Ok(())
        }
    });
}

fn spawn_grpc(supervisor: &Supervisor, config: Config, paths: Paths) {
    supervisor.spawn("grpc", move |_child| {
        let config = config.clone();
        let paths = paths.clone();
        async move {
            api::serve_grpc(config, paths).await.map_err(|err| {
                error!(error = %err, "grpc task failed");
                anyhow::anyhow!(err)
            })
        }
    });
}

fn install_crypto_provider() {
    let _ = rustls::crypto::ring::default_provider().install_default();
}
