//! Config ports and URLs, fail-fast TOML, scoped bearer tokens.

mod common;

use std::time::Duration;

use homeai_common::{Paths, Scope, TokenStore};
use homeai_core::tls;

#[tokio::test]
async fn changed_port_is_the_bound_port_and_urls_come_from_config() {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let root = tempfile::tempdir().unwrap();
    let api_port = common::free_port();
    let grpc_port = common::free_port();
    let paths = common::write_dev_tree(root.path(), api_port, grpc_port);
    let mut store = TokenStore::load(paths.tokens_dir()).unwrap();
    let token = store.create("tester", vec![Scope::Read]).unwrap();

    let (shutdown, server) = common::spawn_core(paths);
    let health = format!("https://127.0.0.1:{api_port}/api/v1/health");
    let _ = common::wait_health(&health).await;

    let client = reqwest::Client::builder()
        .danger_accept_invalid_certs(true)
        .build()
        .unwrap();
    let status = client
        .get(format!("https://127.0.0.1:{api_port}/api/v1/status"))
        .header("Authorization", format!("Bearer {}", token.secret))
        .send()
        .await
        .unwrap();
    assert_eq!(status.status(), 200);
    let body: serde_json::Value = status.json().await.unwrap();
    assert_eq!(body["llm"], "http://127.0.0.1:8200");
    assert_eq!(body["stt"], "http://127.0.0.1:8100");
    assert_eq!(body["wake"], "hey home");
    assert_eq!(body["token_id"], "tester");

    let denied = client
        .get(format!("https://127.0.0.1:{api_port}/api/v1/status"))
        .send()
        .await
        .unwrap();
    assert_eq!(denied.status(), 401);

    shutdown.cancel();
    let _ = tokio::time::timeout(Duration::from_secs(5), server).await;
}

#[tokio::test]
async fn malformed_toml_fails_startup_with_message() {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let root = tempfile::tempdir().unwrap();
    let paths = Paths::prefixed(root.path());
    paths.ensure_runtime_dirs().unwrap();
    tls::write_self_signed(&paths.tls_cert(), &paths.tls_key()).unwrap();
    std::fs::write(&paths.config, "api = ???\n").unwrap();

    let err = homeai_core::run_with(paths, None).await.unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("invalid config") || msg.contains("expected"),
        "unclear error: {msg}"
    );
}

#[tokio::test]
async fn revoked_token_is_rejected() {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let root = tempfile::tempdir().unwrap();
    let api_port = common::free_port();
    let grpc_port = common::free_port();
    let paths = common::write_dev_tree(root.path(), api_port, grpc_port);
    let mut store = TokenStore::load(paths.tokens_dir()).unwrap();
    let token = store.create("gone", vec![Scope::Read]).unwrap();
    store.revoke("gone").unwrap();

    let (shutdown, server) = common::spawn_core(paths);
    let _ = common::wait_health(&format!("https://127.0.0.1:{api_port}/api/v1/health")).await;
    let client = reqwest::Client::builder()
        .danger_accept_invalid_certs(true)
        .build()
        .unwrap();
    let status = client
        .get(format!("https://127.0.0.1:{api_port}/api/v1/status"))
        .header("Authorization", format!("Bearer {}", token.secret))
        .send()
        .await
        .unwrap();
    assert_eq!(status.status(), 401);
    shutdown.cancel();
    let _ = tokio::time::timeout(Duration::from_secs(5), server).await;
}
