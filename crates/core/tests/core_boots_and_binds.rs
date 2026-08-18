//! Core starts, binds HTTPS + gRPC, writes JSON logs.

mod common;

use std::time::Duration;

#[tokio::test]
async fn boots_binds_tls_ports_and_writes_json_log() {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let root = tempfile::tempdir().unwrap();
    let api_port = common::free_port();
    let grpc_port = common::free_port();
    let paths = common::write_dev_tree(root.path(), api_port, grpc_port);
    let (shutdown, server) = common::spawn_core(paths.clone());

    let url = format!("https://127.0.0.1:{api_port}/api/v1/health");
    let body = common::wait_health(&url).await;
    assert_eq!(body["status"], "ok");
    assert!(body["tasks"]["api"]["status"].is_string());
    assert!(body["tasks"]["grpc"]["status"].is_string());

    let grpc_ok = tokio::net::TcpStream::connect(("127.0.0.1", grpc_port))
        .await
        .is_ok();
    assert!(grpc_ok, "gRPC port {grpc_port} not bound");

    let log = std::fs::read_to_string(paths.core_log()).unwrap();
    assert!(
        log.contains("homeai-core starting") || log.contains("\"message\""),
        "core.log should be JSON: {log}"
    );
    assert!(log.trim_start().starts_with('{'), "expected JSON log line");

    shutdown.cancel();
    let _ = tokio::time::timeout(Duration::from_secs(5), server).await;
}
