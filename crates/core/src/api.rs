//! Local HTTPS API on :8443 and gRPC on :50051 (UC-101 bind; UC-106/107 own the full contracts).

use std::net::SocketAddr;

use axum::extract::State;
use axum::routing::get;
use axum::Json;
use axum::Router;
use axum_server::tls_rustls::RustlsConfig;
use homeai_common::{Config, Paths};
use tonic_health::server::HealthReporter;
use tracing::info;

use crate::health::HealthState;
use crate::Error;

#[derive(Clone)]
struct ApiState {
    health: HealthState,
}

pub async fn serve(config: Config, paths: Paths, health: HealthState) -> Result<(), Error> {
    let addr = config.api_addr()?;
    let tls = RustlsConfig::from_pem_file(paths.tls_cert(), paths.tls_key())
        .await
        .map_err(|e| Error::Other(format!("api tls: {e}")))?;

    let app = Router::new()
        .route("/api/v1/health", get(health_handler))
        .with_state(ApiState { health });

    info!(%addr, "api listening (https)");
    axum_server::bind_rustls(addr, tls)
        .serve(app.into_make_service())
        .await
        .map_err(|e| Error::Other(format!("api serve: {e}")))?;
    Ok(())
}

async fn health_handler(State(state): State<ApiState>) -> Json<crate::health::HealthSnapshot> {
    Json(state.health.snapshot())
}

pub async fn serve_grpc(config: Config, paths: Paths) -> Result<(), Error> {
    let addr = config.grpc_addr()?;
    let cert = std::fs::read(paths.tls_cert())?;
    let key = std::fs::read(paths.tls_key())?;
    let identity = tonic::transport::Identity::from_pem(cert, key);
    let tls = tonic::transport::ServerTlsConfig::new().identity(identity);

    let (mut reporter, health_service): (HealthReporter, _) =
        tonic_health::server::health_reporter();
    reporter.set_serving::<HealthSvcMarker>().await;

    info!(%addr, "grpc listening (tls)");
    tonic::transport::Server::builder()
        .tls_config(tls)
        .map_err(|e| Error::Other(format!("grpc tls: {e}")))?
        .add_service(health_service)
        .serve(addr)
        .await
        .map_err(|e| Error::Other(format!("grpc serve: {e}")))?;
    Ok(())
}

/// Marker type so tonic-health has a named service. NodeService (UC-107) replaces this.
enum HealthSvcMarker {}

impl tonic::server::NamedService for HealthSvcMarker {
    const NAME: &'static str = "homeai.core.Health";
}

/// Bind a localhost TCP port for tests without racing `0`.
pub fn ephemeral_localhost() -> std::io::Result<SocketAddr> {
    let listener = std::net::TcpListener::bind("127.0.0.1:0")?;
    listener.local_addr()
}
