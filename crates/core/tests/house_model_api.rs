//! Seed a two-floor house from house.toml and query/add devices over the API.

mod common;

use std::time::Duration;

use homeai_common::{Scope, TokenStore};

fn two_floor_house() -> &'static str {
    r#"
[property]
id = "home"
name = "Villa"

[[floor]]
id = "floor-1"
name = "Ground"

[[floor]]
id = "floor-2"
name = "First"

[[room]]
id = "room-kitchen"
floor_id = "floor-1"
name = "Kitchen"
kind = "indoor"

[[room]]
id = "room-bedroom"
floor_id = "floor-2"
name = "Bedroom"
kind = "indoor"
"#
}

#[tokio::test]
async fn seeds_two_floors_and_attaches_a_device() {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let root = tempfile::tempdir().unwrap();
    let api_port = common::free_port();
    let grpc_port = common::free_port();
    let paths = common::write_dev_tree(root.path(), api_port, grpc_port);
    std::fs::write(&paths.house, two_floor_house()).unwrap();

    let mut store = TokenStore::load(paths.tokens_dir()).unwrap();
    let reader = store.create("reader", vec![Scope::Read]).unwrap();
    let writer = store
        .create("writer", vec![Scope::Read, Scope::Control])
        .unwrap();

    let (shutdown, server) = common::spawn_core(paths);
    common::wait_health(&format!("https://127.0.0.1:{api_port}/api/v1/health")).await;

    let client = reqwest::Client::builder()
        .danger_accept_invalid_certs(true)
        .build()
        .unwrap();
    let auth_read = format!("Bearer {}", reader.secret);
    let auth_write = format!("Bearer {}", writer.secret);
    let base = format!("https://127.0.0.1:{api_port}");

    let house: serde_json::Value = client
        .get(format!("{base}/api/v1/house"))
        .header("Authorization", &auth_read)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(house["property"]["name"], "Villa");
    assert_eq!(house["floors"].as_array().unwrap().len(), 2);
    assert_eq!(house["rooms"].as_array().unwrap().len(), 2);

    let rooms: serde_json::Value = client
        .get(format!("{base}/api/v1/rooms"))
        .header("Authorization", &auth_read)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(rooms
        .as_array()
        .unwrap()
        .iter()
        .any(|r| r["id"] == "room-kitchen"));

    let created = client
        .post(format!("{base}/api/v1/devices"))
        .header("Authorization", &auth_write)
        .json(&serde_json::json!({
            "id": "light-kitchen",
            "room_id": "room-kitchen",
            "name": "Kitchen light",
            "kind": "light",
            "protocol": "matter"
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(created.status(), 201);

    let detail: serde_json::Value = client
        .get(format!("{base}/api/v1/rooms/room-kitchen"))
        .header("Authorization", &auth_read)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(detail["name"], "Kitchen");
    assert_eq!(detail["floor_id"], "floor-1");
    assert_eq!(detail["kind"], "indoor");
    assert_eq!(detail["devices"][0]["id"], "light-kitchen");
    assert_eq!(detail["devices"][0]["room_id"], "room-kitchen");

    shutdown.cancel();
    let _ = tokio::time::timeout(Duration::from_secs(5), server).await;
}
