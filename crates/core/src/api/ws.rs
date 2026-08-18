//! `WS /ws/events` — live `HomeEvent` JSON after bearer upgrade (UC-106).

use std::net::SocketAddr;

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{ConnectInfo, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use homeai_common::Scope;
use homeai_proto::HomeEvent;
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast::error::RecvError;
use tracing::debug;

use super::auth::{authorize, bearer_secret};
use super::ApiState;

#[derive(Default, Deserialize)]
pub struct WsQuery {
    access_token: Option<String>,
}

pub async fn events_handler(
    ws: WebSocketUpgrade,
    State(state): State<ApiState>,
    headers: HeaderMap,
    Query(query): Query<WsQuery>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let header_secret = bearer_secret(&headers, None);
    let secret = header_secret.or(query.access_token);
    authorize(
        &state.paths,
        &state.limiter,
        addr.ip(),
        secret.as_deref(),
        Scope::Read,
    )?;
    let bus = state.bus.clone();
    Ok(ws.on_upgrade(move |socket| stream_events(socket, bus)))
}

async fn stream_events(mut socket: WebSocket, bus: crate::bus::Bus) {
    let mut rx = bus.subscribe();
    loop {
        tokio::select! {
            incoming = socket.recv() => {
                match incoming {
                    None | Some(Err(_)) => break,
                    Some(Ok(Message::Close(_))) => break,
                    Some(Ok(_)) => {}
                }
            }
            ev = rx.recv() => {
                match ev {
                    Ok(event) => {
                        let body = match serde_json::to_string(&EventJson::from(&event)) {
                            Ok(s) => s,
                            Err(err) => {
                                debug!(error = %err, "ws event json");
                                continue;
                            }
                        };
                        if socket.send(Message::Text(body.into())).await.is_err() {
                            break;
                        }
                    }
                    Err(RecvError::Lagged(n)) => {
                        debug!(skipped = n, "ws event lagged");
                    }
                    Err(RecvError::Closed) => break,
                }
            }
        }
    }
}

#[derive(Serialize)]
struct EventJson {
    event_id: String,
    event_type: String,
    source_id: String,
    room_id: String,
    person_id: String,
    timestamp_ms: i64,
    confidence: f64,
    payload: serde_json::Value,
    schema_version: u32,
}

impl From<&HomeEvent> for EventJson {
    fn from(event: &HomeEvent) -> Self {
        let payload = if event.payload.is_empty() {
            serde_json::Value::Null
        } else {
            serde_json::from_slice(&event.payload).unwrap_or_else(|_| {
                serde_json::Value::String(
                    event.payload.iter().map(|b| format!("{b:02x}")).collect(),
                )
            })
        };
        Self {
            event_id: event.event_id.clone(),
            event_type: event.event_type.clone(),
            source_id: event.source_id.clone(),
            room_id: event.room_id.clone(),
            person_id: event.person_id.clone(),
            timestamp_ms: event.timestamp_ms,
            confidence: event.confidence,
            payload,
            schema_version: event.schema_version,
        }
    }
}
