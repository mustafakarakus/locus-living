use std::time::Duration;

use homeai_common::{Config, Paths};
use homeai_core::tls;
use tokio_util::sync::CancellationToken;

pub fn write_dev_tree(root: &std::path::Path, api_port: u16, grpc_port: u16) -> Paths {
    let paths = Paths::prefixed(root);
    paths.ensure_runtime_dirs().unwrap();
    tls::write_self_signed(&paths.tls_cert(), &paths.tls_key()).unwrap();
    let cfg = format!(
        "[api]\nhost = \"127.0.0.1\"\nport = {api_port}\n\n[grpc]\nhost = \"127.0.0.1\"\nport = {grpc_port}\n"
    );
    std::fs::write(&paths.config, cfg).unwrap();
    let _ = Config::load(&paths.config).unwrap();
    paths
}

pub fn free_port() -> u16 {
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    l.local_addr().unwrap().port()
}

pub async fn wait_health(url: &str) -> serde_json::Value {
    let client = reqwest::Client::builder()
        .danger_accept_invalid_certs(true)
        .timeout(Duration::from_secs(2))
        .build()
        .unwrap();
    for _ in 0..50 {
        if let Ok(resp) = client.get(url).send().await {
            if resp.status().is_success() {
                return resp.json().await.unwrap();
            }
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    panic!("health endpoint never became ready: {url}");
}

pub fn spawn_core(
    paths: Paths,
) -> (
    CancellationToken,
    tokio::task::JoinHandle<Result<(), homeai_core::Error>>,
) {
    let shutdown = CancellationToken::new();
    let shutdown_c = shutdown.clone();
    let handle = tokio::spawn(async move { homeai_core::run_with(paths, Some(shutdown_c)).await });
    (shutdown, handle)
}
