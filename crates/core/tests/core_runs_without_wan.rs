//! Core answers health with no remote TCP — WAN is not required.

mod common;

use std::process::Command;
use std::time::Duration;

#[tokio::test]
async fn health_works_with_no_remote_tcp() {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let root = tempfile::tempdir().unwrap();
    let api_port = common::free_port();
    let grpc_port = common::free_port();
    let paths = common::write_dev_tree(root.path(), api_port, grpc_port);
    let (shutdown, server) = common::spawn_core(paths);

    let url = format!("https://127.0.0.1:{api_port}/api/v1/health");
    let body = common::wait_health(&url).await;
    assert_eq!(body["status"], "ok", "API must work without WAN");

    let remote = remote_established_tcp(std::process::id());
    assert!(
        remote.is_empty(),
        "boot must not open remote TCP (WAN-independent): {remote:?}"
    );

    shutdown.cancel();
    let _ = tokio::time::timeout(Duration::from_secs(5), server).await;
}

fn remote_established_tcp(pid: u32) -> Vec<String> {
    let out = Command::new("lsof")
        .args(["-nP", "-a", "-p", &pid.to_string(), "-iTCP"])
        .output()
        .expect("lsof");
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter(|line| line.contains("ESTABLISHED"))
        .filter(|line| {
            !line.contains("127.0.0.1") && !line.contains("[::1]") && !line.contains("localhost")
        })
        .map(str::to_string)
        .collect()
}
