//! UC-106: techstack §8 contract — scoped auth, lockout, live WS events.

mod common;

use std::sync::Arc;
use std::time::Duration;

use futures_util::StreamExt;
use homeai_common::{Scope, TokenStore};
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::header::AUTHORIZATION;
use tokio_tungstenite::tungstenite::Error as WsError;
use tokio_tungstenite::{connect_async_tls_with_config, Connector};

fn two_floor_house() -> &'static str {
    r#"
[property]
id = "home"
name = "Villa"

[[floor]]
id = "floor-1"
name = "Ground"

[[room]]
id = "room-kitchen"
floor_id = "floor-1"
name = "Kitchen"
kind = "indoor"
"#
}

struct Harness {
    _root: tempfile::TempDir,
    api_port: u16,
    base: String,
    client: reqwest::Client,
    admin: String,
    reader: String,
    shutdown: tokio_util::sync::CancellationToken,
    server: tokio::task::JoinHandle<Result<(), homeai_core::Error>>,
}

impl Harness {
    async fn start() -> Self {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let root = tempfile::tempdir().unwrap();
        let api_port = common::free_port();
        let grpc_port = common::free_port();
        let paths = common::write_dev_tree(root.path(), api_port, grpc_port);
        std::fs::write(&paths.house, two_floor_house()).unwrap();

        let mut store = TokenStore::load(paths.tokens_dir()).unwrap();
        let admin = store.create("admin", vec![Scope::Admin]).unwrap();
        let reader = store.create("reader", vec![Scope::Read]).unwrap();

        let (shutdown, server) = common::spawn_core(paths);
        common::wait_health(&format!("https://127.0.0.1:{api_port}/api/v1/health")).await;

        let client = reqwest::Client::builder()
            .danger_accept_invalid_certs(true)
            .build()
            .unwrap();

        Self {
            _root: root,
            api_port,
            base: format!("https://127.0.0.1:{api_port}"),
            client,
            admin: format!("Bearer {}", admin.secret),
            reader: format!("Bearer {}", reader.secret),
            shutdown,
            server,
        }
    }

    async fn stop(self) {
        self.shutdown.cancel();
        let _ = tokio::time::timeout(Duration::from_secs(5), self.server).await;
    }

    async fn send(
        &self,
        method: reqwest::Method,
        path: &str,
        auth: Option<&str>,
        body: Option<serde_json::Value>,
    ) -> reqwest::Response {
        let mut req = self.client.request(method, format!("{}{path}", self.base));
        if let Some(auth) = auth {
            req = req.header("Authorization", auth);
        }
        if let Some(body) = body {
            req = req.json(&body);
        }
        req.send().await.unwrap()
    }
}

fn contract_gets() -> &'static [&'static str] {
    &[
        "/api/v1/house",
        "/api/v1/rooms",
        "/api/v1/rooms/room-kitchen",
        "/api/v1/devices",
        "/api/v1/presence",
        "/api/v1/status",
    ]
}

#[tokio::test]
async fn each_contract_endpoint_returns_200_with_valid_token() {
    let h = Harness::start().await;

    let health = h
        .send(reqwest::Method::GET, "/api/v1/health", None, None)
        .await;
    assert_eq!(health.status(), 200);

    for path in contract_gets() {
        let resp = h
            .send(reqwest::Method::GET, path, Some(&h.reader), None)
            .await;
        assert_eq!(resp.status(), 200, "GET {path}");
    }

    let created = h
        .send(
            reqwest::Method::POST,
            "/api/v1/devices",
            Some(&h.admin),
            Some(serde_json::json!({
                "id": "light-kitchen",
                "room_id": "room-kitchen",
                "name": "Kitchen light",
                "kind": "light"
            })),
        )
        .await;
    assert_eq!(created.status(), 201);

    let posts = [
        (
            "/api/v1/devices/light-kitchen/command",
            serde_json::json!({ "action": "on" }),
        ),
        (
            "/api/v1/voice/say",
            serde_json::json!({ "room_id": "room-kitchen", "text": "hello" }),
        ),
        (
            "/api/v1/conversations/sess-1/attachments",
            serde_json::json!({}),
        ),
        ("/api/v1/system/diagnostics/export", serde_json::json!({})),
        ("/api/v1/system/ownership-transfer", serde_json::json!({})),
        ("/api/v1/system/factory-reset", serde_json::json!({})),
        ("/api/v1/system/update", serde_json::json!({})),
    ];
    for (path, body) in posts {
        let resp = h
            .send(reqwest::Method::POST, path, Some(&h.admin), Some(body))
            .await;
        assert_eq!(resp.status(), 200, "POST {path}");
    }

    h.stop().await;
}

#[tokio::test]
async fn missing_token_is_401_on_every_protected_route() {
    let h = Harness::start().await;

    let health = h
        .send(reqwest::Method::GET, "/api/v1/health", None, None)
        .await;
    assert_eq!(health.status(), 200, "health stays a liveness probe");

    // Stay under the lockout threshold (5 failures). Every protected
    // route uses the same extractor; these cover read / control / admin.
    let cases = [
        (reqwest::Method::GET, "/api/v1/house", None),
        (reqwest::Method::GET, "/api/v1/status", None),
        (
            reqwest::Method::POST,
            "/api/v1/voice/say",
            Some(serde_json::json!({ "room_id": "room-kitchen", "text": "hello" })),
        ),
        (
            reqwest::Method::POST,
            "/api/v1/system/update",
            Some(serde_json::json!({})),
        ),
    ];
    for (method, path, body) in cases {
        let resp = h.send(method, path, None, body).await;
        assert_eq!(resp.status(), 401, "{path}");
    }

    h.stop().await;
}

#[tokio::test]
async fn repeated_failed_auth_triggers_lockout() {
    let h = Harness::start().await;
    let mut last = None;
    for _ in 0..5 {
        let resp = h
            .send(
                reqwest::Method::GET,
                "/api/v1/status",
                Some("Bearer deadbeef"),
                None,
            )
            .await;
        last = Some(resp.status());
        assert_eq!(resp.status(), 401);
    }
    assert_eq!(last.unwrap(), 401);

    let locked = h
        .send(
            reqwest::Method::GET,
            "/api/v1/status",
            Some("Bearer deadbeef"),
            None,
        )
        .await;
    assert_eq!(locked.status(), 429);

    let still = h
        .send(
            reqwest::Method::GET,
            "/api/v1/status",
            Some(&h.reader),
            None,
        )
        .await;
    assert_eq!(still.status(), 429);

    h.stop().await;
}

#[tokio::test]
async fn websocket_rejects_upgrade_without_token() {
    let h = Harness::start().await;
    let err = connect_ws(h.api_port, None).await.unwrap_err();
    match err {
        WsError::Http(resp) => assert_eq!(resp.status(), 401),
        other => panic!("expected HTTP 401, got {other:?}"),
    }
    h.stop().await;
}

#[tokio::test]
async fn websocket_streams_published_events() {
    let h = Harness::start().await;
    let token = h.reader.trim_start_matches("Bearer ").to_string();
    let (mut ws, _) = connect_ws(h.api_port, Some(&token)).await.unwrap();

    let said = h
        .send(
            reqwest::Method::POST,
            "/api/v1/voice/say",
            Some(&h.admin),
            Some(serde_json::json!({
                "room_id": "room-kitchen",
                "text": "good evening"
            })),
        )
        .await;
    assert_eq!(said.status(), 200);

    let text = tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            match ws.next().await {
                Some(Ok(msg)) if msg.is_text() => {
                    return msg.into_text().unwrap().to_string();
                }
                Some(Ok(_)) => continue,
                other => panic!("ws ended: {other:?}"),
            }
        }
    })
    .await
    .expect("event on /ws/events");

    let ev: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(ev["event_type"], "voice.say");
    assert_eq!(ev["room_id"], "room-kitchen");
    assert_eq!(ev["payload"]["text"], "good evening");

    let _ = ws.close(None).await;
    h.stop().await;
}

async fn connect_ws(
    port: u16,
    token: Option<&str>,
) -> Result<
    (
        tokio_tungstenite::WebSocketStream<
            tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
        >,
        tokio_tungstenite::tungstenite::http::Response<Option<Vec<u8>>>,
    ),
    WsError,
> {
    let mut req = format!("wss://127.0.0.1:{port}/ws/events")
        .into_client_request()
        .unwrap();
    if let Some(token) = token {
        req.headers_mut()
            .insert(AUTHORIZATION, format!("Bearer {token}").parse().unwrap());
    }
    let connector = Connector::Rustls(insecure_tls());
    connect_async_tls_with_config(req, None, false, Some(connector)).await
}

fn insecure_tls() -> Arc<rustls::ClientConfig> {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let config = rustls::ClientConfig::builder()
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(NoVerify))
        .with_no_client_auth();
    Arc::new(config)
}

#[derive(Debug)]
struct NoVerify;

impl rustls::client::danger::ServerCertVerifier for NoVerify {
    fn verify_server_cert(
        &self,
        _end_entity: &rustls::pki_types::CertificateDer<'_>,
        _intermediates: &[rustls::pki_types::CertificateDer<'_>],
        _server_name: &rustls::pki_types::ServerName<'_>,
        _ocsp_response: &[u8],
        _now: rustls::pki_types::UnixTime,
    ) -> Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
        Ok(rustls::client::danger::ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &rustls::pki_types::CertificateDer<'_>,
        _dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
    }

    fn verify_tls13_signature(
        &self,
        _message: &[u8],
        _cert: &rustls::pki_types::CertificateDer<'_>,
        _dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
    }

    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        rustls::crypto::ring::default_provider()
            .signature_verification_algorithms
            .supported_schemes()
    }
}
